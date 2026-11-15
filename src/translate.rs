//! Request/response conversion between the OpenAI Chat Completions and
//! Anthropic Messages formats (non-streaming parts).

use crate::util::now_secs;
use serde_json::{json, Map, Value};

/// A string, or the joined `text` fields of an array of blocks/parts.
pub fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

fn copy(src: &Value, dst: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(v) = src.get(from).filter(|v| !v.is_null()) {
        dst.insert(to.into(), v.clone());
    }
}

pub fn finish_openai(stop_reason: Option<&str>) -> &'static str {
    match stop_reason {
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        _ => "stop",
    }
}

pub fn stop_anthropic(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("length") => "max_tokens",
        Some("tool_calls") | Some("function_call") => "tool_use",
        _ => "end_turn",
    }
}

pub fn parse_args(s: Option<&str>) -> Value {
    s.and_then(|s| serde_json::from_str::<Value>(s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

// ---- Anthropic request -> OpenAI request ------------------------------

pub fn req_anthropic_to_openai(req: &Value, model: &str) -> Value {
    let mut msgs: Vec<Value> = Vec::new();
    let sys = text_of(&req["system"]);
    if !sys.is_empty() {
        msgs.push(json!({"role": "system", "content": sys}));
    }

    for m in req["messages"].as_array().into_iter().flatten() {
        let role = m["role"].as_str().unwrap_or("user");
        if let Some(s) = m["content"].as_str() {
            msgs.push(json!({"role": role, "content": s}));
            continue;
        }
        let blocks = m["content"].as_array().map(Vec::as_slice).unwrap_or(&[]);

        if role == "assistant" {
            let mut text = String::new();
            let mut calls = Vec::new();
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => text.push_str(b["text"].as_str().unwrap_or("")),
                    Some("tool_use") => calls.push(json!({
                        "id": b["id"], "type": "function",
                        "function": {"name": b["name"], "arguments": b["input"].to_string()}
                    })),
                    _ => {} // thinking blocks etc. don't translate
                }
            }
            let content = if text.is_empty() && !calls.is_empty() {
                Value::Null
            } else {
                text.into()
            };
            let mut o = json!({"role": "assistant", "content": content});
            if !calls.is_empty() {
                o["tool_calls"] = calls.into();
            }
            msgs.push(o);
            continue;
        }

        // user: tool results become separate "tool" messages, which must come first
        let mut parts = Vec::new();
        let mut has_image = false;
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => parts.push(json!({"type": "text", "text": b["text"]})),
                Some("image") => {
                    let src = &b["source"];
                    let url = if src["type"] == "url" {
                        src["url"].as_str().unwrap_or("").to_string()
                    } else {
                        format!(
                            "data:{};base64,{}",
                            src["media_type"].as_str().unwrap_or("image/png"),
                            src["data"].as_str().unwrap_or("")
                        )
                    };
                    has_image = true;
                    parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
                }
                Some("tool_result") => {
                    let mut c = text_of(&b["content"]);
                    if b["is_error"] == true {
                        c = format!("Error: {c}");
                    }
                    msgs.push(
                        json!({"role": "tool", "tool_call_id": b["tool_use_id"], "content": c}),
                    );
                }
                _ => {}
            }
        }
        if parts.is_empty() {
            continue;
        }
        let content = if has_image {
            Value::Array(parts)
        } else {
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .into()
        };
        msgs.push(json!({"role": "user", "content": content}));
    }

    let mut out = Map::new();
    out.insert("model".into(), model.into());
    out.insert("messages".into(), msgs.into());
    copy(req, &mut out, "max_tokens", "max_tokens");
    copy(req, &mut out, "temperature", "temperature");
    copy(req, &mut out, "top_p", "top_p");
    copy(req, &mut out, "stop_sequences", "stop");

    if let Some(tools) = req["tools"].as_array() {
        let tools: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("input_schema").is_some()) // skip server-side tools
            .map(|t| {
                let mut f = Map::new();
                f.insert("name".into(), t["name"].clone());
                copy(t, &mut f, "description", "description");
                f.insert("parameters".into(), t["input_schema"].clone());
                json!({"type": "function", "function": f})
            })
            .collect();
        if !tools.is_empty() {
            out.insert("tools".into(), tools.into());
            let tc = &req["tool_choice"];
            let choice = match tc["type"].as_str() {
                Some("any") => json!("required"),
                Some("none") => json!("none"),
                Some("tool") => json!({"type": "function", "function": {"name": tc["name"]}}),
                _ => Value::Null,
            };
            if !choice.is_null() {
                out.insert("tool_choice".into(), choice);
            }
        }
    }
    if req["stream"] == true {
        out.insert("stream".into(), true.into());
        out.insert("stream_options".into(), json!({"include_usage": true}));
    }
    Value::Object(out)
}

