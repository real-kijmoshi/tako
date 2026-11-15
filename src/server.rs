use crate::config::{self, Config, Provider, Wire};
use crate::failure::{self, Kind, Policy, Scope};
use crate::stream::{AnthToOai, OaiToAnth, Sse, Translate};
use crate::util::{chain, short};
use crate::{daemon, log, translate as tr};
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::header::HeaderMap;
use hyper::{server::conn::http1, service::service_fn, Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::convert::Infallible;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_BODY: usize = 64 << 20;

type Body = UnsyncBoxBody<Bytes, io::Error>;

struct App {
    path: PathBuf,
    cfg: Mutex<(Arc<Config>, Option<SystemTime>)>,
    http: reqwest::Client,
    cooling: Mutex<HashMap<String, Cool>>,
    rr: AtomicUsize,
    seq: AtomicU64,
    metrics: Mutex<Metrics>,
    started: Instant,
    auth: crate::auth::Auth,
    internal_key: String,
    require_password: bool,
}

#[derive(Clone, Copy)]
struct Cool {
    until: Instant,
    total: u64,
    kind: Kind,
}

#[derive(Default)]
struct Metrics {
    tokens: crate::usage::Tokens,
    token_targets: BTreeMap<String, crate::usage::Tokens>,
    token_limits: BTreeMap<String, Value>,
    /// per target: ok, fail, total ms of successful requests
    targets: BTreeMap<String, [u64; 3]>,
    /// (unix minute, ok, fail), last 60 minutes
    minutes: VecDeque<(u64, u64, u64)>,
    recent: VecDeque<Value>,
    /// client requests: total, served first try, rescued by fallback, failed everywhere
    requests: [u64; 4],
    /// failures by kind
    kinds: BTreeMap<&'static str, u64>,
    /// per target: the most recent failure
    last_error: BTreeMap<String, Value>,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

struct Attempt {
    who: String,
    kind: Kind,
    status: u16,
    wait: u64,
}

impl App {
    /// Current config, reloaded if the file changed since last time.
    fn cfg(&self) -> Arc<Config> {
        let mut g = self.cfg.lock().unwrap();
        let m = mtime(&self.path);
        if m != g.1 {
            g.1 = m;
            match config::load(&self.path) {
                Ok(c) => {
                    log!("config reloaded");
                    c.warnings.iter().for_each(|w| log!("warning: {w}"));
                    g.0 = Arc::new(c);
                    // a new key or a top-up usually comes with a config edit: give those another chance
                    self.cooling
                        .lock()
                        .unwrap()
                        .retain(|_, c| !matches!(c.kind, Kind::Billing | Kind::Auth));
                }
                Err(e) => log!("config error, keeping previous config: {e}"),
            }
        }
        g.0.clone()
    }

    fn cooling(&self, k: &str) -> Option<Cool> {
        let mut m = self.cooling.lock().unwrap();
        match m.get(k) {
            Some(c) if c.until > Instant::now() => Some(*c),
            Some(_) => {
                m.remove(k);
                None
            }
            None => None,
        }
    }

    fn cool(&self, k: &str, secs: u64, kind: Kind) {
        let c = Cool {
            until: Instant::now() + Duration::from_secs(secs),
            total: secs,
            kind,
        };
        self.cooling.lock().unwrap().insert(k.to_string(), c);
    }

    fn clear_cooldowns(&self, key: Option<&str>) -> usize {
        let mut m = self.cooling.lock().unwrap();
        let n = m.len();
        match key {
            Some(k) => m.retain(|c, _| c != k && !c.starts_with(&format!("{k} (key "))),
            None => m.clear(),
        }
        n - m.len()
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        req: u64,
        model: &str,
        tag: &str,
        shown: &str,
        status: u16,
        ms: u64,
        kind: Option<Kind>,
        msg: &str,
    ) {
        let ok = kind.is_none();
        let mut m = self.metrics.lock().unwrap();
        let t = m.targets.entry(tag.to_string()).or_default();
        if ok {
            t[0] += 1;
            t[2] += ms;
        } else {
            t[1] += 1;
        }
        let now = crate::util::now_secs();
        if let Some(k) = kind {
            *m.kinds.entry(k.id()).or_default() += 1;
            let readable = serde_json::from_str::<Value>(msg)
                .ok()
                .and_then(|v| {
                    v["error"]["message"]
                        .as_str()
                        .or(v["message"].as_str())
                        .or(v["error"].as_str())
                        .map(String::from)
                })
                .unwrap_or_else(|| msg.to_string());
            m.last_error.insert(tag.to_string(), json!({"t": now, "kind": k.id(), "label": k.label(), "status": status, "msg": short(&readable, 300)}));
        }
        let minute = now / 60;
        match m.minutes.back_mut() {
            Some(b) if b.0 == minute => {
                if ok {
                    b.1 += 1
                } else {
                    b.2 += 1
                }
            }
            _ => {
                m.minutes.push_back((minute, ok as u64, !ok as u64));
                if m.minutes.len() > 60 {
                    m.minutes.pop_front();
                }
            }
        }
        let mut e = json!({"t": now, "req": req, "model": model, "target": shown, "status": status, "ms": ms});
        if let Some(k) = kind {
            e["kind"] = k.id().into();
            e["label"] = k.label().into();
        }
        m.recent.push_back(e);
        if m.recent.len() > 80 {
            m.recent.pop_front();
        }
    }

    /// A request that never reached a provider because every target was benched.
    fn record_benched(&self, req: u64, model: &str, kind: Kind) {
        let mut m = self.metrics.lock().unwrap();
        let e = json!({"t": crate::util::now_secs(), "req": req, "model": model, "target": "every target benched",
            "status": 0, "ms": 0, "kind": kind.id(), "label": kind.label()});
        m.recent.push_back(e);
        if m.recent.len() > 80 {
            m.recent.pop_front();
        }
    }

    /// outcome: 0 = first try, 1 = rescued by fallback, 2 = failed everywhere
    fn finish(&self, outcome: usize) {
        let mut m = self.metrics.lock().unwrap();
        m.requests[0] += 1;
        m.requests[outcome + 1] += 1;
    }

    fn record_tokens(&self, req: u64, tag: &str, tokens: crate::usage::Tokens) {
        let mut m = self.metrics.lock().unwrap();
        m.tokens.add(tokens);
        m.token_targets
            .entry(tag.to_string())
            .or_default()
            .add(tokens);
        if let Some(e) = m
            .recent
            .iter_mut()
            .rev()
            .find(|e| e["req"] == req && e.get("kind").is_none())
        {
            e["tokens"] = json!(tokens);
        }
    }

    fn record_limits(&self, key: &str, headers: &reqwest::header::HeaderMap) {
        let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let prefix = if get("anthropic-ratelimit-tokens-remaining").is_some() {
            "anthropic-ratelimit"
        } else {
            "x-ratelimit"
        };
        let remaining = get(&format!("{prefix}-remaining-tokens"))
            .or_else(|| get(&format!("{prefix}-tokens-remaining")))
            .and_then(|n| n.parse::<u64>().ok());
        if let Some(remaining) = remaining {
            let limit = get(&format!("{prefix}-limit-tokens"))
                .or_else(|| get(&format!("{prefix}-tokens-limit")))
                .and_then(|n| n.parse::<u64>().ok());
            let reset = get(&format!("{prefix}-reset-tokens"))
                .or_else(|| get(&format!("{prefix}-tokens-reset")));
            self.metrics.lock().unwrap().token_limits.insert(key.to_string(), json!({
                "remaining": remaining, "limit": limit, "reset": reset, "observed_at": crate::util::now_secs()
            }));
        }
    }

    fn status(&self) -> Value {
        let now = Instant::now();
        let live: Vec<(String, Cool)> = self
            .cooling
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c)| c.until > now)
            .map(|(k, c)| (k.clone(), *c))
            .collect();
        let left = |c: &Cool| (c.until - now).as_secs() + 1;
        let cooling: BTreeMap<&String, u64> = live.iter().map(|(k, c)| (k, left(c))).collect();
        let cooling_info: BTreeMap<&String, Value> = live
            .iter()
            .map(|(k, c)| (k, json!({"left": left(c), "total": c.total, "kind": c.kind.id(), "label": c.kind.label()})))
            .collect();
        let m = self.metrics.lock().unwrap();
        let stats: BTreeMap<&String, Value> = m
            .targets
            .iter()
            .map(|(k, [ok, fail, ms])| {
                (
                    k,
                    json!({"ok": ok, "fail": fail, "avg_ms": if *ok > 0 { ms / ok } else { 0 }}),
                )
            })
            .collect();
        let minutes: Vec<Value> = m
            .minutes
            .iter()
            .map(|(t, ok, fail)| json!([t, ok, fail]))
            .collect();
        let [total, first, rescued, failed] = m.requests;
        json!({
            "version": VERSION, "pid": std::process::id(), "uptime": self.started.elapsed().as_secs(),
            "rss": rss(), "stats": stats, "cooling": cooling, "cooling_info": cooling_info,
            "minutes": minutes, "recent": m.recent,
            "requests": {"total": total, "first_try": first, "rescued": rescued, "failed": failed},
            "kinds": m.kinds, "last_error": m.last_error,
            "tokens": m.tokens, "token_targets": m.token_targets, "token_limits": m.token_limits,
        })
    }
}

