//! OpenAI-compatible servers: DeepSeek, OpenRouter, Ollama, LM Studio.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use super::{Reply, Request};
use crate::config::Config;

const DEEPSEEK_URL: &str = "https://api.deepseek.com";

pub fn deepseek_base_url() -> String {
    std::env::var("CK_DEEPSEEK_URL").unwrap_or_else(|_| DEEPSEEK_URL.to_string())
}

pub fn normalize_deepseek_model(m: &str) -> String {
    let t = m.trim().to_lowercase();
    if t.is_empty() || t == "deepseek-flash" || t == "deepseek-v-flash" || t == "deepseek-v4-flash" || t == "deepseek v flash" || !t.starts_with("deepseek") {
        "deepseek-flash".into()
    } else {
        m.trim().to_string()
    }
}

pub fn deepseek_effort(effort: &str) -> &'static str {
    match effort.trim() {
        "none" => "none",
        "low" => "low",
        "max" => "max",
        _ => "high",
    }
}

fn url(cfg: &Config, path: &str) -> Result<String, String> {
    let b = cfg.other_url.trim().trim_end_matches('/');
    if b.is_empty() {
        return Err("Enter the service's address in Settings (Other models).".into());
    }
    Ok(format!("{b}{path}"))
}

fn send_post(endpoint: &str, api_key: &str, body: Value) -> Result<ureq::Response, String> {
    let mut r = ureq::post(endpoint)
        .timeout(Duration::from_secs(1800))
        .set("content-type", "application/json")
        .set("http-referer", "https://github.com/campaign-keeper")
        .set("x-title", "Campaign Keeper");
    if !api_key.trim().is_empty() {
        r = r.set("authorization", &format!("Bearer {}", api_key.trim()));
    }
    match r.send_json(body) {
        Ok(x) => Ok(x),
        Err(ureq::Error::Status(401, _)) => Err("The API key was rejected. Check it in Settings.".into()),
        Err(ureq::Error::Status(c, resp)) => Err(format!("The service reported an error ({c}): {}", crate::util::clip(&resp.into_string().unwrap_or_default(), 300))),
        Err(e) => Err(format!("Couldn't reach {endpoint}: {e}")),
    }
}

fn call(cfg: &Config, body: Value) -> Result<ureq::Response, String> {
    let key = if !cfg.other_key.trim().is_empty() {
        cfg.other_key.trim().to_string()
    } else if cfg.other_url.to_lowercase().contains("deepseek") {
        cfg.resolved_deepseek_key()
    } else {
        String::new()
    };
    send_post(&url(cfg, "/chat/completions")?, &key, body)
}

fn call_deepseek(cfg: &Config, body: Value) -> Result<ureq::Response, String> {
    let key = cfg.resolved_deepseek_key();
    if key.is_empty() {
        return Err("Add your DeepSeek API key in Settings.".into());
    }
    let base = deepseek_base_url();
    let endpoint = format!("{}/chat/completions", base.trim_end_matches('/'));
    send_post(&endpoint, &key, body)
}

fn read_sse_stream(resp: ureq::Response, model: String, req: &Request, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
    let mut reply = Reply { model, ..Default::default() };
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
            if !t.is_empty() {
                reply.text.push_str(t);
                on_delta(t);
            }
        }
        if v["choices"][0]["finish_reason"] == "length" {
            reply.truncated = true;
        }
        if let Some(n) = v["usage"]["prompt_tokens"].as_u64() {
            let cached = v["usage"]["prompt_cache_hit_tokens"]
                .as_u64()
                .or_else(|| v["usage"]["prompt_tokens_details"]["cached_tokens"].as_u64())
                .unwrap_or(0);
            reply.usage.cache_read_input_tokens = cached;
            reply.usage.input_tokens = v["usage"]["prompt_cache_miss_tokens"].as_u64().unwrap_or_else(|| n.saturating_sub(cached));
            reply.usage.output_tokens = v["usage"]["completion_tokens"].as_u64().unwrap_or(0);
        }
    }
    if reply.usage.output_tokens == 0 {
        reply.usage.output_tokens = crate::util::tokens(&reply.text);
    }
    if reply.usage.input_tokens == 0 && reply.usage.cache_read_input_tokens == 0 {
        // Local servers often report nothing; the budget still needs a number.
        reply.usage.input_tokens = req.system.iter().chain(req.messages.iter().map(|m| &m.1)).map(|t| crate::util::tokens(t)).sum();
    }
    Ok(reply)
}

