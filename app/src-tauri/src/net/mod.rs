pub mod client;
pub mod discovery;
pub mod pair;
pub mod secure;
pub mod table;
pub mod web;
#[cfg(test)]
mod table_tests;

pub const PORT: u16 = 47120;
/// The table's port. `CK_PORT` moves it, so a test can run beside a real game.
pub fn port() -> u16 {
    std::env::var("CK_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(PORT)
}

pub const BEACON_PORT: u16 = 47121;
/// Where friends without the app join from a web browser. `CK_WEB_PORT` moves it.
pub const WEB_PORT: u16 = 47122;
pub fn web_port() -> u16 {
    std::env::var("CK_WEB_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(WEB_PORT)
}

/// Which local address a socket aimed at `target` would use. Nothing is sent: aiming a socket at
/// an outside address just reveals which interface it would pick.
pub fn ip_towards(target: &str) -> Option<std::net::IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect(target).ok()?;
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified() && !ip.is_loopback())
}

/// This device's address on the local network, to show the host what to tell their friends.
pub fn local_ip() -> Option<std::net::IpAddr> {
    ip_towards("192.0.2.1:9")
}
/// Bumped when the wire format changes; a mismatch is refused with a clear message.
pub const PROTOCOL: u32 = 3;
