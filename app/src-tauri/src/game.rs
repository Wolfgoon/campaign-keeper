//! One turn of play, start to finish. The order is the design:
//!   player's words -> local commands -> recall -> referee -> rules -> GM -> tags -> sheets ->
//!   save -> background memory work.
//! Everything between the player and the GM fails open: if memory or the referee can't answer,
//! the turn goes on without them and a note says so.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::campaign::{self, keeper};
use crate::chat::{self, Entry, Session};
use crate::config::Config;
use crate::engine::{self, Request};
use crate::memory;
use crate::prompt::{self, Multi};
use crate::referee;
use crate::rpg::{self, Rules};
use crate::tags;
use crate::util::{atomic_write, clip, tail};

pub struct State {
    pub cfg: Config,
    pub dir: Option<PathBuf>,
    pub session: Session,
    pub rules: Rules,
    pub busy: bool,
    pub stream: String,
    pub status: String,
    pub queue: Vec<Pending>,
    pub cancel: Arc<AtomicBool>,
    /// Set while a table is open, so the GM gets the multiplayer rules.
    pub multi: Option<(String, bool)>,
    /// Who joined, left or changed character since the GM's last turn.
    pub table_updates: Vec<String>,
}

pub struct Pending {
    pub text: String,
    pub label: Option<String>,
    pub private: bool,
    pub actions: Vec<(String, String)>,
}

pub type Shared = Arc<Mutex<State>>;

pub fn new_state() -> State {
    State {
        cfg: Config::load(),
        dir: None,
        session: Session::default(),
        rules: Rules::default(),
        busy: false,
        stream: String::new(),
        status: String::new(),
        queue: Vec::new(),
        cancel: Arc::new(AtomicBool::new(false)),
        multi: None,
        table_updates: Vec::new(),
    }
}

fn emit(app: &AppHandle, kind: &str, data: Value) {
    let _ = app.emit("game", json!({ "kind": kind, "data": data }));
}

fn set_status(app: &AppHandle, st: &Shared, text: &str) {
    if let Ok(mut s) = st.lock() {
        s.status = text.to_string();
    }
    emit(app, "status", json!(text));
}

pub fn note(app: &AppHandle, text: &str) {
    emit(app, "note", json!(text));
}

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
struct RpgSnapshot {
    idx: usize,
    before: Rules,
    after_checks: Rules,
}

fn undo_path(dir: &Path) -> PathBuf {
    keeper(dir).join("rpg_undo.json")
}

