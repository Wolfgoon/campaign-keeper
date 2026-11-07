//! The Claude API over raw HTTP (there is no official Rust SDK). Streaming with prompt caching:
//! the instructions and campaign files are a stable prefix, so later turns read them from cache.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use super::{Reply, Request};
use crate::chat::Usage;
use crate::config::Config;

const URL: &str = "https://api.anthropic.com/v1/messages";
const VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

fn base_url() -> String {
    std::env::var("CK_ANTHROPIC_URL").unwrap_or_else(|_| URL.to_string())
}

fn key(cfg: &Config) -> Result<String, String> {
    let k = cfg.resolved_api_key();
    if k.is_empty() {
        return Err("Add your Anthropic API key in Settings, or switch to Claude Code.".into());
    }
    Ok(k)
}

/// `fallbacks: "default"` works on the models whose classifiers can flag ordinary fiction.
fn supports_fallbacks(model: &str) -> bool {
    ["claude-opus-5", "claude-sonnet-5-5", "claude-fable-5"].iter().any(|p| model.starts_with(p))
}

fn body(cfg: &Config, req: &Request) -> Value {
    let mut system: Vec<Value> = req.system.iter().map(|t| json!({ "type": "text", "text": t })).collect();
    if let Some(last) = system.last_mut() {
        last["cache_control"] = json!({ "type": "ephemeral" });
    }
    let messages: Vec<Value> = req.messages.iter().map(|(r, t)| json!({ "role": r, "content": t })).collect();
    let mut b = json!({
        "model": cfg.model,
        "max_tokens": req.max_tokens,
        "stream": true,
        "system": system,
        "messages": messages,
        // Moves the cache point forward with the conversation.
        "cache_control": { "type": "ephemeral" },
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": cfg.effort },
    });
    if cfg.fallbacks && supports_fallbacks(&cfg.model) {
        b["fallbacks"] = json!("default");
    }
    b
}

fn post(cfg: &Config, api_key: &str, body: &Value, cancel: Option<&AtomicBool>) -> Result<ureq::Response, String> {
    let wants_beta = body.get("fallbacks").is_some();
    for attempt in 0..4u32 {
        let mut r = ureq::post(&base_url())
            .timeout(Duration::from_secs(1800))
            .set("x-api-key", api_key)
            .set("anthropic-version", VERSION)
            .set("content-type", "application/json")
            .set("user-agent", concat!("campaign-keeper/", env!("CARGO_PKG_VERSION")));
        if wants_beta {
            r = r.set("anthropic-beta", FALLBACK_BETA);
        }
        match r.send_json(body.clone()) {
            Ok(resp) => return Ok(resp),
            Err(ureq::Error::Status(code, resp)) => {
                let wait = resp.header("retry-after").and_then(|h| h.parse::<u64>().ok()).unwrap_or(2u64.pow(attempt + 1));
                let text = resp.into_string().unwrap_or_default();
                let retryable = matches!(code, 429 | 500 | 502 | 503 | 529);
                if retryable && attempt < 3 {
                    for _ in 0..wait.min(30) * 10 {
                        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                            return Err("stopped".into());
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    continue;
                }
                return Err(friendly(code, &text));
            }
            Err(e) if attempt < 3 => {
                eprintln!("[api] {e}");
                std::thread::sleep(Duration::from_secs(1 << attempt));
            }
            Err(e) => return Err(format!("Couldn't reach the API: {e}")),
        }
    }
    let _ = cfg;
    Err("The API is busy. Try again shortly.".into())
}

fn friendly(code: u16, text: &str) -> String {
    let msg = serde_json::from_str::<Value>(text).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or_else(|| text.to_string());
    match code {
        401 => "The API key was rejected. Check it in Settings.".into(),
        429 => "The API is rate limiting this key. Wait a moment and try again.".into(),
        529 => "Anthropic's API is overloaded right now. Try again shortly.".into(),
        _ => format!("The service reported an error ({code}): {msg}"),
    }
}

fn usage_from(v: &Value, u: &mut Usage) {
    let get = |k: &str| v[k].as_u64();
    if let Some(n) = get("input_tokens") {
        u.input_tokens = n;
    }
    if let Some(n) = get("output_tokens") {
        u.output_tokens = n;
    }
    if let Some(n) = get("cache_creation_input_tokens") {
        u.cache_creation_input_tokens = n;
    }
    if let Some(n) = get("cache_read_input_tokens") {
        u.cache_read_input_tokens = n;
    }
}

pub fn stream(cfg: &Config, req: &Request, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
    let api_key = key(cfg)?;
    let resp = post(cfg, &api_key, &body(cfg, req), Some(cancel))?;
    let reader = BufReader::new(resp.into_reader());
    let mut reply = Reply { model: cfg.model.clone(), ..Default::default() };
    let mut stop = String::new();
    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        let line = line.map_err(|e| format!("the connection closed before the reply finished: {e}"))?;
        let Some(data) = line.strip_prefix("data: ") else { continue };
        let Ok(ev) = serde_json::from_str::<Value>(data) else { continue };
        match ev["type"].as_str().unwrap_or("") {
            "message_start" => {
                usage_from(&ev["message"]["usage"], &mut reply.usage);
                if let Some(m) = ev["message"]["model"].as_str() {
                    reply.model = m.to_string();
                }
            }
            "content_block_delta" => {
                if ev["delta"]["type"] == "text_delta" {
                    if let Some(t) = ev["delta"]["text"].as_str() {
                        reply.text.push_str(t);
                        on_delta(t);
                    }
                }
            }
            "message_delta" => {
                usage_from(&ev["usage"], &mut reply.usage);
                if let Some(s) = ev["delta"]["stop_reason"].as_str() {
                    stop = s.to_string();
                }
            }
            "error" => return Err(ev["error"]["message"].as_str().unwrap_or("The request stopped unexpectedly.").to_string()),
            _ => {}
        }
    }
    match stop.as_str() {
        // Branch on stop_reason before trusting the text: a mid-stream refusal's partial is discarded.
        "refusal" => Err("The model declined this request. Try rephrasing it.".into()),
        "max_tokens" => {
            reply.truncated = true;
            Ok(reply)
        }
        "" => Err("The request stopped unexpectedly.".into()),
        _ => Ok(reply),
    }
}

/// One small non-streaming answer for background work.
pub fn once(cfg: &Config, model: &str, system: &str, user: &str) -> Result<String, String> {
    let api_key = key(cfg)?;
    let b = json!({
        "model": model, "max_tokens": 8000, "system": system,
        "messages": [{ "role": "user", "content": user }],
        "output_config": { "effort": "low" },
    });
    let v: Value = post(cfg, &api_key, &b, None)?.into_json().map_err(|e| e.to_string())?;
    if v["stop_reason"] == "refusal" {
        return Err("the helper declined".into());
    }
    Ok(v["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(""))
}
