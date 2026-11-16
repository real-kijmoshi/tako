use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:20129";

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Wire {
    Openai,
    Anthropic,
}

impl Wire {
    pub fn name(self) -> &'static str {
        match self {
            Wire::Openai => "openai",
            Wire::Anthropic => "anthropic",
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    #[serde(rename = "type")]
    pub wire: Wire,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Clamp max_tokens for providers that reject large values.
    #[serde(default)]
    pub max_tokens: Option<u64>,
    /// Keys were configured but none could be resolved (missing env vars).
    #[serde(skip)]
    pub disabled: bool,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_cooldown")]
    pub cooldown: u64,
    /// Seconds to bench a key that is out of credits, has used up its daily quota
    /// (when the provider doesn't say when it resets), was rejected, or names a missing model.
    #[serde(default = "default_long_cooldown")]
    pub long_cooldown: u64,
    #[serde(default)]
    pub providers: BTreeMap<String, Provider>,
    #[serde(default)]
    pub combos: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// Combo (or provider/model) used when a request names a model tako doesn't know.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(skip)]
    pub warnings: Vec<String>,
}

fn default_listen() -> String {
    DEFAULT_LISTEN.into()
}
fn default_cooldown() -> u64 {
    60
}
fn default_long_cooldown() -> u64 {
    1800
}

#[derive(Debug, Clone)]
pub struct Target {
    pub provider: String,
    pub model: String,
}

impl Config {
    /// "provider/model" -> Target, if the provider exists. The model part may contain more slashes.
    pub fn parse_target(&self, s: &str) -> Option<Target> {
        let (p, m) = s.split_once('/')?;
        (self.providers.contains_key(p) && !m.is_empty()).then(|| Target {
            provider: p.into(),
            model: m.into(),
        })
    }

    pub fn alias(&self, model: &str) -> Option<&str> {
        if let Some(t) = self.aliases.get(model) {
            return Some(t);
        }
        self.aliases
            .iter()
            .filter_map(|(pat, t)| {
                pat.strip_suffix('*')
                    .filter(|pre| model.starts_with(pre))
                    .map(|pre| (pre.len(), t))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, t)| t.as_str())
    }

    /// What the client asked for -> ordered list of targets to try.
    pub fn resolve(&self, model: &str) -> Result<Vec<Target>, String> {
        // smart routes (`free:auto`, `any:coding`, ...) rank models per request; without one, plain order
        if let Some(plan) = crate::route::plan(self, model, &serde_json::Value::Null, &|_| None) {
            return plan.map(|p| p.targets);
        }
        let name = self.alias(model).unwrap_or(model);
        if let Some(t) = self.lookup(name) {
            return Ok(t);
        }
        if let Some(t) = self.default.as_deref().and_then(|d| self.lookup(d)) {
            return Ok(t);
        }
        Err(format!("unknown model '{model}': use a combo name, provider/model, or a smart route like free:auto (run `tako status` to see them)"))
    }

