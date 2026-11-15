//! Classifying upstream failures so fallback can react sensibly:
//! out-of-credits keys sit out for a long time, rate limits wait exactly as
//! long as the provider says, and requests a provider rejects move on
//! without benching anyone.

use reqwest::header::HeaderMap;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Kind {
    /// 402, insufficient credits / balance / quota.
    Billing,
    /// Daily or monthly allowance used up.
    Quota,
    /// Per-minute style rate limit.
    RateLimit,
    /// 401/403: bad, revoked or unauthorized key.
    Auth,
    /// The model doesn't exist (anymore) on this provider.
    NotFound,
    /// Prompt or payload too large for this provider.
    TooLarge,
    /// Provider rejects this particular request.
    BadRequest,
    /// 529 / "overloaded".
    Overloaded,
    /// Other 5xx.
    Server,
    /// Connection refused, DNS, TLS ...
    Network,
    /// Upstream took too long.
    Timeout,
    /// 200 but the body was an error or empty.
    BadResponse,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Billing => "billing",
            Kind::Quota => "quota",
            Kind::RateLimit => "rate_limit",
            Kind::Auth => "auth",
            Kind::NotFound => "not_found",
            Kind::TooLarge => "too_large",
            Kind::BadRequest => "bad_request",
            Kind::Overloaded => "overloaded",
            Kind::Server => "server",
            Kind::Network => "network",
            Kind::Timeout => "timeout",
            Kind::BadResponse => "bad_response",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Billing => "out of credits",
            Kind::Quota => "quota used up",
            Kind::RateLimit => "rate limited",
            Kind::Auth => "key rejected",
            Kind::NotFound => "model not found",
            Kind::TooLarge => "request too large",
            Kind::BadRequest => "request rejected",
            Kind::Overloaded => "overloaded",
            Kind::Server => "server error",
            Kind::Network => "unreachable",
            Kind::Timeout => "timed out",
            Kind::BadResponse => "bad response",
        }
    }

    /// Won't fix itself soon: not worth retrying when everything is cooling down.
    pub fn sticky(self) -> bool {
        matches!(
            self,
            Kind::Billing | Kind::Quota | Kind::Auth | Kind::NotFound
        )
    }

    /// The request itself is the problem for this provider; nobody gets benched.
    pub fn request_problem(self) -> bool {
        matches!(self, Kind::TooLarge | Kind::BadRequest)
    }

    pub fn is_limit(self) -> bool {
        matches!(self, Kind::RateLimit | Kind::Quota)
    }
}

/// What to do about a failure.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Bench this key; other keys of the same provider may still work.
    Key,
    /// Bench every key of this target (e.g. model gone) and move on.
    Target,
    /// Move to the next provider without benching anything.
    Skip,
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub kind: Kind,
    pub scope: Scope,
    /// Seconds to sit out (ignored for Scope::Skip).
    pub cooldown: u64,
}

pub struct Policy {
    pub cooldown: u64,
    pub long: u64,
}

const BILLING: &[&str] = &[
    "insufficient_quota",
    "insufficient quota",
    "insufficient balance",
    "insufficient_balance",
    "insufficient credit",
    "insufficient funds",
    "insufficient_funds",
    "credit balance is too low",
    "out of credits",
    "no credits",
    "not enough credits",
    "requires more credits",
    "payment required",
    "payment_required",
    "exceeded your current quota",
    "billing",
    "balance is insufficient",
    "account balance",
    "top up",
    "purchase credits",
    "add credits",
];

const DAILY: &[&str] = &[
    "per day",
    "per-day",
    "perday",
    "per_day",
    "daily",
    "(rpd)",
    "(tpd)",
    "per month",
    "monthly",
    "free tier limit",
];

const MODEL_GONE: &[&str] = &[
    "model_not_found",
    "model not found",
    "no such model",
    "does not exist",
    "is not a valid model",
    "invalid model",
    "unknown model",
    "decommissioned",
    "no endpoints found",
    "not supported model",
];

const TOO_LARGE: &[&str] = &[
    "context length",
    "context_length",
    "context window",
    "prompt is too long",
    "too many tokens",
    "maximum context",
    "request too large",
    "reduce the length",
];

fn any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

