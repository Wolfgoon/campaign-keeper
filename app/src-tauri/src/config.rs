//! App settings. Same file and field names as Campaign Keeper 1.x (config.json), so an upgrade
//! keeps everything; the new fields have defaults. Unknown fields are carried along untouched.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::util::atomic_write;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// "claude_code", "api", "deepseek" or "other".
    pub engine: String,
    pub claude_path: String,
    pub api_key: String,
    pub deepseek_key: String,
    pub remember_key: bool,
    pub model: String,
    pub effort: String,
    pub max_tokens: u64,
    pub fallbacks: bool,
    pub recent: Vec<String>,
    pub last_open: String,
    pub dark_mode: bool,
    pub ui_scale: f64,
    pub other_url: String,
    pub other_key: String,
    pub other_model: String,
    pub player_name: String,
    pub join_addr: String,
    pub music_volume: f64,
    pub music_muted: bool,
    pub tts_auto: bool,
    pub tts_speed: f64,

    // New in 2.0: the memory system.
    /// Master switch for recall, the keeper and the condenser.
    pub memory_enabled: bool,
    /// Long-term memories attached to each turn.
    pub recall_k: usize,
    /// The chat is condensed when the context passes this many tokens.
    pub context_limit: u64,
    /// The Memory Keeper runs after this many of the GM's replies.
    pub keeper_every: u64,
    /// "auto", or a model id, for the small background jobs.
    pub helper_model: String,
    /// "smart" sends core files whole and recalls the rest; "full" sends every file every turn.
    pub context_mode: String,
    /// Have the helper check each reply against what's remembered.
    pub continuity_check: bool,
    pub playstyle_enabled: bool,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            engine: "claude_code".into(),
            claude_path: String::new(),
            api_key: String::new(),
            deepseek_key: String::new(),
            remember_key: false,
            model: "claude-opus-5-5".into(),
            effort: "medium".into(),
            max_tokens: 32000,
            fallbacks: true,
            recent: Vec::new(),
            last_open: String::new(),
            dark_mode: true,
            ui_scale: 1.0,
            other_url: String::new(),
            other_key: String::new(),
            other_model: String::new(),
            player_name: String::new(),
            join_addr: String::new(),
            music_volume: 0.5,
            music_muted: false,
            tts_auto: false,
            tts_speed: 1.0,
            memory_enabled: true,
            recall_k: 6,
            context_limit: 60_000,
            keeper_every: 4,
            helper_model: "auto".into(),
            context_mode: "smart".into(),
            continuity_check: false,
            playstyle_enabled: true,
            extra: Map::new(),
        }
    }
}

/// Settings live where 1.x kept them. `CK_DATA_DIR` moves everything, for tests.
pub fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CK_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("campaign-keeper")
}

/// Machine-local data that must stay out of OneDrive and the like: Spirit learned the hard way
/// that sync tools and live databases don't mix. What should travel goes in `.keeper/` as text.
pub fn local_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CK_DATA_DIR") {
        return PathBuf::from(d).join("local");
    }
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("Campaign Keeper")
}

impl Config {
    pub fn load() -> Config {
        std::fs::read_to_string(config_dir().join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let mut c = self.clone();
        if !c.remember_key {
            c.api_key.clear();
        }
        atomic_write(&config_dir().join("config.json"), serde_json::to_string_pretty(&c).unwrap_or_default().as_bytes())
    }

    pub fn add_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(12);
        self.last_open = path.to_string();
    }

    pub fn resolved_api_key(&self) -> String {
        if !self.api_key.trim().is_empty() {
            return self.api_key.trim().to_string();
        }
        std::env::var("ANTHROPIC_API_KEY").unwrap_or_default()
    }

    pub fn resolved_deepseek_key(&self) -> String {
        if !self.deepseek_key.trim().is_empty() {
            return self.deepseek_key.trim().to_string();
        }
        if self.other_url.to_lowercase().contains("deepseek") && !self.other_key.trim().is_empty() {
            return self.other_key.trim().to_string();
        }
        std::env::var("DEEPSEEK_API_KEY").unwrap_or_default()
    }
}
