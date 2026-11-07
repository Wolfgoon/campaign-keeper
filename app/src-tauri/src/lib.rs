//! Campaign Keeper 2: the commands the window calls. Game logic lives in the modules below; this
//! file only checks that a campaign is open and hands work to them. Anything slow runs off the
//! UI thread (Tauri runs non-async commands on the main thread).

pub mod campaign;
pub mod chat;
pub mod config;
pub mod engine;
pub mod game;
pub mod memory;
pub mod net;
pub mod prompt;
pub mod referee;
pub mod rpg;
pub mod scribe;
pub mod tags;
pub mod util;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Listener, Manager, State as TauriState};

use chat::{Entry, Session};
use config::Config;
use game::Shared;
use memory::store::Store;
use net::table::{self, Table};

#[derive(Default)]
struct Net {
    table: Option<Arc<Table>>,
    client: Option<Sender<Value>>,
}

struct App {
    game: Shared,
    net: Mutex<Net>,
}

type S<'a> = TauriState<'a, App>;
type R<T> = Result<T, String>;

fn dir(s: &S) -> R<PathBuf> {
    s.game.lock().map_err(|_| "state lost")?.dir.clone().ok_or_else(|| "No campaign is open.".to_string())
}

fn table_of(s: &S) -> R<Arc<Table>> {
    s.net.lock().map_err(|_| "state lost")?.table.clone().ok_or_else(|| "The table is closed.".to_string())
}

async fn off_ui<T: Send + 'static>(f: impl FnOnce() -> R<T> + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

// --- settings ---------------------------------------------------------------

#[tauri::command]
fn get_config(s: S) -> R<Config> {
    Ok(s.game.lock().map_err(|_| "state lost")?.cfg.clone())
}

#[tauri::command]
fn set_config(s: S, cfg: Config) -> R<()> {
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    g.cfg = cfg;
    g.cfg.save()
}

#[tauri::command]
async fn list_models(s: S<'_>) -> R<Vec<String>> {
    let cfg = s.game.lock().map_err(|_| "state lost")?.cfg.clone();
    off_ui(move || engine::openai::list_models(&cfg)).await
}

#[tauri::command]
fn claude_status(s: S) -> R<Value> {
    let cfg = s.game.lock().map_err(|_| "state lost")?.cfg.clone();
    Ok(json!({ "path": engine::claude_code::find(&cfg).map(|p| p.display().to_string()) }))
}

#[tauri::command]
async fn claude_install() -> R<String> {
    off_ui(engine::claude_code::install).await
}

#[tauri::command]
fn claude_sign_in(s: S) -> R<()> {
    let cfg = s.game.lock().map_err(|_| "state lost")?.cfg.clone();
    engine::claude_code::sign_in(&cfg)
}

// --- campaigns --------------------------------------------------------------

fn activate(s: &S, path: PathBuf) -> R<Value> {
    if !campaign::is_campaign(&path) {
        return Err("That isn't a campaign. Drop a campaign folder (one with 00_GM … 08_Saves inside).".into());
    }
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    g.session = chat::load(&path).unwrap_or_default();
    g.rules = rpg::load(&path);
    g.dir = Some(path.clone());
    let p = path.display().to_string();
    g.cfg.add_recent(&p);
    let _ = g.cfg.save();
    drop(g);
    if s.game.lock().map_err(|_| "state lost")?.cfg.memory_enabled {
        memory::index_in_background(&path);
    }
    Ok(json!({ "path": p, "name": campaign::name(&path) }))
}

#[tauri::command]
fn open_campaign(s: S, path: String) -> R<Value> {
    activate(&s, PathBuf::from(path))
}

#[tauri::command]
fn new_campaign(s: S, parent: String, title: String) -> R<Value> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Give the campaign a name.".into());
    }
    let dir = PathBuf::from(parent).join(title);
    campaign::create(&dir, title)?;
    activate(&s, dir)
}

#[tauri::command]
fn close_campaign(s: S) -> R<()> {
    if let Some(t) = s.net.lock().map_err(|_| "state lost")?.table.take() {
        table::close(&t);
    }
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    g.dir = None;
    g.multi = None;
    g.table_updates.clear();
    g.session = Session::default();
    g.rules = rpg::Rules::default();
    Ok(())
}

#[tauri::command]
fn overview(s: S) -> R<campaign::Overview> {
    Ok(campaign::overview(&dir(&s)?))
}

