//! Pairing a device with a table in one paste. An invitation carries the address, the port and the
//! join code in one short line, so someone on a phone doesn't type all three, and over Tailscale
//! the address in it is the host's Tailscale name.
//!
//! `campaignkeeper://join?host=my-pc&port=47120&code=K7Q2-M9XD`
//!
//! Nothing secret is added: the code in it is the code the host would have read out anyway, and
//! the handshake still proves both sides know it.

use std::net::IpAddr;

use super::secure::normalize_code;

pub const SCHEME: &str = "campaignkeeper://";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    /// What `client::targets` takes: a name, an IP, or either followed by a port.
    pub addr: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The invitation to paste.
    pub text: String,
    /// The address it carries.
    pub host: String,
    /// "Tailscale", "Tailscale (address)" or "your network", for the line under it.
    pub over: &'static str,
}

/// K7Q2M9XD -> K7Q2-M9XD, so a code stays easy to read out loud.
fn pretty_code(code: &str) -> String {
    let c = normalize_code(code);
    if c.len() == 8 {
        format!("{}-{}", &c[..4], &c[4..])
    } else {
        c
    }
}

pub fn invite(host: &str, port: u16, code: &str) -> String {
    format!("{SCHEME}join?host={}&port={port}&code={}", encode(host.trim()), pretty_code(code))
}

/// Read an invitation out of whatever the host sent: the link above, or their older message
/// "open http://my-pc:47122 and enter the code K7Q2-M9XD".
pub fn parse(text: &str) -> Result<Invite, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Paste the invitation the host sent you.".into());
    }
    let lower = text.to_ascii_lowercase();
    if let Some(at) = lower.find(SCHEME) {
        return from_link(&text[at + SCHEME.len()..]);
    }
    from_message(text).ok_or_else(|| {
        "That doesn't look like an invitation. Ask the host to copy theirs from the table's Pairing panel.".to_string()
    })
}

fn from_link(rest: &str) -> Result<Invite, String> {
    let rest = rest.split(|c: char| c.is_whitespace() || "\"'<>".contains(c)).next().unwrap_or("");
    let query = rest.split_once('?').map(|(_, q)| q).unwrap_or(rest);
    let (mut host, mut port, mut code) = (None, None, None);
    for part in query.split(['&', ';']) {
        let Some((k, v)) = part.split_once('=') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "host" | "addr" | "address" => host = Some(decode(v)),
            "port" => port = v.trim().parse::<u16>().ok(),
            "code" | "join" => code = Some(decode(v)),
            _ => {}
        }
    }
    let host = host.ok_or("That invitation has no address in it. Ask the host to copy it again.")?;
    let code = code.ok_or("That invitation has no join code in it. Ask the host to copy it again.")?;
    build(host, port, code)
}

fn from_message(text: &str) -> Option<Invite> {
    let lower = text.to_ascii_lowercase();
    let at = lower.find("http://").or_else(|| lower.find("https://"))?;
    let url = text[at..].split_whitespace().next()?;
    let host = url.split_once("://")?.1.split(['/', '?', '#']).next()?.trim_end_matches('.');
    let (name, port) = match host.rsplit_once(':') {
        Some((n, p)) => (n, p.parse::<u16>().ok()),
        None => (host, None),
    };
    // A browser link answers on the web port, not the app's: an address without a port is resolved
    // with the app's own, so only keep a port the app would actually use.
    let port = port.filter(|p| *p == super::port());
    let code = after_word(text, "code")?;
    build(name.to_string(), port, code).ok()
}

/// The first word after `word` that could be a join code.
fn after_word(text: &str, word: &str) -> Option<String> {
    let at = text.to_ascii_lowercase().find(word)? + word.len();
    let rest = text[at..].trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    let token = rest.split(|c: char| !c.is_ascii_alphanumeric() && c != '-').next()?;
    (normalize_code(token).len() == 8).then(|| token.to_string())
}

fn build(host: String, port: Option<u16>, code: String) -> Result<Invite, String> {
    let code = normalize_code(&code);
    if code.len() != 8 {
        return Err("The join code has 8 letters and numbers, like K7Q2-M9XD.".into());
    }
    let host = clean_host(&host)?;
    let has_port = host.parse::<std::net::SocketAddr>().is_ok();
    let addr = match port {
        Some(p) if !has_port && p != super::port() => {
            let bare = host.trim_matches(['[', ']']);
            if bare.parse::<IpAddr>().map(|ip| ip.is_ipv6()).unwrap_or(false) {
                format!("[{bare}]:{p}")
            } else {
                format!("{host}:{p}")
            }
        }
        _ => host,
    };
    Ok(Invite { addr, code: pretty_code(&code) })
}

fn clean_host(raw: &str) -> Result<String, String> {
    let mut h = raw.trim().trim_end_matches('.');
    // Someone may paste the whole browser link the host's other card shows.
    if let Some((_, tail)) = h.split_once("://") {
        h = tail.split(['/', '?', '#']).next().unwrap_or("").trim_end_matches('.');
    }
    let ok = !h.is_empty()
        && (h.parse::<IpAddr>().is_ok()
            || h.parse::<std::net::SocketAddr>().is_ok()
            || h.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
            || (h.starts_with('[') && h.contains(']')));
    if !ok {
        return Err("That invitation's address doesn't look right. Ask the host to copy it again.".into());
    }
    Ok(h.to_string())
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&String::from_utf8_lossy(&b[i + 1..i + 3]), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Tailscale hands out addresses from 100.64.0.0/10.
pub fn is_tailscale(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (64..=127).contains(&o[1])
        }
        IpAddr::V6(_) => false,
    }
}

