//! Keeps a campaign the same on every device signed in to the player's account.
//!
//! A story travels as one zip of the campaign folder. Three hashes decide what to do: this
//! device's files now, the account's copy, and what both were when they last agreed (the base).
//! Only one side changed: copy it across. Both changed: ask the player. Before anything from the
//! account replaces local files, the local ones are zipped into a backup on this device.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::account::{self, Remote};
use crate::campaign;
use crate::config::{library_dir, local_dir};
use crate::memory::store::campaign_id;
use crate::util::atomic_write;

type R<T> = Result<T, String>;
type Files = Vec<(String, Vec<u8>)>;

/// One sync at a time: two at once could upload a half-replaced folder.
static LOCK: Mutex<()> = Mutex::new(());

/// What this device remembers about a story between syncs. Lives outside the campaign folder.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct State {
    /// The files' hash when this device and the account last agreed.
    base: String,
    path: String,
    /// The Claude Code session id that came with a download. It belongs to another device, so a
    /// chat still carrying it must start a new session here.
    foreign_cc: Option<String>,
}

fn state_path(id: &str) -> PathBuf {
    local_dir().join("sync").join(format!("{id}.json"))
}

fn load_state(id: &str) -> State {
    std::fs::read_to_string(state_path(id)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_state(id: &str, s: &State) -> R<()> {
    atomic_write(&state_path(id), serde_json::to_string_pretty(s).unwrap_or_default().as_bytes())
}

/// Things that stay on the device: the trash, backups, work folders and half-written files.
fn skip(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or("");
    let tmp = name.rsplit('.').next().is_some_and(|e| e.len() > 3 && e.starts_with("tmp") && e[3..].bytes().all(|b| b.is_ascii_digit()));
    tmp || name == ".DS_Store"
        || name.eq_ignore_ascii_case("desktop.ini")
        || name.eq_ignore_ascii_case("thumbs.db")
        || [".keeper/trash/", ".keeper/backups/", ".keeper/scribe-work/"].iter().any(|p| rel.starts_with(p))
}

fn read_all(dir: &Path) -> R<Files> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(dir).sort_by_file_name().into_iter().flatten().filter(|e| e.file_type().is_file()) {
        let rel = campaign::rel(dir, e.path());
        if skip(&rel) {
            continue;
        }
        // A file that vanished mid-walk (a temp file, a rename) simply isn't part of this snapshot.
        if let Ok(b) = std::fs::read(e.path()) {
            out.push((rel, b));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn hash_of(files: &Files) -> String {
    let mut h = Sha256::new();
    for (rel, b) in files {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update((b.len() as u64).to_le_bytes());
        h.update(b);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn zip(files: &Files) -> R<Vec<u8>> {
    let e = |e: zip::result::ZipError| e.to_string();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    for (rel, b) in files {
        w.start_file(rel.as_str(), opt).map_err(e)?;
        w.write_all(b).map_err(|e| e.to_string())?;
    }
    Ok(w.finish().map_err(e)?.into_inner())
}

/// The files in a story's zip. Names are checked: nothing can land outside the campaign folder.
fn unzip(bytes: &[u8]) -> R<Files> {
    let bad = |e: zip::result::ZipError| format!("The story in your account couldn't be read: {e}");
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(bad)?;
    let mut out = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(bad)?;
        if f.is_dir() {
            continue;
        }
        let rel = f.name().replace('\\', "/");
        campaign::resolve(Path::new("x"), &rel)?;
        if skip(&rel) {
            continue;
        }
        let mut b = Vec::new();
        f.read_to_end(&mut b).map_err(|e| format!("The story in your account couldn't be read: {e}"))?;
        out.push((rel, b));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn write_all(dir: &Path, files: &Files) -> R<()> {
    for (rel, b) in files {
        atomic_write(&campaign::resolve(dir, rel)?, b)?;
    }
    Ok(())
}

/// Make the folder match `incoming`, after zipping what's there now into a backup on this device.
fn replace(dir: &Path, id: &str, current: &Files, incoming: &Files) -> R<()> {
    let backups = local_dir().join("sync-backups").join(id);
    atomic_write(&backups.join(format!("{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"))), &zip(current)?)?;
    let mut old: Vec<PathBuf> = std::fs::read_dir(&backups).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    old.sort();
    for p in old.iter().rev().skip(5) {
        let _ = std::fs::remove_file(p);
    }
    write_all(dir, incoming)?;
    for (rel, _) in current.iter().filter(|(rel, _)| incoming.binary_search_by(|x| x.0.as_str().cmp(rel)).is_err()) {
        let _ = std::fs::remove_file(dir.join(rel));
    }
    // The memory database is rebuilt from the backup file that just arrived.
    let db = local_dir().join("memory").join(format!("{id}.db"));
    for ext in ["db", "db-wal", "db-shm"] {
        let _ = std::fs::remove_file(db.with_extension(ext));
    }
    Ok(())
}

fn cc_session(files: &Files) -> Option<String> {
    let (_, b) = files.iter().find(|(rel, _)| rel == ".keeper/chat.json")?;
    serde_json::from_slice::<Value>(b).ok()?["cc_session"].as_str().map(String::from)
}

fn device() -> String {
    if cfg!(target_os = "android") {
        return "an Android device".into();
    }
    if cfg!(target_os = "ios") {
        return "an iPhone or iPad".into();
    }
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "a computer".into())
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Step {
    Same,
    Up,
    Down,
    Conflict,
}

/// `local` and `remote` are the two sides now; `base` is what they were when they last agreed.
fn decide(local: &str, base: &str, remote: Option<&str>, prefer: Option<&str>) -> Step {
    match remote {
        None => Step::Up,
        Some(r) if r == local => Step::Same,
        Some(r) if r == base => Step::Up,
        Some(_) if local == base => Step::Down,
        Some(_) => match prefer {
            Some("local") => Step::Up,
            Some("cloud") => Step::Down,
            _ => Step::Conflict,
        },
    }
}

/// True when the chat came from another device and still carries that device's Claude Code session.
pub fn foreign_session(dir: &Path, cc: Option<&str>) -> bool {
    let Some(cc) = cc else { return false };
    campaign_id(dir).ok().and_then(|id| load_state(&id).foreign_cc).is_some_and(|f| f == cc)
}

/// Bring this campaign and the account's copy together. `prefer` settles a conflict: "local" or "cloud".
pub fn run(dir: &Path, prefer: Option<&str>) -> R<Value> {
    if !account::signed_in() {
        return Ok(json!({ "state": "off" }));
    }
    let _one = LOCK.lock().map_err(|_| "sync state lost")?;
    let id = campaign_id(dir)?;
    let files = read_all(dir)?;
    let local = hash_of(&files);
    let mut st = load_state(&id);
    st.path = dir.display().to_string();
    let remote: Option<Remote> = account::list()?.into_iter().find(|r| r.id == id);
    let step = decide(&local, &st.base, remote.as_ref().map(|r| r.hash.as_str()), prefer);
    let state = match step {
        Step::Same => {
            st.base = local;
            "same"
        }
        Step::Up => {
            account::upload(remote.as_ref().map(|r| r.file_id.as_str()), &id, &campaign::name(dir), &local, &device(), &zip(&files)?)?;
            st.base = local;
            "uploaded"
        }
        Step::Down => {
            let r = remote.as_ref().ok_or("The story is no longer in your account.")?;
            let incoming = unzip(&account::download(&r.file_id)?)?;
            if incoming.is_empty() {
                return Err("The story in your account is empty, so this device's copy was left alone.".into());
            }
            replace(dir, &id, &files, &incoming)?;
            st.foreign_cc = cc_session(&incoming);
            // The account's hash, not ours: if they ever differ, the next sync uploads and settles it.
            st.base = r.hash.clone();
            "downloaded"
        }
        Step::Conflict => "conflict",
    };
    save_state(&id, &st)?;
    Ok(json!({ "state": state, "cloud": remote }))
}

/// Like `run`, but gives up at once when a sync is already going. For the quiet sync after a turn.
pub fn run_if_idle(dir: &Path) -> Option<R<Value>> {
    drop(LOCK.try_lock().ok()?);
    Some(run(dir, None))
}

fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').trim();
    if s.is_empty() { "Story".into() } else { s.to_string() }
}

/// The stories in the account, each with its folder on this device when it has one.
pub fn stories() -> R<Vec<Value>> {
    Ok(account::list()?
        .into_iter()
        .map(|r| {
            let p = load_state(&r.id).path;
            let here = !p.is_empty() && campaign::is_campaign(Path::new(&p));
            json!({ "story": r, "path": here.then_some(p) })
        })
        .collect())
}

/// Get a story from the account onto this device, and say where it is. One that's already here
/// just gives its folder: opening it syncs it.
pub fn fetch(file_id: &str) -> R<String> {
    let _one = LOCK.lock().map_err(|_| "sync state lost")?;
    let r = account::list()?.into_iter().find(|r| r.file_id == file_id).ok_or("That story is no longer in your account.")?;
    let mut st = load_state(&r.id);
    if !st.path.is_empty() && campaign::is_campaign(Path::new(&st.path)) {
        return Ok(st.path);
    }
    let incoming = unzip(&account::download(&r.file_id)?)?;
    let base = library_dir().join(safe_name(&r.name));
    let dir = (1..200)
        .map(|n| if n == 1 { base.clone() } else { PathBuf::from(format!("{} ({n})", base.display())) })
        .find(|d| !d.exists())
        .ok_or("Too many stories with that name on this device.")?;
    write_all(&dir, &incoming)?;
    if !campaign::is_campaign(&dir) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err("The story in your account isn't a whole campaign.".into());
    }
    st = State { base: r.hash, path: dir.display().to_string(), foreign_cc: cc_session(&incoming) };
    save_state(&r.id, &st)?;
    Ok(st.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_changed_side_wins_and_two_ask() {
        assert_eq!(decide("a", "", None, None), Step::Up);
        assert_eq!(decide("a", "a", Some("a"), None), Step::Same);
        assert_eq!(decide("b", "a", Some("a"), None), Step::Up);
        assert_eq!(decide("a", "a", Some("b"), None), Step::Down);
        assert_eq!(decide("b", "a", Some("c"), None), Step::Conflict);
        assert_eq!(decide("b", "a", Some("c"), Some("local")), Step::Up);
        assert_eq!(decide("b", "a", Some("c"), Some("cloud")), Step::Down);
        // Never synced here, but the account has the story: ask, unless they already match.
        assert_eq!(decide("a", "", Some("b"), None), Step::Conflict);
        assert_eq!(decide("a", "", Some("a"), None), Step::Same);
    }

    #[test]
    fn a_story_survives_the_round_trip() {
        let root = std::env::temp_dir().join(format!("ck-sync-{}", uuid::Uuid::new_v4()));
        std::env::set_var("CK_DATA_DIR", root.join("data"));
        let (a, b) = (root.join("a"), root.join("b"));
        campaign::create(&a, "Trip").unwrap();
        atomic_write(&a.join(".keeper/chat.json"), br#"{"cc_session":"abc"}"#).unwrap();
        atomic_write(&a.join(".keeper/trash/old.md"), b"gone").unwrap();
        atomic_write(&a.join("03_NPCs/tobin.tmp123"), b"half").unwrap();
        let files = read_all(&a).unwrap();
        assert!(files.iter().all(|(r, _)| !r.contains("trash") && !r.contains("tmp123")));
        let back = unzip(&zip(&files).unwrap()).unwrap();
        assert_eq!(hash_of(&files), hash_of(&back));
        assert_eq!(cc_session(&back).as_deref(), Some("abc"));

        // Another device has an older copy with a file the new one dropped.
        campaign::create(&b, "Trip").unwrap();
        atomic_write(&b.join("03_NPCs/stale.md"), b"old").unwrap();
        let current = read_all(&b).unwrap();
        replace(&b, "test-id", &current, &back).unwrap();
        assert!(!b.join("03_NPCs/stale.md").exists());
        assert_eq!(hash_of(&read_all(&b).unwrap()), hash_of(&files));
        assert_eq!(std::fs::read_dir(local_dir().join("sync-backups/test-id")).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_zip_cannot_write_outside_the_folder() {
        let evil = zip(&vec![("../outside.md".to_string(), b"x".to_vec())]).unwrap();
        assert!(unzip(&evil).is_err());
        assert_eq!(safe_name("a/b:c"), "a_b_c");
        assert_eq!(safe_name(" .. "), "Story");
    }
}