#[cfg(target_os = "macos")]
fn rss() -> u64 {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if n == size {
        info.pti_resident_size
    } else {
        0
    }
}

#[cfg(not(target_os = "macos"))]
fn rss() -> u64 {
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map_or(0, |p| p * page)
}

/// `tako run`: serve in the foreground until SIGINT/SIGTERM.
pub fn run() -> Result<(), String> {
    if let Some(pid) = daemon::running() {
        if pid as u32 != std::process::id() {
            // exit 0 so service managers don't restart-loop us
            println!("tako is already running (pid {pid})");
            return Ok(());
        }
    }
    let path = config::ensure()?;
    let cfg = config::load(&path)?;
    let credentials = crate::auth::load()?;
    let address: std::net::SocketAddr = cfg
        .listen
        .parse()
        .map_err(|_| "listen must be an IP:port address")?;
    if !address.ip().is_loopback() && !credentials.protected() {
        return Err("Public listeners require an admin password. Run tako setup-vps or tako password first.".into());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    daemon::write_pid();
    let r = rt.block_on(serve(path, cfg));
    daemon::remove_pid();
    r
}

async fn serve(path: PathBuf, cfg: Config) -> Result<(), String> {
    let listener = TcpListener::bind(&cfg.listen)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", cfg.listen))?;
    log!("tako {VERSION} listening on http://{}", cfg.listen);
    cfg.warnings.iter().for_each(|w| log!("warning: {w}"));
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(300))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(4)
        .build()
        .map_err(|e| e.to_string())?;
    let require_password = !listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .ip()
        .is_loopback();
    let internal_key = crate::auth::secret();
    crate::auth::write_local_token(&internal_key)?;
    let app = Arc::new(App {
        cfg: Mutex::new((Arc::new(cfg), mtime(&path))),
        path,
        http,
        cooling: Mutex::default(),
        rr: AtomicUsize::new(0),
        seq: AtomicU64::new(1),
        metrics: Mutex::default(),
        started: Instant::now(),
        auth: crate::auth::Auth::default(),
        internal_key,
        require_password,
    });

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => { log!("shutting down"); return Ok(()) }
            r = listener.accept() => {
                let Ok((sock, peer)) = r else {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };
                let _ = sock.set_nodelay(true);
                let app = app.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req| {
                        let app = app.clone();
                        async move { Ok::<_, Infallible>(handle(app, req, peer.ip().is_loopback()).await) }
                    });
                    let _ = http1::Builder::new().serve_connection(TokioIo::new(sock), svc).await;
                });
            }
        }
    }
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("signal handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

// ---- routing ----------------------------------------------------------

const UI_HTML: &str = include_str!("web/index.html");

async fn handle(app: Arc<App>, req: Request<Incoming>, loopback: bool) -> Response<Body> {
    let path = req.uri().path().to_string();
    let method = req.method().as_str().to_string();
    let wire = if path.contains("/messages") {
        Wire::Anthropic
    } else {
        Wire::Openai
    };
    let browser = req
        .headers()
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.contains("text/html"));
    let credentials = match crate::auth::load() {
        Ok(c) => c,
        Err(_) => {
            return error(
                wire,
                503,
                "authentication_error",
                "Credential store unavailable",
            )
        }
    };
    if app.require_password && !credentials.protected() {
        return error(
            wire,
            503,
            "authentication_error",
            "Public listener requires an admin password",
        );
    }
    let session = cookie(req.headers());
    let admin = app.auth.session(&credentials, &session);
    let local_admin = loopback
        && req
            .headers()
            .get("x-tako-local")
            .and_then(|s| s.to_str().ok())
            == Some(app.internal_key.as_str());
    match (method.as_str(), path.as_str()) {
        ("POST", "/api/login") => {
            if !same_origin(req.headers()) || req.headers().get("x-tako").is_none() {
                return error(wire, 403, "permission_error", "Invalid login origin");
            }
            let secure = req
                .headers()
                .get("origin")
                .and_then(|s| s.to_str().ok())
                .is_some_and(|s| s.starts_with("https://"));
            let body = match Limited::new(req.into_body(), 4096).collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => {
                    return error(
                        wire,
                        413,
                        "invalid_request_error",
                        "Login request too large",
                    )
                }
            };
            let password = serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| v["password"].as_str().map(String::from))
                .unwrap_or_default();
            return match app.auth.login(password).await {
                Ok(token) => {
                    let mut r = json_resp(200, &json!({"ok": true}));
                    let flags = if secure { "; Secure" } else { "" };
                    r.headers_mut().insert("set-cookie", format!("tako_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200{flags}").parse().unwrap());
                    r
                }
                Err(e) => error(wire, 401, "authentication_error", &e),
            };
        }
        ("GET", "/") if browser => return redirect("/ui"),
        ("GET", "/" | "/health") => return text(200, "ok"),
        ("GET", "/status") => {
            if credentials.protected() && !admin && !local_admin {
                return error(wire, 401, "authentication_error", "Sign in to view status");
            }
            return json_resp(200, &app.status());
        }
        ("GET" | "POST", p) if p == "/ui" || p.starts_with("/api/") => {
            if !same_origin(req.headers())
                || (!credentials.protected() && !local_host(req.headers()))
            {
                return error(
                    wire,
                    403,
                    "permission_error",
                    "the dashboard only answers on localhost or an IP address",
                );
            }
            if credentials.protected() && !admin {
                if path == "/ui" {
                    return html(include_str!("web/login.html"));
                }
                return error(
                    wire,
                    401,
                    "authentication_error",
                    "Sign in to the web panel",
                );
            }
            return ui(app, &method, &path, req).await;
        }
        _ => {}
    }

    let cfg = app.cfg();
    let token = bearer(req.headers());
    if credentials.api_required(&cfg)
        && !(credentials.accepts(token)
            || cfg
                .api_key
                .as_deref()
                .is_some_and(|k| authorized(req.headers(), k))
            || (loopback && token == app.internal_key))
    {
        return error(
            wire,
            401,
            "authentication_error",
            "invalid or missing api key",
        );
    }
    match (method.as_str(), path.as_str()) {
        ("GET", "/v1/models" | "/models") => json_resp(200, &models(&cfg)),
        ("POST", "/v1/messages/count_tokens") => match read_body(req).await {
            // rough estimate; good enough for clients that only use it for context bookkeeping
            Ok(b) => json_resp(200, &json!({"input_tokens": b.len() / 4})),
            Err(e) => error(wire, 400, "invalid_request_error", &e),
        },
        ("POST", "/v1/chat/completions" | "/chat/completions") => {
            proxy(app, cfg, Wire::Openai, req).await
        }
        ("POST", "/v1/messages") => proxy(app, cfg, Wire::Anthropic, req).await,
        _ => error(
            wire,
            404,
            "not_found_error",
            &format!("no route for {method} {path}"),
        ),
    }
}

