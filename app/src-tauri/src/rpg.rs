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
        let attribute = known.map(|s| s.attribute.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| canon_attribute(&req.attribute));
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

fn canon_attribute(s: &str) -> String {
    ATTRIBUTES.iter().find(|a| a.eq_ignore_ascii_case(s.trim())).map_or_else(|| "Intellect".to_string(), |a| a.to_string())
}

/// One change the GM asked for at the end of a reply, in `<rpg>` lines.
pub fn apply_line(r: &mut Rules, line: &str, default_char: &str) -> Result<String, String> {
    let line = line.trim().trim_start_matches('-').trim();
    // Optional "Name: " prefix picks a character.
    let (who, cmd) = match line.split_once(':') {
        Some((a, b)) if r.sheet_key(a).is_some() && !a.trim().is_empty() => (a.trim().to_string(), b.trim()),
        _ => (default_char.to_string(), line),
    };
    let key = r.sheet_key(&who).ok_or_else(|| format!("no sheet for '{who}'"))?;
    let level = r.sheets.get(&key).map_or(1, |s| s.level) as i64;
    let s = r.sheets.get_mut(&key).ok_or("no sheet")?;
    let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
    let verb = verb.to_lowercase();
    let rest = rest.trim();
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
                s.skills.insert(name.trim().into(), Skill { attribute: canon_attribute(attr), rank: 0, marks: 0 });
            }
            format!("New skill: {}", name.trim())
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
            let attribute = canon_attribute(sk["attribute"].as_str().unwrap_or(""));
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

/// The status window as text: what the GM sees each turn.
pub fn status_text(s: &Sheet) -> String {
    let attrs = ATTRIBUTES.iter().map(|a| format!("{a} {}", s.attribute(a))).collect::<Vec<_>>().join(" · ");
    let skills = s
        .skills
        .iter()
        .map(|(k, v)| {
            let (g, _) = s.gear_bonus(k);
            let total = s.attribute(&v.attribute) + v.rank * 2 + g;
            format!("{k}: rank {} {} ({}, base score {total})", v.rank, rank_title(v.rank), v.attribute)
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
        "{} - Level {} {} | XP {}/{}\nHP {}/{} · Stamina {}/{}",
        s.name,
        s.level,
        if s.class.is_empty() { "Adventurer" } else { &s.class },
        s.xp,
        xp_for_level(s.level + 1),
        s.hp.cur,
        s.hp.max,
        s.stamina.cur,
        s.stamina.max
    );
    if s.mana.max > 0 {
        t.push_str(&format!(" · Mana {}/{}", s.mana.cur, s.mana.max));
    }
    t.push_str(&format!("\n{attrs}\nSkills: {skills}\nMoney: {} · Carrying: {items}\nConditions: {conds}", s.money));
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
        assert!(status_text(&r.sheets["Wren"]).contains("Observation: rank 4 Expert"));
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

    #[test]
    fn no_random_source_in_the_rules() {
        let src = include_str!("rpg.rs");
        assert!(!src.contains(&["ran", "d::"].concat()) && !src.contains(&["thread_", "rng"].concat()));
    }
}
