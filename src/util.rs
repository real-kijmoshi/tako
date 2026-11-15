use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Print a line prefixed with the local time.
#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { println!("{} {}", $crate::util::ts(), format!($($t)*)) };
}

pub fn ts() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Turn a listen address into something we can connect to (0.0.0.0 -> 127.0.0.1).
pub fn local_addr(listen: &str) -> Result<SocketAddr, String> {
    let mut a = listen
        .to_socket_addrs()
        .map_err(|e| format!("bad listen address '{listen}': {e}"))?
        .next()
        .ok_or_else(|| format!("bad listen address '{listen}'"))?;
    if a.ip().is_unspecified() {
        a.set_ip(match a.ip() {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        });
    }
    Ok(a)
}

/// Minimal blocking GET against the local daemon. Returns the body on 200.
pub fn get(listen: &str, path: &str) -> Result<String, String> {
    let addr = local_addr(listen)?;
    let mut s =
        TcpStream::connect_timeout(&addr, Duration::from_millis(500)).map_err(|e| e.to_string())?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let auth = crate::auth::local_token()
        .map(|t| format!("x-tako-local: {t}\r\n"))
        .unwrap_or_default();
    write!(s, "GET {path} HTTP/1.0\r\nHost: localhost\r\n{auth}\r\n").map_err(|e| e.to_string())?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    let (head, body) = buf.split_once("\r\n\r\n").ok_or("bad response")?;
    let status = head.lines().next().unwrap_or("");
    if status.split_whitespace().nth(1) != Some("200") {
        return Err(status.to_string());
    }
    Ok(body.to_string())
}

pub fn e2s(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Full error chain, e.g. "error sending request: connection refused".
pub fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        let m = c.to_string();
        if !s.contains(&m) {
            s.push_str(": ");
            s.push_str(&m);
        }
        cur = c.source();
    }
    s
}

pub fn short(s: &str, n: usize) -> String {
    let s = s.trim().replace('\n', " ");
    if s.chars().count() <= n {
        s
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

pub fn dur(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m{}s", s / 60, s % 60),
        s if s < 86400 => format!("{}h{}m", s / 3600, s % 3600 / 60),
        s => format!("{}d{}h", s / 86400, s % 86400 / 3600),
    }
}

fn paint(code: &str, s: &str) -> String {
    if unsafe { libc::isatty(1) } == 1 {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}
pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}
pub fn bold(s: &str) -> String {
    paint("1", s)
}

pub fn confirm(q: &str) -> bool {
    print!("{q} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut a = String::new();
    let _ = std::io::stdin().read_line(&mut a);
    matches!(a.trim(), "y" | "Y" | "yes")
}

/// Validate a release asset before it can replace the running executable.
pub fn verify_checksum(manifest: &str, asset: &str, bytes: &[u8]) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let entries: Vec<_> = manifest
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            (name == asset && parts.next().is_none()).then_some(hash)
        })
        .collect();
    if entries.len() != 1
        || entries[0].len() != 64
        || !entries[0].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(format!(
            "No unique valid SHA-256 checksum for {asset}; update cancelled"
        ));
    }
    let actual: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !actual.eq_ignore_ascii_case(entries[0]) {
        return Err("Downloaded binary failed SHA-256 verification; update cancelled".into());
    }
    Ok(())
}

#[cfg(test)]
mod release_tests {
    use super::*;
    #[test]
    fn release_checksum_rejects_modified_missing_and_ambiguous_assets() {
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let manifest = format!("{hash}  tako-linux-x86_64\n");
        assert!(verify_checksum(&manifest, "tako-linux-x86_64", b"abc").is_ok());
        assert!(verify_checksum(&manifest, "tako-linux-x86_64", b"modified").is_err());
        assert!(verify_checksum(&manifest, "tako-macos-aarch64", b"abc").is_err());
        assert!(verify_checksum(&manifest.repeat(2), "tako-linux-x86_64", b"abc").is_err());
    }
}
