//! OpenAI-compatible servers: OpenRouter, Ollama, LM Studio.

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

fn call(cfg: &Config, body: Value) -> Result<ureq::Response, String> {
    let mut r = ureq::post(&url(cfg, "/chat/completions")?)
        .timeout(Duration::from_secs(1800))
        .set("content-type", "application/json")
        .set("http-referer", "https://github.com/campaign-keeper")
        .set("x-title", "Campaign Keeper");
    if !cfg.other_key.trim().is_empty() {
        r = r.set("authorization", &format!("Bearer {}", cfg.other_key.trim()));
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
    let resp = call(cfg, json!({ "model": cfg.other_model, "stream": true, "max_tokens": req.max_tokens, "messages": messages, "stream_options": { "include_usage": true } }))?;
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
    if !cfg.other_key.trim().is_empty() {
        r = r.set("authorization", &format!("Bearer {}", cfg.other_key.trim()));
    }
    let v: Value = r.call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(str::to_string)).collect();
    ids.sort();
    Ok(ids)
}
