//! Hosting a table. The host's app runs the game and the AI; players connect from their own copy.
//! The GM waits for one action from every player, then gets them all as one round.
//!
//! What crosses the network is only what players may see: finished story text, pictures, their own
//! status window. Never the raw model text, so the GM's notebook and recalled memories stay home.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender as Tx};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use super::secure::{handshake, new_code};
use super::{discovery, port, PROTOCOL};
use crate::campaign;
use crate::game::{self, Shared};
use crate::rpg;

const MAX_SEATS: usize = 12;
const MAX_FAILS: u32 = 5;
const MAX_NEW_CHARACTERS: u32 = 2;
/// Room for a drawing, which arrives as SVG.
const MAX_ACTION: usize = 200_000;
const HEARTBEAT: Duration = Duration::from_secs(15);

#[derive(Serialize, Clone, Default)]
pub struct Seat {
    pub id: u64,
    pub name: String,
    pub character: String,
    pub connected: bool,
    pub admitted: bool,
    pub acted: bool,
    pub passed: bool,
    #[serde(skip)]
    pub action: Option<String>,
    #[serde(skip)]
    pub token: String,
    #[serde(skip)]
    pub made: u32,
    #[serde(skip)]
    pub addr: Option<IpAddr>,
    #[serde(skip)]
    pub tx: Option<Tx<Value>>,
    /// Which connection holds this seat, so an old one dying can't unseat a player who rejoined.
    #[serde(skip)]
    pub conn: u64,
}

#[derive(Default)]
pub struct Inner {
    pub seats: Vec<Seat>,
    pub host_action: Option<String>,
    pub narration: Option<String>,
    pub talk: Vec<Value>,
    pub blocked: Vec<IpAddr>,
    pub fails: HashMap<IpAddr, u32>,
    pub next_id: u64,
    pub next_conn: u64,
    pub round: u64,
    pub last_sent: usize,
}

pub struct Table {
    pub code: String,
    pub host_name: String,
    pub host_character: Mutex<String>,
    pub host_is_gm: bool,
    pub auto_admit: AtomicBool,
    pub stop: Arc<AtomicBool>,
    pub inner: Mutex<Inner>,
}

impl Table {
    fn broadcast(&self, v: &Value) {
        if let Ok(g) = self.inner.lock() {
            for s in g.seats.iter().filter(|s| s.admitted) {
                if let Some(tx) = &s.tx {
                    let _ = tx.send(v.clone());
                }
            }
        }
    }

    pub fn state(&self) -> Value {
        let g = self.inner.lock().map(|g| {
            json!({
                "seats": g.seats, "round": g.round, "talk": g.talk,
                "host_action": g.host_action.is_some(), "narration": g.narration.is_some()
            })
        }).unwrap_or_default();
        json!({
            "code": self.code, "host": self.host_name, "host_is_gm": self.host_is_gm, "port": port(),
            "host_character": self.host_character.lock().map(|c| c.clone()).unwrap_or_default(),
            "auto_admit": self.auto_admit.load(Ordering::Relaxed), "table": g,
        })
    }
}

/// Characters that exist in this campaign.
pub fn characters(st: &Shared) -> Vec<String> {
    let Ok(s) = st.lock() else { return Vec::new() };
    let mut v: Vec<String> = s.rules.sheets.keys().cloned().collect();
    if let Some(dir) = &s.dir {
        for f in campaign::files(dir) {
            if let Some(n) = f.strip_prefix("01_Character/") {
                if !n.starts_with('_') {
                    let name = n.trim_end_matches(".md").replace('_', " ");
                    let pretty: Vec<String> = name.split(' ').map(|w| {
                        let mut c = w.chars();
                        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                    }).collect();
                    let pretty = pretty.join(" ");
                    if !v.iter().any(|x| x.eq_ignore_ascii_case(&pretty)) {
                        v.push(pretty);
                    }
                }
            }
        }
    }
    v
}