pub fn stream(cfg: &Config, req: &Request, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
    if cfg.other_model.trim().is_empty() {
        return Err("Choose a model in Settings (Other models).".into());
    }
    let model = if cfg.other_url.to_lowercase().contains("deepseek") {
        normalize_deepseek_model(&cfg.other_model)
    } else {
        cfg.other_model.clone()
    };
    let mut messages = vec![json!({ "role": "system", "content": req.system.join("\n\n") })];
    messages.extend(req.messages.iter().map(|(r, t)| json!({ "role": r, "content": t })));
    let resp = call(cfg, json!({ "model": model, "stream": true, "max_tokens": req.max_tokens, "messages": messages, "stream_options": { "include_usage": true } }))?;
    read_sse_stream(resp, model, req, on_delta, cancel)
}

pub fn stream_deepseek(cfg: &Config, req: &Request, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
    let model = normalize_deepseek_model(&cfg.model);
    let effort = deepseek_effort(&cfg.effort);
    let max_tokens = if effort == "none" { req.max_tokens.min(8192) } else { req.max_tokens.min(64000) };
    let mut messages = vec![json!({ "role": "system", "content": req.system.join("\n\n") })];
    messages.extend(req.messages.iter().map(|(r, t)| json!({ "role": r, "content": t })));
    let resp = call_deepseek(cfg, json!({
        "model": model,
        "stream": true,
        "max_tokens": max_tokens,
        "reasoning_effort": effort,
        "messages": messages,
        "stream_options": { "include_usage": true }
    }))?;
    read_sse_stream(resp, model, req, on_delta, cancel)
}

fn strip_think(t: &str) -> String {
    match (t.find("<think>"), t.find("</think>")) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &t[..a], &t[b + 8..]),
        _ => t.to_string(),
    }
}

pub fn once(cfg: &Config, model: &str, system: &str, user: &str) -> Result<String, String> {
    let v: Value = call(cfg, json!({ "model": model, "stream": false, "messages": [
        { "role": "system", "content": system }, { "role": "user", "content": user }
    ] }))?.into_json().map_err(|e| e.to_string())?;
    let t = v["choices"][0]["message"]["content"].as_str().ok_or("unexpected reply from the service")?;
    Ok(strip_think(t))
}

pub fn once_deepseek(cfg: &Config, model: &str, system: &str, user: &str) -> Result<String, String> {
    let model = normalize_deepseek_model(model);
    let v: Value = call_deepseek(cfg, json!({
        "model": model,
        "stream": false,
        "max_tokens": 8000,
        "reasoning_effort": "low",
        "messages": [
            { "role": "system", "content": system }, { "role": "user", "content": user }
        ]
    }))?.into_json().map_err(|e| e.to_string())?;
    let t = v["choices"][0]["message"]["content"].as_str().ok_or("unexpected reply from DeepSeek")?;
    Ok(strip_think(t))
}

pub fn list_models(cfg: &Config) -> Result<Vec<String>, String> {
    let mut r = ureq::get(&url(cfg, "/models")?).timeout(Duration::from_secs(20));
    let key = if !cfg.other_key.trim().is_empty() {
        cfg.other_key.trim().to_string()
    } else if cfg.other_url.to_lowercase().contains("deepseek") {
        cfg.resolved_deepseek_key()
    } else {
        String::new()
    };
    if !key.is_empty() {
        r = r.set("authorization", &format!("Bearer {key}"));
    }
    let v: Value = r.call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(str::to_string)).collect();
    ids.sort();
    Ok(ids)
}
