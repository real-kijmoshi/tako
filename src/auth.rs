//! Admin credentials and client keys are separate from upstream provider credentials.
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::{fs::OpenOptionsExt, io::AsRawFd},
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub password: Option<String>,
    #[serde(default)]
    api_auth: bool,
    #[serde(default)]
    keys: Vec<ClientKey>,
}
#[derive(Serialize, Deserialize)]
struct ClientKey {
    id: String,
    name: String,
    hash: String,
    created: u64,
    #[serde(default)]
    prefix: String,
}
struct Session {
    hash: String,
    password: String,
    expires: Instant,
}
#[derive(Default)]
pub struct Auth {
    sessions: Mutex<Vec<Session>>,
    attempts: Mutex<Vec<Instant>>,
}
fn writer() -> Result<fs::File, String> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(crate::config::dir().join("auth.lock"))
        .map_err(|e| e.to_string())?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(file)
}

pub fn secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn digest(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn load() -> Result<Credentials, String> {
    match fs::read(crate::config::dir().join("auth.json")) {
        Ok(b) => serde_json::from_slice(&b)
            .map_err(|_| "Invalid auth.json; repair it before starting tako".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Credentials::default()),
        Err(e) => Err(format!("Cannot read auth.json: {e}")),
    }
}
fn save(c: &Credentials) -> Result<(), String> {
    let path = crate::config::dir().join("auth.json");
    let tmp = path.with_extension("json.tmp");
    let mut f = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(&serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}
impl Credentials {
    pub fn protected(&self) -> bool {
        self.password.is_some()
    }
    pub fn api_required(&self, cfg: &crate::config::Config) -> bool {
        self.api_auth || self.protected() || !self.keys.is_empty() || cfg.api_key.is_some()
    }
    pub fn accepts(&self, token: &str) -> bool {
        let hash = digest(token);
        self.keys.iter().any(|k| k.hash == hash)
    }
    pub fn summary(&self) -> Value {
        json!({"password_set": self.protected(), "keys": self.keys.iter().map(|k| json!({"id": k.id, "name": k.name, "created": k.created, "prefix": k.prefix})).collect::<Vec<_>>()})
    }
}
impl Auth {
    pub fn session(&self, c: &Credentials, token: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|s| s.expires > Instant::now() && Some(&s.password) == c.password.as_ref());
        sessions.iter().any(|s| s.hash == digest(token))
    }
    pub async fn login(&self, password: String) -> Result<String, String> {
        {
            let mut attempts = self.attempts.lock().unwrap();
            attempts.retain(|t| t.elapsed() < Duration::from_secs(60));
            if attempts.len() >= 5 {
                return Err("Too many login attempts. Wait one minute.".into());
            }
            attempts.push(Instant::now());
        }
        let hash = load()?
            .password
            .ok_or("Set a password in the terminal with tako password")?;
        let check = hash.clone();
        let valid = tokio::task::spawn_blocking(move || {
            PasswordHash::new(&check).is_ok_and(|h| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &h)
                    .is_ok()
            })
        })
        .await
        .map_err(|e| e.to_string())?;
        if !valid {
            return Err("Invalid password".into());
        }
        let token = secret();
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|s| s.expires > Instant::now());
        if sessions.len() >= 100 {
            sessions.remove(0);
        }
        sessions.push(Session {
            hash: digest(&token),
            password: hash,
            expires: Instant::now() + Duration::from_secs(43200),
        });
        Ok(token)
    }
    pub fn logout(&self, token: &str) {
        self.sessions
            .lock()
            .unwrap()
            .retain(|s| s.hash != digest(token));
    }
}
pub fn action(v: &Value) -> Result<Value, String> {
    let _guard = writer()?;
    let mut c = load()?;
    match v["action"].as_str() {
        Some("key.create") => {
            let name = v["name"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.len() <= 100)
                .ok_or("Name must contain 1–100 characters")?;
            if c.keys.len() >= 100 {
                return Err("Limit of 100 API keys reached".into());
            }
            c.api_auth = true;
            let token = format!("tako_{}", secret());
            c.keys.push(ClientKey {
                id: secret()[..16].into(),
                name: name.into(),
                prefix: format!("{}…", &token[..14]),
                hash: digest(&token),
                created: crate::util::now_secs(),
            });
            save(&c)?;
            Ok(
                json!({"key": token, "id": c.keys.last().map(|k| &k.id), "message": "Key created. Copy it now; it is shown only once."}),
            )
        }
        Some("key.rename") => {
            let id = v["id"].as_str().ok_or("Key ID required")?;
            let name = v["name"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.len() <= 100)
                .ok_or("Name must contain 1–100 characters")?;
            let key = c
                .keys
                .iter_mut()
                .find(|k| k.id == id)
                .ok_or("This key no longer exists. Refresh and try again.")?;
            key.name = name.into();
            save(&c)?;
            Ok(json!({"message": "Key renamed"}))
        }
        Some("key.revoke") => {
            let id = v["id"].as_str().ok_or("Key ID required")?;
            c.api_auth |= !c.keys.is_empty();
            c.keys.retain(|k| k.id != id);
            save(&c)?;
            Ok(json!({"message": "Key revoked"}))
        }
        _ => Err("Unknown security action".into()),
    }
}
pub fn password() -> Result<(), String> {
    if unsafe { libc::isatty(0) } != 1 {
        return Err("Password setup needs an interactive terminal. Run `tako password` directly (for SSH, use `ssh -t`).".into());
    }
    crate::config::ensure()?;
    let password = rpassword::prompt_password("New admin password (at least 12 characters): ")
        .map_err(|e| e.to_string())?;
    if password.chars().count() < 12 {
        return Err("Use at least 12 characters".into());
    }
    let confirm = rpassword::prompt_password("Confirm password: ").map_err(|e| e.to_string())?;
    if password != confirm {
        return Err("Passwords do not match".into());
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| e.to_string())?
        .to_string();
    let _guard = writer()?;
    let mut c = load()?;
    c.password = Some(hash);
    save(&c)?;
    println!("Admin password saved. Existing browser sessions are invalidated. Sign in at /ui.");
    Ok(())
}

/// Local terminal clients use a per-process credential kept in a private file.
pub fn local_token() -> Option<String> {
    fs::read_to_string(crate::config::dir().join("local.token"))
        .ok()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub fn write_local_token(token: &str) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(crate::config::dir().join("local.token"))
        .map_err(|e| e.to_string())?;
    f.write_all(token.as_bytes()).map_err(|e| e.to_string())
}