/// This device's address on Tailscale, when it's running: aim a socket at Tailscale's own DNS
/// server and see which local address it would use. Nothing is sent.
pub fn tailscale_ip() -> Option<IpAddr> {
    super::ip_towards("100.100.100.100:53").filter(|ip| is_tailscale(*ip))
}

/// The name Tailscale gives a device: the computer's own name, lower-cased. MagicDNS resolves it
/// for anyone signed in to the same tailnet.
pub fn hostname() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok()?;
    let h: String = raw
        .trim()
        .trim_end_matches('.')
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    (!h.is_empty()).then_some(h)
}

/// Every way a friend could reach this table, best first: over Tailscale by name, then by
/// Tailscale's address, then this computer on the local network.
pub fn routes(port: u16, code: &str) -> Vec<Route> {
    let mut out: Vec<Route> = Vec::new();
    let mut add = |host: String, over: &'static str| {
        if !out.iter().any(|r: &Route| r.host.eq_ignore_ascii_case(&host)) {
            out.push(Route { text: invite(&host, port, code), host, over });
        }
    };
    if let Some(ip) = tailscale_ip() {
        if let Some(name) = hostname() {
            add(name, "Tailscale");
        }
        add(ip.to_string(), "Tailscale (address)");
    }
    if let Some(ip) = super::local_ip() {
        add(ip.to_string(), "your network");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invitation_carries_the_address_the_port_and_the_code() {
        let p = super::super::port();
        let text = invite("my-pc", p, "k7q2m9xd");
        assert_eq!(text, format!("campaignkeeper://join?host=my-pc&port={p}&code=K7Q2-M9XD"));
        assert_eq!(parse(&text).unwrap(), Invite { addr: "my-pc".into(), code: "K7Q2-M9XD".into() });
    }

    #[test]
    fn a_port_that_is_not_the_tables_travels_in_the_address() {
        let other = if super::super::port() == 47120 { 47199 } else { 47120 };
        let v4 = parse(&invite("192.168.1.20", other, "K7Q2M9XD")).unwrap();
        assert_eq!(v4.addr, format!("192.168.1.20:{other}"));
        // An IPv6 address needs brackets once a port follows it.
        let v6 = parse(&invite("fe80::1", other, "K7Q2M9XD")).unwrap();
        assert_eq!(v6.addr, format!("[fe80::1]:{other}"));
    }

    #[test]
    fn the_older_invitation_message_still_fills_both_fields() {
        let msg = "Join my Campaign Keeper game: open http://my-pc:47122 and enter the code K7Q2-M9XD.";
        let inv = parse(msg).unwrap();
        assert_eq!(inv.addr, "my-pc");
        assert_eq!(inv.code, "K7Q2-M9XD");
        // A link carrying the app's own port keeps it.
        let p = super::super::port();
        let app = parse(&format!("open http://192.168.1.20:{p} and enter the code k7q2m9xd")).unwrap();
        assert_eq!(app.addr, "192.168.1.20");
    }

    #[test]
    fn junk_is_refused_with_something_a_player_can_act_on() {
        for text in ["", "   ", "hello", "https://example.com/nothing", "campaignkeeper://join?host=my-pc"] {
            assert!(parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn an_address_that_isnt_an_address_is_refused() {
        assert!(parse("campaignkeeper://join?host=two words&code=K7Q2-M9XD").is_err());
        assert!(parse("campaignkeeper://join?host=../etc&code=K7Q2-M9XD").is_err());
        assert!(parse("campaignkeeper://join?host=my-pc&code=OOPS").is_err());
        assert_eq!(parse("campaignkeeper://join?host=%5Bfe80%3A%3A1%5D&code=K7Q2M9XD").unwrap().addr, "[fe80::1]");
    }

    #[test]
    fn tailscale_owns_the_100_64_range() {
        assert!(is_tailscale("100.64.0.1".parse().unwrap()));
        assert!(is_tailscale("100.127.255.254".parse().unwrap()));
        assert!(!is_tailscale("100.63.255.255".parse().unwrap()));
        assert!(!is_tailscale("100.128.0.1".parse().unwrap()));
        assert!(!is_tailscale("192.168.1.2".parse().unwrap()));
        assert!(!is_tailscale("fd7a:115c:a1e0::1".parse().unwrap()));
    }

    #[test]
    fn every_route_we_show_parses_back_to_the_same_code() {
        let routes = routes(super::super::port(), "K7Q2M9XD");
        for r in &routes {
            let inv = parse(&r.text).unwrap_or_else(|e| panic!("{}: {e}", r.text));
            assert_eq!(inv.code, "K7Q2-M9XD");
            assert_eq!(inv.addr, r.host);
        }
        assert!(routes.len() <= 3);
    }
}