#[tauri::command]
fn list_files(s: S) -> R<Vec<String>> {
    Ok(campaign::files(&dir(&s)?))
}

#[tauri::command]
fn read_file(s: S, path: String) -> R<String> {
    std::fs::read_to_string(campaign::resolve(&dir(&s)?, &path)?).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_file(s: S, path: String, text: String) -> R<()> {
    campaign::write(&dir(&s)?, &path, &text)
}

#[tauri::command]
fn rename_file(s: S, from: String, to: String) -> R<()> {
    campaign::rename(&dir(&s)?, &from, &to)
}

#[tauri::command]
fn trash_file(s: S, path: String) -> R<()> {
    campaign::trash(&dir(&s)?, &path)
}

#[tauri::command]
fn search_files(s: S, query: String) -> R<Vec<campaign::Hit>> {
    Ok(campaign::search(&dir(&s)?, &query))
}

#[tauri::command]
fn store_save(s: S, text: String) -> R<String> {
    campaign::store_save(&dir(&s)?, &text)
}

#[tauri::command]
fn new_session_log(s: S, title: String) -> R<String> {
    let d = dir(&s)?;
    let n = campaign::files(&d).iter().filter(|f| f.starts_with("06_Sessions/") && !f.ends_with("_TEMPLATE.md")).count() + 1;
    let name = title.trim().to_lowercase().replace(' ', "_");
    let file = format!("06_Sessions/session{n}_{name}.md");
    let form = std::fs::read_to_string(d.join("06_Sessions/_TEMPLATE.md")).unwrap_or_default();
    campaign::write(&d, &file, &form.replace("[Title]", title.trim()).replace("[#]", &n.to_string()))?;
    Ok(file)
}

// --- play -------------------------------------------------------------------

fn entry_view(i: usize, e: &Entry) -> Value {
    let text = match (&e.display, e.role.as_str()) {
        (Some(d), _) => d.clone(),
        (None, "assistant") => tags::parse(&e.plain()).display,
        (None, _) => e.label.clone().unwrap_or_else(|| e.plain()),
    };
    json!({
        "idx": i, "role": e.role, "text": text, "label": e.label, "model": e.model, "usage": e.usage,
        "recalled": e.recalled, "rpg": e.rpg, "private": e.flag("private"),
        "pictures": e.extra.get("pictures"), "music": e.extra.get("music"), "voices": e.extra.get("voices"),
        "saved_as": e.extra.get("saved_as"), "character": e.extra.get("character"), "player": e.extra.get("player"),
    })
}

/// Spirit's lazy chat window: a chat can be enormous, so the page gets it in slices.
#[tauri::command]
fn get_entries(s: S, before: Option<usize>, limit: Option<usize>) -> R<Value> {
    let g = s.game.lock().map_err(|_| "state lost")?;
    let e = &g.session.entries;
    let end = before.unwrap_or(e.len()).min(e.len());
    let start = end.saturating_sub(limit.unwrap_or(50));
    let items: Vec<Value> = (start..end).map(|i| entry_view(i, &e[i])).collect();
    Ok(json!({ "entries": items, "start": start, "total": e.len() }))
}

#[tauri::command]
fn play_state(s: S) -> R<Value> {
    let g = s.game.lock().map_err(|_| "state lost")?;
    let u = &g.session;
    Ok(json!({
        "busy": g.busy, "stream": g.stream, "status": g.status, "started": !u.entries.is_empty(),
        "total": u.total, "cost": u.cost, "last_prompt_tokens": u.last_prompt_tokens,
        "activities": u.activities, "queued": g.queue.len(),
        "context_limit": g.cfg.context_limit, "condensed": { "upto": u.condensed.upto, "times": u.condensed.times },
    }))
}

#[tauri::command]
fn send_text(app: AppHandle, s: S, text: String, label: Option<String>, private: Option<bool>) -> R<()> {
    let private = private.unwrap_or(false);
    let t = if private { format!("<gm_private>{text}</gm_private>") } else { text };
    game::send(&app, &s.game, &t, label, private)
}

#[tauri::command]
fn begin(app: AppHandle, s: S, how: String) -> R<()> {
    game::begin(&app, &s.game, &how)
}

#[tauri::command]
fn stop_turn(s: S) -> R<()> {
    let g = s.game.lock().map_err(|_| "state lost")?;
    g.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn end_chat(s: S) -> R<()> {
    let d = dir(&s)?;
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    chat::archive(&d)?;
    g.session = Session::default();
    Ok(())
}

#[tauri::command]
fn notebook(s: S) -> R<String> {
    Ok(prompt::read_notebook(&dir(&s)?))
}

#[tauri::command]
fn activity_close(s: S, id: u64) -> R<()> {
    let d = dir(&s)?;
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    g.session.activities.retain(|a| a["id"].as_u64() != Some(id));
    chat::save(&d, &g.session)
}

#[tauri::command]
fn export_all(s: S) -> R<String> {
    let d = dir(&s)?;
    let instructions = std::fs::read_to_string(d.join(campaign::INSTRUCTIONS)).unwrap_or_default();
    let mut t = instructions;
    t.push_str("\n\n(The campaign already exists: skip the setup.)\n\n<campaign>\n");
    for (k, v) in campaign::game_files(&d) {
        t.push_str(&format!("<file path=\"{k}\">\n{v}\n</file>\n"));
    }
    t.push_str("</campaign>");
    Ok(t)
}

fn kept(dirpath: PathBuf, ext: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(dirpath)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == ext))
                .filter_map(|e| Some((e.path().file_stem()?.to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).ok()?)))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[tauri::command]
