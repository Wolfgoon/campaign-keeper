//! The three ways to reach a model. Everything above this module sees one streaming call and one
//! small one-shot call (`helper`), whichever engine the player chose.

pub mod anthropic;
pub mod claude_code;
pub mod openai;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use crate::chat::Usage;
use crate::config::Config;

pub struct Request {
    pub system: Vec<String>,
    /// (role, text); roles alternate and the first is "user".
    pub messages: Vec<(String, String)>,
    pub max_tokens: u64,
    /// Claude Code only: continue this session and send just the last message.
    pub session: Option<String>,
    pub workdir: PathBuf,
}

#[derive(Default, Debug)]
pub struct Reply {
    pub text: String,
    pub usage: Usage,
    pub model: String,
    pub session: Option<String>,
    /// True when the reply stopped at the output limit.
    pub truncated: bool,
}

pub fn stream(cfg: &Config, req: &Request, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
    match cfg.engine.as_str() {
        "api" => anthropic::stream(cfg, req, on_delta, cancel),
        "other" => openai::stream(cfg, req, on_delta, cancel),
        _ => claude_code::stream(cfg, req, on_delta, cancel),
    }
}

/// A small background job (keeper, condenser, referee). Never touches the game's chat.
pub fn helper(cfg: &Config, system: &str, user: &str) -> Result<String, String> {
    let model = helper_model(cfg);
    match cfg.engine.as_str() {
        "api" => anthropic::once(cfg, &model, system, user),
        "other" => openai::once(cfg, &model, system, user),
        _ => claude_code::once(cfg, &model, system, user),
    }
}

fn helper_model(cfg: &Config) -> String {
    let m = cfg.helper_model.trim();
    if !m.is_empty() && m != "auto" {
        return m.to_string();
    }
    match cfg.engine.as_str() {
        "api" => "claude-haiku-5-5".into(),
        "other" => cfg.other_model.clone(),
        _ => "haiku".into(),
    }
}

pub fn price_per_mtok(model: &str) -> Option<(f64, f64)> {
    let m = model.to_lowercase();
    Some(if m.contains("fable") {
        (10.0, 50.0)
    } else if m.contains("opus") {
        (4.0, 20.0)
    } else if m.contains("sonnet") {
        (2.0, 10.0)
    } else if m.contains("haiku") {
        (0.10, 0.50)
    } else {
        return None;
    })
}

/// Rough cost of one reply on the API. Cache reads are a tenth of the input price, writes 1.25x.
pub fn cost(model: &str, u: &Usage) -> f64 {
    let Some((i, o)) = price_per_mtok(model) else { return 0.0 };
    (u.input_tokens as f64 * i + u.cache_creation_input_tokens as f64 * i * 1.25 + u.cache_read_input_tokens as f64 * i * 0.1 + u.output_tokens as f64 * o)
        / 1_000_000.0
}
