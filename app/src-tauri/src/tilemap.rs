//! The picture map: a 64x64 grid of terrain tiles with tokens (the party, people, monsters) on
//! top, a place name and one line saying what is happening. It's for players who'd rather look
//! than read. A cheap helper model (Haiku, Gemini Flash, whatever `engine::helper` picks) keeps it
//! in step with the story: after each GM reply it gets the map and the reply and answers with
//! small drawing commands. The GM itself never spends a token on it.
//!
//! Each area (a town, a cave, the road) is kept on its own, so going back somewhere shows it as it
//! was. Data lives in `.keeper/tilemaps/`. A bad command is skipped; the good ones still apply.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::campaign::keeper;
use crate::config::Config;
use crate::util::atomic_write;

pub const SIZE: usize = 64;

/// (letter the model writes, name). The page draws each name as a 64x64 pixel-art tile.
pub const TERRAIN: &[(char, &str)] = &[
    ('.', "grass"),
    ('T', "forest"),
    ('~', "water"),
    ('W', "deep"),
    (':', "sand"),
    ('=', "road"),
    (',', "dirt"),
    ('_', "floor"),
    ('#', "wall"),
    ('^', "mountain"),
    ('*', "snow"),
    ('%', "swamp"),
    ('!', "lava"),
    ('H', "bridge"),
    ('D', "door"),
    ('B', "building"),
    ('C', "cave"),
    ('f', "farm"),
    ('o', "rock"),
    (' ', "void"),
];