/// Block DNS rebinding: a web page on evil.example must not be able to read the dashboard API.
fn local_host(h: &HeaderMap) -> bool {
    let Some(host) = h.get("host").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let name = if host.starts_with('[') {
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.rsplit_once(':').map_or(host, |(n, _)| n)
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<std::net::IpAddr>().is_ok()
}

fn bearer(h: &HeaderMap) -> &str {
    h.get("authorization")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| h.get("x-api-key").and_then(|s| s.to_str().ok()))
        .unwrap_or("")
}
fn cookie(h: &HeaderMap) -> String {
    h.get("cookie")
        .and_then(|s| s.to_str().ok())
        .unwrap_or("")
        .split(';')
        .find_map(|s| s.trim().strip_prefix("tako_session="))
        .unwrap_or("")
        .into()
}
fn same_origin(h: &HeaderMap) -> bool {
    if h.get("sec-fetch-site").and_then(|s| s.to_str().ok()) == Some("cross-site") {
        return false;
    }
    let Some(origin) = h.get("origin") else {
        return true;
    };
    let Ok(url) = origin.to_str().unwrap_or("").parse::<reqwest::Url>() else {
        return false;
    };
    let authority = origin
        .to_str()
        .unwrap_or("")
        .split_once("://")
        .map(|(_, a)| a.trim_end_matches('/'))
        .unwrap_or("");
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    h.get("host")
        .and_then(|s| s.to_str().ok())
        .is_some_and(|host| host.eq_ignore_ascii_case(authority))
}
fn html(content: &'static str) -> Response<Body> {
    Response::builder().header("content-type", "text/html; charset=utf-8").header("cache-control", "no-store")
        .header("content-security-policy", "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'")
        .body(Full::new(Bytes::from_static(content.as_bytes())).map_err(|n: Infallible| match n {}).boxed_unsync()).unwrap()
}