/// Decide what an upstream HTTP error means.
pub fn classify(status: u16, headers: Option<&HeaderMap>, body: &str, p: &Policy) -> Failure {
    let lower = body.to_lowercase();
    let wait = reset_secs(headers, body);
    let f = |kind, scope, cooldown: u64| Failure {
        kind,
        scope,
        cooldown: cooldown.clamp(1, 86_400),
    };

    // Billing wording wins over the status code: Anthropic says 400, OpenAI says 429.
    if status == 402 || (matches!(status, 400 | 403 | 429) && any(&lower, BILLING)) {
        return f(Kind::Billing, Scope::Key, p.long);
    }
    match status {
        502 if lower.starts_with("provider returned an empty")
            || lower.starts_with("stream broke") =>
        {
            f(Kind::BadResponse, Scope::Key, p.cooldown)
        }
        429 if lower.contains("overload") => f(
            Kind::Overloaded,
            Scope::Key,
            wait.unwrap_or(p.cooldown.min(30)),
        ),
        429 if any(&lower, DAILY) => f(Kind::Quota, Scope::Key, wait.unwrap_or(p.long)),
        429 => f(Kind::RateLimit, Scope::Key, wait.unwrap_or(p.cooldown)),
        401 | 403 => f(Kind::Auth, Scope::Key, p.long),
        404 => f(Kind::NotFound, Scope::Target, p.long),
        408 | 504 | 524 => f(Kind::Timeout, Scope::Key, p.cooldown),
        413 if lower.contains("rate") => f(Kind::RateLimit, Scope::Skip, 0), // Groq: over tokens-per-minute
        413 => f(Kind::TooLarge, Scope::Skip, 0),
        400 | 422 if any(&lower, MODEL_GONE) => f(Kind::NotFound, Scope::Target, p.long),
        400 | 422 if any(&lower, TOO_LARGE) => f(Kind::TooLarge, Scope::Skip, 0),
        400 | 422 => f(Kind::BadRequest, Scope::Skip, 0),
        529 => f(
            Kind::Overloaded,
            Scope::Key,
            wait.unwrap_or(p.cooldown.min(30)),
        ),
        503 if lower.contains("overload") => f(
            Kind::Overloaded,
            Scope::Key,
            wait.unwrap_or(p.cooldown.min(30)),
        ),
        _ => f(Kind::Server, Scope::Key, wait.unwrap_or(p.cooldown)),
    }
}

/// A 200 response whose body is really an error (some gateways do that).
/// Returns (status to treat it as, raw error text).
pub fn error_in_body(v: &Value) -> Option<(u16, String)> {
    let e = v.get("error").filter(|e| !e.is_null())?;
    if v.get("choices")
        .is_some_and(|c| c.as_array().is_some_and(|a| !a.is_empty()))
        || v.get("content").is_some()
    {
        return None;
    }
    let code = e["code"]
        .as_u64()
        .or(e["status"].as_u64())
        .filter(|c| (400..600).contains(c))
        .unwrap_or(502) as u16;
    Some((code, v.to_string()))
}

/// Scan the first SSE events of a stream for an error event.
pub fn error_in_stream(head: &[u8]) -> Option<(u16, String)> {
    let s = String::from_utf8_lossy(head);
    let mut event = "";
    for line in s.lines() {
        if let Some(e) = line.strip_prefix("event:") {
            event = e.trim();
        } else if let Some(d) = line.strip_prefix("data:") {
            let d = d.trim();
            if d == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(d) else {
                continue;
            };
            if event == "error" || v["type"] == "error" {
                let code = v["error"]["code"]
                    .as_u64()
                    .filter(|c| (400..600).contains(c))
                    .unwrap_or(502) as u16;
                let code = if v["error"]["type"] == "overloaded_error" {
                    529
                } else {
                    code
                };
                return Some((code, d.to_string()));
            }
            if let Some(r) = error_in_body(&v) {
                return Some(r);
            }
            return None; // first real event is fine
        }
    }
    None
}

/// How long the provider asks us to wait, from headers or the error body.
pub fn reset_secs(headers: Option<&HeaderMap>, body: &str) -> Option<u64> {
    let h = |name: &str| {
        headers
            .and_then(|h| h.get(name))
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
    };
    if let Some(s) = h("retry-after").and_then(|v| v.parse::<f64>().ok()) {
        return Some(s.ceil() as u64);
    }
    if let Some(s) = h("x-ratelimit-reset").and_then(epochish) {
        return Some(s);
    }
    // OpenAI / Groq style: reset per bucket, only one of which is exhausted.
    let mut best: Option<u64> = None;
    for b in ["requests", "tokens"] {
        let empty = h(&format!("x-ratelimit-remaining-{b}")).is_some_and(|v| v == "0");
        if let Some(d) = h(&format!("x-ratelimit-reset-{b}")).and_then(go_duration) {
            if empty {
                best = Some(best.map_or(d, |x: u64| x.max(d)));
            }
        }
    }
    if best.is_some() {
        return best;
    }
    from_body(body)
}

