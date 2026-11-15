//! Portable provider/routing settings. Machine authentication stays on its host.
use crate::{config, util::e2s};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use toml_edit::DocumentMut;

const FIELDS: &[&str] = &[
    "providers",
    "combos",
    "aliases",
    "default",
    "cooldown",
    "long_cooldown",
];
pub const MAX_BYTES: usize = 256 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    format: String,
    version: u32,
    settings: String,
}
fn invalid() -> String {
    "Invalid settings. Check provider definitions, combo targets, and the default route. Destination unchanged.".into()
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
    if let Err(e) = f.write_all(bytes).and_then(|_| f.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(e2s(e));
    }
    Ok(())
}
fn read_current() -> Result<String, String> {
    match fs::read_to_string(config::path()) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e2s(e)),
    }
}
fn export_text(raw: &str) -> Result<Bundle, String> {
    config::parse(raw).map_err(|_| invalid())?;
    let mut table: toml::Table = toml::from_str(raw).map_err(|_| invalid())?;
    table.retain(|k, _| FIELDS.contains(&k));
    let bundle = Bundle {
        format: "tako-settings".into(),
        version: 1,
        settings: toml::to_string_pretty(&table).map_err(|_| invalid())?,
    };
    if serde_json::to_vec_pretty(&bundle).map_err(e2s)?.len() > MAX_BYTES {
        return Err("Settings export exceeds the 256 KB limit".into());
    }
    Ok(bundle)
}
pub fn export() -> Result<Value, String> {
    let bundle = export_text(&fs::read_to_string(config::ensure()?).map_err(e2s)?)?;
    Ok(json!({"bundle": bundle, "contains_secrets": true}))
}
fn decode(value: Value) -> Result<Bundle, String> {
    if serde_json::to_vec(&value).map_err(e2s)?.len() > MAX_BYTES {
        return Err("Settings file exceeds the 256 KB limit".into());
    }
    let bundle: Bundle = serde_json::from_value(value).map_err(|_| "Not a Tako settings export")?;
    if bundle.format != "tako-settings" || bundle.version != 1 {
        return Err("Unsupported Tako settings format or version".into());
    }
    Ok(bundle)
}
fn merge(bundle: &Bundle, current: &str) -> Result<(String, Value), String> {
    let incoming: toml::Table = toml::from_str(&bundle.settings).map_err(|_| invalid())?;
    if incoming.keys().any(|k| !FIELDS.contains(&k.as_str())) {
        return Err(
            "Export contains machine-specific or unknown settings. Destination unchanged.".into(),
        );
    }
    let source = bundle
        .settings
        .parse::<DocumentMut>()
        .map_err(|_| invalid())?;
    let mut destination = current
        .parse::<DocumentMut>()
        .map_err(|_| "Current configuration cannot be read. Destination unchanged.")?;
    for field in FIELDS {
        destination.remove(field);
        if let Some(item) = source.get(field) {
            destination.insert(field, item.clone());
        }
    }
    let text = destination.to_string();
    let cfg = config::parse(&text).map_err(|_| invalid())?;
    let summary = json!({"providers": cfg.providers.len(), "combos": cfg.combos.len(), "aliases": cfg.aliases.len(), "warnings": cfg.warnings});
    Ok((text, summary))
}
pub fn preview(value: Value) -> Result<Value, String> {
    let bundle = decode(value)?;
    let (_, summary) = merge(&bundle, &read_current()?)?;
    Ok(summary)
}
pub fn import(value: Value) -> Result<Value, String> {
    let bundle = decode(value)?;
    let current = read_current()?;
    let (text, mut summary) = merge(&bundle, &current)?;
    let path = config::ensure()?;
    // Back up exactly the destination state being replaced, including its local settings.
    let previous = fs::read_to_string(&path).map_err(e2s)?;
    if !current.is_empty() && previous != current {
        return Err("Configuration changed during import. Try again.".into());
    }
    let id = crate::auth::secret();
    let backup = config::dir().join(format!("config.before-import-{}.toml", &id[..16]));
    let temp = config::dir().join(format!(".config-import-{}.tmp", &id[..16]));
    write_private(&backup, previous.as_bytes())?;
    write_private(&temp, text.as_bytes())?;
    if let Err(e) = fs::rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("Import failed; original configuration kept: {e}"));
    }
    summary["backup"] = json!(backup);
    summary["message"] =
        json!("Settings imported. A backup of the previous configuration was saved.");
    Ok(summary)
}
pub fn command(importing: bool, args: &[String]) -> Result<(), String> {
    let usage = "usage: tako export FILE.json | tako import FILE.json";
    if args.len() != 1 {
        return Err(usage.into());
    }
    let path = PathBuf::from(&args[0]);
    if importing {
        let file =
            fs::File::open(&path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(e2s)?;
        if bytes.len() > MAX_BYTES {
            return Err("Settings file exceeds the 256 KB limit".into());
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| "Not a valid JSON settings file")?;
        let result = import(value)?;
        println!(
            "Imported {} providers and {} combos. Backup: {}",
            result["providers"],
            result["combos"],
            result["backup"].as_str().unwrap_or("")
        );
        for warning in result["warnings"].as_array().into_iter().flatten() {
            println!("Warning: {}", warning.as_str().unwrap_or(""));
        }
    } else {
        let value = export()?;
        write_private(
            &path,
            &serde_json::to_vec_pretty(&value["bundle"]).map_err(e2s)?,
        )?;
        println!(
            "Exported settings to {}. Contains provider API keys: keep this file private.",
            path.display()
        );
        println!("Environment-variable references are preserved; set those variables on the destination.");
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_preserves_routes_and_host_security_without_resolving_secrets() {
        let raw = "listen = '0.0.0.0:9000'\napi_key = 'HOST-SECRET' # PRIVATE-COMMENT\ndefault = 'daily'\n[providers.remote]\ntype = 'openai'\nkeys = ['env:TAKO_TEST_MISSING', 'PROVIDER-SECRET']\n[combos]\ndaily = ['remote/model']\n";
        let bundle = export_text(raw).unwrap();
        assert!(!bundle.settings.contains("HOST-SECRET"));
        assert!(!bundle.settings.contains("PRIVATE-COMMENT"));
        assert!(bundle.settings.contains("env:TAKO_TEST_MISSING"));
        let (text, summary) = merge(&bundle, "listen = '127.0.0.1:8765'\napi_key = 'DEST-SECRET'\n[providers.old]\ntype = 'anthropic'\n").unwrap();
        let c = config::parse(&text).unwrap();
        assert_eq!(c.listen, "127.0.0.1:8765");
        assert_eq!(c.api_key.as_deref(), Some("DEST-SECRET"));
        assert_eq!(summary["providers"], 1);
        assert!(!c.providers.contains_key("old"));
        assert_eq!(c.default.as_deref(), Some("daily"));
        assert_eq!(c.providers["remote"].keys, ["PROVIDER-SECRET"]);
    }
    #[test]
    fn rejects_host_overrides_unknown_versions_and_invalid_routes() {
        let b = Bundle {
            format: "tako-settings".into(),
            version: 1,
            settings: "listen = '0.0.0.0:80'".into(),
        };
        assert!(merge(&b, "").is_err());
        assert!(decode(json!({"format":"tako-settings","version":2,"settings":""})).is_err());
        let b = Bundle {
            settings: "[combos]\nbad=['missing/model']".into(),
            ..b
        };
        assert!(merge(&b, "").is_err());
        let b = Bundle {
            settings: "[providers.bad]\ntype='openai'\nkeys='DO-NOT-EXPOSE'".into(),
            ..b
        };
        assert!(!merge(&b, "").unwrap_err().contains("DO-NOT-EXPOSE"));
    }
}
