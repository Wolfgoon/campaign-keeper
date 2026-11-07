//! Long-term memory: one SQLite database per campaign, searched by words (FTS5/BM25) and by
//! meaning (bge-small embeddings, the model Spirit uses), merged with reciprocal rank fusion.
//!
//! Two things come straight from Spirit's experience. The database lives on this machine, outside
//! OneDrive (sync tools corrupt live databases); and after every change a plain-text backup is
//! written into the campaign's `.keeper/`, so memory travels with the folder and the database can
//! always be rebuilt from it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::campaign::{self, keeper};
use crate::config::local_dir;
use crate::memory::embed;
use crate::util::{atomic_write, hash, now};

const CHUNK: usize = 1200;
const RRF_K: f64 = 60.0;
const MIN_SIMILARITY: f32 = 0.35;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Memory {
    pub id: String,
    pub text: String,
    /// fact, event, npc, place, promise, turn (a verbatim exchange) or file (a campaign file chunk).
    pub kind: String,
    pub tags: String,
    pub source: String,
    pub importance: f64,
    /// The chat entry it came from; -1 when none.
    pub turn: i64,
    pub created: String,
    pub updated: String,
    #[serde(skip_deserializing)]
    pub score: f64,
}

pub struct Store {
    conn: Connection,
    backup: PathBuf,
}

