//! Provider usage, with explicitly marked text estimates when counters are absent.
use crate::stream::Sse;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Default, Serialize)]
pub struct Tokens {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub estimated_requests: u64,
}

impl Tokens {
    pub fn add(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.estimated_requests += other.estimated_requests;
    }
}

pub struct Usage {
    parser: Sse,
    input: Option<u64>,
    output: Option<u64>,
    input_estimate: u64,
    output_bytes: usize,
}

fn estimate(bytes: usize) -> u64 {
    (bytes as u64).div_ceil(4)
}

impl Usage {
    pub fn new(request: &Value) -> Self {
        let bytes: usize = ["messages", "system", "tools", "tool_choice"]
            .iter()
            .filter_map(|k| request.get(k))
            .map(|v| v.to_string().len())
            .sum();
        Self {
            parser: Sse::default(),
            input: None,
            output: None,
            input_estimate: estimate(bytes),
            output_bytes: 0,
        }
    }

    pub fn value(&mut self, v: &Value) {
        let usage = if v["message"]["usage"].is_object() {
            &v["message"]["usage"]
        } else {
            &v["usage"]
        };
        if let Some(n) = usage["prompt_tokens"]
            .as_u64()
            .or(usage["input_tokens"].as_u64())
        {
            self.input = Some(
                n + usage["cache_read_input_tokens"].as_u64().unwrap_or(0)
                    + usage["cache_creation_input_tokens"].as_u64().unwrap_or(0),
            );
        }
        if let Some(n) = usage["completion_tokens"]
            .as_u64()
            .or(usage["output_tokens"].as_u64())
        {
            if v["type"] != "message_start" {
                self.output = Some(n);
            }
        }
        // Count only generated content, never SSE envelopes or usage metadata.
        if let Some(choices) = v["choices"].as_array() {
            for c in choices {
                let m = if c["delta"].is_object() {
                    &c["delta"]
                } else {
                    &c["message"]
                };
                for key in ["content", "reasoning_content", "reasoning"] {
                    self.output_bytes += m[key].as_str().map_or(0, str::len);
                }
                if let Some(calls) = m["tool_calls"].as_array() {
                    for call in calls {
                        for key in ["name", "arguments"] {
                            self.output_bytes += call["function"][key].as_str().map_or(0, str::len);
                        }
                    }
                }
            }
        }
        if let Some(content) = v["content"].as_array() {
            for block in content {
                self.output_bytes += block["text"].as_str().map_or(0, str::len);
                if block["type"] == "tool_use" {
                    self.output_bytes += block["input"].to_string().len();
                }
            }
        }
        for key in ["text", "thinking", "partial_json"] {
            self.output_bytes += v["delta"][key].as_str().map_or(0, str::len);
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::take(&mut self.parser);
        parser.push(bytes, &mut |_, data| {
            if let Ok(v) = serde_json::from_str(data) {
                self.value(&v);
            }
        });
        self.parser = parser;
    }

    /// Output tokens generated so far: reported, or estimated from the text streamed.
    pub fn live_output(&self) -> u64 {
        self.output.unwrap_or(estimate(self.output_bytes))
    }

    pub fn finish(&mut self) -> Tokens {
        let mut parser = std::mem::take(&mut self.parser);
        parser.flush(&mut |_, data| {
            if let Ok(v) = serde_json::from_str(data) {
                self.value(&v);
            }
        });
        let input_tokens = self.input.unwrap_or(self.input_estimate);
        let output_tokens = self.output.unwrap_or(estimate(self.output_bytes));
        Tokens {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            estimated_requests: u64::from(self.input.is_none() || self.output.is_none()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn partial_stream_events_and_final_counters() {
        let mut u = Usage::new(&json!({"messages": []}));
        let events = b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\ndata: {\"usage\":{\"prompt_tokens\":42,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n";
        for chunk in events.chunks(3) {
            u.push(chunk);
        }
        let t = u.finish();
        assert_eq!(
            (t.input_tokens, t.output_tokens, t.estimated_requests),
            (42, 3, 0)
        );
    }

    #[test]
    fn anthropic_cache_and_partial_usage() {
        let mut u = Usage::new(&json!({}));
        u.value(&json!({"message": {"usage": {"input_tokens": 10, "cache_read_input_tokens": 20, "cache_creation_input_tokens": 5, "output_tokens": 0}}}));
        u.value(&json!({"usage": {"output_tokens": 7}}));
        let t = u.finish();
        assert_eq!(
            (
                t.input_tokens,
                t.output_tokens,
                t.total_tokens,
                t.estimated_requests
            ),
            (35, 7, 42, 0)
        );
    }

    #[test]
    fn interrupted_anthropic_stream_estimates_generated_output() {
        let mut u = Usage::new(&json!({}));
        u.value(&json!({"type": "message_start", "message": {"usage": {"input_tokens": 10, "output_tokens": 0}}}));
        u.push(b"data: {\"delta\":{\"text\":\"abcdefgh\"}}\n\n");
        let t = u.finish();
        assert_eq!(
            (t.input_tokens, t.output_tokens, t.estimated_requests),
            (10, 2, 1)
        );
    }

    #[test]
    fn missing_counts_estimate_content_and_tools() {
        let mut u = Usage::new(&json!({"messages": [{"role":"user", "content":"hello"}]}));
        u.value(&json!({"choices": [{"message": {"content": "abcd", "tool_calls": [{"function": {"name": "tool", "arguments": "{}"}}]}}]}));
        let t = u.finish();
        assert_eq!(t.output_tokens, 3);
        assert!(t.input_tokens > 0);
        assert_eq!(t.estimated_requests, 1);
    }
}