async fn ui(app: Arc<App>, method: &str, path: &str, req: Request<Incoming>) -> Response<Body> {
    // Custom headers require a CORS preflight cross-origin, which we never grant.
    if method == "POST" && req.headers().get("x-tako").is_none() {
        return error(
            Wire::Openai,
            403,
            "permission_error",
            "missing x-tako header",
        );
    }
    match (method, path) {
        ("POST", "/api/logout") => {
            app.auth.logout(&cookie(req.headers()));
            let mut r = json_resp(200, &json!({"ok": true}));
            r.headers_mut().insert("set-cookie", "tako_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0".parse().unwrap());
            r
        }
        ("GET", "/api/security") => match crate::auth::load() {
            Ok(c) => json_resp(200, &c.summary()), Err(e) => error(Wire::Openai, 503, "authentication_error", &e),
        },
        ("GET", "/api/catalog") => json_resp(200, &crate::dashboard::catalog()),
        ("GET", "/api/logs") => match crate::dashboard::logs(&app.cfg()) {
            Ok(v) => json_resp(200, &v),
            Err(e) => error(Wire::Openai, 500, "dashboard_error", &e),
        },
        ("POST", "/api/action") => {
            let body = match Limited::new(req.into_body(), 1 << 20).collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => return error(Wire::Openai, 413, "invalid_request_error", "Dashboard request exceeds 1 MB"),
            };
            let value: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => return error(Wire::Openai, 400, "invalid_request_error", "Invalid JSON"),
            };
            let result = if value["action"].as_str().is_some_and(|s| s.starts_with("key.")) {
                crate::auth::action(&value)
            } else { crate::dashboard::action_with_token(&app.cfg(), value, app.internal_key.clone()).await };
            match result {
                Ok(v) => {
                    app.cfg();
                    json_resp(200, &v)
                }
                Err(e) => error(Wire::Openai, 400, "dashboard_error", &e),
            }
        }
        ("GET", "/ui") => Response::builder()
            .header("content-type", "text/html; charset=utf-8")
            .header("cache-control", "no-cache")
            .header("content-security-policy", "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'")
            .body(Full::new(Bytes::from_static(UI_HTML.as_bytes())).map_err(|n: Infallible| match n {}).boxed_unsync())
            .unwrap(),
        ("GET", "/api/overview") => json_resp(200, &overview(&app.cfg())),
        ("POST", "/api/cooldowns/clear") => {
            let body = read_body(req).await.unwrap_or_default();
            let v: Value = serde_json::from_slice(&body).unwrap_or_default();
            let n = app.clear_cooldowns(v["key"].as_str());
            log!("dashboard cleared {n} cooldown(s)");
            json_resp(200, &json!({"cleared": n}))
        }
        _ => error(Wire::Openai, 404, "not_found_error", "no such dashboard route"),
    }
}