fn campaign_id(dir: &Path) -> Result<String, String> {
    let p = keeper(dir).join("campaign_id");
    if let Ok(t) = std::fs::read_to_string(&p) {
        if !t.trim().is_empty() {
            return Ok(t.trim().to_string());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    atomic_write(&p, id.as_bytes())?;
    Ok(id)
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// Vectors are unit length, so the dot product is the cosine.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

const STOP: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "have", "has", "was", "were", "are", "you", "your", "his", "her", "she",
    "him", "they", "them", "what", "when", "where", "who", "why", "how", "into", "onto", "not", "but", "can", "will", "just",
    "then", "there", "their", "out", "our", "all", "any", "try", "going", "want",
];

/// An FTS5 query from free text: every word quoted, joined by OR, so punctuation can't break it.
fn fts_query(q: &str) -> String {
    let mut seen = HashSet::new();
    q.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| w.len() > 2 && !STOP.contains(&w.as_str()) && seen.insert(w.clone()))
        .take(32)
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for para in text.replace('\r', "").split('\n') {
        // A heading starts a new chunk once there's something to close.
        if para.starts_with('#') && cur.len() > 200 {
            out.push(cur.trim().to_string());
            cur.clear();
        }
        let mut para = para;
        if para.len() > CHUNK && !cur.trim().is_empty() {
            // A long paragraph starts its own chunk, so what's pending can't push it over.
            out.push(cur.trim().to_string());
            cur.clear();
        }
        while para.len() > CHUNK {
            let mut cut = CHUNK;
            while !para.is_char_boundary(cut) {
                cut -= 1;
            }
            cut = para[..cut].rfind(' ').filter(|c| *c > 0).unwrap_or(cut);
            out.push(para[..cut].trim().to_string());
            para = para[cut..].trim_start();
        }
        if cur.len() + para.len() + 1 > CHUNK && !cur.is_empty() {
            out.push(cur.trim().to_string());
            cur.clear();
        }
        cur.push('\n');
        cur.push_str(para);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store, String> {
        let id = campaign_id(dir)?;
        let path = local_dir().join("memory").join(format!("{id}.db"));
        std::fs::create_dir_all(path.parent().unwrap_or(dir)).map_err(|e| e.to_string())?;
        let fresh = !path.exists();
        let conn = Connection::open(&path).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS memories(
               id TEXT PRIMARY KEY, text TEXT NOT NULL, kind TEXT NOT NULL, tags TEXT NOT NULL DEFAULT '',
               source TEXT NOT NULL DEFAULT '', importance REAL NOT NULL DEFAULT 0.5, turn INTEGER NOT NULL DEFAULT -1,
               created TEXT NOT NULL, updated TEXT NOT NULL, vector BLOB);
             CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(id UNINDEXED, text, tags);
             CREATE TABLE IF NOT EXISTS file_index(path TEXT PRIMARY KEY, hash INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS memories_source ON memories(source);",
        )
        .map_err(|e| e.to_string())?;
        let s = Store { conn, backup: keeper(dir).join("memory-backup.jsonl") };
        if fresh {
            s.restore()?;
        }
        Ok(s)
    }

    /// First run on this computer: bring memory back from the file that travelled with the folder.
    fn restore(&self) -> Result<(), String> {
        let Ok(text) = std::fs::read_to_string(&self.backup) else { return Ok(()) };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(m) = serde_json::from_str::<Memory>(line) {
                self.insert(&m)?;
            }
        }
        Ok(())
    }

    fn insert(&self, m: &Memory) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO memories(id,text,kind,tags,source,importance,turn,created,updated) VALUES(?,?,?,?,?,?,?,?,?)",
                params![m.id, m.text, m.kind, m.tags, m.source, m.importance, m.turn, m.created, m.updated],
            )
            .map_err(|e| e.to_string())?;
        self.conn.execute("DELETE FROM memories_fts WHERE id=?", [&m.id]).map_err(|e| e.to_string())?;
        self.conn.execute("INSERT INTO memories_fts(id,text,tags) VALUES(?,?,?)", params![m.id, m.text, m.tags]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn add(&self, text: &str, kind: &str, tags: &[String], source: &str, importance: f64, turn: i64) -> Result<String, String> {
        let t = now();
        let m = Memory {
            id: uuid::Uuid::new_v4().to_string(),
            text: text.trim().to_string(),
            kind: kind.into(),
            tags: tags.join(","),
            source: source.into(),
            importance: importance.clamp(0.0, 1.0),
            turn,
            created: t.clone(),
            updated: t,
            score: 0.0,
        };
        self.insert(&m)?;
        Ok(m.id)
    }

    pub fn update(&self, id: &str, text: Option<&str>, importance: Option<f64>, tags: Option<&str>) -> Result<(), String> {
        let mut m = self.get(id)?.ok_or("no such memory")?;
        if let Some(t) = text {
            if t.trim() != m.text {
                self.conn.execute("UPDATE memories SET vector=NULL WHERE id=?", [id]).map_err(|e| e.to_string())?;
            }
            m.text = t.trim().to_string();
        }
        if let Some(i) = importance {
            m.importance = i.clamp(0.0, 1.0);
        }
        if let Some(t) = tags {
            m.tags = t.to_string();
        }
        m.updated = now();
        self.conn
            .execute("UPDATE memories SET text=?,importance=?,tags=?,updated=? WHERE id=?", params![m.text, m.importance, m.tags, m.updated, id])
            .map_err(|e| e.to_string())?;
        self.conn.execute("DELETE FROM memories_fts WHERE id=?", [id]).map_err(|e| e.to_string())?;
        self.conn.execute("INSERT INTO memories_fts(id,text,tags) VALUES(?,?,?)", params![id, m.text, m.tags]).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn row(r: &rusqlite::Row) -> rusqlite::Result<Memory> {
        Ok(Memory {
            id: r.get(0)?,
            text: r.get(1)?,
            kind: r.get(2)?,
            tags: r.get(3)?,
            source: r.get(4)?,
            importance: r.get(5)?,
            turn: r.get(6)?,
            created: r.get(7)?,
            updated: r.get(8)?,
            score: 0.0,
        })
    }

    const COLS: &'static str = "id,text,kind,tags,source,importance,turn,created,updated";

    pub fn get(&self, id: &str) -> Result<Option<Memory>, String> {
        self.conn
            .query_row(&format!("SELECT {} FROM memories WHERE id=?", Self::COLS), [id], Self::row)
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.conn.execute("DELETE FROM memories WHERE id=?", [id]).map_err(|e| e.to_string())?;
        self.conn.execute("DELETE FROM memories_fts WHERE id=?", [id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_source(&self, source: &str) -> Result<(), String> {
        let ids: Vec<String> = {
            let mut st = self.conn.prepare("SELECT id FROM memories WHERE source=?").map_err(|e| e.to_string())?;
            let x = st.query_map([source], |r| r.get(0)).map_err(|e| e.to_string())?.flatten().collect();
            x
        };
        for id in ids {
            self.delete(&id)?;
        }
        Ok(())
    }

    /// Memories of the given kinds, newest first.
    pub fn list(&self, kinds: &[&str], limit: usize) -> Result<Vec<Memory>, String> {
        let marks = vec!["?"; kinds.len().max(1)].join(",");
        let sql = format!("SELECT {} FROM memories WHERE kind IN ({marks}) ORDER BY created DESC, rowid DESC LIMIT ?", Self::COLS);
        let mut st = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut p: Vec<&dyn rusqlite::ToSql> = kinds.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
        let limit = limit as i64;
        p.push(&limit);
        let rows = st.query_map(p.as_slice(), Self::row).map_err(|e| e.to_string())?.flatten().collect();
        Ok(rows)
    }

    pub fn counts(&self) -> Result<HashMap<String, i64>, String> {
        let mut st = self.conn.prepare("SELECT kind,COUNT(*) FROM memories GROUP BY kind").map_err(|e| e.to_string())?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(|e| e.to_string())?.flatten().collect();
        Ok(rows)
    }

    pub fn unembedded(&self) -> Result<i64, String> {
        self.conn.query_row("SELECT COUNT(*) FROM memories WHERE vector IS NULL", [], |r| r.get(0)).map_err(|e| e.to_string())
    }

    /// Hybrid search. `skip` holds ids not to return; `live_from` hides verbatim turns the GM can
    /// still read in the chat itself.
    pub fn search(&self, query: &str, k: usize, skip: &[String], live_from: Option<i64>) -> Result<Vec<Memory>, String> {
        let mut rank: HashMap<String, f64> = HashMap::new();
        let fq = fts_query(query);
        if !fq.is_empty() {
            let mut st = self
                .conn
                .prepare("SELECT id FROM memories_fts WHERE memories_fts MATCH ? ORDER BY bm25(memories_fts) LIMIT 40")
                .map_err(|e| e.to_string())?;
            let ids: Vec<String> = st.query_map([&fq], |r| r.get(0)).map_err(|e| e.to_string())?.flatten().collect();
            for (i, id) in ids.into_iter().enumerate() {
                *rank.entry(id).or_default() += 1.0 / (RRF_K + i as f64);
            }
        }
        if let Some(qv) = embed::query(query) {
            let mut st = self.conn.prepare("SELECT id,vector FROM memories WHERE vector IS NOT NULL").map_err(|e| e.to_string())?;
            let mut sims: Vec<(String, f32)> = st
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|(id, b)| {
                    let s = cosine(&qv, &from_blob(&b));
                    (id, s)
                })
                .filter(|(_, s)| *s >= MIN_SIMILARITY)
                .collect();
            sims.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (i, (id, _)) in sims.into_iter().take(40).enumerate() {
                *rank.entry(id).or_default() += 1.0 / (RRF_K + i as f64);
            }
        }
        let skip: HashSet<&String> = skip.iter().collect();
        let mut out = Vec::new();
        for (id, r) in rank {
            if skip.contains(&id) {
                continue;
            }
            let Some(mut m) = self.get(&id)? else { continue };
            if m.kind == "turn" && live_from.is_some_and(|l| m.turn >= l) {
                continue;
            }
            // Important memories beat incidental ones of equal relevance.
            m.score = r * (0.7 + 0.6 * m.importance);
            out.push(m);
        }
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        out.truncate(k);
        Ok(out)
    }

    /// The closest existing memory by meaning, to merge instead of duplicating. Falls back to an
    /// exact comparison while embeddings aren't available.
    pub fn nearest(&self, text: &str, kinds: &[&str]) -> Result<Option<(String, f32)>, String> {
        let wanted = text.trim().to_lowercase();
        for m in self.list(kinds, 5000)? {
            if m.text.to_lowercase() == wanted {
                return Ok(Some((m.id, 1.0)));
            }
        }
        let Some(qv) = embed::passage(text) else { return Ok(None) };
        let mut st = self.conn.prepare("SELECT id,kind,vector FROM memories WHERE vector IS NOT NULL").map_err(|e| e.to_string())?;
        let best = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .filter(|(_, k, _)| kinds.contains(&k.as_str()))
            .map(|(id, _, b)| {
                let s = cosine(&qv, &from_blob(&b));
                (id, s)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        Ok(best)
    }

    /// Embed whatever has no vector yet. Runs in the background, a batch at a time.
    pub fn embed_pending(&self, limit: usize) -> Result<usize, String> {
        let rows: Vec<(String, String)> = {
            let mut st = self.conn.prepare("SELECT id,text FROM memories WHERE vector IS NULL LIMIT ?").map_err(|e| e.to_string())?;
            let x = st.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?.flatten().collect();
            x
        };
        if rows.is_empty() {
            return Ok(0);
        }
        let texts: Vec<String> = rows.iter().map(|r| r.1.clone()).collect();
        let Some(vecs) = embed::passages(&texts) else { return Ok(0) };
        for ((id, _), v) in rows.iter().zip(vecs) {
            self.conn.execute("UPDATE memories SET vector=? WHERE id=?", params![to_blob(&v), id]).map_err(|e| e.to_string())?;
        }
        Ok(rows.len())
    }

    /// Index the campaign files, chunked, so anything in them can be recalled. Only changed files
    /// are redone. Chunks of deleted files go too.
    pub fn sync_files(&self, dir: &Path) -> Result<usize, String> {
        let files = campaign::game_files(dir);
        let mut changed = 0;
        for (path, text) in &files {
            let h = hash(text) as i64;
            let old: Option<i64> = self.conn.query_row("SELECT hash FROM file_index WHERE path=?", [path], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
            if old == Some(h) {
                continue;
            }
            let source = format!("file:{path}");
            self.delete_source(&source)?;
            for c in chunks(text) {
                if c.len() > 40 {
                    self.add(&format!("[{path}]\n{c}"), "file", &[], &source, 0.5, -1)?;
                }
            }
            self.conn.execute("INSERT OR REPLACE INTO file_index(path,hash) VALUES(?,?)", params![path, h]).map_err(|e| e.to_string())?;
            changed += 1;
        }
        let known: Vec<String> = {
            let mut st = self.conn.prepare("SELECT path FROM file_index").map_err(|e| e.to_string())?;
            let x = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?.flatten().collect();
            x
        };
        for p in known.iter().filter(|p| !files.contains_key(*p)) {
            self.delete_source(&format!("file:{p}"))?;
            self.conn.execute("DELETE FROM file_index WHERE path=?", [p]).map_err(|e| e.to_string())?;
        }
        Ok(changed)
    }

    /// Keeper memories and turns as JSON lines in the campaign folder. File chunks aren't included:
    /// they can always be rebuilt from the files. Written atomically.
    pub fn export_backup(&self) -> Result<usize, String> {
        let kinds = ["fact", "event", "npc", "place", "promise", "turn"];
        let mut rows = self.list(&kinds, 1_000_000)?;
        rows.reverse();
        let mut out = String::new();
        for m in &rows {
            out.push_str(&serde_json::to_string(m).map_err(|e| e.to_string())?);
            out.push('\n');
        }
        atomic_write(&self.backup, out.as_bytes())?;
        Ok(rows.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ck-test-{}", uuid::Uuid::new_v4()));
        std::env::set_var("CK_DATA_DIR", dir.join("data"));
        std::env::set_var("CK_NO_EMBED", "1");
        let camp = dir.join("camp");
        campaign::create(&camp, "T").unwrap();
        (Store::open(&camp).unwrap(), camp)
    }

    #[test]
    fn finds_by_words_and_respects_skip_and_live() {
        let (s, _) = store();
        let a = s.add("Bram Oakes owns the fishing boat Kittiwake", "npc", &[], "keeper", 0.7, -1).unwrap();
        s.add("The harbor bell rang under the water", "event", &[], "keeper", 0.5, -1).unwrap();
        let t = s.add("PLAYER: ask about the Kittiwake\nGM: Bram grunts.", "turn", &[], "turn:9", 0.3, 9).unwrap();
        let hits = s.search("who owns the Kittiwake?", 5, &[], None).unwrap();
        assert_eq!(hits[0].id, a);
        assert!(hits.iter().any(|h| h.id == t));
        let hits = s.search("Kittiwake", 5, &[a.clone()], Some(5)).unwrap();
        assert!(hits.iter().all(|h| h.id != a && h.id != t));
    }

    #[test]
    fn odd_punctuation_cannot_break_search() {
        let (s, _) = store();
        s.add("A plain fact about ropes", "fact", &[], "k", 0.5, -1).unwrap();
        for q in ["\"", "AND OR NOT", "rope*", "ro'pes:(", "", "()"] {
            s.search(q, 3, &[], None).unwrap();
        }
    }

    #[test]
    fn backup_rebuilds_the_database() {
        let (s, camp) = store();
        s.add("The locket held Mara's hair", "fact", &["mara".into()], "keeper", 0.9, -1).unwrap();
        s.export_backup().unwrap();
        // A new computer: no database, only the folder.
        let id = std::fs::read_to_string(keeper(&camp).join("campaign_id")).unwrap();
        let db = local_dir().join("memory").join(format!("{}.db", id.trim()));
        drop(s);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
        }
        let s = Store::open(&camp).unwrap();
        let hits = s.search("locket", 3, &[], None).unwrap();
        assert_eq!(hits[0].importance, 0.9);
        assert_eq!(hits[0].tags, "mara");
    }

    #[test]
    fn files_are_indexed_once_and_follow_changes() {
        let (s, camp) = store();
        campaign::write(&camp, "03_NPCs/tobin.md", "# Tobin\nThe innkeeper of the Crossed Keys, who hides smuggled silver.").unwrap();
        assert!(s.sync_files(&camp).unwrap() >= 1);
        assert_eq!(s.sync_files(&camp).unwrap(), 0);
        assert!(!s.search("smuggled silver", 3, &[], None).unwrap().is_empty());
        campaign::write(&camp, "03_NPCs/tobin.md", "# Tobin\nThe innkeeper, who now hides stolen rubies in the cellar.").unwrap();
        s.sync_files(&camp).unwrap();
        assert!(s.search("smuggled", 3, &[], None).unwrap().is_empty());
        assert!(!s.search("rubies", 3, &[], None).unwrap().is_empty());
    }

    #[test]
    fn chunks_respect_the_limit_and_keep_everything() {
        let text = format!("# A\n{}\n# B\n{}", "word ".repeat(600), "é".repeat(2000));
        let c = chunks(&text);
        assert!(c.len() > 2);
        assert!(c.iter().all(|x| x.len() <= CHUNK + 2));
        assert_eq!(c.join(" ").matches("word").count(), 600);
    }
}
