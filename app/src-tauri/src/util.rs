use std::path::Path;

/// Write to a temp file and rename over the target, so a crash never leaves half a file.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, data).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not replace {}: {e}", path.display())
    })
}

/// A stable content hash (FNV-1a, not the std hasher, which may change between Rust releases).
pub fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn clip(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s.to_string(),
    }
}

/// The last `n` characters of `s`.
pub fn tail(s: &str, n: usize) -> &str {
    match s.char_indices().rev().nth(n) {
        Some((i, _)) => &s[i..],
        None => s,
    }
}

/// About 4 characters to a token: good enough for budgeting, corrected by the real counts.
pub fn tokens(s: &str) -> u64 {
    (s.len() as u64) / 4
}

/// The first JSON object in a model's reply, which may wrap it in prose or a code fence.
pub fn json_object(reply: &str) -> Option<serde_json::Value> {
    let a = reply.find('{')?;
    let b = reply.rfind('}')?;
    (b > a).then(|| serde_json::from_str(&reply[a..=b]).ok()).flatten()
}

pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}
