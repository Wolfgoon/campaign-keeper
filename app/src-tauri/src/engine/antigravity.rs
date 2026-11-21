//! Antigravity as the engine: the player's Google account through the Antigravity CLI (`agy`),
//! no API key. Runs `agy` in headless `stream-json` mode, sending the prompt over stdin as a
//! `stream-json` user event and reading live `step_update` deltas and the final `result` from
//! stdout. Resumes conversations across turns with `--conversation`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::claude_code::{command, no_window};
use super::{Reply, Request};
use crate::config::Config;

const NO_TOOLS: &str = "IMPORTANT: You are acting solely as the Game Master text engine inside Campaign Keeper. \
Follow the <system> instructions and reply with your response text ONLY. \
Do NOT call any tools, run commands, read or write files, or search the web.";

/// Antigravity's CLI truncates a single user message at 192,000 bytes. Stay comfortably below that
/// so the end of the prompt (the newest turns and the player's latest action) is never cut off.
const MAX_PROMPT_BYTES: usize = 175_000;

pub fn find(cfg: &Config) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CK_AGY") {
        return Some(PathBuf::from(p));
    }
    if !cfg.agy_path.trim().is_empty() {
        return Some(PathBuf::from(cfg.agy_path.trim()));
    }
    let names: &[&str] = if cfg!(windows) { &["agy.exe", "agy.cmd"] } else { &["agy"] };
    let mut dirs_: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = dirs::home_dir() {
        dirs_.push(h.join(".local").join("bin"));
        dirs_.push(h.join("bin"));
    }
    if let Ok(a) = std::env::var("LOCALAPPDATA") {
        dirs_.push(Path::new(&a).join("agy").join("bin"));
    }
    dirs_.push(PathBuf::from("/opt/homebrew/bin"));
    dirs_.push(PathBuf::from("/usr/local/bin"));
    if let Ok(a) = std::env::var("APPDATA") {
        dirs_.push(Path::new(&a).join("npm"));
    }
    for d in dirs_ {
        for n in names {
            let p = d.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn friendly(text: &str) -> String {
    let t = text.to_lowercase();
    if t.contains("log in") || t.contains("login") || t.contains("sign in") || t.contains("authenticat") || t.contains("unauthorized") {
        "Antigravity isn't signed in. Open Settings and press Sign in to Antigravity, then try again.".into()
    } else if t.contains("quota") || t.contains("rate limit") || t.contains("usage limit") || t.contains("resource_exhausted") {
        format!("Your Antigravity usage limit for this model was reached ({}). Pick another model in Settings or try again after it resets.", crate::util::clip(text.trim(), 200))
    } else {
        format!("Antigravity reported an error: {}", crate::util::clip(text.trim(), 400))
    }
}

/// A quiet folder to run in, so no campaign or project rules get picked up.
fn workdir(sub: &str) -> Result<PathBuf, String> {
    let d = crate::config::local_dir().join("antigravity").join(sub);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

/// `--print=` enables headless mode with an empty command-line prompt so `--input-format stream-json`
/// can read the whole prompt from stdin without length limits or flag collisions.
fn base_command(exe: &Path, model: &str) -> Command {
    let mut cmd = command(exe);
    cmd.args([
        "--print=",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--disable-slash-commands",
    ]);
    if !model.trim().is_empty() && model.trim() != "auto" {
        cmd.args(["--model", model.trim()]);
    }
    no_window(&mut cmd);
    cmd
}

/// One NDJSON user message line for `--input-format stream-json`.
pub fn input_line(text: &str) -> String {
    let mut line = serde_json::to_string(&json!({
        "event": "user",
        "message": { "role": "user", "content": text }
    }))
    .unwrap_or_default();
    line.push('\n');
    line
}

fn compose(system: &str, conversation: &str) -> String {
    format!("{NO_TOOLS}\n\n<system>\n{system}\n</system>\n\n{conversation}")
}

pub fn stream(cfg: &Config, req: &Request, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
    let exe = find(cfg).ok_or("Antigravity wasn't found on this computer. Install it from Settings, set its location there, or switch engines.")?;
    let dir = workdir("gm")?;

    let resume_id = req.session.as_deref().and_then(|s| s.strip_prefix("agy2:"));
    let mut cmd = base_command(&exe, &cfg.agy_model);
    if let Some(id) = resume_id {
        cmd.args(["--conversation", id]);
    }
    let prompt = if resume_id.is_some() {
        let latest = req.messages.last().map(|m| m.1.as_str()).unwrap_or_default();
        format!("{NO_TOOLS}\n\n{latest}")
    } else {
        compose_first_turn(&req.system.join("\n\n"), &req.messages)
    };
    let payload = input_line(&prompt);

    let mut child = cmd
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start Antigravity: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    std::thread::spawn(move || {
        let _ = stdin.write_all(payload.as_bytes());
    });
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let err_text = Arc::new(Mutex::new(String::new()));
    let et = err_text.clone();
    std::thread::spawn(move || {
        for l in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut e) = et.lock() {
                e.push_str(&l);
                e.push('\n');
            }
        }
    });
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let child = Mutex::new(child);
    let label = if cfg.agy_model.trim().is_empty() { "antigravity".to_string() } else { cfg.agy_model.trim().to_string() };
    let mut reply = Reply { model: label, ..Default::default() };
    let mut result_error: Option<String> = None;
    let cancelled = AtomicBool::new(false);
    let finished = AtomicBool::new(false);
    std::thread::scope(|scope| {
        // Stop button: the reader blocks on stdout, so a watcher kills the process.
        scope.spawn(|| {
            while !finished.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    cancelled.store(true, Ordering::Relaxed);
                    if let Ok(mut c) = child.lock() {
                        let _ = c.kill();
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            match v["event"].as_str().unwrap_or("") {
                "init" => {
                    if let Some(id) = v["conversation_id"].as_str() {
                        reply.session = Some(format!("agy2:{id}"));
                    }
                }
                "step_update" => {
                    let su = &v["step_update"];
                    if su["step_type"] == "agent_response" {
                        if let Some(t) = su["text_delta"].as_str() {
                            if !t.is_empty() {
                                reply.text.push_str(t);
                                on_delta(t);
                            }
                        }
                    }
                }
                "result" => {
                    let r = &v["result"];
                    if let Some(id) = r["conversation_id"].as_str() {
                        reply.session = Some(format!("agy2:{id}"));
                    }
                    let u = &r["usage"];
                    if u.is_object() {
                        reply.usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
                        reply.usage.output_tokens = u["output_tokens"].as_u64().unwrap_or(0);
                        reply.usage.cache_read_input_tokens = u["cache_read_tokens"].as_u64().unwrap_or(0);
                    }
                    if r["status"] == "ERROR" {
                        result_error = Some(r["error"].as_str().unwrap_or("Antigravity stopped without answering").to_string());
                    } else if reply.text.is_empty() {
                        if let Some(t) = r["response"].as_str() {
                            if !t.is_empty() {
                                reply.text = t.to_string();
                                on_delta(t);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        finished.store(true, Ordering::Relaxed);
    });
    let status = child.lock().map_err(|_| "lost the process")?.wait().map_err(|e| e.to_string())?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("stopped".into());
    }
    if let Some(e) = result_error {
        return Err(friendly(&e));
    }
    if reply.text.trim().is_empty() || !status.success() {
        if !reply.text.trim().is_empty() {
            return Ok(reply);
        }
        let e = err_text.lock().map(|e| e.clone()).unwrap_or_default();
        return Err(if e.trim().is_empty() { "Antigravity stopped without answering".to_string() } else { friendly(&e) });
    }
    Ok(reply)
}

/// Build the first turn of an Antigravity session while staying under `MAX_PROMPT_BYTES` so `agy`
/// never truncates the end of the message. Keeps `<system>` and the newest player message intact,
/// and fills the remaining space with the most recent past turns in order.
fn compose_first_turn(system: &str, messages: &[(String, String)]) -> String {
    let last = messages.last().map(|m| m.1.as_str()).unwrap_or_default();
    let past = if messages.len() > 1 { &messages[..messages.len() - 1] } else { &[] };
    let fixed = NO_TOOLS.len() + system.len() + last.len() + 300;
    let sys_cow: String;
    let sys = if fixed > MAX_PROMPT_BYTES {
        let keep = MAX_PROMPT_BYTES.saturating_sub(NO_TOOLS.len() + last.len() + 400);
        let mut end = keep.min(system.len());
        while end > 0 && !system.is_char_boundary(end) {
            end -= 1;
        }
        sys_cow = format!("{}\n\n( middle of system prompt omitted for length )", &system[..end]);
        sys_cow.as_str()
    } else {
        system
    };
    if past.is_empty() {
        return compose(sys, last);
    }
    let budget = MAX_PROMPT_BYTES.saturating_sub(NO_TOOLS.len() + sys.len() + last.len() + 300);
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0usize;
    let mut skipped = false;
    for (r, m) in past.iter().rev() {
        let block = format!("{}\n{}\n\n", if r == "user" { "PLAYER:" } else { "GM:" }, m);
        if used + block.len() > budget {
            skipped = true;
            break;
        }
        used += block.len();
        kept.push(block);
    }
    kept.reverse();
    let mut t = String::from("<earlier_turns>\n");
    if skipped {
        t.push_str("(Earlier turns omitted to fit the prompt limit; the campaign files and recent turns are here.)\n\n");
    }
    for b in kept {
        t.push_str(&b);
    }
    t.push_str("</earlier_turns>\nThose turns have already happened. Continue from here.\n\n");
    t.push_str(last);
    compose(sys, &t)
}

pub fn once(cfg: &Config, model: &str, system: &str, user: &str) -> Result<String, String> {
    let exe = find(cfg).ok_or("Antigravity wasn't found")?;
    let dir = workdir("helper")?;
    let payload = input_line(&compose(system, user));
    let mut child = base_command(&exe, model)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start Antigravity: {e}"))?;
    child.stdin.take().ok_or("no stdin")?.write_all(payload.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let mut text = String::new();
    let mut err: Option<String> = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        match v["event"].as_str().unwrap_or("") {
            "step_update" if v["step_update"]["step_type"] == "agent_response" => {
                if let Some(t) = v["step_update"]["text_delta"].as_str() {
                    text.push_str(t);
                }
            }
            "result" => {
                let r = &v["result"];
                if r["status"] == "ERROR" {
                    err = Some(r["error"].as_str().unwrap_or("Antigravity failed").to_string());
                } else if text.is_empty() {
                    if let Some(t) = r["response"].as_str() {
                        text = t.to_string();
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(e) = err {
        return Err(friendly(&e));
    }
    if !out.status.success() && text.trim().is_empty() {
        return Err(format!("Antigravity failed: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")));
    }
    Ok(text.trim().to_string())
}

/// Query `agy models` for the models available to the signed-in account.
pub fn list_models(cfg: &Config) -> Result<Vec<(String, String)>, String> {
    let exe = find(cfg).ok_or("Antigravity wasn't found")?;
    let mut cmd = command(&exe);
    cmd.arg("models");
    no_window(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let mut list = Vec::new();
    for line in text.lines() {
        if let Some((id, name)) = line.split_once('\t') {
            let id = id.trim();
            let name = name.trim();
            if !id.is_empty() && !name.is_empty() && !id.contains(' ') {
                list.push((id.to_string(), name.to_string()));
            }
        }
    }
    Ok(list)
}

/// Install the Antigravity CLI with Google's own installer.
pub fn install() -> Result<String, String> {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm https://antigravity.google/cli/install.ps1 | iex"]);
        c
    } else {
        let mut c = Command::new("bash");
        c.args(["-c", "curl -fsSL https://antigravity.google/cli/install.sh | bash"]);
        c
    };
    no_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok("Antigravity is installed. Now sign in once.".into())
    } else {
        Err(format!("Antigravity didn't finish installing: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")))
    }
}

/// Open a visible terminal running `agy`, so the player can sign in.
pub fn sign_in(cfg: &Config) -> Result<(), String> {
    let exe = find(cfg).ok_or("Antigravity wasn't found")?;
    let res = if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", "Antigravity", "cmd", "/K"]).arg(&exe).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("osascript").args(["-e", &format!("tell app \"Terminal\" to do script \"{}\"", exe.display())]).spawn()
    } else {
        Command::new("x-terminal-emulator").arg("-e").arg(&exe).spawn()
    };
    res.map(|_| ()).map_err(|e| e.to_string())
}
