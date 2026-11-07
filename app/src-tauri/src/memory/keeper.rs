//! The Memory Keeper: a separate model whose only job is to fold recent play into long-term
//! memory. It reads only what the player has seen, never the GM's notebook, so nothing in the
//! Memory tab can spoil a secret.

use std::path::Path;

use serde_json::Value;

use super::store::Store;
use super::{playstyle, transcript};
use crate::chat::Entry;
use crate::config::Config;
use crate::util::{atomic_write, json_object, tail};

const PROMPT: &str = "\
You are the Memory Keeper for a text-adventure campaign. You are not the Game Master and you never talk to the player. \
You get the player's existing memories on these subjects, what you know of how they play, and the latest stretch of the game. \
Reply with ONLY a JSON object:
{\"journal\": \"2-5 sentences of what happened, with in-world dates\",
 \"memories\": [{\"text\": \"one durable fact, clear on its own\", \"kind\": \"fact|event|npc|place|promise\", \"tags\": [\"names\", \"places\"], \"importance\": 0.0}],
 \"playstyle\": {\"add\": [{\"text\": \"a recurring way this player likes to play\", \"strength\": 0.3}],
                 \"reinforce\": [{\"id\": \"id of a pattern seen again\"}], \"weaken\": [\"id of a contradicted pattern\"]}}
Good memories: who someone is and how they feel about the character, promises, debts and threats, clues, what was \
found and where it is, how places connect, turning points. Quote exact words of promises, inscriptions and clues. \
Importance: 0.9 for what the story turns on, 0.5 for useful detail, 0.2 for colour. Skip what the GM narrates in passing. \
Use absolute in-world dates, never \"yesterday\". Merge rather than repeat what's already listed, and never invent. \
Playstyle is about the PLAYER, not the character, and only patterns that recur: how they like to be answered, what they \
enjoy, what they catch. At most 2 additions. Use empty lists when nothing fits.";

const DUPLICATE: f32 = 0.92;
const KINDS: [&str; 5] = ["fact", "event", "npc", "place", "promise"];

/// Run over `entries`. Returns a one-line summary for the app's activity line.
pub fn run(cfg: &Config, dir: &Path, entries: &[Entry]) -> Result<String, String> {
    let text = transcript(entries);
    if text.trim().is_empty() {
        return Ok("nothing new".into());
    }
    let store = Store::open(dir)?;
    let known = store.search(tail(&text, 2000), 15, &[], Some(-1))?;
    let known: Vec<String> = known.iter().filter(|m| KINDS.contains(&m.kind.as_str())).map(|m| format!("- {}", m.text)).collect();
    let style = playstyle::listing();
    let user = format!(
        "EXISTING MEMORIES:\n{}\n\nHOW THIS PLAYER PLAYS:\n{}\n\nTHE LATEST STRETCH:\n{}",
        if known.is_empty() { "(none yet)".to_string() } else { known.join("\n") },
        if style.is_empty() { "(nothing noted yet)" } else { &style },
        tail(&text, 120_000)
    );
    let reply = crate::engine::helper(cfg, PROMPT, &user)?;
    let v = json_object(&reply).ok_or_else(|| format!("the keeper gave no JSON: {}", crate::util::clip(&reply, 120)))?;
    file(dir, &store, &v)
}

/// File a keeper's answer. Separate from the model call so it can be tested.
pub fn file(dir: &Path, store: &Store, v: &Value) -> Result<String, String> {
    let (mut added, mut merged) = (0, 0);
    for m in v["memories"].as_array().into_iter().flatten() {
        let Some(text) = m["text"].as_str().map(str::trim).filter(|t| !t.is_empty()) else { continue };
        let importance = m["importance"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0);
        let kind = m["kind"].as_str().filter(|k| KINDS.contains(k)).unwrap_or("fact");
        let tags: Vec<String> = m["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(str::to_string)).collect();
        match store.nearest(text, &KINDS)? {
            Some((id, sim)) if sim >= DUPLICATE => {
                let old = store.get(&id)?.map_or(0.0, |o| o.importance);
                store.update(&id, Some(text), Some(old.max(importance)), None)?;
                merged += 1;
            }
            _ => {
                store.add(text, kind, &tags, "keeper", importance, -1)?;
                added += 1;
            }
        }
    }
    if let Some(j) = v["journal"].as_str().map(str::trim).filter(|j| !j.is_empty()) {
        let now = chrono::Local::now();
        let path = crate::campaign::keeper(dir).join("journal").join(format!("{}.md", now.format("%Y-%m")));
        let mut t = std::fs::read_to_string(&path).unwrap_or_default();
        t.push_str(&format!("\n## {}\n{j}\n", now.format("%Y-%m-%d %H:%M")));
        atomic_write(&path, t.as_bytes())?;
    }
    let styled = playstyle::update(&v["playstyle"]);
    store.export_backup()?;
    Ok(format!("{added} new, {merged} merged{styled}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_memories_without_duplicates_and_backs_up() {
        let dir = std::env::temp_dir().join(format!("ck-keeper-{}", uuid::Uuid::new_v4()));
        std::env::set_var("CK_DATA_DIR", dir.join("data"));
        std::env::set_var("CK_NO_EMBED", "1");
        let camp = dir.join("camp");
        crate::campaign::create(&camp, "T").unwrap();
        let store = Store::open(&camp).unwrap();
        let v = serde_json::json!({
            "journal": "Wren found the button.",
            "memories": [
                {"text":"The brass button reads G.H.L.","kind":"fact","tags":["button"],"importance":0.8},
                {"text":"Hale owes Wren an apology","kind":"nonsense","importance":9},
                {"text":"  "}
            ]
        });
        assert!(file(&camp, &store, &v).unwrap().starts_with("2 new"));
        let again = serde_json::json!({"memories":[{"text":"the brass button reads g.h.l.","importance":0.2}]});
        assert!(file(&camp, &store, &again).unwrap().starts_with("0 new, 1 merged"));
        let found = store.search("brass button", 3, &[], None).unwrap();
        assert_eq!(found[0].importance, 0.8); // never lowered by a merge
        assert!(crate::campaign::keeper(&camp).join("memory-backup.jsonl").exists());
        let journal = std::fs::read_dir(crate::campaign::keeper(&camp).join("journal")).unwrap().count();
        assert_eq!(journal, 1);
    }
}