    pub fn lookup(&self, name: &str) -> Option<Vec<Target>> {
        if let Some(list) = self.combos.get(name) {
            return Some(list.iter().filter_map(|t| self.parse_target(t)).collect());
        }
        self.parse_target(name).map(|t| vec![t])
    }
}

pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("TAKO_HOME") {
        return d.into();
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(d).join("tako");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(".config/tako")
}

pub fn path() -> PathBuf {
    dir().join("config.toml")
}

/// Create the config dir and a commented starter config if missing. Returns the config path.
pub fn ensure() -> Result<PathBuf, String> {
    let d = dir();
    if !d.exists() {
        fs::create_dir_all(&d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
        let _ = fs::set_permissions(&d, fs::Permissions::from_mode(0o700));
    }
    let p = path();
    if !p.exists() {
        fs::write(&p, TEMPLATE).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(p)
}

pub fn load(path: &Path) -> Result<Config, String> {
    let s = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse(&s).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse(s: &str) -> Result<Config, String> {
    let mut c: Config = toml::from_str(s).map_err(|e| e.to_string())?;

    let mut warnings = Vec::new();
    for (name, p) in c.providers.iter_mut() {
        if name.contains('/') {
            return Err(format!("provider name '{name}' must not contain '/'"));
        }
        if let Some(k) = p.key.take() {
            p.keys.insert(0, k);
        }
        let configured = !p.keys.is_empty();
        let mut keys = Vec::with_capacity(p.keys.len());
        for k in &p.keys {
            match k.strip_prefix("env:") {
                Some(var) => match std::env::var(var) {
                    Ok(v) if !v.is_empty() => keys.push(v),
                    _ => warnings.push(format!("provider '{name}': env var {var} is not set")),
                },
                None if !k.is_empty() => keys.push(k.clone()),
                None => {}
            }
        }
        p.disabled = configured && keys.is_empty();
        p.keys = keys;
        if p.base_url.is_empty() {
            p.base_url = match p.wire {
                Wire::Openai => "https://api.openai.com/v1",
                Wire::Anthropic => "https://api.anthropic.com",
            }
            .into();
        }
        p.base_url = p.base_url.trim_end_matches('/').to_string();
    }

    for (name, list) in &c.combos {
        if list.is_empty() {
            return Err(format!("combo '{name}' is empty"));
        }
        for t in list {
            if c.parse_target(t).is_none() {
                return Err(format!(
                    "combo '{name}': '{t}' is not provider/model with a configured provider"
                ));
            }
        }
    }
    for (pat, t) in &c.aliases {
        if !c.combos.contains_key(t)
            && c.parse_target(t).is_none()
            && crate::route::parse(t).is_none()
        {
            return Err(format!(
                "alias '{pat}': '{t}' is not a combo, provider/model, or smart route"
            ));
        }
    }
    if let Some(d) = &c.default {
        if !c.combos.contains_key(d)
            && c.parse_target(d).is_none()
            && crate::route::parse(d).is_none()
        {
            return Err(format!(
                "default '{d}' is not a combo, provider/model, or smart route"
            ));
        }
    }
    c.warnings = warnings;
    Ok(c)
}

pub fn edit() -> Result<(), String> {
    open_editor()?;
    check()
}

/// Open the config in $VISUAL / $EDITOR / nano and wait for it to close.
pub fn open_editor() -> Result<(), String> {
    let p = ensure()?;
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".into());
    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("nano");
    std::process::Command::new(bin)
        .args(parts)
        .arg(&p)
        .status()
        .map_err(|e| format!("cannot run editor '{editor}': {e} (set $EDITOR)"))?;
    Ok(())
}

pub fn check() -> Result<(), String> {
    let p = ensure()?;
    let c = load(&p)?;
    println!(
        "{} config ok: {} providers, {} combos ({})",
        crate::util::green("✓"),
        c.providers.len(),
        c.combos.len(),
        p.display()
    );
    println!("  changes apply on the next request; `listen` needs `tako restart`");
    Ok(())
}

const TEMPLATE: &str = r#"# tako config. Changes apply automatically on the next request,
# except `listen`, which needs `tako restart`.

listen = "127.0.0.1:20129"

# If set, clients must send this key (Authorization: Bearer ... or x-api-key).
# api_key = "change-me"

# Seconds to skip a key/model after it fails (rate limit, 5xx).
# A provider's retry-after / rate-limit-reset hint wins when it sends one.
cooldown = 60

# Seconds to skip a key that is out of credits, used up its daily quota,
# was rejected (401/403), or a model that doesn't exist. Editing the config
# clears credit/auth cooldowns, so a topped-up or fixed key is retried at once.
long_cooldown = 1800

# Combo used when a client asks for a model tako doesn't know
# (e.g. Claude Code asking for "claude-sonnet-..."). The TUI sets this for you.
# default = "free"

# ---- providers ---------------------------------------------------------
# type = "openai"     any OpenAI-compatible API: OpenAI, OpenRouter, Gemini,
#                     Groq, DeepSeek, Mistral, Together, Ollama, LM Studio, ...
# type = "anthropic"  the Anthropic Messages API
#
# keys: one or more; tako rotates through them and skips ones that fail.
#       "env:NAME" reads an environment variable instead (note: autostart
#       services don't see your shell's env, so plain keys are simpler).

# [providers.anthropic]
# type = "anthropic"
# keys = ["sk-ant-..."]

# [providers.openrouter]
# type = "openai"
# base_url = "https://openrouter.ai/api/v1"
# keys = ["sk-or-..."]

# [providers.gemini]
# type = "openai"
# base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
# keys = ["AIza..."]

# [providers.ollama]
# type = "openai"
# base_url = "http://localhost:11434/v1"

# ---- combos ------------------------------------------------------------
# A combo is a model name that tries each target in order until one works.

# [combos]
# smart = ["anthropic/claude-sonnet-5-5", "openrouter/anthropic/claude-sonnet-5.5"]
# fast  = ["gemini/gemini-2.5-flash", "ollama/qwen3:8b"]

# ---- aliases -----------------------------------------------------------
# Rewrite the model a client asks for. A trailing * matches anything.

# [aliases]
# "claude-*" = "smart"
"#;