fn load_snapshots(dir: &Path) -> Vec<RpgSnapshot> {
    std::fs::read_to_string(undo_path(dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_snapshot(dir: &Path, snap: RpgSnapshot) {
    let mut v = load_snapshots(dir);
    v.retain(|s| s.idx != snap.idx);
    v.push(snap);
    let n = v.len();
    if n > 12 {
        v.drain(..n - 12);
    }
    let _ = atomic_write(&undo_path(dir), serde_json::to_string(&v).unwrap_or_default().as_bytes());
}

fn slug(title: &str) -> String {
    let s: String = title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let s = s.trim();
    if s.is_empty() { "untitled".to_string() } else { s.chars().take(80).collect() }
}

/// Things typed in the box that the app does itself. Returns what to show, if it handled it.
fn local_command(dir: &Path, st: &mut State, text: &str) -> Option<Result<String, String>> {
    let t = text.trim();
    let (cmd, arg) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
    let arg = arg.trim();
    match cmd {
        "/remember" if !arg.is_empty() => {
            let p = "05_Memory/pinned_memory.md";
            let mut f = std::fs::read_to_string(dir.join(p)).unwrap_or_default();
            if !f.ends_with('\n') {
                f.push('\n');
            }
            f.push_str(&format!("- {arg}\n"));
            Some(campaign::write(dir, p, &f).map(|_| "Added to Pinned Memory.".to_string()))
        }
        "/forget" if !arg.is_empty() => {
            let p = "05_Memory/pinned_memory.md";
            let f = std::fs::read_to_string(dir.join(p)).unwrap_or_default();
            let low = arg.to_lowercase();
            let kept: Vec<&str> = f.lines().filter(|l| !(l.starts_with("- ") && l.to_lowercase().contains(&low))).collect();
            if kept.len() == f.lines().count() {
                return Some(Ok("Nothing in Pinned Memory matched; the GM will still forget it.".into()));
            }
            Some(campaign::write(dir, p, &(kept.join("\n") + "\n")).map(|_| "Removed from Pinned Memory.".to_string()))
        }
        "/note" if !arg.is_empty() => {
            let body = if arg == "clear" { "(none)" } else { arg };
            let f = format!("# Author's Note\n_Style or direction notes that shape every response. Set them in game with `/note [text]`._\n\n{body}\n");
            Some(campaign::write(dir, "00_GM/authors_note.md", &f).map(|_| "Updated the Author's Note.".to_string()))
        }
        "/status" | "/stats" if st.rules.enabled && !st.rules.sheets.is_empty() => {
            Some(Ok(st.rules.sheets.values().map(rpg::status_text).collect::<Vec<_>>().join("\n\n")))
        }
        _ => None,
    }
}

/// Queue a turn, or run it now.
pub fn send(app: &AppHandle, st: &Shared, typed: &str, label: Option<String>, private: bool) -> Result<(), String> {
    let actions = vec![(String::new(), typed.to_string())];
    let mut s = st.lock().map_err(|_| "state lost")?;
    let dir = s.dir.clone().ok_or("No campaign is open.")?;
    if !typed.starts_with("/") || !matches!(typed.split_whitespace().next(), Some("/retry" | "/undo")) {
        if let Some(r) = local_command(&dir, &mut s, typed) {
            let msg = r?;
            drop(s);
            emit(app, "system", json!(msg));
            return Ok(());
        }
    }
    let pending = Pending { text: typed.to_string(), label, private, actions };
    if s.busy {
        s.queue.push(pending);
        drop(s);
        note(app, "Waiting to send: it goes as soon as the GM finishes its current reply.");
        return Ok(());
    }
    s.busy = true;
    drop(s);
    spawn_turn(app.clone(), st.clone(), pending);
    Ok(())
}

/// A whole table's round, as one message. `actions` holds (character, text) for the referee.
pub fn send_round(app: &AppHandle, st: &Shared, text: String, actions: Vec<(String, String)>) -> Result<(), String> {
    let mut s = st.lock().map_err(|_| "state lost")?;
    let pending = Pending { text, label: Some("Round".into()), private: false, actions };
    if s.busy {
        s.queue.push(pending);
        return Ok(());
    }
    s.busy = true;
    drop(s);
    spawn_turn(app.clone(), st.clone(), pending);
    Ok(())
}

fn spawn_turn(app: AppHandle, st: Shared, p: Pending) {
    std::thread::spawn(move || {
        let mut next = Some(p);
        while let Some(p) = next.take() {
            if let Err(e) = run_turn(&app, &st, &p) {
                if e != "stopped" {
                    emit(&app, "error", json!(e));
                } else {
                    emit(&app, "stopped", json!(null));
                }
            }
            if let Ok(mut s) = st.lock() {
                s.stream.clear();
                s.status.clear();
                if s.queue.is_empty() {
                    s.busy = false;
                } else {
                    next = Some(s.queue.remove(0));
                }
            }
            emit(&app, "done", json!(null));
        }
    });
}

fn scene_text(s: &Session) -> String {
    s.entries.iter().rev().find(|e| e.role == "assistant").map(|e| e.display.clone().unwrap_or_else(|| tags::parse(&e.plain()).display)).unwrap_or_default()
}

/// /undo drops the last exchange; /retry drops the reply and keeps the player's message, with its
/// resolved checks, because the outcome of an attempt doesn't change by asking again.
fn rewind(dir: &Path, s: &mut State, keep_user: bool) -> Result<Option<usize>, String> {
    let Some(last_u) = s.session.entries.iter().rposition(|e| e.role == "user") else {
        return Err("There's nothing to take back yet.".into());
    };
    let Some(last_a) = s.session.entries.iter().rposition(|e| e.role == "assistant") else {
        s.session.entries.truncate(if keep_user { last_u + 1 } else { last_u });
        return Ok(Some(last_u));
    };
    if last_u > last_a {
        // The newest message never got a reply (an error, or Stop). There's no reply to take back,
        // and the exchange before it is not this one's to touch.
        if !keep_user {
            if let Some(snap) = load_snapshots(dir).into_iter().find(|x| x.idx == last_u) {
                s.rules = snap.before;
                rpg::save(dir, &s.rules)?;
            }
            s.session.entries.truncate(last_u);
        }
        return Ok(Some(last_u));
    }
    if let Some(prev) = s.session.entries[last_a].notebook_prev.clone() {
        let _ = atomic_write(&keeper(dir).join("gm_notebook.md"), prev.as_bytes());
    }
    if let Some(snap) = load_snapshots(dir).into_iter().find(|x| x.idx == last_u) {
        s.rules = if keep_user { snap.after_checks } else { snap.before };
        rpg::save(dir, &s.rules)?;
    }
    memory::forget_turn(dir, last_u);
    s.session.entries.truncate(if keep_user { last_u + 1 } else { last_u });
    s.session.condensed.upto = s.session.condensed.upto.min(s.session.entries.len());
    s.session.keeper_upto = s.session.keeper_upto.min(s.session.entries.len());
    Ok(Some(last_u))
}

fn run_turn(app: &AppHandle, st: &Shared, p: &Pending) -> Result<(), String> {
    let (cfg, dir, cancel) = {
        let mut s = st.lock().map_err(|_| "state lost")?;
        s.cancel = Arc::new(AtomicBool::new(false));
        (s.cfg.clone(), s.dir.clone().ok_or("No campaign is open.")?, s.cancel.clone())
    };
    let word = p.text.split_whitespace().next().unwrap_or("");
    let retry = word == "/retry";
    let undo = word == "/undo";

    if undo || retry {
        let mut s = st.lock().map_err(|_| "state lost")?;
        rewind(&dir, &mut s, retry)?;
        if s.session.cc_session.is_some() {
            s.session.undo_note = true;
        }
        chat::save(&dir, &s.session)?;
        // Never emit holding the lock: a table's listener needs it, and would wait forever.
        drop(s);
        if undo {
            emit(app, "entry", json!(null));
            return Ok(());
        }
    }

    // Prepare the system prompt and notice changed files.
    let mut update = String::new();
    {
        let mut s = st.lock().map_err(|_| "state lost")?;
        let multi = s.multi.as_ref().map(|(host, gm)| Multi { host: host.clone(), human_gm: *gm });
        let rules_on = s.rules.enabled;
        if s.session.system.is_empty() {
            s.session.system = prompt::build_system(&dir, &cfg, rules_on, multi.as_ref());
            s.session.known = campaign::known(&dir);
            s.session.known_v = 2;
        } else if s.session.known_v != 2 {
            // 1.x hashed differently: re-baseline once instead of resending every file.
            s.session.known = campaign::known(&dir);
            s.session.known_v = 2;
        } else {
            let now = campaign::known(&dir);
            let changed: Vec<String> = now.iter().filter(|(k, v)| s.session.known.get(*k) != Some(*v)).map(|(k, _)| k.clone()).collect();
            if !changed.is_empty() {
                let files = campaign::game_files(&dir);
                update.push_str("<campaign_update>\nThe player changed these campaign files during play. These versions replace the earlier ones.\n");
                for f in &changed {
                    if let Some(t) = files.get(f) {
                        update.push_str(&format!("<file path=\"{f}\">\n{t}\n</file>\n"));
                    }
                }
                update.push_str("</campaign_update>");
                s.session.known = now;
                let names: Vec<&str> = changed.iter().map(|f| f.rsplit('/').next().unwrap_or(f)).collect();
                note(app, &format!("Sent file changes to the GM: {}", names.join(", ")));
            }
        }
    }

    if !retry {
        compose_user_turn(app, st, p, &cfg, &dir, update)?;
    }

    // Build the request.
    set_status(app, st, "The GM is thinking…");
    let (request, last_user_idx) = {
        let s = st.lock().map_err(|_| "state lost")?;
        let mut messages = prompt::build_messages(&s.session);
        if s.session.undo_note {
            if let Some(m) = messages.last_mut() {
                let n = if retry { "(The player used /retry: discard your last reply and write a different one for this same action. The checks stand.)" } else { "(The player used /undo: treat their previous action and your reply to it as if they never happened.)" };
                m.1 = format!("{n}\n\n{}", m.1);
            }
        }
        let session = if s.session.cc_reset { None } else { s.session.cc_session.clone() };
        (
            Request { system: s.session.system.clone(), messages, max_tokens: cfg.max_tokens, session, workdir: keeper(&dir) },
            s.session.entries.len().saturating_sub(1),
        )
    };

    let t0 = Instant::now();
    let mut buffer = String::new();
    let mut last_emit = Instant::now();
    let app2 = app.clone();
    let st2 = st.clone();
    let mut on_delta = |d: &str| {
        buffer.push_str(d);
        if last_emit.elapsed().as_millis() >= 60 {
            last_emit = Instant::now();
            let v = tags::visible(&buffer);
            if let Ok(mut s) = st2.lock() {
                s.stream = v.clone();
            }
            emit(&app2, "stream", json!(v));
        }
    };
    let reply = engine::stream(&cfg, &request, &mut on_delta, &cancel);
    let reply = match reply {
        Ok(r) => r,
        Err(e) => {
            // Keep what the player typed, and let them try the turn again.
            // An /undo note still has to reach the GM; a /retry's goes with the retry.
            let mut s = st.lock().map_err(|_| "state lost")?;
            if retry {
                s.session.undo_note = false;
            }
            chat::save(&dir, &s.session)?;
            return Err(e);
        }
    };
    if reply.text.trim().is_empty() {
        return Err("The GM's reply came back empty. Try again.".into());
    }

    let parsed = tags::parse(&reply.text);
    let mut s = st.lock().map_err(|_| "state lost")?;
    let mut entry = Entry::text("assistant", &reply.text);
    entry.usage = Some(reply.usage.clone());
    entry.model = Some(reply.model.clone());
    entry.display = Some(parsed.display.clone());
    if p.private {
        entry.extra.insert("private".into(), json!(true));
    }
    if let Some(n) = &parsed.notebook {
        entry.notebook_prev = Some(prompt::read_notebook(&dir));
        atomic_write(&keeper(&dir).join("gm_notebook.md"), n.as_bytes())?;
        s.session.known = campaign::known(&dir);
    }
    keep_media(&dir, &parsed, &mut entry);
    if let Some(a) = &parsed.activity {
        s.session.activity_seq += 1;
        let mut a = a.clone();
        a["id"] = json!(s.session.activity_seq);
        s.session.activities.push(a);
    }
    if let Some(block) = &parsed.save_block {
        if let Ok(f) = campaign::store_save(&dir, block) {
            entry.extra.insert("saved_as".into(), json!(f));
        }
    }
    apply_rpg(&dir, &mut s, &parsed, &mut entry)?;
    if reply.truncated {
        note(app, "Cut off: this reply hit the output limit (see Settings).");
    }

    s.session.total.add(&reply.usage);
    s.session.cost += engine::cost(&reply.model, &reply.usage);
    s.session.last_prompt_tokens = reply.usage.prompt();
    if let Some(id) = reply.session {
        s.session.cc_session = Some(id);
        s.session.cc_reset = false;
    }
    s.session.undo_note = false;
    s.session.entries.push(entry);
    s.session.keeper_pending += 1;
    chat::save(&dir, &s.session)?;
    let player = s.session.entries.get(last_user_idx).map(|e| e.display.clone().unwrap_or_else(|| e.plain())).unwrap_or_default();
    drop(s);

    emit(app, "entry", json!({ "seconds": t0.elapsed().as_secs_f32() }));
    if cfg.memory_enabled && !p.private {
        after_turn(app, st, &cfg, &dir, player, parsed.display, last_user_idx);
    }
    Ok(())
}

/// Recall, referee, rules, and the player's message with what they produced.
fn compose_user_turn(app: &AppHandle, st: &Shared, p: &Pending, cfg: &Config, dir: &Path, update: String) -> Result<(), String> {
    let is_command = p.text.trim_start().starts_with('/') || p.private;
    let is_story = p.text.trim_start().starts_with('~');
    let (session_snapshot, rules_snapshot) = {
        let s = st.lock().map_err(|_| "state lost")?;
        (s.session.clone(), s.rules.clone())
    };
    let scene = scene_text(&session_snapshot);

    let mut recalled = None;
    let mut seen = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    if cfg.memory_enabled && !is_command {
        set_status(app, st, "Remembering…");
        let files = campaign::game_files(dir);
        let (whole, _) = prompt::split_files(cfg, &files);
        let query = format!("{}\n{}", p.text, tail(&scene, 600));
        match memory::recall(dir, cfg, &query, &session_snapshot, &whole) {
            Ok(r) => {
                if !r.block.is_empty() {
                    blocks.push(r.block);
                    recalled = Some(r.lines);
                    seen = r.ids;
                }
            }
            Err(e) => note(app, &format!("Recall skipped: {e}")),
        }
    }

    // The Referee works from a snapshot, but the rules run on the live sheets, so a point spent
    // while it was thinking isn't lost.
    let rules = rules_snapshot;
    let mut requests = Vec::new();
    // Starting or resuming a game is setup, not an attempt.
    let setup = p.label.as_deref().is_some_and(|l| l != "Round");
    if rules.enabled && !rules.sheets.is_empty() && !is_command && !is_story && !setup {
        set_status(app, st, "The referee is weighing it…");
        let actions: Vec<(String, String)> = p.actions.iter().map(|(c, t)| {
            let who = if c.is_empty() { rules.sheets.keys().next().cloned().unwrap_or_default() } else { c.clone() };
            (who, t.clone())
        }).collect();
        match referee::ask(cfg, &rules, &scene, &actions) {
            Ok(r) => requests = r,
            Err(e) => note(app, &format!("The referee didn't answer, so the GM rules this turn by judgment ({e}).")),
        }
    }

    let mut text = String::new();
    if !update.is_empty() {
        text.push_str(&update);
        text.push_str("\n\n");
    }
    text.push_str(&p.text);
    let mut s = st.lock().map_err(|_| "state lost")?;
    let before = s.rules.clone();
    let (block, results, errors) = referee::resolve_all(&mut s.rules, &requests);
    if !block.is_empty() {
        blocks.push(block);
    }
    for e in errors {
        note(app, &format!("Check skipped: {e}"));
    }
    if s.rules.enabled && !s.rules.sheets.is_empty() {
        let sheets = s.rules.sheets.values().map(rpg::status_text).collect::<Vec<_>>().join("\n\n");
        blocks.push(format!("<status_window>\n{sheets}\n</status_window>"));
    }
    if !s.session.rpg_notices.is_empty() {
        blocks.push(format!("<rpg_notice>\n{}\n</rpg_notice>", s.session.rpg_notices.join("\n")));
        s.session.rpg_notices.clear();
    }
    if !s.table_updates.is_empty() {
        blocks.push(format!("<table_update>\n{}\n</table_update>", s.table_updates.join("\n")));
        s.table_updates.clear();
    }
    for b in blocks {
        text.push_str("\n\n");
        text.push_str(&b);
    }
    let mut entry = Entry::text("user", &text);
    entry.display = Some(p.text.clone());
    entry.label = p.label.clone();
    entry.recalled = recalled;
    if p.private {
        entry.extra.insert("private".into(), json!(true));
    }
    if !results.is_empty() {
        entry.rpg = Some(json!({ "checks": results }));
    }
    let idx = s.session.entries.len();
    memory::note_seen(&mut s.session, &seen);
    if s.rules.enabled {
        save_snapshot(dir, RpgSnapshot { idx, before, after_checks: s.rules.clone() });
        rpg::save(dir, &s.rules)?;
    }
    s.session.entries.push(entry);
    chat::save(dir, &s.session)?;
    drop(s);
    emit(app, "entry", json!(null));
    Ok(())
}

fn keep_media(dir: &Path, parsed: &tags::Parsed, entry: &mut Entry) {
    let mut pics = Vec::new();
    for p in &parsed.pictures {
        let title = p["title"].as_str().unwrap_or("Picture");
        let file = keeper(dir).join("pictures").join(format!("{}.svg", slug(title)));
        match p["svg"].as_str() {
            Some(svg) => {
                let _ = atomic_write(&file, svg.as_bytes());
                pics.push(p.clone());
            }
            None => {
                if let Ok(svg) = std::fs::read_to_string(&file) {
                    pics.push(json!({ "title": title, "kind": p["kind"], "svg": svg }));
                }
            }
        }
    }
    if !pics.is_empty() {
        entry.extra.insert("pictures".into(), json!(pics));
    }
    if let Some(m) = &parsed.music {
        let mut m = m.clone();
        if let (Some(t), Some(_)) = (m["title"].as_str(), m["score"].as_str()) {
            let _ = atomic_write(&keeper(dir).join("music").join(format!("{}.json", slug(t))), m.to_string().as_bytes());
        } else if let (Some(t), true) = (m["title"].as_str(), m["replay"].as_bool().unwrap_or(false)) {
            if let Some(kept) = std::fs::read_to_string(keeper(dir).join("music").join(format!("{}.json", slug(t)))).ok().and_then(|x| serde_json::from_str(&x).ok()) {
                m = kept;
            }
        }
        entry.extra.insert("music".into(), m);
    }
    if !parsed.voices.is_empty() {
        entry.extra.insert("voices".into(), json!(parsed.voices));
    }
}

/// Apply what the GM's reply changed. A refused change goes back to the GM next turn.
fn apply_rpg(dir: &Path, s: &mut State, parsed: &tags::Parsed, entry: &mut Entry) -> Result<(), String> {
    if !s.rules.enabled {
        return Ok(());
    }
    let mut changes = Vec::new();
    if let Some(v) = &parsed.rpg_sheet {
        if let Some(sheet) = rpg::sheet_from_json(v) {
            // A blank sheet from joining the table gives way to the GM's real one, once.
            let key = s.rules.sheets.keys().find(|k| k.eq_ignore_ascii_case(&sheet.name)).cloned();
            let blank = key.as_ref().and_then(|k| s.rules.sheets.get(k)).is_none_or(|x| x.skills.is_empty() && x.xp == 0);
            if blank {
                changes.push(format!("Status window opened for {}", sheet.name));
                s.rules.sheets.insert(key.unwrap_or_else(|| sheet.name.clone()), sheet);
            }
        }
    }
    let default = s.rules.sheets.keys().next().cloned().unwrap_or_default();
    for line in &parsed.rpg_lines {
        match rpg::apply_line(&mut s.rules, line, &default) {
            Ok(m) => changes.push(m),
            Err(e) => {
                changes.push(format!("Refused: {line} ({e})"));
                s.session.rpg_notices.push(format!("Refused: {line} ({e})"));
            }
        }
    }
    rpg::save(dir, &s.rules)?;
    if !changes.is_empty() {
        entry.rpg = Some(json!({ "changes": changes }));
    }
    Ok(())
}

/// The slow memory work, off the player's turn. None of it blocks the next action.
fn after_turn(app: &AppHandle, st: &Shared, cfg: &Config, dir: &Path, player: String, gm: String, turn: usize) {
    let (app, st, cfg, dir) = (app.clone(), st.clone(), cfg.clone(), dir.to_path_buf());
    std::thread::spawn(move || {
        if let Err(e) = memory::store_turn(&dir, &player, &gm, turn) {
            note(&app, &format!("Memory: {e}"));
        }
        if let Ok(store) = memory::store::Store::open(&dir) {
            let _ = store.sync_files(&dir);
            let _ = store.embed_pending(64);
        }
        let (due, slice, overhead, snapshot) = {
            let Ok(mut s) = st.lock() else { return };
            let due = s.session.keeper_pending >= cfg.keeper_every;
            // The newest exchange stays out: /undo can still take it back.
            let end = s.session.entries.len().saturating_sub(2);
            let start = s.session.keeper_upto.min(end);
            let slice = s.session.entries[start..end].to_vec();
            if due {
                s.session.keeper_pending = 0;
                s.session.keeper_upto = end;
            }
            let overhead: u64 = s.session.system.iter().map(|t| crate::util::tokens(t)).sum();
            (due, slice, overhead, s.session.clone())
        };
        if due && !slice.is_empty() {
            emit(&app, "memory", json!("The Keeper is filing what happened…"));
            match memory::keeper::run(&cfg, &dir, &slice) {
                Ok(m) => emit(&app, "memory", json!(format!("Keeper: {m}"))),
                Err(e) => note(&app, &format!("Keeper skipped: {e}")),
            }
        }
        if snapshot.last_prompt_tokens > cfg.context_limit {
            emit(&app, "memory", json!("Condensing older turns…"));
            match memory::condense(&cfg, &snapshot, overhead) {
                Ok(Some(c)) => {
                    let Ok(mut s) = st.lock() else { return };
                    let upto = c.upto;
                    if c.upto <= s.session.entries.len() {
                        s.session.condensed = c;
                        // The next turn starts Claude Code afresh, and the files are re-read.
                        s.session.cc_reset = true;
                        s.session.system.clear();
                        let _ = chat::save(&dir, &s.session);
                    }
                    drop(s);
                    emit(&app, "memory", json!(format!("Condensed everything before turn {upto}. Nothing was deleted.")));
                }
                Ok(None) => {}
                Err(e) => note(&app, &format!("Condensing skipped: {e}")),
            }
        }
        if cfg.continuity_check {
            continuity(&app, &st, &cfg, &dir, &gm);
        }
    });
}

const CONTINUITY: &str = "\
You check a text-adventure GM's newest reply against the campaign's memory. Report only a clear contradiction: a wrong name, a \
dead character speaking, an item they don't have, a broken timeline. If there is none, reply with exactly: ok. Otherwise reply \
with one short sentence stating the correction.";

/// Spirit's output processors, for a game: one quiet check, and a note for the GM's next turn.
fn continuity(app: &AppHandle, st: &Shared, cfg: &Config, dir: &Path, gm: &str) {
    let Ok(store) = memory::store::Store::open(dir) else { return };
    let Ok(hits) = store.search(gm, 8, &[], None) else { return };
    if hits.is_empty() {
        return;
    }
    let mem: Vec<String> = hits.iter().map(|h| format!("- {}", clip(&h.text, 400))).collect();
    let user = format!("MEMORY:\n{}\n\nTHE GM'S NEWEST REPLY:\n{gm}", mem.join("\n"));
    if let Ok(r) = engine::helper(cfg, CONTINUITY, &user) {
        let r = r.trim();
        if !r.is_empty() && !r.eq_ignore_ascii_case("ok") && !r.eq_ignore_ascii_case("ok.") {
            if let Ok(mut s) = st.lock() {
                s.session.rpg_notices.push(format!("Continuity: {r}"));
                let _ = chat::save(dir, &s.session);
            }
            emit(app, "memory", json!(format!("Continuity note for the GM: {r}")));
        }
    }
}

/// Start a chat from the first message for a new game or a resume.
pub fn begin(app: &AppHandle, st: &Shared, how: &str) -> Result<(), String> {
    let dir = st.lock().map_err(|_| "state lost")?.dir.clone().ok_or("No campaign is open.")?;
    let (text, label) = match how {
        "new" => ("Let's start a new game.".to_string(), "New game".to_string()),
        "save" => {
            let f = campaign::latest_save(&dir).ok_or("There's no save in 08_Saves yet.")?;
            let t = std::fs::read_to_string(dir.join(&f)).map_err(|e| e.to_string())?;
            (format!("/load\n{t}\n\n(Skip the setup: the player is resuming from this save.)"), format!("/load {}", f.rsplit('/').next().unwrap_or("")))
        }
        _ => (
            "/load\n\n(There is no SAVE BLOCK yet. Resume from the campaign files: the latest session log's \"Where We Left Off\" is the current scene.)".to_string(),
            "/load from the campaign files".to_string(),
        ),
    };
    send(app, st, &text, Some(label), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(roles: &[&str]) -> (State, PathBuf) {
        let d = std::env::temp_dir().join(format!("ck-game-{}", uuid::Uuid::new_v4())).join("c");
        campaign::create(&d, "T").unwrap();
        let mut s = new_state();
        s.session.entries = roles.iter().enumerate().map(|(i, r)| Entry::text(r, &format!("{i}"))).collect();
        (s, d)
    }

    fn texts(s: &State) -> Vec<String> {
        s.session.entries.iter().map(|e| e.plain()).collect()
    }

    #[test]
    fn undo_takes_back_one_exchange() {
        let (mut s, d) = state(&["user", "assistant", "user", "assistant"]);
        rewind(&d, &mut s, false).unwrap();
        assert_eq!(texts(&s), ["0", "1"]);
    }

    #[test]
    fn retry_keeps_the_players_message() {
        let (mut s, d) = state(&["user", "assistant", "user", "assistant"]);
        rewind(&d, &mut s, true).unwrap();
        assert_eq!(texts(&s), ["0", "1", "2"]);
    }

    #[test]
    fn retry_after_a_failed_reply_loses_nothing() {
        let (mut s, d) = state(&["user", "assistant", "user"]);
        rewind(&d, &mut s, true).unwrap();
        assert_eq!(texts(&s), ["0", "1", "2"]);
    }

    #[test]
    fn undo_after_a_failed_reply_removes_only_that_message() {
        let (mut s, d) = state(&["user", "assistant", "user"]);
        rewind(&d, &mut s, false).unwrap();
        assert_eq!(texts(&s), ["0", "1"]);
    }

    #[test]
    fn nothing_to_take_back_is_an_error_not_a_crash() {
        let (mut s, d) = state(&[]);
        assert!(rewind(&d, &mut s, false).is_err());
    }
}
