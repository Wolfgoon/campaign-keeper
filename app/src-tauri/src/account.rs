//! The player's account: their Google account, with stories kept in a "Campaign Keeper" folder in
//! their own Google Drive. The app can only see files it put there itself (the `drive.file` scope).
//!
//! Signing in uses Google's device flow: the app shows a short code, the player types it at
//! google.com/device in any browser. It's the one flow that works the same on a PC and a phone.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::config_dir;
use crate::util::atomic_write;

const SCOPE: &str = "openid email https://www.googleapis.com/auth/drive.file";
const FOLDER: &str = "Campaign Keeper";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const FIELDS: &str = "id,name,modifiedTime,size,appProperties";

type R<T> = Result<T, String>;

struct Keys {
    id: String,
    secret: String,
}

/// The Google OAuth client to sign in with: the one typed into Settings if there is one, otherwise
/// the one this copy was built with (google_oauth.json, filled in by whoever builds the app).
fn keys() -> R<Keys> {
    let cfg = crate::config::Config::load();
    let k = if !cfg.google_client_id.trim().is_empty() {
        Keys { id: cfg.google_client_id.trim().into(), secret: cfg.google_client_secret.trim().into() }
    } else {
        let v: Value = serde_json::from_str(include_str!("../google_oauth.json")).unwrap_or(Value::Null);
        let field = |f: &str| v[f].as_str().unwrap_or("").trim().to_string();
        Keys { id: field("client_id"), secret: field("client_secret") }
    };
    if k.id.is_empty() {
        return Err("Google sign-in isn't set up yet. Add a Google client under Account in Settings.".into());
    }
    Ok(k)
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Saved {
    refresh_token: String,
    email: String,
}

fn path() -> std::path::PathBuf {
    config_dir().join("account.json")
}

fn saved() -> Saved {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

struct Live {
    token: String,
    until: Instant,
    folder: Option<String>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

pub fn signed_in() -> bool {
    !saved().refresh_token.is_empty()
}

pub fn status() -> Value {
    let s = saved();
    json!({ "configured": keys().is_ok(), "email": (!s.refresh_token.is_empty()).then_some(s.email) })
}

pub fn sign_out() {
    let _ = std::fs::remove_file(path());
    if let Ok(mut l) = LIVE.lock() {
        *l = None;
    }
}

/// A reply's status and JSON body, whether Google said yes or no. Only a dead connection is an error.
fn read(r: Result<ureq::Response, ureq::Error>) -> R<(u16, Value)> {
    match r {
        Ok(resp) => Ok((resp.status(), resp.into_json().unwrap_or(Value::Null))),
        Err(ureq::Error::Status(c, resp)) => Ok((c, resp.into_json().unwrap_or(Value::Null))),
        Err(e) => Err(format!("Couldn't reach Google. Check your internet connection. ({e})")),
    }
}

fn ok(r: Result<ureq::Response, ureq::Error>) -> R<Value> {
    let (c, v) = read(r)?;
    if (200..300).contains(&c) {
        return Ok(v);
    }
    if c == 401 {
        if let Ok(mut l) = LIVE.lock() {
            *l = None;
        }
    }
    let msg = v["error"]["message"].as_str().or(v["error_description"].as_str()).or(v["error"].as_str()).unwrap_or("no details");
    Err(format!("Google Drive reported an error ({c}): {msg}"))
}

const WAIT: Duration = Duration::from_secs(60);

/// Google answers 400 or 401 when the client can't use the device flow at all — it was made as a
/// Desktop app, a Web application or an Android app, which is the easy mistake in the console — or
/// when its secret is missing or wrong. A bare "(401)" costs an afternoon, so name the fix.
fn refused(e: String) -> String {
    if e.contains("(401)") || e.contains("(400)") {
        format!(
            "{e} — signing in this way needs a Google client of the type \"TVs and Limited Input devices\", with \
             its client secret. Desktop app, Web application and Android clients are refused here, and an \
             Android client's package name and SHA-1 don't apply."
        )
    } else {
        e
    }
}

/// Start signing in. The page shows `user_code` and sends the player to `verification_url`.
pub fn begin() -> R<Value> {
    let k = keys()?;
    let v = ok(ureq::post("https://oauth2.googleapis.com/device/code").timeout(WAIT).send_form(&[("client_id", &k.id), ("scope", SCOPE)])).map_err(refused)?;
    Ok(json!({
        "device_code": v["device_code"], "user_code": v["user_code"],
        "verification_url": v["verification_url"].as_str().unwrap_or("https://www.google.com/device"),
        "interval": v["interval"].as_u64().unwrap_or(5), "expires_in": v["expires_in"].as_u64().unwrap_or(1800),
    }))
}

fn email_of(id_token: &str) -> String {
    use base64::Engine;
    id_token
        .split('.')
        .nth(1)
        .and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p.trim_end_matches('=')).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["email"].as_str().map(String::from))
        .unwrap_or_else(|| "your Google account".into())
}

/// Ask Google once whether the player has finished. `None` means not yet.
pub fn poll(device_code: &str) -> R<Option<String>> {
    let k = keys()?;
    let (c, v) = read(ureq::post("https://oauth2.googleapis.com/token").timeout(WAIT).send_form(&[
        ("client_id", &k.id),
        ("client_secret", &k.secret),
        ("device_code", device_code),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
    ]))?;
    if c == 200 {
        let refresh = v["refresh_token"].as_str().unwrap_or("");
        if refresh.is_empty() {
            return Err("Google didn't finish the sign-in. Try again.".into());
        }
        let s = Saved { refresh_token: refresh.into(), email: email_of(v["id_token"].as_str().unwrap_or("")) };
        atomic_write(&path(), serde_json::to_string_pretty(&s).unwrap_or_default().as_bytes())?;
        keep(&v);
        return Ok(Some(s.email));
    }
    match v["error"].as_str().unwrap_or("") {
        "authorization_pending" | "slow_down" => Ok(None),
        "access_denied" => Err("The sign-in was cancelled.".into()),
        "expired_token" => Err("The code ran out. Start the sign-in again.".into()),
        e => Err(format!("Google refused the sign-in ({c}): {} {e}", v["error_description"].as_str().unwrap_or(""))),
    }
}

fn keep(v: &Value) {
    let secs = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(120);
    if let (Some(t), Ok(mut l)) = (v["access_token"].as_str(), LIVE.lock()) {
        let folder = l.take().and_then(|x| x.folder);
        *l = Some(Live { token: t.into(), until: Instant::now() + Duration::from_secs(secs), folder });
    }
}

fn token() -> R<String> {
    if let Ok(l) = LIVE.lock() {
        if let Some(x) = l.as_ref().filter(|x| x.until > Instant::now()) {
            return Ok(x.token.clone());
        }
    }
    let s = saved();
    if s.refresh_token.is_empty() {
        return Err("Sign in to your account first.".into());
    }
    let k = keys()?;
    let (c, v) = read(ureq::post("https://oauth2.googleapis.com/token").timeout(WAIT).send_form(&[
        ("client_id", &k.id),
        ("client_secret", &k.secret),
        ("refresh_token", &s.refresh_token),
        ("grant_type", "refresh_token"),
    ]))?;
    if c != 200 {
        if v["error"] == "invalid_grant" {
            sign_out();
            return Err("Your Google sign-in has expired. Sign in again.".into());
        }
        return Err(format!("Google refused the sign-in ({c}): {}", v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("")));
    }
    keep(&v);
    v["access_token"].as_str().map(String::from).ok_or_else(|| "Google sent no access token.".to_string())
}

fn req(method: &str, url: &str) -> R<ureq::Request> {
    Ok(ureq::request(method, url).timeout(Duration::from_secs(600)).set("authorization", &format!("Bearer {}", token()?)))
}

/// The Drive folder the stories live in, made on first use.
fn folder() -> R<String> {
    if let Ok(l) = LIVE.lock() {
        if let Some(f) = l.as_ref().and_then(|x| x.folder.clone()) {
            return Ok(f);
        }
    }
    let q = format!("name = '{FOLDER}' and mimeType = '{FOLDER_MIME}' and trashed = false");
    let v = ok(req("GET", "https://www.googleapis.com/drive/v3/files")?.query("q", &q).query("fields", "files(id)").call())?;
    let id = match v["files"][0]["id"].as_str() {
        Some(id) => id.to_string(),
        None => ok(req("POST", "https://www.googleapis.com/drive/v3/files")?.query("fields", "id").send_json(json!({ "name": FOLDER, "mimeType": FOLDER_MIME })))?["id"]
            .as_str()
            .ok_or("Google Drive didn't make the folder.")?
            .to_string(),
    };
    if let Ok(mut l) = LIVE.lock() {
        if let Some(x) = l.as_mut() {
            x.folder = Some(id.clone());
        }
    }
    Ok(id)
}

/// One story in the account.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Remote {
    pub file_id: String,
    /// The campaign's own id (`.keeper/campaign_id`): the same story on every device.
    pub id: String,
    pub name: String,
    /// Hash of the story's files, to tell whether a device already has this version.
    pub hash: String,
    pub device: String,
    pub modified: String,
    pub size: u64,
}