fn notify(app: &AppHandle) {
    let _ = app.emit("table", json!(null));
}

/// What players may see of the chat.
fn safe_entries(st: &Shared, table: &Table) -> Vec<Value> {
    let Ok(s) = st.lock() else { return Vec::new() };
    let start = s.session.entries.len().saturating_sub(80);
    let _ = table;
    s.session.entries[start..]
        .iter()
        .filter(|e| !e.flag("private") && (e.role == "assistant" || e.role == "user"))
        .map(|e| {
            let text = e.display.clone().unwrap_or_else(|| if e.role == "assistant" { crate::tags::parse(&e.plain()).display } else { e.label.clone().unwrap_or_default() });
            json!({
                "role": e.role, "text": text, "label": e.label, "pictures": e.extra.get("pictures"),
                "music": e.extra.get("music"), "voices": e.extra.get("voices"), "rpg": e.rpg
            })
        })
        .collect()
}

fn table_for(st: &Shared, t: &Table, character: &str, you: u64) -> Value {
    let (sheet, activities, busy) = match st.lock() {
        Ok(s) => (
            s.rules.sheets.iter().find(|(k, _)| k.eq_ignore_ascii_case(character)).map(|(_, v)| v.clone()),
            s.session.activities.iter().filter(|a| a["for"].as_str().is_none_or(|f| f.eq_ignore_ascii_case(character))).cloned().collect::<Vec<_>>(),
            s.busy,
        ),
        Err(_) => (None, Vec::new(), false),
    };
    json!({ "t": "table", "you": you, "busy": busy, "state": t.state(), "characters": characters(st), "sheet": sheet, "activities": activities })
}

/// Push the current picture to everyone. Called on every game event.
pub fn sync(st: &Shared, t: &Arc<Table>) {
    let entries = safe_entries(st, t);
    t.broadcast(&json!({ "t": "transcript", "entries": entries }));
    let seats: Vec<(Option<Tx<Value>>, String, u64)> = t.inner.lock().map(|g| g.seats.iter().filter(|s| s.admitted).map(|s| (s.tx.clone(), s.character.clone(), s.id)).collect()).unwrap_or_default();
    for (tx, c, id) in seats {
        if let Some(tx) = tx {
            let _ = tx.send(table_for(st, t, &c, id));
        }
    }
}