/// Config summary for the dashboard. Never includes keys.
fn overview(cfg: &Config) -> Value {
    let providers: BTreeMap<&String, Value> = cfg
        .providers
        .iter()
        .map(|(n, p)| {
            (n, json!({"type": p.wire.name(), "base_url": p.base_url, "keys": p.keys.len(), "disabled": p.disabled, "max_tokens": p.max_tokens}))
        })
        .collect();
    json!({
        "listen": cfg.listen, "providers": providers, "combos": cfg.combos, "aliases": cfg.aliases,
        "default": cfg.default, "cooldown": cfg.cooldown, "long_cooldown": cfg.long_cooldown,
        "auth": crate::auth::load().map(|c| c.api_required(cfg)).unwrap_or(true), "warnings": cfg.warnings,
    })
}

fn authorized(h: &HeaderMap, key: &str) -> bool {
    let get = |name: &str| h.get(name).and_then(|v| v.to_str().ok());
    get("authorization").and_then(|s| s.strip_prefix("Bearer ")) == Some(key)
        || get("x-api-key") == Some(key)
}

fn models(cfg: &Config) -> Value {
    let mut ids: BTreeSet<&str> = cfg.combos.keys().map(String::as_str).collect();
    ids.extend(
        cfg.aliases
            .keys()
            .filter(|k| !k.ends_with('*'))
            .map(String::as_str),
    );
    ids.extend(cfg.combos.values().flatten().map(String::as_str));
    let data: Vec<Value> = ids
        .iter()
        .map(|id| json!({"id": id, "object": "model", "created": 0, "owned_by": "tako"}))
        .collect();
    json!({"object": "list", "data": data})
}

// ---- proxying with fallback --------------------------------------------

