//! An encrypted channel keyed by the join code. SPAKE2 proves both sides know the code without
//! the code, or anything that could be guessed from it offline, crossing the network. Frames are
//! ChaCha20-Poly1305 with a counter nonce per direction, so replays and reorders fail to decrypt.
//!
//! 2.0 is not wire-compatible with 1.x: everyone at a table needs the same major version.

use std::io::{Read, Write};
use std::net::TcpStream;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use serde_json::Value;
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

const MAX_FRAME: u32 = 8 * 1024 * 1024;
const CONFIRM: &[u8] = b"campaign-keeper: key confirmed";

pub struct SealKey {
    cipher: ChaCha20Poly1305,
    counter: u64,
}

impl SealKey {
    fn new(key: &[u8; 32]) -> SealKey {
        SealKey { cipher: ChaCha20Poly1305::new(Key::from_slice(key)), counter: 0 }
    }
    fn nonce(&mut self) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&self.counter.to_le_bytes());
        self.counter += 1;
        n
    }
}

/// The join code without dashes or case, so K7Q2-M9XD and k7q2m9xd match.
pub fn normalize_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// An 8-character code from the OS's secure generator. This is a secret, not game randomness.
pub fn new_code() -> Result<String, String> {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut b = [0u8; 8];
    getrandom::getrandom(&mut b).map_err(|e| e.to_string())?;
    let s: String = b.iter().map(|x| ALPHABET[*x as usize % ALPHABET.len()] as char).collect();
    Ok(format!("{}-{}", &s[..4], &s[4..]))
}

fn write_frame(s: &mut TcpStream, data: &[u8]) -> Result<(), String> {
    s.write_all(&(data.len() as u32).to_be_bytes()).map_err(|e| e.to_string())?;
    s.write_all(data).map_err(|e| e.to_string())
}

fn read_frame(s: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len);
    if len > MAX_FRAME {
        return Err("frame has an impossible size".into());
    }
    let mut buf = vec![0u8; len as usize];
    s.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

pub struct Sender {
    stream: TcpStream,
    key: SealKey,
}

pub struct Receiver {
    stream: TcpStream,
    key: SealKey,
}

impl Sender {
    /// End the connection for both directions, so the other side finds out at once.
    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    pub fn send(&mut self, v: &Value) -> Result<(), String> {
        let n = self.key.nonce();
        let ct = self.key.cipher.encrypt(Nonce::from_slice(&n), v.to_string().as_bytes()).map_err(|_| "couldn't seal a frame")?;
        write_frame(&mut self.stream, &ct)
    }
}

impl Receiver {
    /// Set on this very handle: on Windows a timeout set through a clone doesn't reach it.
    pub fn set_timeout(&self, t: Option<std::time::Duration>) -> Result<(), String> {
        self.stream.set_read_timeout(t).map_err(|e| e.to_string())
    }

    pub fn recv(&mut self) -> Result<Value, String> {
        let ct = read_frame(&mut self.stream)?;
        let n = self.key.nonce();
        let pt = self.key.cipher.decrypt(Nonce::from_slice(&n), ct.as_slice()).map_err(|_| "a frame failed to decrypt")?;
        serde_json::from_slice(&pt).map_err(|e| e.to_string())
    }
}

/// `Err("code")` means the join codes didn't match.
pub fn handshake(mut stream: TcpStream, code: &str, host: bool) -> Result<(Sender, Receiver), String> {
    let pw = Password::new(normalize_code(code).as_bytes());
    let (a, b) = (Identity::new(b"campaign-keeper host"), Identity::new(b"campaign-keeper player"));
    let (state, msg) = if host { Spake2::<Ed25519Group>::start_a(&pw, &a, &b) } else { Spake2::<Ed25519Group>::start_b(&pw, &a, &b) };
    write_frame(&mut stream, &msg)?;
    let theirs = read_frame(&mut stream)?;
    let shared = state.finish(&theirs).map_err(|_| "That address isn't a Campaign Keeper table (or it's a different version of the app).")?;
    let hk = Hkdf::<Sha256>::new(Some(b"campaign-keeper table v2"), &shared);
    let (mut h2p, mut p2h) = ([0u8; 32], [0u8; 32]);
    hk.expand(b"host to player", &mut h2p).map_err(|_| "key derivation failed")?;
    hk.expand(b"player to host", &mut p2h).map_err(|_| "key derivation failed")?;
    let (mut tx, mut rx) = if host { (SealKey::new(&h2p), SealKey::new(&p2h)) } else { (SealKey::new(&p2h), SealKey::new(&h2p)) };

    // Each side proves it has the same key.
    let n = tx.nonce();
    let ct = tx.cipher.encrypt(Nonce::from_slice(&n), CONFIRM).map_err(|_| "couldn't seal a frame")?;
    write_frame(&mut stream, &ct)?;
    let ct = read_frame(&mut stream)?;
    let n = rx.nonce();
    let pt = rx.cipher.decrypt(Nonce::from_slice(&n), ct.as_slice()).map_err(|_| "code")?;
    if pt != CONFIRM {
        return Err("code".into());
    }
    let other = stream.try_clone().map_err(|e| e.to_string())?;
    Ok((Sender { stream, key: tx }, Receiver { stream: other, key: rx }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::TcpListener;

    fn pair(host_code: &'static str, client_code: &'static str) -> (Result<(Sender, Receiver), String>, Result<(Sender, Receiver), String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let t = std::thread::spawn(move || handshake(TcpStream::connect(addr).unwrap(), client_code, false));
        let (s, _) = l.accept().unwrap();
        let h = handshake(s, host_code, true);
        (h, t.join().unwrap())
    }

    #[test]
    fn same_code_talks_both_ways() {
        let (h, c) = pair("K7Q2-M9XD", "k7q2m9xd");
        let (mut hs, mut hr) = h.unwrap();
        let (mut cs, mut cr) = c.unwrap();
        hs.send(&json!({"t":"welcome"})).unwrap();
        assert_eq!(cr.recv().unwrap()["t"], "welcome");
        cs.send(&json!({"n":1})).unwrap();
        cs.send(&json!({"n":2})).unwrap();
        assert_eq!(hr.recv().unwrap()["n"], 1);
        assert_eq!(hr.recv().unwrap()["n"], 2);
    }

    #[test]
    fn wrong_code_is_refused_on_both_sides() {
        let (h, c) = pair("AAAA-AAAA", "BBBB-BBBB");
        assert!(h.is_err() && c.is_err());
    }

    #[test]
    fn a_quiet_player_is_not_dropped_after_the_handshake_timeout() {
        // What table.rs does: a short timeout for the handshake, then none.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let t = std::thread::spawn(move || handshake(TcpStream::connect(addr).unwrap(), "AAAA-AAAA", false).unwrap());
        let (s, _) = l.accept().unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
        let (_hs, mut hr) = handshake(s, "AAAA-AAAA", true).unwrap();
        hr.set_timeout(None).unwrap();
        let (mut cs, _cr) = t.join().unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            cs.send(&json!({"n":1})).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(1));
        });
        assert_eq!(hr.recv().unwrap()["n"], 1);
    }

    #[test]
    fn codes_look_right() {
        let c = new_code().unwrap();
        assert_eq!(c.len(), 9);
        assert_eq!(normalize_code(&c).len(), 8);
    }
}
