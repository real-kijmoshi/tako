//! Smart routes: model names like `free:auto` or `any:coding` that pick the best model
//! tako knows for the request, instead of a fixed combo.
//!
//! The name is `pool:task`. The pool says which models may be used (free, local, paid,
//! any); the task says what to optimise for. `auto` reads the request to choose a task.
//! Candidates are the models in your combos plus the catalog's suggestions for the
//! providers you've connected. They're ranked by what their names say they're good at,
//! then by how they've actually been doing (success rate, latency), and the whole
//! ranked list is the fallback order, so failover works as it does for a combo.

use crate::catalog::{self, Tier};
use crate::config::{Config, Target};
use serde_json::Value;
use std::collections::BTreeSet;

pub const POOLS: &[(&str, &str)] = &[
    ("free", "free and local models"),
    ("local", "models running on your machine"),
    ("paid", "paid models"),
    ("any", "every model"),
];

pub const TASKS: &[(&str, &str)] = &[
    (
        "auto",
        "reads each request and picks the task (also :router)",
    ),
    ("coding", "code, tool calls and agents"),
    ("reasoning", "hard problems worked step by step"),
    ("fast", "quick replies, lowest latency"),
    ("long", "very long prompts and documents"),
    ("vision", "prompts with images"),
    ("chat", "writing, questions, everyday chat"),
];

/// Most models a route will try before giving up.
const MAX_TRY: usize = 8;

/// How a target has been doing since the router started: (ok, failed, average ms when ok).
pub type Health<'a> = &'a dyn Fn(&str) -> Option<(u64, u64, u64)>;

pub struct Plan {
    /// what to show on the dashboard and in logs, e.g. `free:auto → coding`
    pub label: String,
    pub targets: Vec<Target>,
}

/// `free:coding` -> ("free", "coding"), if it names a route.
pub fn parse(name: &str) -> Option<(&'static str, &'static str)> {
    let (p, t) = name.split_once(':')?;
    let pool = POOLS.iter().find(|(n, _)| *n == p)?.0;
    // `router` is another name for `auto`: route each request automatically
    let t = if t == ROUTER { "auto" } else { t };
    let task = TASKS.iter().find(|(n, _)| *n == t)?.0;
    Some((pool, task))
}

/// Alias of `auto`, so `free:router` reads like what it does.
pub const ROUTER: &str = "router";

/// Every route that has at least one model to use, in a stable order.
pub fn available(cfg: &Config) -> Vec<String> {
    POOLS
        .iter()
        .filter(|(p, _)| !candidates(cfg, p).is_empty())
        .flat_map(|(p, _)| {
            TASKS
                .iter()
                .map(|(t, _)| *t)
                .chain([ROUTER])
                .map(move |t| format!("{p}:{t}"))
        })
        .collect()
}

/// The route a request should take, if its model (after aliases, or the default
/// when the model is unknown) names one.
pub fn plan(
    cfg: &Config,
    model: &str,
    body: &Value,
    health: Health,
) -> Option<Result<Plan, String>> {
    let name = cfg.alias(model).unwrap_or(model);
    let route = if parse(name).is_some() {
        name
    } else if cfg.lookup(name).is_none() {
        cfg.default.as_deref().filter(|d| parse(d).is_some())?
    } else {
        return None;
    };
    let (pool, asked) = parse(route)?;
    let task = if asked == "auto" {
        classify(body)
    } else {
        asked
    };
    // keep the word the client used: `free:router → coding`, `free:auto → coding`
    let label = if asked == "auto" {
        format!("{route} → {task}")
    } else {
        route.to_string()
    };
    let targets = rank(cfg, pool, task, health);
    Some(if targets.is_empty() {
        Err(format!(
            "no {} models to route '{pool}:{asked}' to: connect a provider that has some",
            if pool == "any" { "usable" } else { pool }
        ))
    } else {
        Ok(Plan { label, targets })
    })
}

