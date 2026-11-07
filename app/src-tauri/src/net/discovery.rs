//! Finding tables on the local network. A host that opts in broadcasts the campaign and its name,
//! never the join code.

use std::collections::BTreeMap;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{BEACON_PORT, PORT};

pub fn announce(campaign: String, host: String, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else { return };
        let _ = sock.set_broadcast(true);
        let msg = json!({ "app": "campaign-keeper", "campaign": campaign, "host": host, "port": PORT }).to_string();
        while !stop.load(Ordering::Relaxed) {
            let _ = sock.send_to(msg.as_bytes(), ("255.255.255.255", BEACON_PORT));
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// Listen for a few seconds and list what's out there.
pub fn scan(seconds: u64) -> Result<Vec<Value>, String> {
    let sock = UdpSocket::bind(("0.0.0.0", BEACON_PORT)).map_err(|_| "Couldn't listen for tables here (another app may be using the port). Type the host's address instead.")?;
    sock.set_read_timeout(Some(Duration::from_millis(300))).map_err(|e| e.to_string())?;
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut found = BTreeMap::new();
    let mut buf = [0u8; 1024];
    while Instant::now() < end {
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            if let Ok(v) = serde_json::from_slice::<Value>(&buf[..n]) {
                if v["app"] == "campaign-keeper" {
                    let addr = format!("{}:{}", from.ip(), v["port"].as_u64().unwrap_or(PORT as u64));
                    found.insert(addr.clone(), json!({ "addr": addr, "campaign": v["campaign"], "host": v["host"] }));
                }
            }
        }
    }
    Ok(found.into_values().collect())
}
