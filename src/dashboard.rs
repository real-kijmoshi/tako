//! Dashboard operations share the TUI's provider clients and validated config editor.
use crate::{catalog, config::Config, daemon, edit, tui::net};
use serde_json::{json, Value};
use std::{
    io::{Read, Seek, SeekFrom},
    sync::Mutex,
};

static EDIT: Mutex<()> = Mutex::new(());

pub fn catalog() -> Value {
    json!(catalog::ALL
        .iter()
        .map(|e| json!({
            "id": e.id, "name": e.name, "tier": e.tier.label(), "type": e.wire.name(),
            "base_url": e.base_url, "key_url": e.key_url, "blurb": e.blurb,
            "limits": e.limits, "max_tokens": e.max_tokens,
        }))
        .collect::<Vec<_>>())
}

pub fn logs(cfg: &Config) -> Result<Value, String> {
    let mut f = match std::fs::File::open(daemon::log_file()) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"lines": []})),
        Err(e) => return Err(e.to_string()),
    };
    let start = f
        .metadata()
        .map_err(|e| e.to_string())?
        .len()
        .saturating_sub(128 * 1024);
    f.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    f.take(128 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    for key in cfg
        .providers
        .values()
        .flat_map(|p| &p.keys)
        .chain(cfg.api_key.iter())
    {
        if !key.is_empty() {
            text = text.replace(key, "[redacted]");
        }
    }
    let lines: Vec<_> = text.lines().skip(usize::from(start > 0)).collect();
    Ok(json!({"lines": &lines[lines.len().saturating_sub(1000)..]}))
}

fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

#[cfg(test)]
pub async fn action(cfg: &Config, v: Value) -> Result<Value, String> {
    action_with_token(cfg, v, cfg.api_key.clone().unwrap_or_else(|| "tako".into())).await
}
pub async fn action_with_token(
    cfg: &Config,
    v: Value,
    router_token: String,
) -> Result<Value, String> {
    let action = string(&v, "action")?;
    if matches!(action, "models" | "test") {
        let id = string(&v, "provider")?;
        let p = cfg.providers.get(id).ok_or("Provider no longer exists")?;
        if p.disabled {
            return Err(
                "Provider key is unavailable. Add a key or set its environment variable.".into(),
            );
        }
        let target = net::Target::from(id, p);
        if action == "test" {
            return net::test(target, v["model"].as_str().map(String::from))
                .await
                .map(|message| json!({"message": message}));
        }
        let models = net::models(target).await?;
        return Ok(
            json!({"models": models.iter().map(|(id_model, priced)| json!({
            "id": id_model, "target": format!("{id}/{id_model}"),
            "free": catalog::get(id).is_some_and(|e| e.is_free(id_model, *priced)),
        })).collect::<Vec<_>>()}),
        );
    }
    if action == "chat" {
        let model = string(&v, "model")?;
        cfg.resolve(model)?;
        let messages = v["messages"]
            .as_array()
            .filter(|m| !m.is_empty() && m.len() <= 200)
            .ok_or("Supply 1–200 messages")?;
        if messages.iter().any(|m| {
            !matches!(m["role"].as_str(), Some("user" | "assistant"))
                || m["content"].as_str().is_none()
        }) {
            return Err(
                "Chat messages must contain a user or assistant role and text content".into(),
            );
        }
        return net::chat(
            cfg.listen.clone(),
            router_token,
            model.into(),
            messages.clone(),
        )
        .await
        .map(|reply| json!({"reply": reply}));
    }
    // Keep read/modify/write operations from concurrent browser requests serialized.
    let _guard = EDIT.lock().map_err(|_| "Config editor unavailable")?;
    match action {
        "provider.add" => {
            let id = string(&v, "provider")?;
            if id.contains('/') || id.len() > 100 {
                return Err("Use a provider name of up to 100 characters without slashes".into());
            }
            let entry = catalog::get(id);
            let wire = v["type"]
                .as_str()
                .or(entry.map(|e| e.wire.name()))
                .unwrap_or("openai");
            if !matches!(wire, "openai" | "anthropic") {
                return Err("Unsupported API format".into());
            }
            let base = v["base_url"]
                .as_str()
                .or(entry.map(|e| e.base_url))
                .ok_or("Base URL is required")?;
            let cloudflare;
            let base = if id == "cloudflare" && base.contains("{account_id}") {
                cloudflare = catalog::cloudflare_url(string(&v, "account_id")?)?;
                cloudflare.as_str()
            } else {
                base
            };
            let url = reqwest::Url::parse(base).map_err(|_| "Invalid base URL")?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err("Base URL must use http or https".into());
            }
            let key = v["key"].as_str().map(str::trim).filter(|k| !k.is_empty());
            if entry.is_some_and(|e| e.needs_key())
                && key.is_none()
                && !cfg.providers.contains_key(id)
            {
                return Err("An API key is required for this provider".into());
            }
            edit::add_provider(
                id,
                wire,
                base.trim_end_matches('/'),
                key,
                v["max_tokens"]
                    .as_u64()
                    .or(entry.and_then(|e| e.max_tokens)),
            )?;
        }
        "provider.key" => edit::add_key(string(&v, "provider")?, string(&v, "key")?.trim())?,
        "provider.remove" => edit::remove_provider(string(&v, "provider")?)?,
        "combo.save" => {
            let name = string(&v, "name")?;
            if name.contains('/') {
                return Err("Combo names cannot contain slashes".into());
            }
            let targets: Vec<String> = v["targets"]
                .as_array()
                .ok_or("Targets must be a list")?
                .iter()
                .map(|t| {
                    t.as_str()
                        .map(String::from)
                        .ok_or_else(|| "Targets must be text".to_string())
                })
                .collect::<Result<_, _>>()?;
            if targets.is_empty() {
                return Err("Pick at least one model".into());
            }
            edit::set_combo(name, &targets)?;
        }
        "combo.delete" => edit::delete_combo(string(&v, "name")?)?,
        "combo.default" => edit::set_default(string(&v, "name")?)?,
        _ => return Err("Unknown dashboard action".into()),
    }
    Ok(json!({"message": "Changes saved. Routing updated."}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_contains_metadata_without_credentials() {
        let items = catalog();
        assert!(items.as_array().unwrap().len() > 5);
        for e in items.as_array().unwrap() {
            assert!(e["id"].is_string());
            assert!(e["base_url"].as_str().unwrap().starts_with("http"));
            assert!(e.get("key").is_none());
        }
    }
    #[test]
    fn rejects_invalid_chat_and_unknown_operations() {
        let cfg: Config = toml::from_str("").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(rt
            .block_on(action(
                &cfg,
                json!({"action": "chat", "model": "missing", "messages": []})
            ))
            .is_err());
        assert!(rt
            .block_on(action(
                &cfg,
                json!({"action": "provider.add", "provider": "bad/name"})
            ))
            .is_err());
        assert!(rt
            .block_on(action(&cfg, json!({"action": "unknown"})))
            .is_err());
    }
}
