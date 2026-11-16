//! Talking to providers directly from the TUI: list models, test a key.

use crate::config::{Provider, Wire};
use crate::util::{chain, e2s, short};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Target {
    pub name: String,
    pub wire: Wire,
    pub base_url: String,
    pub key: Option<String>,
    pub headers: Vec<(String, String)>,
    pub models_url: Option<String>,
}

impl Target {
    pub fn from(name: &str, p: &Provider) -> Self {
        Target {
            name: name.to_string(),
            wire: p.wire,
            base_url: p.base_url.clone(),
            key: p.keys.first().cloned(),
            headers: p
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            models_url: crate::catalog::get(name)
                .and_then(|e| e.models_url)
                .map(String::from),
        }
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(8))
        .user_agent(concat!("tako/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

fn auth(mut rb: reqwest::RequestBuilder, t: &Target) -> reqwest::RequestBuilder {
    match t.wire {
        Wire::Openai => {
            if let Some(k) = &t.key {
                rb = rb.bearer_auth(k);
            }
        }
        Wire::Anthropic => {
            if let Some(k) = &t.key {
                rb = rb.header("x-api-key", k);
            }
            rb = rb.header("anthropic-version", "2023-06-01");
        }
    }
    for (k, v) in &t.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    rb
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            let e = &v["error"];
            e["message"]
                .as_str()
                .or(e.as_str())
                .or(v["message"].as_str())
                .or(v["detail"].as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| short(body, 160))
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    #[test]
    fn model_metadata_handles_zero_prices_without_guessing_missing_prices() {
        let list = parse_models("custom", &json!({"data": [
            {"id": "numeric", "pricing": {"prompt": 0, "completion": 0}},
            {"id": "string", "pricing": {"prompt": "0.0", "completion": "0"}},
            {"id": "blockrun", "pricing": {"input": 0, "output": 0}},
            {"id": "unknown"},
            {"id": "partial", "pricing": {"prompt": "0"}},
            {"id": "paid", "pricing": {"prompt": "0", "completion": "0.001"}},
            {"id": "request-fee", "pricing": {"prompt": 0, "completion": 0, "request": "0.01"}},
            {"id": "offline", "available": false, "pricing": {"input": 0, "output": 0}},
            {"id": "music", "architecture": {"output_modalities": ["audio"]}, "pricing": {"prompt": "0", "completion": "0"}}
        ]})).unwrap();
        for id in ["numeric", "string", "blockrun"] {
            assert!(list.contains(&(id.into(), true)));
        }
        for id in ["unknown", "partial", "paid", "request-fee"] {
            assert!(list.contains(&(id.into(), false)));
        }
        assert!(!list.iter().any(|(id, _)| id == "offline" || id == "music"));
    }

    #[test]
    fn cloudflare_lists_model_names_instead_of_internal_ids() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
            }
            let body = r#"{"success":true,"result":[{"id":"internal-id","name":"@cf/zai-org/glm-4.7-flash"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            String::from_utf8(request).unwrap()
        });
        let target = Target {
            name: "cloudflare".into(),
            wire: Wire::Openai,
            base_url: format!("http://{address}/accounts/account/ai/v1"),
            key: Some("test-token".into()),
            headers: vec![],
            models_url: None,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let list = rt.block_on(models(target)).unwrap();
        assert!(list.iter().any(|(id, _)| id == "@cf/zai-org/glm-4.7-flash"));
        assert!(!list.iter().any(|(id, _)| id == "internal-id"));
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /accounts/account/ai/models/search HTTP/1.1"));
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer test-token"));
    }
}

