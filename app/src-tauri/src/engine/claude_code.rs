//! Claude Code as the engine: the player's Claude plan, no API key. Runs `claude -p` with
//! stream-json and no tools, MCP servers or settings, so it's just the model. Spirit's chat
//! window drives Claude Code the same way.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use super::{Reply, Request};
use crate::chat::Usage;
use crate::config::Config;

pub fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

pub fn find(cfg: &Config) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CK_CLAUDE") {
        return Some(PathBuf::from(p));
    }
    if !cfg.claude_path.trim().is_empty() {
        return Some(PathBuf::from(cfg.claude_path.trim()));
    }
    let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
    let mut dirs_: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = dirs::home_dir() {
        dirs_.push(h.join(".local").join("bin"));
        dirs_.push(h.join(".claude").join("local"));
        dirs_.push(h.join("bin"));
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

pub fn command(path: &Path) -> Command {
    // A .cmd has to go through the shell on Windows.
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd")) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(path);
        c
    } else {
        Command::new(path)
    }
}

fn friendly(text: &str) -> String {
    let t = text.to_lowercase();
    if t.contains("log in") || t.contains("login") || t.contains("authenticat") {
        "Claude Code isn't signed in. Open Settings and press Sign in to Claude Code, then try again.".into()
    } else if t.contains("usage limit") || t.contains("rate limit") {
        "Your Claude plan's usage limit was reached. Try again after it resets.".into()
    } else if t.contains("'node' is not recognized") || t.contains("node: not found") {
        "Claude Code was installed with npm, which needs Node.js. Install Node.js (nodejs.org), or install Claude Code with its own installer instead.".into()
    } else {
        format!("Claude Code reported an error: {}", crate::util::clip(text.trim(), 400))
    }
}

pub fn stream(cfg: &Config, req: &Request, on_delta: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
    let exe = find(cfg).ok_or("Claude Code wasn't found on this computer. Install it from Settings, set its location there, or switch to an API key.")?;
    std::fs::create_dir_all(&req.workdir).map_err(|e| e.to_string())?;
    let sys = req.workdir.join("gm_system_prompt.md");
    std::fs::write(&sys, req.system.join("\n\n")).map_err(|e| e.to_string())?;

    let mut cmd = command(&exe);
    cmd.args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--strict-mcp-config"])
        .args(["--setting-sources", "", "--tools", "", "--model", &cfg.model, "--effort", &cfg.effort])
        .arg("--system-prompt-file")
        .arg(&sys);
    if let Some(s) = &req.session {
        cmd.arg("--resume").arg(s);
    }
    no_window(&mut cmd);
    let mut child = cmd
        .current_dir(&req.workdir)
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start Claude Code: {e}"))?;

    // On a resumed session only the newest message goes in; otherwise the whole replay.
    let prompt = if req.session.is_some() {
        req.messages.last().map(|m| m.1.clone()).unwrap_or_default()
    } else {
        flatten(&req.messages)
    };
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
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
    let mut reply = Reply { model: cfg.model.clone(), ..Default::default() };
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
        match v["type"].as_str().unwrap_or("") {
            "system" => {
                if let Some(s) = v["session_id"].as_str() {
                    reply.session = Some(s.to_string());
                }
            }
            "stream_event" => {
                let ev = &v["event"];
                if ev["type"] == "content_block_delta" && ev["delta"]["type"] == "text_delta" {
                    if let Some(t) = ev["delta"]["text"].as_str() {
                        reply.text.push_str(t);
                        on_delta(t);
                    }
                }
                if ev["type"] == "message_delta" && ev["delta"]["stop_reason"] == "max_tokens" {
                    reply.truncated = true;
                }
            }
            "assistant" => {
                if let Some(m) = v["message"]["model"].as_str() {
                    reply.model = m.to_string();
                }
            }
            "result" => {
                if let Ok(u) = serde_json::from_value::<Usage>(v["usage"].clone()) {
                    reply.usage = u;
                }
                if let Some(s) = v["session_id"].as_str() {
                    reply.session = Some(s.to_string());
                }
                if v["is_error"].as_bool().unwrap_or(false) {
                    result_error = Some(v["result"].as_str().unwrap_or("Claude Code stopped without answering").to_string());
                } else if reply.text.is_empty() {
                    if let Some(t) = v["result"].as_str() {
                        reply.text = t.to_string();
                        on_delta(t);
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
    if !status.success() || reply.text.is_empty() {
        let e = err_text.lock().map(|e| e.clone()).unwrap_or_default();
        if !reply.text.is_empty() {
            return Ok(reply);
        }
        return Err(if e.trim().is_empty() { "Claude Code stopped without answering".to_string() } else { friendly(&e) });
    }
    Ok(reply)
}

/// A fresh session can't be given a history, so the past turns go in as one first message.
fn flatten(messages: &[(String, String)]) -> String {
    if messages.len() <= 1 {
        return messages.first().map(|m| m.1.clone()).unwrap_or_default();
    }
    let (last, past) = messages.split_last().expect("not empty");
    let mut t = String::from("<earlier_turns>\n");
    for (r, m) in past {
        t.push_str(if r == "user" { "PLAYER:\n" } else { "GM:\n" });
        t.push_str(m);
        t.push_str("\n\n");
    }
    t.push_str("</earlier_turns>\nThose turns have already happened. Continue from here.\n\n");
    t.push_str(&last.1);
    t
}

pub fn once(cfg: &Config, model: &str, system: &str, user: &str) -> Result<String, String> {
    let exe = find(cfg).ok_or("Claude Code wasn't found")?;
    let dir = crate::config::local_dir().join("helper");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let sys = dir.join(format!("system-{}.md", std::process::id()));
    std::fs::write(&sys, system).map_err(|e| e.to_string())?;
    let mut cmd = command(&exe);
    cmd.args(["-p", "--strict-mcp-config", "--setting-sources", "", "--tools", "", "--no-session-persistence", "--model", model])
        .arg("--system-prompt-file")
        .arg(&sys);
    no_window(&mut cmd);
    let mut child = cmd
        .current_dir(&dir)
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {model}: {e}"))?;
    child.stdin.take().ok_or("no stdin")?.write_all(user.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("{model} failed: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Install Claude Code with Anthropic's own installer.
pub fn install() -> Result<String, String> {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm https://claude.ai/install.ps1 | iex"]);
        c
    } else {
        let mut c = Command::new("bash");
        c.args(["-c", "curl -fsSL https://claude.ai/install.sh | bash"]);
        c
    };
    no_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok("Claude Code is installed. Now sign in once.".into())
    } else {
        Err(format!("Claude Code didn't finish installing: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")))
    }
}

/// Open a visible terminal running Claude Code, so the player can sign in.
pub fn sign_in(cfg: &Config) -> Result<(), String> {
    let exe = find(cfg).ok_or("Claude Code wasn't found")?;
    let res = if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", "Claude Code", "cmd", "/K"]).arg(&exe).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("osascript").args(["-e", &format!("tell app \"Terminal\" to do script \"{}\"", exe.display())]).spawn()
    } else {
        Command::new("x-terminal-emulator").arg("-e").arg(&exe).spawn()
    };
    res.map(|_| ()).map_err(|e| e.to_string())
}
