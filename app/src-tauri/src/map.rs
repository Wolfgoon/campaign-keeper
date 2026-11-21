//! ASCII maps the GM keeps and edits. A map is a character grid plus named markers (people, the
//! party, a ship) drawn on top, so moving someone doesn't erase what's underneath. Edits are small
//! commands, so the GM never has to redraw the whole thing and can't lose part of it.
//!
//! The data lives in `.keeper/maps/`. A rendered copy goes to `07_Maps/` so it's a normal campaign
//! file. A bad command is reported back; the good ones still apply.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::campaign::{self, keeper};
use crate::util::atomic_write;

pub const MAX_W: usize = 160;
pub const MAX_H: usize = 80;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Marker {
    pub x: usize,
    pub y: usize,
    pub ch: char,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Map {
    pub name: String,
    pub rows: Vec<Vec<char>>,
    pub markers: BTreeMap<String, Marker>,
    /// Symbol -> what it means.
    pub legend: BTreeMap<String, String>,
}

impl Map {
    fn grow(&mut self, w: usize, h: usize) -> Result<(), String> {
        if w > MAX_W || h > MAX_H {
            return Err(format!("the map can't be larger than {MAX_W}x{MAX_H}"));
        }
        while self.rows.len() < h {
            self.rows.push(Vec::new());
        }
        for r in self.rows.iter_mut().take(h) {
            while r.len() < w {
                r.push(' ');
            }
        }
        Ok(())
    }

    fn put(&mut self, x: usize, y: usize, c: char) -> Result<(), String> {
        self.grow(x + 1, y + 1)?;
        self.rows[y][x] = c;
        Ok(())
    }

    fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, mut f: impl FnMut(usize, usize) -> Option<char>) -> Result<(), String> {
        if w == 0 || h == 0 {
            return Err("that area is empty".into());
        }
        self.grow(x + w, y + h)?;
        for j in 0..h {
            for i in 0..w {
                if let Some(c) = f(i, j) {
                    self.rows[y + j][x + i] = c;
                }
            }
        }
        Ok(())
    }

    /// The whole map with markers on top. Trailing spaces go.
    pub fn render(&self) -> String {
        let mut rows = self.rows.clone();
        for m in self.markers.values() {
            if m.y >= rows.len() {
                rows.resize(m.y + 1, Vec::new());
            }
            if rows[m.y].len() <= m.x {
                rows[m.y].resize(m.x + 1, ' ');
            }
            rows[m.y][m.x] = m.ch;
        }
        let text: Vec<String> = rows.iter().map(|r| r.iter().collect::<String>().trim_end().to_string()).collect();
        text.join("\n")
    }

    pub fn key(&self) -> String {
        let mut k: Vec<String> = self.markers.iter().map(|(n, m)| format!("{} {n}", m.ch)).collect();
        k.extend(self.legend.iter().map(|(c, n)| format!("{c} {n}")));
        k.join("\n")
    }

    /// Run one edit command.
    pub fn edit(&mut self, line: &str) -> Result<(), String> {
        let line = line.trim();
        let (verb, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        let nums = |n: usize| -> Result<Vec<usize>, String> {
            let v: Vec<usize> = rest.split_whitespace().take(n).filter_map(|t| t.parse().ok()).collect();
            if v.len() == n { Ok(v) } else { Err(format!("'{verb}' needs {n} whole numbers")) }
        };
        // The text after `n` numbers.
        let after = |n: usize| -> String { rest.split_whitespace().skip(n).collect::<Vec<_>>().join(" ") };
        let ch_at = |n: usize| -> Result<char, String> { after(n).chars().next().ok_or_else(|| format!("'{verb}' needs a character")) };
        match verb.to_lowercase().as_str() {
            "size" => {
                let v = nums(2)?;
                self.grow(v[0], v[1])
            }
            "put" => {
                let v = nums(2)?;
                self.put(v[0], v[1], ch_at(2)?)
            }
            "text" => {
                let v = nums(2)?;
                // Spacing inside the label is kept.
                let mut it = rest.splitn(3, char::is_whitespace);
                it.next();
                it.next();
                let t = it.next().unwrap_or("").trim_start();
                if t.is_empty() {
                    return Err("'text' needs something to write".into());
                }
                for (i, c) in t.chars().enumerate() {
                    self.put(v[0] + i, v[1], c)?;
                }
                Ok(())
            }
            "hline" => {
                let v = nums(3)?;
                let c = ch_at(3)?;
                self.rect(v[0], v[1], v[2], 1, |_, _| Some(c))
            }
            "vline" => {
                let v = nums(3)?;
                let c = ch_at(3)?;
                self.rect(v[0], v[1], 1, v[2], |_, _| Some(c))
            }
            "fill" => {
                let v = nums(4)?;
                let c = ch_at(4)?;
                self.rect(v[0], v[1], v[2], v[3], |_, _| Some(c))
            }
            "clear" => {
                let v = nums(4)?;
                self.rect(v[0], v[1], v[2], v[3], |_, _| Some(' '))
            }
            "box" => {
                let v = nums(4)?;
                let (w, h) = (v[2], v[3]);
                self.rect(v[0], v[1], w, h, |i, j| {
                    let (ex, ey) = (i == 0 || i == w - 1, j == 0 || j == h - 1);
                    match (ex, ey) {
                        (true, true) => Some('+'),
                        (false, true) => Some('-'),
                        (true, false) => Some('|'),
                        _ => None,
                    }
                })
            }
            "marker" => {
                // marker @ 5 3 Wren Halloway
                let mut t = rest.split_whitespace();
                let ch = t.next().and_then(|c| c.chars().next()).ok_or("'marker' needs a symbol")?;
                let x: usize = t.next().and_then(|n| n.parse().ok()).ok_or("'marker' needs x")?;
                let y: usize = t.next().and_then(|n| n.parse().ok()).ok_or("'marker' needs y")?;
                let name = t.collect::<Vec<_>>().join(" ");
                if name.is_empty() {
                    return Err("'marker' needs a name".into());
                }
                self.grow(x + 1, y + 1)?;
                self.markers.insert(name, Marker { x, y, ch });
                Ok(())
            }
            "move" => {
                // move 6 3 Wren Halloway
                let v = nums(2)?;
                let name = after(2);
                self.grow(v[0] + 1, v[1] + 1)?;
                let m = self.markers.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|x| x.1).ok_or_else(|| format!("no marker called {name}"))?;
                m.x = v[0];
                m.y = v[1];
                Ok(())
            }
            "remove" => {
                let key = self.markers.keys().find(|k| k.eq_ignore_ascii_case(rest)).cloned().ok_or_else(|| format!("no marker called {rest}"))?;
                self.markers.remove(&key);
                Ok(())
            }
            "legend" => {
                let (c, what) = rest.split_once(char::is_whitespace).ok_or("'legend' needs a symbol and a meaning")?;
                self.legend.insert(c.to_string(), what.trim().to_string());
                Ok(())
            }
            other => Err(format!("unknown map command '{other}'")),
        }
    }
}