/// (model id, priced as free) for every model the provider lists.
pub async fn models(t: Target) -> Result<Vec<(String, bool)>, String> {
    let url = t.models_url.clone().unwrap_or_else(|| match t.wire {
        Wire::Openai if t.name == "cloudflare" => {
            format!("{}/models/search", t.base_url.trim_end_matches("/v1"))
        }
        Wire::Openai => format!("{}/models", t.base_url),
        Wire::Anthropic => format!("{}/v1/models", t.base_url),
    });
    let mut rb = auth(client().get(&url), &t);
    for (k, v) in crate::catalog::get(&t.name)
        .map(|e| e.list_headers)
        .unwrap_or_default()
    {
        rb = rb.header(*k, *v);
    }
    let resp = rb.send().await.map_err(|e| chain(&e))?;
    let status = resp.status();
    let body = resp.text().await.map_err(e2s)?;
    if !status.is_success() {
        return Err(format!("{}: {}", status.as_u16(), error_message(&body)));
    }
    let v: Value =
        serde_json::from_str(&body).map_err(|_| "unexpected model list format".to_string())?;
    parse_models(&t.name, &v)
}

fn parse_models(provider: &str, v: &Value) -> Result<Vec<(String, bool)>, String> {
    let arr = v
        .get("data")
        .or(v.get("models"))
        .or(v.get("result"))
        .unwrap_or(v)
        .as_array()
        .ok_or("unexpected model list format")?;
    let mut out: Vec<(String, bool)> = arr
        .iter()
        .filter_map(|m| {
            if m["available"] == false {
                return None;
            }
            if m["architecture"]["output_modalities"]
                .as_array()
                .is_some_and(|outputs| !outputs.iter().any(|v| v == "text"))
            {
                return None;
            }
            let id = if provider == "cloudflare" {
                m["name"].as_str()
            } else {
                m["id"].as_str().or(m["name"].as_str())
            }?;
            let id = id.strip_prefix("models/").unwrap_or(id).to_string();
            let p = &m["pricing"];
            let zero =
                |v: &Value| v.as_f64().or_else(|| v.as_str()?.parse::<f64>().ok()) == Some(0.0);
            let token_free = (zero(&p["prompt"]) && zero(&p["completion"]))
                || (zero(&p["input"]) && zero(&p["output"]));
            let extras = [
                "request",
                "image",
                "audio",
                "web_search",
                "internal_reasoning",
                "input_cache_read",
                "input_cache_write",
            ];
            let priced_free = token_free && extras.iter().all(|key| p.get(key).is_none_or(&zero));
            Some((id, priced_free))
        })
        .collect();
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    if let Some(e) = crate::catalog::get(provider).filter(|e| e.list_incomplete) {
        for m in e.models {
            if !out.iter().any(|(id, _)| id == m) {
                out.push((m.to_string(), false));
            }
        }
    }
    Ok(out)
}

/// The model to test a fresh key with: a suggested one the provider really lists,
/// else its first free model, else whatever it lists first.
pub fn pick_test_model(name: &str, live: &[(String, bool)]) -> Option<String> {
    let entry = crate::catalog::get(name);
    let has = |m: &str| live.iter().any(|(id, _)| id == m);
    entry
        .and_then(|e| e.models.iter().find(|m| has(m)).map(|m| m.to_string()))
        .or_else(|| {
            live.iter()
                .find(|(id, pf)| entry.is_some_and(|e| e.is_free(id, *pf)))
                .map(|(id, _)| id.clone())
        })
        .or_else(|| live.first().map(|(id, _)| id.clone()))
}