/// Candidates for `pool`, best first for `task`.
pub fn rank(cfg: &Config, pool: &str, task: &str, health: Health) -> Vec<Target> {
    let combo: BTreeSet<&str> = cfg.combos.values().flatten().map(String::as_str).collect();
    let mut scored: Vec<(bool, i64, String)> = candidates(cfg, pool)
        .into_iter()
        .map(|tag| {
            let model = tag.split_once('/').map_or(tag.as_str(), |(_, m)| m);
            let tr = traits(model);
            let fit = tr.get(task);
            let mut score = fit * 10 + tr.quality * if task == "fast" { 1 } else { 3 };
            if combo.contains(tag.as_str()) {
                score += 4; // you picked it yourself
            }
            if let Some((ok, fail, ms)) = health(&tag) {
                let n = ok + fail;
                if n > 0 {
                    score += (ok as i64 * 20 / n as i64) - 10; // -10 .. +10 by success rate
                }
                if ok > 0 {
                    let per_sec = if task == "fast" { 6 } else { 2 };
                    score -= (ms as i64 * per_sec / 1000).min(30);
                }
            }
            // vision and long prompts only work on models built for them: those go first
            let must = matches!(task, "vision" | "long") && fit > 0;
            (must, score, tag)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    scored
        .into_iter()
        .take(MAX_TRY)
        .filter_map(|(_, _, tag)| cfg.parse_target(&tag))
        .collect()
}

/// Every model tako could send to in this pool: your combos plus catalog suggestions.
fn candidates(cfg: &Config, pool: &str) -> Vec<String> {
    let mut all: BTreeSet<String> = cfg.combos.values().flatten().cloned().collect();
    for (name, p) in &cfg.providers {
        if p.disabled {
            continue;
        }
        if let Some(e) = catalog::get(name) {
            all.extend(e.models.iter().map(|m| format!("{name}/{m}")));
        }
    }
    all.into_iter()
        .filter(|tag| {
            let Some(t) = cfg.parse_target(tag) else {
                return false;
            };
            if cfg.providers.get(&t.provider).is_none_or(|p| p.disabled) {
                return false;
            }
            in_pool(&t, pool)
        })
        .collect()
}

fn in_pool(t: &Target, pool: &str) -> bool {
    let entry = catalog::get(&t.provider);
    let local = entry.is_some_and(|e| e.tier == Tier::Local);
    let free = local
        || match entry {
            Some(e) => e.is_free(&t.model, false),
            // a provider tako doesn't know: only trust what the name says
            None => t.model.ends_with(":free") || t.model.ends_with("/free"),
        };
    match pool {
        "free" => free,
        "local" => local,
        "paid" => !free,
        _ => true,
    }
}

/// What the request looks like it needs.
pub fn classify(body: &Value) -> &'static str {
    let mut text = String::new();
    let mut image = false;
    collect(&body["system"], &mut text, &mut image);
    let messages = body["messages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for m in messages {
        collect(&m["content"], &mut text, &mut image);
    }
    if image {
        return "vision";
    }
    if text.len() / 4 > 48_000 {
        return "long";
    }
    let thinking = body["thinking"]["type"] == "enabled"
        || body.get("reasoning_effort").is_some()
        || body.get("reasoning").is_some();
    if thinking {
        return "reasoning";
    }
    if body["tools"].as_array().is_some_and(|t| !t.is_empty()) {
        return "coding";
    }
    let mut last = String::new();
    if let Some(m) = messages.iter().rev().find(|m| m["role"] == "user") {
        collect(&m["content"], &mut last, &mut image);
    }
    let lower = last.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if last.contains("```")
        || any(&[
            "code",
            "function",
            "bug",
            "error:",
            "stack trace",
            "traceback",
            "compile",
            "refactor",
            "regex",
            "sql",
            "python",
            " rust",
            "javascript",
            "typescript",
            "golang",
            "java ",
            "c++",
            "html",
            " css",
            " api",
            "script",
            "def ",
            "class ",
            "import ",
        ])
    {
        return "coding";
    }
    if any(&[
        "prove",
        "step by step",
        "solve",
        "calculate",
        "equation",
        "puzzle",
        "derive",
        "probability",
        "integral",
        "theorem",
        "logic",
    ]) {
        return "reasoning";
    }
    if last.chars().count() < 280 && text.len() < 8_000 {
        return "fast";
    }
    "chat"
}

/// Append the text of an OpenAI or Anthropic content value; note any images.
fn collect(v: &Value, out: &mut String, image: &mut bool) {
    match v {
        Value::String(s) => {
            out.push_str(s);
            out.push('\n');
        }
        Value::Array(parts) => {
            for p in parts {
                match p["type"].as_str() {
                    Some("image" | "image_url" | "input_image") => *image = true,
                    _ => {
                        if let Some(s) = p["text"].as_str() {
                            out.push_str(s);
                            out.push('\n');
                        }
                        // tool results carry their own content
                        if !p["content"].is_null() {
                            collect(&p["content"], out, image);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// What a model's name says it's good at. Rough on purpose: live health corrects it.
#[derive(Default, Debug, PartialEq)]
pub struct Traits {
    pub coding: i64,
    pub reasoning: i64,
    pub fast: i64,
    pub long: i64,
    pub vision: i64,
    pub chat: i64,
    pub quality: i64,
}

impl Traits {
    fn get(&self, task: &str) -> i64 {
        match task {
            "coding" => self.coding,
            "reasoning" => self.reasoning,
            "fast" => self.fast,
            "long" => self.long,
            "vision" => self.vision,
            _ => self.chat,
        }
    }
}

pub fn traits(model: &str) -> Traits {
    let m = model.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    let mut t = Traits::default();
    let params = billions(&m);

    if has(&["coder", "codestral", "devstral", "-code", "code-", "codex"]) {
        t.coding += 3;
    }
    if has(&[
        "qwen3",
        "glm-4.5",
        "glm-4.6",
        "glm-4.7",
        "glm-5",
        "kimi-k2",
        "gpt-oss",
        "deepseek",
        "claude",
        "gpt-5",
        "minimax",
        "sonnet",
        "opus",
        "longcat",
        "step-3.7",
        "step-5",
        "ling-3",
        "solar-mini4",
        "apodex",
        "laguna",
        "vikasit-nova",
    ]) {
        t.coding += 2;
    }
    if has(&[
        "-r1",
        "/r1",
        "reason",
        "think",
        "qwq",
        "o3",
        "o4-",
        "nemotron",
        "magistral",
    ]) {
        t.reasoning += 3;
    }
    if has(&[
        "gpt-oss",
        "glm-5",
        "kimi",
        "deepseek-v",
        "opus",
        "gpt-5",
        "ultra",
        "longcat",
        "step-3.7",
        "step-5",
        "ling-3",
        "apodex",
    ]) {
        t.reasoning += 1;
    }
    if has(&["lite", "nano", "mini", "instant", "small", "haiku", "tiny"]) {
        t.fast += 3;
    } else if has(&["flash", "turbo", "fast"]) {
        t.fast += 2;
    }
    if params.is_some_and(|b| b <= 20) {
        t.fast += 1;
    }
    if params.is_some_and(|b| b >= 200) || has(&["ultra", "opus", "-large", "reasoner"]) {
        t.fast -= 2;
    }
    if has(&[
        "vision", "-vl", "vl-", "gemini", "gpt-4o", "gpt-5", "pixtral", "llama-4", "gemma-3",
        "gemma-4", "glm-4.5v", "glm-4.6v", "claude",
    ]) {
        t.vision += 3;
    }
    if has(&[
        "gemini",
        "kimi",
        "-1m",
        "long",
        "minimax",
        "claude",
        "gpt-5",
        "ling-3",
        "solar-mini4",
        "apodex",
    ]) {
        t.long += 3;
    } else if has(&["glm", "deepseek", "qwen3", "command-a"]) {
        t.long += 1;
    }
    if has(&[
        "command", "mistral", "llama", "gemma", "chat", "instruct", "glm", "kimi", "claude", "gpt",
    ]) {
        t.chat += 2;
    }
    // bigger, flagship and "pro" models answer better; the smallest ones less so
    t.quality = match params {
        Some(b) if b >= 200 => 3,
        Some(b) if b >= 70 => 2,
        Some(b) if b >= 25 => 1,
        Some(b) if b < 10 => -1,
        _ => 0,
    };
    if has(&[
        "pro", "-large", "ultra", "opus", "sonnet", "plus", "-max", "gpt-5", "kimi-k2", "glm-5",
    ]) {
        t.quality += 2;
    }
    if has(&["lite", "nano", "tiny"]) {
        t.quality -= 1;
    }
    t
}

/// "nemotron-3-ultra-550b-a55b" -> 550 (the first `<number>b` that isn't an active-params `a55b`).
fn billions(m: &str) -> Option<u64> {
    let b = m.as_bytes();
    (0..b.len()).find_map(|i| {
        if b[i] != b'b' || i == 0 || !b[i - 1].is_ascii_digit() {
            return None;
        }
        let start = (0..i)
            .rev()
            .take_while(|&j| b[j].is_ascii_digit() || b[j] == b'.')
            .last()?;
        let boundary = start == 0 || !b[start - 1].is_ascii_alphabetic();
        let after = b.get(i + 1).is_none_or(|c| !c.is_ascii_alphanumeric());
        (boundary && after)
            .then(|| m[start..i].parse::<f64>().ok().map(|v| v as u64))
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(toml: &str) -> Config {
        crate::config::parse(toml).unwrap()
    }

    const CFG: &str = r#"
[providers.groq]
type = "openai"
base_url = "https://api.groq.com/openai/v1"
key = "k"
[providers.zai]
type = "openai"
base_url = "https://api.z.ai/api/paas/v4"
key = "k"
[providers.deepseek]
type = "openai"
base_url = "https://api.deepseek.com/v1"
key = "k"
[combos]
mine = ["zai/glm-4.5-flash", "deepseek/deepseek-chat"]
"#;

    const NONE: Health = &|_| None;

    #[test]
    fn parses_only_known_routes() {
        assert_eq!(parse("free:coding"), Some(("free", "coding")));
        assert_eq!(parse("any:auto"), Some(("any", "auto")));
        assert_eq!(parse("free:router"), Some(("free", "auto")));
        assert_eq!(parse("free:dance"), None);
        assert_eq!(parse("openrouter/x:free"), None);
        assert_eq!(parse("mine"), None);
    }

    #[test]
    fn reads_what_a_request_needs() {
        let ask = |s: &str| json!({"messages": [{"role": "user", "content": s}]});
        assert_eq!(classify(&ask("hi there")), "fast");
        assert_eq!(
            classify(&ask("fix this python function that throws a KeyError")),
            "coding"
        );
        assert_eq!(
            classify(&ask("prove that the square root of 2 is irrational")),
            "reasoning"
        );
        assert_eq!(
            classify(&ask(&"tell me a story about the sea. ".repeat(20))),
            "chat"
        );
        assert_eq!(classify(&ask(&"x".repeat(200_004))), "long");
        let img = json!({"messages": [{"role": "user", "content": [{"type": "text", "text": "what is this"}, {"type": "image_url", "image_url": {"url": "data:"}}]}]});
        assert_eq!(classify(&img), "vision");
        assert_eq!(
            classify(&json!({"tools": [{"name": "bash"}], "messages": []})),
            "coding"
        );
        assert_eq!(
            classify(&json!({"thinking": {"type": "enabled"}, "messages": []})),
            "reasoning"
        );
    }

    #[test]
    fn names_say_what_models_do() {
        assert!(traits("qwen3-coder-480b").coding > traits("gemma-4-31b-it").coding);
        assert!(traits("deepseek-reasoner").reasoning > traits("deepseek-chat").reasoning);
        assert!(
            traits("gemini-flash-lite-latest").fast
                > traits("nvidia/nemotron-3-ultra-550b-a55b:free").fast
        );
        assert_eq!(billions("nemotron-3-ultra-550b-a55b"), Some(550));
        assert_eq!(billions("gpt-oss-20b"), Some(20));
        assert_eq!(billions("glm-4.5-flash"), None);
    }

    #[test]
    fn free_routes_use_only_free_models() {
        let c = cfg(CFG);
        let free = rank(&c, "free", "chat", NONE);
        assert!(!free.is_empty());
        assert!(
            free.iter().all(|t| t.provider != "deepseek"),
            "DeepSeek has no free models"
        );
        let paid: Vec<String> = rank(&c, "paid", "coding", NONE)
            .iter()
            .map(|t| format!("{}/{}", t.provider, t.model))
            .collect();
        assert!(paid.contains(&"deepseek/deepseek-chat".to_string()));
        assert!(!paid
            .iter()
            .any(|t| t.starts_with("zai/") || t.starts_with("groq/")));
    }

    #[test]
    fn gated_and_unverified_services_cannot_join_free_routes() {
        let c = cfg(r#"
[providers.nous]
type = "openai"
key = "test"
[providers.novita]
type = "openai"
key = "test"
[providers.vikasit]
type = "openai"
key = "test"
[providers.blockrun]
type = "openai"
[combos]
manual = ["nous/upstage/solar-mini4:free", "novita/apodex/apodex-1.1-mini", "vikasit/vikasit-nova"]
"#);
        let free = rank(&c, "free", "coding", NONE);
        assert!(!free.is_empty());
        assert!(free.iter().all(|target| target.provider == "blockrun"));
        for provider in ["nous", "novita", "vikasit"] {
            for target in catalog::get(provider).unwrap().models {
                assert!(!in_pool(
                    &Target {
                        provider: provider.into(),
                        model: (*target).into()
                    },
                    "free"
                ));
            }
        }
    }

    #[test]
    fn picks_the_model_for_the_task() {
        let c = cfg(CFG);
        let first = |pool, task| {
            let t = &rank(&c, pool, task, NONE)[0];
            format!("{}/{}", t.provider, t.model)
        };
        assert_eq!(first("paid", "reasoning"), "deepseek/deepseek-reasoner");
        assert_eq!(first("free", "coding"), "groq/openai/gpt-oss-120b");
        assert_eq!(first("free", "fast"), "zai/glm-4.5-flash");
    }

    #[test]
    fn health_moves_a_failing_model_down() {
        let c = cfg(CFG);
        let base = rank(&c, "free", "chat", NONE);
        let top = format!("{}/{}", base[0].provider, base[0].model);
        let sick = move |t: &str| (t == top).then_some((0, 9, 0));
        let after = rank(&c, "free", "chat", &sick);
        assert_ne!(
            format!("{}/{}", after[0].provider, after[0].model),
            format!("{}/{}", base[0].provider, base[0].model)
        );
    }

    #[test]
    fn aliases_and_default_can_point_at_routes() {
        let c = cfg(&format!(
            "default = \"free:auto\"\n[aliases]\n\"claude-*\" = \"any:coding\"\n{CFG}"
        ));
        let body = json!({"messages": [{"role": "user", "content": "hello"}]});
        let p = plan(&c, "claude-sonnet-9", &body, NONE).unwrap().unwrap();
        assert_eq!(p.label, "any:coding");
        let p = plan(&c, "something-unknown", &body, NONE).unwrap().unwrap();
        assert_eq!(p.label, "free:auto → fast");
        let p = plan(
            &c,
            "free:router",
            &json!({"tools": [{"name": "bash"}], "messages": []}),
            NONE,
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.label, "free:router → coding");
        assert!(c.resolve("any:router").is_ok());
        assert!(
            plan(&c, "mine", &body, NONE).is_none(),
            "a real combo is not a route"
        );
        assert!(c.resolve("free:coding").is_ok());
    }

    #[test]
    fn empty_pool_explains_itself() {
        let c = cfg(CFG);
        let e = plan(&c, "local:auto", &json!({}), NONE)
            .unwrap()
            .err()
            .unwrap();
        assert!(e.contains("no local models"), "{e}");
    }
}