async fn proxy(
    app: Arc<App>,
    cfg: Arc<Config>,
    wire: Wire,
    req: Request<Incoming>,
) -> Response<Body> {
    let beta = req
        .headers()
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let body = match read_body(req).await {
        Ok(b) => b,
        Err(e) => return error(wire, 400, "invalid_request_error", &e),
    };
    let body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error(
                wire,
                400,
                "invalid_request_error",
                &format!("invalid json: {e}"),
            )
        }
    };
    let Some(model) = body["model"].as_str().map(String::from) else {
        return error(wire, 400, "invalid_request_error", "missing \"model\"");
    };
    let targets = match cfg.resolve(&model) {
        Ok(t) => t,
        Err(e) => return error(wire, 404, "not_found_error", &e),
    };
    let stream = body["stream"] == true;
    let policy = Policy {
        cooldown: cfg.cooldown,
        long: cfg.long_cooldown,
    };
    let req_id = app.seq.fetch_add(1, Ordering::Relaxed);
    let mut attempts: Vec<Attempt> = Vec::new();
    let mut benched: BTreeMap<String, Cool> = BTreeMap::new();
    let mut last: Option<(Wire, u16, String)> = None;

    // First pass skips anything cooling down. If that leaves nothing to try, a second
    // pass retries what was benched for a transient reason (rate limits, 5xx) rather than
    // failing outright. Keys that are out of credits or rejected stay benched.
    for respect_cooldown in [true, false] {
        let mut tried = false;
        'targets: for t in &targets {
            let Some(p) = cfg.providers.get(&t.provider).filter(|p| !p.disabled) else {
                continue;
            };
            let keys: Vec<&str> = if p.keys.is_empty() {
                vec![""]
            } else {
                p.keys.iter().map(String::as_str).collect()
            };
            let tag = format!("{}/{}", t.provider, t.model);
            let key_id = |ki: usize| {
                if keys.len() > 1 {
                    format!("{tag} (key {})", ki + 1)
                } else {
                    tag.clone()
                }
            };
            let start = app.rr.fetch_add(1, Ordering::Relaxed);
            for i in 0..keys.len() {
                let ki = (start + i) % keys.len();
                let ck = key_id(ki);
                if let Some(c) = app.cooling(&ck) {
                    if respect_cooldown || c.kind.sticky() {
                        benched.insert(ck, c);
                        continue;
                    }
                }
                tried = true;
                let began = Instant::now();
                let up = upstream_body(&body, wire, p, &t.model);
                let (status, headers, text) =
                    match send(&app.http, p, keys[ki], wire, beta.as_deref(), &up).await {
                        Ok(resp) if resp.status().is_success() => {
                            let status = resp.status().as_u16();
                            app.record_limits(&ck, resp.headers());
                            match accept(resp, stream).await {
                                Ok(got) => {
                                    let ms = began.elapsed().as_millis() as u64;
                                    if attempts.is_empty() {
                                        log!("{model} → {ck} {status} {ms}ms");
                                    } else {
                                        log!(
                                        "{model} → {ck} {status} {ms}ms (fallback after {} failed)",
                                        attempts.len()
                                    );
                                    }
                                    app.record(req_id, &model, &tag, &ck, status, ms, None, "");
                                    app.finish(if attempts.is_empty() { 0 } else { 1 });
                                    let mut usage = crate::usage::Usage::new(&up);
                                    let got = match got {
                                        Got::Full(bytes) => {
                                            if let Ok(v) = serde_json::from_slice(&bytes) {
                                                usage.value(&v);
                                            }
                                            app.record_tokens(req_id, &tag, usage.finish());
                                            Got::Full(bytes)
                                        }
                                        Got::Stream(head, rest) => {
                                            usage.push(&head);
                                            let mut meter = StreamMeter {
                                                app: app.clone(),
                                                req: req_id,
                                                tag: tag.clone(),
                                                usage,
                                            };
                                            let rest = rest
                                                .map(move |chunk| {
                                                    if let Ok(bytes) = &chunk {
                                                        meter.usage.push(bytes);
                                                    }
                                                    chunk
                                                })
                                                .boxed();
                                            Got::Stream(head, rest)
                                        }
                                    };
                                    return deliver(got, wire, p.wire);
                                }
                                Err((s, text)) => (s, None, text),
                            }
                        }
                        Ok(resp) => {
                            let status = resp.status().as_u16();
                            app.record_limits(&ck, resp.headers());
                            let headers = resp.headers().clone();
                            (status, Some(headers), resp.text().await.unwrap_or_default())
                        }
                        Err(e) => {
                            let msg = chain(&e);
                            let ms = began.elapsed().as_millis() as u64;
                            let kind = if e.is_timeout() {
                                Kind::Timeout
                            } else {
                                Kind::Network
                            };
                            log!("{model} → {ck} {} {ms}ms: {msg}", kind.label());
                            app.record(req_id, &model, &tag, &ck, 0, ms, Some(kind), &msg);
                            app.cool(&ck, cfg.cooldown, kind);
                            attempts.push(Attempt {
                                who: ck.clone(),
                                kind,
                                status: 0,
                                wait: cfg.cooldown,
                            });
                            last = Some((wire, 502, format!("{ck}: {msg}")));
                            continue;
                        }
                    };
                let ms = began.elapsed().as_millis() as u64;
                let f = failure::classify(status, headers.as_ref(), &text, &policy);
                let wait = if f.scope == Scope::Skip {
                    0
                } else {
                    f.cooldown
                };
                let note = if wait > 0 {
                    format!(", benched {}", crate::util::dur(wait))
                } else {
                    String::new()
                };
                log!(
                    "{model} → {ck} {status} {}{note} {ms}ms: {}",
                    f.kind.label(),
                    short(&text, 200)
                );
                app.record(req_id, &model, &tag, &ck, status, ms, Some(f.kind), &text);
                attempts.push(Attempt {
                    who: ck.clone(),
                    kind: f.kind,
                    status,
                    wait,
                });
                last = Some((p.wire, status, text));
                match f.scope {
                    // Other keys of the same provider would fail the same way; try the next target.
                    Scope::Skip => continue 'targets,
                    Scope::Target => {
                        (0..keys.len()).for_each(|k| app.cool(&key_id(k), f.cooldown, f.kind));
                        continue 'targets;
                    }
                    Scope::Key => app.cool(&ck, f.cooldown, f.kind),
                }
            }
        }
        if tried {
            break;
        }
    }
    app.finish(2);
    if attempts.is_empty() {
        if let Some(c) = benched.values().min_by_key(|c| c.until) {
            app.record_benched(req_id, &model, c.kind);
        }
    }
    exhausted(wire, &model, &attempts, &benched, last)
}

/// Every target failed or is benched: tell the client why, as usefully as possible.
fn exhausted(
    wire: Wire,
    model: &str,
    attempts: &[Attempt],
    benched: &BTreeMap<String, Cool>,
    last: Option<(Wire, u16, String)>,
) -> Response<Body> {
    // The request itself was the problem everywhere (e.g. prompt too long): pass the
    // provider's own error through so the client can react (compact, trim, ...).
    if !attempts.is_empty() && attempts.iter().all(|a| a.kind.request_problem()) {
        if let Some((pw, status, text)) = last {
            return upstream_error(wire, pw, status, &text);
        }
    }
    let now = Instant::now();
    let mut parts: Vec<String> = attempts
        .iter()
        .map(|a| {
            let code = if a.status > 0 {
                format!(" ({})", a.status)
            } else {
                String::new()
            };
            let wait = if a.wait > 0 {
                format!(", retry in {}", crate::util::dur(a.wait))
            } else {
                String::new()
            };
            format!("{}: {}{code}{wait}", a.who, a.kind.label())
        })
        .collect();
    let mut kinds: Vec<(Kind, u64)> = attempts.iter().map(|a| (a.kind, a.wait)).collect();
    for (k, c) in benched {
        if attempts.iter().any(|a| &a.who == k) {
            continue;
        }
        let left = c.until.saturating_duration_since(now).as_secs() + 1;
        parts.push(format!(
            "{k}: {} ({} left)",
            c.kind.label(),
            crate::util::dur(left)
        ));
        kinds.push((c.kind, left));
    }
    if parts.is_empty() {
        return error(
            wire,
            503,
            "overloaded_error",
            &format!("no usable provider for '{model}' (missing keys or env vars?)"),
        );
    }
    let msg = format!(
        "tako: every target for '{model}' failed. {}",
        parts.join("; ")
    );
    let limits: Vec<u64> = kinds
        .iter()
        .filter(|(k, _)| k.is_limit())
        .map(|(_, w)| *w)
        .filter(|w| *w > 0)
        .collect();
    if kinds.iter().any(|(k, _)| k.is_limit()) {
        let retry = limits.iter().min().copied();
        return error_with_retry(wire, 429, "rate_limit_error", &msg, retry);
    }
    if kinds.iter().all(|(k, _)| *k == Kind::Billing) {
        return error(
            wire,
            402,
            "billing_error",
            &format!("{msg}. Top up a provider or add a free one."),
        );
    }
    if kinds.iter().any(|(k, _)| {
        matches!(
            k,
            Kind::Overloaded | Kind::Server | Kind::Timeout | Kind::Network
        )
    }) {
        return error(wire, 503, "overloaded_error", &msg);
    }
    error(wire, 502, "api_error", &msg)
}

