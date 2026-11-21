//! Whole-table scenarios over real connections on this machine: a table is opened on a free port
//! and players join it through the same encrypted handshake the app uses. Only the AI is missing:
//! finished rounds land in a list instead of going to a model.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::secure::handshake;
use super::table::*;
use super::web::{WsIn, WsOut};
use super::PROTOCOL;
use crate::chat::Entry;
use crate::game::{self, Shared};

#[derive(Default)]
struct Fake {
    rounds: Mutex<Vec<String>>,
}

impl Host for Fake {
    fn changed(&self) {}
    fn round(&self, _st: &Shared, text: String, _actions: Vec<(String, String)>) -> Result<(), String> {
        self.rounds.lock().unwrap().push(text);
        Ok(())
    }
}

struct World {
    t: Arc<Table>,
    st: Shared,
    host: Arc<Fake>,
    root: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        close(&self.t);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn world(host_is_gm: bool, auto_admit: bool) -> World {
    let root = std::env::temp_dir().join(format!("ck-table-{}", uuid::Uuid::new_v4()));
    let camp = root.join("camp");
    crate::campaign::create(&camp, "Test").unwrap();
    let mut s = game::new_state();
    s.dir = Some(camp);
    let st: Shared = Arc::new(Mutex::new(s));
    let host = Arc::new(Fake::default());
    let t = open_on(host.clone(), st.clone(), "Hosty".into(), host_is_gm, 0).unwrap();
    t.auto_admit.store(auto_admit, std::sync::atomic::Ordering::Relaxed);
    World { t, st, host, root }
}

impl World {
    fn rounds(&self) -> Vec<String> {
        self.host.rounds.lock().unwrap().clone()
    }
    fn seats(&self) -> Vec<Seat> {
        self.t.inner.lock().unwrap().seats.clone()
    }
    fn seat(&self, name: &str) -> Seat {
        self.seats().into_iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no seat for {name}"))
    }
    fn join(&self, name: &str) -> Player {
        self.join_as(name, &format!("token-{name}"))
    }
    fn join_as(&self, name: &str, token: &str) -> Player {
        connect(self.t.port, &self.t.code, json!({ "t": "hello", "name": name, "token": token, "protocol": PROTOCOL })).unwrap()
    }
    fn host_acts(&self, text: &str) -> bool {
        self.t.inner.lock().unwrap().host_action = Some(text.into());
        try_round(&*self.host, &self.st, &self.t, false).unwrap()
    }
    fn web(&self) -> u16 {
        self.t.web_port.expect("the table has a browser link")
    }
    fn browser(&self, name: &str) -> Player {
        browser(self.web(), json!({ "t": "hello", "name": name, "token": format!("web-{name}"), "protocol": PROTOCOL, "code": self.t.code.to_lowercase() })).unwrap()
    }
    fn updates(&self) -> Vec<String> {
        self.st.lock().unwrap().table_updates.clone()
    }
}

struct Player {
    tx: Box<dyn Out>,
    inbox: Receiver<Value>,
}

fn listen(mut rx: impl In + 'static) -> Receiver<Value> {
    let (itx, inbox) = channel();
    std::thread::spawn(move || {
        while let Ok(v) = rx.recv() {
            if v["t"] != "ping" && itx.send(v).is_err() {
                break;
            }
        }
    });
    inbox
}

/// A plain HTTP GET, as a browser asks for the page.
fn get(port: u16, path: &str) -> Result<String, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: table\r\n\r\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    s.read_to_end(&mut out).map_err(|e| e.to_string())?;
    if out.is_empty() {
        return Err("the host hung up".into());
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Join the way a browser does: an HTTP upgrade to a WebSocket, then a hello carrying the code.
fn browser(port: u16, hello: Value) -> Result<Player, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    s.write_all(b"GET /ws HTTP/1.1\r\nHost: table\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").map_err(|e| e.to_string())?;
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if s.read(&mut b).map_err(|e| e.to_string())? == 0 {
            return Err("the host hung up".into());
        }
        head.push(b[0]);
    }
    let head = String::from_utf8_lossy(&head);
    // The answer every WebSocket server must give to this key (RFC 6455's own example).
    assert!(head.starts_with("HTTP/1.1 101") && head.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="), "{head}");
    let mut tx = WsOut { stream: s.try_clone().map_err(|e| e.to_string())?, mask: true };
    let rx = WsIn { stream: s, first: None };
    tx.send(&hello)?;
    rx.set_timeout(None)?;
    Ok(Player { tx: Box::new(tx), inbox: listen(rx) })
}

fn connect(port: u16, code: &str, hello: Value) -> Result<Player, String> {
    let s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    let (mut tx, rx) = handshake(s, code, false)?;
    tx.send(&hello)?;
    rx.set_timeout(None)?;
    Ok(Player { tx: Box::new(tx), inbox: listen(rx) })
}

const PATIENCE: Duration = Duration::from_secs(5);

impl Player {
    /// The next message of this kind, skipping others.
    fn wait(&self, kind: &str) -> Value {
        let end = Instant::now() + PATIENCE;
        loop {
            let left = end.checked_duration_since(Instant::now()).unwrap_or_else(|| panic!("no \"{kind}\" message arrived"));
            match self.inbox.recv_timeout(left) {
                Ok(v) if v["t"] == kind => return v,
                Ok(_) => {}
                Err(_) => panic!("no \"{kind}\" message arrived"),
            }
        }
    }
    /// True once the host has hung up.
    fn hung_up(&self) -> bool {
        let end = Instant::now() + PATIENCE;
        while Instant::now() < end {
            if let Err(std::sync::mpsc::RecvTimeoutError::Disconnected) = self.inbox.recv_timeout(Duration::from_millis(50)) {
                return true;
            }
        }
        false
    }
    fn say(&mut self, v: Value) {
        self.tx.send(&v).unwrap();
    }
    fn leave(self) {
        self.tx.shutdown();
    }
}

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + PATIENCE;
    while !f() {
        assert!(Instant::now() < end, "never happened: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A player seated with a new character of their own.
fn seated(w: &World, name: &str, character: &str) -> Player {
    let mut p = w.join(name);
    p.wait("welcome");
    p.say(json!({ "t": "claim", "character": character, "new": true }));
    until("the character is taken", || w.seat(name).character == character);
    p
}

#[test]
fn the_ad_system_reaches_players_and_only_they_choose_for_themselves() {
    let w = world(true, true);
    {
        let mut s = w.st.lock().unwrap();
        s.rules.enabled = true;
        s.rules.ruleset = "ad".into();
    }
    let mut ann = seated(&w, "Ann", "Aria");
    let _bob = seated(&w, "Bob", "Brom");
    // A seat gets an AD sheet, and the player's view of it comes over the wire.
    let sheet = loop {
        let m = ann.wait("table");
        if m["sheet"]["ruleset"] == "ad" {
            break m["sheet"].clone();
        }
    };
    assert_eq!(sheet["name"], "Aria");
    assert!(sheet["spectrums"].as_array().is_some_and(|a| !a.is_empty()));
    assert!(!sheet.to_string().contains("System only"));
    // The System offers a class; the player takes it from their own panel.
    w.st.lock().unwrap().rules.apply("Aria: class offer scout", "Aria").unwrap();
    ann.say(json!({ "t": "spend", "kind": "class", "name": "scout" }));
    until("Aria is a scout", || w.st.lock().unwrap().rules.ad.sheets["Aria"].cls.as_deref() == Some("scout"));
    // Spending only ever touches the player's own character.
    assert!(w.st.lock().unwrap().rules.ad.sheets["Brom"].cls.is_none());
}

#[test]
fn a_stranger_waits_until_the_host_decides() {
    let w = world(false, false);
    let ann = w.join("Ann");
    ann.wait("waiting");
    until("Ann asks", || w.seats().len() == 1);
    assert!(!w.seat("Ann").admitted);
    decide(&*w.host, &w.st, &w.t, w.seat("Ann").id, true, false);
    ann.wait("welcome");
    ann.wait("transcript");
    assert_eq!(ann.wait("table")["you"], w.seat("Ann").id);

    let bob = w.join("Bob");
    bob.wait("waiting");
    until("Bob asks", || w.seats().len() == 2);
    decide(&*w.host, &w.st, &w.t, w.seat("Bob").id, false, false);
    assert_eq!(bob.wait("goodbye")["reason"], "The host didn't let you in.");
    assert!(bob.hung_up());
    assert_eq!(w.seats().len(), 1);
}

#[test]
fn someone_who_gives_up_waiting_leaves_the_hosts_list() {
    let w = world(false, false);
    let ann = w.join("Ann");
    ann.wait("waiting");
    until("Ann asks", || w.seats().len() == 1);
    ann.leave();
    until("Ann is gone from the list", || w.seats().is_empty());
}

#[test]
fn wrong_codes_block_an_address_but_typos_are_forgiven() {
    let w = world(false, true);
    let hello = || json!({ "t": "hello", "name": "Mallory", "token": "m", "protocol": PROTOCOL });
    // Four typos, then the right code: in, and the slate is clean.
    for _ in 0..2 {
        for _ in 0..4 {
            assert!(connect(w.t.port, "AAAA-AAAA", hello()).is_err());
        }
        let p = connect(w.t.port, &w.t.code, hello()).unwrap();
        p.wait("welcome");
        p.leave();
    }
    // Five in a row: shut out, even with the right code.
    for _ in 0..5 {
        assert!(connect(w.t.port, "AAAA-AAAA", hello()).is_err());
    }
    until("the address is blocked", || !w.t.inner.lock().unwrap().blocked.is_empty());
    assert!(connect(w.t.port, &w.t.code, hello()).is_err());
}

#[test]
fn old_and_nameless_apps_are_refused_with_a_reason() {
    let w = world(false, true);
    let old = connect(w.t.port, &w.t.code, json!({ "t": "hello", "name": "Old", "token": "o", "protocol": 1 })).unwrap();
    assert!(old.wait("refused")["reason"].as_str().unwrap().contains("different version"));
    let blank = connect(w.t.port, &w.t.code, json!({ "t": "hello", "name": "<<>>", "token": "b", "protocol": PROTOCOL })).unwrap();
    assert_eq!(blank.wait("refused")["reason"], "Enter your name to join.");
    assert!(w.seats().is_empty());
}

#[test]
fn a_round_waits_for_everyone_here_and_nobody_else() {
    let w = world(false, true);
    claim_host(&*w.host, &w.st, &w.t, "Hero", true).unwrap();
    let mut ann = seated(&w, "Ann", "Aria");
    let mut bob = seated(&w, "Bob", "Brom");

    // One character, one player.
    bob.say(json!({ "t": "claim", "character": "Aria" }));
    assert_eq!(bob.wait("notice")["text"], "Someone else plays this character");
    bob.say(json!({ "t": "claim", "character": "Hero" }));
    assert_eq!(bob.wait("notice")["text"], "Someone else plays this character");
    assert!(claim_host(&*w.host, &w.st, &w.t, "aria", false).is_err());
    assert_eq!(w.seat("Bob").character, "Brom");

    ann.say(json!({ "t": "submit", "action": "> open the door" }));
    until("Ann acted", || w.seat("Ann").acted);
    assert!(w.rounds().is_empty(), "a round went without the host");
    assert!(!w.host_acts("> draw my sword"), "a round went without Bob");
    // Ann changes her mind before the round goes.
    ann.say(json!({ "t": "submit", "action": "> knock <politely> & wait" }));
    // Her new action must land before Bob's pass lets the round go: two sockets, two threads.
    until("Ann changed her mind", || w.seat("Ann").action.as_deref() == Some("> knock <politely> & wait"));
    bob.say(json!({ "t": "submit", "pass": true }));
    until("the round goes", || w.rounds().len() == 1);
    let r = &w.rounds()[0];
    assert!(r.contains("<action character=\"Hero\" player=\"Hosty\">&gt; draw my sword</action>"), "{r}");
    assert!(r.contains("<action character=\"Aria\" player=\"Ann\">&gt; knock &lt;politely&gt; &amp; wait</action>"), "{r}");
    assert!(r.contains("<action character=\"Brom\" player=\"Bob\" pass=\"true\"/>"), "{r}");
    assert!(!r.contains("open the door"));
    until("the slate is clean", || w.seats().iter().all(|s| !s.acted && !s.passed && s.action.is_none()));

    // Bob drops out mid-round: the table stops waiting for him.
    ann.say(json!({ "t": "submit", "action": "\"Hello?\"" }));
    until("Ann acted", || w.seat("Ann").acted);
    assert!(!w.host_acts("> listen"));
    bob.leave();
    until("the round goes without Bob", || w.rounds().len() == 2);
    assert!(w.rounds()[1].contains("<absent character=\"Brom\"/>"));
    assert!(w.updates().iter().any(|u| u.contains("Brom's player left the table")));
}

#[test]
fn the_host_can_send_a_round_without_the_slow() {
    let w = world(false, true);
    let mut ann = seated(&w, "Ann", "Aria");
    let _bob = seated(&w, "Bob", "Brom");
    // Nothing to send yet.
    assert!(try_round(&*w.host, &w.st, &w.t, true).is_err());
    ann.say(json!({ "t": "submit", "action": "> run" }));
    until("Ann acted", || w.seat("Ann").acted);
    assert!(w.rounds().is_empty());
    assert!(try_round(&*w.host, &w.st, &w.t, true).unwrap());
    let r = &w.rounds()[0];
    assert!(r.contains("character=\"Aria\"") && r.contains("<absent character=\"Brom\"/>"), "{r}");
}

#[test]
fn a_human_gm_narrates_and_the_round_waits_for_them() {
    let w = world(true, true);
    let mut ann = seated(&w, "Ann", "Aria");
    ann.say(json!({ "t": "submit", "action": "> climb" }));
    until("Ann acted", || w.seat("Ann").acted);
    assert!(w.rounds().is_empty());
    w.t.inner.lock().unwrap().narration = Some("Rain falls.".into());
    assert!(try_round(&*w.host, &w.st, &w.t, false).unwrap());
    assert!(w.rounds()[0].contains("<narration by=\"GM\">Rain falls.</narration>"));
}

#[test]
fn players_cannot_narrate_command_or_act_as_nobody() {
    let w = world(false, true);
    let mut ann = w.join("Ann");
    ann.wait("welcome");
    ann.say(json!({ "t": "submit", "action": "> go" }));
    assert_eq!(ann.wait("notice")["text"], "Choose your character first.");
    ann.say(json!({ "t": "claim", "character": "Nobody Here" }));
    assert_eq!(ann.wait("notice")["text"], "That character isn't in this campaign.");
    ann.say(json!({ "t": "claim", "character": "Aria", "new": true }));
    until("Aria is Ann's", || w.seat("Ann").character == "Aria");
    // The sheet is written just after the seat is taken, so wait for it rather than race it.
    until("Aria's sheet is written", || w.root.join("camp/01_Character/aria.md").exists());
    assert!(w.root.join("camp/01_Character/aria.md").exists());
    ann.say(json!({ "t": "submit", "action": "~ the dragon dies" }));
    assert!(ann.wait("notice")["text"].as_str().unwrap().contains("Only the GM can narrate"));
    ann.say(json!({ "t": "submit", "action": "/undo" }));
    assert!(ann.wait("notice")["text"].as_str().unwrap().contains("is for the host"));
    ann.say(json!({ "t": "submit", "action": "   " }));
    ann.say(json!({ "t": "nonsense" }));
    ann.say(json!({ "t": "talk", "text": "still here" }));
    ann.wait("talk");
    assert!(!w.seat("Ann").acted);
    // A third new character is one too many.
    ann.say(json!({ "t": "claim", "character": "Second", "new": true }));
    until("Second is Ann's", || w.seat("Ann").character == "Second");
    ann.say(json!({ "t": "claim", "character": "Third", "new": true }));
    assert!(ann.wait("notice")["text"].as_str().unwrap().contains("enough new characters"));
    // A name can't reach outside the character folder.
    ann.say(json!({ "t": "claim", "character": "../../evil", "new": false }));
    ann.wait("notice");
    assert!(!w.root.join("evil.md").exists());
}

#[test]
fn a_dropped_player_gets_their_seat_and_character_back() {
    let w = world(false, true);
    let ann = seated(&w, "Ann", "Aria");
    ann.leave();
    until("Ann is marked away", || !w.seat("Ann").connected);
    let again = w.join("Ann");
    again.wait("welcome");
    until("Ann is back", || w.seat("Ann").connected);
    assert_eq!(w.seats().len(), 1);
    assert_eq!(w.seat("Ann").character, "Aria");
    assert!(w.updates().iter().any(|u| u.contains("Aria's player is back")));

    // Joining again while still connected (a second window, a retry): the newer one wins,
    // and the old one closing doesn't unseat them.
    let newer = w.join("Ann");
    assert_eq!(again.wait("goodbye")["reason"], "You rejoined from a new connection.");
    assert!(again.hung_up());
    newer.wait("welcome");
    std::thread::sleep(Duration::from_millis(300));
    assert!(w.seat("Ann").connected);
    assert_eq!(w.seats().len(), 1);
}

#[test]
fn a_removed_player_is_told_and_a_blocked_one_stays_out() {
    let w = world(false, true);
    let mut ann = seated(&w, "Ann", "Aria");
    let bob = seated(&w, "Bob", "Brom");
    // Ann has acted and the host has too; only Bob is holding the round up.
    ann.say(json!({ "t": "submit", "action": "> wait" }));
    until("Ann acted", || w.seat("Ann").acted);
    assert!(!w.host_acts("> wait"));
    decide(&*w.host, &w.st, &w.t, w.seat("Bob").id, false, true);
    assert_eq!(bob.wait("goodbye")["reason"], "The host removed you from the table.");
    assert!(bob.hung_up());
    until("the round goes once Bob is removed", || w.rounds().len() == 1);
    assert!(w.updates().iter().any(|u| u.contains("Brom's player left the table")));
    // Everyone here shares one address in a test, so the block shuts the door for all newcomers.
    assert!(connect(w.t.port, &w.t.code, json!({ "t": "hello", "name": "Bob", "token": "token-Bob", "protocol": PROTOCOL })).is_err());
    // Brom is free for someone else now.
    ann.say(json!({ "t": "claim", "character": "Brom" }));
    until("Ann takes Brom", || w.seat("Ann").character == "Brom");
}

#[test]
fn closing_the_table_tells_everyone_even_those_still_waiting() {
    let w = world(false, true);
    let ann = seated(&w, "Ann", "Aria");
    w.t.auto_admit.store(false, std::sync::atomic::Ordering::Relaxed);
    let bob = w.join("Bob");
    bob.wait("waiting");
    close(&w.t);
    assert_eq!(ann.wait("goodbye")["reason"], "The host closed the table.");
    assert_eq!(bob.wait("goodbye")["reason"], "The host closed the table.");
    assert!(ann.hung_up() && bob.hung_up());
}

#[test]
fn a_full_table_turns_people_away() {
    let w = world(false, true);
    let players: Vec<Player> = (0..12).map(|i| w.join(&format!("P{i}"))).collect();
    for p in &players {
        p.wait("welcome");
    }
    let extra = w.join("Late");
    assert_eq!(extra.wait("refused")["reason"], "The table is full.");
    assert_eq!(w.seats().len(), 12);
}

#[test]
fn two_players_with_one_name_can_be_told_apart() {
    let w = world(false, true);
    let a = w.join_as("Alex", "one");
    a.wait("welcome");
    let b = w.join_as("alex", "two");
    b.wait("welcome");
    let c = w.join_as("Hosty", "three");
    c.wait("welcome");
    let names: Vec<String> = w.seats().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["Alex", "alex (2)", "Hosty (2)"]);
}

#[test]
fn table_talk_reaches_the_table_and_stays_bounded() {
    let w = world(false, true);
    let mut ann = w.join("Ann");
    ann.wait("welcome");
    let bob = w.join("Bob");
    bob.wait("welcome");
    ann.say(json!({ "t": "talk", "text": "pizza?" }));
    assert_eq!(bob.wait("talk")["line"], json!({ "from": "Ann", "text": "pizza?" }));
    for i in 0..250 {
        talk(&*w.host, &w.t, "Hosty", &format!("line {i}"));
    }
    let g = w.t.inner.lock().unwrap();
    assert_eq!(g.talk.len(), 200);
    assert_eq!(g.talk.last().unwrap()["text"], "line 249");
}

#[test]
fn a_silent_phone_is_noticed_but_a_quiet_thinker_is_not() {
    let w = world(false, true);
    // An app that promises heartbeats and then sends none has dropped off.
    let asleep = connect(w.t.port, &w.t.code, json!({ "t": "hello", "name": "Phone", "token": "p", "protocol": PROTOCOL, "pings": true })).unwrap();
    asleep.wait("welcome");
    // An older app never sends any: silence is just thinking.
    let thinker = w.join("Thinker");
    thinker.wait("welcome");
    until("the silent phone is marked away", || !w.seat("Phone").connected);
    assert!(w.seat("Thinker").connected);
    assert!(asleep.hung_up());
}

#[test]
fn a_heartbeat_keeps_a_player_seated() {
    let w = world(false, true);
    let mut p = connect(w.t.port, &w.t.code, json!({ "t": "hello", "name": "Phone", "token": "p", "protocol": PROTOCOL, "pings": true })).unwrap();
    p.wait("welcome");
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(900));
        p.say(json!({ "t": "ping" }));
    }
    assert!(w.seat("Phone").connected);
}