fn slug(name: &str) -> String {
    let s: String = name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    if s.trim_matches('_').is_empty() { "map".into() } else { s.chars().take(60).collect() }
}

fn path(dir: &Path, name: &str) -> PathBuf {
    keeper(dir).join("maps").join(format!("{}.json", slug(name)))
}

pub fn load(dir: &Path, name: &str) -> Option<Map> {
    serde_json::from_str(&std::fs::read_to_string(path(dir, name)).ok()?).ok()
}

pub fn all(dir: &Path) -> Vec<Map> {
    let mut v: Vec<Map> = std::fs::read_dir(keeper(dir).join("maps"))
        .map(|d| d.flatten().filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

pub fn views(dir: &Path) -> Vec<Value> {
    all(dir).iter().map(|m| json!({ "name": m.name, "text": m.render(), "key": m.key() })).collect()
}

fn store(dir: &Path, m: &Map) -> Result<(), String> {
    atomic_write(&path(dir, &m.name), serde_json::to_string(m).unwrap_or_default().as_bytes())?;
    let key = m.key();
    let mut md = format!("# {}\n_Kept by the GM. Edits here are replaced the next time the map changes._\n\n```\n{}\n```\n", m.name, m.render());
    if !key.is_empty() {
        md.push_str(&format!("\n```\n{key}\n```\n"));
    }
    campaign::write(dir, &format!("07_Maps/{}.md", slug(&m.name)), &md)
}

pub struct Outcome {
    /// One line for the player.
    pub note: String,
    pub errors: Vec<String>,
    /// What to tell the GM it now looks like.
    pub render: String,
}

/// Apply a `<map>` (a whole new drawing) or `<map_edit>` (commands). `before` collects what the map
/// was, so /undo can put it back.
pub fn apply(dir: &Path, name: &str, body: &str, edit: bool, before: &mut Vec<Value>) -> Result<Outcome, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a map needs a name".into());
    }
    let old = load(dir, name);
    before.push(json!({ "name": name, "map": old }));
    let mut m = old.unwrap_or(Map { name: name.to_string(), ..Default::default() });
    let mut errors = Vec::new();
    if edit {
        let mut done = 0;
        for line in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
            match m.edit(line) {
                Ok(()) => done += 1,
                Err(e) => errors.push(format!("{line} ({e})")),
            }
        }
        if done == 0 {
            return Ok(Outcome { note: String::new(), errors, render: String::new() });
        }
    } else {
        let lines: Vec<&str> = body.trim_matches('\n').lines().collect();
        if lines.len() > MAX_H || lines.iter().any(|l| l.chars().count() > MAX_W) {
            return Err(format!("the map can't be larger than {MAX_W}x{MAX_H}"));
        }
        m.rows = lines.iter().map(|l| l.trim_end().chars().collect()).collect();
    }
    store(dir, &m)?;
    Ok(Outcome { note: format!("Map updated: {}", m.name), errors, render: format!("<map_updated name=\"{}\">\n{}\n{}\n</map_updated>", m.name, m.render(), m.key()) })
}