fn remote(v: &Value) -> Remote {
    let p = &v["appProperties"];
    let text = |x: &Value| x.as_str().unwrap_or("").to_string();
    Remote {
        file_id: text(&v["id"]),
        id: text(&p["ck_id"]),
        name: text(&v["name"]).trim_end_matches(".ckstory").to_string(),
        hash: text(&p["ck_hash"]),
        device: text(&p["ck_device"]),
        modified: text(&v["modifiedTime"]),
        size: v["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
    }
}

pub fn list() -> R<Vec<Remote>> {
    let q = format!("'{}' in parents and trashed = false", folder()?);
    let mut out = Vec::new();
    let mut page = String::new();
    loop {
        let mut r = req("GET", "https://www.googleapis.com/drive/v3/files")?.query("q", &q).query("pageSize", "200").query("fields", &format!("nextPageToken,files({FIELDS})"));
        if !page.is_empty() {
            r = r.query("pageToken", &page);
        }
        let v = ok(r.call())?;
        out.extend(v["files"].as_array().into_iter().flatten().map(remote).filter(|r| !r.id.is_empty()));
        match v["nextPageToken"].as_str() {
            Some(p) => page = p.to_string(),
            None => break,
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(out)
}

/// Put a story in the account, replacing `existing` (a Drive file id) when it's already there.
pub fn upload(existing: Option<&str>, id: &str, name: &str, hash: &str, device: &str, bytes: &[u8]) -> R<Remote> {
    let device: String = device.chars().take(40).collect();
    let mut meta = json!({ "name": format!("{name}.ckstory"), "appProperties": { "ck_id": id, "ck_hash": hash, "ck_device": device } });
    let start = match existing {
        Some(f) => req("PATCH", &format!("https://www.googleapis.com/upload/drive/v3/files/{f}"))?,
        None => {
            meta["parents"] = json!([folder()?]);
            req("POST", "https://www.googleapis.com/upload/drive/v3/files")?
        }
    };
    let resp = match start.query("uploadType", "resumable").query("fields", FIELDS).set("x-upload-content-type", "application/zip").send_json(meta) {
        Ok(r) => r,
        other => return Err(ok(other).err().unwrap_or_else(|| "Google Drive didn't start the upload.".into())),
    };
    let session = resp.header("location").ok_or("Google Drive didn't start the upload.")?.to_string();
    let v = ok(ureq::put(&session).timeout(Duration::from_secs(1800)).set("content-type", "application/zip").send_bytes(bytes))?;
    let r = remote(&v);
    if r.file_id.is_empty() {
        return Err("Google Drive didn't confirm the upload.".into());
    }
    Ok(r)
}

pub fn download(file_id: &str) -> R<Vec<u8>> {
    use std::io::Read;
    let resp = match req("GET", &format!("https://www.googleapis.com/drive/v3/files/{file_id}"))?.query("alt", "media").call() {
        Ok(r) => r,
        other => return Err(ok(other).err().unwrap_or_else(|| "Google Drive didn't send the story.".into())),
    };
    let mut b = Vec::new();
    resp.into_reader().take(4 << 30).read_to_end(&mut b).map_err(|e| format!("The download was cut off: {e}"))?;
    Ok(b)
}

/// Move a story to the Drive trash (Google keeps it 30 days). Copies on devices are untouched.
pub fn remove(file_id: &str) -> R<()> {
    ok(req("PATCH", &format!("https://www.googleapis.com/drive/v3/files/{file_id}"))?.send_json(json!({ "trashed": true }))).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::refused;

    #[test]
    fn a_refused_client_is_told_which_type_to_make() {
        let e = refused("Google Drive reported an error (401): Unauthorized".into());
        assert!(e.starts_with("Google Drive reported an error (401): Unauthorized"), "{e}");
        assert!(e.contains("TVs and Limited Input devices"), "{e}");
        assert!(e.contains("client secret"), "{e}");
        // A dead connection has nothing to do with the client, so it keeps its own words.
        let offline = "Couldn't reach Google. Check your internet connection. (io)";
        assert_eq!(refused(offline.into()), offline);
    }
}
