//! Painted pictures from an image model, on this PC or through a service. Optional: with no service set up the
//! GM's own SVGs are used, and if a painting fails the turn goes on without it.
//!
//! The seed comes from the picture's title and prompt, so nothing here is left to chance: the same
//! request gets the same painting.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::config::Config;
use crate::util::hash;

const FAL_URL: &str = "https://fal.run/fal-ai/flux/schnell";
const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;
const MAX_IMAGE: u64 = 32 * 1024 * 1024;
/// SD-Turbo through stable-diffusion.cpp, kept in the machine-local data folder.
const LOCAL_MODEL: &str = "sd_turbo-f16-q8_0.gguf";
const LOCAL_SIZE: u32 = 512;
const LOCAL_STEPS: u32 = 4;
const LOCAL_TIMEOUT: Duration = Duration::from_secs(300);

pub fn enabled(cfg: &Config) -> bool {
    matches!(cfg.image_provider.as_str(), "fal" | "openai" | "local")
}

pub fn seed(title: &str, prompt: &str) -> u32 {
    (hash(&format!("{title}\n{prompt}")) & 0x7fff_ffff) as u32
}

/// Aim the prompt at what will survive being turned into pixels.
fn styled(prompt: &str, pixel: bool) -> String {
    if pixel {
        format!("{prompt}. Bold simple shapes, strong silhouette, clear lighting, flat areas of colour, video game key art.")
    } else {
        prompt.to_string()
    }
}

pub fn paint(cfg: &Config, title: &str, prompt: &str, pixel: bool) -> Result<Vec<u8>, String> {
    let text = styled(prompt.trim(), pixel);
    let seed = seed(title, prompt);
    if cfg.image_provider == "local" {
        return paint_local(&text, seed);
    }
    let (url, auth, body) = match cfg.image_provider.as_str() {
        "fal" => (
            if cfg.image_url.trim().is_empty() { FAL_URL.to_string() } else { cfg.image_url.trim().to_string() },
            format!("Key {}", cfg.image_key.trim()),
            json!({ "prompt": text, "image_size": { "width": WIDTH, "height": HEIGHT }, "num_inference_steps": 4, "seed": seed, "num_images": 1, "output_format": "png", "enable_safety_checker": true }),
        ),
        "openai" => (
            format!("{}/images/generations", cfg.image_url.trim().trim_end_matches('/')),
            format!("Bearer {}", cfg.image_key.trim()),
            json!({ "model": cfg.image_model.trim(), "prompt": text, "width": WIDTH, "height": HEIGHT, "size": format!("{WIDTH}x{HEIGHT}"), "steps": 4, "seed": seed, "n": 1, "response_format": "b64_json" }),
        ),
        _ => return Err("no image service is set up".into()),
    };
    let resp = ureq::post(&url).timeout(Duration::from_secs(300)).set("authorization", &auth).set("content-type", "application/json").send_json(body).map_err(|e| match e {
        ureq::Error::Status(c, r) => refused(c, &r.into_string().unwrap_or_default()),
        e => format!("couldn't reach the image service: {e}"),
    })?;
    let v: Value = resp.into_json().map_err(|e| e.to_string())?;
    let item = v["images"][0].as_object().or(v["data"][0].as_object()).ok_or("the image service sent no picture")?;
    if let Some(b) = item.get("b64_json").and_then(Value::as_str) {
        return decode_b64(b);
    }
    let link = item.get("url").and_then(Value::as_str).ok_or("the image service sent no picture")?;
    if let Some((_, b)) = link.strip_prefix("data:").and_then(|d| d.split_once(";base64,")) {
        return decode_b64(b);
    }
    if !(link.starts_with("https://") || link.starts_with("http://")) {
        return Err("the image service sent an odd link".into());
    }
    let mut out = Vec::new();
    ureq::get(link).timeout(Duration::from_secs(120)).call().map_err(|e| e.to_string())?.into_reader().take(MAX_IMAGE).read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn painter_args(model: &Path, prompt: &str, seed: u32, out: &Path, backend: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = ["--model", &model.to_string_lossy(), "--prompt", prompt, "--output", &out.to_string_lossy(), "--seed", &seed.to_string()].map(String::from).to_vec();
    args.extend(["--steps".to_string(), LOCAL_STEPS.to_string(), "--cfg-scale".to_string(), "1".to_string()]);
    args.extend(["--width".to_string(), LOCAL_SIZE.to_string(), "--height".to_string(), LOCAL_SIZE.to_string()]);
    if let Some(b) = backend {
        args.extend(["--backend".to_string(), b.to_string()]);
    }
    args
}

fn run_painter(exe: &Path, args: &[String], log: &Path) -> Result<(), String> {
    let out = std::fs::File::create(log).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.current_dir(exe.parent().unwrap_or(Path::new("."))).args(args).stdin(Stdio::null()).stdout(out).stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start the painter: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let text = std::fs::read_to_string(log).unwrap_or_default();
                let tail: Vec<&str> = text.lines().rev().filter(|l| !l.trim().is_empty()).take(2).collect();
                return Err(format!("the painter stopped ({status}): {}", crate::util::clip(&tail.join(" / "), 200)));
            }
            None if started.elapsed() > LOCAL_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the painter took too long".into());
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

fn painter_dir() -> PathBuf {
    crate::config::local_dir().join("painter")
}

/// The graphics card is often full of other models, so a failed run is tried again on the CPU.
fn paint_local(prompt: &str, seed: u32) -> Result<Vec<u8>, String> {
    let dir = painter_dir();
    let (exe, model) = (dir.join("bin").join("sd-cli.exe"), dir.join(LOCAL_MODEL));
    if !exe.exists() || !model.exists() {
        return Err(format!("the painter isn't installed on this PC (looked in {})", dir.display()));
    }
    let out = dir.join(format!("paint-{}-{seed}.png", std::process::id()));
    let log = dir.join("last-run.log");
    let run = run_painter(&exe, &painter_args(&model, prompt, seed, &out, None), &log)
        .or_else(|_| run_painter(&exe, &painter_args(&model, prompt, seed, &out, Some("cpu")), &log));
    let bytes = run.and_then(|_| std::fs::read(&out).map_err(|_| "the painter finished without a picture".to_string()));
    let _ = std::fs::remove_file(&out);
    bytes
}

/// Say what the service actually said. A 403 can mean an empty account rather than a bad key.
fn refused(code: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body).ok().and_then(|v| v["detail"].as_str().or(v["error"]["message"].as_str()).or(v["error"].as_str()).map(str::to_string)).unwrap_or_else(|| body.trim().to_string());
    let low = detail.to_lowercase();
    if code == 402 || low.contains("top_up") || low.contains("balance") || low.contains("credit") {
        return format!("the image service account needs credit ({detail})");
    }
    let detail = crate::util::clip(&detail, 200);
    match code {
        401 => format!("the image service didn't accept the key ({detail})"),
        403 => format!("the image service refused the request ({detail})"),
        c => format!("the image service reported an error ({c}): {detail}"),
    }
}

