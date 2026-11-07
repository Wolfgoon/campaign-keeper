//! Builds what the GM reads. The instructions and files are a stable prefix (cheap, cached); what
//! changes every turn travels on the player's message instead.

use std::collections::BTreeMap;
use std::path::Path;

use crate::campaign::{self, keeper, INSTRUCTIONS};
use crate::chat::Session;
use crate::config::Config;
use crate::memory::playstyle;
use crate::util::tokens;

const RUNNING: &str = include_str!("../prompts/running_inside.md");
const MEMORY: &str = include_str!("../prompts/memory.md");
const LITRPG: &str = include_str!("../prompts/litrpg.md");
const MULTIPLAYER: &str = include_str!("../prompts/multiplayer.md");
const HUMAN_GM: &str = include_str!("../prompts/human_gm.md");
const FALLBACK_INSTRUCTIONS: &str = include_str!("../../../TEMPLATE/00_GM/ai_dungeon_plus_instructions.txt");

const EMPTY_NOTEBOOK: &str = "(Empty. Write it in your first reply.)

## Characters: Stats & Abilities
(One section per player character.)

## Threats, Secrets & Agendas

## Plans & Foreshadowing

## Clocks";

/// Below this, every file simply goes along: recall adds nothing when it all fits.
const SMART_THRESHOLD_TOKENS: u64 = 12_000;

pub struct Multi {
    pub host: String,
    pub human_gm: bool,
}

pub fn read_notebook(dir: &Path) -> String {
    std::fs::read_to_string(keeper(dir).join("gm_notebook.md")).unwrap_or_default()
}

/// Files that always go whole: who the character is, what's going on, what must stay true.
pub fn is_core(path: &str) -> bool {
    path.starts_with("00_GM/")
        || path.starts_with("01_Character/")
        || path.starts_with("04_Quests/active")
        || path.starts_with("05_Memory/")
        || path.starts_with("08_Saves/")
        || path.starts_with("07_Maps/")
        || path == "02_World/world_overview.md"
}

fn latest_session(files: &BTreeMap<String, String>) -> Option<String> {
    files.keys().filter(|f| f.starts_with("06_Sessions/")).next_back().cloned()
}

/// (files sent whole, files left to recall)
pub fn split_files(cfg: &Config, files: &BTreeMap<String, String>) -> (Vec<String>, Vec<String>) {
    let total: u64 = files.values().map(|t| tokens(t)).sum();
    if cfg.context_mode == "full" || !cfg.memory_enabled || total < SMART_THRESHOLD_TOKENS {
        return (files.keys().cloned().collect(), Vec::new());
    }
    let last = latest_session(files);
    files.keys().cloned().partition(|f| is_core(f) || Some(f) == last.as_ref())
}

