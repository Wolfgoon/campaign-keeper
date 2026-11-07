//! Memory, after Spirit's: a keeper that files what matters, a condenser that keeps the working
//! context small, recall that brings the right things back, and a playstyle notebook of what
//! keeps recurring. A failing step never blocks the game: the turn goes on without it.

pub mod embed;
pub mod keeper;
pub mod playstyle;
pub mod store;

use std::path::Path;

use serde_json::Value;

use crate::chat::{Condensed, Entry, Session};
use crate::config::Config;
use crate::util::{clip, tokens};
use store::{Memory, Store};

/// Share of the limit a condense keeps, as in Spirit: it happens every several turns, not every one.
const KEEP_FRACTION: f64 = 0.75;
/// Instructions and files, which count toward the context too.
const CONDENSE_MAX_CHARS: usize = 400_000;
/// Recall doesn't repeat what the GM saw this recently.
const SEEN_WINDOW: usize = 30;

const CONDENSE_PROMPT: &str = "\
You condense the older part of a long text-adventure game between a player and a Game Master. This part is about to \
leave the GM's working memory. Write a list of EVERYTHING IMPORTANT in it, so the GM can carry on without it:
- what happened, in order, with in-world days and times;
- who the characters met, what they said that matters (quote promises, threats and clues exactly), how each NPC now \
feels about them;
- exact numbers: money, supplies, wounds, distances, prices, deadlines;
- items gained, lost or moved, and where they are now;
- places visited and how they connect;
- every open thread and exactly where it left off;
- what the player character knows, and what they don't.
If the text starts with an earlier condensed list, carry forward everything in it that still matters, updating what \
has changed. A plain bullet list under short headings, at most about 900 words, no preamble. Never invent anything.";

/// The readable text of a stretch of the chat.
pub fn transcript(entries: &[Entry]) -> String {
    let mut parts = Vec::new();
    for e in entries {
        if e.flag("private") {
            continue;
        }
        let who = match e.role.as_str() {
            "assistant" => "GM",
            "user" => "PLAYER",
            _ => continue,
        };
        let body = match (&e.display, e.role.as_str()) {
            (Some(d), _) if !d.is_empty() => d.clone(),
            _ => crate::tags::parse(&e.plain()).display,
        };
        if !body.trim().is_empty() {
            parts.push(format!("{who}: {}", body.trim()));
        }
    }
    parts.join("\n\n")
}

/// Index a just-finished exchange verbatim, so exact words can be recalled long after they've
/// been condensed away.
pub fn store_turn(dir: &Path, player: &str, gm: &str, turn: usize) -> Result<(), String> {
    let s = Store::open(dir)?;
    s.add(&clip(&format!("PLAYER: {player}\nGM: {gm}"), 1600), "turn", &[], &format!("turn:{turn}"), 0.3, turn as i64)?;
    Ok(())
}

pub fn forget_turn(dir: &Path, turn: usize) {
    if let Ok(s) = Store::open(dir) {
        let _ = s.delete_source(&format!("turn:{turn}"));
    }
}

pub struct Recall {
    pub block: String,
    pub lines: Vec<String>,
    pub ids: Vec<String>,
}

fn memory_line(m: &Memory) -> String {
    let label = match m.kind.as_str() {
        "turn" => "earlier exchange",
        "file" => "campaign file",
        k => k,
    };
    format!("({label}, {}) {}", m.created.get(..10).unwrap_or(""), m.text)
}

/// What the GM should be reminded of for this action: searched on the action and the scene it
/// happens in. Files the GM has whole aren't repeated; nor are memories it saw lately, nor turns
/// still in the chat.
pub fn recall(dir: &Path, cfg: &Config, query: &str, session: &Session, core: &[String]) -> Result<Recall, String> {
    let s = Store::open(dir)?;
    let mut skip = session.recall_seen.clone();
    if cfg.context_mode != "full" {
        // Chunks of files already sent whole are noise.
        for m in s.list(&["file"], 100_000)? {
            if core.iter().any(|f| m.source == format!("file:{f}")) {
                skip.push(m.id);
            }
        }
    } else {
        for m in s.list(&["file"], 100_000)? {
            skip.push(m.id);
        }
    }
    let hits = s.search(query, cfg.recall_k, &skip, Some(session.condensed.upto as i64))?;
    let lines: Vec<String> = hits.iter().map(memory_line).collect();
    let ids = hits.iter().map(|h| h.id.clone()).collect();
    let block = if lines.is_empty() {
        String::new()
    } else {
        format!("<recalled_memories>\nThings from earlier in this campaign that may matter now. Canon, but only use what fits.\n- {}\n</recalled_memories>", lines.join("\n- "))
    };
    Ok(Recall { block, lines, ids })
}

