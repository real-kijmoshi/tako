//! OpenAI Responses API adapter for the Tako Code gateway, so Codex CLI workers can run
//! on free chat-completions routes. Requests are converted to chat completions, sent
//! without streaming, and the answer is converted back (as SSE events when asked).

use serde_json::{json, Map, Value};

/// Tools Codex declares that chat-completion models only see as plain functions.
const SHELL: &str = "local_shell";

fn text_parts(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.as_str().or_else(|| p["text"].as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn message_content(content: &Value) -> Value {
    let Value::Array(parts) = content else {
        return Value::String(text_parts(content));
    };
    if !parts.iter().any(|p| p["type"] == "input_image") {
        return Value::String(text_parts(content));
    }
    Value::Array(
        parts
            .iter()
            .filter_map(|p| match p["type"].as_str() {
                Some("input_image") => p["image_url"]
                    .as_str()
                    .map(|url| json!({"type": "image_url", "image_url": {"url": url}})),
                _ => p["text"]
                    .as_str()
                    .map(|t| json!({"type": "text", "text": t})),
            })
            .collect(),
    )
}

/// Append a tool call to the assistant turn that precedes it, starting one if needed.
fn push_call(messages: &mut Vec<Value>, id: &str, name: &str, arguments: String) {
    let call =
        json!({"id": id, "type": "function", "function": {"name": name, "arguments": arguments}});
    match messages.last_mut() {
        Some(m) if m["role"] == "assistant" && m["tool_calls"].is_array() => {
            m["tool_calls"].as_array_mut().unwrap().push(call);
        }
        Some(m) if m["role"] == "assistant" && m.get("tool_calls").is_none() => {
            m["tool_calls"] = json!([call]);
        }
        _ => messages.push(json!({"role": "assistant", "content": null, "tool_calls": [call]})),
    }
}

/// Convert a Responses API request body to a non-streaming chat-completions body.
pub fn to_chat(req: &Value) -> Result<Value, String> {
    let mut messages = Vec::new();
    if let Some(s) = req["instructions"].as_str().filter(|s| !s.is_empty()) {
        messages.push(json!({"role": "system", "content": s}));
    }
    match &req["input"] {
        Value::String(s) => messages.push(json!({"role": "user", "content": s})),
        Value::Array(items) => {
            for item in items {
                let call_id = item["call_id"].as_str().unwrap_or("call");
                match item["type"].as_str().unwrap_or("message") {
                    "message" => {
                        let role = match item["role"].as_str() {
                            Some("assistant") => "assistant",
                            Some("system" | "developer") => "system",
                            _ => "user",
                        };
                        messages.push(
                            json!({"role": role, "content": message_content(&item["content"])}),
                        );
                    }
                    "function_call" => push_call(
                        &mut messages,
                        call_id,
                        item["name"].as_str().unwrap_or(""),
                        item["arguments"].as_str().unwrap_or("{}").to_string(),
                    ),
                    "custom_tool_call" => push_call(
                        &mut messages,
                        call_id,
                        item["name"].as_str().unwrap_or(""),
                        json!({"input": item["input"]}).to_string(),
                    ),
                    "local_shell_call" => {
                        let a = &item["action"];
                        push_call(
                            &mut messages,
                            call_id,
                            SHELL,
                            json!({"command": a["command"], "workdir": a["working_directory"]})
                                .to_string(),
                        )
                    }
                    "function_call_output"
                    | "custom_tool_call_output"
                    | "local_shell_call_output" => {
                        let output = match &item["output"] {
                            Value::Object(o) => {
                                text_parts(o.get("content").unwrap_or(&Value::Null))
                            }
                            v => text_parts(v),
                        };
                        messages.push(
                            json!({"role": "tool", "tool_call_id": call_id, "content": output}),
                        );
                    }
                    // Reasoning summaries, web searches etc. have no chat equivalent.
                    _ => {}
                }
            }
        }
        _ => return Err("missing \"input\"".into()),
    }
    let tools: Vec<Value> = req["tools"]
        .as_array()
        .map(|tools| tools.iter().filter_map(chat_tool).collect())
        .unwrap_or_default();
    let mut out = Map::new();
    out.insert("model".into(), req["model"].clone());
    out.insert("messages".into(), Value::Array(messages));
    out.insert("stream".into(), Value::Bool(false));
    if !tools.is_empty() {
        out.insert("tools".into(), Value::Array(tools));
        match &req["tool_choice"] {
            Value::String(s) if ["auto", "none", "required"].contains(&s.as_str()) => {
                out.insert("tool_choice".into(), Value::String(s.clone()));
            }
            Value::Object(o) if o.get("name").is_some() => {
                out.insert(
                    "tool_choice".into(),
                    json!({"type": "function", "function": {"name": o["name"]}}),
                );
            }
            _ => {}
        }
        if let Some(b) = req["parallel_tool_calls"].as_bool() {
            out.insert("parallel_tool_calls".into(), Value::Bool(b));
        }
    }
    for (from, to) in [
        ("max_output_tokens", "max_tokens"),
        ("temperature", "temperature"),
        ("top_p", "top_p"),
    ] {
        if req[from].is_number() {
            out.insert(to.into(), req[from].clone());
        }
    }
    Ok(Value::Object(out))
}

fn chat_tool(t: &Value) -> Option<Value> {
    let name = t["name"].as_str();
    let (name, description, parameters) = match t["type"].as_str()? {
        "function" => (
            name?.to_string(),
            t["description"].as_str().unwrap_or("").to_string(),
            t["parameters"].clone(),
        ),
        // Freeform tools (Codex's apply_patch) become a function taking one string.
        "custom" => {
            let mut d = t["description"].as_str().unwrap_or("").to_string();
            if let Some(g) = t["format"]["definition"].as_str() {
                d.push_str("\nThe input must follow this grammar:\n");
                d.push_str(g);
            }
            (
                name?.to_string(),
                d,
                json!({"type": "object", "properties": {"input": {"type": "string", "description": "Raw tool input"}}, "required": ["input"]}),
            )
        }
        "local_shell" => (
            SHELL.to_string(),
            "Run a command in the workspace. Pass the program and its arguments as an array."
                .into(),
            json!({"type": "object", "properties": {
                "command": {"type": "array", "items": {"type": "string"}},
                "workdir": {"type": "string"}}, "required": ["command"]}),
        ),
        _ => return None,
    };
    let parameters = if parameters.is_object() {
        parameters
    } else {
        json!({"type": "object", "properties": {}})
    };
    Some(
        json!({"type": "function", "function": {"name": name, "description": description, "parameters": parameters}}),
    )
}

fn kind_of<'a>(req: &'a Value, name: &str) -> &'a str {
    if name == SHELL {
        return "local_shell";
    }
    req["tools"]
        .as_array()
        .and_then(|ts| ts.iter().find(|t| t["name"] == name))
        .and_then(|t| t["type"].as_str())
        .unwrap_or("function")
}

