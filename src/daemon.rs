//! Background process, autostart service, status, update and uninstall.

use crate::config;
use crate::server::VERSION;
use crate::util::{bold, confirm, dim, dur, e2s, green, red, yellow};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

/// GitHub repo that `tako update` pulls releases from. Override at build time with TAKO_REPO.
const REPO: &str = match option_env!("TAKO_REPO") {
    Some(r) => r,
    None => "real-kijmoshi/tako",
};
const LABEL: &str = "dev.tako";

fn pid_file() -> PathBuf {
    config::dir().join("tako.pid")
}
pub fn log_file() -> PathBuf {
    config::dir().join("tako.log")
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

pub fn running() -> Option<i32> {
    let pid: i32 = fs::read_to_string(pid_file()).ok()?.trim().parse().ok()?;
    alive(pid).then_some(pid)
}

pub fn write_pid() {
    let _ = fs::write(pid_file(), std::process::id().to_string());
}
pub fn remove_pid() {
    let _ = fs::remove_file(pid_file());
}

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("cannot find own binary: {e}"))
}

fn listen() -> String {
    config::load(&config::path())
        .map(|c| c.listen)
        .unwrap_or_else(|_| config::DEFAULT_LISTEN.into())
}

fn wait_ready(listen: &str) -> bool {
    (0..40).any(|_| {
        sleep(Duration::from_millis(100));
        crate::util::get(listen, "/health").is_ok()
    })
}

fn tail(lines: usize) -> String {
    let s = fs::read_to_string(log_file()).unwrap_or_default();
    let v: Vec<&str> = s.lines().collect();
    v[v.len().saturating_sub(lines)..].join("\n")
}

// ---- start / stop -----------------------------------------------------

pub fn start() -> Result<String, String> {
    if let Some(pid) = running() {
        return Ok(format!("already running (pid {pid})"));
    }
    let cfg = config::load(&config::ensure()?)?; // fail early on a broken config

    if service_installed() {
        service_start()?;
    } else {
        // keep the log from growing forever
        if fs::metadata(log_file())
            .map(|m| m.len() > 5 << 20)
            .unwrap_or(false)
        {
            let _ = fs::rename(log_file(), log_file().with_extension("log.old"));
        }
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file())
            .map_err(|e| format!("cannot open log: {e}"))?;
        let mut child = Command::new(exe()?)
            .arg("run")
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(e2s)?)
            .stderr(log)
            .process_group(0)
            .spawn()
            .map_err(|e| format!("cannot start: {e}"))?;
        for _ in 0..40 {
            sleep(Duration::from_millis(100));
            if let Ok(Some(st)) = child.try_wait() {
                return Err(format!("tako exited ({st}). last log lines:\n{}", tail(10)));
            }
            if crate::util::get(&cfg.listen, "/health").is_ok() {
                return Ok(format!("running on http://{}", cfg.listen));
            }
        }
        return Err("tako did not become ready, see `tako logs`".into());
    }
    if wait_ready(&cfg.listen) {
        Ok(format!("running on http://{}", cfg.listen))
    } else {
        Err(format!(
            "tako did not become ready. last log lines:\n{}",
            tail(10)
        ))
    }
}

pub fn stop() -> Result<String, String> {
    let Some(pid) = running() else {
        return Ok("not running".into());
    };
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let gone = (0..50).any(|_| {
        sleep(Duration::from_millis(100));
        !alive(pid)
    });
    if !gone {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    remove_pid();
    Ok("stopped".into())
}

pub fn restart() -> Result<String, String> {
    if running().is_some() {
        stop()?;
    }
    start().map(|m| format!("restarted, {m}"))
}

pub fn logs(follow: bool) -> Result<(), String> {
    let f = log_file();
    if !f.exists() {
        println!("no logs yet ({})", f.display());
        return Ok(());
    }
    if follow {
        let e = Command::new("tail").args(["-n", "50", "-f"]).arg(&f).exec();
        return Err(format!("cannot run tail: {e}"));
    }
    println!("{}", tail(100));
    Ok(())
}

// ---- autostart (launchd / systemd --user) -----------------------------

fn service_file() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    if cfg!(target_os = "macos") {
        home.join(format!("Library/LaunchAgents/{LABEL}.plist"))
    } else {
        home.join(".config/systemd/user/tako.service")
    }
}

