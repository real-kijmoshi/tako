//! Opt-in daily updates, scheduled by the user's service manager.
use crate::{config, util::e2s};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

const LABEL: &str = "dev.tako.update";
const TIMER: &str = "tako-update.timer";

fn root() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents")
    } else {
        PathBuf::from(
            std::env::var_os("XDG_CONFIG_HOME")
                .unwrap_or_else(|| home.join(".config").into_os_string()),
        )
        .join("systemd/user")
    }
}
fn file() -> PathBuf {
    root().join(if cfg!(target_os = "macos") {
        "dev.tako.update.plist"
    } else {
        TIMER
    })
}
fn domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}
fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(program).args(args).output().map_err(e2s)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}
fn loaded() -> bool {
    if cfg!(target_os = "macos") {
        run("launchctl", &["print", &format!("{}/{LABEL}", domain())]).is_ok()
    } else {
        run("systemctl", &["--user", "is-active", TIMER]).is_ok()
    }
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn quote(s: &str, exec: bool) -> String {
    let s = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{}\"", if exec { s.replace('$', "$$") } else { s })
}
fn plist(exe: &str, home: &str, log: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>update</string></array>
<key>EnvironmentVariables</key><dict><key>TAKO_HOME</key><string>{}</string></dict>
<key>StartInterval</key><integer>86400</integer>
<key>StandardOutPath</key><string>{}</string>
<key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        xml(exe),
        xml(home),
        xml(log),
        xml(log)
    )
}
fn unit(exe: &str, home: &str) -> String {
    format!("[Unit]\nDescription=Check for verified Tako updates\nWants=network-online.target\nAfter=network-online.target\n\n[Service]\nType=oneshot\nExecStart={} update\nEnvironment={}\nTimeoutStartSec=10min\n# A restarted standalone router must outlive the updater.\nKillMode=process\n", quote(exe, true), quote(&format!("TAKO_HOME={home}"), false))
}
const SCHEDULE: &str = "[Unit]\nDescription=Update Tako daily\n\n[Timer]\nOnCalendar=daily\nPersistent=true\nRandomizedDelaySec=1h\n\n[Install]\nWantedBy=timers.target\n";

pub fn disable() -> Result<(), String> {
    if !file().exists() {
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        if loaded() {
            run("launchctl", &["bootout", &format!("{}/{LABEL}", domain())])?;
        }
    } else {
        run("systemctl", &["--user", "disable", "--now", TIMER])?;
    }
    fs::remove_file(file()).map_err(e2s)?;
    if !cfg!(target_os = "macos") {
        let service = root().join("tako-update.service");
        if service.exists() {
            fs::remove_file(service).map_err(e2s)?;
        }
        run("systemctl", &["--user", "daemon-reload"])?;
    }
    Ok(())
}

pub fn command(args: &[String]) -> Result<(), String> {
    if args.len() > 1 {
        return Err("usage: tako auto-update [on|off|status]".into());
    }
    match args.first().map(String::as_str).unwrap_or("status") {
        "status" => println!(
            "automatic updates: {}",
            if file().exists() && loaded() {
                "on (daily)"
            } else {
                "off"
            }
        ),
        "off" => {
            disable()?;
            println!("automatic updates: off");
        }
        "on" => {
            config::ensure()?;
            let exe = std::env::current_exe()
                .and_then(|p| p.canonicalize())
                .map_err(e2s)?;
            let home = config::dir().canonicalize().map_err(e2s)?;
            let exe = exe.to_string_lossy();
            let home = home.to_string_lossy();
            if [&*exe, &*home]
                .iter()
                .any(|s| s.contains(['\n', '\r', '\0']))
            {
                return Err("automatic updates require paths without control characters".into());
            }
            if file().exists() {
                disable()?;
            }
            fs::create_dir_all(root()).map_err(e2s)?;
            if cfg!(target_os = "macos") {
                let log = config::dir().join("updates.log");
                fs::write(file(), plist(&exe, &home, &log.to_string_lossy())).map_err(e2s)?;
                fs::set_permissions(file(), fs::Permissions::from_mode(0o600)).map_err(e2s)?;
                run(
                    "launchctl",
                    &["bootstrap", &domain(), &file().to_string_lossy()],
                )?;
            } else {
                fs::write(root().join("tako-update.service"), unit(&exe, &home)).map_err(e2s)?;
                fs::write(file(), SCHEDULE).map_err(e2s)?;
                run("systemctl", &["--user", "daemon-reload"])?;
                run("systemctl", &["--user", "enable", "--now", TIMER])?;
            }
            println!("automatic updates: on (daily). Verified updates restart a running router; active requests may be interrupted.");
        }
        _ => return Err("usage: tako auto-update [on|off|status]".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launchd_preserves_paths_and_config_without_shell_expansion() {
        let p = plist("/a & b/tako", "/config/<local>", "/logs/updates.log");
        assert!(p.contains("<string>/a &amp; b/tako</string><string>update</string>"));
        assert!(p.contains("<string>/config/&lt;local&gt;</string>"));
        assert!(p.contains("<integer>86400</integer>"));
        assert!(!p.contains("RunAtLoad"));
    }
    #[test]
    fn systemd_preserves_spaces_and_literal_specifiers() {
        let u = unit("/a b/100%/$tool", "/config/%u/$home");
        assert!(u.contains("ExecStart=\"/a b/100%%/$$tool\" update"));
        assert!(u.contains("KillMode=process\n"));
        assert!(u.contains("Environment=\"TAKO_HOME=/config/%%u/$home\""));
        assert!(SCHEDULE.contains("Persistent=true"));
        assert!(SCHEDULE.contains("OnCalendar=daily"));
    }
}
