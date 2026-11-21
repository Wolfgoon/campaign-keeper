//! Joining someone else's table. Everything arrives as messages from the host and is handed to
//! the window as it comes.

use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender as Tx};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use super::secure::{handshake, normalize_code};
use super::{port, PROTOCOL};
use crate::config::config_dir;
use crate::util::atomic_write;

/// Who this computer is to a table, so a dropped player gets their seat back.
fn token() -> String {
    let p = config_dir().join("client_token");
    if let Ok(t) = std::fs::read_to_string(&p) {
        if !t.trim().is_empty() {
            return t.trim().to_string();
        }
    }
    let t = uuid::Uuid::new_v4().to_string();
    let _ = atomic_write(&p, t.as_bytes());
    t
}

/// The host sends a heartbeat every 15 seconds, so this much silence means the line is dead.
const QUIET: Duration = Duration::from_secs(45);
const HEARTBEAT: Duration = Duration::from_secs(15);

/// Every address the host's name could mean, IPv4 first: tables listen on IPv4, and a VPN name
/// often resolves to both kinds.
pub fn targets(addr: &str) -> Result<Vec<SocketAddr>, String> {
    let a = addr.trim();
    if let Ok(ip) = a.trim_matches(|c| c == '[' || c == ']').parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port())]);
    }
    if let Ok(sa) = a.parse::<SocketAddr>() {
        return Ok(vec![sa]);
    }
    let target = if a.contains(':') { a.to_string() } else { format!("{a}:{}", port()) };
    let mut v: Vec<SocketAddr> = target.to_socket_addrs().map_err(|e| format!("{target}: {e}"))?.collect();
    v.sort_by_key(|s| s.is_ipv6());
    if v.is_empty() {
        return Err("Enter the host's address.".into());
    }
    Ok(v)
}

pub fn join(app: AppHandle, addr: &str, code: &str, name: &str) -> Result<Tx<Value>, String> {
    if addr.trim().is_empty() {
        return Err("Enter the host's address.".into());
    }
    if normalize_code(code).len() != 8 {
        return Err("The join code has 8 letters and numbers, like K7Q2-M9XD.".into());
    }
    if name.trim().is_empty() {
        return Err("Enter your name to join.".into());
    }
    let stream = targets(addr)?.into_iter().find_map(|sock| TcpStream::connect_timeout(&sock, Duration::from_secs(6)).ok()).ok_or_else(|| {
        format!("{} didn't answer. Check that you're on the same network (or VPN) and that the host's firewall lets Campaign Keeper accept connections.", addr.trim())
    })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let (mut tx, mut rx) = handshake(stream, code, false).map_err(|e| if e == "code" { "The join code didn't match. Check it with the host.".to_string() } else { e })?;
    tx.send(&json!({ "t": "hello", "name": name.trim(), "token": token(), "protocol": PROTOCOL, "app": env!("CARGO_PKG_VERSION"), "pings": true }))?;
    rx.set_timeout(Some(QUIET))?;

    let (out_tx, out_rx) = channel::<Value>();
    let left = Arc::new(AtomicBool::new(false));
    let left2 = left.clone();
    std::thread::spawn(move || {
        loop {
            // A heartbeat tells the host this player is still here while they think.
            let v = match out_rx.recv_timeout(HEARTBEAT) {
                Ok(v) => v,
                Err(RecvTimeoutError::Timeout) => json!({ "t": "ping" }),
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if tx.send(&v).is_err() {
                break;
            }
        }
        // The window dropped its end: the player left. Hang up so the host knows, and so the
        // reader below stops instead of feeding a closed table into their next game.
        left2.store(true, Ordering::SeqCst);
        tx.shutdown();
    });
    std::thread::spawn(move || {
        // Once the host has spoken, a dead line is a lost connection the page may retry.
        let mut seen = false;
        loop {
            match rx.recv() {
                Ok(v) if v["t"] == "ping" => {}
                Ok(v) => {
                    seen = true;
                    let done = matches!(v["t"].as_str(), Some("goodbye" | "refused"));
                    let _ = app.emit("client", v);
                    if done {
                        return;
                    }
                }
                Err(_) if left.load(Ordering::SeqCst) => return,
                Err(_) => break,
            }
        }
        let reason = if seen {
            "The connection to the host was lost."
        } else {
            "The host closed the connection. The table may be full, or this device may be blocked after too many wrong join codes."
        };
        let _ = app.emit("client", json!({ "t": "goodbye", "reason": reason, "lost": seen }));
    });
    Ok(out_tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_of_every_shape_resolve() {
        let p = port();
        assert_eq!(targets(" 192.168.1.20 ").unwrap(), vec![format!("192.168.1.20:{p}").parse().unwrap()]);
        assert_eq!(targets("192.168.1.20:5000").unwrap(), vec!["192.168.1.20:5000".parse().unwrap()]);
        // A bare IPv6 address has colons but no port.
        assert_eq!(targets("fe80::1").unwrap(), vec![format!("[fe80::1]:{p}").parse().unwrap()]);
        assert_eq!(targets("[fe80::1]:5000").unwrap(), vec!["[fe80::1]:5000".parse().unwrap()]);
        let local = targets("localhost").unwrap();
        assert!(!local.is_empty() && local.iter().all(|a| a.port() == p));
        assert!(!local[0].is_ipv6() || local.iter().all(|a| a.is_ipv6()));
    }
}