fn pictures(s: S) -> R<Value> {
    Ok(kept(campaign::keeper(&dir(&s)?).join("pictures"), "svg").into_iter().map(|(t, svg)| json!({ "title": t, "svg": svg })).collect())
}

#[tauri::command]
fn tracks(s: S) -> R<Value> {
    Ok(kept(campaign::keeper(&dir(&s)?).join("music"), "json").into_iter().filter_map(|(_, m)| serde_json::from_str::<Value>(&m).ok()).collect())
}

// --- litrpg -----------------------------------------------------------------

#[tauri::command]
fn rpg_state(s: S) -> R<rpg::Rules> {
    Ok(s.game.lock().map_err(|_| "state lost")?.rules.clone())
}

/// Turn the rules on. For an existing character a helper builds the sheet from their files;
/// a new game gets one from the GM when the character is decided.
#[tauri::command]
async fn rpg_enable(s: S<'_>) -> R<rpg::Rules> {
    let (cfg, d) = {
        let g = s.game.lock().map_err(|_| "state lost")?;
        (g.cfg.clone(), g.dir.clone().ok_or("No campaign is open.")?)
    };
    let d2 = d.clone();
    let sheets = off_ui(move || referee::sheets_from_campaign(&cfg, &d2)).await?;
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    g.rules.enabled = true;
    for sheet in sheets {
        g.rules.sheets.entry(sheet.name.clone()).or_insert(sheet);
    }
    rpg::save(&d, &g.rules)?;
    // The GM needs the rules: the next turn rebuilds its instructions.
    g.session.system.clear();
    g.session.cc_reset = true;
    chat::save(&d, &g.session)?;
    Ok(g.rules.clone())
}

#[tauri::command]
fn rpg_disable(s: S) -> R<()> {
    let d = dir(&s)?;
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    g.rules.enabled = false;
    rpg::save(&d, &g.rules)?;
    g.session.system.clear();
    g.session.cc_reset = true;
    chat::save(&d, &g.session)
}

/// Spending points is the player's choice, not the model's, so it's only reachable from here.
#[tauri::command]
fn rpg_spend(s: S, character: String, kind: String, name: String) -> R<rpg::Rules> {
    table::spend(&s.game, &character, &kind, &name)?;
    Ok(s.game.lock().map_err(|_| "state lost")?.rules.clone())
}

// --- memory -----------------------------------------------------------------

#[tauri::command]
fn memory_status(s: S) -> R<Value> {
    let d = dir(&s)?;
    let g = s.game.lock().map_err(|_| "state lost")?;
    let mut v = memory::status(&d);
    v["condensed"] = json!(g.session.condensed);
    v["keeper_pending"] = json!(g.session.keeper_pending);
    v["journal"] = json!(journal_text(&d));
    v["playstyle"] = json!(memory::playstyle::load());
    Ok(v)
}

fn journal_text(d: &std::path::Path) -> String {
    let mut files: Vec<PathBuf> = std::fs::read_dir(campaign::keeper(d).join("journal")).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    files.iter().rev().take(3).rev().filter_map(|p| std::fs::read_to_string(p).ok()).collect::<Vec<_>>().join("\n")
}

#[tauri::command]
fn memory_list(s: S) -> R<Vec<memory::store::Memory>> {
    Store::open(&dir(&s)?)?.list(&["fact", "event", "npc", "place", "promise"], 2000)
}

