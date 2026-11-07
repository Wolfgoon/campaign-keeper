//! The Scribe: after a session, Claude reads the campaign and the play session and proposes edits.
//! Nothing is written until the player approves each file; old versions go to `.keeper/backups`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};

use crate::campaign::{self, keeper, INSTRUCTIONS};
use crate::chat::Entry;
use crate::config::Config;
use crate::engine::claude_code;
use crate::memory::transcript;

const SYSTEM: &str = include_str!("../prompts/scribe.md");

pub const PRESET_AFTER: &str = "Update the campaign files from this play session, following the README's \"After each session\" checklist: add a session log, update the character sheet (condition, inventory, money, relationships), the NPC files and minor_npcs.md, the quests (move finished ones to completed_quests.md; add new leads and mysteries), the map and locations, key documents (word for word), and the pinned-memory continuity anchors.";
pub const PRESET_LOG: &str = "Write the session log for this play session in 06_Sessions, following the folder's _TEMPLATE.md. Don't change other files.";
pub const PRESET_CHECK: &str = "Read all the campaign files and fix contradictions between them (money, injuries, dates, who knows what, item locations). Use the most recent session and save as the truth. List anything you couldn't resolve.";

#[derive(Serialize, Clone, Debug)]
pub struct Line {
    pub tag: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Proposal {
    pub path: String,
    pub is_new: bool,
    pub old: String,
    pub new: String,
    pub lines: Vec<Line>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Outcome {
    pub summary: String,
    pub proposals: Vec<Proposal>,
}

fn allowed(path: &str) -> Result<(), String> {
    if !(path.ends_with(".md") || path.ends_with(".txt")) {
        return Err("Only .md and .txt files can be used.".into());
    }
    if path == INSTRUCTIONS {
        return Err("The GM instructions file is off limits.".into());
    }
    if path.starts_with("08_Saves/") {
        return Err("The app manages the saves in 08_Saves; don't edit them.".into());
    }
    if path.starts_with('.') || path == "README.md" || path.rsplit('/').next().unwrap_or("").starts_with('_') {
        return Err("That file can't be edited.".into());
    }
    Ok(())
}

pub fn diff(old: &str, new: &str) -> Vec<Line> {
    let d = TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    for group in d.grouped_ops(2) {
        for op in group {
            for c in d.iter_changes(&op) {
                let tag = match c.tag() {
                    ChangeTag::Delete => "del",
                    ChangeTag::Insert => "add",
                    ChangeTag::Equal => "same",
                };
                out.push(Line { tag: tag.into(), text: c.to_string_lossy().trim_end_matches('\n').to_string() });
            }
        }
        out.push(Line { tag: "gap".into(), text: String::new() });
    }
    out.pop();
    out
}

fn proposals(dir: &Path, edits: &BTreeMap<String, String>) -> Vec<Proposal> {
    edits
        .iter()
        .filter_map(|(path, new)| {
            let old = std::fs::read_to_string(dir.join(path)).ok();
            if old.as_deref() == Some(new.as_str()) {
                return None;
            }
            let is_new = old.is_none();
            let old = old.unwrap_or_default();
            Some(Proposal { path: path.clone(), is_new, lines: diff(&old, new), old, new: new.clone() })
        })
        .collect()
}

/// Run one edit tool against the proposals so far. Same rules whichever engine asked.
fn tool(dir: &Path, edits: &mut BTreeMap<String, String>, name: &str, a: &Value) -> Result<String, String> {
    let path = a["path"].as_str().ok_or("missing path")?.trim().replace('\\', "/");
    campaign::resolve(dir, &path)?;
    let current = |e: &BTreeMap<String, String>| e.get(&path).cloned().or_else(|| std::fs::read_to_string(dir.join(&path)).ok());
    if name == "read_file" {
        return current(edits).ok_or_else(|| format!("{path} doesn't exist"));
    }
    allowed(&path)?;
    match name {
        "create_file" => {
            if current(edits).is_some() {
                return Err(format!("{path} already exists"));
            }
            edits.insert(path.clone(), a["content"].as_str().ok_or("missing content")?.to_string());
        }
        "rewrite_file" => {
            current(edits).ok_or_else(|| format!("{path} doesn't exist"))?;
            edits.insert(path.clone(), a["content"].as_str().ok_or("missing content")?.to_string());
        }
        "append_to_file" => {
            let mut t = current(edits).ok_or_else(|| format!("{path} doesn't exist"))?;
            if !t.ends_with('\n') {
                t.push('\n');
            }
            t.push_str(a["content"].as_str().ok_or("missing content")?);
            edits.insert(path.clone(), t);
        }
        "replace_in_file" => {
            let old = a["old_text"].as_str().ok_or("missing old_text")?;
            if old.is_empty() {
                return Err("old_text is empty.".into());
            }
            let t = current(edits).ok_or_else(|| format!("{path} doesn't exist"))?;
            match t.matches(old).count() {
                0 => return Err("old_text wasn't found in the file. read_file shows its current text.".into()),
                1 => {}
                n => return Err(format!("old_text appears {n} times; include more around it so it's unique.")),
            }
            edits.insert(path.clone(), t.replacen(old, a["new_text"].as_str().ok_or("missing new_text")?, 1));
        }
        _ => return Err(format!("unknown tool {name}")),
    }
    Ok("ok".into())
}

fn tools() -> Value {
    let path = json!({ "type": "string", "description": "Path relative to the campaign folder, e.g. 03_NPCs/tobin.md" });
    let def = |n: &str, d: &str, props: Value, req: &[&str]| {
        json!({ "name": n, "description": d, "input_schema": { "type": "object", "properties": props, "required": req } })
    };
    json!([
        def("read_file", "Read a campaign file as it is now, including edits you already proposed. Use it before editing a file you changed earlier, so old_text matches exactly.", json!({ "path": path }), &["path"]),
        def("create_file", "Create a new campaign file (a new NPC, location, faction, lore entry, or session log). Fails if the file exists. Follow the _TEMPLATE.md form in the same folder.", json!({ "path": path, "content": { "type": "string", "description": "The full text of the new file" } }), &["path", "content"]),
        def("rewrite_file", "Replace the whole text of an existing campaign file. Use only when most of the file changes; otherwise use replace_in_file.", json!({ "path": path, "content": { "type": "string", "description": "The full new text of the file" } }), &["path", "content"]),
        def("append_to_file", "Add text to the end of an existing campaign file, such as a new table row or a new section at the end.", json!({ "path": path, "content": { "type": "string", "description": "Text to add at the end of the file" } }), &["path", "content"]),
        def("replace_in_file", "Replace one passage of a campaign file. old_text must appear exactly once in the file; copy it exactly, including line breaks and Markdown. Prefer this for targeted updates.", json!({ "path": path, "old_text": { "type": "string", "description": "The exact passage to replace" }, "new_text": { "type": "string", "description": "The text to put in its place" } }), &["path", "old_text", "new_text"]),
    ])
}

fn all_files(dir: &Path) -> String {
    let mut t = String::from("Here are the campaign files. Blank forms (_TEMPLATE.md) are included so you can copy their structure.\n");
    for f in campaign::files(dir) {
        if f == INSTRUCTIONS {
            continue;
        }
        if let Ok(x) = std::fs::read_to_string(dir.join(&f)) {
            t.push_str(&format!("<file path=\"{f}\">\n{x}\n</file>\n"));
        }
    }
    t
}

fn user_prompt(entries: &[Entry], request: &str, include_chat: bool) -> String {
    let mut t = String::new();
    if include_chat && !entries.is_empty() {
        t.push_str(&format!("Transcript of the play session (PLAYER is the player, GM is the Game Master):\n<play_transcript>\n{}\n</play_transcript>\n\n", transcript(entries)));
    }
    t.push_str(request);
    t
}

pub fn run(cfg: &Config, dir: &Path, entries: &[Entry], request: &str, include_chat: bool) -> Result<Outcome, String> {
    let prompt = user_prompt(entries, request, include_chat);
    match cfg.engine.as_str() {
        "api" => run_api(cfg, dir, &prompt),
        "claude_code" => run_claude_code(cfg, dir, &prompt),
        _ => Err("The Scribe edits files with Claude's tools, so it needs Claude Code or an Anthropic API key (Settings). The GM can still play on other models.".into()),
    }
}

fn run_api(cfg: &Config, dir: &Path, prompt: &str) -> Result<Outcome, String> {
    let key = cfg.resolved_api_key();
    if key.is_empty() {
        return Err("Add your Anthropic API key in Settings, or switch to Claude Code.".into());
    }
    let url = std::env::var("CK_ANTHROPIC_URL").unwrap_or_else(|_| "https://api.anthropic.com/v1/messages".into());
    let mut messages = vec![json!({ "role": "user", "content": [
        { "type": "text", "text": all_files(dir), "cache_control": { "type": "ephemeral" } },
        { "type": "text", "text": prompt }
    ] })];
    let mut edits = BTreeMap::new();
    let mut summary = String::new();
    for _ in 0..40 {
        let body = json!({
            "model": cfg.model, "max_tokens": 32000, "system": SYSTEM, "tools": tools(), "messages": messages,
            "thinking": { "type": "adaptive" }, "output_config": { "effort": cfg.effort },
        });
        let resp = ureq::post(&url)
            .timeout(Duration::from_secs(900))
            .set("x-api-key", &key)
            .set("anthropic-version", "2026-10-21")
            .send_json(body)
            .map_err(|e| match e {
                ureq::Error::Status(c, r) => format!("The service reported an error ({c}): {}", crate::util::clip(&r.into_string().unwrap_or_default(), 300)),
                e => e.to_string(),
            })?;
        let v: Value = resp.into_json().map_err(|e| e.to_string())?;
        let stop = v["stop_reason"].as_str().unwrap_or("");
        if stop == "refusal" {
            return Err("The model declined this request.".into());
        }
        let content = v["content"].clone();
        summary = content.as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
        if stop == "max_tokens" {
            summary.push_str("\n\nThe Scribe ran out of output space before it finished, so its last edit was skipped. Review what's here, then run it again for anything missing.");
            break;
        }
        if stop != "tool_use" {
            break;
        }
        let mut results = Vec::new();
        for b in content.as_array().into_iter().flatten().filter(|b| b["type"] == "tool_use") {
            let r = tool(dir, &mut edits, b["name"].as_str().unwrap_or(""), &b["input"]);
            let (text, err) = match r {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            results.push(json!({ "type": "tool_result", "tool_use_id": b["id"], "content": text, "is_error": err }));
        }
        // The whole assistant content goes back unchanged, thinking blocks included.
        messages.push(json!({ "role": "assistant", "content": content }));
        messages.push(json!({ "role": "user", "content": results }));
    }
    Ok(Outcome { summary, proposals: proposals(dir, &edits) })
}

/// Claude Code edits a private copy, with its own tools; the differences become the proposals.
fn run_claude_code(cfg: &Config, dir: &Path, prompt: &str) -> Result<Outcome, String> {
    let exe = claude_code::find(cfg).ok_or("Claude Code wasn't found on this computer.")?;
    let work: PathBuf = keeper(dir).join("scribe-work");
    let _ = std::fs::remove_dir_all(&work);
    for f in campaign::files(dir) {
        let to = work.join(&f);
        std::fs::create_dir_all(to.parent().unwrap_or(&work)).map_err(|e| e.to_string())?;
        std::fs::copy(dir.join(&f), &to).map_err(|e| e.to_string())?;
    }
    let extra = "\n\n- The campaign folder is your current directory. Read a file before you edit it.\n- Use Edit for targeted updates, and Write for new files.";
    let sys = keeper(dir).join("scribe_system.md");
    std::fs::write(&sys, format!("{SYSTEM}{extra}")).map_err(|e| e.to_string())?;
    let mut cmd = claude_code::command(&exe);
    cmd.args(["-p", "--strict-mcp-config", "--setting-sources", "", "--permission-mode", "acceptEdits", "--tools", "Read,Edit,Write,Glob,Grep", "--model", &cfg.model, "--effort", &cfg.effort])
        .arg("--system-prompt-file")
        .arg(&sys);
    claude_code::no_window(&mut cmd);
    let mut child = cmd
        .current_dir(&work)
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start Claude Code: {e}"))?;
    let full = format!("The campaign files are in the current directory. README.md explains the layout, and the _TEMPLATE.md files are the blank forms for new entries.\n\n{prompt}");
    child.stdin.take().ok_or("no stdin")?.write_all(full.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("The Scribe stopped unexpectedly.".to_string());
    }
    let mut edits = BTreeMap::new();
    for f in campaign::files(&work) {
        if allowed(&f).is_ok() {
            if let Ok(t) = std::fs::read_to_string(work.join(&f)) {
                edits.insert(f, t);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(Outcome { summary: String::from_utf8_lossy(&out.stdout).trim().to_string(), proposals: proposals(dir, &edits) })
}

/// Write the approved files. Returns the backup folder, for "Restore".
pub fn apply(dir: &Path, approved: &[(String, String)]) -> Result<String, String> {
    let paths: Vec<String> = approved.iter().map(|a| a.0.clone()).collect();
    for p in &paths {
        allowed(p)?;
    }
    let backup = campaign::backup(dir, &paths)?;
    for (p, t) in approved {
        campaign::write(dir, p, t)?;
    }
    let mut manifest = String::new();
    for p in &paths {
        manifest.push_str(p);
        manifest.push('\n');
    }
    crate::util::atomic_write(&backup.join(".applied"), manifest.as_bytes())?;
    Ok(backup.display().to_string())
}

/// Put back what `apply` changed: backups restored, files it created moved to the trash.
pub fn restore(dir: &Path, backup: &str) -> Result<(), String> {
    let root = PathBuf::from(backup);
    if !root.starts_with(keeper(dir)) {
        return Err("That isn't one of this campaign's backups.".into());
    }
    let manifest = std::fs::read_to_string(root.join(".applied")).map_err(|e| e.to_string())?;
    for p in manifest.lines().filter(|l| !l.is_empty()) {
        let b = root.join(p);
        if b.exists() {
            std::fs::copy(&b, campaign::resolve(dir, p)?).map_err(|e| e.to_string())?;
        } else {
            campaign::trash(dir, p)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ck-scribe-{}", uuid::Uuid::new_v4())).join("c");
        campaign::create(&d, "T").unwrap();
        d
    }

    #[test]
    fn tools_propose_without_touching_the_disk() {
        let d = camp();
        let mut e = BTreeMap::new();
        tool(&d, &mut e, "create_file", &json!({"path":"03_NPCs/tobin.md","content":"# Tobin\nold"})).unwrap();
        tool(&d, &mut e, "replace_in_file", &json!({"path":"03_NPCs/tobin.md","old_text":"old","new_text":"new"})).unwrap();
        assert_eq!(tool(&d, &mut e, "read_file", &json!({"path":"03_NPCs/tobin.md"})).unwrap(), "# Tobin\nnew");
        assert!(!d.join("03_NPCs/tobin.md").exists());
        let p = proposals(&d, &e);
        assert!(p[0].is_new);
    }

    #[test]
    fn forbidden_files_stay_forbidden() {
        let d = camp();
        let mut e = BTreeMap::new();
        for path in [INSTRUCTIONS, "08_Saves/x.md", "README.md", "03_NPCs/_TEMPLATE.md", "../evil.md", "/etc/passwd", "a.exe"] {
            assert!(tool(&d, &mut e, "rewrite_file", &json!({"path":path,"content":"x"})).is_err(), "{path}");
        }
    }

    #[test]
    fn replace_needs_exactly_one_match() {
        let d = camp();
        campaign::write(&d, "03_NPCs/a.md", "x x").unwrap();
        let mut e = BTreeMap::new();
        assert!(tool(&d, &mut e, "replace_in_file", &json!({"path":"03_NPCs/a.md","old_text":"x","new_text":"y"})).is_err());
    }

    #[test]
    fn apply_then_restore_puts_everything_back() {
        let d = camp();
        campaign::write(&d, "03_NPCs/a.md", "one").unwrap();
        let b = apply(&d, &[("03_NPCs/a.md".into(), "two".into()), ("03_NPCs/b.md".into(), "new".into())]).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("03_NPCs/a.md")).unwrap(), "two");
        restore(&d, &b).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("03_NPCs/a.md")).unwrap(), "one");
        assert!(!d.join("03_NPCs/b.md").exists());
    }

    #[test]
    fn diffs_show_only_what_changed() {
        let l = diff("a\nb\nc\nd\ne\nf\ng\n", "a\nb\nc\nX\ne\nf\ng\n");
        assert_eq!(l.iter().filter(|x| x.tag == "del").count(), 1);
        assert_eq!(l.iter().filter(|x| x.tag == "add").count(), 1);
        assert!(l.len() < 8);
    }
}
