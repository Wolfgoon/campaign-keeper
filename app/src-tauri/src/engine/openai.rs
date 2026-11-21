//! OpenAI-compatible servers: the free hosted tiers (Gemini, OpenRouter, Mistral, GLM, …),
//! DeepSeek, Xiaomi MiMo, and local servers like Ollama and LM Studio.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use super::{Reply, Request};
use crate::config::Config;

fn url(cfg: &Config, path: &str) -> Result<String, String> {
    let b = cfg.other_url.trim().trim_end_matches('/');
    if b.is_empty() {
        return Err("Enter the service's address in Settings (Other models).".into());
    }
    Ok(format!("{b}{path}"))
}

/// A key kept out of the settings file: a service's own variable name, read from the environment
/// so it can live in a shell profile instead of config.json.
fn env_var(url: &str) -> Option<&'static str> {
    if url.to_lowercase().contains("deepseek") {
        Some("DEEPSEEK_API_KEY")
    } else {
        None
    }
}

/// The key to send: the one in Settings, else the service's environment variable.
fn key(cfg: &Config) -> Option<String> {
    let k = cfg.other_key.trim();
    if !k.is_empty() {
        return Some(k.to_string());
    }
    let from_env = std::env::var(env_var(&cfg.other_url)?).ok()?;
    let from_env = from_env.trim();
    (!from_env.is_empty()).then(|| from_env.to_string())
}

fn call(cfg: &Config, body: Value) -> Result<ureq::Response, String> {
    let mut r = ureq::post(&url(cfg, "/chat/completions")?)
        .timeout(Duration::from_secs(1800))
        .set("content-type", "application/json")
        .set("http-referer", "https://github.com/campaign-keeper")
        .set("x-title", "Campaign Keeper");
    if let Some(k) = key(cfg) {
        r = r.set("authorization", &format!("Bearer {k}"));
    }
    match r.send_json(body) {
        Ok(x) => Ok(x),
        Err(ureq::Error::Status(c, resp)) => Err(format!("The service reported an error ({c}): {}", crate::util::clip(&resp.into_string().unwrap_or_default(), 300))),
        Err(e) => Err(format!("Couldn't reach {}: {e}", cfg.other_url)),
    }
}

pub fn stream(cfg: &Config, req: &Request, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
    if cfg.other_model.trim().is_empty() {
        return Err("Choose a model in Settings (Other models).".into());
    }
    let mut messages = vec![json!({ "role": "system", "content": req.system.join("\n\n") })];
    messages.extend(req.messages.iter().map(|(r, t)| json!({ "role": r, "content": t })));
    let mut body = json!({ "model": cfg.other_model, "stream": true, "max_tokens": req.max_tokens, "messages": messages, "stream_options": { "include_usage": true } });
    // Hosted services cap output below the app's ceiling (DeepSeek's chat model at 8K) and refuse
    // a larger max_tokens outright, and a few don't know stream_options. On a request error, try
    // once more with only the basics; they then use their own limits.
    let resp = match call(cfg, body.clone()) {
        Err(e) if e.starts_with("The service reported an error (400") || e.starts_with("The service reported an error (422") => {
            if let Some(o) = body.as_object_mut() {
                o.remove("max_tokens");
                o.remove("stream_options");
            }
            call(cfg, body).map_err(|e2| if e2 == e { e2 } else { format!("{e2}\n(First try: {e})") })?
        }
        r => r?,
    };
    let mut reply = Reply { model: cfg.other_model.clone(), ..Default::default() };
    for line in BufReader::new(resp.into_reader()).lines() {
        if cancel.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        let line = line.map_err(|e| e.to_string())?;
        let Some(data) = line.strip_prefix("data: ") else { continue };
        if data.trim() == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
        if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
            reply.text.push_str(t);
            on_delta(t);
        }
        if v["choices"][0]["finish_reason"] == "length" {
            reply.truncated = true;
        }
        if let Some(n) = v["usage"]["prompt_tokens"].as_u64() {
            reply.usage.input_tokens = n;
            reply.usage.output_tokens = v["usage"]["completion_tokens"].as_u64().unwrap_or(0);
        }
    }
    if reply.usage.output_tokens == 0 {
        reply.usage.output_tokens = crate::util::tokens(&reply.text);
    }
    if reply.usage.input_tokens == 0 {
        // Local servers often report nothing; the budget still needs a number.
        reply.usage.input_tokens = req.system.iter().chain(req.messages.iter().map(|m| &m.1)).map(|t| crate::util::tokens(t)).sum();
    }
    Ok(reply)
}

pub fn once(cfg: &Config, model: &str, system: &str, user: &str) -> Result<String, String> {
    let v: Value = call(cfg, json!({ "model": model, "stream": false, "messages": [
        { "role": "system", "content": system }, { "role": "user", "content": user }
    ] }))?.into_json().map_err(|e| e.to_string())?;
    let t = v["choices"][0]["message"]["content"].as_str().ok_or("unexpected reply from the service")?;
    // Some local models show their reasoning.
    Ok(match (t.find("<think>"), t.find("</think>")) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &t[..a], &t[b + 8..]),
        _ => t.to_string(),
    })
}

pub fn list_models(cfg: &Config) -> Result<Vec<String>, String> {
    let mut r = ureq::get(&url(cfg, "/models")?).timeout(Duration::from_secs(20));
    if let Some(k) = key(cfg) {
        r = r.set("authorization", &format!("Bearer {k}"));
    }
    let v: Value = r.call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    // Gemini lists its models as "models/gemini-…" but takes the bare name.
    let mut ids: Vec<String> = v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(|s| s.trim_start_matches("models/").to_string())).collect();
    // Free models first, for OpenRouter.
    ids.sort_by_key(|id| (!id.ends_with(":free"), id.clone()));
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deepseek_key_can_come_from_the_environment() {
        assert_eq!(env_var("https://api.deepseek.com/v1"), Some("DEEPSEEK_API_KEY"));
        assert_eq!(env_var("https://API.DeepSeek.com/v1"), Some("DEEPSEEK_API_KEY"));
        assert_eq!(env_var("https://openrouter.ai/api/v1"), None);
        assert_eq!(env_var("http://localhost:11434/v1"), None);
    }
}