// ---- OpenAI request -> Anthropic request ------------------------------

fn push(msgs: &mut Vec<(&'static str, Vec<Value>)>, role: &'static str, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    // Anthropic wants alternating roles: merge consecutive same-role messages.
    match msgs.last_mut() {
        Some(last) if last.0 == role => last.1.extend(blocks),
        _ => msgs.push((role, blocks)),
    }
}

pub fn req_openai_to_anthropic(req: &Value, model: &str) -> Value {
    let mut system = Vec::new();
    let mut msgs = Vec::new();

    for m in req["messages"].as_array().into_iter().flatten() {
        let content = &m["content"];
        match m["role"].as_str().unwrap_or("user") {
            "system" | "developer" => system.push(text_of(content)),
            "assistant" => {
                let mut blocks = Vec::new();
                let t = text_of(content);
                if !t.is_empty() {
                    blocks.push(json!({"type": "text", "text": t}));
                }
                for (i, tc) in m["tool_calls"].as_array().into_iter().flatten().enumerate() {
                    let id = tc["id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .unwrap_or_else(|| format!("toolu_{i}"));
                    blocks.push(json!({
                        "type": "tool_use", "id": id, "name": tc["function"]["name"],
                        "input": parse_args(tc["function"]["arguments"].as_str())
                    }));
                }
                push(&mut msgs, "assistant", blocks);
            }
            "tool" => push(
                &mut msgs,
                "user",
                vec![
                    json!({"type": "tool_result", "tool_use_id": m["tool_call_id"], "content": text_of(content)}),
                ],
            ),
            _ => {
                let mut blocks = Vec::new();
                match content {
                    Value::String(s) if !s.is_empty() => {
                        blocks.push(json!({"type": "text", "text": s}))
                    }
                    Value::Array(parts) => {
                        for p in parts {
                            match p["type"].as_str() {
                                Some("text")
                                    if p["text"].as_str().is_some_and(|t| !t.is_empty()) =>
                                {
                                    blocks.push(json!({"type": "text", "text": p["text"]}))
                                }
                                Some("image_url") => blocks.push(image_block(
                                    p["image_url"]["url"].as_str().unwrap_or(""),
                                )),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                push(&mut msgs, "user", blocks);
            }
        }
    }

    let mut out = Map::new();
    out.insert("model".into(), model.into());
    let messages: Vec<Value> = msgs
        .into_iter()
        .map(|(r, c)| json!({"role": r, "content": c}))
        .collect();
    out.insert("messages".into(), messages.into());
    let system: Vec<String> = system.into_iter().filter(|s| !s.is_empty()).collect();
    if !system.is_empty() {
        out.insert("system".into(), system.join("\n\n").into());
    }
    let max = req["max_completion_tokens"]
        .as_u64()
        .or(req["max_tokens"].as_u64())
        .unwrap_or(8192);
    out.insert("max_tokens".into(), max.into());
    if let Some(t) = req["temperature"].as_f64() {
        out.insert("temperature".into(), t.min(1.0).into());
    }
    copy(req, &mut out, "top_p", "top_p");
    match &req["stop"] {
        Value::String(s) => {
            out.insert("stop_sequences".into(), json!([s]));
        }
        Value::Array(a) => {
            out.insert("stop_sequences".into(), a.clone().into());
        }
        _ => {}
    }
    if let Some(tools) = req["tools"].as_array() {
        let tools: Vec<Value> = tools
            .iter()
            .filter(|t| t["type"] == "function")
            .map(|t| {
                let f = &t["function"];
                let mut o = Map::new();
                o.insert("name".into(), f["name"].clone());
                copy(f, &mut o, "description", "description");
                let schema = if f["parameters"].is_object() {
                    f["parameters"].clone()
                } else {
                    json!({"type": "object"})
                };
                o.insert("input_schema".into(), schema);
                Value::Object(o)
            })
            .collect();
        if !tools.is_empty() {
            out.insert("tools".into(), tools.into());
            let choice = match &req["tool_choice"] {
                Value::String(s) if s == "required" => json!({"type": "any"}),
                Value::String(s) if s == "none" => json!({"type": "none"}),
                Value::Object(o) => {
                    json!({"type": "tool", "name": o.get("function").map(|f| f["name"].clone())})
                }
                _ => Value::Null,
            };
            if !choice.is_null() {
                out.insert("tool_choice".into(), choice);
            }
        }
    }
    if req["stream"] == true {
        out.insert("stream".into(), true.into());
    }
    Value::Object(out)
}

fn image_block(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((mt, data)) = rest.split_once(";base64,") {
            return json!({"type": "image", "source": {"type": "base64", "media_type": mt, "data": data}});
        }
    }
    json!({"type": "image", "source": {"type": "url", "url": url}})
}

// ---- responses --------------------------------------------------------

pub fn resp_anthropic_to_openai(r: &Value) -> Value {
    let mut text = String::new();
    let mut calls = Vec::new();
    for b in r["content"].as_array().into_iter().flatten() {
        match b["type"].as_str() {
            Some("text") => text.push_str(b["text"].as_str().unwrap_or("")),
            Some("tool_use") => calls.push(json!({
                "id": b["id"], "type": "function",
                "function": {"name": b["name"], "arguments": b["input"].to_string()}
            })),
            _ => {}
        }
    }
    let content = if text.is_empty() && !calls.is_empty() {
        Value::Null
    } else {
        text.into()
    };
    let mut msg = json!({"role": "assistant", "content": content});
    if !calls.is_empty() {
        msg["tool_calls"] = calls.into();
    }
    let u = &r["usage"];
    let input = n(&u["input_tokens"])
        + n(&u["cache_read_input_tokens"])
        + n(&u["cache_creation_input_tokens"]);
    let output = n(&u["output_tokens"]);
    json!({
        "id": r["id"], "object": "chat.completion", "created": now_secs(), "model": r["model"],
        "choices": [{"index": 0, "message": msg, "finish_reason": finish_openai(r["stop_reason"].as_str())}],
        "usage": {"prompt_tokens": input, "completion_tokens": output, "total_tokens": input + output}
    })
}

pub fn resp_openai_to_anthropic(r: &Value) -> Value {
    let c = &r["choices"][0];
    let m = &c["message"];
    let mut content = Vec::new();
    let t = text_of(&m["content"]);
    if !t.is_empty() {
        content.push(json!({"type": "text", "text": t}));
    }
    let mut has_tools = false;
    for (i, tc) in m["tool_calls"].as_array().into_iter().flatten().enumerate() {
        has_tools = true;
        let id = tc["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("toolu_{i}"));
        content.push(json!({
            "type": "tool_use", "id": id, "name": tc["function"]["name"],
            "input": parse_args(tc["function"]["arguments"].as_str())
        }));
    }
    if content.is_empty() {
        content.push(json!({"type": "text", "text": ""}));
    }
    let mut stop = stop_anthropic(c["finish_reason"].as_str());
    if has_tools && stop == "end_turn" {
        stop = "tool_use"; // some providers say "stop" even when calling tools
    }
    let u = &r["usage"];
    json!({
        "id": format!("msg_{}", r["id"].as_str().unwrap_or("tako")),
        "type": "message", "role": "assistant", "model": r["model"], "content": content,
        "stop_reason": stop, "stop_sequence": null,
        "usage": {"input_tokens": n(&u["prompt_tokens"]), "output_tokens": n(&u["completion_tokens"])}
    })
}