pub fn service_installed() -> bool {
    service_file().exists()
}

fn sh(cmd: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {cmd}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{cmd} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn gui_domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

fn service_start() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let f = service_file();
        let _ = sh(
            "launchctl",
            &["bootout", &format!("{}/{LABEL}", gui_domain())],
        );
        sh(
            "launchctl",
            &["bootstrap", &gui_domain(), &f.to_string_lossy()],
        )
    } else {
        sh("systemctl", &["--user", "start", "tako.service"])
    }
}

pub fn enable() -> Result<String, String> {
    config::ensure()?;
    let exe = exe()?;
    let log = log_file();
    let f = service_file();
    if let Some(d) = f.parent() {
        fs::create_dir_all(d).map_err(e2s)?;
    }
    if running().is_some() {
        stop()?;
    }
    if cfg!(target_os = "macos") {
        // KeepAlive only on crash: a clean exit (tako stop) stays stopped.
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key><array><string>{}</string><string>run</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>StandardOutPath</key><string>{}</string>
  <key>StandardErrorPath</key><string>{}</string>
</dict>
</plist>
"#,
            exe.display(),
            log.display(),
            log.display()
        );
        fs::write(&f, plist).map_err(e2s)?;
        service_start()?;
    } else {
        let unit = format!(
            "[Unit]\nDescription=tako AI router\nAfter=network-online.target\n\n\
             [Service]\nExecStart={} run\nRestart=on-failure\nRestartSec=3\n\n\
             [Install]\nWantedBy=default.target\n",
            exe.display()
        );
        fs::write(&f, unit).map_err(e2s)?;
        sh("systemctl", &["--user", "daemon-reload"])?;
        sh("systemctl", &["--user", "enable", "--now", "tako.service"])?;
    }
    let l = listen();
    if !wait_ready(&l) {
        return Err(format!(
            "autostart installed but tako did not come up. last log lines:\n{}",
            tail(10)
        ));
    }
    Ok(format!("autostart on; running on http://{l}"))
}

pub fn disable() -> Result<String, String> {
    if !service_installed() {
        return Ok("autostart is already off".into());
    }
    if cfg!(target_os = "macos") {
        let _ = sh(
            "launchctl",
            &["bootout", &format!("{}/{LABEL}", gui_domain())],
        );
    } else {
        let _ = sh("systemctl", &["--user", "disable", "--now", "tako.service"]);
    }
    fs::remove_file(service_file()).map_err(e2s)?;
    if !cfg!(target_os = "macos") {
        let _ = sh("systemctl", &["--user", "daemon-reload"]);
    }
    let _ = stop_quiet();
    Ok("autostart off, tako stopped".into())
}

fn stop_quiet() -> Option<()> {
    let pid = running()?;
    unsafe { libc::kill(pid, libc::SIGTERM) };
    remove_pid();
    Some(())
}

// ---- status -----------------------------------------------------------

