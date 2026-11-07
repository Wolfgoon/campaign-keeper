//! A campaign is a folder of Markdown files (00_GM .. 08_Saves) plus a hidden `.keeper/` folder.
//! The layout and file names are the same as in 1.x, so old campaigns just open.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use include_dir::{include_dir, Dir};
use serde::Serialize;

use crate::util::{atomic_write, hash};

static TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../TEMPLATE");

pub const FOLDERS: [&str; 9] =
    ["00_GM", "01_Character", "02_World", "03_NPCs", "04_Quests", "05_Memory", "06_Sessions", "07_Maps", "08_Saves"];
pub const INSTRUCTIONS: &str = "00_GM/ai_dungeon_plus_instructions.txt";

pub fn is_campaign(dir: &Path) -> bool {
    FOLDERS.iter().filter(|f| dir.join(f).is_dir()).count() >= 7
}

pub fn keeper(dir: &Path) -> PathBuf {
    dir.join(".keeper")
}

pub fn name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Campaign".into())
}

/// Make a new campaign from the built-in template.
pub fn create(dir: &Path, title: &str) -> Result<(), String> {
    if dir.exists() && std::fs::read_dir(dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(format!("{} isn't empty", dir.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    TEMPLATE.extract(dir).map_err(|e| e.to_string())?;
    let readme = dir.join("README.md");
    if let Ok(t) = std::fs::read_to_string(&readme) {
        let _ = std::fs::write(&readme, t.replace("[Campaign Title]", title));
    }
    Ok(())
}

/// A path inside the campaign, or an error. Nothing outside the folder is ever touched.
pub fn resolve(dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim().replace('\\', "/");
    let p = Path::new(&rel);
    let ok = !rel.is_empty() && !p.is_absolute() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !ok {
        return Err(format!("\"{rel}\" isn't a valid path. Use a path inside the campaign folder, like 03_NPCs/tobin.md."));
    }
    Ok(dir.join(p))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    items.sort();
    for p in items {
        let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if n.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else if n.ends_with(".md") || n.ends_with(".txt") {
            out.push(p);
        }
    }
}

pub fn rel(dir: &Path, p: &Path) -> String {
    p.strip_prefix(dir).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Every campaign file, as paths relative to the folder.
pub fn files(dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    walk(dir, &mut v);
    v.iter().map(|p| rel(dir, p)).collect()
}

fn is_blank_form(path: &str) -> bool {
    path.rsplit('/').next().unwrap_or("").starts_with('_')
}

/// Text sent to the GM: not the instructions, README or blank forms, and only the latest save.
pub fn game_files(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for f in files(dir) {
        if f == INSTRUCTIONS || f == "README.md" || is_blank_form(&f) || f.starts_with("08_Saves/") {
            continue;
        }
        if let Ok(t) = std::fs::read_to_string(dir.join(&f)) {
            out.insert(f, t);
        }
    }
    if let Some(s) = latest_save(dir) {
        if let Ok(t) = std::fs::read_to_string(dir.join(&s)) {
            out.insert(s, t);
        }
    }
    out
}

pub fn latest_save(dir: &Path) -> Option<String> {
    let mut saves: Vec<String> = files(dir).into_iter().filter(|f| f.starts_with("08_Saves/") && !is_blank_form(f)).collect();
    saves.sort();
    saves.pop()
}

pub fn store_save(dir: &Path, block: &str) -> Result<String, String> {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let n = (1..100).find(|n| !dir.join(format!("08_Saves/{date}_save_{n:02}.md")).exists()).unwrap_or(99);
    let f = format!("08_Saves/{date}_save_{n:02}.md");
    atomic_write(&dir.join(&f), block.as_bytes())?;
    Ok(f)
}

pub fn write(dir: &Path, rel_path: &str, text: &str) -> Result<(), String> {
    atomic_write(&resolve(dir, rel_path)?, text.as_bytes())
}

/// Nothing is ever deleted: it moves to `.keeper/trash`.
pub fn trash(dir: &Path, rel_path: &str) -> Result<(), String> {
    let p = resolve(dir, rel_path)?;
    let to = keeper(dir).join("trash").join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()).join(rel_path);
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&p, &to).map_err(|e| e.to_string())
}

pub fn rename(dir: &Path, from: &str, to: &str) -> Result<(), String> {
    let name = to.rsplit('/').next().unwrap_or("");
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err("File names can't contain < > : \" | ? or *".into());
    }
    let (a, b) = (resolve(dir, from)?, resolve(dir, to)?);
    if b.exists() {
        return Err(format!("{to} already exists"));
    }
    std::fs::rename(a, b).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
pub struct Hit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

pub fn search(dir: &Path, query: &str) -> Vec<Hit> {
    let q = query.to_lowercase();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for f in files(dir) {
        let Ok(t) = std::fs::read_to_string(dir.join(&f)) else { continue };
        for (i, l) in t.lines().enumerate() {
            if l.to_lowercase().contains(&q) {
                hits.push(Hit { path: f.clone(), line: i + 1, text: l.chars().take(240).collect() });
                if hits.len() >= 300 {
                    return hits;
                }
            }
        }
    }
    hits
}

/// A snapshot of the files, to tell what changed during play.
pub fn known(dir: &Path) -> BTreeMap<String, u64> {
    game_files(dir).iter().map(|(k, v)| (k.clone(), hash(v))).collect()
}

/// Copy the files about to change, so anything can be restored.
pub fn backup(dir: &Path, paths: &[String]) -> Result<PathBuf, String> {
    let root = keeper(dir).join("backups").join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
    for p in paths {
        let from = resolve(dir, p)?;
        if from.exists() {
            let to = root.join(p);
            std::fs::create_dir_all(to.parent().unwrap_or(&root)).map_err(|e| e.to_string())?;
            std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(root)
}

/// The value of a `- **Label:** value` line in a Markdown file.
pub fn field(text: &str, label: &str) -> Option<String> {
    let needle = format!("**{label}:**");
    text.lines().find_map(|l| {
        let i = l.find(&needle)?;
        let v = l[i + needle.len()..].trim().trim_matches('_').trim();
        (!v.is_empty()).then(|| v.to_string())
    })
}

#[derive(Serialize)]
pub struct Overview {
    pub name: String,
    pub path: String,
    pub setting: Option<String>,
    pub difficulty: Option<String>,
    pub characters: Vec<String>,
    pub quests: String,
    pub pinned: String,
    pub counts: BTreeMap<String, usize>,
    pub latest_save: Option<String>,
    pub last_session: Option<String>,
    pub file_tokens: u64,
}

pub fn overview(dir: &Path) -> Overview {
    let all = files(dir);
    let count = |pre: &str| all.iter().filter(|f| f.starts_with(pre) && !is_blank_form(f)).count();
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default();
    let settings = read("00_GM/settings.md");
    let mut counts = BTreeMap::new();
    for (k, pre) in [("NPC files", "03_NPCs/"), ("Locations", "02_World/Locations/"), ("Lore entries", "02_World/Items_and_Lore/"), ("Session logs", "06_Sessions/"), ("Saves", "08_Saves/")] {
        counts.insert(k.to_string(), count(pre));
    }
    Overview {
        name: name(dir),
        path: dir.display().to_string(),
        setting: field(&settings, "Setting"),
        difficulty: field(&settings, "Difficulty"),
        characters: all.iter().filter(|f| f.starts_with("01_Character/") && !is_blank_form(f)).cloned().collect(),
        quests: read("04_Quests/active_quests.md"),
        pinned: read("05_Memory/pinned_memory.md"),
        counts,
        latest_save: latest_save(dir),
        last_session: all.iter().filter(|f| f.starts_with("06_Sessions/") && !is_blank_form(f)).next_back().cloned(),
        file_tokens: game_files(dir).values().map(|t| crate::util::tokens(t)).sum(),
    }
}