pub fn relay_stream(t: &Table, text: &Value) {
    t.broadcast(&json!({ "t": "stream", "text": text }));
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Names reach file names and the GM's prompts, so they're kept to what a name needs.
fn clean_name(s: &str, max: usize) -> String {
    let c: String = s.chars().filter(|c| c.is_alphanumeric() || " -'.".contains(*c)).take(max).collect();
    c.split_whitespace().collect::<Vec<_>>().join(" ").trim_matches('.').trim().to_string()
}

fn file_slug(name: &str) -> String {
    name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect()
}

/// What a player may not send: narration and the host's commands.
fn refused_action(a: &str) -> Option<String> {
    let t = a.trim();
    if t.starts_with('~') {
        return Some("Only the GM can narrate. Use > to act or \" to speak.".into());
    }
    let word = t.split_whitespace().next().unwrap_or("");
    if word.starts_with('/') && !["/status", "/inventory", "/map", "/recap", "/scene", "/help", "/ooc"].contains(&word) {
        return Some(format!("\"{word}\" is for the host. Players can use /status, /inventory, /map, /recap, /scene, /help and /ooc."));
    }
    None
}

/// Everyone, the host included, has had their say? A round never goes out behind the host's back:
/// a host with no character yet, or a human GM with nothing to add, sends it with "Send now".
fn all_in(host_is_gm: bool, g: &Inner) -> bool {
    let host_done = if host_is_gm { g.narration.is_some() } else { g.host_action.is_some() };
    host_done && g.seats.iter().filter(|s| s.admitted && s.connected && !s.character.is_empty()).all(|s| s.action.is_some() || s.passed)
}

fn ready(t: &Table, g: &Inner) -> bool {
    all_in(t.host_is_gm, g)
}

/// Send the round to the GM. `force` goes ahead without those who haven't acted.
pub fn try_round(app: &AppHandle, st: &Shared, t: &Arc<Table>, force: bool) -> Result<bool, String> {
    let (text, actions) = {
        let mut g = t.inner.lock().map_err(|_| "table lost")?;
        if !force && !ready(t, &g) {
            return Ok(false);
        }
        if g.host_action.is_none() && g.narration.is_none() && !g.seats.iter().any(|s| s.action.is_some() || s.passed) {
            return if force { Err("Nobody has acted yet.".into()) } else { Ok(false) };
        }
        let mut x = String::from("<round>\n");
        let mut actions = Vec::new();
        let hc = t.host_character.lock().map(|c| c.clone()).unwrap_or_default();
        if !hc.is_empty() && !t.host_is_gm {
            match g.host_action.take() {
                Some(a) => {
                    x.push_str(&format!("<action character=\"{}\" player=\"{}\">{}</action>\n", esc(&hc), esc(&t.host_name), esc(&a)));
                    actions.push((hc.clone(), a));
                }
                None => x.push_str(&format!("<absent character=\"{}\"/>\n", esc(&hc))),
            }
        }
        for s in g.seats.iter_mut().filter(|s| s.admitted && !s.character.is_empty()) {
            match s.action.take() {
                Some(a) => {
                    x.push_str(&format!("<action character=\"{}\" player=\"{}\">{}</action>\n", esc(&s.character), esc(&s.name), esc(&a)));
                    actions.push((s.character.clone(), a));
                }
                None if s.passed => x.push_str(&format!("<action character=\"{}\" player=\"{}\" pass=\"true\"/>\n", esc(&s.character), esc(&s.name))),
                None => x.push_str(&format!("<absent character=\"{}\"/>\n", esc(&s.character))),
            }
            s.passed = false;
            s.acted = false;
        }
        if let Some(n) = g.narration.take() {
            x.push_str(&format!("<narration by=\"GM\">{}</narration>\n", esc(&n)));
        }
        x.push_str("</round>");
        g.round += 1;
        (x, actions)
    };
    game::send_round(app, st, text, actions)?;
    notify(app);
    Ok(true)
}

fn fail_count(t: &Table, ip: IpAddr) {
    if let Ok(mut g) = t.inner.lock() {
        let n = g.fails.entry(ip).or_insert(0);
        *n += 1;
        if *n >= MAX_FAILS && !g.blocked.contains(&ip) {
            g.blocked.push(ip);
        }
    }
}

fn serve(app: AppHandle, st: Shared, t: Arc<Table>, stream: TcpStream, addr: SocketAddr) {
    let ip = addr.ip();
    if t.inner.lock().map(|g| g.blocked.contains(&ip)).unwrap_or(true) {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let (mut tx, mut rx) = match handshake(stream, &t.code, true) {
        Ok(p) => p,
        Err(e) => {
            if e == "code" {
                fail_count(&t, ip);
            }
            return;
        }
    };
    let Ok(hello) = rx.recv() else { return };
    if hello["t"] != "hello" {
        return;
    }
    if hello["protocol"].as_u64() != Some(PROTOCOL as u64) {
        let _ = tx.send(&json!({ "t": "refused", "reason": "This table runs a different version of Campaign Keeper. Update the app to join." }));
        return;
    }
    // The handshake's limit ends here: a player thinking for an hour sends nothing, and that's fine.
    if rx.set_timeout(None).is_err() {
        return;
    }
    let name = clean_name(hello["name"].as_str().unwrap_or("Player"), 40);
    if name.is_empty() {
        let _ = tx.send(&json!({ "t": "refused", "reason": "Enter your name to join." }));
        return;
    }
    let token = hello["token"].as_str().unwrap_or("").to_string();

    let (out_tx, out_rx) = channel::<Value>();
    let mut back = None;
    let (id, conn) = {
        let Ok(mut g) = t.inner.lock() else { return };
        g.next_conn += 1;
        let conn = g.next_conn;
        // A returning player gets their seat back; their old connection gives way.
        if let Some(s) = g.seats.iter_mut().find(|s| s.token == token && !token.is_empty()) {
            if let Some(old) = &s.tx {
                let _ = old.send(json!({ "t": "goodbye", "reason": "You rejoined from a new connection." }));
            }
            s.tx = Some(out_tx.clone());
            s.connected = true;
            s.addr = Some(ip);
            s.conn = conn;
            if !s.character.is_empty() {
                back = Some(s.character.clone());
            }
            (s.id, conn)
        } else {
            if g.seats.len() >= MAX_SEATS {
                let _ = tx.send(&json!({ "t": "refused", "reason": "The table is full." }));
                return;
            }
            g.next_id += 1;
            let id = g.next_id;
            let admitted = t.auto_admit.load(Ordering::Relaxed);
            g.seats.push(Seat { id, name, connected: true, admitted, token, addr: Some(ip), tx: Some(out_tx.clone()), conn, ..Default::default() });
            (id, conn)
        }
    };
    if let Some(c) = back {
        table_update(&st, format!("{c}'s player is back at the table."));
    }
    notify(&app);
    let writer = std::thread::spawn(move || {
        loop {
            // A heartbeat keeps VPNs and routers from dropping a quiet connection.
            let v = match out_rx.recv_timeout(HEARTBEAT) {
                Ok(v) => v,
                Err(RecvTimeoutError::Timeout) => json!({ "t": "ping" }),
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let bye = matches!(v["t"].as_str(), Some("goodbye" | "refused"));
            if tx.send(&v).is_err() || bye {
                break;
            }
        }
        // Hang up, so the read below ends too and nothing lingers after a removal.
        tx.shutdown();
    });
    let admitted = t.inner.lock().map(|g| g.seats.iter().any(|s| s.id == id && s.admitted)).unwrap_or(false);
    let _ = out_tx.send(json!({ "t": if admitted { "welcome" } else { "waiting" } }));
    if admitted {
        welcome(&st, &t, id);
    }

    while let Ok(msg) = rx.recv() {
        let admitted = t.inner.lock().map(|g| g.seats.iter().any(|s| s.id == id && s.admitted)).unwrap_or(false);
        if !admitted {
            continue;
        }
        handle(&app, &st, &t, id, &msg);
    }
    if let Ok(mut g) = t.inner.lock() {
        mark_gone(&mut g, id, conn);
        // Only a real goodbye: not an old connection after a rejoin.
        let left = g.seats.iter().find(|s| s.id == id && !s.connected && !s.character.is_empty()).map(|s| s.character.clone());
        drop(g);
        if let Some(c) = left {
            table_update(&st, format!("{c}'s player left the table. Keep {c} safe in the background."));
        }
    }
    drop(out_tx);
    let _ = writer.join();
    notify(&app);
    let _ = try_round(&app, &st, &t, false);
}

/// This connection ended. If the player has already rejoined on a newer one, leave them seated.
fn mark_gone(g: &mut Inner, id: u64, conn: u64) {
    if let Some(s) = g.seats.iter_mut().find(|s| s.id == id && s.conn == conn) {
        s.connected = false;
        s.tx = None;
    }
}

fn welcome(st: &Shared, t: &Arc<Table>, id: u64) {
    let c = t.inner.lock().ok().and_then(|g| g.seats.iter().find(|s| s.id == id).map(|s| (s.tx.clone(), s.character.clone())));
    if let Some((Some(tx), character)) = c {
        let _ = tx.send(json!({ "t": "transcript", "entries": safe_entries(st, t) }));
        let _ = tx.send(table_for(st, t, &character, id));
    }
}

fn handle(app: &AppHandle, st: &Shared, t: &Arc<Table>, id: u64, msg: &Value) {
    match msg["t"].as_str().unwrap_or("") {
        "claim" => {
            let character = clean_name(msg["character"].as_str().unwrap_or(""), 60);
            let new = msg["new"].as_bool().unwrap_or(false);
            let known = characters(st);
            let exists = known.iter().any(|c| c.eq_ignore_ascii_case(&character));
            if let Ok(mut g) = t.inner.lock() {
                let host_has = t.host_character.lock().map(|c| c.eq_ignore_ascii_case(&character)).unwrap_or(true);
                let taken = host_has || g.seats.iter().any(|s| s.id != id && s.character.eq_ignore_ascii_case(&character));
                let Some(seat) = g.seats.iter_mut().find(|s| s.id == id) else { return };
                let reply = |m: &str| json!({ "t": "notice", "text": m });
                if character.is_empty() || (!exists && !new) {
                    if let Some(tx) = &seat.tx {
                        let _ = tx.send(reply("That character isn't in this campaign."));
                    }
                    return;
                }
                if taken {
                    if let Some(tx) = &seat.tx {
                        let _ = tx.send(reply("Someone else plays this character"));
                    }
                    return;
                }
                if !exists {
                    if seat.made >= MAX_NEW_CHARACTERS {
                        if let Some(tx) = &seat.tx {
                            let _ = tx.send(reply("You've made enough new characters for now. Pick one, or ask the host."));
                        }
                        return;
                    }
                    seat.made += 1;
                }
                seat.character = character.clone();
            }
            ready_character(st, &character, !exists);
            let player = t.inner.lock().ok().and_then(|g| g.seats.iter().find(|s| s.id == id).map(|s| s.name.clone())).unwrap_or_default();
            table_update(st, format!("{player} now plays {character}. Bring them in at the next natural moment."));
            sync(st, t);
            notify(app);
        }
        "spend" => {
            // Only ever on the player's own character.
            let character = t.inner.lock().ok().and_then(|g| g.seats.iter().find(|s| s.id == id).map(|s| s.character.clone())).unwrap_or_default();
            let (kind, name) = (msg["kind"].as_str().unwrap_or(""), msg["name"].as_str().unwrap_or(""));
            if let Err(e) = spend(st, &character, kind, name) {
                if let Ok(g) = t.inner.lock() {
                    if let Some(tx) = g.seats.iter().find(|s| s.id == id).and_then(|s| s.tx.as_ref()) {
                        let _ = tx.send(json!({ "t": "notice", "text": e }));
                    }
                }
            }
            sync(st, t);
        }
        "submit" => {
            let action: String = msg["action"].as_str().unwrap_or("").chars().take(MAX_ACTION).collect();
            let pass = msg["pass"].as_bool().unwrap_or(false);
            if let Ok(mut g) = t.inner.lock() {
                if let Some(s) = g.seats.iter_mut().find(|s| s.id == id) {
                    if let Some(why) = refused_action(&action).filter(|_| !pass) {
                        if let Some(tx) = &s.tx {
                            let _ = tx.send(json!({ "t": "notice", "text": why }));
                        }
                        return;
                    }
                    if s.character.is_empty() {
                        if let Some(tx) = &s.tx {
                            let _ = tx.send(json!({ "t": "notice", "text": "Choose your character first." }));
                        }
                        return;
                    }
                    s.passed = pass;
                    s.acted = true;
                    s.action = (!pass).then_some(action);
                }
            }
            notify(app);
            let _ = try_round(app, st, t, false);
            sync(st, t);
        }
        "talk" => {
            let text: String = msg["text"].as_str().unwrap_or("").chars().take(1000).collect();
            let name = t.inner.lock().ok().and_then(|g| g.seats.iter().find(|s| s.id == id).map(|s| s.name.clone())).unwrap_or_default();
            talk(app, t, &name, &text);
        }
        _ => {}
    }
}

pub fn talk(app: &AppHandle, t: &Arc<Table>, name: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let line = json!({ "from": name, "text": text });
    if let Ok(mut g) = t.inner.lock() {
        g.talk.push(line.clone());
    }
    t.broadcast(&json!({ "t": "talk", "line": line }));
    notify(app);
}

pub fn table_update(st: &Shared, note: String) {
    if let Ok(mut s) = st.lock() {
        s.table_updates.push(note);
    }
}

/// Make sure a character can play: a blank sheet from the template if they're new, and a status
/// window if the rules are on. The GM fills both in as they play.
fn ready_character(st: &Shared, name: &str, new: bool) {
    let Ok(mut s) = st.lock() else { return };
    let Some(dir) = s.dir.clone() else { return };
    if new {
        let file = format!("01_Character/{}.md", file_slug(name));
        let form = std::fs::read_to_string(dir.join("01_Character/_TEMPLATE.md")).unwrap_or_default();
        let _ = campaign::write(&dir, &file, &form.replace("[Character]", name));
    }
    if s.rules.enabled && !s.rules.sheets.keys().any(|k| k.eq_ignore_ascii_case(name)) {
        s.rules.sheets.insert(name.to_string(), rpg::Sheet { name: name.to_string(), ..rpg::Sheet::default() });
        let _ = rpg::save(&dir, &s.rules);
    }
}

/// The host takes a character, new or existing, under the same rules as anyone else.
pub fn claim_host(app: &AppHandle, st: &Shared, t: &Arc<Table>, character: &str, new: bool) -> Result<(), String> {
    let character = clean_name(character, 60);
    if character.is_empty() {
        *t.host_character.lock().map_err(|_| "table lost")? = String::new();
        notify(app);
        return Ok(());
    }
    let exists = characters(st).iter().any(|c| c.eq_ignore_ascii_case(&character));
    if !exists && !new {
        return Err("That character isn't in this campaign.".into());
    }
    let taken = t.inner.lock().map_err(|_| "table lost")?.seats.iter().any(|s| s.character.eq_ignore_ascii_case(&character));
    if taken {
        return Err("Someone else plays this character".into());
    }
    ready_character(st, &character, !exists);
    *t.host_character.lock().map_err(|_| "table lost")? = character.clone();
    table_update(st, format!("{} now plays {character}. Bring them in at the next natural moment.", t.host_name));
    sync(st, t);
    notify(app);
    Ok(())
}

/// Spend a level-up point. A player's choice, never a model's.
pub fn spend(st: &Shared, character: &str, kind: &str, name: &str) -> Result<(), String> {
    let mut s = st.lock().map_err(|_| "state lost")?;
    let dir = s.dir.clone().ok_or("No campaign is open.")?;
    let sheet = s.rules.sheets.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(character)).map(|(_, v)| v).ok_or("No such character.")?;
    match kind {
        "attribute" => sheet.spend_attribute(name)?,
        _ => sheet.spend_skill(name)?,
    }
    rpg::save(&dir, &s.rules)
}

pub fn open(app: AppHandle, st: Shared, host_name: String, host_is_gm: bool, announce: bool) -> Result<Arc<Table>, String> {
    let listener = TcpListener::bind(("0.0.0.0", port())).map_err(|e| format!("Couldn't open the table (port {}): {e}", port()))?;
    let t = Arc::new(Table {
        code: new_code()?,
        host_name: host_name.clone(),
        host_character: Mutex::new(String::new()),
        host_is_gm,
        auto_admit: AtomicBool::new(false),
        stop: Arc::new(AtomicBool::new(false)),
        inner: Mutex::new(Inner::default()),
    });
    if announce {
        let campaign = st.lock().ok().and_then(|s| s.dir.as_deref().map(campaign::name)).unwrap_or_default();
        discovery::announce(campaign, host_name, t.stop.clone());
    }
    let t2 = t.clone();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        while !t2.stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, addr)) => {
                    let _ = s.set_nonblocking(false);
                    let (a, st2, t3) = (app.clone(), st.clone(), t2.clone());
                    std::thread::spawn(move || serve(a, st2, t3, s, addr));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    Ok(t)
}

pub fn close(t: &Arc<Table>) {
    t.stop.store(true, Ordering::Relaxed);
    t.broadcast(&json!({ "t": "goodbye", "reason": "The host closed the table." }));
}

/// Let in, turn away, or remove someone.
pub fn decide(app: &AppHandle, st: &Shared, t: &Arc<Table>, id: u64, admit: bool, block: bool) {
    let mut let_in = false;
    if let Ok(mut g) = t.inner.lock() {
        let addr = g.seats.iter().find(|s| s.id == id).and_then(|s| s.addr);
        if admit {
            if let Some(s) = g.seats.iter_mut().find(|s| s.id == id) {
                s.admitted = true;
                if let Some(tx) = &s.tx {
                    let _ = tx.send(json!({ "t": "welcome" }));
                }
                let_in = true;
            }
        } else {
            if let Some(s) = g.seats.iter().find(|s| s.id == id) {
                if let Some(tx) = &s.tx {
                    let reason = if s.admitted { "The host removed you from the table." } else { "The host didn't let you in." };
                    let _ = tx.send(json!({ "t": "goodbye", "reason": reason }));
                }
            }
            g.seats.retain(|s| s.id != id);
            if let (true, Some(ip)) = (block, addr) {
                g.blocked.push(ip);
            }
        }
    }
    if let_in {
        welcome(st, t, id);
    }
    notify(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_connection_dying_leaves_a_rejoined_player_seated() {
        let mut g = Inner::default();
        g.seats.push(Seat { id: 1, connected: true, conn: 2, ..Default::default() });
        mark_gone(&mut g, 1, 1); // the old connection
        assert!(g.seats[0].connected);
        mark_gone(&mut g, 1, 2); // the current one
        assert!(!g.seats[0].connected);
    }

    fn seat(acted: bool) -> Seat {
        Seat { admitted: true, connected: true, character: "Tess".into(), action: acted.then(|| "go".to_string()), ..Default::default() }
    }

    #[test]
    fn a_round_never_goes_without_the_host() {
        // A player's action alone isn't a round, whether the host plays, has no character, or is GM.
        let mut g = Inner { seats: vec![seat(true)], ..Default::default() };
        assert!(!all_in(false, &g));
        assert!(!all_in(true, &g));
        g.host_action = Some("go".into());
        assert!(all_in(false, &g));
        assert!(!all_in(true, &g));
        g.narration = Some("rain falls".into());
        assert!(all_in(true, &g));
        // And it waits for players who are here.
        g.seats.push(seat(false));
        assert!(!all_in(false, &g));
        // But not for one who left.
        g.seats[1].connected = false;
        assert!(all_in(false, &g));
    }

    #[test]
    fn names_cannot_escape_the_character_folder_or_the_prompt() {
        assert_eq!(clean_name("../../evil<x>\"", 60), "evilx");
        assert_eq!(clean_name("  Tess   Vale ", 60), "Tess Vale");
        assert_eq!(clean_name("O'Brien-Smith Jr.", 60), "O'Brien-Smith Jr");
        assert_eq!(clean_name("<>/\\", 60), "");
        assert!(!file_slug("a/b\\c:d.e").contains(['/', '\\', ':', '.']));
    }

    #[test]
    fn players_cannot_narrate_or_use_host_commands() {
        assert!(refused_action("~ the dragon dies").is_some());
        for c in ["/undo", "/retry", "/save", "/load x", "/remember I win", "/note be nice"] {
            assert!(refused_action(c).is_some(), "{c}");
        }
        for c in ["/status", "/ooc what time is it", "> open the door", "\"hello\"", "go north"] {
            assert!(refused_action(c).is_none(), "{c}");
        }
    }
}
