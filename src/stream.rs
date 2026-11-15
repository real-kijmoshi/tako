//! Streaming (SSE) conversion between OpenAI and Anthropic formats.

use crate::translate::{finish_openai, n, stop_anthropic};
use crate::util::now_secs;
use serde_json::{json, Value};
use std::collections::HashMap;

pub trait Translate: Send {
    fn event(&mut self, event: &str, data: &str, out: &mut Vec<u8>);
    fn finish(&mut self, out: &mut Vec<u8>);
}

/// Incremental server-sent-events parser.
#[derive(Default)]
pub struct Sse {
    buf: Vec<u8>,
}

impl Sse {
    pub fn push(&mut self, chunk: &[u8], f: &mut dyn FnMut(&str, &str)) {
        self.buf
            .extend(chunk.iter().copied().filter(|&b| b != b'\r'));
        while let Some(pos) = self.buf.windows(2).position(|w| w == b"\n\n") {
            let ev: Vec<u8> = self.buf.drain(..pos + 2).collect();
            dispatch(&ev, f);
        }
    }

    pub fn flush(&mut self, f: &mut dyn FnMut(&str, &str)) {
        if !self.buf.is_empty() {
            let ev = std::mem::take(&mut self.buf);
            dispatch(&ev, f);
        }
    }
}

fn dispatch(raw: &[u8], f: &mut dyn FnMut(&str, &str)) {
    let s = String::from_utf8_lossy(raw);
    let mut event = "";
    let mut data = String::new();
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("event:") {
            event = v.trim();
        } else if let Some(v) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(v.strip_prefix(' ').unwrap_or(v));
        }
    }
    if !data.is_empty() {
        f(event, &data);
    }
}

fn write_data(out: &mut Vec<u8>, v: &Value) {
    out.extend_from_slice(b"data: ");
    let _ = serde_json::to_writer(&mut *out, v);
    out.extend_from_slice(b"\n\n");
}

fn write_event(out: &mut Vec<u8>, v: &Value) {
    out.extend_from_slice(b"event: ");
    out.extend_from_slice(v["type"].as_str().unwrap_or("message").as_bytes());
    out.push(b'\n');
    write_data(out, v);
}

// ---- Anthropic upstream -> OpenAI client ------------------------------

#[derive(Default)]
pub struct AnthToOai {
    id: String,
    model: String,
    created: u64,
    tools: HashMap<u64, usize>, // anthropic block index -> openai tool_call index
    finish: &'static str,
    input: u64,
    output: u64,
    done: bool,
}

impl AnthToOai {
    fn chunk(&self, out: &mut Vec<u8>, delta: Value, finish: Option<&str>, usage: Option<Value>) {
        let mut c = json!({
            "id": self.id, "object": "chat.completion.chunk", "created": self.created, "model": self.model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]
        });
        if let Some(u) = usage {
            c["usage"] = u;
        }
        write_data(out, &c);
    }
}

