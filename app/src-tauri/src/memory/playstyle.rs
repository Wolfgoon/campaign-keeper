//! How this player likes to play, across every campaign (Spirit's habits notebook, for a game).
//! Patterns get added, reinforced when seen again, and weakened when contradicted, never deleted.
//! The strong ones travel with the GM instructions.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::config_dir;
use crate::util::atomic_write;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Habit {
    pub id: String,
    pub text: String,
    pub strength: f64,
}

fn path() -> std::path::PathBuf {
    config_dir().join("playstyle.json")
}

pub fn load() -> Vec<Habit> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(list: &[Habit]) {
    let _ = atomic_write(&path(), serde_json::to_string_pretty(list).unwrap_or_default().as_bytes());
}

pub fn listing() -> String {
    load().iter().map(|h| format!("[{}] (strength {:.2}) {}", h.id, h.strength, h.text)).collect::<Vec<_>>().join("\n")
}

/// The patterns worth telling the GM about, strongest first.
pub fn for_gm() -> Vec<String> {
    let mut l = load();
    l.retain(|h| h.strength >= 0.3);
    l.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    l.into_iter().take(10).map(|h| h.text).collect()
}

pub fn update(v: &Value) -> String {
    let mut list = load();
    let (mut added, mut reinforced, mut weakened) = (0, 0, 0);
    for a in v["add"].as_array().into_iter().flatten().take(2) {
        let Some(text) = a["text"].as_str().map(str::trim).filter(|t| !t.is_empty()) else { continue };
        if list.iter().any(|h| h.text.eq_ignore_ascii_case(text)) {
            continue;
        }
        let id = format!("p{}", list.len() + 1);
        list.push(Habit { id, text: text.to_string(), strength: a["strength"].as_f64().unwrap_or(0.3).clamp(0.1, 0.6) });
        added += 1;
    }
    for r in v["reinforce"].as_array().into_iter().flatten() {
        if let Some(h) = r["id"].as_str().and_then(|id| list.iter_mut().find(|h| h.id == id)) {
            h.strength = (h.strength + 0.1).min(1.0);
            reinforced += 1;
        }
    }
    for id in v["weaken"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if let Some(h) = list.iter_mut().find(|h| h.id == id) {
            h.strength = (h.strength - 0.15).max(0.05);
            weakened += 1;
        }
    }
    if added + reinforced + weakened == 0 {
        return String::new();
    }
    save(&list);
    format!("; playstyle {added}+ {reinforced}^ {weakened}v")
}
