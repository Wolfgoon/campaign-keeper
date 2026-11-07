pub mod client;
pub mod discovery;
pub mod secure;
pub mod table;

pub const PORT: u16 = 47120;
/// The table's port. `CK_PORT` moves it, so a test can run beside a real game.
pub fn port() -> u16 {
    std::env::var("CK_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(PORT)
}

pub const BEACON_PORT: u16 = 47121;
/// Bumped when the wire format changes; a mismatch is refused with a clear message.
pub const PROTOCOL: u32 = 2;