fn kept_list(dir: &Path, sub: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(keeper(dir).join(sub))
        .map(|d| d.flatten().filter_map(|e| e.path().file_stem().map(|n| n.to_string_lossy().into_owned())).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// The two system blocks. Frozen for a chat, so the cache stays warm; rebuilt on a fresh chat or
/// when the chat is condensed.
pub fn build_system(dir: &Path, cfg: &Config, rules_on: bool, multi: Option<&Multi>) -> Vec<String> {
    let instructions = std::fs::read_to_string(dir.join(INSTRUCTIONS)).unwrap_or_else(|_| FALLBACK_INSTRUCTIONS.to_string());
    let mut head = format!("{}\n\n{RUNNING}", instructions.trim_end());
    if cfg.memory_enabled {
        head.push_str(&format!("\n{MEMORY}"));
    }
    if rules_on {
        head.push_str(&format!("\n{LITRPG}"));
    }
    if let Some(m) = multi {
        head.push_str(&format!("\n{MULTIPLAYER}"));
        if m.human_gm {
            head.push_str(&format!("\n{}", HUMAN_GM.replace("{host}", &m.host)));
        }
    }
    if cfg.playstyle_enabled {
        let style = playstyle::for_gm();
        if !style.is_empty() {
            head.push_str(&format!("\n\nHOW THIS PLAYER LIKES TO PLAY (learned across campaigns)\n- {}\n", style.join("\n- ")));
        }
    }

    let files = campaign::game_files(dir);
    let (whole, rest) = split_files(cfg, &files);
    let name = campaign::name(dir);
    let mut body = format!("=====================================================================\n  CAMPAIGN FILES\n=====================================================================\nEverything below is canon: this game's World Info and Pinned Memory.\n\n<campaign name=\"{name}\">\n");
    for f in &whole {
        body.push_str(&format!("<file path=\"{f}\">\n{}\n</file>\n", files[f].trim_end()));
    }
    body.push_str("</campaign>\n");
    if !rest.is_empty() {
        body.push_str(&format!("\n<file_index>\n{}\n</file_index>\n", rest.join("\n")));
    }
    let nb = read_notebook(dir);
    body.push_str(&format!("\n<gm_notebook>\n{}\n</gm_notebook>\n", if nb.trim().is_empty() { EMPTY_NOTEBOOK } else { nb.trim() }));
    let (pics, music) = (kept_list(dir, "pictures"), kept_list(dir, "music"));
    if !pics.is_empty() {
        body.push_str(&format!("\n<pictures>\nPictures you've already made (show one again with <picture title=\"…\"/>):\n{}\n</pictures>\n", pics.join("\n")));
    }
    if !music.is_empty() {
        body.push_str(&format!("\n<music_kept>\nTracks you've already composed (play one again with <music title=\"…\"/>):\n{}\n</music_kept>\n", music.join("\n")));
    }
    vec![head, body]
}

/// The conversation as the model should see it: condensed part first, then the live turns.
pub fn build_messages(s: &Session) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |role: &str, text: String| {
        if text.trim().is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.0 == role => {
                last.1.push_str("\n\n");
                last.1.push_str(&text);
            }
            _ => out.push((role.to_string(), text)),
        }
    };
    if !s.condensed.text.is_empty() {
        push("user", format!("<story_so_far>\n{}\n</story_so_far>", s.condensed.text));
    }
    for e in s.entries.iter().skip(s.condensed.upto) {
        // 1.x stored file updates as system-role entries: they go in as the player's.
        let role = if e.role == "assistant" { "assistant" } else { "user" };
        push(role, e.plain());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Condensed, Entry};

    #[test]
    fn messages_alternate_and_skip_what_was_condensed() {
        let mut s = Session::default();
        s.entries = vec![Entry::text("user", "a"), Entry::text("assistant", "b"), Entry::text("user", "c"), Entry::text("system", "d"), Entry::text("assistant", "e"), Entry::text("user", "f")];
        s.condensed = Condensed { upto: 2, text: "SUMMARY".into(), times: 1 };
        let m = build_messages(&s);
        assert_eq!(m[0].0, "user");
        assert!(m[0].1.contains("SUMMARY") && m[0].1.contains("c\n\nd"));
        assert!(!m.iter().any(|x| x.1 == "a" || x.1 == "b"));
        assert!(m.windows(2).all(|w| w[0].0 != w[1].0));
        assert_eq!(m.last().unwrap().1, "f");
    }

    #[test]
    fn big_campaigns_send_core_files_and_recall_the_rest() {
        let mut cfg = Config::default();
        let mut f = BTreeMap::new();
        f.insert("00_GM/settings.md".to_string(), "s".repeat(1000));
        f.insert("03_NPCs/tobin.md".to_string(), "t".repeat(60_000));
        f.insert("06_Sessions/day1.md".to_string(), "x".repeat(100));
        f.insert("06_Sessions/day2.md".to_string(), "y".repeat(100));
        let (whole, rest) = split_files(&cfg, &f);
        assert!(whole.contains(&"00_GM/settings.md".to_string()) && whole.contains(&"06_Sessions/day2.md".to_string()));
        assert_eq!(rest, vec!["03_NPCs/tobin.md", "06_Sessions/day1.md"]);
        cfg.context_mode = "full".into();
        assert!(split_files(&cfg, &f).1.is_empty());
    }
}