#[test]
fn players_never_see_private_entries_and_big_pictures_dont_break_the_line() {
    let w = world(true, true);
    {
        let mut s = w.st.lock().unwrap();
        let mut secret = Entry::text("assistant", "The butler did it.");
        secret.extra.insert("private".into(), json!(true));
        s.session.entries.push(secret);
        for i in 0..4 {
            let mut e = Entry::text("assistant", &format!("Scene {i}"));
            e.extra.insert("pictures".into(), json!([{ "title": format!("pic {i}"), "png": "x".repeat(1_400_000) }]));
            s.session.entries.push(e);
        }
    }
    let ann = w.join("Ann");
    ann.wait("welcome");
    let entries = ann.wait("transcript")["entries"].as_array().unwrap().clone();
    assert_eq!(entries.len(), 4);
    assert!(entries.iter().all(|e| !e["text"].as_str().unwrap().contains("butler")));
    // The newest pictures arrive; the oldest are dropped to keep the message a sane size.
    let with_pics: Vec<bool> = entries.iter().map(|e| !e["pictures"].is_null()).collect();
    assert_eq!(with_pics, [false, false, true, true]);
}

#[test]
fn the_players_page_is_served_and_nothing_else() {
    let w = world(false, true);
    let home = get(w.web(), "/").unwrap();
    assert!(home.starts_with("HTTP/1.1 200") && home.contains("text/html") && home.contains("<div id=\"app\">"), "{home}");
    let script = home.split("src=\"").nth(1).and_then(|x| x.split('"').next()).expect("the page loads a script");
    let js = get(w.web(), script).unwrap();
    assert!(js.starts_with("HTTP/1.1 200") && js.contains("text/javascript"));
    for path in ["/nope.html", "/../Cargo.toml", "/assets/../../Cargo.toml", "/%2e%2e/Cargo.toml"] {
        assert!(get(w.web(), path).unwrap().starts_with("HTTP/1.1 404"), "{path}");
    }
}