#[tauri::command]
fn memory_search(s: S, query: String) -> R<Vec<memory::store::Memory>> {
    Store::open(&dir(&s)?)?.search(&query, 30, &[], None)
}

#[tauri::command]
fn memory_add(s: S, text: String, kind: String, importance: f64) -> R<()> {
    let st = Store::open(&dir(&s)?)?;
    st.add(&text, &kind, &[], "player", importance, -1)?;
    st.export_backup().map(|_| ())
}

#[tauri::command]
fn memory_update(s: S, id: String, text: String, importance: f64) -> R<()> {
    let st = Store::open(&dir(&s)?)?;
    st.update(&id, Some(&text), Some(importance), None)?;
    st.export_backup().map(|_| ())
}

#[tauri::command]
fn memory_delete(s: S, id: String) -> R<()> {
    let st = Store::open(&dir(&s)?)?;
    st.delete(&id)?;
    st.export_backup().map(|_| ())
}

/// Index a whole existing game. Runs the Keeper over everything played so far, in stretches.
#[tauri::command]
async fn memory_catch_up(s: S<'_>) -> R<String> {
    let (cfg, d, entries, from) = {
        let g = s.game.lock().map_err(|_| "state lost")?;
        (g.cfg.clone(), g.dir.clone().ok_or("No campaign is open.")?, g.session.entries.clone(), g.session.keeper_upto)
    };
    let d2 = d.clone();
    let total = entries.len();
    let msg = off_ui(move || {
        let mut done = 0;
        for chunk in entries[from..].chunks(40) {
            memory::keeper::run(&cfg, &d2, chunk)?;
            done += chunk.len();
        }
        Ok(format!("Filed {done} entries."))
    })
    .await?;
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    g.session.keeper_upto = total;
    chat::save(&d, &g.session)?;
    Ok(msg)
}

// --- scribe -----------------------------------------------------------------

#[tauri::command]
async fn scribe_run(s: S<'_>, request: String, include_chat: bool) -> R<scribe::Outcome> {
    let (cfg, d, entries) = {
        let g = s.game.lock().map_err(|_| "state lost")?;
        (g.cfg.clone(), g.dir.clone().ok_or("No campaign is open.")?, g.session.entries.clone())
    };
    off_ui(move || scribe::run(&cfg, &d, &entries, &request, include_chat)).await
}

#[tauri::command]
fn scribe_presets() -> Value {
    json!({ "after": scribe::PRESET_AFTER, "log": scribe::PRESET_LOG, "check": scribe::PRESET_CHECK })
}

#[tauri::command]
fn scribe_apply(s: S, approved: Vec<(String, String)>) -> R<String> {
    let d = dir(&s)?;
    let b = scribe::apply(&d, &approved)?;
    memory::index_in_background(&d);
    Ok(b)
}

#[tauri::command]
fn scribe_restore(s: S, backup: String) -> R<()> {
    scribe::restore(&dir(&s)?, &backup)
}

// --- table ------------------------------------------------------------------

#[tauri::command]
fn table_open(app: AppHandle, s: S, host_is_gm: bool, announce: bool) -> R<Value> {
    dir(&s)?;
    let name = s.game.lock().map_err(|_| "state lost")?.cfg.player_name.clone();
    let name = if name.trim().is_empty() { "Host".to_string() } else { name };
    let t = table::open(app, s.game.clone(), name.clone(), host_is_gm, announce)?;
    {
        let mut g = s.game.lock().map_err(|_| "state lost")?;
        g.multi = Some((name, host_is_gm));
        g.session.system.clear();
        g.session.cc_reset = true;
    }
    let v = t.state();
    s.net.lock().map_err(|_| "state lost")?.table = Some(t);
    Ok(v)
}

#[tauri::command]
fn table_close(s: S) -> R<()> {
    if let Some(t) = s.net.lock().map_err(|_| "state lost")?.table.take() {
        table::close(&t);
    }
    let mut g = s.game.lock().map_err(|_| "state lost")?;
    g.multi = None;
    g.session.system.clear();
    g.session.cc_reset = true;
    Ok(())
}

#[tauri::command]
fn table_state(s: S) -> R<Value> {
    let t = s.net.lock().map_err(|_| "state lost")?.table.clone();
    Ok(match t {
        Some(t) => {
            let mut v = t.state();
            v["characters"] = json!(table::characters(&s.game));
            v
        }
        None => Value::Null,
    })
}