/// Convert a chat-completions response to a Responses API response object.
pub fn from_chat(chat: &Value, req: &Value, id: &str, created: u64) -> Value {
    let choice = &chat["choices"][0];
    let msg = &choice["message"];
    let mut output = Vec::new();
    let text = text_parts(&msg["content"]);
    if !text.trim().is_empty() {
        output.push(json!({
            "type": "message", "id": format!("msg_{id}"), "status": "completed", "role": "assistant",
            "content": [{"type": "output_text", "text": text, "annotations": []}]
        }));
    }
    for (i, call) in msg["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = call["function"]["name"].as_str().unwrap_or("");
        let arguments = call["function"]["arguments"].as_str().unwrap_or("{}");
        let call_id = call["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("call_{id}_{i}"));
        let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
        output.push(match kind_of(req, name) {
            "custom" => json!({
                "type": "custom_tool_call", "id": format!("ctc_{id}_{i}"), "status": "completed",
                "call_id": call_id, "name": name,
                "input": args["input"].as_str().unwrap_or(arguments),
            }),
            "local_shell" => json!({
                "type": "local_shell_call", "id": format!("lsh_{id}_{i}"), "status": "completed",
                "call_id": call_id,
                "action": {"type": "exec", "command": args["command"], "working_directory": args["workdir"]},
            }),
            _ => json!({
                "type": "function_call", "id": format!("fc_{id}_{i}"), "status": "completed",
                "call_id": call_id, "name": name, "arguments": arguments,
            }),
        });
    }
    let u = &chat["usage"];
    let input = u["prompt_tokens"].as_u64().unwrap_or(0);
    let out_tokens = u["completion_tokens"].as_u64().unwrap_or(0);
    let truncated = choice["finish_reason"] == "length";
    json!({
        "id": format!("resp_{id}"), "object": "response", "created_at": created,
        "status": if truncated { "incomplete" } else { "completed" },
        "incomplete_details": if truncated { json!({"reason": "max_output_tokens"}) } else { Value::Null },
        "model": chat["model"].as_str().unwrap_or("free:coding"),
        "output": output,
        "parallel_tool_calls": req["parallel_tool_calls"].as_bool().unwrap_or(true),
        "tool_choice": req["tool_choice"].clone(),
        "usage": {
            "input_tokens": input, "output_tokens": out_tokens,
            "total_tokens": u["total_tokens"].as_u64().unwrap_or(input + out_tokens),
            "input_tokens_details": {"cached_tokens": u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0)},
            "output_tokens_details": {"reasoning_tokens": 0},
        },
    })
}