#[test]
fn a_browser_player_plays_beside_an_app_player() {
    let w = world(false, true);
    let mut ivy = w.browser("Ivy");
    ivy.wait("welcome");
    ivy.wait("transcript");
    ivy.say(json!({ "t": "claim", "character": "Iris", "new": true }));
    until("Iris is Ivy's", || w.seat("Ivy").character == "Iris");
    let mut ann = seated(&w, "Ann", "Aria");

    // A long action, to cross the sizes a WebSocket frames differently.
    let long = format!("> {}", "walk ".repeat(20_000));
    ivy.say(json!({ "t": "submit", "action": long }));
    ann.say(json!({ "t": "submit", "action": "> follow Iris" }));
    until("both acted", || w.seat("Ivy").acted && w.seat("Ann").acted);
    assert!(w.host_acts("> lead the way"));
    let r = &w.rounds()[0];
    assert!(r.contains("<action character=\"Iris\" player=\"Ivy\">&gt; walk walk"), "{}", &r[..200]);
    assert!(r.contains("<action character=\"Aria\" player=\"Ann\">&gt; follow Iris</action>"));
    assert!(r.len() > 100_000);

    // Table talk crosses between the two kinds of player.
    ivy.say(json!({ "t": "talk", "text": "from the browser" }));
    assert_eq!(ann.wait("talk")["line"]["from"], "Ivy");

    // The browser tab closes; reopening it gives the seat and character back.
    ivy.leave();
    until("Ivy is marked away", || !w.seat("Ivy").connected);
    let again = w.browser("Ivy");
    again.wait("welcome");
    until("Ivy is back", || w.seat("Ivy").connected);
    assert_eq!(w.seat("Ivy").character, "Iris");
    assert_eq!(w.seats().len(), 2);
}

