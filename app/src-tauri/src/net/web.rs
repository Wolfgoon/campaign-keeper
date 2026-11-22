//! Joining from a web browser. The host's app serves the player's screen as a web page, so a
//! friend on an iPhone, an iPad or anything else with a browser opens a link and plays, with
//! nothing to install. They get the same seat, rules and limits as a player using the app.
//!
//! A browser can't do the app's encrypted handshake, so this connection is plain: the join code
//! and the story cross the network unencrypted. That's fine on a home network or a VPN, and the
//! host can turn the browser link off.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use include_dir::{include_dir, Dir};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};

use super::secure::normalize_code;
use super::table::{blocked, fail_count, forgive, seat, Host, In, Out, Table};
use crate::game::Shared;

/// The app's own pages: the same build the window shows. In a browser it opens as the player's screen.
static SITE: Dir = include_dir!("$CARGO_MANIFEST_DIR/../dist");

const MAX_HEAD: usize = 8 * 1024;
const MAX_MESSAGE: usize = 8 * 1024 * 1024;

pub fn start(listener: TcpListener, app: Arc<dyn Host>, st: Shared, t: Arc<Table>) -> Result<(), String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        while !t.stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, addr)) => {
                    let _ = s.set_nonblocking(false);
                    let (a, st2, t2) = (app.clone(), st.clone(), t.clone());
                    std::thread::spawn(move || serve(a, st2, t2, s, addr));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    Ok(())
}

/// The request line's path and the headers, lower-cased names.
fn read_head(s: &mut TcpStream) -> Option<(String, Vec<(String, String)>)> {
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if buf.len() >= MAX_HEAD || s.read(&mut b).ok()? == 0 {
            return None;
        }
        buf.push(b[0]);
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    if first.next()? != "GET" {
        return None;
    }
    let path = first.next()?.to_string();
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    Some((path, headers))
}

fn kind(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn page(s: &mut TcpStream, path: &str) {
    let path = path.split(['?', '#']).next().unwrap_or("").trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let file = (!path.contains("..")).then(|| SITE.get_file(path)).flatten();
    let (status, kind, body): (&str, &str, &[u8]) = match file {
        Some(f) => ("200 OK", kind(path), f.contents()),
        None => ("404 Not Found", "text/plain; charset=utf-8", b"Not found."),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; media-src 'self' data: blob:; connect-src 'self' ws: wss:\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes()).and_then(|_| s.write_all(body));
}

/// The half of a WebSocket that sends. `mask` is for the browser's side of the conversation.
pub struct WsOut {
    pub stream: TcpStream,
    pub mask: bool,
}

impl Out for WsOut {
    fn send(&mut self, v: &Value) -> Result<(), String> {
        let data = v.to_string().into_bytes();
        let m = if self.mask { 0x80 } else { 0 };
        let mut f = vec![0x81];
        match data.len() {
            n if n < 126 => f.push(m | n as u8),
            n if n <= u16::MAX as usize => {
                f.push(m | 126);
                f.extend((n as u16).to_be_bytes());
            }
            n => {
                f.push(m | 127);
                f.extend((n as u64).to_be_bytes());
            }
        }
        if self.mask {
            // A zero key leaves the bytes as they are, and is still a valid masked frame.
            f.extend([0u8; 4]);
        }
        f.extend(data);
        self.stream.write_all(&f).map_err(|e| e.to_string())
    }

    fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// The half that receives. `first` is a message already read, handed back to be read again.
pub struct WsIn {
    pub stream: TcpStream,
    pub first: Option<Value>,
}

impl In for WsIn {
    fn recv(&mut self) -> Result<Value, String> {
        if let Some(v) = self.first.take() {
            return Ok(v);
        }
        let e = |e: std::io::Error| e.to_string();
        let mut message = Vec::new();
        loop {
            let mut h = [0u8; 2];
            self.stream.read_exact(&mut h).map_err(e)?;
            let (fin, op, masked) = (h[0] & 0x80 != 0, h[0] & 0x0f, h[1] & 0x80 != 0);
            let len = match h[1] & 0x7f {
                126 => {
                    let mut b = [0u8; 2];
                    self.stream.read_exact(&mut b).map_err(e)?;
                    u16::from_be_bytes(b) as usize
                }
                127 => {
                    let mut b = [0u8; 8];
                    self.stream.read_exact(&mut b).map_err(e)?;
                    usize::try_from(u64::from_be_bytes(b)).map_err(|_| "a message has an impossible size")?
                }
                n => n as usize,
            };
            if len > MAX_MESSAGE || message.len() + len > MAX_MESSAGE {
                return Err("a message has an impossible size".into());
            }
            let mut key = [0u8; 4];
            if masked {
                self.stream.read_exact(&mut key).map_err(e)?;
            }
            let mut data = vec![0u8; len];
            self.stream.read_exact(&mut data).map_err(e)?;
            if masked {
                data.iter_mut().enumerate().for_each(|(i, b)| *b ^= key[i % 4]);
            }
            match op {
                0x8 => return Err("the browser closed the connection".into()),
                0x9 | 0xA => continue, // ping, pong
                _ => message.extend(data),
            }
            if fin {
                return serde_json::from_slice(&message).map_err(|e| e.to_string());
            }
        }
    }

    fn set_timeout(&self, t: Option<Duration>) -> Result<(), String> {
        self.stream.set_read_timeout(t).map_err(|e| e.to_string())
    }
}

/// The reply that turns an HTTP request into a WebSocket.
pub fn accept_key(key: &str) -> String {
    use base64::Engine;
    let mut h = Sha1::new();
    h.update(key.as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

fn serve(app: Arc<dyn Host>, st: Shared, t: Arc<Table>, mut stream: TcpStream, addr: SocketAddr) {
    let ip = addr.ip();
    if blocked(&t, ip) {
        return;
    }
    let _ = stream.set_read_timeout(Some(super::REACH));
    let _ = stream.set_write_timeout(Some(super::REACH));
    let Some((path, headers)) = read_head(&mut stream) else { return };
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    if !header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket")) {
        return page(&mut stream, &path);
    }
    let Some(key) = header("sec-websocket-key") else { return };
    let reply = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n", accept_key(key));
    if stream.write_all(reply.as_bytes()).is_err() {
        return;
    }
    let Ok(other) = stream.try_clone() else { return };
    let mut tx = WsOut { stream: other, mask: false };
    let mut rx = WsIn { stream, first: None };
    // The app proves the code inside its handshake; a browser sends it with its hello.
    let Ok(hello) = rx.recv() else { return };
    if normalize_code(hello["code"].as_str().unwrap_or("")) != normalize_code(&t.code) {
        fail_count(&t, ip);
        let _ = tx.send(&json!({ "t": "refused", "reason": "The join code didn't match. Check it with the host." }));
        tx.shutdown();
        return;
    }
    forgive(&t, ip);
    rx.first = Some(hello);
    seat(&*app, &st, &t, Box::new(tx), Box::new(rx), ip);
}