pub fn event(out: &mut Vec<u8>, seq: &mut u64, kind: &str, mut data: Value) {
    data["type"] = Value::String(kind.into());
    data["sequence_number"] = json!(*seq);
    *seq += 1;
    out.extend_from_slice(format!("event: {kind}\ndata: {data}\n\n").as_bytes());
}

/// The opening events, sent before the model answers so clients see progress.
pub fn opening(id: &str, created: u64, model: &Value, seq: &mut u64) -> Vec<u8> {
    let r = json!({"id": format!("resp_{id}"), "object": "response", "created_at": created,
        "status": "in_progress", "model": model, "output": []});
    let mut out = Vec::new();
    event(&mut out, seq, "response.created", json!({"response": r}));
    event(
        &mut out,
        seq,
        "response.in_progress",
        json!({"response": r}),
    );
    out
}

/// Replay a finished response as the item and completion events Responses clients expect.
pub fn closing(resp: &Value, seq: &mut u64) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, item) in resp["output"].as_array().into_iter().flatten().enumerate() {
        let mut added = item.clone();
        if item["type"] == "message" {
            added["status"] = json!("in_progress");
            added["content"] = json!([]);
        }
        event(
            &mut out,
            seq,
            "response.output_item.added",
            json!({"output_index": i, "item": added}),
        );
        match item["type"].as_str() {
            Some("message") => {
                let item_id = &item["id"];
                let text = &item["content"][0]["text"];
                let part = json!({"type": "output_text", "text": "", "annotations": []});
                event(
                    &mut out,
                    seq,
                    "response.content_part.added",
                    json!({"item_id": item_id, "output_index": i, "content_index": 0, "part": part}),
                );
                event(
                    &mut out,
                    seq,
                    "response.output_text.delta",
                    json!({"item_id": item_id, "output_index": i, "content_index": 0, "delta": text}),
                );
                event(
                    &mut out,
                    seq,
                    "response.output_text.done",
                    json!({"item_id": item_id, "output_index": i, "content_index": 0, "text": text}),
                );
                event(
                    &mut out,
                    seq,
                    "response.content_part.done",
                    json!({"item_id": item_id, "output_index": i, "content_index": 0, "part": item["content"][0]}),
                );
            }
            Some("function_call") => event(
                &mut out,
                seq,
                "response.function_call_arguments.done",
                json!({"item_id": item["id"], "output_index": i, "arguments": item["arguments"]}),
            ),
            _ => {}
        }
        event(
            &mut out,
            seq,
            "response.output_item.done",
            json!({"output_index": i, "item": item}),
        );
    }
    let kind = if resp["status"] == "incomplete" {
        "response.incomplete"
    } else {
        "response.completed"
    };
    event(&mut out, seq, kind, json!({"response": resp}));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_request() -> Value {
        json!({
            "model": "free:coding", "instructions": "You are Codex.", "stream": true,
            "tools": [
                {"type": "function", "name": "shell", "description": "Run", "parameters": {"type": "object", "properties": {"command": {"type": "array"}}}},
                {"type": "custom", "name": "apply_patch", "description": "Patch", "format": {"type": "grammar", "definition": "start: patch"}},
                {"type": "web_search"}
            ],
            "tool_choice": "auto", "parallel_tool_calls": false,
            "input": [
                {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Sandbox: workspace-write"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Add a file"}]},
                {"type": "reasoning", "summary": []},
                {"type": "function_call", "name": "shell", "arguments": "{\"command\":[\"ls\"]}", "call_id": "c1"},
                {"type": "function_call_output", "call_id": "c1", "output": "a.py"},
                {"type": "custom_tool_call", "name": "apply_patch", "input": "*** Begin Patch", "call_id": "c2"},
                {"type": "custom_tool_call_output", "call_id": "c2", "output": "Done"}
            ]
        })
    }

    #[test]
    fn request_becomes_chat_with_paired_tool_calls() {
        let chat = to_chat(&codex_request()).unwrap();
        let m = chat["messages"].as_array().unwrap();
        assert_eq!(chat["stream"], false);
        assert_eq!(m[0], json!({"role": "system", "content": "You are Codex."}));
        assert_eq!(m[1]["role"], "system");
        assert_eq!(m[2]["content"], "Add a file");
        assert_eq!(m[3]["tool_calls"][0]["function"]["name"], "shell");
        assert_eq!(
            m[4],
            json!({"role": "tool", "tool_call_id": "c1", "content": "a.py"})
        );
        let patch: Value = serde_json::from_str(
            m[5]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(patch["input"], "*** Begin Patch");
        assert_eq!(m[6]["role"], "tool");
        let tools = chat["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            2,
            "hosted tools such as web_search are dropped"
        );
        assert!(tools[1]["function"]["description"]
            .as_str()
            .unwrap()
            .contains("start: patch"));
        assert_eq!(chat["parallel_tool_calls"], false);
    }

    #[test]
    fn answer_maps_back_to_typed_items_and_events() {
        let req = codex_request();
        let chat = json!({"model": "zai/glm", "choices": [{"finish_reason": "tool_calls", "message": {
            "content": "Patching", "tool_calls": [
                {"id": "x1", "function": {"name": "apply_patch", "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"}},
                {"id": "x2", "function": {"name": "shell", "arguments": "{\"command\":[\"pytest\"]}"}}
            ]}}], "usage": {"prompt_tokens": 10, "completion_tokens": 5}});
        let r = from_chat(&chat, &req, "abc", 1);
        let out = r["output"].as_array().unwrap();
        assert_eq!(out[0]["content"][0]["text"], "Patching");
        assert_eq!(out[1]["type"], "custom_tool_call");
        assert_eq!(out[1]["input"], "*** Begin Patch\n*** End Patch");
        assert_eq!(out[2]["type"], "function_call");
        assert_eq!(out[2]["call_id"], "x2");
        assert_eq!(r["usage"]["total_tokens"], 15);
        let mut seq = 0;
        let mut sse = opening("abc", 1, &json!("free:coding"), &mut seq);
        sse.extend(closing(&r, &mut seq));
        let text = String::from_utf8(sse).unwrap();
        assert!(text.starts_with("event: response.created\n"));
        assert_eq!(
            text.matches("event: response.output_item.done\n").count(),
            3
        );
        assert!(text.trim_end().ends_with("}") && text.contains("event: response.completed"));
    }

    #[test]
    fn truncated_answer_is_incomplete() {
        let chat =
            json!({"choices": [{"finish_reason": "length", "message": {"content": "partial"}}]});
        let r = from_chat(&chat, &json!({}), "t", 1);
        assert_eq!(r["status"], "incomplete");
        let mut seq = 0;
        assert!(String::from_utf8(closing(&r, &mut seq))
            .unwrap()
            .contains("response.incomplete"));
    }

    #[test]
    fn plain_string_input_and_missing_input() {
        let chat = to_chat(&json!({"model": "m", "input": "hi", "max_output_tokens": 50})).unwrap();
        assert_eq!(chat["messages"][0]["content"], "hi");
        assert_eq!(chat["max_tokens"], 50);
        assert!(chat.get("tools").is_none());
        assert!(to_chat(&json!({"model": "m"})).is_err());
    }
}
