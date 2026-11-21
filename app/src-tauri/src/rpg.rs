//! LitRPG rules. Everything here is arithmetic on stored numbers: there is no random number
//! generator anywhere in the game, so the same character in the same situation gets the same
//! result, every time. Models judge what a situation IS (how hard, what's at stake); this module
//! alone decides what HAPPENS, and a model cannot move a number except through `apply_line`,
//! which checks every change against the rules.
//!
//! The sheets live in `.keeper/rpg.json`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::campaign::keeper;
use crate::util::atomic_write;

pub const ATTRIBUTES: [&str; 6] = ["Might", "Agility", "Endurance", "Intellect", "Insight", "Presence"];

/// What the GM is told when the rules came on after the story had already begun (Classic): the
/// sheet's words were never fitted to this world, so it must ask the table before it uses them.
pub const TERMS_ASK: &str = "\
The rules have only now come to a story already in motion, so the sheet's words were never fitted \
to it. Ask the table before you carry the scene on. In one short message, in the story's voice, ask \
what this world calls each of these, and offer a name of your own for each: the six attributes \
(Might, Agility, Endurance, Intellect, Insight, Presence), HP, Stamina, Mana, Money, and the skills \
their character has.
End your reply there, with the scene waiting. When they answer — or leave a word as it is — write \
the `rename` lines in <rpg> (`rename stat HP -> Vigour`, `rename skill Observation -> Reading \
Signs`) and carry on in their words; use them from then on. If they keep the sheet's words as they \
are, write `rename settled` instead. If the table has already answered this in a recent turn, do \
that now and don't ask again.";