fn upstream_body(body: &Value, wire: Wire, p: &Provider, model: &str) -> Value {
    let mut v = match (wire, p.wire) {
        (Wire::Anthropic, Wire::Openai) => tr::req_anthropic_to_openai(body, model),
        (Wire::Openai, Wire::Anthropic) => tr::req_openai_to_anthropic(body, model),
        _ => {
            let mut v = body.clone();
            v["model"] = model.into();
            v
        }
    };
    if let Some(cap) = p.max_tokens {
        for k in ["max_tokens", "max_completion_tokens"] {
            if v.get(k).and_then(Value::as_u64).is_some_and(|n| n > cap) {
                v[k] = cap.into();
            }
        }
    }
    v
}

async fn send(
    http: &reqwest::Client,
    p: &Provider,
    key: &str,
    wire: Wire,
    beta: Option<&str>,
    body: &Value,
) -> Result<reqwest::Response, reqwest::Error> {
    let url = match p.wire {
        Wire::Openai => format!("{}/chat/completions", p.base_url),
        Wire::Anthropic => format!("{}/v1/messages", p.base_url),
    };
    let mut rb = http.post(url).header("content-type", "application/json");
    match p.wire {
        Wire::Openai if !key.is_empty() => rb = rb.bearer_auth(key),
        Wire::Openai => {}
        Wire::Anthropic => {
            if !key.is_empty() {
                rb = rb.header("x-api-key", key);
            }
            rb = rb.header("anthropic-version", "2023-06-01");
            if let (Wire::Anthropic, Some(b)) = (wire, beta) {
                rb = rb.header("anthropic-beta", b);
            }
        }
    }
    for (k, v) in &p.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    rb.body(serde_json::to_vec(body).unwrap_or_default())
        .send()
        .await
}

// The meter is dropped on EOF, upstream errors, and client disconnects alike.
struct StreamMeter {
    app: Arc<App>,
    req: u64,
    tag: String,
    usage: crate::usage::Usage,
}

impl Drop for StreamMeter {
    fn drop(&mut self) {
        self.app
            .record_tokens(self.req, &self.tag, self.usage.finish());
    }
}

type ByteStream = futures_util::stream::BoxStream<'static, Result<Bytes, reqwest::Error>>;

/// A 2xx response we've looked into enough to trust.
enum Got {
    Full(Bytes),
    /// The first event(s), already read, and the rest of the stream.
    Stream(Bytes, ByteStream),
}

const PEEK_LIMIT: usize = 64 << 10;

/// Has the buffer got a complete SSE event carrying data (not just `: keepalive` comments)?
fn has_data_event(buf: &[u8]) -> bool {
    let s = String::from_utf8_lossy(buf).replace('\r', "");
    let Some(end) = s.rfind("\n\n") else {
        return false;
    };
    s[..end]
        .split("\n\n")
        .any(|ev| ev.lines().any(|l| l.starts_with("data:")))
}

/// Some providers answer 200 and then send an error (or nothing). Catch that before
/// anything reaches the client, so we can still fall back. Err is (status, error text).
async fn accept(resp: reqwest::Response, stream: bool) -> Result<Got, (u16, String)> {
    let is_sse = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.starts_with("text/event-stream"));
    if stream && is_sse {
        let mut up = resp.bytes_stream().boxed();
        let mut head = Vec::new();
        while !has_data_event(&head) && head.len() < PEEK_LIMIT {
            match up.next().await {
                Some(Ok(c)) => head.extend_from_slice(&c),
                Some(Err(e)) => {
                    return Err((502, format!("stream broke before any data: {}", chain(&e))))
                }
                None => break,
            }
        }
        if head.iter().all(u8::is_ascii_whitespace) {
            return Err((502, "provider returned an empty stream".into()));
        }
        if let Some(e) = failure::error_in_stream(&head) {
            return Err(e);
        }
        return Ok(Got::Stream(Bytes::from(head), up));
    }
    let bytes = resp.bytes().await.map_err(|e| (502, chain(&e)))?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err((502, "provider returned an empty body".into()));
    }
    if let Some(e) = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .as_ref()
        .and_then(failure::error_in_body)
    {
        return Err(e);
    }
    Ok(Got::Full(bytes))
}