#[tauri::command]
fn table_decide(app: AppHandle, s: S, id: u64, admit: bool, block: bool) -> R<()> {
    table::decide(&app, &s.game, &table_of(&s)?, id, admit, block);
    Ok(())
}

#[tauri::command]
fn table_auto_admit(s: S, on: bool) -> R<()> {
    table_of(&s)?.auto_admit.store(on, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn table_host_character(app: AppHandle, s: S, character: String, new: bool) -> R<()> {
    table::claim_host(&app, &s.game, &table_of(&s)?, &character, new)
}

#[tauri::command]
fn table_host_action(app: AppHandle, s: S, text: String, gm_narration: bool) -> R<()> {
    let t = table_of(&s)?;
    if !gm_narration && t.host_character.lock().map(|c| c.is_empty()).unwrap_or(true) {
        return Err("Choose your character in the 👥 Table panel first, or press Send the round now.".into());
    }
    if let Ok(mut g) = t.inner.lock() {
        if gm_narration {
            g.narration = Some(text);
        } else {
            g.host_action = Some(text);
        }
    }
    table::try_round(&app, &s.game, &t, false).map(|_| ())
}

#[tauri::command]
fn table_send_now(app: AppHandle, s: S) -> R<()> {
    table::try_round(&app, &s.game, &table_of(&s)?, true).map(|_| ())
}

#[tauri::command]
fn table_talk(app: AppHandle, s: S, text: String) -> R<()> {
    let t = table_of(&s)?;
    table::talk(&app, &t, &t.host_name, &text);
    Ok(())
}

#[tauri::command]
async fn table_scan() -> R<Vec<Value>> {
    off_ui(|| net::discovery::scan(3)).await
}

#[tauri::command]
async fn client_join(app: AppHandle, s: S<'_>, addr: String, code: String, name: String) -> R<()> {
    let a = app.clone();
    let tx = off_ui(move || net::client::join(a, &addr, &code, &name)).await?;
    s.net.lock().map_err(|_| "state lost")?.client = Some(tx);
    Ok(())
}

#[tauri::command]
fn client_send(s: S, msg: Value) -> R<()> {
    let n = s.net.lock().map_err(|_| "state lost")?;
    n.client.as_ref().ok_or("You've left the table.")?.send(msg).map_err(|_| "The host closed the connection.".to_string())
}

#[tauri::command]
fn client_leave(s: S) -> R<()> {
    s.net.lock().map_err(|_| "state lost")?.client = None;
    Ok(())
}

fn install_crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let _ = std::fs::create_dir_all(config::config_dir());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(config::config_dir().join("crash.log")) {
            let _ = writeln!(f, "===== {} =====\n{info}\n", util::now());
        }
        default(info);
    }));
}

pub fn run() {
    install_crash_log();
    let game: Shared = Arc::new(Mutex::new(game::new_state()));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(App { game, net: Mutex::new(Net::default()) })
        .setup(|app| {
            // Keep a table's players in step with the game, whatever happened in it.
            let handle = app.handle().clone();
            app.listen("game", move |ev| {
                let Ok(v) = serde_json::from_str::<Value>(ev.payload()) else { return };
                let st = handle.state::<App>();
                let Some(t) = st.net.lock().ok().and_then(|n| n.table.clone()) else { return };
                match v["kind"].as_str().unwrap_or("") {
                    "stream" => table::relay_stream(&t, &v["data"]),
                    "entry" | "done" => table::sync(&st.game, &t),
                    _ => {}
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config, set_config, list_models, claude_status, claude_install, claude_sign_in,
            open_campaign, new_campaign, close_campaign, overview, list_files, read_file, write_file, rename_file,
            trash_file, search_files, store_save, new_session_log, get_entries, play_state, send_text, begin, stop_turn,
            end_chat, notebook, activity_close, export_all, pictures, tracks, rpg_state, rpg_enable, rpg_disable, rpg_spend,
            memory_status, memory_list, memory_search, memory_add, memory_update, memory_delete, memory_catch_up,
            scribe_run, scribe_presets, scribe_apply, scribe_restore, table_open, table_close, table_state, table_decide,
            table_auto_admit, table_host_character, table_host_action, table_send_now, table_talk, table_scan,
            client_join, client_send, client_leave
        ])
        .run(tauri::generate_context!())
        .expect("error while running Campaign Keeper");
}
