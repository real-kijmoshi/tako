//! Programmatic config edits that keep the user's comments and layout.
//! Every write is validated before it replaces the real file.

use crate::config;
use crate::util::e2s;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use toml_edit::{value, Array, DocumentMut, Item, Table};

fn load() -> Result<DocumentMut, String> {
    let p = config::ensure()?;
    fs::read_to_string(&p)
        .map_err(e2s)?
        .parse::<DocumentMut>()
        .map_err(|e| format!("config: {e}"))
}

fn save(doc: &DocumentMut) -> Result<(), String> {
    let p = config::path();
    let tmp = p.with_extension("toml.tmp");
    fs::write(&tmp, doc.to_string()).map_err(e2s)?;
    let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    if let Err(e) = config::load(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(e.replace(&tmp.display().to_string(), "config"));
    }
    fs::rename(&tmp, &p).map_err(e2s)
}

fn table<'a>(doc: &'a mut DocumentMut, key: &str, implicit: bool) -> Result<&'a mut Table, String> {
    if !doc.contains_key(key) {
        let mut t = Table::new();
        t.set_implicit(implicit);
        doc.insert(key, Item::Table(t));
    }
    doc[key]
        .as_table_mut()
        .ok_or_else(|| format!("`{key}` in the config is not a table"))
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn add_provider(
    id: &str,
    wire: &str,
    base_url: &str,
    key: Option<&str>,
    max_tokens: Option<u64>,
) -> Result<(), String> {
    let mut doc = load()?;
    if doc.get("providers").and_then(|p| p.get(id)).is_some() {
        return match key {
            Some(k) => add_key(id, k),
            None => Ok(()),
        };
    }
    let mut t = Table::new();
    t["type"] = value(wire);
    t["base_url"] = value(base_url);
    if let Some(k) = key {
        t["keys"] = value(Array::from_iter([k]));
    }
    if let Some(m) = max_tokens {
        t["max_tokens"] = value(m as i64);
    }
    table(&mut doc, "providers", true)?.insert(id, Item::Table(t));
    save(&doc)
}

pub fn add_key(id: &str, key: &str) -> Result<(), String> {
    let mut doc = load()?;
    let p = doc
        .get_mut("providers")
        .and_then(|p| p.get_mut(id))
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("no provider '{id}'"))?;
    let mut keys = strings(p.get("keys"));
    if let Some(k) = p.get("key").and_then(Item::as_str) {
        keys.insert(0, k.to_string());
    }
    if keys.iter().any(|k| k == key) {
        return Err("that key is already added".into());
    }
    keys.push(key.to_string());
    p.remove("key");
    p["keys"] = value(Array::from_iter(keys));
    save(&doc)
}

/// Remove a provider and everything that points at it.
pub fn remove_provider(id: &str) -> Result<(), String> {
    let mut doc = load()?;
    if let Some(p) = doc.get_mut("providers").and_then(Item::as_table_mut) {
        p.remove(id);
    }
    let prefix = format!("{id}/");
    let mut gone: Vec<String> = Vec::new();
    if let Some(c) = doc.get_mut("combos").and_then(Item::as_table_mut) {
        for (name, item) in c.iter_mut() {
            if let Some(a) = item.as_array_mut() {
                a.retain(|v| !v.as_str().is_some_and(|s| s.starts_with(&prefix)));
                if a.is_empty() {
                    gone.push(name.get().to_string());
                }
            }
        }
        for n in &gone {
            c.remove(n);
        }
    }
    let dead = |s: &str| s.starts_with(&prefix) || gone.iter().any(|g| g == s);
    if let Some(a) = doc.get_mut("aliases").and_then(Item::as_table_mut) {
        a.retain(|_, v| !v.as_str().is_some_and(dead));
    }
    if doc.get("default").and_then(Item::as_str).is_some_and(dead) {
        doc.remove("default");
    }
    save(&doc)
}

/// Append targets to a combo (creating it). Returns how many were new.
pub fn add_to_combo(combo: &str, targets: &[String]) -> Result<usize, String> {
    let mut doc = load()?;
    let c = table(&mut doc, "combos", false)?;
    let mut list = strings(c.get(combo));
    let before = list.len();
    for t in targets {
        if !list.contains(t) {
            list.push(t.clone());
        }
    }
    let added = list.len() - before;
    c[combo] = value(Array::from_iter(list));
    save(&doc)?;
    Ok(added)
}

/// Replace a combo's targets; an empty list deletes the combo.
pub fn set_combo(combo: &str, targets: &[String]) -> Result<(), String> {
    if targets.is_empty() {
        return delete_combo(combo);
    }
    let mut doc = load()?;
    table(&mut doc, "combos", false)?[combo] = value(Array::from_iter(targets.iter().cloned()));
    save(&doc)
}

pub fn delete_combo(combo: &str) -> Result<(), String> {
    let mut doc = load()?;
    if let Some(c) = doc.get_mut("combos").and_then(Item::as_table_mut) {
        c.remove(combo);
    }
    if let Some(a) = doc.get_mut("aliases").and_then(Item::as_table_mut) {
        a.retain(|_, v| v.as_str() != Some(combo));
    }
    if doc.get("default").and_then(Item::as_str) == Some(combo) {
        doc.remove("default");
    }
    save(&doc)
}

pub fn set_default(name: &str) -> Result<(), String> {
    let mut doc = load()?;
    doc["default"] = value(name);
    save(&doc)
}

pub fn set_listen(address: &str) -> Result<(), String> {
    address
        .parse::<std::net::SocketAddr>()
        .map_err(|_| "Use an IP:port listen address")?;
    let mut doc = load()?;
    doc["listen"] = value(address);
    save(&doc)?;
    println!("Listening address saved: {address}. Run tako restart (or tako run). Use HTTPS through a reverse proxy for internet access.");
    Ok(())
}