fn deliver(got: Got, wire: Wire, pwire: Wire) -> Response<Body> {
    let bytes = match got {
        Got::Stream(head, rest) => return deliver_stream(head, rest, wire, pwire),
        Got::Full(b) => b,
    };
    if wire == pwire {
        return respond_bytes(200, "application/json", bytes);
    }
    let v: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return error(wire, 502, "api_error", &format!("bad upstream json: {e}")),
    };
    let out = match pwire {
        Wire::Anthropic => tr::resp_anthropic_to_openai(&v),
        Wire::Openai => tr::resp_openai_to_anthropic(&v),
    };
    json_resp(200, &out)
}

fn deliver_stream(head: Bytes, rest: ByteStream, wire: Wire, pwire: Wire) -> Response<Body> {
    let full = futures_util::stream::once(async move { Ok(head) }).chain(rest);
    if wire == pwire {
        let s = full.map(|r| r.map(Frame::data).map_err(io::Error::other));
        return sse(StreamBody::new(s).boxed_unsync());
    }
    let mut t: Box<dyn Translate> = match pwire {
        Wire::Anthropic => Box::new(AnthToOai::default()),
        Wire::Openai => Box::new(OaiToAnth::new()),
    };
    let (tx, rx) = mpsc::channel::<Bytes>(16);
    tokio::spawn(async move {
        let mut up = Box::pin(full);
        let mut parser = Sse::default();
        let mut out = Vec::with_capacity(4096);
        while let Some(chunk) = up.next().await {
            match chunk {
                Ok(c) => parser.push(&c, &mut |ev, data| t.event(ev, data, &mut out)),
                Err(e) => {
                    log!("upstream stream error: {}", chain(&e));
                    break;
                }
            }
            if !out.is_empty()
                && tx
                    .send(Bytes::from(std::mem::take(&mut out)))
                    .await
                    .is_err()
            {
                return; // client went away
            }
        }
        parser.flush(&mut |ev, data| t.event(ev, data, &mut out));
        t.finish(&mut out);
        if !out.is_empty() {
            let _ = tx.send(Bytes::from(out)).await;
        }
    });
    let s = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|b| (Ok::<_, io::Error>(Frame::data(b)), rx))
    });
    sse(StreamBody::new(s).boxed_unsync())
}

async fn read_body(req: Request<Incoming>) -> Result<Bytes, String> {
    Limited::new(req.into_body(), MAX_BODY)
        .collect()
        .await
        .map(|c| c.to_bytes())
        .map_err(|e| format!("cannot read request body: {e}"))
}

// ---- responses --------------------------------------------------------

fn respond_bytes(status: u16, ctype: &str, b: impl Into<Bytes>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("cache-control", "no-store")
        .header("content-type", ctype)
        .body(
            Full::new(b.into())
                .map_err(|n: Infallible| match n {})
                .boxed_unsync(),
        )
        .unwrap()
}

fn text(status: u16, s: &'static str) -> Response<Body> {
    respond_bytes(status, "text/plain", s)
}

fn json_resp(status: u16, v: &Value) -> Response<Body> {
    respond_bytes(
        status,
        "application/json",
        serde_json::to_vec(v).unwrap_or_default(),
    )
}

fn sse(body: Body) -> Response<Body> {
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(body)
        .unwrap()
}

fn error(wire: Wire, status: u16, kind: &str, msg: &str) -> Response<Body> {
    error_with_retry(wire, status, kind, msg, None)
}

fn error_with_retry(
    wire: Wire,
    status: u16,
    kind: &str,
    msg: &str,
    retry_after: Option<u64>,
) -> Response<Body> {
    let v = match wire {
        Wire::Anthropic => json!({"type": "error", "error": {"type": kind, "message": msg}}),
        Wire::Openai => json!({"error": {"message": msg, "type": kind}}),
    };
    let mut r = json_resp(status, &v);
    if let Some(s) = retry_after {
        r.headers_mut().insert("retry-after", s.into());
    }
    r
}

fn redirect(to: &'static str) -> Response<Body> {
    Response::builder()
        .status(302)
        .header("location", to)
        .body(
            Full::new(Bytes::new())
                .map_err(|n: Infallible| match n {})
                .boxed_unsync(),
        )
        .unwrap()
}

/// Pass an upstream error through, re-wrapped if the client speaks the other format.
fn upstream_error(wire: Wire, pwire: Wire, status: u16, text: &str) -> Response<Body> {
    let parsed = serde_json::from_str::<Value>(text).ok();
    if wire == pwire && parsed.is_some() {
        return respond_bytes(status, "application/json", text.to_string());
    }
    let msg = parsed
        .as_ref()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or(v["message"].as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| short(text, 500));
    error(wire, status, "api_error", &msg)
}