fn from_body(body: &str) -> Option<u64> {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        // OpenRouter tucks the upstream headers into the error metadata.
        let hs = &v["error"]["metadata"]["headers"];
        for k in ["X-RateLimit-Reset", "x-ratelimit-reset"] {
            if let Some(s) = hs[k]
                .as_str()
                .and_then(epochish)
                .or_else(|| hs[k].as_u64().and_then(|n| epochish(&n.to_string())))
            {
                return Some(s);
            }
        }
    }
    let lower = body.to_lowercase();
    // Gemini: "retryDelay": "37s"
    if let Some(i) = lower.find("retrydelay") {
        let rest = &lower[i + 10..];
        let start = rest.find(|c: char| c.is_ascii_digit())?;
        let tail: String = rest[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        if let Some(d) = go_duration(&tail) {
            return Some(d);
        }
    }
    // Groq and others: "Please try again in 7m12.48s"
    for marker in ["try again in ", "retry in ", "retry after "] {
        if let Some(i) = lower.find(marker) {
            let tail: String = lower[i + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
                .collect();
            let tail = tail.trim_end_matches('.');
            if let Some(d) = go_duration(tail) {
                return Some(d);
            }
            if let Ok(n) = tail.parse::<f64>() {
                return Some(n.ceil() as u64);
            }
        }
    }
    None
}

/// A reset given as seconds-from-now, unix seconds, or unix milliseconds.
fn epochish(v: &str) -> Option<u64> {
    let n = v.parse::<f64>().ok()? as u64;
    let now = crate::util::now_secs();
    Some(match n {
        n if n > 1_000_000_000_000 => (n / 1000).saturating_sub(now),
        n if n > 1_000_000_000 => n.saturating_sub(now),
        n => n,
    })
    .map(|s| s.max(1))
}

/// "1h2m3.5s", "6m0s", "250ms", "37s" -> whole seconds (rounded up).
pub fn go_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total = 0f64;
    let mut num = String::new();
    let mut chars = s.chars().peekable();
    let mut any_unit = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let n: f64 = num.parse().ok()?;
        num.clear();
        let mult = match c {
            'h' => 3600.0,
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                0.001
            }
            'm' => 60.0,
            's' => 1.0,
            'd' => 86400.0,
            _ => return None,
        };
        total += n * mult;
        any_unit = true;
    }
    if !num.is_empty() || !any_unit {
        return None;
    }
    Some((total.ceil() as u64).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Policy = Policy {
        cooldown: 60,
        long: 1800,
    };

    #[test]
    fn billing_variants() {
        assert_eq!(classify(402, None, "{}", &P).kind, Kind::Billing);
        let anth = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API."}}"#;
        assert_eq!(classify(400, None, anth, &P).kind, Kind::Billing);
        let oai = r#"{"error":{"message":"You exceeded your current quota, please check your plan and billing details.","type":"insufficient_quota"}}"#;
        let f = classify(429, None, oai, &P);
        assert_eq!((f.kind, f.cooldown), (Kind::Billing, 1800));
    }

    #[test]
    fn limits() {
        let groq = r#"{"error":{"message":"Rate limit reached for model on tokens per day (TPD): Limit 100000. Please try again in 7m12.48s."}}"#;
        let f = classify(429, None, groq, &P);
        assert_eq!((f.kind, f.cooldown), (Kind::Quota, 433));
        let rl = classify(429, None, r#"{"error":{"message":"slow down"}}"#, &P);
        assert_eq!((rl.kind, rl.cooldown), (Kind::RateLimit, 60));
        let zai = r#"{"error":{"code":"1305","message":"The service may be temporarily overloaded, please try again later"}}"#;
        assert_eq!(classify(429, None, zai, &P).kind, Kind::Overloaded);
        let zai_paid = r#"{"error":{"code":"1113","message":"Insufficient balance or no resource package. Please recharge."}}"#;
        assert_eq!(classify(429, None, zai_paid, &P).kind, Kind::Billing);
        let gem = r#"{"error":{"details":[{"retryDelay":"37s"}]}}"#;
        assert_eq!(classify(429, None, gem, &P).cooldown, 37);
    }

    #[test]
    fn request_problems() {
        assert_eq!(
            classify(400, None, "prompt is too long: 300000 tokens", &P).kind,
            Kind::TooLarge
        );
        assert_eq!(
            classify(400, None, "The model `x` does not exist", &P).scope,
            Scope::Target
        );
        assert_eq!(
            classify(422, None, "unsupported field", &P).scope,
            Scope::Skip
        );
    }

    #[test]
    fn durations() {
        assert_eq!(go_duration("6m0s"), Some(360));
        assert_eq!(go_duration("250ms"), Some(1));
        assert_eq!(go_duration("1h2m3.5s"), Some(3724));
        assert_eq!(go_duration("abc"), None);
    }

    #[test]
    fn body_errors() {
        let v: Value =
            serde_json::from_str(r#"{"error":{"message":"Insufficient credits","code":402}}"#)
                .unwrap();
        assert_eq!(error_in_body(&v).map(|e| e.0), Some(402));
        let ok: Value = serde_json::from_str(r#"{"choices":[{"message":{}}]}"#).unwrap();
        assert!(error_in_body(&ok).is_none());
        let sse = b": OPENROUTER PROCESSING\n\ndata: {\"error\":{\"message\":\"Rate limit\",\"code\":429}}\n\n";
        assert_eq!(error_in_stream(sse).map(|e| e.0), Some(429));
        let anth = b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        assert_eq!(error_in_stream(anth).map(|e| e.0), Some(529));
        assert!(error_in_stream(b"data: {\"choices\":[{\"delta\":{}}]}\n\n").is_none());
    }
}
