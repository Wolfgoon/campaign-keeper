//! Joining someone else's table. Everything arrives as messages from the host and is handed to
//! the window as it comes.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender as Tx};
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
    let target = if addr.contains(':') { addr.trim().to_string() } else { format!("{}:{}", addr.trim(), port()) };
    let sock = target.to_socket_addrs().map_err(|e| format!("{target}: {e}"))?.next().ok_or("Enter the host's address.")?;
    let stream = TcpStream::connect_timeout(&sock, Duration::from_secs(8)).map_err(|_| {
        format!("{target} didn't answer. Check that you're on the same network (or VPN) and that the host's firewall lets Campaign Keeper accept connections.")
    })?;
    let (mut tx, mut rx) = handshake(stream, code, false).map_err(|e| if e == "code" { "The join code didn't match. Check it with the host.".to_string() } else { e })?;
    tx.send(&json!({ "t": "hello", "name": name.trim(), "token": token(), "protocol": PROTOCOL, "app": env!("CARGO_PKG_VERSION") }))?;

    let (out_tx, out_rx) = channel::<Value>();
    let left = Arc::new(AtomicBool::new(false));
    let left2 = left.clone();
    std::thread::spawn(move || {
        while let Ok(v) = out_rx.recv() {
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
        let reason = loop {
            match rx.recv() {
                Ok(v) if v["t"] == "ping" => {}
                Ok(v) => {
                    let done = matches!(v["t"].as_str(), Some("goodbye" | "refused"));
                    let _ = app.emit("client", v);
                    if done {
                        return;
                    }
                }
                Err(_) if left.load(Ordering::SeqCst) => return,
                Err(_) => break "The host closed the connection. The table may be full, or this computer may be blocked after too many wrong join codes.",
            }
        };
        let _ = app.emit("client", json!({ "t": "goodbye", "reason": reason }));
    });
    Ok(out_tx)
}