pub fn note_seen(session: &mut Session, ids: &[String]) {
    session.recall_seen.extend(ids.iter().cloned());
    let n = session.recall_seen.len();
    if n > SEEN_WINDOW {
        session.recall_seen.drain(..n - SEEN_WINDOW);
    }
}

/// Where to cut so the part kept fits KEEP_FRACTION of the limit. The kept part starts on a
/// player's message. None when nothing needs condensing.
pub fn cut_point(entries: &[Entry], from: usize, overhead: u64, limit: u64) -> Option<usize> {
    let target = ((limit as f64 * KEEP_FRACTION) as u64).saturating_sub(overhead).max(4_000);
    let mut kept = 0;
    let mut cut = entries.len();
    while cut > from && kept + tokens(&entries[cut - 1].plain()) <= target {
        cut -= 1;
        kept += tokens(&entries[cut].plain());
    }
    while cut < entries.len() && entries[cut].role != "user" {
        cut += 1;
    }
    // Always keep the newest exchange, which /undo and /retry act on.
    let cut = cut.min(entries.len().saturating_sub(2));
    (cut > from).then_some(cut)
}

/// Condense everything before the cut. The chat itself isn't changed: nothing is deleted, only
/// what gets sent to the model.
pub fn condense(cfg: &Config, session: &Session, overhead: u64) -> Result<Option<Condensed>, String> {
    let Some(cut) = cut_point(&session.entries, session.condensed.upto, overhead, cfg.context_limit) else { return Ok(None) };
    let mut text = String::new();
    if !session.condensed.text.is_empty() {
        text.push_str(&format!("EARLIER CONDENSED LIST:\n{}\n\n", session.condensed.text));
    }
    text.push_str(&transcript(&session.entries[session.condensed.upto..cut]));
    if text.len() > CONDENSE_MAX_CHARS {
        text = crate::util::tail(&text, CONDENSE_MAX_CHARS).to_string();
    }
    let list = crate::engine::helper(cfg, CONDENSE_PROMPT, &text)?;
    if list.trim().is_empty() {
        return Err("the condenser returned nothing".into());
    }
    Ok(Some(Condensed { upto: cut, text: list.trim().to_string(), times: session.condensed.times + 1 }))
}

pub fn status(dir: &Path) -> Value {
    let (counts, pending) = match Store::open(dir) {
        Ok(s) => (s.counts().unwrap_or_default(), s.unembedded().unwrap_or(0)),
        Err(_) => (Default::default(), 0),
    };
    serde_json::json!({ "counts": counts, "pending": pending, "embeddings": embed::status() })
}

/// Background upkeep after opening a campaign or changing files.
pub fn index_in_background(dir: &Path) {
    let dir = dir.to_path_buf();
    embed::warm();
    std::thread::spawn(move || {
        if let Ok(s) = Store::open(&dir) {
            let _ = s.sync_files(&dir);
            // Wait for the model, then work through everything without a vector.
            for _ in 0..600 {
                match embed::status().as_str() {
                    "ready" => break,
                    "loading" => std::thread::sleep(std::time::Duration::from_secs(1)),
                    _ => return,
                }
            }
            while s.embed_pending(32).unwrap_or(0) > 0 {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(role: &str, chars: usize) -> Entry {
        Entry::text(role, &"x".repeat(chars))
    }

    #[test]
    fn nothing_to_cut_when_small() {
        let e = vec![long("user", 400), long("assistant", 400)];
        assert_eq!(cut_point(&e, 0, 1000, 60_000), None);
    }

    #[test]
    fn cut_lands_on_a_player_message_and_keeps_the_newest_exchange() {
        let mut e = Vec::new();
        for _ in 0..40 {
            e.push(long("user", 8_000));
            e.push(long("assistant", 40_000));
        }
        let cut = cut_point(&e, 0, 10_000, 60_000).unwrap();
        assert_eq!(e[cut].role, "user");
        assert!(cut > 0 && cut <= e.len() - 2);
        let kept: u64 = e[cut..].iter().map(|x| tokens(&x.plain())).sum();
        assert!(kept <= 45_000);
    }

    #[test]
    fn a_giant_last_message_still_stays() {
        let e = vec![long("user", 10), long("assistant", 10), long("user", 900_000), long("assistant", 900_000)];
        assert_eq!(cut_point(&e, 0, 10_000, 60_000), Some(2));
    }

    #[test]
    fn transcript_hides_secrets_and_private_chat() {
        let mut a = Entry::text("assistant", "The door opens.<gm_notebook>the butler</gm_notebook>");
        a.display = None;
        let mut p = Entry::text("user", "plan twist");
        p.extra.insert("private".into(), serde_json::json!(true));
        let t = transcript(&[a, p]);
        assert!(t.contains("door") && !t.contains("butler") && !t.contains("twist"));
    }
}