impl Translate for AnthToOai {
    fn event(&mut self, ev: &str, data: &str, out: &mut Vec<u8>) {
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return;
        };
        let idx = n(&v["index"]);
        match v["type"].as_str().unwrap_or(ev) {
            "message_start" => {
                let m = &v["message"];
                self.id = m["id"].as_str().unwrap_or("tako").to_string();
                self.model = m["model"].as_str().unwrap_or("").to_string();
                self.created = now_secs();
                let u = &m["usage"];
                self.input = n(&u["input_tokens"])
                    + n(&u["cache_read_input_tokens"])
                    + n(&u["cache_creation_input_tokens"]);
                self.chunk(out, json!({"role": "assistant", "content": ""}), None, None);
            }
            "content_block_start" => {
                let b = &v["content_block"];
                if b["type"] == "tool_use" {
                    let ti = self.tools.len();
                    self.tools.insert(idx, ti);
                    let call = json!({"index": ti, "id": b["id"], "type": "function", "function": {"name": b["name"], "arguments": ""}});
                    self.chunk(out, json!({"tool_calls": [call]}), None, None);
                }
            }
            "content_block_delta" => {
                let d = &v["delta"];
                match d["type"].as_str() {
                    Some("text_delta") => {
                        self.chunk(out, json!({"content": d["text"]}), None, None)
                    }
                    Some("thinking_delta") => {
                        self.chunk(out, json!({"reasoning_content": d["thinking"]}), None, None)
                    }
                    Some("input_json_delta") => {
                        if let Some(&ti) = self.tools.get(&idx) {
                            let call =
                                json!({"index": ti, "function": {"arguments": d["partial_json"]}});
                            self.chunk(out, json!({"tool_calls": [call]}), None, None);
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                self.finish = finish_openai(v["delta"]["stop_reason"].as_str());
                self.output = n(&v["usage"]["output_tokens"]);
            }
            "message_stop" => self.finish(out),
            "error" => {
                write_data(out, &json!({"error": v["error"]}));
                self.done = true;
            }
            _ => {}
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.done {
            return;
        }
        self.done = true;
        let finish = if self.finish.is_empty() {
            "stop"
        } else {
            self.finish
        };
        let usage = json!({"prompt_tokens": self.input, "completion_tokens": self.output, "total_tokens": self.input + self.output});
        self.chunk(out, json!({}), Some(finish), Some(usage));
        out.extend_from_slice(b"data: [DONE]\n\n");
    }
}

// ---- OpenAI upstream -> Anthropic client ------------------------------

enum Open {
    Text(usize),
    Tool(usize),
}

pub struct OaiToAnth {
    started: bool,
    open: Option<Open>,
    next: usize,
    tools: HashMap<u64, usize>, // openai tool_call index -> anthropic block index
    stop: &'static str,
    input: u64,
    output: u64,
    model: String,
    done: bool,
}

impl OaiToAnth {
    pub fn new() -> Self {
        OaiToAnth {
            started: false,
            open: None,
            next: 0,
            tools: HashMap::new(),
            stop: "end_turn",
            input: 0,
            output: 0,
            model: String::new(),
            done: false,
        }
    }

    fn start(&mut self, out: &mut Vec<u8>) {
        if self.started {
            return;
        }
        self.started = true;
        let id = format!(
            "msg_tako_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        write_event(
            out,
            &json!({
                "type": "message_start",
                "message": {"id": id, "type": "message", "role": "assistant", "model": self.model, "content": [],
                            "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": self.input, "output_tokens": 0}}
            }),
        );
    }

    fn close(&mut self, out: &mut Vec<u8>) {
        if let Some(Open::Text(i) | Open::Tool(i)) = self.open.take() {
            write_event(out, &json!({"type": "content_block_stop", "index": i}));
        }
    }
}

impl Translate for OaiToAnth {
    fn event(&mut self, _ev: &str, data: &str, out: &mut Vec<u8>) {
        if data.trim() == "[DONE]" {
            return self.finish(out);
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return;
        };
        if v["error"].is_object() {
            let msg = v["error"]["message"].as_str().unwrap_or("upstream error");
            write_event(
                out,
                &json!({"type": "error", "error": {"type": "api_error", "message": msg}}),
            );
            self.done = true;
            return;
        }
        if self.model.is_empty() {
            self.model = v["model"].as_str().unwrap_or("").to_string();
        }
        self.start(out);
        if v["usage"].is_object() {
            self.input = n(&v["usage"]["prompt_tokens"]);
            self.output = n(&v["usage"]["completion_tokens"]);
        }
        let Some(c) = v["choices"].get(0) else { return };
        let d = &c["delta"];

        if let Some(t) = d["content"].as_str().filter(|t| !t.is_empty()) {
            let i = match self.open {
                Some(Open::Text(i)) => i,
                _ => {
                    self.close(out);
                    let i = self.next;
                    self.next += 1;
                    write_event(
                        out,
                        &json!({"type": "content_block_start", "index": i, "content_block": {"type": "text", "text": ""}}),
                    );
                    self.open = Some(Open::Text(i));
                    i
                }
            };
            write_event(
                out,
                &json!({"type": "content_block_delta", "index": i, "delta": {"type": "text_delta", "text": t}}),
            );
        }

        for tc in d["tool_calls"].as_array().into_iter().flatten() {
            let k = n(&tc["index"]);
            let bi = match self.tools.get(&k) {
                Some(&b) => b,
                None => {
                    self.close(out);
                    let b = self.next;
                    self.next += 1;
                    let id = tc["id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .unwrap_or_else(|| format!("toolu_tako_{b}"));
                    write_event(
                        out,
                        &json!({
                            "type": "content_block_start", "index": b,
                            "content_block": {"type": "tool_use", "id": id, "name": tc["function"]["name"].as_str().unwrap_or(""), "input": {}}
                        }),
                    );
                    self.tools.insert(k, b);
                    self.open = Some(Open::Tool(b));
                    b
                }
            };
            if let Some(a) = tc["function"]["arguments"]
                .as_str()
                .filter(|a| !a.is_empty())
            {
                write_event(
                    out,
                    &json!({"type": "content_block_delta", "index": bi, "delta": {"type": "input_json_delta", "partial_json": a}}),
                );
            }
        }

        if let Some(f) = c["finish_reason"].as_str() {
            self.stop = stop_anthropic(Some(f));
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.done {
            return;
        }
        self.done = true;
        self.start(out);
        self.close(out);
        if !self.tools.is_empty() && self.stop == "end_turn" {
            self.stop = "tool_use";
        }
        write_event(
            out,
            &json!({
                "type": "message_delta",
                "delta": {"stop_reason": self.stop, "stop_sequence": null},
                "usage": {"input_tokens": self.input, "output_tokens": self.output}
            }),
        );
        write_event(out, &json!({"type": "message_stop"}));
    }
}