pub fn status() -> Result<(), String> {
    let path = config::ensure()?;
    let cfg = config::load(&path);
    let listen = cfg
        .as_ref()
        .map(|c| c.listen.clone())
        .unwrap_or_else(|_| config::DEFAULT_LISTEN.into());
    let pid = running();
    let live: Option<Value> = pid
        .and_then(|_| crate::util::get(&listen, "/status").ok())
        .and_then(|s| serde_json::from_str(&s).ok());

    match (&live, pid) {
        (Some(v), _) => println!(
            "{} {}  v{}  pid {}  up {}  http://{listen}",
            green("●"),
            bold("tako is running"),
            v["version"].as_str().unwrap_or("?"),
            v["pid"],
            dur(v["uptime"].as_u64().unwrap_or(0))
        ),
        (None, Some(pid)) => println!(
            "{} tako (pid {pid}) is not answering on {listen}",
            yellow("●")
        ),
        (None, None) => println!(
            "{} {}  (start it with: tako start)",
            dim("○"),
            bold("tako is stopped")
        ),
    }
    println!(
        "  {}",
        dim(&format!(
            "autostart {}  ·  config {}",
            if service_installed() { "on" } else { "off" },
            path.display()
        ))
    );

    let cfg = match cfg {
        Ok(c) => c,
        Err(e) => {
            println!("\n{} {e}", red("config error:"));
            return Ok(());
        }
    };
    if cfg.providers.is_empty() {
        println!("\nno providers yet. add one with: tako config");
        return Ok(());
    }

    println!("\n{}", bold("providers"));
    for (name, p) in &cfg.providers {
        let keys = match (p.disabled, p.keys.len()) {
            (true, _) => red("key missing (env var not set)"),
            (_, 0) => "no key".into(),
            (_, 1) => "1 key".into(),
            (_, n) => format!("{n} keys"),
        };
        println!(
            "  {:<14} {:<10} {:<8} {}",
            name,
            p.wire.name(),
            keys,
            dim(&p.base_url)
        );
    }
    if !cfg.combos.is_empty() {
        println!("\n{}", bold("combos"));
        for (name, list) in &cfg.combos {
            println!("  {:<14} {}", name, list.join(&dim(" → ")));
        }
    }
    if let Some(d) = &cfg.default {
        println!(
            "  {:<14} {}",
            dim("default"),
            dim(&format!("{d} (used for any model tako doesn't know)"))
        );
    }
    if !cfg.aliases.is_empty() {
        println!("\n{}", bold("aliases"));
        for (pat, t) in &cfg.aliases {
            println!("  {:<14} → {}", pat, t);
        }
    }
    if let Some(v) = &live {
        if let Some(tokens) = v.get("tokens") {
            println!("\n{}", bold("tokens since start"));
            println!(
                "  {} used · {} input · {} output · {} estimated responses",
                tokens["total_tokens"],
                tokens["input_tokens"],
                tokens["output_tokens"],
                tokens["estimated_requests"]
            );
            if let Some(limits) = v["token_limits"].as_object().filter(|l| !l.is_empty()) {
                for (key, q) in limits {
                    println!(
                        "  {key}: {} remaining (provider report, {}s ago; reset: {})",
                        q["remaining"],
                        crate::util::now_secs()
                            .saturating_sub(q["observed_at"].as_u64().unwrap_or(0)),
                        q["reset"].as_str().unwrap_or("unknown")
                    );
                }
            } else {
                println!("  remaining: unknown (no provider limit headers)");
            }
        }
        if let Some(stats) = v["stats"].as_object().filter(|s| !s.is_empty()) {
            println!("\n{}", bold("traffic since start"));
            for (k, s) in stats {
                let fail = s["fail"].as_u64().unwrap_or(0);
                let f = format!("{fail} failed");
                println!(
                    "  {:<40} {:>6} ok  {}",
                    k,
                    s["ok"],
                    if fail > 0 { yellow(&f) } else { dim(&f) }
                );
            }
        }
        if let Some(c) = v["cooling"].as_object().filter(|c| !c.is_empty()) {
            println!("\n{}", bold("cooling down"));
            for (k, secs) in c {
                let why = v["cooling_info"][k.as_str()]["label"]
                    .as_str()
                    .unwrap_or("");
                let left = crate::util::dur(secs.as_u64().unwrap_or(0));
                println!("  {:<40} {}  {}", k, yellow(&left), dim(why));
            }
        }
    }
    let tok = crate::auth::local_token()
        .or(cfg.api_key.clone())
        .unwrap_or_else(|| "tako".into());
    println!("\n{}", bold("connect"));
    println!("  Claude Code    tako claude");
    println!("  OpenAI-style   base url http://{listen}/v1   api key {tok}");
    Ok(())
}

// ---- update / uninstall -----------------------------------------------