pub const KINDS: &[&str] = &["hero", "ally", "npc", "enemy", "monster", "item", "chest", "camp", "quest"];

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Token {
    pub x: usize,
    pub y: usize,
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Area {
    pub name: String,
    /// SIZE rows of SIZE terrain letters.
    pub rows: Vec<String>,
    pub tokens: BTreeMap<String, Token>,
    pub labels: BTreeMap<String, (usize, usize)>,
    /// What is happening right now, in a few words.
    pub now: String,
}

impl Default for Area {
    fn default() -> Self {
        Area { name: String::new(), rows: vec![".".repeat(SIZE); SIZE], tokens: BTreeMap::new(), labels: BTreeMap::new(), now: String::new() }
    }
}

fn terrain(word: &str) -> Result<char, String> {
    let w = word.trim().to_lowercase();
    TERRAIN
        .iter()
        .find(|(c, n)| *n == w || (w.chars().count() == 1 && w.starts_with(*c)))
        .map(|(c, _)| *c)
        .ok_or_else(|| format!("unknown terrain '{word}'"))
}

fn coord(t: Option<&str>) -> Result<usize, String> {
    let n: usize = t.and_then(|s| s.parse().ok()).ok_or("needs whole-number coordinates")?;
    if n >= SIZE { Err(format!("coordinates go from 0 to {}", SIZE - 1)) } else { Ok(n) }
}

impl Area {
    fn set(&mut self, x: usize, y: usize, c: char) {
        if x < SIZE && y < SIZE {
            let mut r: Vec<char> = self.rows[y].chars().collect();
            r[x] = c;
            self.rows[y] = r.into_iter().collect();
        }
    }

    fn fix(&mut self) {
        self.rows.resize(SIZE, String::new());
        for r in self.rows.iter_mut() {
            let mut v: Vec<char> = r.chars().map(|c| if TERRAIN.iter().any(|(t, _)| *t == c) { c } else { '.' }).take(SIZE).collect();
            v.resize(SIZE, '.');
            *r = v.into_iter().collect();
        }
    }

    /// One drawing command inside this area. Switching areas is handled by `apply`.
    pub fn edit(&mut self, line: &str) -> Result<(), String> {
        let mut t = line.split_whitespace();
        let verb = t.next().unwrap_or("").to_lowercase();
        let rest = |t: std::str::SplitWhitespace| t.collect::<Vec<_>>().join(" ");
        match verb.as_str() {
            "put" => {
                let (x, y) = (coord(t.next())?, coord(t.next())?);
                let c = terrain(&rest(t))?;
                self.set(x, y, c);
            }
            "fill" | "rect" => {
                let (x, y) = (coord(t.next())?, coord(t.next())?);
                let w: usize = t.next().and_then(|s| s.parse().ok()).ok_or("fill needs x y w h terrain")?;
                let h: usize = t.next().and_then(|s| s.parse().ok()).ok_or("fill needs x y w h terrain")?;
                let c = terrain(&rest(t))?;
                for j in y..(y + h).min(SIZE) {
                    for i in x..(x + w).min(SIZE) {
                        self.set(i, j, c);
                    }
                }
            }
            "line" => {
                let (x0, y0, x1, y1) = (coord(t.next())? as i64, coord(t.next())? as i64, coord(t.next())? as i64, coord(t.next())? as i64);
                let c = terrain(&rest(t))?;
                // Bresenham, so roads and rivers join up without gaps.
                let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
                let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
                let (mut x, mut y, mut err) = (x0, y0, dx + dy);
                loop {
                    self.set(x as usize, y as usize, c);
                    if x == x1 && y == y1 {
                        break;
                    }
                    let e2 = 2 * err;
                    if e2 >= dy {
                        err += dy;
                        x += sx;
                    }
                    if e2 <= dx {
                        err += dx;
                        y += sy;
                    }
                }
            }
            "circle" => {
                let (cx, cy) = (coord(t.next())? as i64, coord(t.next())? as i64);
                let r: i64 = t.next().and_then(|s| s.parse().ok()).filter(|r| *r <= SIZE as i64).ok_or("circle needs x y radius terrain")?;
                let c = terrain(&rest(t))?;
                for j in (cy - r).max(0)..=(cy + r).min(SIZE as i64 - 1) {
                    for i in (cx - r).max(0)..=(cx + r).min(SIZE as i64 - 1) {
                        if (i - cx).pow(2) + (j - cy).pow(2) <= r * r + r {
                            self.set(i as usize, j as usize, c);
                        }
                    }
                }
            }
            "token" => {
                let kind = t.next().unwrap_or("").to_lowercase();
                if !KINDS.contains(&kind.as_str()) {
                    return Err(format!("token kind must be one of {}", KINDS.join(", ")));
                }
                let (x, y) = (coord(t.next())?, coord(t.next())?);
                let name = rest(t);
                if name.is_empty() {
                    return Err("token needs a name".into());
                }
                let old = self.tokens.keys().find(|k| k.eq_ignore_ascii_case(&name)).cloned();
                if let Some(o) = old {
                    self.tokens.remove(&o);
                }
                self.tokens.insert(name, Token { x, y, kind });
            }
            "move" => {
                let (x, y) = (coord(t.next())?, coord(t.next())?);
                let name = rest(t);
                let tk = self.tokens.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|p| p.1).ok_or_else(|| format!("no token called {name}"))?;
                tk.x = x;
                tk.y = y;
            }
            "remove" => {
                let name = rest(t);
                let k = self.tokens.keys().find(|k| k.eq_ignore_ascii_case(&name)).cloned().ok_or_else(|| format!("no token called {name}"))?;
                self.tokens.remove(&k);
            }
            "label" => {
                let (x, y) = (coord(t.next())?, coord(t.next())?);
                let text: String = rest(t).chars().take(40).collect();
                if text.is_empty() {
                    return Err("label needs text".into());
                }
                self.labels.retain(|k, _| !k.eq_ignore_ascii_case(&text));
                self.labels.insert(text, (x, y));
            }
            "unlabel" => {
                let text = rest(t);
                self.labels.retain(|k, _| !k.eq_ignore_ascii_case(&text));
            }
            "now" => self.now = rest(t).chars().take(160).collect(),
            other => return Err(format!("unknown command '{other}'")),
        }
        Ok(())
    }

    /// The map as the helper reads it: a coordinate ruler, the rows, then what's on it.
    pub fn describe(&self) -> String {
        let mut s = format!("AREA: {}\nNOW: {}\n", self.name, self.now);
        s.push_str("    ");
        for x in 0..SIZE {
            s.push(char::from(b'0' + (x % 10) as u8));
        }
        s.push('\n');
        for (y, r) in self.rows.iter().enumerate() {
            s.push_str(&format!("{y:>2}  {r}\n"));
        }
        s.push_str("TOKENS:\n");
        for (n, t) in &self.tokens {
            s.push_str(&format!("  {} {} {} {n}\n", t.kind, t.x, t.y));
        }
        s.push_str("LABELS:\n");
        for (n, (x, y)) in &self.labels {
            s.push_str(&format!("  {x} {y} {n}\n"));
        }
        s
    }
}

