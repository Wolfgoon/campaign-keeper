//! The Ledger: a chat of its own for keeping the status windows straight, like the Scribe is for
//! the files. The player says what's wrong in plain words ("I sold the lamp for 4 silver", "add the
//! rope I found last session"); the game's model answers and proposes `<rpg>` lines. Nothing
//! changes until the player applies them, and the GM is told what changed on its next turn.
//!
//! The conversation is kept in `.keeper/ledger.json` so it survives a restart.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::campaign::keeper;
use crate::config::Config;
use crate::engine;
use crate::rpg::Rules;
use crate::util::atomic_write;

const SYSTEM: &str = include_str!("../prompts/ledger.md");
const LITRPG: &str = include_str!("../prompts/litrpg.md");
const LITRPG_AD: &str = include_str!("../prompts/litrpg_ad.md");
const KEEP: usize = 40;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Line {
    /// "user" or "ledger".
    pub role: String,
    pub text: String,
    /// The changes the Ledger proposed with this answer.
    pub changes: Vec<String>,
    /// What applying them did, line by line ("Refused: …" for those that failed). Empty until applied.
    pub results: Vec<String>,
    pub discarded: bool,
    /// Proposed on its own, every few turns, rather than asked for.
    pub auto: bool,
}

/// One change to the conversation at a time: the automatic check and a question can overlap.
pub static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// How often the Ledger checks by itself, in exchanges.
pub const AUTO_EVERY: usize = 3;

const AUTO_ASK: &str = "Automatic check: look only at the last few exchanges in <recent_story>. List anything they changed that the status window doesn't show yet: items gained, used, sold, given or lost, money, injuries, rest, conditions that began or ended. Propose only changes the story plainly shows. If nothing needs changing, reply with exactly: nothing";

/// The recent play as the Ledger reads it.
pub fn story(entries: &[crate::chat::Entry], last: usize) -> String {
    let start = entries.len().saturating_sub(last);
    entries[start..]
        .iter()
        .filter(|e| !e.extra.contains_key("private") && !e.extra.contains_key("ooc"))
        .map(|e| format!("{}: {}", if e.role == "user" { "PLAYER" } else { "GM" }, crate::util::clip(&e.display.clone().unwrap_or_else(|| e.plain()), 1500)))
        .collect::<Vec<_>>()
        .join("

")
}

/// The check every few turns. Keeps a proposal only when there's something to change.
pub fn auto(cfg: &Config, dir: &Path, rules: &Rules, story: &str) -> Result<bool, String> {
    let hist = load(dir);
    let mut line = ask(cfg, dir, rules, story, &hist, AUTO_ASK)?;
    if line.changes.is_empty() {
        return Ok(false);
    }
    line.auto = true;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut hist = load(dir);
    hist.push(line);
    save(dir, &hist)?;
    Ok(true)
}

fn path(dir: &Path) -> std::path::PathBuf {
    keeper(dir).join("ledger.json")
}

pub fn load(dir: &Path) -> Vec<Line> {
    std::fs::read_to_string(path(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, v: &[Line]) -> Result<(), String> {
    let skip = v.len().saturating_sub(KEEP);
    atomic_write(&path(dir), serde_json::to_string(&v[skip..]).unwrap_or_default().as_bytes())
}

/// The `<rpg>` lines in a reply, and the reply with them taken out.
pub fn split(reply: &str) -> (String, Vec<String>) {
    let re = regex::Regex::new(r"(?s)<rpg>(.*?)</rpg>").expect("static regex");
    let changes = re.captures_iter(reply).flat_map(|c| c[1].lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect::<Vec<_>>()).collect();
    (re.replace_all(reply, "").trim().to_string(), changes)
}

/// Ask the Ledger. `story` is the recent play, so it can count what happened.
pub fn ask(cfg: &Config, dir: &Path, rules: &Rules, story: &str, history: &[Line], message: &str) -> Result<Line, String> {
    if !rules.enabled {
        return Err("The rules are off for this game, so there's no status window to keep. Turn them on in the Status panel.".into());
    }
    let mut messages: Vec<(String, String)> = Vec::new();
    for l in history.iter().rev().take(16).rev() {
        let role = if l.role == "user" { "user" } else { "assistant" };
        let mut text = l.text.clone();
        if !l.changes.is_empty() {
            text.push_str(&format!("\n<rpg>\n{}\n</rpg>", l.changes.join("\n")));
            if l.discarded {
                text.push_str("\n(The player discarded these.)");
            } else if !l.results.is_empty() {
                text.push_str(&format!("\n(Applied: {})", l.results.join("; ")));
            }
        }
        // Roles must alternate, starting with the player.
        match messages.last_mut() {
            Some(last) if last.0 == role => last.1.push_str(&format!("\n\n{text}")),
            None if role == "assistant" => {}
            _ => messages.push((role.into(), text)),
        }
    }
    let now = format!("<status_window>\n{}\n</status_window>\n<recent_story>\n{story}\n</recent_story>\n\n{message}", rules.status_all());
    match messages.last_mut() {
        Some(last) if last.0 == "user" => last.1.push_str(&format!("\n\n{now}")),
        _ => messages.push(("user".into(), now)),
    }
    let req = engine::Request {
        system: vec![SYSTEM.to_string(), if rules.is_ad() { LITRPG_AD } else { LITRPG }.to_string()],
        messages,
        max_tokens: 4000,
        session: None,
        workdir: dir.to_path_buf(),
    };
    let reply = engine::stream(cfg, &req, &mut |_| {}, &AtomicBool::new(false))?;
    let (text, changes) = split(&reply.text);
    Ok(Line { role: "ledger".into(), text, changes, ..Default::default() })
}

/// The one character an unnamed line belongs to, when there's only one.
pub fn default_character(rules: &Rules) -> String {
    let n = rules.names();
    if n.len() == 1 { n[0].clone() } else { String::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpg_lines_are_taken_out_of_the_answer() {
        let (t, c) = split("Sold, and the lamp's gone.\n<rpg>\nitem - Lamp\n\nmoney +4\n</rpg>");
        assert_eq!(t, "Sold, and the lamp's gone.");
        assert_eq!(c, vec!["item - Lamp", "money +4"]);
        assert_eq!(split("Nothing to change.").1.len(), 0);
    }

    #[test]
    fn the_conversation_is_kept_and_capped() {
        let d = std::env::temp_dir().join(format!("ck-ledger-{}", uuid::Uuid::new_v4())).join("c");
        crate::campaign::create(&d, "T").unwrap();
        let v: Vec<Line> = (0..KEEP + 5).map(|i| Line { role: "user".into(), text: format!("{i}"), ..Default::default() }).collect();
        save(&d, &v).unwrap();
        let back = load(&d);
        assert_eq!(back.len(), KEEP);
        assert_eq!(back.last().unwrap().text, format!("{}", KEEP + 4));
    }
}