fn version_tuple(v: &str) -> Vec<u64> {
    v.trim_start_matches('v')
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

pub fn update() -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(e2s)?;
    let updated = rt.block_on(async {
        let http = reqwest::Client::builder()
            .user_agent(concat!("tako/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(e2s)?;
        let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
        let resp = http.get(&api).send().await.map_err(e2s)?;
        if !resp.status().is_success() {
            return Err(format!(
                "cannot check for updates ({}): {api}",
                resp.status()
            ));
        }
        let rel: Value = serde_json::from_slice(&resp.bytes().await.map_err(e2s)?).map_err(e2s)?;
        let tag = rel["tag_name"]
            .as_str()
            .ok_or("no release found")?
            .to_string();
        if version_tuple(&tag) <= version_tuple(VERSION) {
            println!("{} already up to date (v{VERSION})", green("✓"));
            return Ok(false);
        }
        let asset = format!("tako-{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        let url = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");
        println!("downloading {tag} …");
        let resp = http.get(&url).send().await.map_err(e2s)?;
        if !resp.status().is_success() {
            return Err(format!("download failed ({}): {url}", resp.status()));
        }
        let bin = resp.bytes().await.map_err(e2s)?;
        let sums_url = format!("https://github.com/{REPO}/releases/download/{tag}/SHA256SUMS");
        let sums = http
            .get(&sums_url)
            .send()
            .await
            .map_err(e2s)?
            .error_for_status()
            .map_err(|e| format!("Cannot fetch release checksums: {e}. Current binary kept."))?
            .text()
            .await
            .map_err(e2s)?;
        crate::util::verify_checksum(&sums, &asset, &bin)?;
        let exe = exe()?;
        let tmp = exe.with_extension("new");
        fs::write(&tmp, &bin).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)).map_err(e2s)?;
        fs::rename(&tmp, &exe).map_err(|e| format!("cannot replace {}: {e}", exe.display()))?;
        println!("{} updated v{VERSION} → {tag}", green("✓"));
        Ok(true)
    })?;
    if updated && running().is_some() {
        // re-exec the new binary to restart
        let st = Command::new(exe()?).arg("restart").status().map_err(e2s)?;
        if !st.success() {
            return Err("restart failed, run `tako restart`".into());
        }
    }
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let exe = exe()?;
    println!(
        "This stops tako, removes autostart, and deletes {}",
        exe.display()
    );
    if !confirm("Continue?") {
        return Ok(());
    }
    crate::auto_update::disable()?;
    if service_installed() {
        println!("{} {}", green("✓"), disable()?);
    }
    if running().is_some() {
        println!("{} {}", green("✓"), stop()?);
    }
    let dir = config::dir();
    if dir.exists()
        && confirm(&format!(
            "Also delete your config and logs in {}?",
            dir.display()
        ))
    {
        fs::remove_dir_all(&dir).map_err(e2s)?;
        println!("{} removed {}", green("✓"), dir.display());
    }
    fs::remove_file(&exe).map_err(|e| format!("cannot remove {}: {e}", exe.display()))?;
    println!("{} tako uninstalled", green("✓"));
    Ok(())
}

// ---- launch Claude Code through tako ---------------------------------

/// `tako claude [combo] [claude args...]`
/// `tako web`: make sure the router runs, then open the dashboard.
pub fn web() -> Result<(), String> {
    if running().is_none() {
        println!("{} {}", crate::util::green("✓"), start()?);
    }
    let url = format!("http://{}/ui", crate::util::local_addr(&listen())?);
    crate::tui::open_url(&url)?;
    println!("{} dashboard at {url}", crate::util::green("✓"));
    Ok(())
}

pub fn claude(args: &[String]) -> Result<(), String> {
    if running().is_none() {
        println!("{} {}", green("✓"), start()?);
    }
    let cfg = config::load(&config::path())?;
    let (model, rest) = match args.first() {
        Some(a) if cfg.combos.contains_key(a) => (Some(a.clone()), &args[1..]),
        _ => (
            cfg.default
                .clone()
                .or_else(|| cfg.combos.keys().next().cloned()),
            args,
        ),
    };
    let tok = crate::auth::local_token()
        .or(cfg.api_key.clone())
        .unwrap_or_else(|| "tako".into());
    let mut cmd = Command::new("claude");
    cmd.args(rest)
        .env(
            "ANTHROPIC_BASE_URL",
            format!("http://{}", crate::util::local_addr(&cfg.listen)?),
        )
        .env("ANTHROPIC_AUTH_TOKEN", tok)
        .env_remove("ANTHROPIC_API_KEY");
    if let Some(m) = &model {
        for k in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_SMALL_FAST_MODEL",
        ] {
            cmd.env(k, m);
        }
        println!("{}", dim(&format!("claude → tako → combo '{m}'")));
    }
    let e = cmd.exec();
    Err(if e.kind() == std::io::ErrorKind::NotFound {
        "Claude Code isn't installed. Install it with: curl -fsSL https://claude.ai/install.sh | bash".into()
    } else {
        format!("cannot run claude: {e}")
    })
}