fn slug(name: &str) -> String {
    let s: String = name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    if s.trim_matches('_').is_empty() { "area".into() } else { s.chars().take(60).collect() }
}

fn folder(dir: &Path) -> PathBuf {
    keeper(dir).join("tilemaps")
}

fn load(dir: &Path, name: &str) -> Option<Area> {
    let mut a: Area = serde_json::from_str(&std::fs::read_to_string(folder(dir).join(format!("{}.json", slug(name)))).ok()?).ok()?;
    a.fix();
    Some(a)
}

fn store(dir: &Path, a: &Area) -> Result<(), String> {
    atomic_write(&folder(dir).join(format!("{}.json", slug(&a.name))), serde_json::to_string(a).unwrap_or_default().as_bytes())?;
    atomic_write(&folder(dir).join("_current.txt"), a.name.as_bytes())
}

/// Things the GM said that a picture can't show (what was learned, gained, agreed), newest last.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    pub at: i64,
}

pub const MAX_NOTES: usize = 60;

fn notes(dir: &Path) -> Vec<Note> {
    std::fs::read_to_string(folder(dir).join("_notes.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn current(dir: &Path) -> Option<Area> {
    let name = std::fs::read_to_string(folder(dir).join("_current.txt")).ok()?;
    load(dir, name.trim())
}

/// Changes whenever the map, its notes or its art change. Cheap: file times only.
pub fn stamp(dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for d in [folder(dir), keeper(dir).join("tileset")] {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            e.file_name().hash(&mut h);
            e.metadata().and_then(|m| m.modified()).ok().hash(&mut h);
        }
    }
    h.finish()
}

/// What the page draws.
pub fn view(dir: &Path) -> Value {
    let notes = notes(dir);
    match current(dir) {
        None if !notes.is_empty() => json!({ "size": SIZE, "name": "", "now": "", "rows": [], "tokens": {}, "labels": {}, "terrain": {}, "notes": notes, "art": {} }),
        Some(a) => json!({ "notes": notes, "art": crate::tileart::all(dir), "size": SIZE, "name": a.name, "now": a.now, "rows": a.rows, "tokens": a.tokens, "labels": a.labels,
                           "terrain": TERRAIN.iter().map(|(c, n)| (c.to_string(), *n)).collect::<BTreeMap<_, _>>() }),
        None => Value::Null,
    }
}

/// Run a block of commands against the current area. `area <name>` switches (or starts) an area;
/// `new <terrain> <name>` starts one afresh, filled with that terrain. Returns (applied, errors).
pub fn apply(dir: &Path, body: &str) -> Result<(usize, Vec<String>), String> {
    let mut a = current(dir);
    let mut news = notes(dir);
    let before = news.len();
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let mut done = 0;
    let mut errors = Vec::new();
    for line in body.lines().map(|l| l.trim().trim_start_matches(['-', '*', '`']).trim()).filter(|l| !l.is_empty()) {
        let (verb, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match verb.to_lowercase().as_str() {
            "none" | "nothing" => {}
            "note" if !rest.is_empty() => {
                news.push(Note { text: rest.chars().take(200).collect(), at });
                done += 1;
            }
            "area" if !rest.is_empty() => {
                if let Some(old) = &a {
                    store(dir, old)?;
                }
                let name: String = rest.chars().take(60).collect();
                a = Some(load(dir, &name).unwrap_or(Area { name, ..Default::default() }));
                done += 1;
            }
            "new" => {
                let (t, name) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                match terrain(t) {
                    Ok(c) if !name.trim().is_empty() => {
                        if let Some(old) = &a {
                            store(dir, old)?;
                        }
                        let name: String = name.trim().chars().take(60).collect();
                        a = Some(Area { name, rows: vec![c.to_string().repeat(SIZE); SIZE], ..Default::default() });
                        done += 1;
                    }
                    Ok(_) => errors.push(format!("{line} (new needs a terrain and a name)")),
                    Err(e) => errors.push(format!("{line} ({e})")),
                }
            }
            _ => {
                let area = a.get_or_insert_with(|| Area { name: "Here".into(), ..Default::default() });
                match area.edit(line) {
                    Ok(()) => done += 1,
                    Err(e) => errors.push(format!("{line} ({e})")),
                }
            }
        }
    }
    if news.len() != before {
        let skip = news.len().saturating_sub(MAX_NOTES);
        atomic_write(&folder(dir).join("_notes.json"), serde_json::to_string(&news[skip..]).unwrap_or_default().as_bytes())?;
    }
    if let Some(a) = &a {
        if done > 0 {
            store(dir, a)?;
        }
    }
    Ok((done, errors))
}

const PROMPT: &str = include_str!("../prompts/tilemap.md");

/// One update at a time, so two quick turns can't interleave their edits.
static BUSY: Mutex<()> = Mutex::new(());

/// Ask the helper model to bring the map in line with the GM's newest reply.
pub fn update(cfg: &Config, dir: &Path, player: &str, gm: &str) -> Result<(usize, Vec<String>), String> {
    let _g = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    let map = current(dir).map(|a| a.describe()).unwrap_or_else(|| "NO MAP YET: start one with `new`.".into());
    let user = format!("THE MAP NOW:\n{map}\nTHE PLAYER DID:\n{player}\n\nTHE GM REPLIED:\n{gm}\n\nReply with map commands only.");
    let reply = crate::engine::helper(cfg, PROMPT, &user)?;
    // Models like code fences; keep only what's inside if there is one.
    let body = match reply.split("```").nth(1) {
        Some(inner) => inner.trim_start_matches(|c: char| c.is_alphabetic()).to_string(),
        None => reply,
    };
    apply(dir, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ck-tile-{}", uuid::Uuid::new_v4())).join("c");
        crate::campaign::create(&d, "T").unwrap();
        d
    }

    #[test]
    fn commands_draw_and_tokens_move() {
        let d = camp();
        let (n, e) = apply(&d, "new grass Millbrook\nline 0 10 63 10 road\nfill 2 2 3 3 building\ncircle 40 40 3 water\ntoken hero 5 10 Wren\nlabel 2 1 Inn\nnow Wren arrives at dusk").unwrap();
        assert_eq!((n, e.len()), (7, 0));
        let a = current(&d).unwrap();
        assert_eq!(a.rows[10], "=".repeat(SIZE));
        assert_eq!(&a.rows[3][2..5], "BBB");
        assert_eq!(a.rows[40].chars().nth(40), Some('~'));
        apply(&d, "move 6 11 wren").unwrap();
        assert_eq!(current(&d).unwrap().tokens["Wren"], Token { x: 6, y: 11, kind: "hero".into() });
    }

    #[test]
    fn areas_are_kept_apart_and_come_back() {
        let d = camp();
        apply(&d, "new grass Town\nput 1 1 wall").unwrap();
        apply(&d, "new floor Cave\ntoken monster 3 3 Rat").unwrap();
        assert_eq!(current(&d).unwrap().name, "Cave");
        apply(&d, "area Town").unwrap();
        let a = current(&d).unwrap();
        assert_eq!(a.rows[1].chars().nth(1), Some('#'));
        assert!(a.tokens.is_empty());
    }

    #[test]
    fn bad_lines_are_skipped_without_harm() {
        let d = camp();
        let (n, e) = apply(&d, "new grass X\nput 99 0 wall\nfill 0 0 999 999 lava\ntoken dragon 1 1 Bob\nmove 1 1 Nobody\nput 1 1 plasma\ngibberish\n```").unwrap();
        assert_eq!(n, 2, "new and the oversized-but-clamped fill");
        assert_eq!(e.len(), 5);
        let a = current(&d).unwrap();
        assert!(a.rows.iter().all(|r| r.chars().count() == SIZE) && a.rows.len() == SIZE);
    }

    #[test]
    fn notes_pile_up_newest_last_and_are_capped() {
        let d = camp();
        apply(&d, "note You learned the mayor is lying
note +20 gold").unwrap();
        for i in 0..MAX_NOTES {
            apply(&d, &format!("note n{i}")).unwrap();
        }
        let v = view(&d);
        let n = v["notes"].as_array().unwrap();
        assert_eq!(n.len(), MAX_NOTES);
        assert_eq!(n.last().unwrap()["text"], format!("n{}", MAX_NOTES - 1));
    }

    #[test]
    fn names_cannot_escape_the_folder() {
        let d = camp();
        apply(&d, "new grass ../../evil").unwrap();
        assert!(!d.parent().unwrap().join("evil.json").exists());
    }
}