fn decode_b64(b: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(b.trim()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_request_same_seed() {
        assert_eq!(seed("Key", "a rusty key"), seed("Key", "a rusty key"));
        assert_ne!(seed("Key", "a rusty key"), seed("Key", "a brass key"));
        assert!(seed("x", "y") <= 0x7fff_ffff);
    }

    #[test]
    fn an_empty_account_is_not_called_a_bad_key() {
        assert!(refused(403, r#"{"detail":"User is locked. Reason: TOP_UP."}"#).contains("needs credit"));
        assert!(refused(403, r#"{"detail":"User is locked. Reason: Exhausted balance."}"#).contains("needs credit"));
        assert!(refused(401, r#"{"detail":"Authentication is required"}"#).contains("didn't accept the key"));
        assert!(refused(500, "plain text").contains("(500): plain text"));
    }

    #[test]
    fn the_local_painter_gets_a_fixed_request() {
        let args = painter_args(Path::new("m.gguf"), "a key", 7, Path::new("o.png"), Some("cpu"));
        let at = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!((at("--prompt"), at("--seed"), at("--output"), at("--backend")), ("a key".into(), "7".into(), "o.png".into(), "cpu".into()));
        assert!(!painter_args(Path::new("m"), "p", 1, Path::new("o"), None).contains(&"--backend".to_string()));
        assert!(enabled(&Config { image_provider: "local".into(), ..Config::default() }));
    }

    #[test]
    fn nothing_is_sent_without_a_service() {
        let c = Config::default();
        assert!(!enabled(&c));
        assert!(paint(&c, "t", "p", true).is_err());
    }
}