#[test]
fn the_party_shares_one_page_and_each_player_gets_only_their_own_sheet() {
    let w = world(true, true);
    w.st.lock().unwrap().rules.enabled = true;
    let ann = seated(&w, "Ann", "Aria");
    let mut bob = w.browser("Bob");
    bob.wait("welcome");
    bob.say(json!({ "t": "claim", "character": "Brom", "new": true }));
    until("Brom is Bob's", || w.seat("Bob").character == "Brom");

    // A browser player writes the shared page: it lands in the campaign's files...
    bob.say(json!({ "t": "note", "text": "# Party Notes\n\n- The innkeeper lied.\n" }));
    let file = w.root.join("camp").join("04_Quests").join("party_notes.md");
    until("the note is saved", || std::fs::read_to_string(&file).map(|t| t.contains("innkeeper")).unwrap_or(false));
    // ...and in every other player's picture of the table.
    let m = loop {
        let m = ann.wait("table");
        if m["note"].as_str().is_some_and(|n| n.contains("innkeeper")) {
            break m;
        }
    };
    // Each player only ever gets their own sheet.
    assert_eq!(m["sheet"]["name"], "Aria");
    assert!(m.get("sheets").is_none());

    // The host writes the same page; the browser player sees the change at once.
    host_note(&*w.host, &w.st, Some(&w.t), "# Party Notes\n\n- The innkeeper was telling the truth.\n").unwrap();
    let m = loop {
        let m = bob.wait("table");
        if m["note"].as_str().is_some_and(|n| n.contains("truth")) {
            break m;
        }
    };
    assert_eq!(m["sheet"]["name"], "Brom");
    assert!(std::fs::read_to_string(&file).unwrap().contains("truth"));
}