pub const BANDS: [(&str, i32); 8] = [
    ("trivial", 6),
    ("easy", 10),
    ("moderate", 13),
    ("hard", 16),
    ("very hard", 19),
    ("extreme", 22),
    ("legendary", 26),
    ("impossible", 99),
];
pub const CIRCUMSTANCE_CAP: i32 = 5;
/// Stats the GM may give a new name.
pub const LABELLED: [&str; 10] = ["Might", "Agility", "Endurance", "Intellect", "Insight", "Presence", "HP", "Stamina", "Mana", "Money"];
const RANK_TITLES: [&str; 7] = ["untrained", "novice", "trained", "expert", "master", "grandmaster", "legend"];
pub const RANK_MAX: i32 = 10;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Skill {
    pub attribute: String,
    pub rank: i32,
    /// Marks of practice toward the next rank.
    pub marks: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Item {
    pub name: String,
    pub qty: i32,
    /// Bonus this gear gives to a skill while carried.
    pub bonuses: BTreeMap<String, i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Condition {
    pub name: String,
    /// Added to every check (usually negative) while it lasts.
    pub modifier: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Pool {
    pub cur: i32,
    pub max: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Sheet {
    pub name: String,
    pub class: String,
    pub level: i32,
    pub xp: i64,
    pub attributes: BTreeMap<String, i32>,
    pub skills: BTreeMap<String, Skill>,
    pub hp: Pool,
    pub stamina: Pool,
    pub mana: Pool,
    pub money: i64,
    pub inventory: Vec<Item>,
    pub conditions: Vec<Condition>,
    pub perks: BTreeMap<String, String>,
    pub attribute_points: i32,
    pub skill_points: i32,
}

impl Default for Sheet {
    fn default() -> Self {
        Sheet {
            name: String::new(),
            class: String::new(),
            level: 1,
            xp: 0,
            attributes: ATTRIBUTES.iter().map(|a| (a.to_string(), 10)).collect(),
            skills: BTreeMap::new(),
            hp: Pool { cur: 20, max: 20 },
            stamina: Pool { cur: 10, max: 10 },
            mana: Pool::default(),
            money: 0,
            inventory: Vec::new(),
            conditions: Vec::new(),
            perks: BTreeMap::new(),
            attribute_points: 0,
            skill_points: 0,
        }
    }
}

/// Total XP needed to reach `level`: 100, 300, 600, 1000...
pub fn xp_for_level(level: i32) -> i64 {
    let l = (level - 1).max(0) as i64;
    50 * l * (l + 1)
}

pub fn marks_for_rank(rank: i32) -> i32 {
    (rank + 1) * 3
}

pub fn rank_title(rank: i32) -> &'static str {
    match rank {
        i32::MIN..=0 => "Untrained",
        1 => "Novice",
        2 | 3 => "Trained",
        4 | 5 => "Expert",
        6 | 7 => "Master",
        8 | 9 => "Grandmaster",
        _ => "Legend",
    }
}

/// A skill's name in lower case, without rank words or punctuation around them:
/// "Varrow Streets (Expert, rank 4)" and "varrow streets expert untrained" are both "varrow streets".
pub fn base_skill_name(name: &str) -> String {
    let low = name.to_lowercase();
    let mut words: Vec<&str> = low
        .split(|c: char| c.is_whitespace() || "()[],:;".contains(c))
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .collect();
    while words.len() > 1 {
        let last = words[words.len() - 1];
        if last == "rank" || last.parse::<i32>().is_ok() || RANK_TITLES.contains(&last) {
            words.pop();
        } else {
            break;
        }
    }
    words.join(" ")
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl Sheet {
    pub fn attribute(&self, name: &str) -> i32 {
        self.attributes.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or(10, |(_, v)| *v)
    }

    /// Which skill on the sheet a name means. A rank is never part of a skill's name, however a
    /// model words it, and a near match beats a rank 0 copy of something they've mastered:
    ///   exact > same name once rank words are gone > the only skill sharing all its words.
    fn skill_key(&self, name: &str) -> Option<String> {
        let name = name.trim();
        if let Some(k) = self.skills.keys().find(|k| k.eq_ignore_ascii_case(name)) {
            return Some(k.clone());
        }
        let base = base_skill_name(name);
        if let Some(k) = self.skills.keys().find(|k| base_skill_name(k) == base) {
            return Some(k.clone());
        }
        let words: Vec<&str> = base.split(' ').collect();
        let mut close = self.skills.keys().filter(|k| {
            let theirs = base_skill_name(k);
            let theirs: Vec<&str> = theirs.split(' ').collect();
            words.iter().all(|w| theirs.contains(w)) || theirs.iter().all(|w| words.contains(w))
        });
        match (close.next(), close.next()) {
            (Some(k), None) => Some(k.clone()),
            _ => None,
        }
    }

    /// The best single bonus from carried gear; gear doesn't stack.
    pub fn gear_bonus(&self, skill: &str) -> (i32, Option<String>) {
        self.inventory
            .iter()
            .filter(|i| i.qty > 0)
            .filter_map(|i| i.bonuses.iter().find(|(k, _)| k.eq_ignore_ascii_case(skill)).map(|(_, v)| (*v, Some(i.name.clone()))))
            .max_by_key(|(v, _)| *v)
            .unwrap_or((0, None))
    }

    pub fn condition_total(&self) -> i32 {
        self.conditions.iter().map(|c| c.modifier).sum()
    }

    /// Below a quarter of their health a character fights on at -2.
    pub fn wound_modifier(&self) -> i32 {
        if self.hp.max > 0 && self.hp.cur * 4 < self.hp.max {
            -2
        } else {
            0
        }
    }

    pub fn inventory_count(&self, name: &str) -> i32 {
        self.inventory.iter().find(|i| norm(&i.name) == norm(name)).map_or(0, |i| i.qty)
    }

    /// Level up as far as the XP goes. Fixed rewards; points are spent by the player.
    fn settle_level(&mut self, log: &mut Vec<String>) {
        while self.xp >= xp_for_level(self.level + 1) {
            self.level += 1;
            self.attribute_points += 1;
            self.skill_points += 2;
            let gain = 2 + (self.attribute("Endurance") - 10).max(0) / 2;
            self.hp.max += gain;
            self.hp.cur += gain;
            self.stamina.max += 1;
            self.stamina.cur += 1;
            log.push(format!("LEVEL UP! {} is now level {}. +1 attribute point, +2 skill points, +{gain} max HP.", self.name, self.level));
        }
    }

    pub fn gain_xp(&mut self, amount: i64, log: &mut Vec<String>) {
        if amount > 0 {
            self.xp += amount;
            self.settle_level(log);
        }
    }

    pub fn spend_attribute(&mut self, attribute: &str) -> Result<(), String> {
        if self.attribute_points < 1 {
            return Err("No attribute points to spend.".into());
        }
        let key = self.attributes.keys().find(|k| k.eq_ignore_ascii_case(attribute)).cloned().ok_or("No such attribute.")?;
        *self.attributes.entry(key).or_insert(10) += 1;
        self.attribute_points -= 1;
        Ok(())
    }

    pub fn spend_skill(&mut self, skill: &str) -> Result<(), String> {
        if self.skill_points < 1 {
            return Err("No skill points to spend.".into());
        }
        let key = self.skill_key(skill).ok_or("That skill isn't on the sheet.")?;
        let s = self.skills.get_mut(&key).ok_or("That skill isn't on the sheet.")?;
        if s.rank >= RANK_MAX {
            return Err("That skill is already at the top rank.".into());
        }
        s.rank += 1;
        s.marks = 0;
        self.skill_points -= 1;
        Ok(())
    }

    fn add_marks(&mut self, skill: &str, marks: i32, log: &mut Vec<String>) {
        let name = self.name.clone();
        let Some(key) = self.skill_key(skill) else { return };
        let Some(s) = self.skills.get_mut(&key) else { return };
        s.marks += marks;
        while s.marks >= marks_for_rank(s.rank) && s.rank < RANK_MAX {
            s.marks -= marks_for_rank(s.rank);
            s.rank += 1;
            log.push(format!("SKILL UP! {name}'s {key} reached rank {} ({}).", s.rank, rank_title(s.rank)));
        }
    }
}

/// What a referee says about one attempt. Judgment only; no outcome in here.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct CheckRequest {
    pub character: String,
    pub skill: String,
    /// The attribute, when this skill isn't on the sheet yet.
    pub attribute: String,
    /// What is being attempted against: "the cellar door", "Dunn Hale". Same subject and same
    /// skill means the same difficulty, so a failed check can't simply be asked again.
    pub subject: String,
    /// A difficulty band from BANDS.
    pub difficulty: String,
    /// For an attempt against someone: their name, and their fixed score once it's known.
    pub opponent: String,
    pub opponent_score: i32,
    pub circumstances: Vec<Circumstance>,
    pub stakes: Stakes,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Circumstance {
    pub label: String,
    pub value: i32,
}

/// What failure costs. The engine applies it, scaled by how badly the check failed.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Stakes {
    pub hp: i32,
    pub stamina: i32,
    pub condition: String,
    pub condition_modifier: i32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Decisive,
    Success,
    Cost,
    Failure,
    Disaster,
}

impl Tier {
    pub fn from_margin(m: i32) -> Tier {
        match m {
            5.. => Tier::Decisive,
            0..=4 => Tier::Success,
            -2..=-1 => Tier::Cost,
            -6..=-3 => Tier::Failure,
            _ => Tier::Disaster,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tier::Decisive => "Decisive success",
            Tier::Success => "Success",
            Tier::Cost => "Success at a cost",
            Tier::Failure => "Failure",
            Tier::Disaster => "Disaster",
        }
    }
    pub fn succeeded(self) -> bool {
        matches!(self, Tier::Decisive | Tier::Success | Tier::Cost)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CheckResult {
    pub character: String,
    pub skill: String,
    pub subject: String,
    pub rank: i32,
    pub attribute: String,
    pub attribute_value: i32,
    pub gear: i32,
    pub gear_item: Option<String>,
    pub conditions: i32,
    pub circumstances: Vec<Circumstance>,
    pub circumstance_total: i32,
    pub score: i32,
    pub target: i32,
    pub target_label: String,
    pub margin: i32,
    pub tier: Tier,
    pub reused: bool,
    pub effects: Vec<String>,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Record {
    pub character: String,
    pub skill: String,
    pub subject: String,
    pub target: i32,
    pub target_label: String,
    pub circumstance_total: i32,
    pub score: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Rules {
    pub enabled: bool,
    pub sheets: BTreeMap<String, Sheet>,
    /// NPCs' fixed scores, set the first time they oppose someone: "name|skill" -> score.
    pub rivals: BTreeMap<String, i32>,
    /// Every situation already judged, so the same attempt gets the same answer.
    pub history: Vec<Record>,
    /// Lines of the system log, newest last.
    pub log: Vec<String>,
    /// What this story calls the stats: "Might" -> "Grit". The rules keep the fixed names inside,
    /// so a rename can't touch a single number.
    pub labels: BTreeMap<String, String>,
    /// Which rules: "" is Classic (this file), "ad" is the AD System (`ad.rs`, Spirit's rules).
    pub ruleset: String,
    pub ad: crate::ad::World,
    /// The System came on after the story had begun: it still has to ask the table what this world
    /// calls its words. Cleared when it renames something, or when the host settles the words.
    pub terms_pending: bool,
}

/// A line meant for every character at once: `all: xp +40`, `everyone: money +10`.
pub fn party_line(line: &str) -> Option<&str> {
    let t = line.trim().trim_start_matches('-').trim();
    ["all", "everyone", "party", "the party"].iter().find_map(|p| {
        let head = t.get(..p.len())?;
        (head.eq_ignore_ascii_case(p) && t.get(p.len()..p.len() + 1) == Some(":")).then(|| t.get(p.len() + 1..).map(str::trim).unwrap_or_default())
    })
}

/// A line that changes the System itself and belongs to no character: `define …`, `rename …`, and
/// the crafting sums (`lens`, `crystal`, `impurity`, `battery`). Such a line may stand alone, and a
/// character's name in front of it changes nothing — the GM names its lines.
pub fn world_line(line: &str) -> bool {
    let t = line.trim().trim_start_matches('-').trim();
    let verb = t.split_once(char::is_whitespace).map_or(t, |(v, _)| v).to_lowercase();
    ["define", "rename", "lens", "crystal", "impurity", "battery"].contains(&verb.as_str())
}

impl Rules {
    pub fn is_ad(&self) -> bool {
        self.ruleset == "ad"
    }

    pub fn has_sheets(&self) -> bool {
        if self.is_ad() { !self.ad.sheets.is_empty() } else { !self.sheets.is_empty() }
    }

    pub fn names(&self) -> Vec<String> {
        if self.is_ad() { self.ad.sheets.keys().cloned().collect() } else { self.sheets.keys().cloned().collect() }
    }

    /// Every status window, as the GM reads it.
    pub fn status_all(&self) -> String {
        if self.is_ad() {
            let t = self.ad.sheets.values().map(|x| crate::ad::status_text(x, &self.ad.defs)).collect::<Vec<_>>().join("\n\n");
            let names = self.ad.labels.iter().map(|(k, v)| format!("{k} -> {v}")).collect::<Vec<_>>().join(", ");
            let t = crate::ad::relabel(&t, &self.ad.labels);
            if names.is_empty() { t } else { format!("{t}\n(System only) This story's names: {names}") }
        } else {
            self.sheets.values().map(|x| status_text(x, &self.labels)).collect::<Vec<_>>().join("\n\n")
        }
    }

    /// One `<rpg>` line, under whichever rules this game uses.
    pub fn apply(&mut self, line: &str, default_char: &str) -> Result<String, String> {
        match party_line(line) {
            Some(rest) => self.apply_party(rest),
            None => self.apply_one(line, default_char),
        }
    }

    /// `all: xp +40`: the same change for every character, one sheet at a time.
    fn apply_party(&mut self, rest: &str) -> Result<String, String> {
        if rest.is_empty() {
            return Err("all: needs a change after it (all: xp +40)".into());
        }
        if rest.get(..6).is_some_and(|w| w.eq_ignore_ascii_case("rename")) {
            return Err("a rename is the story's, not one character's".into());
        }
        let names = self.names();
        if names.is_empty() {
            return Err("no character sheets yet".into());
        }
        let (mut done, mut refused) = (Vec::new(), Vec::new());
        for n in names {
            match self.apply_one(rest, &n) {
                // For the System alone (item 73): never shown to the player, whoever it was.
                Ok(m) if m.starts_with(crate::ad::SYSTEM_ONLY) => done.push(m),
                Ok(m) if m.starts_with(&format!("{n}:")) => done.push(m),
                Ok(m) => done.push(format!("{n}: {m}")),
                Err(e) => refused.push(format!("{n}: {e}")),
            }
        }
        if refused.is_empty() {
            return Ok(done.join(" · "));
        }
        if done.is_empty() {
            return Err(refused.join(" · "));
        }
        refused.push("the rest took it".into());
        Err(refused.join(" · "))
    }

    /// Does a line say whose change it is ("Aria: xp +40")?
    pub fn names_character(&self, line: &str) -> bool {
        let line = line.trim().trim_start_matches('-').trim();
        match line.split_once(':') {
            Some((a, _)) if !a.trim().is_empty() => {
                if self.is_ad() {
                    self.ad.sheet_key(a).is_some()
                } else {
                    self.sheet_key(a).is_some()
                }
            }
            _ => false,
        }
    }

    /// What to tell a GM whose line should have said whose change it is.
    pub(crate) fn say_whose(&self) -> String {
        let mut names = self.names();
        names.sort();
        format!("say whose it is: {} (or all: for everyone)", names.join(", "))
    }

    /// One `<rpg>` line, for one character.
    fn apply_one(&mut self, line: &str, default_char: &str) -> Result<String, String> {
        // A name in front of a world line changes nothing: `Aria: rename …` is `rename …`.
        let t = line.trim_start_matches('-').trim();
        let bare = if self.names_character(t) { t.split_once(':').map_or(t, |(_, b)| b.trim()) } else { t };
        let low = bare.to_ascii_lowercase();
        // The table kept the sheet's words: nothing changes, and the asking ends.
        if matches!(low.as_str(), "rename settled" | "words settled" | "settle words") {
            self.terms_pending = false;
            self.log.push("the words are settled".into());
            return Ok("the words are settled".into());
        }
        let renaming = low.starts_with("rename");
        let r = if self.is_ad() {
            let r = self.ad.apply_line(line, default_char).map(|m| crate::ad::relabel(&m, &self.ad.labels));
            if let Ok(m) = &r {
                self.log.push(m.trim_start_matches(crate::ad::SYSTEM_ONLY).to_string());
            }
            r
        } else {
            apply_line(self, line, default_char)
        };
        // It has fitted its words to the world (`rename settled` never reaches here).
        if renaming && r.is_ok() {
            self.terms_pending = false;
        }
        r
    }

    /// A character joining the table gets a blank sheet under the rules in use.
    pub fn ensure_sheet(&mut self, name: &str) {
        if self.is_ad() {
            if self.ad.sheet_key(name).is_none() {
                let base = crate::ad::STATS.iter().map(|s| (s.to_string(), 10)).collect();
                let s = crate::ad::Sheet::new(name, "human", base, &self.ad.defs);
                self.ad.sheets.insert(name.to_string(), s);
            }
        } else if !self.sheets.keys().any(|k| k.eq_ignore_ascii_case(name)) {
            self.sheets.insert(name.to_string(), Sheet { name: name.to_string(), ..Sheet::default() });
        }
    }

    /// One character's sheet for the status panel.
    pub fn view(&self, character: &str) -> Option<Value> {
        if self.is_ad() {
            let k = self.ad.sheet_key(character)?;
            let mut v = crate::ad::sheet_view(&self.ad.sheets[&k], &self.ad.defs);
            crate::ad::relabel_value(&mut v, &self.ad.labels);
            // The derived stats are the System's words too: amplitude -> Aether.
            if let Some(d) = v.get_mut("derived").and_then(Value::as_object_mut) {
                let named: serde_json::Map<String, Value> =
                    d.iter().map(|(n, x)| (crate::ad::relabel(n, &self.ad.labels), x.clone())).collect();
                *d = named;
            }
            // The panel names its own headings and stats by these (lowercase keys).
            v["labels"] = serde_json::json!(crate::ad::panel_labels(&self.ad.labels));
            Some(v)
        } else {
            self.sheets.iter().find(|(k, _)| k.eq_ignore_ascii_case(character)).and_then(|(_, v)| serde_json::to_value(v).ok())
        }
    }
}

pub fn load(dir: &Path) -> Rules {
    let mut r: Rules = std::fs::read_to_string(keeper(dir).join("rpg.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    r.tidy();
    r
}

pub fn save(dir: &Path, r: &Rules) -> Result<(), String> {
    atomic_write(&keeper(dir).join("rpg.json"), serde_json::to_string_pretty(r).unwrap_or_default().as_bytes())
}

fn band_target(name: &str) -> Option<(String, i32)> {
    let n = norm(name).replace(['-', '_'], " ");
    BANDS.iter().find(|(b, _)| *b == n).map(|(b, t)| (b.to_string(), *t))
}

impl Rules {
    /// Repair sheets from before skill names were matched properly: a copy with a rank in its name
    /// folds back into the real skill, keeping its practice, and past rulings follow it.
    pub fn tidy(&mut self) {
        let mut renamed: Vec<(String, String)> = Vec::new();
        for sheet in self.sheets.values_mut() {
            let names: Vec<String> = sheet.skills.keys().cloned().collect();
            for copy in names.iter().filter(|n| **n != base_skill_name(n)) {
                let base = base_skill_name(copy);
                let Some(real) = names.iter().find(|n| *n != copy && n.to_lowercase() == base).cloned() else { continue };
                let Some(c) = sheet.skills.remove(copy) else { continue };
                if let Some(r) = sheet.skills.get_mut(&real) {
                    r.rank = r.rank.max(c.rank);
                    r.marks += c.marks;
                    while r.marks >= marks_for_rank(r.rank) && r.rank < RANK_MAX {
                        r.marks -= marks_for_rank(r.rank);
                        r.rank += 1;
                    }
                }
                renamed.push((copy.clone(), real));
            }
        }
        for (copy, real) in renamed {
            for h in self.history.iter_mut().filter(|h| h.skill == copy) {
                h.skill = real.clone();
            }
            let suffix = format!("|{}", norm(&copy));
            let keys: Vec<String> = self.rivals.keys().filter(|k| k.ends_with(&suffix)).cloned().collect();
            for k in keys {
                if let Some(v) = self.rivals.remove(&k) {
                    let new = format!("{}|{}", k.trim_end_matches(&suffix), norm(&real));
                    self.rivals.entry(new).or_insert(v);
                }
            }
        }
    }

    pub fn label<'a>(&'a self, name: &'a str) -> &'a str {
        self.labels.get(name).map_or(name, String::as_str)
    }

    /// Give something a name that fits the story. Only the name changes: ranks, marks, values,
    /// and every past ruling carry over.
    fn rename(&mut self, key: &str, what: &str) -> Result<String, String> {
        let (kind, rest) = what.split_once(char::is_whitespace).ok_or("rename needs: skill Old -> New")?;
        let (old, new) = rest.split_once("->").ok_or("rename needs: Old -> New")?;
        let (old, new) = (old.trim(), clean_label(new, 40)?);
        match kind.to_lowercase().as_str() {
            "skill" => {
                let s = self.sheets.get_mut(key).ok_or("no sheet")?;
                let from = s.skill_key(old).ok_or_else(|| format!("{key} has no skill called {old}"))?;
                // A rank is never part of a name, or the same trouble comes back.
                if base_skill_name(&new) != new.to_lowercase() {
                    return Err(format!("'{new}' has a rank in its name"));
                }
                if s.skills.keys().any(|k| k != &from && k.eq_ignore_ascii_case(&new)) {
                    return Err(format!("{key} already has a skill called {new}"));
                }
                let skill = s.skills.remove(&from).ok_or("no such skill")?;
                s.skills.insert(new.clone(), skill);
                for item in s.inventory.iter_mut() {
                    if let Some(b) = item.bonuses.remove(&from) {
                        item.bonuses.insert(new.clone(), b);
                    }
                }
                for h in self.history.iter_mut().filter(|h| h.skill == from) {
                    h.skill = new.clone();
                }
                let suffix = format!("|{}", norm(&from));
                let keys: Vec<String> = self.rivals.keys().filter(|k| k.ends_with(&suffix)).cloned().collect();
                for k in keys {
                    if let Some(v) = self.rivals.remove(&k) {
                        self.rivals.insert(format!("{}|{}", k.trim_end_matches(&suffix), norm(&new)), v);
                    }
                }
                Ok(format!("{key}: {from} is now {new}"))
            }
            "perk" => {
                let s = self.sheets.get_mut(key).ok_or("no sheet")?;
                let from = s.perks.keys().find(|k| k.eq_ignore_ascii_case(old)).cloned().ok_or_else(|| format!("{key} has no perk called {old}"))?;
                if s.perks.keys().any(|k| k != &from && k.eq_ignore_ascii_case(&new)) {
                    return Err(format!("{key} already has a perk called {new}"));
                }
                let d = s.perks.remove(&from).unwrap_or_default();
                s.perks.insert(new.clone(), d);
                Ok(format!("{key}: perk {from} is now {new}"))
            }
            "stat" | "attribute" => {
                let canon = LABELLED
                    .iter()
                    .find(|a| a.eq_ignore_ascii_case(old) || self.labels.get(**a).is_some_and(|l| l.eq_ignore_ascii_case(old)))
                    .ok_or_else(|| format!("no stat called {old}"))?;
                let clash = LABELLED.iter().any(|a| a != canon && (a.eq_ignore_ascii_case(&new) || self.labels.get(*a).is_some_and(|l| l.eq_ignore_ascii_case(&new))));
                if clash {
                    return Err(format!("another stat is already called {new}"));
                }
                self.labels.insert(canon.to_string(), new.clone());
                Ok(format!("{old} is now {new}"))
            }
            other => Err(format!("can't rename a {other}")),
        }
    }

    fn sheet_key(&self, character: &str) -> Option<String> {
        if let Some(k) = self.sheets.keys().find(|k| norm(k) == norm(character)) {
            return Some(k.clone());
        }
        // With one character at the table, an unnamed check is theirs.
        (self.sheets.len() == 1 && character.trim().is_empty()).then(|| self.sheets.keys().next().cloned()).flatten()
    }

    /// Resolve one check. Pure arithmetic: same sheet and same request give the same result.
    pub fn resolve(&mut self, req: &CheckRequest) -> Result<CheckResult, String> {
        let key = self.sheet_key(&req.character).ok_or_else(|| format!("no sheet for {}", req.character))?;
        let sheet = self.sheets.get(&key).ok_or("no sheet")?;
        if sheet.hp.max > 0 && sheet.hp.cur <= 0 {
            return Err(format!("{key} is down and can't act"));
        }
        let skill_name = sheet.skill_key(&req.skill).unwrap_or_else(|| req.skill.trim().to_string());
        if skill_name.is_empty() {
            return Err("a check needs a skill".into());
        }
        let known = sheet.skills.get(&skill_name);
        let attribute = known.map(|s| s.attribute.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| canon_attribute(&req.attribute, &self.labels));
        let rank = known.map_or(0, |s| s.rank);
        let attribute_value = sheet.attribute(&attribute);
        let (gear, gear_item) = sheet.gear_bonus(&skill_name);
        let conditions = sheet.condition_total() + sheet.wound_modifier();

        // The same attempt on the same subject keeps its judged difficulty and circumstances, so
        // asking twice can't fish for a better ruling. A better SCORE (training, gear) still counts.
        let subject = norm(&req.subject);
        let earlier = if subject.is_empty() {
            None
        } else {
            self.history.iter().rev().find(|h| norm(&h.character) == norm(&key) && norm(&h.skill) == norm(&skill_name) && norm(&h.subject) == subject)
        };
        let circumstances: Vec<Circumstance> =
            req.circumstances.iter().map(|c| Circumstance { label: c.label.clone(), value: c.value.clamp(-3, 3) }).collect();
        let judged: i32 = circumstances.iter().map(|c| c.value).sum::<i32>().clamp(-CIRCUMSTANCE_CAP, CIRCUMSTANCE_CAP);
        let (target, target_label, circumstance_total, reused) = match earlier {
            Some(h) => (h.target, h.target_label.clone(), h.circumstance_total, true),
            None => {
                let (t, l) = self.judge_target(req, &skill_name)?;
                (t, l, judged, false)
            }
        };

        let score = attribute_value + rank * 2 + gear + conditions + circumstance_total;
        let margin = score - target;
        let tier = if target >= 99 { Tier::Disaster } else { Tier::from_margin(margin) };
        let mut effects = Vec::new();
        self.apply_outcome(&key, &skill_name, &attribute, target, tier, &req.stakes, reused, &mut effects);
        if !reused {
            self.history.push(Record {
                character: key.clone(),
                skill: skill_name.clone(),
                subject: req.subject.clone(),
                target,
                target_label: target_label.clone(),
                circumstance_total,
                score,
            });
        }
        let r = CheckResult {
            character: key,
            skill: skill_name,
            subject: req.subject.clone(),
            rank,
            attribute,
            attribute_value,
            gear,
            gear_item,
            conditions,
            circumstances: if reused { Vec::new() } else { circumstances },
            circumstance_total,
            score,
            target,
            target_label,
            margin,
            tier,
            reused,
            effects,
            reason: req.reason.clone(),
        };
        self.log.push(format!("[{}] {} {} vs {} ({}): {}", r.character, r.skill, r.score, r.target, r.target_label, r.tier.label()));
        Ok(r)
    }

    fn judge_target(&mut self, req: &CheckRequest, skill: &str) -> Result<(i32, String), String> {
        if !req.opponent.trim().is_empty() {
            let k = format!("{}|{}", norm(&req.opponent), norm(skill));
            let score = match self.rivals.get(&k) {
                Some(s) => *s,
                None => {
                    let s = if req.opponent_score > 0 { req.opponent_score.clamp(4, 40) } else { band_target(&req.difficulty).map_or(13, |b| b.1) };
                    self.rivals.insert(k, s);
                    s
                }
            };
            return Ok((score, format!("{}'s {score}", req.opponent.trim())));
        }
        band_target(&req.difficulty).map(|(l, t)| (t, l)).ok_or_else(|| format!("unknown difficulty '{}'", req.difficulty))
    }

    /// Fixed consequences: full stakes on a Failure, half (rounded up) on a Cost, one and a half
    /// on a Disaster, nothing on a success. Practice marks and XP depend only on the difficulty,
    /// and only the first attempt at something teaches: repeating it can't farm levels, though a
    /// failure still costs what it costs.
    #[allow(clippy::too_many_arguments)]
    fn apply_outcome(&mut self, key: &str, skill: &str, attribute: &str, target: i32, tier: Tier, stakes: &Stakes, repeat: bool, out: &mut Vec<String>) {
        let Some(s) = self.sheets.get_mut(key) else { return };
        let scale = |n: i32| match tier {
            Tier::Cost => (n + 1) / 2,
            Tier::Failure => n,
            Tier::Disaster => n + (n + 1) / 2,
            _ => 0,
        };
        let (hp, st) = (scale(stakes.hp.max(0)), scale(stakes.stamina.max(0)));
        if hp > 0 {
            s.hp.cur = (s.hp.cur - hp).max(0);
            out.push(format!("-{hp} HP ({}/{})", s.hp.cur, s.hp.max));
        }
        if st > 0 {
            s.stamina.cur = (s.stamina.cur - st).max(0);
            out.push(format!("-{st} Stamina ({}/{})", s.stamina.cur, s.stamina.max));
        }
        if !tier.succeeded() && !stakes.condition.trim().is_empty() && !s.conditions.iter().any(|c| norm(&c.name) == norm(&stakes.condition)) {
            s.conditions.push(Condition { name: stakes.condition.trim().into(), modifier: stakes.condition_modifier.clamp(-5, 0) });
            out.push(format!("Condition: {}", stakes.condition.trim()));
        }
        // Using a skill for the first time puts it on the sheet at rank 0.
        if s.skill_key(skill).is_none() {
            s.skills.insert(skill.to_string(), Skill { attribute: attribute.to_string(), rank: 0, marks: 0 });
            out.push(format!("New skill: {skill}"));
        }
        let rank = s.skills.get(skill).map_or(0, |x| x.rank);
        // Only a real challenge teaches: nothing from what's far below you, or hopeless attempts.
        let base = s.attribute(attribute) + rank * 2;
        if !repeat && target >= base - 5 && target < 99 {
            let marks = match tier {
                Tier::Decisive | Tier::Success => 2,
                Tier::Cost | Tier::Failure => 1,
                Tier::Disaster => 0,
            };
            s.add_marks(skill, marks, out);
            let xp = if tier.succeeded() { (target as i64) * 2 } else { target as i64 / 2 };
            out.push(format!("+{xp} XP"));
            s.gain_xp(xp, out);
        }
    }
}

fn canon_attribute(s: &str, labels: &BTreeMap<String, String>) -> String {
    let s = s.trim();
    ATTRIBUTES
        .iter()
        .find(|a| a.eq_ignore_ascii_case(s) || labels.get(**a).is_some_and(|l| l.eq_ignore_ascii_case(s)))
        .map_or_else(|| "Intellect".to_string(), |a| a.to_string())
}

fn clean_label(s: &str, max: usize) -> Result<String, String> {
    let t: String = s.chars().filter(|c| c.is_alphanumeric() || " -'/&.".contains(*c)).take(max).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() { Err("that name is empty".into()) } else { Ok(t) }
}

/// One change the GM asked for at the end of a reply, in `<rpg>` lines.
pub fn apply_line(r: &mut Rules, line: &str, default_char: &str) -> Result<String, String> {
    let line = line.trim().trim_start_matches('-').trim();
    // Optional "Name: " prefix picks a character.
    let (who, cmd) = match line.split_once(':') {
        Some((a, b)) if r.sheet_key(a).is_some() && !a.trim().is_empty() => (a.trim().to_string(), b.trim()),
        _ => (default_char.to_string(), line),
    };
    let key = r.sheet_key(&who).ok_or_else(|| if who.trim().is_empty() { r.say_whose() } else { format!("no sheet for '{who}'") })?;
    let level = r.sheets.get(&key).map_or(1, |s| s.level) as i64;
    let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
    let verb = verb.to_lowercase();
    let rest = rest.trim();
    if verb == "rename" {
        let msg = r.rename(&key, rest)?;
        r.log.push(msg.clone());
        return Ok(msg);
    }
    let labels = r.labels.clone();
    let s = r.sheets.get_mut(&key).ok_or("no sheet")?;
    let number = |t: &str| t.split_whitespace().next().and_then(|n| n.trim_start_matches('+').parse::<i64>().ok());
    let mut extra = Vec::new();
    let msg = match verb.as_str() {
        "xp" => {
            let n = number(rest).ok_or("xp needs a number")?;
            if n < 0 {
                return Err("XP can't be taken away".into());
            }
            // A cap per reply, so one story beat can't hand out levels.
            let n = n.min(50 * level);
            s.gain_xp(n, &mut extra);
            format!("+{n} XP")
        }
        "hp" | "stamina" | "mana" => {
            let n = number(rest).ok_or("needs a number")? as i32;
            let p = match verb.as_str() {
                "hp" => &mut s.hp,
                "stamina" => &mut s.stamina,
                _ => &mut s.mana,
            };
            p.cur = (p.cur + n).clamp(0, p.max);
            format!("{n:+} {verb} ({}/{})", p.cur, p.max)
        }
        "money" | "gold" => {
            let n = number(rest).ok_or("money needs a number")?;
            if s.money + n < 0 {
                return Err(format!("{key} has only {}: can't spend {}", s.money, -n));
            }
            s.money += n;
            format!("{n:+} money ({})", s.money)
        }
        "item" => {
            let (sign, name) = rest.split_at(rest.chars().next().map_or(0, char::len_utf8));
            let (name, qty) = match name.trim().rsplit_once(" x") {
                Some((a, q)) if q.parse::<i32>().is_ok() => (a.trim().to_string(), q.parse::<i32>().unwrap_or(1)),
                _ => (name.trim().to_string(), 1),
            };
            match sign {
                "+" => {
                    match s.inventory.iter_mut().find(|i| norm(&i.name) == norm(&name)) {
                        Some(i) => i.qty += qty,
                        None => s.inventory.push(Item { name: name.clone(), qty, bonuses: BTreeMap::new() }),
                    }
                    format!("Item: {name} x{qty}")
                }
                "-" => {
                    let i = s.inventory.iter_mut().find(|i| norm(&i.name) == norm(&name) && i.qty >= qty);
                    let i = i.ok_or_else(|| format!("{key} doesn't have {qty} {name}"))?;
                    i.qty -= qty;
                    s.inventory.retain(|i| i.qty > 0);
                    format!("Lost: {name} x{qty}")
                }
                _ => return Err("item needs + or -".into()),
            }
        }
        "gear" => {
            // gear Hand lens | Observation | +2
            let parts: Vec<&str> = rest.split('|').map(str::trim).collect();
            let [name, skill, bonus] = parts[..] else { return Err("gear needs: item | skill | +n".into()) };
            let n = bonus.trim_start_matches('+').parse::<i32>().map_err(|_| "gear needs a number")?.clamp(-3, 5);
            let i = s.inventory.iter_mut().find(|i| norm(&i.name) == norm(name)).ok_or("gear must be an item they carry")?;
            i.bonuses.insert(skill.to_string(), n);
            format!("{name}: {n:+} {skill}")
        }
        "condition" => {
            let (sign, rest2) = rest.split_at(rest.chars().next().map_or(0, char::len_utf8));
            match sign {
                "+" => {
                    let words: Vec<&str> = rest2.split_whitespace().collect();
                    let (name, m) = match words.last().and_then(|w| w.parse::<i32>().ok()) {
                        Some(m) => (words[..words.len() - 1].join(" "), m.clamp(-5, 2)),
                        None => (words.join(" "), -1),
                    };
                    if !s.conditions.iter().any(|c| norm(&c.name) == norm(&name)) {
                        s.conditions.push(Condition { name: name.clone(), modifier: m });
                    }
                    format!("Condition: {name} ({m:+})")
                }
                "-" => {
                    let name = rest2.trim();
                    s.conditions.retain(|c| norm(&c.name) != norm(name));
                    format!("Cleared: {name}")
                }
                _ => return Err("condition needs + or -".into()),
            }
        }
        "skill" => {
            // skill + Herbalism Insight: a new skill, always at rank 0. Ranks are earned.
            let t = rest.trim_start_matches('+').trim();
            let (name, attr) = t.rsplit_once(char::is_whitespace).ok_or("skill needs: name attribute")?;
            if s.skill_key(name).is_none() {
                s.skills.insert(name.trim().into(), Skill { attribute: canon_attribute(attr, &labels), rank: 0, marks: 0 });
            }
            format!("New skill: {}", name.trim())
        }
        "class" => {
            s.class = clean_label(rest, 60)?;
            format!("Class: {}", s.class)
        }
        "perk" => {
            let t = rest.trim_start_matches('+').trim();
            let (name, what) = t.split_once(':').ok_or("perk needs: name: what it does")?;
            s.perks.insert(name.trim().into(), what.trim().into());
            format!("Perk: {}", name.trim())
        }
        other => return Err(format!("unknown change '{other}'")),
    };
    let mut parts = vec![format!("{key}: {msg}")];
    parts.extend(extra);
    let joined = parts.join(" · ");
    r.log.push(joined.clone());
    Ok(joined)
}

/// A sheet from a model's JSON. Everything is clamped, and no model can grant points to spend.
pub fn sheet_from_json(v: &Value) -> Option<Sheet> {
    let name = v["name"].as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let mut s = Sheet { name, class: v["class"].as_str().unwrap_or("").trim().to_string(), ..Sheet::default() };
    for a in ATTRIBUTES {
        if let Some(n) = v["attributes"][a].as_i64() {
            s.attributes.insert(a.to_string(), (n as i32).clamp(6, 16));
        }
    }
    if let Some(m) = v["skills"].as_object() {
        for (k, sk) in m.iter().take(20) {
            let attribute = canon_attribute(sk["attribute"].as_str().unwrap_or(""), &BTreeMap::new());
            let rank = (sk["rank"].as_i64().unwrap_or(0) as i32).clamp(0, 6);
            s.skills.insert(k.trim().to_string(), Skill { attribute, rank, marks: 0 });
        }
    }
    let end = s.attribute("Endurance");
    s.hp = Pool { cur: 10 + end, max: 10 + end };
    s.stamina = Pool { cur: end, max: end };
    let mana = (v["mana"].as_i64().unwrap_or(0) as i32).clamp(0, 30);
    s.mana = Pool { cur: mana, max: mana };
    s.money = v["money"].as_i64().unwrap_or(0).max(0);
    for it in v["inventory"].as_array().into_iter().flatten().take(40) {
        if let Some(name) = it["name"].as_str().or(it.as_str()) {
            let bonuses = it["bonuses"]
                .as_object()
                .map(|m| m.iter().filter_map(|(k, b)| Some((k.clone(), (b.as_i64()? as i32).clamp(-3, 3)))).collect())
                .unwrap_or_default();
            s.inventory.push(Item { name: name.trim().into(), qty: (it["qty"].as_i64().unwrap_or(1) as i32).max(1), bonuses });
        }
    }
    for c in v["conditions"].as_array().into_iter().flatten().take(10) {
        if let Some(name) = c["name"].as_str() {
            s.conditions.push(Condition { name: name.trim().into(), modifier: (c["modifier"].as_i64().unwrap_or(-1) as i32).clamp(-5, 2) });
        }
    }
    Some(s)
}

/// The status window as text, in the story's own names: what the GM sees each turn.
pub fn status_text(s: &Sheet, labels: &BTreeMap<String, String>) -> String {
    let l = |n: &'static str| labels.get(n).map_or(n, String::as_str);
    let attrs = ATTRIBUTES.iter().map(|a| format!("{} {}", l(a), s.attribute(a))).collect::<Vec<_>>().join(" · ");
    let skills = s
        .skills
        .iter()
        .map(|(k, v)| {
            let (g, _) = s.gear_bonus(k);
            let total = s.attribute(&v.attribute) + v.rank * 2 + g;
            let attr = labels.get(&v.attribute).unwrap_or(&v.attribute);
            format!("{k}: rank {} {} ({attr}, base score {total})", v.rank, rank_title(v.rank))
        })
        .collect::<Vec<_>>()
        .join("; ");
    let items = if s.inventory.is_empty() {
        "(nothing)".to_string()
    } else {
        s.inventory.iter().map(|i| if i.qty > 1 { format!("{} x{}", i.name, i.qty) } else { i.name.clone() }).collect::<Vec<_>>().join(", ")
    };
    let conds = if s.conditions.is_empty() {
        "none".to_string()
    } else {
        s.conditions.iter().map(|c| format!("{} ({:+})", c.name, c.modifier)).collect::<Vec<_>>().join(", ")
    };
    let mut t = format!(
        "{} - Level {} {} | XP {}/{}\n{} {}/{} · {} {}/{}",
        s.name,
        s.level,
        if s.class.is_empty() { "Adventurer" } else { &s.class },
        s.xp,
        xp_for_level(s.level + 1),
        l("HP"),
        s.hp.cur,
        s.hp.max,
        l("Stamina"),
        s.stamina.cur,
        s.stamina.max
    );
    if s.mana.max > 0 {
        t.push_str(&format!(" · {} {}/{}", l("Mana"), s.mana.cur, s.mana.max));
    }
    t.push_str(&format!("\n{attrs}\nSkills: {skills}\n{}: {} · Carrying: {items}\nConditions: {conds}", l("Money"), s.money));
    if !s.perks.is_empty() {
        t.push_str(&format!("\nPerks: {}", s.perks.keys().cloned().collect::<Vec<_>>().join(", ")));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        let mut r = Rules { enabled: true, ..Default::default() };
        let mut s = Sheet { name: "Wren".into(), ..Sheet::default() };
        s.skills.insert("Observation".into(), Skill { attribute: "Insight".into(), rank: 4, marks: 0 });
        s.skills.insert("Strength".into(), Skill { attribute: "Might".into(), rank: 0, marks: 0 });
        s.inventory.push(Item { name: "Hand lens".into(), qty: 1, bonuses: [("Observation".to_string(), 2)].into() });
        r.sheets.insert("Wren".into(), s);
        r
    }

    fn req(skill: &str, band: &str, subject: &str) -> CheckRequest {
        CheckRequest { character: "Wren".into(), skill: skill.into(), subject: subject.into(), difficulty: band.into(), ..Default::default() }
    }

    /// A second character at the table, so a line has to say whose it is.
    fn with_bram(mut r: Rules) -> Rules {
        r.sheets.insert("Bram".into(), Sheet { name: "Bram".into(), ..Sheet::default() });
        r
    }

    #[test]
    fn all_prefixes_a_party_line() {
        assert_eq!(party_line("all: xp +40"), Some("xp +40"));
        assert_eq!(party_line("  Everyone: money +10 "), Some("money +10"));
        assert_eq!(party_line("party: hp +2"), Some("hp +2"));
        assert_eq!(party_line("all"), None);
        assert_eq!(party_line("all : xp +40"), None); // the colon comes straight after the word
        assert_eq!(party_line("Wren: xp +5"), None);
        assert_eq!(party_line("xp +5"), None);
    }

    #[test]
    fn names_character_sees_the_sheets_own_names() {
        let r = rules();
        assert!(r.names_character("Wren: xp +5"));
        assert!(r.names_character("- wren: hp -1"));
        assert!(!r.names_character("xp +5"));
        assert!(!r.names_character("gear Hand lens | Observation | +1"));
        assert!(!r.names_character("perk + Tidewise: never lost at low tide"));
    }

    #[test]
    fn a_party_line_changes_every_character() {
        let mut r = with_bram(rules());
        let m = r.apply("all: xp +40", "Wren").unwrap();
        assert_eq!(r.sheets["Wren"].xp, 40);
        assert_eq!(r.sheets["Bram"].xp, 40, "{m}");
        assert!(m.contains("Wren") && m.contains("Bram"), "{m}");
        // A change one character can't take doesn't undo the others, but the GM hears about it.
        if let Some(w) = r.sheets.get_mut("Wren") {
            w.money = 20;
        }
        let e = r.apply("all: money -5", "").unwrap_err();
        assert!(e.contains("Bram"), "{e}");
        assert_eq!(r.sheets["Wren"].money, 15);
        assert_eq!(r.sheets["Bram"].money, 0);
    }

    #[test]
    fn a_party_line_refuses_a_rename() {
        let mut r = with_bram(rules());
        assert!(r.apply("all: rename stat Might -> Grit", "Wren").is_err());
        assert!(r.labels.is_empty());
        assert!(r.apply("all:", "Wren").is_err());
    }

    #[test]
    fn an_unnamed_line_with_two_characters_says_whose_it_should_be() {
        let mut r = with_bram(rules());
        let e = apply_line(&mut r, "xp +40", "").unwrap_err();
        assert!(e.contains("Wren") && e.contains("Bram"), "{e}");
        // One character at the table still gets an unnamed line, as it always has.
        let mut solo = rules();
        assert!(apply_line(&mut solo, "xp +40", "").is_ok());
        assert_eq!(solo.sheets["Wren"].xp, 40);
    }

    /// The AD System's rules, with two characters behind two status windows.
    fn ad_table() -> Rules {
        let mut r = ad_rules();
        let base = crate::ad::STATS.iter().map(|s| (s.to_string(), 10)).collect();
        let sheet = crate::ad::Sheet::new("Bram", "human", base, &r.ad.defs);
        r.ad.sheets.insert("Bram".into(), sheet);
        r
    }

    #[test]
    fn a_party_line_reaches_every_status_window() {
        let mut r = ad_table();
        let m = r.apply("all: xp 1000", "").unwrap();
        assert!(m.contains("Wren") && m.contains("Bram"), "{m}");
        for n in ["Wren", "Bram"] {
            assert!(r.ad.sheets[n].level > 1, "{n} stayed at level 1: {m}");
        }
    }

    #[test]
    fn the_system_too_asks_whose_line_it_is() {
        let mut r = ad_table();
        let e = r.apply("xp 1000", "").unwrap_err();
        assert!(e.contains("Wren") && e.contains("Bram"), "{e}");
        assert_eq!(r.ad.sheets["Wren"].level, 1);
    }

    #[test]
    fn score_is_attribute_rank_and_gear() {
        let mut r = rules();
        let c = r.resolve(&req("Observation", "hard", "the tracks")).unwrap();
        assert_eq!(c.score, 10 + 8 + 2);
        assert_eq!(c.target, 16);
        assert_eq!(c.margin, 4);
        assert_eq!(c.tier, Tier::Success);
    }

    #[test]
    fn identical_situation_identical_result_over_many_runs() {
        let first = {
            let mut r = rules();
            r.resolve(&req("Strength", "hard", "the door")).unwrap()
        };
        for _ in 0..1000 {
            let mut r = rules();
            let c = r.resolve(&req("Strength", "hard", "the door")).unwrap();
            assert_eq!((c.score, c.target, c.tier), (first.score, first.target, first.tier));
        }
    }

    #[test]
    fn tiers_have_no_gaps() {
        for m in -40..40 {
            assert_eq!(Tier::from_margin(m).succeeded(), m >= -2, "margin {m}");
        }
    }

    #[test]
    fn asking_again_cannot_fish_for_an_easier_ruling() {
        let mut r = rules();
        let a = r.resolve(&req("Strength", "hard", "the door")).unwrap();
        let mut easier = req("Strength", "trivial", "The  Door");
        easier.circumstances.push(Circumstance { label: "I insist".into(), value: 3 });
        let b = r.resolve(&easier).unwrap();
        assert!(b.reused);
        assert_eq!(b.target, a.target);
        assert_eq!(b.circumstance_total, a.circumstance_total);
    }

    #[test]
    fn repeating_an_attempt_cannot_farm_xp_or_skill() {
        let mut r = rules();
        r.resolve(&req("Strength", "easy", "the crate")).unwrap();
        let (xp, marks) = (r.sheets["Wren"].xp, r.sheets["Wren"].skills["Strength"].marks);
        assert!(xp > 0);
        for _ in 0..50 {
            r.resolve(&req("Strength", "easy", "the crate")).unwrap();
        }
        assert_eq!((r.sheets["Wren"].xp, r.sheets["Wren"].skills["Strength"].marks), (xp, marks));
        // A new subject still teaches.
        r.resolve(&req("Strength", "easy", "the barrel")).unwrap();
        assert!(r.sheets["Wren"].xp > xp);
    }

    #[test]
    fn a_repeated_failure_still_hurts() {
        let mut r = rules();
        let mut q = req("Strength", "moderate", "the wall");
        q.stakes.hp = 3;
        r.resolve(&q).unwrap();
        r.resolve(&q).unwrap();
        assert_eq!(r.sheets["Wren"].hp.cur, 14);
    }

    #[test]
    fn a_rank_title_in_the_name_still_means_that_skill() {
        let mut r = rules();
        let c = r.resolve(&req("Observation Expert", "hard", "the tide marks")).unwrap();
        assert_eq!((c.skill.as_str(), c.rank), ("Observation", 4));
        assert_eq!(r.sheets["Wren"].skills.len(), 2);
        // The status window names skills in a way that can be copied exactly.
        assert!(status_text(&r.sheets["Wren"], &r.labels).contains("Observation: rank 4 Expert"));
    }

    #[test]
    fn a_rank_is_never_part_of_a_skills_name() {
        let mut r = rules();
        r.sheets.get_mut("Wren").unwrap().skills.insert("Varrow Streets".into(), Skill { attribute: "Insight".into(), rank: 4, marks: 0 });
        for (i, wording) in ["Varrow Streets Expert", "varrow streets expert untrained", "Varrow Streets (Expert)", "Varrow Streets, rank 4", "Varrow Streets: Expert", "VARROW STREETS - EXPERT", "Streets"].iter().enumerate() {
            let c = r.resolve(&req(wording, "hard", &format!("lane {i}"))).unwrap();
            assert_eq!((c.skill.as_str(), c.rank), ("Varrow Streets", 4), "{wording}");
        }
        assert_eq!(r.sheets["Wren"].skills.len(), 3);
    }

    #[test]
    fn real_new_skills_still_get_made_and_keep_their_names() {
        let mut r = rules();
        let mut s = Sheet { name: "Ann".into(), ..Sheet::default() };
        s.skills.insert("Wright".into(), Skill { attribute: "Insight".into(), rank: 3, marks: 0 });
        s.skills.insert("Wright Lore".into(), Skill { attribute: "Intellect".into(), rank: 2, marks: 0 });
        r.sheets.insert("Ann".into(), s);
        let ask = |skill: &str, sub: &str| CheckRequest { character: "Ann".into(), skill: skill.into(), subject: sub.into(), difficulty: "easy".into(), attribute: "Might".into(), ..Default::default() };
        assert_eq!(r.resolve(&ask("Wright Lore", "a")).unwrap().rank, 2); // exact beats close
        assert_eq!(r.resolve(&ask("Wright", "b")).unwrap().rank, 3);
        assert_eq!(r.resolve(&ask("Swimming", "c")).unwrap().rank, 0); // unrelated is new
        assert!(r.sheets["Ann"].skills.contains_key("Swimming"));
        assert_eq!(r.resolve(&ask("Harbor Master", "d")).unwrap().skill, "Harbor Master"); // a title in a real name stays
    }

    #[test]
    fn old_sheets_repair_themselves() {
        let mut r = rules();
        let s = r.sheets.get_mut("Wren").unwrap();
        s.skills.insert("Varrow Streets".into(), Skill { attribute: "Insight".into(), rank: 4, marks: 1 });
        s.skills.insert("Varrow Streets Expert".into(), Skill { attribute: "Insight".into(), rank: 0, marks: 1 });
        r.history.push(Record { character: "Wren".into(), skill: "Varrow Streets Expert".into(), subject: "Wharf Lane".into(), target: 14, ..Default::default() });
        r.rivals.insert("the watcher|varrow streets expert".into(), 14);
        r.rivals.insert("ottilie|varrow streets".into(), 12);
        r.tidy();
        let w = &r.sheets["Wren"];
        assert!(!w.skills.contains_key("Varrow Streets Expert"));
        assert_eq!((w.skills["Varrow Streets"].rank, w.skills["Varrow Streets"].marks), (4, 2));
        assert_eq!(r.history[0].skill, "Varrow Streets");
        assert_eq!(r.rivals.get("the watcher|varrow streets"), Some(&14));
        assert_eq!(r.rivals.get("ottilie|varrow streets"), Some(&12));
        // A real skill with a title in its name is left alone when nothing else matches.
        r.sheets.get_mut("Wren").unwrap().skills.insert("Harbor Master".into(), Skill::default());
        r.tidy();
        assert!(r.sheets["Wren"].skills.contains_key("Harbor Master"));
    }

    #[test]
    fn renaming_changes_names_and_no_numbers() {
        let mut r = rules();
        r.resolve(&req("Observation", "hard", "the tide")).unwrap();
        let mut q = req("Observation", "", "Hale");
        q.opponent = "Hale".into();
        q.opponent_score = 15;
        r.resolve(&q).unwrap();
        let before = r.sheets["Wren"].clone();
        apply_line(&mut r, "rename skill Observation -> Tidereading", "Wren").unwrap();
        let s = &r.sheets["Wren"];
        assert_eq!(s.skills["Tidereading"], before.skills["Observation"]);
        assert!(!s.skills.contains_key("Observation"));
        assert_eq!(s.inventory[0].bonuses["Tidereading"], 2); // gear follows
        assert_eq!((s.xp, s.level), (before.xp, before.level));
        // Past rulings and rivals follow, so the name change can't dodge them.
        let c = r.resolve(&req("Tidereading", "trivial", "the tide")).unwrap();
        assert!(c.reused && c.target == 16);
        let mut q2 = req("Tidereading", "", "Hale again");
        q2.opponent = "Hale".into();
        q2.opponent_score = 4;
        assert_eq!(r.resolve(&q2).unwrap().target, 15);
    }

    #[test]
    fn stats_can_be_renamed_and_still_work() {
        let mut r = rules();
        apply_line(&mut r, "rename stat Might -> Grit", "Wren").unwrap();
        apply_line(&mut r, "rename stat Money -> Shillings", "Wren").unwrap();
        apply_line(&mut r, "rename stat Grit -> Brawn", "Wren").unwrap(); // by its new name too
        assert_eq!(r.label("Might"), "Brawn");
        assert_eq!(r.sheets["Wren"].attribute("Might"), 10);
        let t = status_text(&r.sheets["Wren"], &r.labels);
        assert!(t.contains("Brawn 10") && t.contains("Shillings: 0") && !t.contains("Might"));
        // The Referee answers in the story's names.
        let mut q = req("Wrestling", "easy", "the bear");
        q.attribute = "Brawn".into();
        assert_eq!(r.resolve(&q).unwrap().attribute, "Might");
        assert!(apply_line(&mut r, "rename stat Agility -> Brawn", "Wren").is_err());
        assert!(apply_line(&mut r, "rename stat Luck -> Fate", "Wren").is_err());
    }

    #[test]
    fn classes_and_perks_can_be_renamed() {
        let mut r = rules();
        apply_line(&mut r, "class Shadow Wright", "Wren").unwrap();
        assert_eq!(r.sheets["Wren"].class, "Shadow Wright");
        apply_line(&mut r, "perk + Tidewise: never lost at low tide", "Wren").unwrap();
        apply_line(&mut r, "rename perk Tidewise -> Salt Sense", "Wren").unwrap();
        assert_eq!(r.sheets["Wren"].perks["Salt Sense"], "never lost at low tide");
        assert!(apply_line(&mut r, "rename perk Nope -> X", "Wren").is_err());
    }

    #[test]
    fn a_rename_cannot_smuggle_a_rank_or_clobber_a_skill() {
        let mut r = rules();
        assert!(apply_line(&mut r, "rename skill Observation -> Sharp Eye Expert", "Wren").is_err());
        assert!(apply_line(&mut r, "rename skill Observation -> Strength", "Wren").is_err());
        assert!(apply_line(&mut r, "rename skill Observation -> <b>x", "Wren").is_ok()); // cleaned
        assert!(r.sheets["Wren"].skills.contains_key("bx"));
        assert!(apply_line(&mut r, "rename skill Nope -> X", "Wren").is_err());
    }

    #[test]
    fn stakes_scale_with_how_badly_it_went() {
        let stakes = Stakes { hp: 4, ..Default::default() };
        let hit = |band: &str| {
            let mut r = rules();
            let mut q = req("Strength", band, "x");
            q.stakes = stakes.clone();
            r.resolve(&q).unwrap();
            20 - r.sheets["Wren"].hp.cur
        };
        assert_eq!(hit("trivial"), 0); // success
        assert_eq!(hit("moderate"), 4); // 10 vs 13: failure
        assert_eq!(hit("very hard"), 6); // 10 vs 19: disaster
    }

    #[test]
    fn circumstances_are_capped() {
        let mut r = rules();
        let mut q = req("Strength", "moderate", "x");
        q.circumstances = (0..3).map(|i| Circumstance { label: i.to_string(), value: 99 }).collect();
        assert_eq!(r.resolve(&q).unwrap().circumstance_total, CIRCUMSTANCE_CAP);
    }

    #[test]
    fn opponent_keeps_a_fixed_score() {
        let mut r = rules();
        let mut q = req("Observation", "", "Hale's lie");
        q.opponent = "Dunn Hale".into();
        q.opponent_score = 15;
        assert_eq!(r.resolve(&q).unwrap().target, 15);
        let mut q2 = req("Observation", "", "Hale's other lie");
        q2.opponent = "dunn  hale".into();
        q2.opponent_score = 3; // a model can't weaken them later
        assert_eq!(r.resolve(&q2).unwrap().target, 15);
    }

    #[test]
    fn levels_and_skills_are_earned_at_fixed_thresholds() {
        let mut r = rules();
        let mut log = Vec::new();
        let s = r.sheets.get_mut("Wren").unwrap();
        s.gain_xp(99, &mut log);
        assert_eq!(s.level, 1);
        s.gain_xp(1, &mut log);
        assert_eq!(s.level, 2);
        assert_eq!((s.attribute_points, s.skill_points), (1, 2));
        s.add_marks("Strength", 3, &mut log);
        assert_eq!(s.skills["Strength"].rank, 1);
        assert_eq!(s.skills["Strength"].marks, 0);
    }

    #[test]
    fn the_gm_cannot_break_the_economy() {
        let mut r = rules();
        assert!(apply_line(&mut r, "money -5", "Wren").is_err());
        assert!(apply_line(&mut r, "item - Excalibur", "Wren").is_err());
        assert!(apply_line(&mut r, "xp -10", "Wren").is_err());
        assert!(apply_line(&mut r, "xp 99999", "Wren").is_ok());
        assert_eq!(r.sheets["Wren"].xp, 50); // capped per reply
        apply_line(&mut r, "hp -999", "Wren").unwrap();
        assert_eq!(r.sheets["Wren"].hp.cur, 0);
        assert!(r.resolve(&req("Observation", "easy", "y")).is_err()); // down
    }

    #[test]
    fn items_and_conditions_round_trip() {
        let mut r = rules();
        apply_line(&mut r, "item + Rope x3", "Wren").unwrap();
        apply_line(&mut r, "- Wren: item - Rope x2", "Wren").unwrap();
        assert_eq!(r.sheets["Wren"].inventory_count("rope"), 1);
        apply_line(&mut r, "condition + Soaked -2", "Wren").unwrap();
        assert_eq!(r.sheets["Wren"].condition_total(), -2);
        assert_eq!(r.resolve(&req("Strength", "easy", "k")).unwrap().score, 8);
        apply_line(&mut r, "condition - soaked", "Wren").unwrap();
        assert_eq!(r.sheets["Wren"].condition_total(), 0);
    }

    #[test]
    fn imported_sheets_are_clamped() {
        let v = serde_json::json!({"name":"X","attributes":{"Might":99},"skills":{"Sword":{"attribute":"might","rank":99}},"inventory":[{"name":"Vorpal","bonuses":{"Sword":50}}]});
        let s = sheet_from_json(&v).unwrap();
        assert_eq!(s.attribute("Might"), 16);
        assert_eq!(s.skills["Sword"].rank, 6);
        assert_eq!(s.inventory[0].bonuses["Sword"], 3);
        assert_eq!((s.attribute_points, s.skill_points), (0, 0));
    }

    fn ad_rules() -> Rules {
        let mut r = Rules { enabled: true, ruleset: "ad".into(), ..Default::default() };
        let base = crate::ad::STATS.iter().map(|s| (s.to_string(), 10)).collect();
        let s = crate::ad::Sheet::new("Wren", "human", base, &r.ad.defs);
        r.ad.sheets.insert("Wren".into(), s);
        r
    }

    #[test]
    fn the_gm_names_the_system_and_the_asking_stops() {
        let mut r = ad_rules();
        // The rules came on after the story had begun, so the words aren't the world's yet.
        r.terms_pending = true;
        assert_eq!(r.view("Wren").unwrap()["labels"], serde_json::json!({}));
        assert!(r.apply("rename skills -> Talents", "Wren").is_ok());
        assert!(!r.terms_pending, "a rename means the words were settled");
        let v = r.view("Wren").unwrap();
        assert_eq!(v["labels"]["skills"], "Talents");
        // "Keep the System's words as they are" settles it too, without touching a name.
        r.terms_pending = true;
        assert!(r.apply("rename settled", "Wren").is_ok());
        assert!(!r.terms_pending);
        assert_eq!(r.view("Wren").unwrap()["labels"]["skills"], "Talents");
        // A line the rules refuse settles nothing.
        r.terms_pending = true;
        assert!(r.apply("rename skills ->", "Wren").is_err());
        assert!(r.terms_pending);
        // A name in front of a world line is the same world change, and settles the same way.
        r.terms_pending = true;
        assert!(r.apply("Wren: rename speed -> Haste", "Wren").is_ok());
        assert!(!r.terms_pending, "a named rename settles the words too");
        assert_eq!(r.view("Wren").unwrap()["labels"]["speed"], "Haste");
        r.terms_pending = true;
        assert!(r.apply("Wren: rename settled", "Wren").is_ok());
        assert!(!r.terms_pending);
    }

    #[test]
    fn keeping_the_sheets_words_settles_the_ask_under_either_ruleset() {
        let mut r = rules();
        r.terms_pending = true;
        assert!(r.apply("rename settled", "Wren").is_ok());
        assert!(!r.terms_pending);
        // A real rename settles it too, and only when the rules accept it.
        r.terms_pending = true;
        assert!(r.apply("rename stat Luck -> Fate", "Wren").is_err());
        assert!(r.terms_pending);
        assert!(r.apply("rename stat Might -> Grit", "Wren").is_ok());
        assert!(!r.terms_pending);
    }

    #[test]
    fn no_random_source_in_the_rules() {
        let src = include_str!("rpg.rs");
        assert!(!src.contains(&["ran", "d::"].concat()) && !src.contains(&["thread_", "rng"].concat()));
    }
}