/// Send one tiny request to prove the key and model work.
pub async fn test(t: Target, model: Option<String>) -> Result<String, String> {
    let model = match model {
        Some(m) => m,
        None => {
            let live = models(t.clone()).await?;
            match pick_test_model(&t.name, &live) {
                Some(m) => m,
                None => return Ok(format!("{} is reachable but lists no models", t.name)),
            }
        }
    };
    let prompt = "Reply with exactly one word: pong";
    let (url, body) = match t.wire {
        Wire::Openai => (
            format!("{}/chat/completions", t.base_url),
            json!({"model": model, "max_tokens": 64, "messages": [{"role": "user", "content": prompt}]}),
        ),
        Wire::Anthropic => (
            format!("{}/v1/messages", t.base_url),
            json!({"model": model, "max_tokens": 64, "messages": [{"role": "user", "content": prompt}]}),
        ),
    };
    let began = Instant::now();
    let resp = auth(client().post(url), &t)
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&body).unwrap_or_default())
        .send()
        .await
        .map_err(|e| chain(&e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(e2s)?;
    let ms = began.elapsed().as_millis();
    if !status.is_success() {
        let kind = crate::failure::classify(
            status.as_u16(),
            None,
            &text,
            &crate::failure::Policy {
                cooldown: 60,
                long: 1800,
            },
        )
        .kind;
        let hint = match kind {
            crate::failure::Kind::Billing => {
                " (this model isn't free on your account: pick a free one)"
            }
            crate::failure::Kind::Auth => " (check the key)",
            crate::failure::Kind::NotFound => " (model gone: pick another with m)",
            crate::failure::Kind::Quota | crate::failure::Kind::RateLimit => {
                " (key works, but it's rate-limited right now)"
            }
            _ => "",
        };
        return Err(format!(
            "{}/{model}: {} {}: {}{hint}",
            t.name,
            status.as_u16(),
            kind.label(),
            error_message(&text)
        ));
    }
    let v: Value = serde_json::from_str(&text).unwrap_or_default();
    let reply = v["choices"][0]["message"]["content"]
        .as_str()
        .or(v["content"][0]["text"].as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(if reply.is_empty() {
        format!("{}/{model} works ({ms} ms)", t.name)
    } else {
        format!(
            "{}/{model} answered in {ms} ms: \"{}\"",
            t.name,
            short(&reply, 30)
        )
    })
}

/// Use the local router so chat gets the same fallback and key rotation as clients.
pub async fn chat(
    listen: String,
    token: String,
    model: String,
    messages: Vec<Value>,
) -> Result<String, String> {
    let addr = crate::util::local_addr(&listen)?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(180))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(e2s)?
        .post(format!("http://{addr}/v1/chat/completions"))
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(
            serde_json::to_vec(
                &json!({"model": model, "messages": messages, "max_tokens": 2048, "stream": false}),
            )
            .map_err(e2s)?,
        )
        .send()
        .await
        .map_err(|e| chain(&e))?;
    let status = response.status();
    let body = response.text().await.map_err(e2s)?;
    if !status.is_success() {
        return Err(format!("{}: {}", status.as_u16(), error_message(&body)));
    }
    let value: Value =
        serde_json::from_str(&body).map_err(|_| "Unexpected chat response".to_string())?;
    value["choices"][0]["message"]["content"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(String::from)
        .ok_or_else(|| "The model returned no text. Try another combo.".into())
}

#[cfg(test)]
mod chat_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn request(status: &str, body: &str) -> (Result<String, String>, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let len: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(bytes).unwrap()
        });
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(chat(
            addr.to_string(),
            "local-token".into(),
            "free".into(),
            vec![
                json!({"role": "user", "content": "hello"}),
                json!({"role": "assistant", "content": "hi"}),
                json!({"role": "user", "content": "remember me?"}),
            ],
        ));
        (result, server.join().unwrap())
    }

    #[test]
    fn local_chat_sends_auth_combo_and_conversation() {
        let (result, request) = request("200 OK", r#"{"choices":[{"message":{"content":"yes"}}]}"#);
        assert_eq!(result.unwrap(), "yes");
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer local-token"));
        let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["model"], "free");
        assert_eq!(body["messages"].as_array().unwrap().len(), 3);
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn local_chat_reports_router_errors_and_empty_replies() {
        assert_eq!(
            request(
                "429 Too Many Requests",
                r#"{"error":{"message":"all targets cooling down"}}"#
            )
            .0
            .unwrap_err(),
            "429: all targets cooling down"
        );
        assert!(
            request("200 OK", r#"{"choices":[{"message":{"content":""}}]}"#)
                .0
                .is_err()
        );
    }
}