#[test]
fn a_browser_needs_the_code_and_guessing_gets_it_blocked() {
    let w = world(false, true);
    let guess = |code: &str| browser(w.web(), json!({ "t": "hello", "name": "Mallory", "token": "m", "protocol": PROTOCOL, "code": code }));
    for code in ["", "AAAA-AAAA", "0000-0000", "nope"] {
        let p = guess(code).unwrap();
        assert_eq!(p.wait("refused")["reason"], "The join code didn't match. Check it with the host.");
        assert!(p.hung_up());
    }
    assert!(w.seats().is_empty());
    // The right code after a few misses still works, and clears them.
    let ok = guess(&w.t.code).unwrap();
    ok.wait("welcome");
    ok.leave();
    for _ in 0..5 {
        guess("AAAA-AAAA").unwrap().wait("refused");
    }
    // Shut out: no table, and no page either.
    assert!(guess(&w.t.code).is_err());
    assert!(get(w.web(), "/").is_err());
}

#[test]
fn a_browser_waits_to_be_let_in_like_anyone_else() {
    let w = world(false, false);
    let ivy = w.browser("Ivy");
    ivy.wait("waiting");
    until("Ivy asks", || w.seats().len() == 1);
    decide(&*w.host, &w.st, &w.t, w.seat("Ivy").id, true, false);
    ivy.wait("welcome");
    close(&w.t);
    assert_eq!(ivy.wait("goodbye")["reason"], "The host closed the table.");
    assert!(ivy.hung_up());
}

