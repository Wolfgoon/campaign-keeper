//! The play session: same `.keeper/chat.json` as 1.x, so old games continue. Every field this
//! version doesn't know is kept in `extra` and written back unchanged.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::campaign::keeper;
use crate::util::atomic_write;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, o: &Usage) {
        self.input_tokens += o.input_tokens;
        self.output_tokens += o.output_tokens;
        self.cache_creation_input_tokens += o.cache_creation_input_tokens;
        self.cache_read_input_tokens += o.cache_read_input_tokens;
    }
    pub fn prompt(&self) -> u64 {
        self.input_tokens + self.cache_creation_input_tokens + self.cache_read_input_tokens
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Entry {
    pub role: String,
    /// Anthropic-style content blocks.
    pub content: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_prev: Option<String>,
    /// The story text shown to players, with the app's tags taken out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// What memory fed this turn (shown on request, never sent to players).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recalled: Option<Vec<String>>,
    /// LitRPG: the checks resolved this turn and what changed on the sheets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpg: Option<Value>,
    /// Multiplayer metadata and everything else.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Entry {
    pub fn text(role: &str, text: &str) -> Entry {
        Entry { role: role.into(), content: json!([{ "type": "text", "text": text }]), ..Default::default() }
    }

    pub fn plain(&self) -> String {
        match &self.content {
            Value::String(s) => s.clone(),
            Value::Array(b) => b.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n"),
            _ => String::new(),
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        self.extra.get(key).and_then(Value::as_bool).unwrap_or(false)
    }
}

/// What condensing has carried forward so far.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Condensed {
    /// Entries before this index are represented by `text` and no longer sent.
    pub upto: usize,
    pub text: String,
    pub times: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Session {
    pub version: u32,
    pub started: String,
    pub system: Vec<String>,
    pub known: BTreeMap<String, u64>,
    pub entries: Vec<Entry>,
    pub total: Usage,
    pub cost: f64,
    pub last_prompt_tokens: u64,
    pub cc_session: Option<String>,
    pub undo_note: bool,
    pub activities: Vec<Value>,
    pub activity_seq: u64,
    pub multiplayer: Option<Value>,

    // New in 2.0
    /// Hash scheme of `known`; 1.x used another, so an old session is re-baselined once.
    pub known_v: u32,
    pub condensed: Condensed,
    /// Replies since the keeper last ran, and the entry it reached.
    pub keeper_pending: u64,
    pub keeper_upto: usize,
    /// Memory ids put in front of the GM lately: recall doesn't repeat itself.
    pub recall_seen: Vec<String>,
    /// Set when the next Claude Code turn must start a new session from the condensed list.
    pub cc_reset: bool,
    /// Changes the rules refused, for the GM's next turn.
    pub rpg_notices: Vec<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            version: 1,
            started: crate::util::now(),
            system: Vec::new(),
            known: BTreeMap::new(),
            entries: Vec::new(),
            total: Usage::default(),
            cost: 0.0,
            last_prompt_tokens: 0,
            cc_session: None,
            undo_note: false,
            activities: Vec::new(),
            activity_seq: 0,
            multiplayer: None,
            known_v: 0,
            condensed: Condensed::default(),
            keeper_pending: 0,
            keeper_upto: 0,
            recall_seen: Vec::new(),
            cc_reset: false,
            rpg_notices: Vec::new(),
            extra: Map::new(),
        }
    }
}

pub fn load(dir: &Path) -> Option<Session> {
    let text = std::fs::read_to_string(keeper(dir).join("chat.json")).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(dir: &Path, s: &Session) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    atomic_write(&keeper(dir).join("chat.json"), &data)
}

/// Put the chat away in `.keeper/chats/` and start fresh. Nothing is deleted.
pub fn archive(dir: &Path) -> Result<(), String> {
    let from = keeper(dir).join("chat.json");
    if from.exists() {
        let to = keeper(dir).join("chats").join(format!("chat-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        std::fs::create_dir_all(to.parent().unwrap_or(dir)).map_err(|e| e.to_string())?;
        std::fs::rename(from, to).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_sessions_keep_every_field() {
        let old = json!({
            "version": 1, "started": "2026-10-23 16:58", "system": ["a", "b"], "known": {"x.md": 13888283755533273295u64},
            "entries": [
                {"role": "assistant", "content": [{"type":"text","text":"hi"}], "usage": {"input_tokens": 2},
                 "model": "claude-opus-5-5", "notebook_prev": "n", "pictures": [{"title":"Key"}], "saved_as": "08_Saves/a.md"}
            ],
            "total": {"input_tokens": 6}, "cost": 0.0, "last_prompt_tokens": 1, "cc_session": "abc", "undo_note": false,
            "activities": [], "activity_seq": 0, "multiplayer": null, "some_future_field": 7
        });
        let s: Session = serde_json::from_value(old).unwrap();
        assert_eq!(s.known_v, 0);
        assert_eq!(s.entries[0].plain(), "hi");
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["entries"][0]["pictures"][0]["title"], "Key");
        assert_eq!(back["entries"][0]["saved_as"], "08_Saves/a.md");
        assert_eq!(back["some_future_field"], 7);
    }
}