/// Put maps back as they were, for /undo and /retry.
pub fn restore(dir: &Path, before: &Value) {
    for b in before.as_array().into_iter().flatten().rev() {
        let name = b["name"].as_str().unwrap_or("");
        match serde_json::from_value::<Map>(b["map"].clone()) {
            Ok(m) if !b["map"].is_null() => {
                let _ = store(dir, &m);
            }
            _ => {
                let _ = std::fs::remove_file(path(dir, name));
                let _ = campaign::trash(dir, &format!("07_Maps/{}.md", slug(name)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ck-map-{}", uuid::Uuid::new_v4())).join("c");
        campaign::create(&d, "T").unwrap();
        d
    }

    #[test]
    fn commands_draw_what_they_say() {
        let mut m = Map::default();
        for l in ["box 0 0 6 3", "text 1 1 Inn", "hline 6 1 3 =", "put 9 1 X"] {
            m.edit(l).unwrap();
        }
        assert_eq!(m.render(), "+----+\n|Inn |===X\n+----+");
    }

    #[test]
    fn markers_sit_on_top_and_moving_leaves_the_map_intact() {
        let mut m = Map::default();
        m.edit("fill 0 0 4 1 ~").unwrap();
        m.edit("marker @ 1 0 Wren Halloway").unwrap();
        assert_eq!(m.render(), "~@~~");
        m.edit("move 2 0 wren halloway").unwrap();
        assert_eq!(m.render(), "~~@~");
        m.edit("remove Wren Halloway").unwrap();
        assert_eq!(m.render(), "~~~~");
    }

    #[test]
    fn bad_commands_are_refused_without_harm() {
        let mut m = Map::default();
        for l in ["put 1", "put -1 2 x", "nonsense 1 2", "move 1 1 Nobody", "remove Nobody", "fill 0 0 0 0 x", "put 999 0 x", "box 0 0 999 2", "text 1 1"] {
            assert!(m.edit(l).is_err(), "{l}");
        }
        assert!(m.rows.iter().all(|r| r.len() <= MAX_W));
    }

    #[test]
    fn unicode_keeps_columns() {
        let mut m = Map::default();
        m.edit("text 0 0 caf\u{e9}").unwrap();
        m.edit("put 4 0 X").unwrap();
        assert_eq!(m.render(), "caf\u{e9}X");
    }

    #[test]
    fn a_drawn_map_can_then_be_edited_and_undone() {
        let d = camp();
        let mut prev = Vec::new();
        apply(&d, "Varrow", "+--+\n|  |\n+--+", false, &mut prev).unwrap();
        assert!(std::fs::read_to_string(d.join("07_Maps/varrow.md")).unwrap().contains("|  |"));
        let mut prev2 = Vec::new();
        let o = apply(&d, "Varrow", "marker @ 1 1 Wren\nput 99999 0 x\nlegend ~ the Murl", true, &mut prev2).unwrap();
        assert_eq!(o.errors.len(), 1);
        assert!(o.render.contains("|@ |") && o.render.contains("~ the Murl"));
        restore(&d, &serde_json::json!(prev2));
        assert_eq!(load(&d, "Varrow").unwrap().render(), "+--+\n|  |\n+--+");
        assert!(load(&d, "Varrow").unwrap().markers.is_empty());
        restore(&d, &serde_json::json!(prev));
        assert!(load(&d, "Varrow").is_none());
        assert!(!d.join("07_Maps/varrow.md").exists());
    }

    #[test]
    fn map_names_cannot_escape_the_folder() {
        let d = camp();
        apply(&d, "../../evil", "x", false, &mut Vec::new()).unwrap();
        assert!(!d.parent().unwrap().join("evil.json").exists());
        assert_eq!(all(&d).len(), 1);
    }

    #[test]
    fn oversized_drawings_are_refused() {
        let d = camp();
        assert!(apply(&d, "Big", &"x".repeat(MAX_W + 1), false, &mut Vec::new()).is_err());
    }
}