#[test]
fn a_browser_tab_put_to_sleep_stops_holding_up_the_round() {
    let w = world(false, true);
    let asleep = browser(w.web(), json!({ "t": "hello", "name": "Tab", "token": "t", "protocol": PROTOCOL, "code": w.t.code, "pings": true })).unwrap();
    asleep.wait("welcome");
    until("the sleeping tab is marked away", || !w.seat("Tab").connected);
    assert!(asleep.hung_up());
}

/// Not a check: holds a real table open so a browser can be pointed at it by hand.
/// `CK_HOLD=60 cargo test --lib by_hand -- --ignored --nocapture`
#[test]
#[ignore]
fn a_table_held_open_to_try_by_hand() {
    TEST_QUIET_MS.store(50_000, std::sync::atomic::Ordering::Relaxed);
    let w = world(false, true);
    w.st.lock().unwrap().session.entries.push(Entry::text("assistant", "The rain has not stopped for three days. Water runs down the cellar steps, and somewhere below, a door that should be locked stands open.

**What do you do?**"));
    println!("TABLE web={} code={}", w.web(), w.t.code);
    let secs: u64 = std::env::var("CK_HOLD").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let end = Instant::now() + Duration::from_secs(secs);
    let mut seen = 0;
    while Instant::now() < end {
        let rounds = w.rounds();
        for r in &rounds[seen..] {
            println!("ROUND {r}");
        }
        seen = rounds.len();
        // Stand in for the host: once anyone has acted, add the host's action so the round goes.
        if w.seats().iter().any(|s| s.acted) {
            w.host_acts("> nod");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The invitation a host copies is a real way in: parsed, it reaches the table and seats a player.
#[test]
fn a_pairing_invitation_is_a_way_in() {
    let w = world(false, true);
    let invitation = super::pair::invite("127.0.0.1", w.t.port, &w.t.code);
    // As it arrives: the host's message and the link on its own line.
    let message = format!("Join my Campaign Keeper game. Open Campaign Keeper, tap Join a friend's game, and paste this:\n{invitation}");
    let parsed = super::pair::parse(&message).unwrap();
    assert_eq!(parsed.code, w.t.code);
    // The app resolves the invitation's address with the same code path a typed address uses.
    let target = super::client::targets(&parsed.addr).unwrap()[0];
    assert_eq!(target.port(), w.t.port);
    let ann = connect(target.port(), &parsed.code, json!({ "t": "hello", "name": "Ann", "token": "ann", "protocol": PROTOCOL, "pings": true })).unwrap();
    ann.wait("welcome");
    assert_eq!(w.seat("Ann").character, "");
    // A port the host isn't on fails like any other wrong address: no silent half-join.
    let wrong = super::pair::invite("127.0.0.1", target.port().wrapping_add(1), &w.t.code);
    let parsed = super::pair::parse(&wrong).unwrap();
    assert!(super::client::targets(&parsed.addr).unwrap()[0].port() != target.port());
}

/// The panel's own line, as the window receives it, matches the table it describes.
#[test]
fn the_pairing_panel_describes_this_table() {
    let w = world(false, true);
    let state = w.t.state();
    match state["pairing"]["text"].as_str() {
        Some(text) => {
            let inv = super::pair::parse(text).unwrap();
            assert_eq!(inv.code, w.t.code);
            assert_eq!(super::client::targets(&inv.addr).unwrap()[0].port(), w.t.port);
        }
        // A machine with no network address has nothing to offer, and the panel hides itself.
        None => assert!(state["pairing"].is_null(), "{state}"),
    }
}

/// The browser page is compiled into the host, but its protocol number is written in TypeScript:
/// if the two drift apart, every browser player is refused as an old app they can't update.
#[test]
fn the_web_page_speaks_the_hosts_protocol() {
    let page = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/web.ts")).expect("web.ts");
    let line = page.lines().find(|l| l.trim_start().starts_with("const PROTOCOL")).expect("web.ts declares its protocol");
    let n: u32 = line.split('=').nth(1).unwrap().trim().trim_end_matches(';').trim().parse().unwrap();
    assert_eq!(n, PROTOCOL, "web.ts speaks {n} but the host speaks {PROTOCOL}: browser players would be refused");
}
