//! The AD System: the LitRPG rules of Wolfgang's world, replicated from Spirit's game engine
//! (Spirit/game-engine: engine/system.py, engine/frequency.py, engine/semi.py, and his own file
//! design/AD-magic-crafting-world.md). The formulas, tiers, thresholds and words are his and are kept
//! exactly; only the container is Campaign Keeper's.
//!
//! THE FIXED RULES (Wolfgang, 2026-11-12/13, 2026-11-18/19). Everything else is the System's judgment.
//!
//!   Levels      experience for the next level x:  y = 100 * ((sqrt(x) + 10) / sqrt(x)) ^ (0.5 * sqrt(x)).  Cap 15,625.
//!               Skills level by the SAME curve, each with its own level and experience.
//!   Defeats     a = 0.05 * 100 * ((sqrt(b) + 10) / sqrt(b)) ^ (0.6 * sqrt(b)), b = the enemy's level, times the
//!               difference multiplier.
//!   Base stats  HP, STR, INT, DEX; they rise a set amount per level (species growth + class growth).
//!   Derived     amplitude = INT ^ 100 (the most each frequency can hold), speed = DEX ^ STR * 0.01,
//!               stamina = STR ^ HP * 0.1, plus whatever a class or species adds.
//!   Class       null, common, uncommon, rare, epic, legendary, mythical; next tier at level 5 ^ b.
//!   Species     base, adapted, heroic, cataclysmic; next tier at level 25 ^ d. Each species upgrade opens a frequency.
//!   Skill tiers unskilled, skilled, trained, experienced, expert, founder, unique; next at skill level 5 ^ tier.
//!   Racial skill slots at level (50 ^ d) * f; class skill slots at level (5 ^ b) * h.
//!   Feats       may carry an upgrade only when the character had at most 1/100 (skill), 1/1,000 (class) or
//!               1/10,000 (species) chance of completing it.
//!   Past the cap: bonus points, bought on the same curve.
//!   Frequencies places from 5; everyone holds 5, every other starting number 1 in 10 ^ (places from 5).
//!               Amplitude per frequency; regen INT x sqrt(max) ^ 0.1 x INT a minute, doubled resting.
//!               More than 75% of one frequency you are not tuned for: all stats -20%, regen halved.
//!               Overuse decreases that frequency's maximum and hurts; decreased to nothing, it is blocked.
//!   Semi skills one per frequency held; power = base x log10(1 + amplitude) / 2 x (1 + 0.05 x skill level).
//!   Overpower   anything frequental on a person without consent needs max(10 x their remaining, 10% of their max).
//!
//! The GM sits in the System's seat, as a model does in Spirit (`Sheet.auto = False`): it judges deeds and
//! hands out experience, skills, feats and tier-up paths, and this module checks every change against
//! the fixed rules. Nothing here is random: where Spirit rolls a soul prism once and keeps it forever,
//! the roll here comes from the character's name, so it is the same every time (Spirit does the same
//! for a sheet saved before prisms existed: `random.Random(f"prism:{name}")`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const STATS: [&str; 4] = ["HP", "STR", "INT", "DEX"];
pub const MAX_LEVEL: u32 = 15625;
pub const CLASS_TIERS: [&str; 7] = ["null", "common", "uncommon", "rare", "epic", "legendary", "mythical"];
pub const SPECIES_TIERS: [&str; 4] = ["base", "adapted", "heroic", "cataclysmic"];
pub const SKILL_TIERS: [&str; 7] = ["unskilled", "skilled", "trained", "experienced", "expert", "founder", "unique"];
const DERIVED: [(&str, &str); 3] = [("amplitude", "int ^ 100"), ("speed", "dex ^ str * 0.01"), ("stamina", "str ^ hp * 0.1")];
const DEFAULT_GROWTH: [(&str, i64); 4] = [("HP", 2), ("STR", 1), ("INT", 1), ("DEX", 1)];
pub const BP_COST: [(&str, u64); 5] = [("skill_tier", 100), ("class_tier", 5000), ("species_tier", 10000), ("class_skill", 500), ("racial_skill", 1000)];
pub const REVIVAL_SHARE: f64 = 0.25;
pub const FEAT_ODDS: [(&str, f64); 3] = [("species", 1.0 / 10000.0), ("class", 1.0 / 1000.0), ("skill", 1.0 / 100.0)];
// frequency.py: an unbalanced environment, and overuse (the overuse sizes are Spirit's fill-ins).
const UNBALANCED: f64 = 0.75;
const OVERUSE_PAIN: f64 = 20.0;
const OVERUSE_PAIN_MIN: f64 = 2.0;
/// Overuse pain fades this much a second (frequencies.json "overuse_pain_fades_per_s").
const PAIN_FADES_PER_S: f64 = 0.05;
/// A creature with no soul prism: its amplitude is its level x this (frequencies.json "direct").
const CREATURE_AMPLITUDE_PER_LEVEL: u64 = 10;

fn stat_name(s: &str) -> &'static str {
    match s {
        "HP" => "Health",
        "STR" => "Strength",
        "INT" => "Intelligence",
        _ => "Dexterity",
    }
}

// ------------------------------------------------------------------ exact numbers of any size

/// An exact number of any size (Python's int and Fraction in Spirit), kept in files as text.
#[derive(Clone, Debug, PartialEq)]
pub struct Big(pub BigRational);

impl Default for Big {
    fn default() -> Self {
        Big(BigRational::zero())
    }
}

impl Big {
    pub fn int(n: impl Into<BigInt>) -> Big {
        Big(BigRational::from_integer(n.into()))
    }
}

impl Serialize for Big {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.0.is_integer() {
            s.serialize_str(&self.0.to_integer().to_string())
        } else {
            s.serialize_str(&format!("{}/{}", self.0.numer(), self.0.denom()))
        }
    }
}

impl<'de> Deserialize<'de> for Big {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let t = match &v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => "0".into(),
        };
        Ok(Big(parse_rational(&t).unwrap_or_else(BigRational::zero)))
    }
}

fn rat(n: i64) -> BigRational {
    BigRational::from_integer(BigInt::from(n))
}

fn ratio(n: i64, d: i64) -> BigRational {
    BigRational::new(BigInt::from(n), BigInt::from(d))
}

fn pow10(e: u32) -> BigInt {
    num_traits::pow(BigInt::from(10), e as usize)
}

/// "250", "3.2e415", "1/3", "12.5": exact.
pub fn parse_rational(t: &str) -> Option<BigRational> {
    let t = t.trim().trim_start_matches('+').replace(['_', ','], "");
    if let Some((a, b)) = t.split_once('/') {
        let (a, b) = (parse_rational(a)?, parse_rational(b)?);
        return (!b.is_zero()).then(|| a / b);
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.to_string()),
        None => (false, t.clone()),
    };
    let (mant, exp) = match t.to_lowercase().split_once('e') {
        Some((m, e)) => (m.to_string(), e.parse::<i64>().ok()?),
        None => (t.clone(), 0),
    };
    let (whole, frac) = mant.split_once('.').unwrap_or((&mant, ""));
    if whole.is_empty() && frac.is_empty() || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let digits: BigInt = format!("{whole}{frac}").parse().ok()?;
    let e = exp - frac.len() as i64;
    if e.abs() > 2_000_000 {
        return None;
    }
    let mut r = BigRational::from_integer(digits);
    if e >= 0 {
        r *= BigRational::from_integer(pow10(e as u32));
    } else {
        r /= BigRational::from_integer(pow10((-e) as u32));
    }
    Some(if neg { -r } else { r })
}

/// Experience from text: a whole number of any size, never negative (system.py `to_xp`).
pub fn to_xp(t: &str) -> BigInt {
    parse_rational(t).map(|r| r.floor().to_integer()).filter(|n| n.is_positive()).unwrap_or_else(BigInt::zero)
}

/// A number of any size in short form (system.py `short`): "438", "1.00e100".
pub fn short(n: &BigRational) -> String {
    if !n.is_integer() && n < &rat(1_000_000) {
        return g4(n.to_f64().unwrap_or(0.0));
    }
    short_int(&n.floor().to_integer())
}

pub fn short_int(n: &BigInt) -> String {
    if n < &BigInt::from(1_000_000_000u64) {
        return n.to_string();
    }
    let mut e = ((n.bits() - 1) as f64 * 0.30102999566398) as u32;
    let mut lead = n / pow10(e - 2);
    if lead >= BigInt::from(1000) {
        e += 1;
        lead /= 10;
    } else if lead < BigInt::from(100) {
        lead = n / pow10(e - 3);
        e -= 1;
    }
    let lead = lead.to_u64().unwrap_or(0);
    format!("{}.{:02}e{}", lead / 100, lead % 100, e)
}

/// Python's f"{x:.4g}".
fn g4(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return "0".into();
    }
    let exp = x.abs().log10().floor() as i32;
    let trim = |s: String| if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if !(-4..4).contains(&exp) {
        let m = x / 10f64.powi(exp);
        return format!("{}e{}{:02}", trim(format!("{m:.3}")), if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    trim(format!("{:.*}", (3 - exp).max(0) as usize, x))
}

/// A derived stat: + - * / ^ and brackets over hp, str, int, dex and numbers. Exact. Refuses anything
/// else, and a power so large it would never finish (system.py `formula`).
pub fn formula(text: &str, stats: &BTreeMap<String, i64>) -> Result<BigRational, String> {
    let names: BTreeMap<String, BigRational> = stats.iter().map(|(k, v)| (k.to_lowercase(), rat(*v))).collect();
    let mut p = Parser { s: text.replace("**", "^").chars().collect(), i: 0, names: &names };
    let v = p.expr()?;
    p.ws();
    if p.i < p.s.len() {
        return Err("only + - * / ^ ( ) over hp, str, int, dex and numbers".into());
    }
    Ok(v)
}

struct Parser<'a> {
    s: Vec<char>,
    i: usize,
    names: &'a BTreeMap<String, BigRational>,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: char) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Result<BigRational, String> {
        let mut a = self.term()?;
        loop {
            if self.eat('+') {
                a += self.term()?;
            } else if self.eat('-') {
                a -= self.term()?;
            } else {
                return Ok(a);
            }
        }
    }
    fn term(&mut self) -> Result<BigRational, String> {
        let mut a = self.unary()?;
        loop {
            if self.eat('*') {
                a *= self.unary()?;
            } else if self.eat('/') {
                let b = self.unary()?;
                if b.is_zero() {
                    return Err("division by zero".into());
                }
                a /= b;
            } else {
                return Ok(a);
            }
        }
    }
    fn unary(&mut self) -> Result<BigRational, String> {
        if self.eat('-') {
            return Ok(-self.unary()?);
        }
        self.power()
    }
    fn power(&mut self) -> Result<BigRational, String> {
        let a = self.atom()?;
        if !self.eat('^') {
            return Ok(a);
        }
        let b = self.unary()?;
        if !b.is_integer() || b.is_negative() {
            return Err("a power must be a whole number, 0 or more".into());
        }
        let e = b.to_integer().to_u64().ok_or("too large to work out")?;
        let size = a.numer().bits().max(a.denom().bits()).max(1);
        if e.saturating_mul(size) > 4_000_000 {
            return Err("too large to work out".into());
        }
        Ok(a.pow(e as i32))
    }
    fn atom(&mut self) -> Result<BigRational, String> {
        self.ws();
        if self.eat('(') {
            let v = self.expr()?;
            if !self.eat(')') {
                return Err("a bracket is not closed".into());
            }
            return Ok(v);
        }
        let start = self.i;
        let c = *self.s.get(self.i).ok_or("only + - * / ^ ( ) over hp, str, int, dex and numbers")?;
        if c.is_ascii_digit() || c == '.' {
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                self.i += 1;
            }
            if self.s.get(self.i).is_some_and(|c| *c == 'e' || *c == 'E') && self.s.get(self.i + 1).is_some_and(|c| c.is_ascii_digit() || *c == '-' || *c == '+') {
                self.i += 2;
                while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
            let t: String = self.s[start..self.i].iter().collect();
            return parse_rational(&t).ok_or_else(|| format!("not a number: {t}"));
        }
        if c.is_alphabetic() {
            while self.s.get(self.i).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                self.i += 1;
            }
            let t: String = self.s[start..self.i].iter().collect::<String>().to_lowercase();
            return self.names.get(&t).cloned().ok_or_else(|| "only + - * / ^ ( ) over hp, str, int, dex and numbers".to_string());
        }
        Err("only + - * / ^ ( ) over hp, str, int, dex and numbers".into())
    }
}

// ------------------------------------------------------------------ the fixed curves (system.py)

/// Experience to go from `level` to the next: y = 100 * ((sqrt(x) + 10) / sqrt(x)) ^ (0.5 * sqrt(x)),
/// x = the NEXT level. About 438 for level 2, 3,200 around level 100, 12,300 at the cap.
pub fn level_xp_needed(level: u32) -> u64 {
    let r = ((level + 1) as f64).sqrt();
    (100.0 * ((r + 10.0) / r).powf(0.5 * r)) as u64
}

/// Past the cap a character is level 15625 Bp1, Bp2, ...: the same curve, with the cost of the last level in place of the 100.
pub fn bonus_xp_needed(bp: u64) -> u64 {
    let r = ((bp + 1).min(1_000_000_000_000) as f64).sqrt();
    (level_xp_needed(MAX_LEVEL - 1) as f64 * ((r + 10.0) / r).powf(0.5 * r)) as u64
}

/// Gross experience for defeating an enemy: a = 0.05 * 100 * ((sqrt(b) + 10) / sqrt(b)) ^ (0.6 * sqrt(b)).
pub fn defeat_xp(enemy_level: u32) -> f64 {
    let r = (enemy_level.max(1) as f64).sqrt();
    5.0 * ((r + 10.0) / r).powf(0.6 * r)
}

/// Which class-tier band a level falls in, counted from 0: 1-4, 5-24, 25-124, 125-624, 625-3124, 3125-15624, 15625.
pub fn tier_of_level(level: u32) -> usize {
    let mut t = 0;
    while t < CLASS_TIERS.len() - 1 && level as u64 >= 5u64.pow(t as u32 + 1) {
        t += 1;
    }
    t
}

fn band(tier: usize) -> (u64, u64) {
    (if tier == 0 { 1 } else { 5u64.pow(tier as u32) }, 5u64.pow(tier as u32 + 1))
}

/// t = 1.5 * (class tier + 1) / (1 + 0.25 * species tier), tiers counted from 0.
pub fn tier_wave(class_tier: usize, species_tier: usize) -> f64 {
    1.5 * (class_tier as f64 + 1.0) / (1.0 + 0.25 * species_tier as f64)
}

/// How much an enemy's level, against the character's, changes the experience (Wolfgang, 2026-11-13).
pub fn difference_multiplier(char_level: u32, enemy_level: u32, class_tier: usize, species_tier: usize) -> f64 {
    if enemy_level == char_level {
        return 1.0;
    }
    let (ct, et) = (tier_of_level(char_level), tier_of_level(enemy_level));
    let m = if ct == et {
        let (lo, hi) = band(ct);
        let q = (enemy_level as f64 - char_level as f64).abs() / (hi - lo).max(1) as f64;
        let m = if q <= 0.25 {
            1.0 + 4.0 * q
        } else if q <= 0.75 {
            2.0 + 2.0 * (q - 0.25)
        } else {
            3.0 + 4.0 * (q - 0.75)
        };
        m.min(4.0)
    } else {
        let (lo, hi) = band(et);
        let span = (hi - lo).max(1) as f64;
        let into = if enemy_level > char_level { (enemy_level as f64 - lo as f64) / span } else { (hi as f64 - 1.0 - enemy_level as f64) / span };
        let fifths = (into * 5.0 + 1e-9) as i64 + 5 * ((et as i64 - ct as i64).abs() - 1);
        4.0 * 2f64.powi(fifths.min(200) as i32)
    };
    if enemy_level > char_level {
        m
    } else {
        1.0 / m.powf(1.5 / tier_wave(class_tier, species_tier))
    }
}

pub fn class_tier_level(tier: usize) -> u64 {
    5u64.pow(tier as u32 + 1)
}
pub fn species_tier_level(tier: usize) -> u64 {
    25u64.pow(tier as u32 + 1)
}
pub fn skill_tier_level(tier: usize) -> u64 {
    5u64.pow(tier as u32 + 1)
}
/// e = (50 ^ d) * f
pub fn racial_skill_level(species_tier: usize, held: usize) -> u64 {
    50u64.pow(species_tier as u32 + 1) * held as u64
}
/// g = (5 ^ b) * h
pub fn class_skill_level(class_tier: usize, held: usize) -> u64 {
    5u64.pow(class_tier as u32 + 1) * held as u64
}
/// Revival skills are semi-common: 1 in 4 with none held, half as likely again for each one held.
pub fn revival_share(held: usize) -> f64 {
    REVIVAL_SHARE / 2f64.powi(held as i32)
}

fn feat_bar(kind: &str) -> Option<f64> {
    FEAT_ODDS.iter().find(|(k, _)| *k == kind).map(|(_, v)| *v)
}

// ------------------------------------------------------------------ data (Spirit's data/*.json, copied as they are)

pub struct Freq {
    pub name: String,
    pub spectrum: String,
    pub does: String,
}

pub struct Builtin {
    pub species: Map<String, Value>,
    pub classes: Map<String, Value>,
    pub skills: Map<String, Value>,
    pub freq: BTreeMap<i32, Freq>,
    /// "5", "5.5" -> the semi skill.
    pub semi: BTreeMap<String, Value>,
}

pub fn builtin() -> &'static Builtin {
    static B: OnceLock<Builtin> = OnceLock::new();
    B.get_or_init(|| {
        let obj = |s: &str| serde_json::from_str::<Value>(s).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
        let fq: Value = serde_json::from_str(include_str!("../ad/frequencies.json")).unwrap_or(Value::Null);
        let freq = fq["frequencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| {
                Some((f["place"].as_i64()? as i32, Freq { name: f["name"].as_str()?.into(), spectrum: f["spectrum"].as_str()?.into(), does: f["does"].as_str()?.into() }))
            })
            .collect();
        let semi = fq["semi_skills"].as_object().into_iter().flatten().filter(|(k, _)| !k.starts_with('_')).map(|(k, v)| (k.clone(), v.clone())).collect();
        Builtin { species: obj(include_str!("../ad/species.json")), classes: obj(include_str!("../ad/classes.json")), skills: obj(include_str!("../ad/skills.json")), freq, semi }
    })
}

/// How Wolfgang writes a number: 0-10 as is, then "10 + 1", "0 - 2".
pub fn label(place: i32) -> String {
    if place > 10 {
        format!("10 + {}", place - 10)
    } else if place < 0 {
        format!("0 - {}", -place)
    } else {
        place.to_string()
    }
}

fn freq(place: i32) -> &'static Freq {
    static NONE: OnceLock<Freq> = OnceLock::new();
    builtin().freq.get(&place).unwrap_or_else(|| NONE.get_or_init(|| Freq { name: "unknown".into(), spectrum: "unknown".into(), does: String::new() }))
}

pub fn describe(place: i32) -> String {
    let f = freq(place);
    format!("{} ({}), {}. {}", label(place), f.name, f.spectrum, f.does)
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The whole frequency a semi skill draws on (a half place draws on its spectrum's: 5.5 on 5).
fn semi_base(key: &str) -> i32 {
    builtin().semi.get(key).and_then(|d| d["of"].as_i64()).map(|v| v as i32).or_else(|| key.parse().ok()).unwrap_or(5)
}

/// "Heat semi skill", or a half place's own name ("Hardening semi skill").
pub fn semi_name(key: &str) -> String {
    if let Some(n) = builtin().semi.get(key).and_then(|d| d["name"].as_str()) {
        return n.to_string();
    }
    format!("{} semi skill", capital(&freq(semi_base(key)).spectrum))
}

fn semi_skills_data() -> Map<String, Value> {
    builtin()
        .semi
        .iter()
        .map(|(k, d)| {
            let f = freq(semi_base(k));
            let how = if d["lasting"].as_bool() == Some(true) {
                "put on someone, and kept on by upkeep"
            } else if d["effect"] == "analyse" {
                "aimed at something"
            } else {
                "thrown"
            };
            let about = format!("A basic use of the {} spectrum ({}): {}, {how}. Weak, but basic semi skills can be projectiled for attack.", f.spectrum, f.name, d["what"].as_str().unwrap_or(""));
            (semi_name(k), json!({"stat": "INT", "semi": k, "about": about}))
        })
        .collect()
}

/// Which frequency a name means: its number ("6", "10 + 1", "11"), name ("blue") or spectrum ("liquid").
pub fn place_from(text: &str) -> Option<i32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches("semi skill").trim().trim_end_matches(" frequency").trim();
    if let Ok(n) = t.replace(' ', "").parse::<i32>() {
        return builtin().freq.contains_key(&n).then_some(n);
    }
    let compact = t.replace(' ', "");
    builtin().freq.iter().find(|(p, f)| f.name.to_lowercase() == t || f.spectrum.to_lowercase() == t || label(**p).replace(' ', "") == compact).map(|(p, _)| *p)
}

/// Which semi skill a name means, among the frequencies held: a number, name, spectrum, or a semi skill's name.
pub fn semi_from(text: &str, held: &[i32]) -> Option<String> {
    let t = text.trim().to_lowercase().replace("semi skill", "").trim().to_string();
    for k in builtin().semi.keys() {
        if !held.contains(&semi_base(k)) {
            continue;
        }
        let mut names = vec![k.clone(), semi_name(k).to_lowercase().replace(" semi skill", "")];
        if let Ok(p) = k.parse::<i32>() {
            let f = freq(p);
            names.extend([f.name.to_lowercase(), f.spectrum.to_lowercase(), label(p).to_lowercase()]);
        }
        if names.iter().any(|n| n.trim() == t) {
            return Some(k.clone());
        }
    }
    None
}

/// What the System has defined while the game runs, on top of Spirit's built-in lists. At once there
/// for everyone, as in Spirit.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Defs {
    pub species: Map<String, Value>,
    pub classes: Map<String, Value>,
    pub skills: Map<String, Value>,
}

impl Defs {
    pub fn species(&self, k: &str) -> Option<&Value> {
        self.species.get(k).or_else(|| builtin().species.get(k))
    }
    pub fn class(&self, k: &str) -> Option<&Value> {
        self.classes.get(k).or_else(|| builtin().classes.get(k))
    }
    pub fn skill(&self, k: &str) -> Option<Value> {
        self.skills.get(k).or_else(|| builtin().skills.get(k)).cloned().or_else(|| semi_skills_data().get(k).cloned())
    }
    /// A skill's real name from however it is written.
    pub fn skill_key(&self, name: &str) -> Option<String> {
        let n = name.trim();
        if self.skill(n).is_some() {
            return Some(n.to_string());
        }
        self.skills.keys().chain(builtin().skills.keys()).cloned().chain(semi_skills_data().keys().cloned()).find(|k| k.eq_ignore_ascii_case(n))
    }
    pub fn class_key(&self, name: &str) -> Option<String> {
        let n = name.trim();
        self.classes
            .iter()
            .chain(builtin().classes.iter())
            .find(|(k, v)| k.eq_ignore_ascii_case(n) || v["name"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(n)))
            .map(|(k, _)| k.clone())
    }
    pub fn species_key(&self, name: &str) -> Option<String> {
        let n = name.trim();
        self.species
            .iter()
            .chain(builtin().species.iter())
            .find(|(k, v)| k.eq_ignore_ascii_case(n) || v["name"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(n)))
            .map(|(k, _)| k.clone())
    }
    fn revival(&self) -> Vec<String> {
        let mut v: Vec<String> = self.skills.iter().chain(builtin().skills.iter()).filter(|(_, s)| s["revival"].as_bool() == Some(true)).map(|(k, _)| k.clone()).collect();
        v.sort();
        v.dedup();
        v
    }
}

fn num_of(v: &Value, k: &str) -> i64 {
    v[k].as_i64().or_else(|| v[k].as_f64().map(|f| f as i64)).unwrap_or(0)
}

// ------------------------------------------------------------------ the soul prism (frequency.py)

/// The chance of starting with a number: 1 in 10 ^ (places from 5). Only 0-10 can be started with.
pub fn odds(place: i32) -> f64 {
    if (0..=10).contains(&place) {
        1.0 / 10f64.powi((place - 5).abs())
    } else {
        0.0
    }
}

/// A fixed number in [0, 1) from a seed.
fn unit(seed: &str) -> f64 {
    let h = Sha256::digest(seed.as_bytes());
    let mut b = [0u8; 8];
    b.copy_from_slice(&h[..8]);
    (u64::from_be_bytes(b) >> 11) as f64 / (1u64 << 53) as f64
}

/// A soul prism's spectrums: 5, and each other starting number rolled on its own. Nothing in between
/// is filled in (item 2). Death and revival never reroll it (item 51).
pub fn roll(name: &str) -> Vec<i32> {
    let mut v: Vec<i32> = (0..=10).filter(|p| *p == 5 || unit(&format!("prism:{}:{p}", name.trim().to_lowercase())) < odds(*p)).collect();
    v.sort();
    v
}

/// The connected runs of held numbers: [(minimum, maximum)], lowest first (item 43).
pub fn runs(held: &[i32]) -> Vec<(i32, i32)> {
    let mut h: Vec<i32> = held.to_vec();
    h.sort();
    h.dedup();
    let mut out: Vec<(i32, i32)> = Vec::new();
    for p in h {
        match out.last_mut() {
            Some(r) if p == r.1 + 1 => r.1 = p,
            _ => out.push((p, p)),
        }
    }
    out
}

/// What a species upgrade could open: one number just past either end of any run, within the table.
pub fn openings(held: &[i32]) -> Vec<i32> {
    let (lo, hi) = (*builtin().freq.keys().next().unwrap_or(&-3), *builtin().freq.keys().last().unwrap_or(&13));
    let mut out = BTreeSet::new();
    for (a, b) in runs(held) {
        for p in [a - 1, b + 1] {
            if lo <= p && p <= hi && !held.contains(&p) {
                out.insert(p);
            }
        }
    }
    out.into_iter().collect()
}

/// Amplitude gathered per minute in one frequency: INT x sqrt(max) ^ 0.1 x INT = INT^2 x max^(1/20), doubled resting.
pub fn rate(int_stat: i64, maximum: &BigInt, resting: bool) -> BigInt {
    let root = maximum.to_biguint().unwrap_or_default().nth_root(20);
    BigInt::from(int_stat) * BigInt::from(int_stat) * BigInt::from(root) * if resting { 2 } else { 1 }
}

/// What the surroundings do to a character holding these spectrums. `shares`: {place or "base": share};
/// None means balanced. Returns (regen factor per held place, unbalanced for this character, why).
pub fn environment(shares: Option<&BTreeMap<String, f64>>, held: &[i32]) -> (BTreeMap<i32, BigRational>, bool, String) {
    let Some(shares) = shares else {
        return (held.iter().map(|p| (*p, rat(1))).collect(), false, "balanced".into());
    };
    let shares: BTreeMap<&String, f64> = shares.iter().filter(|(_, v)| **v > 0.0).map(|(k, v)| (k, *v)).collect();
    if shares.is_empty() {
        return (held.iter().map(|p| (*p, rat(0))).collect(), false, "no frequency here".into());
    }
    let total: f64 = shares.values().sum();
    let top = shares.iter().filter(|(k, _)| k.as_str() != "base").max_by(|a, b| a.1.total_cmp(b.1)).map(|(k, v)| (k.parse::<i32>().unwrap_or(5), *v));
    let regen: BTreeMap<i32, BigRational> = held.iter().map(|p| (*p, if shares.get(&p.to_string()).is_some_and(|v| *v > 0.0) { rat(1) } else { rat(0) })).collect();
    match top {
        Some((t, v)) if v / total > UNBALANCED && !held.contains(&t) => {
            (regen.into_iter().map(|(p, f)| (p, f * ratio(1, 2))).collect(), true, format!("unbalanced: mostly {}", freq(t).spectrum))
        }
        Some((t, v)) if v / total > UNBALANCED => (regen, false, format!("mostly {}, which you are tuned for", freq(t).spectrum)),
        _ => (regen, false, "balanced".into()),
    }
}

/// A character's soul prism: the spectrums tuned to it, and what each frequency holds now.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Prism {
    pub held: Vec<i32>,
    /// place -> amplitude held now; filled to the maximum on first use.
    pub amplitude: BTreeMap<i32, Big>,
    /// place -> share of its maximum lost to overuse.
    pub decrease: BTreeMap<i32, Big>,
    pub cracked: bool,
}

pub struct Spent {
    pub overdrawn: BigRational,
    pub decrease_percent: f64,
    pub pain: f64,
    pub blocked: bool,
}

impl Prism {
    pub fn new(name: &str) -> Prism {
        let held = roll(name);
        Prism { decrease: held.iter().map(|p| (*p, Big::default())).collect(), held, ..Default::default() }
    }
    fn dec(&self, place: i32) -> BigRational {
        self.decrease.get(&place).map(|b| b.0.clone()).unwrap_or_else(BigRational::zero)
    }
    /// The most this frequency can hold now: the maximum less what overuse has taken.
    pub fn top(&self, place: i32, maximum: &BigInt) -> BigInt {
        let v = (BigRational::from_integer(maximum.clone()) * (rat(1) - self.dec(place))).floor().to_integer();
        v.max(BigInt::zero())
    }
    pub fn blocked(&self, place: i32) -> bool {
        self.dec(place) >= rat(1)
    }
    pub fn fill(&mut self, maximum: &BigInt) {
        for p in self.held.clone() {
            if !self.amplitude.contains_key(&p) {
                let t = self.top(p, maximum);
                self.amplitude.insert(p, Big::int(t));
            }
        }
    }
    pub fn have(&self, place: i32) -> BigRational {
        self.amplitude.get(&place).map(|b| b.0.clone()).unwrap_or_else(BigRational::zero)
    }
    /// Gather amplitude for `minutes` from the surroundings. Returns what was gained per place.
    pub fn regen(&mut self, minutes: &BigRational, int_stat: i64, maximum: &BigInt, shares: Option<&BTreeMap<String, f64>>, resting: bool) -> BTreeMap<i32, BigRational> {
        self.fill(maximum);
        let (factor, _, _) = environment(shares, &self.held);
        let per = BigRational::from_integer(rate(int_stat, maximum, resting)) * minutes;
        let mut got = BTreeMap::new();
        for p in self.held.clone() {
            if self.blocked(p) {
                continue;
            }
            let cap = BigRational::from_integer(self.top(p, maximum));
            let had = self.have(p);
            let new = (had.clone() + per.clone() * factor.get(&p).cloned().unwrap_or_else(|| rat(1))).min(cap);
            got.insert(p, new.clone() - had);
            self.amplitude.insert(p, Big(new));
        }
        got
    }
    /// Use `amount` of a frequency. More than it holds is overuse: its maximum decreases and it hurts.
    pub fn spend(&mut self, place: i32, amount: &BigRational, maximum: &BigInt) -> Result<Spent, String> {
        if !self.held.contains(&place) {
            return Err("not a spectrum you can use".into());
        }
        if self.blocked(place) {
            return Err("blocked by overuse".into());
        }
        self.fill(maximum);
        let have = self.have(place);
        let over = (amount.clone() - have.clone()).max(rat(0));
        self.amplitude.insert(place, Big((have - amount.clone()).max(rat(0))));
        let mut out = Spent { overdrawn: over.clone(), decrease_percent: 0.0, pain: 0.0, blocked: false };
        if over.is_positive() {
            let m = BigRational::from_integer(maximum.clone().max(BigInt::one()));
            let share = (over / m).min(rat(1));
            let d = (self.dec(place) + share.clone()).min(rat(1));
            self.decrease.insert(place, Big(d));
            let t = BigRational::from_integer(self.top(place, maximum));
            let now = self.have(place).min(t);
            self.amplitude.insert(place, Big(now));
            let s = share.to_f64().unwrap_or(1.0);
            out.decrease_percent = s * 100.0;
            out.pain = OVERUSE_PAIN_MIN.max(s * OVERUSE_PAIN);
            out.blocked = self.blocked(place);
        }
        Ok(out)
    }
    /// A species upgrade opens this number (it must be one of `openings`).
    pub fn extend(&mut self, place: i32) -> bool {
        if !openings(&self.held).contains(&place) {
            return false;
        }
        self.held.push(place);
        self.held.sort();
        self.decrease.insert(place, Big::default());
        true
    }
}

/// Semi skill power: base x log10(1 + amplitude) / 2 x (1 + 0.05 x skill level), after the half
/// place's impurity and the taxing spectrums' cost (taxing x3, very taxing x10, incredibly taxing x30).
pub fn semi_power(key: &str, amount: &BigRational, level: u32) -> f64 {
    let d = builtin().semi.get(key).cloned().unwrap_or(Value::Null);
    let impure = d["impure"].as_f64().unwrap_or(0.0);
    let taxing = d["taxing"].as_i64().unwrap_or(1).max(1);
    let amt = amount.floor().to_integer();
    // log10 of a number of any size, without turning it into a float first
    let kept: BigInt = (BigRational::from_integer(amt) * BigRational::from_float(1.0 - impure).unwrap_or_else(|| rat(1))).floor().to_integer() / BigInt::from(taxing);
    let kept: BigInt = kept.max(BigInt::zero()) + BigInt::one();
    let lg = if kept.bits() < 1000 { kept.to_f64().unwrap_or(1.0).log10() } else { (kept.bits() as f64 - 1.0) * 0.30102999566398 };
    d["power"].as_f64().unwrap_or(0.0) * lg / 2.0 * (1.0 + 0.05 * level as f64)
}

/// Items 13 and 48: 10x what the person has remaining, and when exhausted or near it, 10% of their maximum.
pub fn overpower_needed(have: &BigRational, most: &BigRational) -> BigRational {
    (have.clone() * rat(10)).max(most.clone() / rat(10))
}

/// Item 24, Spirit's fill-in: the k-th payment every 10 s is 10% x 2^k of what was first put in.
pub fn upkeep(amount: &BigRational, k: u32) -> BigRational {
    amount.clone() * ratio(1, 10) * BigRational::from_integer(num_traits::pow(BigInt::from(2), k as usize))
}

// ------------------------------------------------------------------ crafting (his file: lenses, elements)

/// Item 62: loss = distance from the intended whole number x 100% (6.25 used as 6 = 25%, as 7 = 75%).
pub fn impurity(place: f64, used_as: i32) -> f64 {
    ((place - used_as as f64).abs()).min(1.0)
}

/// What a lens does to a frequency, in places from 5. Returns (place out, amplitude out) for each output.
pub fn lens(op: &str, a: f64, b: Option<f64>, amp: &BigRational, amp_b: Option<&BigRational>, steps: u32) -> Result<Vec<(f64, BigRational)>, String> {
    let from5 = |p: f64| p - 5.0;
    match op {
        // "Merges 2 frequencies together then divided in half to get a new one thats worth the amplitude of the two"
        "merge" | "merging" => {
            let b = b.ok_or("merging needs two frequencies in")?;
            Ok(vec![((from5(a) + from5(b)) / 2.0 + 5.0, amp.clone() + amp_b.cloned().unwrap_or_else(|| amp.clone()))])
        }
        // "Frequency is divided in half to get a new one thats worth half the amplitude" (two out)
        "divide" | "division" => {
            let p = from5(a) / 2.0 + 5.0;
            let half = amp.clone() / rat(2);
            Ok(vec![(p, half.clone()), (p, half)])
        }
        // "Raises the frequency by 1 for the divide amplitude by 10 per step up" (above 5; never to quantum)
        "compress" | "compression" => {
            if a < 5.0 {
                return Err("compression is for 5 and above; stretching goes below 5".into());
            }
            let p = a + steps as f64;
            if p >= 13.0 {
                return Err("quantum (10 + 3) can't be reached by compression (item 59)".into());
            }
            Ok(vec![(p, amp.clone() / BigRational::from_integer(pow10(steps)))])
        }
        "stretch" | "stretching" => {
            if a > 5.0 {
                return Err("stretching is for 5 and below; compression goes above 5".into());
            }
            let p = a - steps as f64;
            if p <= -3.0 {
                return Err("gravitational (0 - 3) can't be reached by stretching (item 59)".into());
            }
            Ok(vec![(p, amp.clone() / BigRational::from_integer(pow10(steps)))])
        }
        // "Goes to that many places the frequency is in the opposite direction from 5"
        "opposite" => Ok(vec![(10.0 - a, amp.clone())]),
        "inversion" | "invert" => Ok(vec![(a, -amp.clone())]),
        // "Puts frequencies safely back into the world in the base frequency"
        "dissipation" | "dissipate" => Ok(vec![]),
        other => Err(format!("unknown lens '{other}': merge, divide, compress, stretch, opposite, inversion, dissipation")),
    }
}

// ------------------------------------------------------------------ the sheet (system.py `Sheet`)

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Ability {
    pub name: String,
    pub about: String,
    pub effects: BTreeMap<String, f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Skill {
    pub level: u32,
    pub xp: Big,
    pub tier: usize,
    /// general, racial or class
    pub kind: String,
    pub abilities: Vec<Ability>,
    /// Use since the System last looked.
    pub used: f64,
}

/// One of the five paths of a tier-up.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct PathOpt {
    pub name: String,
    pub about: String,
    pub unique: bool,
    /// A built-in or defined class / species to become.
    pub key: Option<String>,
    /// The frequency a species path opens.
    pub frequency: Option<i32>,
    pub stats: BTreeMap<String, i64>,
    pub effects: BTreeMap<String, f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Feat {
    pub name: String,
    pub about: String,
    pub rewards: Vec<String>,
    pub chance: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Item {
    pub name: String,
    pub qty: i32,
    pub about: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Sheet {
    pub name: String,
    pub species: String,
    pub cls: Option<String>,
    pub class_tier: usize,
    pub species_tier: usize,
    pub level: u32,
    pub xp: Big,
    /// Past the cap: bonus points earned in all, not yet spent, and 0.1% steps bought per stat.
    pub bp: u64,
    pub bp_free: u64,
    pub stat_pct: BTreeMap<String, u64>,
    pub skills: BTreeMap<String, Skill>,
    /// Uses of skills the character doesn't have yet.
    pub attempts: BTreeMap<String, f64>,
    pub titles: Vec<String>,
    pub offered: Vec<String>,
    pub told: BTreeSet<String>,
    pub feats: Vec<Feat>,
    /// Tiers past the last, each named by the System.
    pub beyond: BTreeMap<String, Vec<String>>,
    /// Open tier-ups: "class" | "species" | "skill:<name>" -> five paths.
    pub pending: BTreeMap<String, Vec<PathOpt>>,
    /// What this character's class, species and skills are called now.
    pub names: BTreeMap<String, String>,
    pub boosts: BTreeMap<String, f64>,
    pub base: BTreeMap<String, i64>,
    pub grown: BTreeMap<String, i64>,
    pub adjust: BTreeMap<String, i64>,
    pub prism: Prism,
    /// Where the character is: {place or "base": share}; None = balanced; empty = an alien zone.
    pub shares: Option<BTreeMap<String, f64>>,
    pub env_unbalanced: bool,
    pub env_why: String,
    pub chosen: Vec<Value>,
    /// Who may use frequencies on this character without overpowering them.
    pub consent: Vec<String>,
    /// Overuse pain, fading.
    pub pain: f64,
    pub crafted: Vec<String>,
    pub money: i64,
    pub inventory: Vec<Item>,
    pub conditions: Vec<String>,
    /// Bonus point purchases asked for that need a tier name from the System: "class_tier", "skill_tier:<skill>".
    pub bp_asked: Vec<String>,
    #[serde(skip)]
    pub messages: Vec<String>,
}

fn zero_stats() -> BTreeMap<String, i64> {
    STATS.iter().map(|s| (s.to_string(), 0)).collect()
}

impl Sheet {
    pub fn new(name: &str, species: &str, base: BTreeMap<String, i64>, defs: &Defs) -> Sheet {
        let species = defs.species_key(species).unwrap_or_else(|| "human".into());
        let mut s = Sheet {
            name: name.trim().to_string(),
            species,
            level: 1,
            base: STATS.iter().map(|k| (k.to_string(), base.get(*k).copied().unwrap_or(10).max(1))).collect(),
            grown: zero_stats(),
            adjust: zero_stats(),
            stat_pct: STATS.iter().map(|s| (s.to_string(), 0)).collect(),
            prism: Prism::new(name),
            env_why: "balanced".into(),
            ..Default::default()
        };
        let sp = defs.species(&s.species).and_then(|v| v["name"].as_str()).unwrap_or("Human").to_string();
        s.say(&format!("Welcome, {}. Species: {sp} (base). Level 1. Class: none (null).", s.name));
        s.check_slots(defs);
        s
    }

    fn say(&mut self, text: &str) {
        self.messages.push(format!("[System] {text}"));
    }

    pub fn drain(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    fn get(m: &BTreeMap<String, i64>, k: &str) -> i64 {
        m.get(k).copied().unwrap_or(0)
    }

    /// What one level adds: the species' set amounts plus the class's.
    pub fn growth(&self, defs: &Defs) -> BTreeMap<String, i64> {
        let sp = defs.species(&self.species);
        let mut g: BTreeMap<String, i64> = match sp.and_then(|v| v["growth"].as_object()) {
            Some(m) => STATS.iter().map(|s| (s.to_string(), m.get(*s).and_then(Value::as_i64).unwrap_or(0))).collect(),
            None => DEFAULT_GROWTH.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        };
        if let Some(c) = self.cls.as_deref().and_then(|c| defs.class(c)) {
            for s in STATS {
                *g.entry(s.into()).or_insert(0) += num_of(&c["growth"], s);
            }
        }
        g
    }

    pub fn true_stat(&self, s: &str, defs: &Defs) -> i64 {
        let mut v = Self::get(&self.base, s) + Self::get(&self.grown, s) + Self::get(&self.adjust, s);
        if let Some(sp) = defs.species(&self.species) {
            v += num_of(&sp["stats"], s) * (self.species_tier as i64).max(1);
        }
        if let Some(c) = self.cls.as_deref().and_then(|c| defs.class(c)) {
            v += num_of(&c["stats"], s) * (self.class_tier as i64).max(1);
        }
        let pct = self.stat_pct.get(s).copied().unwrap_or(0) as i64;
        let v = if pct > 0 { (v * (1000 + pct)).div_euclid(1000).max(1) } else { v.max(1) };
        // an unbalanced environment: every stat x0.8
        if self.env_unbalanced {
            (v * 4 / 5).max(1)
        } else {
            v
        }
    }

    pub fn stats(&self, defs: &Defs) -> BTreeMap<String, i64> {
        STATS.iter().map(|s| (s.to_string(), self.true_stat(s, defs))).collect()
    }

    /// [(name, exact value or why it can't be worked out)] for amplitude, speed, stamina and whatever the species and class add.
    pub fn derived(&self, defs: &Defs) -> Vec<(String, Result<BigRational, String>)> {
        let mut forms: Vec<(String, String)> = DERIVED.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let mut add = |m: Option<&Map<String, Value>>| {
            for (k, v) in m.into_iter().flatten() {
                let t = v.as_str().unwrap_or("").to_string();
                match forms.iter_mut().find(|(n, _)| n == k) {
                    Some(f) => f.1 = t,
                    None => forms.push((k.clone(), t)),
                }
            }
        };
        add(defs.species(&self.species).and_then(|v| v["derived"].as_object()));
        add(self.cls.as_deref().and_then(|c| defs.class(c)).and_then(|v| v["derived"].as_object()));
        let st = self.stats(defs);
        forms.into_iter().map(|(n, f)| { let v = formula(&f, &st).map_err(|e| format!("can't be worked out: {e}")); (n, v) }).collect()
    }

    pub fn amplitude_max(&self, defs: &Defs) -> BigInt {
        let st = self.stats(defs);
        let mut f = DERIVED[0].1.to_string();
        for v in [defs.species(&self.species), self.cls.as_deref().and_then(|c| defs.class(c))].into_iter().flatten() {
            if let Some(t) = v["derived"]["amplitude"].as_str() {
                f = t.to_string();
            }
        }
        formula(&f, &st).map(|v| v.floor().to_integer()).unwrap_or_else(|_| BigInt::zero())
    }

    pub fn trait_of(&self, key: &str, default: f64, defs: &Defs) -> f64 {
        let mut v = defs.species(&self.species).and_then(|s| s["traits"][key].as_f64()).unwrap_or(default);
        if let Some(c) = self.cls.as_deref().and_then(|c| defs.class(c)) {
            v = c["traits"][key].as_f64().unwrap_or(v);
        }
        v
    }

    pub fn skill_key(&self, name: &str) -> Option<String> {
        self.skills.keys().find(|k| k.eq_ignore_ascii_case(name.trim())).cloned()
    }

    /// The character did something a skill covers: only RECORDED, for the System to judge.
    pub fn use_skill(&mut self, skill: &str, amount: f64, defs: &Defs) {
        let Some(skill) = defs.skill_key(skill) else { return };
        let amount = amount * self.trait_of("learning", 1.0, defs);
        match self.skills.get_mut(&skill) {
            Some(s) => s.used += amount,
            None => *self.attempts.entry(skill).or_insert(0.0) += amount,
        }
    }

    /// Give a skill (or raise it to a level). kind: general, racial or class.
    pub fn learn(&mut self, skill: &str, kind: &str, level: u32, defs: &Defs) -> bool {
        let Some(skill) = defs.skill_key(skill) else { return false };
        let level = level.max(1);
        if let Some(had) = self.skills.get_mut(&skill) {
            had.level = had.level.max(level);
            let l = had.level;
            self.say(&format!("Skill raised: {skill} (level {l})."));
        } else {
            let kind = if ["general", "racial", "class"].contains(&kind) { kind } else { "general" };
            let used = self.attempts.remove(&skill).unwrap_or(0.0);
            self.skills.insert(skill.clone(), Skill { level, kind: kind.into(), used, ..Default::default() });
            let label = match kind {
                "racial" => "Racial skill",
                "class" => "Class skill",
                _ => "Skill",
            };
            let about = defs.skill(&skill).and_then(|v| v["about"].as_str().map(String::from)).unwrap_or_default();
            self.say(format!("{label} learned: {skill} (level {level}, unskilled). {about}").trim());
        }
        self.check_skill_tier(&skill);
        true
    }

    /// Experience for one skill (its own level, on the same curve as the character's).
    pub fn skill_xp(&mut self, skill: &str, xp: BigInt, why: Option<&str>) -> bool {
        let Some(key) = self.skill_key(skill) else { return false };
        let Some(sk) = self.skills.get_mut(&key) else { return false };
        sk.xp.0 += BigRational::from_integer(xp.max(BigInt::zero()));
        let mut ups = 0;
        while sk.level < MAX_LEVEL && sk.xp.0 >= rat(level_xp_needed(sk.level) as i64) {
            sk.xp.0 -= rat(level_xp_needed(sk.level) as i64);
            sk.level += 1;
            ups += 1;
        }
        if ups > 0 {
            let l = sk.level;
            self.say(&format!("Skill level up: {key} is now level {l}.{}", why.map(|w| format!(" ({w})")).unwrap_or_default()));
            self.check_skill_tier(&key);
        }
        true
    }

    pub fn can_tier_skill(&self, skill: &str) -> bool {
        self.skills.get(skill).is_some_and(|s| s.tier < SKILL_TIERS.len() - 1 && s.level as u64 >= skill_tier_level(s.tier))
    }

    fn tier_skill(&mut self, skill: &str, ab: Ability) -> bool {
        if !self.can_tier_skill(skill) {
            return false;
        }
        let Some(sk) = self.skills.get_mut(skill) else { return false };
        sk.tier += 1;
        let ab = Ability { name: ab.name.chars().take(60).collect(), about: ab.about.chars().take(200).collect(), effects: ab.effects };
        let msg = format!("SKILL EVOLUTION: {skill} is now {}. New ability: {}. {}", SKILL_TIERS[sk.tier], ab.name, ab.about);
        sk.abilities.push(ab);
        self.say(msg.trim());
        true
    }

    fn tell_once(&mut self, mark: String, text: &str) {
        if self.told.insert(mark) {
            self.say(text);
        }
    }

    fn check_skill_tier(&mut self, skill: &str) {
        if self.can_tier_skill(skill) {
            let t = self.skills[skill].tier;
            self.tell_once(format!("skill|{skill}|{t}"), &format!("{skill} is ready to evolve. The System is considering what it has become."));
        }
    }

    pub fn gain(&mut self, xp: BigInt, why: Option<&str>, defs: &Defs) {
        let xp = xp.max(BigInt::zero());
        self.xp.0 += BigRational::from_integer(xp.clone());
        if let Some(w) = why {
            self.say(&format!("+{} experience: {w}.", short_int(&xp)));
        }
        let before = self.stats(defs);
        let mut ups = 0;
        while self.level < MAX_LEVEL && self.xp.0 >= rat(level_xp_needed(self.level) as i64) {
            self.xp.0 -= rat(level_xp_needed(self.level) as i64);
            self.level += 1;
            for (s, v) in self.growth(defs) {
                *self.grown.entry(s).or_insert(0) += v;
            }
            ups += 1;
        }
        if self.level >= MAX_LEVEL {
            // past the cap: no more levels; experience buys bonus points
            let mut got: u64 = 0;
            while got < 20000 && self.xp.0 >= rat(bonus_xp_needed(self.bp) as i64) {
                self.xp.0 -= rat(bonus_xp_needed(self.bp) as i64);
                self.bp += 1;
                self.bp_free += 1;
                got += 1;
            }
            let cost = rat(bonus_xp_needed(self.bp).max(1) as i64);
            if self.xp.0 >= cost {
                // an enormous award: the curve has levelled off by now, so the rest is counted in one go
                let more = (self.xp.0.clone() / cost.clone()).floor();
                self.xp.0 -= more.clone() * cost;
                let more = more.to_integer().to_u64().unwrap_or(u64::MAX / 4);
                self.bp = self.bp.saturating_add(more);
                self.bp_free = self.bp_free.saturating_add(more);
                got = got.saturating_add(more);
            }
            if got > 0 {
                let (bp, free) = (self.bp, self.bp_free);
                self.say(&format!("Level {MAX_LEVEL} Bp{bp}. +{got} bonus point{} ({free} to spend).", if got > 1 { "s" } else { "" }));
            }
        }
        if ups > 0 {
            let after = self.stats(defs);
            let changes: Vec<String> = STATS.iter().filter(|s| after[**s] != before[**s]).map(|s| format!("{s} +{}", after[*s] - before[*s])).collect();
            let lvl = self.level;
            self.say(&format!("LEVEL UP{}. You are now level {lvl}. {}.", if ups > 1 { format!(" x{ups}") } else { String::new() }, changes.join(", ")));
            self.check_tiers();
            self.check_slots(defs);
        }
    }

    fn revival_held(&self, defs: &Defs) -> usize {
        self.skills.keys().filter(|s| defs.skill(s).is_some_and(|v| v["revival"].as_bool() == Some(true))).count()
    }
    fn racial_held(&self) -> usize {
        self.skills.values().filter(|s| s.kind == "racial").count()
    }
    fn class_held(&self) -> usize {
        self.skills.values().filter(|s| s.kind == "class").count()
    }
    pub fn racial_open(&self) -> bool {
        self.level < MAX_LEVEL && self.level as u64 >= racial_skill_level(self.species_tier, self.racial_held())
    }
    pub fn class_open(&self) -> bool {
        self.cls.is_some() && self.level < MAX_LEVEL && self.level as u64 >= class_skill_level(self.class_tier, self.class_held())
    }

    /// A racial or class skill slot that has opened: the character is told, and the System decides which.
    fn check_slots(&mut self, _defs: &Defs) {
        if self.racial_open() {
            let h = self.racial_held();
            self.tell_once(format!("racial|{h}"), "A racial skill has opened to you. The System is considering which.");
        }
        if self.class_open() {
            let h = self.class_held();
            self.tell_once(format!("class|{h}"), "A class skill has opened to you. The System is considering which.");
        }
    }

    pub fn can_evolve_class(&self) -> bool {
        self.cls.is_some() && self.class_tier < CLASS_TIERS.len() - 1 && self.level as u64 >= class_tier_level(self.class_tier)
    }
    pub fn can_evolve_species(&self) -> bool {
        self.species_tier < SPECIES_TIERS.len() - 1 && self.level as u64 >= species_tier_level(self.species_tier)
    }

    fn evolve_class(&mut self, key: Option<String>, defs: &Defs) -> bool {
        if !self.can_evolve_class() {
            return false;
        }
        self.class_tier += 1;
        if let Some(k) = key.filter(|k| defs.class(k).is_some()) {
            self.cls = Some(k);
        }
        let name = self.cls.as_deref().and_then(|c| defs.class(c)).and_then(|v| v["name"].as_str()).unwrap_or("").to_string();
        let next = if self.class_tier < CLASS_TIERS.len() - 1 { format!(" Next tier at level {}.", class_tier_level(self.class_tier)) } else { " This is the last tier.".into() };
        self.say(&format!("CLASS EVOLUTION: {name} is now {}.{next}", CLASS_TIERS[self.class_tier]));
        self.check_slots(defs);
        true
    }

    fn evolve_species(&mut self, key: Option<String>, frequency: Option<i32>, defs: &Defs) -> bool {
        if !self.can_evolve_species() {
            return false;
        }
        self.species_tier += 1;
        self.open_frequency(frequency);
        if let Some(k) = key.filter(|k| defs.species(k).is_some()) {
            self.species = k;
        }
        let name = defs.species(&self.species).and_then(|v| v["name"].as_str()).unwrap_or("").to_string();
        let next = if self.species_tier < SPECIES_TIERS.len() - 1 { format!(" Next tier at level {}.", species_tier_level(self.species_tier)) } else { " This is the last tier.".into() };
        self.say(&format!("SPECIES EVOLUTION: {name} is now {}.{next}", SPECIES_TIERS[self.species_tier]));
        self.check_slots(defs);
        true
    }

    fn check_tiers(&mut self) {
        if self.can_evolve_class() {
            let t = self.class_tier;
            self.tell_once(format!("class_tier|{t}"), "A class evolution is open to you. The System is considering what you have become.");
        }
        if self.can_evolve_species() {
            let t = self.species_tier;
            self.tell_once(format!("species_tier|{t}"), "A species evolution is open to you. The System is considering what you have become.");
        }
    }

    /// What this character's class, species or skill is called now.
    pub fn shown_name(&self, kind: &str, key: Option<&str>, defs: &Defs) -> String {
        let tag = if kind == "skill" { format!("skill:{}", key.unwrap_or("")) } else { kind.to_string() };
        if let Some(n) = self.names.get(&tag) {
            return n.clone();
        }
        match kind {
            "class" => self.cls.as_deref().and_then(|c| defs.class(c)).and_then(|v| v["name"].as_str()).unwrap_or("none").to_string(),
            "species" => defs.species(&self.species).and_then(|v| v["name"].as_str()).unwrap_or(&self.species).to_string(),
            _ => key.unwrap_or("").to_string(),
        }
    }

    fn ready(&self, kind: &str, key: Option<&str>) -> bool {
        match kind {
            "class" => self.can_evolve_class(),
            "species" => self.can_evolve_species(),
            _ => key.is_some_and(|k| self.can_tier_skill(k)),
        }
    }

    /// Open a tier-up as a choice of five paths. The first two are BASIC: the same two for every
    /// character that ever tiers up this class / species / skill at this tier. The last three are
    /// UNIQUE to this character. Every option needs a name and at least a sentence saying what it does.
    pub fn offer(&mut self, kind: &str, key: Option<&str>, options: Vec<PathOpt>, basics: &mut BTreeMap<String, Vec<PathOpt>>, defs: &Defs) -> Result<Vec<PathOpt>, String> {
        let tag = if kind == "skill" { format!("skill:{}", key.unwrap_or("")) } else { kind.to_string() };
        if !self.ready(kind, key) {
            return Err(format!("{tag} is not ready to rise a tier"));
        }
        if self.pending.contains_key(&tag) {
            return Err(format!("{tag} already has paths open"));
        }
        let base_key = match kind {
            "class" => self.cls.clone().unwrap_or_default(),
            "species" => self.species.clone(),
            _ => key.unwrap_or("").to_string(),
        };
        let tier = match kind {
            "class" => self.class_tier,
            "species" => self.species_tier,
            _ => self.skills.get(key.unwrap_or("")).map_or(0, |s| s.tier),
        };
        let store = format!("{kind}:{base_key}:{tier}");
        let mut opts: Vec<PathOpt> = options.into_iter().filter(|o| !o.name.trim().is_empty() && o.about.split_whitespace().count() >= 4).collect();
        match basics.get(&store) {
            Some(b) if opts.len() == 3 => opts.splice(0..0, b.iter().cloned()).for_each(drop),
            Some(b) if opts.len() >= 5 => opts.splice(0..2, b.iter().cloned()).for_each(drop),
            Some(_) => return Err("five paths are needed (or three, when the two basic ones are already set), each with a name and a sentence saying what it does".into()),
            None if opts.len() >= 5 => {
                basics.insert(store, opts[..2].to_vec());
            }
            None => return Err("five paths are needed, each with a name and a sentence saying what it does".into()),
        }
        opts.truncate(5);
        for (i, o) in opts.iter_mut().enumerate() {
            o.name = o.name.chars().take(60).collect();
            o.about = o.about.chars().take(240).collect();
            o.unique = i >= 2;
        }
        let name = self.shown_name(kind, key, defs);
        let list: Vec<String> = opts.iter().enumerate().map(|(i, o)| format!("{}. {}{}: {}", i + 1, o.name, if o.unique { " *" } else { "" }, o.about)).collect();
        self.pending.insert(tag, opts.clone());
        self.say(&format!("{name} can rise a tier. Five paths are open; the last three are yours alone: {}", list.join(" | ")));
        Ok(opts)
    }

    /// Take path `pick` (1-5) of the open tier-up `tag`. The thing rises a tier and is called by the path's name from then on.
    pub fn choose(&mut self, tag: &str, pick: usize, defs: &Defs) -> Result<PathOpt, String> {
        let tag = self.pending.keys().find(|k| k.eq_ignore_ascii_case(tag.trim())).cloned().ok_or_else(|| format!("no paths are open for {tag}"))?;
        let o = self.pending.get(&tag).and_then(|v| v.get(pick.wrapping_sub(1))).cloned().ok_or("pick a path from 1 to 5")?;
        let (kind, key) = match tag.split_once(':') {
            Some((k, rest)) => (k.to_string(), Some(rest.to_string())),
            None => (tag.clone(), None),
        };
        self.pending.remove(&tag);
        if !self.ready(&kind, key.as_deref()) {
            return Err("it is no longer ready to rise".into());
        }
        match kind.as_str() {
            "class" => {
                self.evolve_class(o.key.clone(), defs);
            }
            "species" => {
                self.evolve_species(o.key.clone(), o.frequency, defs);
            }
            _ => {
                self.tier_skill(key.as_deref().unwrap_or(""), Ability { name: o.name.clone(), about: o.about.clone(), effects: o.effects.clone() });
            }
        }
        self.names.insert(tag.clone(), o.name.clone());
        for (s, v) in &o.stats {
            let s = s.to_uppercase();
            if STATS.contains(&s.as_str()) {
                *self.adjust.entry(s).or_insert(0) += (*v).clamp(-5, 10);
            }
        }
        self.say(&format!("You chose {}. {}", o.name, o.about));
        self.chosen.push(json!({"what": tag, "path": o.name, "unique": o.unique}));
        if kind == "skill" {
            self.check_skill_tier(key.as_deref().unwrap_or(""));
        } else {
            self.check_tiers();
        }
        Ok(o)
    }

    /// The name of the tier a class, species or skill is in now.
    pub fn tier_name(&self, kind: &str, key: Option<&str>) -> String {
        let tag = if kind == "skill" { format!("skill:{}", key.unwrap_or("")) } else { kind.to_string() };
        if let Some(n) = self.beyond.get(&tag).and_then(|v| v.last()) {
            return n.clone();
        }
        match kind {
            "class" => CLASS_TIERS[self.class_tier].into(),
            "species" => SPECIES_TIERS[self.species_tier].into(),
            _ => SKILL_TIERS[self.skills.get(key.unwrap_or("")).map_or(0, |s| s.tier)].into(),
        }
    }

    /// A feat, awarded by the System for a deed. An upgrade is allowed only with `chance` at or under its bar.
    pub fn feat(&mut self, f: FeatArgs, defs: &Defs) -> Vec<String> {
        let mut gave = Vec::new();
        self.say(format!("FEAT: {}. {}", f.name, f.about).trim());
        if f.xp.is_positive() {
            self.gain(f.xp.clone(), Some(&format!("feat: {}", f.name)), defs);
            gave.push(format!("{} experience", short_int(&f.xp)));
        }
        for (sk, tier, kind) in &f.skills {
            let Some(sk) = defs.skill_key(sk) else { continue };
            if !self.skills.contains_key(&sk) {
                self.learn(&sk, kind, 1, defs);
            }
            let t = (*tier).min(SKILL_TIERS.len() - 1);
            if let Some(s) = self.skills.get_mut(&sk) {
                if t > s.tier {
                    s.tier = t;
                    self.say(&format!("{sk} is granted at the {} tier.", SKILL_TIERS[t]));
                }
            }
            gave.push(format!("skill {sk} ({})", SKILL_TIERS[self.skills.get(&sk).map_or(0, |s| s.tier)]));
        }
        let mut shown = Vec::new();
        for (k, v) in &f.boosts {
            *self.boosts.entry(k.clone()).or_insert(0.0) += v;
            gave.push(format!("boost {k} {v:+}"));
            shown.push(format!("boost {k} {v:+}"));
        }
        for (k, v) in &f.stats {
            let k = k.to_uppercase();
            if STATS.contains(&k.as_str()) {
                *self.adjust.entry(k.clone()).or_insert(0) += v;
                gave.push(format!("{k} {v:+}"));
                shown.push(format!("{k} {v:+}"));
            }
        }
        if !shown.is_empty() {
            self.say(&format!("Boost: {}.", shown.join(", ")));
        }
        let mut upgrade = f.upgrade.clone().filter(|u| feat_bar(u).is_some());
        if let Some(u) = upgrade.clone() {
            let bar = feat_bar(&u).unwrap_or(0.0);
            let odds = f.chance.unwrap_or(1.0);
            if !(0.0..=bar).contains(&odds) {
                gave.push(format!("no {u} upgrade: the feat's chance of completion ({odds}) is above the bar ({bar})"));
                upgrade = None;
            }
        }
        if let Some(u) = upgrade {
            // At the last tier a feat can still go further, into a tier the System names. Levels never can.
            let tag = if u == "skill" { format!("skill:{}", f.upgrade_skill.clone().unwrap_or_default()) } else { u.clone() };
            let skill = f.upgrade_skill.as_deref().and_then(|s| self.skill_key(s));
            let at_top = (u == "species" && self.species_tier == SPECIES_TIERS.len() - 1)
                || (u == "class" && self.cls.is_some() && self.class_tier == CLASS_TIERS.len() - 1)
                || (u == "skill" && skill.as_ref().is_some_and(|s| self.skills[s].tier == SKILL_TIERS.len() - 1));
            if at_top {
                match f.tier_name.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    Some(t) => {
                        let t: String = t.chars().take(40).collect();
                        let tag = if u == "skill" { format!("skill:{}", skill.clone().unwrap_or_default()) } else { tag };
                        self.beyond.entry(tag).or_default().push(t.clone());
                        let what = match u.as_str() {
                            "species" => self.shown_name("species", None, defs),
                            "class" => self.shown_name("class", None, defs),
                            _ => skill.clone().unwrap_or_default(),
                        };
                        self.say(&format!("BEYOND THE LAST TIER. {what} is now {t}."));
                        gave.push(format!("{u} raised past its last tier: {t}"));
                    }
                    None => gave.push(format!("no {u} upgrade: it is at its last tier, and a tier beyond needs a name from the System (tier_name)")),
                }
            } else if u == "species" && self.species_tier < SPECIES_TIERS.len() - 1 {
                self.species_tier += 1;
                let n = self.shown_name("species", None, defs);
                self.say(&format!("A FEAT BEYOND MEASURE. Species upgrade: {n} is now {}.", SPECIES_TIERS[self.species_tier]));
                gave.push(format!("species upgrade to {}", SPECIES_TIERS[self.species_tier]));
            } else if u == "class" && self.cls.is_some() && self.class_tier < CLASS_TIERS.len() - 1 {
                self.class_tier += 1;
                let n = self.shown_name("class", None, defs);
                self.say(&format!("A LEGENDARY FEAT. Class upgrade: {n} is now {}.", CLASS_TIERS[self.class_tier]));
                gave.push(format!("class upgrade to {}", CLASS_TIERS[self.class_tier]));
            } else if u == "skill" {
                let can: Vec<String> = self.skills.iter().filter(|(_, v)| v.tier < SKILL_TIERS.len() - 1).map(|(k, _)| k.clone()).collect();
                let pick = skill.filter(|s| can.contains(s)).or_else(|| {
                    can.iter().max_by(|a, b| (self.skills[*a].used + self.skills[*a].level as f64).total_cmp(&(self.skills[*b].used + self.skills[*b].level as f64))).cloned()
                });
                if let Some(p) = pick {
                    let s = self.skills.get_mut(&p).expect("held");
                    s.tier += 1;
                    let t = SKILL_TIERS[s.tier];
                    self.say(&format!("A RARE FEAT. Skill upgrade: {p} is now {t}."));
                    gave.push(format!("skill upgrade: {p} to {t}"));
                }
            }
            self.check_slots(defs);
        }
        self.feats.push(Feat { name: f.name.clone(), about: f.about.clone(), rewards: gave.clone(), chance: f.chance });
        gave
    }

    pub fn title(&mut self, t: &str, why: &str) {
        if !self.titles.iter().any(|x| x == t) {
            self.titles.push(t.to_string());
            self.say(&format!("Title earned: {t}.{}", if why.is_empty() { String::new() } else { format!(" {why}") }));
        }
    }

    /// The System offers a class (Spirit offers the built-in ones whose skill requirements are met).
    pub fn offer_class(&mut self, key: &str, defs: &Defs) -> Result<String, String> {
        if self.cls.is_some() {
            return Err("they already have a class".into());
        }
        let key = defs.class_key(key).ok_or_else(|| format!("no class '{key}': define it first"))?;
        if !self.offered.contains(&key) {
            self.offered.push(key.clone());
        }
        let c = defs.class(&key).cloned().unwrap_or(Value::Null);
        self.say(&format!("Class available: {}. {}", c["name"].as_str().unwrap_or(&key), c["about"].as_str().unwrap_or("")));
        Ok(key)
    }

    /// The character's choice among the classes offered. It starts in the class's first tier, null.
    pub fn choose_class(&mut self, key: &str, defs: &Defs) -> Result<(), String> {
        if self.cls.is_some() {
            return Err("they already have a class".into());
        }
        let key = self.offered.iter().find(|k| k.eq_ignore_ascii_case(key.trim()) || defs.class(k).and_then(|v| v["name"].as_str()).is_some_and(|n| n.eq_ignore_ascii_case(key.trim()))).cloned().ok_or("that class hasn't been offered")?;
        self.cls = Some(key.clone());
        let c = defs.class(&key).cloned().unwrap_or(Value::Null);
        let growth: Vec<String> = STATS.iter().filter(|s| num_of(&c["growth"], s) != 0).map(|s| format!("{s} +{}", num_of(&c["growth"], s))).collect();
        self.say(&format!("Class chosen: {} ({}). Each level now also gives {}.", c["name"].as_str().unwrap_or(&key), CLASS_TIERS[self.class_tier], growth.join(", ")));
        self.check_slots(defs);
        Ok(())
    }

    /// Would this purchase make a tier past the last one (which the System names, not the player)?
    pub fn needs_tier_name(&self, what: &str, target: &str) -> bool {
        matches!(what, "class_tier" | "species_tier") || (what == "skill_tier" && self.skills.get(target).is_some_and(|s| s.tier >= SKILL_TIERS.len() - 1))
    }

    /// Spend bonus points (only earned past the cap). The player's choice, never the System's.
    pub fn spend_bp(&mut self, what: &str, target: &str, points: u64, tier_name: Option<&str>, defs: &Defs) -> Result<(), String> {
        if what == "stat" {
            let s = target.to_uppercase();
            let n = points.max(1);
            if !STATS.contains(&s.as_str()) {
                return Err("stat is HP/STR/INT/DEX".into());
            }
            if n > self.bp_free {
                return Err("not enough bonus points".into());
            }
            self.bp_free -= n;
            *self.stat_pct.entry(s.clone()).or_insert(0) += n;
            let (now, left) = (self.true_stat(&s, defs), self.bp_free);
            self.say(&format!("{} +{}% (now {now}). {left} bonus points left.", stat_name(&s), n as f64 / 10.0));
            return Ok(());
        }
        let cost = BP_COST.iter().find(|(k, _)| *k == what).map(|(_, c)| *c).ok_or("spend on: stat, skill_tier, class_tier, species_tier, class_skill, racial_skill")?;
        if cost > self.bp_free {
            return Err(format!("that costs {cost} bonus points; you have {}", self.bp_free));
        }
        match what {
            "skill_tier" => {
                let t = self.skill_key(target).ok_or("you don't have that skill")?;
                let sk = self.skills.get_mut(&t).expect("held");
                if sk.tier < SKILL_TIERS.len() - 1 {
                    sk.tier += 1;
                    let n = SKILL_TIERS[sk.tier];
                    self.say(&format!("{t} is now {n}."));
                } else if let Some(n) = tier_name {
                    self.beyond.entry(format!("skill:{t}")).or_default().push(n.chars().take(40).collect());
                    self.say(&format!("{t} is now {n}."));
                } else {
                    return Err("it is at its last tier; a tier beyond needs a name from the System".into());
                }
            }
            "class_tier" | "species_tier" => {
                let kind = what.split('_').next().unwrap_or("");
                if kind == "class" && self.cls.is_none() {
                    return Err("you have no class".into());
                }
                let n = tier_name.ok_or("a tier past the last needs a name from the System")?;
                self.beyond.entry(kind.into()).or_default().push(n.chars().take(40).collect());
                self.say(&format!("Your {kind} rises past its last tier: {n}."));
            }
            _ => {
                let kind = if what == "class_skill" { "class" } else { "racial" };
                let whose = if kind == "class" { "class" } else { "species" };
                if self.beyond.get(whose).is_none_or(|v| v.is_empty()) {
                    return Err(format!("new {kind} skills open only once your {whose} has been raised past its last tier"));
                }
                let sk = defs.skill_key(target).filter(|s| !self.skills.contains_key(s)).ok_or("no such skill, or you already have it")?;
                self.learn(&sk, kind, 1, defs);
            }
        }
        self.bp_free -= cost;
        Ok(())
    }

    // ---------------------------------------------------------------- frequencies

    /// Gather amplitude for `minutes` from where the character is. Overuse pain fades meanwhile.
    pub fn regen(&mut self, minutes: &BigRational, resting: bool, defs: &Defs) -> BTreeMap<i32, BigRational> {
        let (int, top) = (self.true_stat("INT", defs), self.amplitude_max(defs));
        let shares = self.shares.clone();
        let got = self.prism.regen(minutes, int, &top, shares.as_ref(), resting);
        self.pain = (self.pain - PAIN_FADES_PER_S * 60.0 * minutes.to_f64().unwrap_or(0.0)).max(0.0);
        got
    }

    /// Where the character is now: an unbalanced environment it isn't tuned for takes 20% off every stat.
    pub fn surround(&mut self, shares: Option<BTreeMap<String, f64>>) {
        let (_, unbalanced, why) = environment(shares.as_ref(), &self.prism.held);
        if unbalanced != self.env_unbalanced {
            if unbalanced {
                let known = self.knows_dominant(shares.as_ref());
                let what = if known { why.split(": ").last().unwrap_or("").to_string() } else { "a frequency you do not know".into() };
                self.say(&format!("Unbalanced environment ({what}): all stats -20%, frequency regen halved."));
            } else {
                self.say("The surroundings are balanced again: your stats and regen are whole.");
            }
        }
        self.shares = shares;
        self.env_unbalanced = unbalanced;
        self.env_why = why;
    }

    fn knows_dominant(&self, shares: Option<&BTreeMap<String, f64>>) -> bool {
        let top = shares.and_then(|s| s.iter().filter(|(k, _)| k.as_str() != "base").max_by(|a, b| a.1.total_cmp(b.1)).and_then(|(k, _)| k.parse::<i32>().ok()));
        top.is_some_and(|t| (0..=10).contains(&t) || self.prism.held.contains(&t))
    }

    /// Use amplitude of one frequency. Overuse decreases that frequency's maximum and hurts.
    pub fn spend(&mut self, place: i32, amount: &BigRational, defs: &Defs) -> Result<Spent, String> {
        let top = self.amplitude_max(defs);
        let out = self.prism.spend(place, amount, &top)?;
        if out.overdrawn.is_positive() {
            self.pain += out.pain;
            self.say(&format!("You overuse {}: it can hold {:.0}% less now.{}", freq(place).name, out.decrease_percent, if out.blocked { " It no longer answers you." } else { "" }));
        }
        Ok(out)
    }

    /// A species upgrade opens one number past an end of a run: `place` if it is one of the openings,
    /// otherwise one chosen the same way every time for this character and tier.
    pub fn open_frequency(&mut self, place: Option<i32>) -> Option<i32> {
        let can = openings(&self.prism.held);
        if can.is_empty() {
            return None;
        }
        let place = place.filter(|p| can.contains(p)).unwrap_or_else(|| {
            let i = (unit(&format!("open:{}:{}", self.name.to_lowercase(), self.species_tier)) * can.len() as f64) as usize;
            can[i.min(can.len() - 1)]
        });
        self.prism.extend(place);
        let f = freq(place);
        self.say(&format!("Your soul prism is tuned to a new frequency: {} ({}), {}. {}.", label(place), f.name, f.spectrum, f.does));
        Some(place)
    }

    /// (amplitude remaining, maximum) over every frequency held.
    pub fn remaining(&mut self, defs: &Defs) -> (BigRational, BigRational) {
        let top = self.amplitude_max(defs);
        self.prism.fill(&top);
        let held = self.prism.held.clone();
        let have = held.iter().filter(|p| !self.prism.blocked(**p)).fold(rat(0), |a, p| a + self.prism.have(*p));
        let most = held.iter().fold(rat(0), |a, p| a + BigRational::from_integer(self.prism.top(*p, &top)));
        (have, most)
    }

    /// The System has judged the use so far: start counting afresh.
    pub fn looked(&mut self) {
        for v in self.skills.values_mut() {
            v.used = 0.0;
        }
        self.attempts.clear();
    }
}

pub struct FeatArgs {
    pub name: String,
    pub about: String,
    pub xp: BigInt,
    /// (skill, tier index, kind)
    pub skills: Vec<(String, usize, String)>,
    pub boosts: BTreeMap<String, f64>,
    pub stats: BTreeMap<String, i64>,
    pub upgrade: Option<String>,
    pub chance: Option<f64>,
    pub upgrade_skill: Option<String>,
    pub tier_name: Option<String>,
}

// ------------------------------------------------------------------ the world: every sheet, and what the System defined

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct World {
    pub sheets: BTreeMap<String, Sheet>,
    /// The two basic paths of each tier-up, the same for everyone: "class:scout:1" -> [two paths].
    pub basics: BTreeMap<String, Vec<PathOpt>>,
    pub defs: Defs,
    /// What this story calls the System's words: "amplitude" -> "Aether". Only the shown name
    /// changes; the rules keep Wolfgang's words inside, so a rename can't touch a single number.
    pub labels: BTreeMap<String, String>,
}

/// The words the System may give a new name to fit the story.
pub fn renamable() -> Vec<String> {
    let mut v: Vec<String> = [
        "HP", "STR", "INT", "DEX", "Health", "Strength", "Intelligence", "Dexterity", "amplitude", "speed", "stamina", "frequency", "frequencies",
        "spectrum", "spectrums", "soul prism", "semi skill", "semi skills", "base frequency", "unbalanced environment", "overpower", "world hoppers",
        "world hopper", "alien zone", "alien zones", "experience", "level", "bonus points", "System",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for f in builtin().freq.values() {
        v.push(f.name.clone());
        v.push(f.spectrum.clone());
    }
    for t in CLASS_TIERS.iter().chain(SPECIES_TIERS.iter()).chain(SKILL_TIERS.iter()) {
        v.push(t.to_string());
    }
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup_by_key(|s| s.to_lowercase());
    v
}

fn word_regex(words: &[String]) -> Option<regex::Regex> {
    if words.is_empty() {
        return None;
    }
    let mut w: Vec<&String> = words.iter().collect();
    w.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let alt = w.iter().map(|s| regex::escape(s)).collect::<Vec<_>>().join("|");
    regex::Regex::new(&format!(r"(?i)\b(?:{alt})\b")).ok()
}

/// Text in the story's own words, in one pass.
pub fn relabel(text: &str, labels: &BTreeMap<String, String>) -> String {
    let keys: Vec<String> = labels.keys().cloned().collect();
    let Some(re) = word_regex(&keys) else { return text.to_string() };
    re.replace_all(text, |c: &regex::Captures| {
        let m = &c[0];
        let new = labels.get(&m.to_lowercase()).cloned().unwrap_or_else(|| m.to_string());
        if m.chars().next().is_some_and(char::is_uppercase) { capital(&new) } else { new }
    })
    .into_owned()
}

/// Every string in a view, in the story's own words.
pub fn relabel_value(v: &mut Value, labels: &BTreeMap<String, String>) {
    match v {
        Value::String(s) => *s = relabel(s, labels),
        Value::Array(a) => a.iter_mut().for_each(|x| relabel_value(x, labels)),
        Value::Object(o) => o.values_mut().for_each(|x| relabel_value(x, labels)),
        _ => {}
    }
}

impl World {
    /// The story's names back to the System's words, for arguments that name a frequency or stat.
    fn unlabel(&self, text: &str) -> String {
        let back: BTreeMap<String, String> = self.labels.iter().map(|(k, v)| (v.to_lowercase(), k.clone())).collect();
        let keys: Vec<String> = back.keys().cloned().collect();
        let Some(re) = word_regex(&keys) else { return text.to_string() };
        re.replace_all(text, |c: &regex::Captures| back.get(&c[0].to_lowercase()).cloned().unwrap_or_else(|| c[0].to_string())).into_owned()
    }

    /// rename amplitude -> Aether   (also: rename term amplitude -> Aether; rename Aether -> Ether)
    fn rename(&mut self, t: &str) -> Result<String, String> {
        let t = t.trim();
        let t = t.strip_prefix("term ").or_else(|| t.strip_prefix("word ")).unwrap_or(t);
        let (old, new) = t.split_once("->").ok_or("rename needs: rename amplitude -> Aether")?;
        let old = old.trim().to_lowercase();
        let new: String = new.chars().filter(|c| c.is_alphanumeric() || " -'".contains(*c)).take(40).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
        if new.is_empty() {
            return Err("that name is empty".into());
        }
        let terms = renamable();
        // by its own word, or by the name it already has
        let canon = terms
            .iter()
            .find(|w| w.to_lowercase() == old)
            .map(|w| w.to_lowercase())
            .or_else(|| self.labels.iter().find(|(_, v)| v.to_lowercase() == old).map(|(k, _)| k.clone()))
            .ok_or_else(|| format!("'{old}' isn't one of the System's words that can be renamed"))?;
        let clash = terms.iter().any(|w| w.to_lowercase() == new.to_lowercase() && w.to_lowercase() != canon)
            || self.labels.iter().any(|(k, v)| k != &canon && v.eq_ignore_ascii_case(&new));
        if clash {
            return Err(format!("'{new}' is already the name of something else"));
        }
        if new.eq_ignore_ascii_case(&canon) {
            self.labels.remove(&canon);
        } else {
            self.labels.insert(canon.clone(), new.clone());
        }
        Ok(format!("{canon} is now called {new}"))
    }
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn amount_of(t: &str, have: &BigRational) -> Option<BigRational> {
    let t = t.trim();
    if t.eq_ignore_ascii_case("all") {
        return Some(have.clone());
    }
    if let Some(p) = t.strip_suffix('%') {
        return parse_rational(p).map(|p| (have.clone() * p / rat(100)).floor());
    }
    parse_rational(t).filter(|v| !v.is_negative())
}

/// "HP 1, STR 0, INT +2" or "HP:1 STR:0" -> {HP: 1, ...}
fn stat_list(t: &str) -> BTreeMap<String, i64> {
    let words: Vec<String> = t.replace([',', ':', '='], " ").split_whitespace().map(String::from).collect();
    let mut out = BTreeMap::new();
    for w in words.windows(2) {
        let k = w[0].to_uppercase();
        if STATS.contains(&k.as_str()) {
            if let Ok(v) = w[1].trim_start_matches('+').parse::<i64>() {
                out.insert(k, v);
            }
        }
    }
    out
}

/// "speed 0.1, sight 0.02" -> {speed: 0.1, ...}
fn number_list(t: &str) -> BTreeMap<String, f64> {
    let words: Vec<String> = t.replace([',', ':', '='], " ").split_whitespace().map(String::from).collect();
    words.windows(2).filter_map(|w| Some((w[0].to_lowercase(), w[1].trim_start_matches('+').parse::<f64>().ok()?))).collect()
}

/// "Name: what it does [frequency 6] [STR +2] [key scout] [effects speed 0.1]" -> a path.
fn path_from(t: &str) -> PathOpt {
    let mut o = PathOpt::default();
    let mut text = String::new();
    let mut rest = t;
    while let Some(i) = rest.find('[') {
        text.push_str(&rest[..i]);
        let Some(j) = rest[i..].find(']') else { break };
        let inner = rest[i + 1..i + j].trim();
        let (k, v) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
        match k.to_lowercase().as_str() {
            "frequency" => o.frequency = place_from(v),
            "key" => o.key = Some(v.trim().to_string()),
            "effects" | "effect" => o.effects = number_list(v),
            _ => o.stats.extend(stat_list(inner)),
        }
        rest = &rest[i + j + 1..];
    }
    text.push_str(rest);
    let (n, a) = text.split_once(':').unwrap_or((&text, ""));
    o.name = n.trim().to_string();
    o.about = a.trim().to_string();
    o
}

/// A tier-up's tag from "class", "species", "skill Inspect" or "skill:Inspect".
fn tag_of(t: &str, s: &Sheet) -> (String, Option<String>) {
    let t = t.trim();
    let low = t.to_lowercase();
    if low == "class" || low == "species" {
        return (low, None);
    }
    let name = t.trim_start_matches(|c: char| c.is_alphabetic()).trim_start_matches([':', ' ']).trim();
    let key = s.skill_key(name).unwrap_or_else(|| name.to_string());
    ("skill".into(), Some(key))
}

/// What the line's result is: shown to the player, or for the System alone (item 73).
pub const SYSTEM_ONLY: &str = "§ ";

impl World {
    pub fn sheet_key(&self, character: &str) -> Option<String> {
        if let Some(k) = self.sheets.keys().find(|k| norm(k) == norm(character)) {
            return Some(k.clone());
        }
        (self.sheets.len() == 1 && character.trim().is_empty()).then(|| self.sheets.keys().next().cloned()).flatten()
    }

    /// The System has seen the use since its last look.
    pub fn looked(&mut self) {
        for s in self.sheets.values_mut() {
            s.looked();
        }
    }

    /// One change the System asked for, in an `<rpg>` line.
    pub fn apply_line(&mut self, line: &str, default_char: &str) -> Result<String, String> {
        let line = line.trim().trim_start_matches('-').trim();
        let (verb0, _) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        // Defining things is for everyone at once; no character needed.
        if verb0.eq_ignore_ascii_case("define") {
            return self.define(line[verb0.len()..].trim());
        }
        if verb0.eq_ignore_ascii_case("rename") {
            return self.rename(line[verb0.len()..].trim());
        }
        if ["lens", "crystal", "impurity", "battery"].contains(&verb0.to_lowercase().as_str()) {
            return crafting_calc(&self.unlabel(line)).map(|m| format!("{SYSTEM_ONLY}{m}"));
        }
        let (who, cmd) = match line.split_once(':') {
            Some((a, b)) if !a.trim().is_empty() && self.sheet_key(a).is_some() => (a.trim().to_string(), b.trim()),
            _ => (default_char.to_string(), line),
        };
        let key = self.sheet_key(&who).ok_or_else(|| format!("no sheet for '{who}'"))?;
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let (verb, rest) = (verb.to_lowercase(), rest.trim());
        // Arguments that name a frequency or a stat may use the story's names.
        let rest_owned = if ["surroundings", "environment", "spend", "amplitude", "semi", "stat"].contains(&verb.as_str()) { self.unlabel(rest) } else { rest.to_string() };
        let rest = rest_owned.as_str();

        // A frequency used on someone else needs their amplitude, before the caster is borrowed.
        let target_amp = if verb == "semi" { self.semi_target(rest, &key) } else { None };

        let defs = &self.defs;
        let s = self.sheets.get_mut(&key).ok_or("no sheet")?;
        let mut gm_only = false;
        let msg: String = match verb.as_str() {
            "xp" => {
                let (n, why) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let xp = to_xp(n);
                if parse_rational(n).is_some_and(|v| v.is_negative()) {
                    return Err("experience can't be taken away".into());
                }
                let why = why.trim();
                s.gain(xp.clone(), Some(if why.is_empty() { "the System's judgment" } else { why }), defs);
                format!("+{} experience", short_int(&xp))
            }
            "defeat" | "defeated" => {
                let (n, what) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let b: u32 = n.trim_start_matches("level").trim().parse().map_err(|_| "defeat needs the enemy's level: defeat 12 the grey wolf")?;
                let b = b.clamp(1, MAX_LEVEL);
                let m = difference_multiplier(s.level, b, s.class_tier, s.species_tier);
                let xp = BigRational::from_float(defeat_xp(b) * m).unwrap_or_else(|| rat(0)).floor().to_integer();
                let what = if what.trim().is_empty() { "an enemy".to_string() } else { what.trim().to_string() };
                s.gain(xp.clone(), Some(&format!("defeated {what} (level {b})")), defs);
                format!("+{} experience for {what}", short_int(&xp))
            }
            "skill" => {
                if let Some(r) = rest.strip_prefix("xp").or_else(|| rest.strip_prefix("XP")) {
                    // skill xp Inspect +40
                    let r = r.trim();
                    let (name, n) = r.rsplit_once(char::is_whitespace).ok_or("skill xp needs: skill xp Name +n")?;
                    let k = s.skill_key(name).ok_or_else(|| format!("{key} doesn't have {}", name.trim()))?;
                    let xp = to_xp(n);
                    s.skill_xp(&k, xp.clone(), None);
                    format!("{k} +{} experience", short_int(&xp))
                } else {
                    // skill + Inspect [racial|class|general] [| what it does, to define a new one]
                    let t = rest.trim_start_matches('+').trim();
                    let (head, about) = t.split_once('|').map(|(a, b)| (a.trim(), b.trim())).unwrap_or((t, ""));
                    let mut words: Vec<&str> = head.split_whitespace().collect();
                    let kind = match words.last().map(|w| w.to_lowercase()) {
                        Some(w) if ["racial", "class", "general"].contains(&w.as_str()) && words.len() > 1 => {
                            words.pop();
                            w
                        }
                        _ => "general".into(),
                    };
                    let name = words.join(" ");
                    if defs.skill_key(&name).is_none() {
                        if about.is_empty() {
                            return Err(format!("no skill called {name}: give it a line saying what it does (skill + {name} | what it does)"));
                        }
                        // drop the borrow and define it for everyone, then learn it
                        let (name, about, kind) = (name.clone(), about.to_string(), kind.clone());
                        self.defs.skills.insert(name.clone(), json!({"stat": "INT", "about": about}));
                        let defs = &self.defs;
                        let s = self.sheets.get_mut(&key).ok_or("no sheet")?;
                        s.learn(&name, &kind, 1, defs);
                        let out = finish(&key, &format!("Skill: {name}"), s.drain());
                        return Ok(out);
                    }
                    if !s.learn(&name, &kind, 1, defs) {
                        return Err(format!("no skill called {name}"));
                    }
                    format!("Skill: {}", defs.skill_key(&name).unwrap_or(name))
                }
            }
            "class" => {
                let (sub, arg) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                match sub.to_lowercase().as_str() {
                    "offer" => format!("Class offered: {}", s.offer_class(arg, defs)?),
                    "choose" | "take" => {
                        s.choose_class(arg, defs)?;
                        format!("Class: {}", s.shown_name("class", None, defs))
                    }
                    _ => return Err("class needs: class offer <class>, or class choose <class> once they pick it".into()),
                }
            }
            "paths" => {
                let mut parts = rest.split('|').map(str::trim);
                let (kind, k) = tag_of(parts.next().unwrap_or(""), s);
                let opts: Vec<PathOpt> = parts.filter(|p| !p.is_empty()).map(path_from).collect();
                let mut basics = std::mem::take(&mut self.basics);
                let defs = &self.defs;
                let s = self.sheets.get_mut(&key).ok_or("no sheet")?;
                let r = s.offer(&kind, k.as_deref(), opts, &mut basics, defs);
                self.basics = basics;
                let opts = r?;
                let s = self.sheets.get_mut(&key).ok_or("no sheet")?;
                let tag = if kind == "skill" { format!("skill:{}", k.unwrap_or_default()) } else { kind };
                return Ok(finish(&key, &format!("Paths open for {tag}: {}", opts.iter().map(|o| o.name.clone()).collect::<Vec<_>>().join(", ")), s.drain()));
            }
            "choose" => {
                let (tag, n) = rest.rsplit_once(char::is_whitespace).ok_or("choose needs: choose class 2")?;
                let (kind, k) = tag_of(tag, s);
                let tag = if kind == "skill" { format!("skill:{}", k.unwrap_or_default()) } else { kind };
                let o = s.choose(&tag, n.trim().parse().map_err(|_| "choose needs a path number")?, defs)?;
                format!("Path taken: {}", o.name)
            }
            "feat" => {
                let f = feat_from(rest)?;
                let gave = s.feat(f, defs);
                format!("Feat: {}", gave.join(", "))
            }
            "title" => {
                let (t, why) = rest.split_once(':').map(|(a, b)| (a.trim(), b.trim())).unwrap_or((rest, ""));
                s.title(t, why);
                format!("Title: {t}")
            }
            "stat" => {
                let m = stat_list(rest);
                if m.is_empty() {
                    return Err("stat needs: stat STR +2".into());
                }
                for (k, v) in &m {
                    *s.adjust.entry(k.clone()).or_insert(0) += (*v).clamp(-10, 10);
                }
                format!("Stats: {}", m.iter().map(|(k, v)| format!("{k} {v:+}")).collect::<Vec<_>>().join(", "))
            }
            "surroundings" | "environment" => {
                let low = rest.to_lowercase();
                let shares = if low.is_empty() || low == "balanced" {
                    None
                } else if ["alien", "alien zone", "none", "nothing"].contains(&low.as_str()) {
                    Some(BTreeMap::new())
                } else {
                    let mut m = BTreeMap::new();
                    for part in rest.split(',') {
                        let (name, v) = part.trim().rsplit_once(char::is_whitespace).ok_or("surroundings needs: heat 0.8, base 0.2")?;
                        let v: f64 = v.trim_end_matches('%').parse().map_err(|_| "surroundings needs shares: heat 0.8, base 0.2")?;
                        let k = if name.trim().eq_ignore_ascii_case("base") { "base".to_string() } else { place_from(name).ok_or_else(|| format!("no frequency called {name}"))?.to_string() };
                        m.insert(k, v);
                    }
                    Some(m)
                };
                s.surround(shares);
                gm_only = true;
                format!("Surroundings: {}", s.env_why)
            }
            "regen" | "rest" => {
                let mut words = rest.split_whitespace();
                let minutes = words.next().and_then(parse_rational).filter(|m| m.is_positive()).ok_or("regen needs minutes: regen 10 [resting]")?;
                let resting = verb == "rest" || words.any(|w| w.eq_ignore_ascii_case("resting"));
                let got = s.regen(&minutes, resting, defs);
                let g: Vec<String> = got.iter().filter(|(_, v)| v.is_positive()).map(|(p, v)| format!("{} +{}", freq(*p).spectrum, short(v))).collect();
                format!("{} minutes{}: {}", short(&minutes), if resting { " resting" } else { "" }, if g.is_empty() { "nothing gathered".into() } else { g.join(", ") })
            }
            "spend" | "amplitude" => {
                let (f, n) = rest.rsplit_once(char::is_whitespace).ok_or("spend needs: spend heat 300")?;
                let place = place_from(f).ok_or_else(|| format!("no frequency called {f}"))?;
                let top = s.amplitude_max(defs);
                s.prism.fill(&top);
                let have = s.prism.have(place);
                let amt = amount_of(n.trim_start_matches('-'), &have).ok_or("spend needs an amount: 300, 10% or all")?;
                s.spend(place, &amt, defs)?;
                format!("{} -{} ({} left)", freq(place).spectrum, short(&amt), short(&s.prism.have(place)))
            }
            "semi" => {
                // semi heat 300 [at Name [level N]]
                let (head, at) = match rest.to_lowercase().find(" at ") {
                    Some(i) => (&rest[..i], Some(rest[i + 4..].trim())),
                    None => (rest, None),
                };
                let (f, n) = head.trim().rsplit_once(char::is_whitespace).ok_or("semi needs: semi heat 300 [at Name]")?;
                let k = semi_from(f, &s.prism.held).ok_or_else(|| format!("{key} has no semi skill of {}", f.trim()))?;
                let base = semi_base(&k);
                let top = s.amplitude_max(defs);
                s.prism.fill(&top);
                let have = s.prism.have(base);
                let amt = amount_of(n, &have).ok_or("semi needs an amount: 300, 10% or all")?;
                s.spend(base, &amt, defs)?;
                let name = semi_name(&k);
                let level = s.skills.get(&name).map_or(0, |x| x.level);
                s.use_skill(&name, 1.0 + 0.2 * short_log10(&amt), defs);
                let pw = semi_power(&k, &amt, level);
                let d = builtin().semi.get(&k).cloned().unwrap_or(Value::Null);
                let mut out = format!("{name}: {} with {} amplitude, power {}", d["does"].as_str().unwrap_or(""), short(&amt), g4(pw));
                if let (Some(at), Some((need, consent))) = (at, target_amp) {
                    if d["effect"] != "heal" && !consent && amt < need {
                        out.push_str(&format!(". It doesn't take hold on {}: their amplitude resists it (overpowering them takes {})", at.split(" level").next().unwrap_or(at), short(&need.ceil())));
                    } else {
                        out.push_str(&format!(" on {}", at.split(" level").next().unwrap_or(at)));
                    }
                }
                if d["lasting"].as_bool() == Some(true) {
                    out.push_str(&format!(". Upkeep every 10 s, starting at {} and doubling each time", short(&upkeep(&amt, 0))));
                }
                out
            }
            "consent" => {
                let (revoke, name) = match rest.strip_prefix('-') {
                    Some(n) => (true, n.trim()),
                    None => (false, rest.trim_start_matches('+').trim_start_matches("to ").trim()),
                };
                s.consent.retain(|c| norm(c) != norm(name));
                if !revoke {
                    s.consent.push(name.to_string());
                }
                format!("{} {name}", if revoke { "No longer consents to" } else { "Consents to frequencies from" })
            }
            "craft" | "crafted" => {
                let (name, about) = rest.split_once('|').map(|(a, b)| (a.trim(), b.trim())).unwrap_or((rest, ""));
                if name.is_empty() {
                    return Err("craft needs what was made".into());
                }
                s.crafted.push(name.to_string());
                match s.inventory.iter_mut().find(|i| norm(&i.name) == norm(name)) {
                    Some(i) => i.qty += 1,
                    None => s.inventory.push(Item { name: name.to_string(), qty: 1, about: about.to_string() }),
                }
                // General skills come when the requirement is met (crafting: having crafted one thing).
                if s.skill_key("Crafting").is_none() {
                    s.learn("Crafting", "general", 1, defs);
                } else {
                    s.use_skill("Crafting", 1.0, defs);
                }
                format!("Crafted: {name}")
            }
            "item" => {
                let (sign, name) = rest.split_at(rest.chars().next().map_or(0, char::len_utf8));
                let (name, about) = name.split_once('|').map(|(a, b)| (a.trim(), b.trim())).unwrap_or((name.trim(), ""));
                let (name, qty) = match name.rsplit_once(" x") {
                    Some((a, q)) if q.parse::<i32>().is_ok() => (a.trim().to_string(), q.parse::<i32>().unwrap_or(1)),
                    _ => (name.to_string(), 1),
                };
                match sign {
                    "+" => {
                        match s.inventory.iter_mut().find(|i| norm(&i.name) == norm(&name)) {
                            Some(i) => i.qty += qty,
                            None => s.inventory.push(Item { name: name.clone(), qty, about: about.into() }),
                        }
                        format!("Item: {name} x{qty}")
                    }
                    "-" => {
                        let i = s.inventory.iter_mut().find(|i| norm(&i.name) == norm(&name) && i.qty >= qty).ok_or_else(|| format!("{key} doesn't have {qty} {name}"))?;
                        i.qty -= qty;
                        s.inventory.retain(|i| i.qty > 0);
                        format!("Lost: {name} x{qty}")
                    }
                    _ => return Err("item needs + or -".into()),
                }
            }
            "money" => {
                let n: i64 = rest.split_whitespace().next().and_then(|n| n.trim_start_matches('+').parse().ok()).ok_or("money needs a number")?;
                if s.money + n < 0 {
                    return Err(format!("{key} has only {}: can't spend {}", s.money, -n));
                }
                s.money += n;
                format!("{n:+} money ({})", s.money)
            }
            "condition" => {
                let (sign, name) = rest.split_at(rest.chars().next().map_or(0, char::len_utf8));
                let name = name.trim();
                match sign {
                    "+" => {
                        if !s.conditions.iter().any(|c| norm(c) == norm(name)) {
                            s.conditions.push(name.to_string());
                        }
                        format!("Condition: {name}")
                    }
                    "-" => {
                        s.conditions.retain(|c| norm(c) != norm(name));
                        format!("Cleared: {name}")
                    }
                    _ => return Err("condition needs + or -".into()),
                }
            }
            "beyond" => {
                // beyond class_tier -> Name   /   beyond skill_tier Inspect -> Name: names a tier the player bought
                let (what, name) = rest.split_once("->").ok_or("beyond needs: beyond class_tier -> Name")?;
                let (what, target) = what.trim().split_once(char::is_whitespace).map(|(a, b)| (a.trim(), b.trim())).unwrap_or((what.trim(), ""));
                let ask = if target.is_empty() { what.to_string() } else { format!("{what}:{}", s.skill_key(target).unwrap_or(target.to_string())) };
                let i = s.bp_asked.iter().position(|a| a.eq_ignore_ascii_case(&ask)).ok_or("the player hasn't asked to buy that tier")?;
                let tgt = ask.split_once(':').map(|(_, t)| t.to_string()).unwrap_or_default();
                s.spend_bp(what, &tgt, 1, Some(name.trim()), defs)?;
                s.bp_asked.remove(i);
                format!("Named: {}", name.trim())
            }
            other => return Err(format!("unknown change '{other}'")),
        };
        let msgs = s.drain();
        let out = finish(&key, &msg, msgs);
        Ok(if gm_only { format!("{SYSTEM_ONLY}{out}") } else { out })
    }

    /// For `semi ... at Name [level N]`: what it takes to overpower the target, and whether they consent.
    fn semi_target(&mut self, rest: &str, caster: &str) -> Option<(BigRational, bool)> {
        let i = rest.to_lowercase().find(" at ")?;
        let at = rest[i + 4..].trim();
        let (name, level) = match at.to_lowercase().find(" level ") {
            Some(j) => (at[..j].trim().to_string(), at[j + 7..].trim().parse::<u64>().ok()),
            None => (at.to_string(), None),
        };
        if norm(&name) == norm(caster) || ["self", "myself", "themselves", "itself"].contains(&name.to_lowercase().as_str()) {
            return Some((rat(0), true));
        }
        if let Some(k) = self.sheet_key(&name) {
            let defs = self.defs.clone();
            let t = self.sheets.get_mut(&k)?;
            let (have, most) = t.remaining(&defs);
            let consent = t.consent.iter().any(|c| norm(c) == norm(caster));
            return Some((overpower_needed(&have, &most), consent));
        }
        // A creature with no soul prism: level x 10 amplitude, remaining and maximum alike.
        let a = rat((level.unwrap_or(1) * CREATURE_AMPLITUDE_PER_LEVEL) as i64);
        Some((overpower_needed(&a, &a), false))
    }

    /// define class Key | Name | about | growth HP 1, STR 0, INT 1, DEX 3 | stats DEX 2 | skills A, B | derived awareness = int * dex
    /// define species Key | Name | about | ...the same
    /// define skill Name | about [| revival]
    fn define(&mut self, t: &str) -> Result<String, String> {
        let (kind, rest) = t.split_once(char::is_whitespace).ok_or("define needs: define class|species|skill ...")?;
        let parts: Vec<&str> = rest.split('|').map(str::trim).collect();
        match kind.to_lowercase().as_str() {
            "skill" => {
                let name = parts.first().copied().filter(|n| !n.is_empty()).ok_or("define skill needs a name")?;
                let about = parts.get(1).copied().unwrap_or("");
                if about.split_whitespace().count() < 2 {
                    return Err("a skill needs a line saying what it does".into());
                }
                let mut v = json!({"stat": "INT", "about": about});
                if parts.iter().skip(2).any(|p| p.eq_ignore_ascii_case("revival")) {
                    v["revival"] = json!(true);
                }
                self.defs.skills.insert(name.to_string(), v);
                Ok(format!("{SYSTEM_ONLY}Defined skill: {name}"))
            }
            k @ ("class" | "species") => {
                let key = parts.first().copied().filter(|n| !n.is_empty()).ok_or("needs a key")?.to_lowercase().replace(' ', "_");
                let name = parts.get(1).copied().unwrap_or(&key).to_string();
                let about = parts.get(2).copied().unwrap_or("").to_string();
                let mut v = json!({"name": name, "about": about, "stats": {}, "growth": {}, "skills": [], "traits": {}, "custom": true});
                for p in parts.iter().skip(3) {
                    let (field, val) = p.split_once(char::is_whitespace).unwrap_or((p, ""));
                    match field.to_lowercase().as_str() {
                        "growth" => v["growth"] = json!(stat_list(val)),
                        "stats" => v["stats"] = json!(stat_list(val)),
                        "skills" => v["skills"] = json!(val.split(',').map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>()),
                        "derived" => {
                            let (n, f) = val.split_once('=').ok_or("derived needs: derived name = formula")?;
                            formula(f, &STATS.iter().map(|s| (s.to_string(), 10)).collect()).map_err(|e| format!("that formula {e}"))?;
                            v["derived"][n.trim()] = json!(f.trim());
                        }
                        _ => {}
                    }
                }
                if k == "class" {
                    self.defs.classes.insert(key.clone(), v);
                } else {
                    self.defs.species.insert(key.clone(), v);
                }
                Ok(format!("{SYSTEM_ONLY}Defined {k}: {name} ({key})"))
            }
            other => Err(format!("can't define a {other}")),
        }
    }

    /// A new character's sheet from the System's JSON. Stats are clamped; nothing to spend is granted.
    pub fn sheet_from_json(&self, v: &Value) -> Option<Sheet> {
        let name = v["name"].as_str()?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        let base: BTreeMap<String, i64> = STATS.iter().map(|s| (s.to_string(), v["stats"][*s].as_i64().unwrap_or(10).clamp(1, 20))).collect();
        let mut s = Sheet::new(&name, v["species"].as_str().unwrap_or("human"), base, &self.defs);
        for (k, sk) in v["skills"].as_object().into_iter().flatten().take(20) {
            let kind = sk["kind"].as_str().unwrap_or("general");
            let level = sk["level"].as_u64().unwrap_or(1).clamp(1, 5) as u32;
            s.learn(k, kind, level, &self.defs);
        }
        s.money = v["money"].as_i64().unwrap_or(0).max(0);
        for it in v["inventory"].as_array().into_iter().flatten().take(40) {
            if let Some(n) = it["name"].as_str().or(it.as_str()) {
                s.inventory.push(Item { name: n.trim().into(), qty: it["qty"].as_i64().unwrap_or(1).max(1) as i32, about: it["about"].as_str().unwrap_or("").into() });
            }
        }
        Some(s)
    }

    /// A Classic sheet carried into the AD System: its attributes read as the four base stats, its skills kept.
    pub fn from_classic(&self, c: &crate::rpg::Sheet) -> Sheet {
        let base: BTreeMap<String, i64> = [("HP", "Endurance"), ("STR", "Might"), ("INT", "Intellect"), ("DEX", "Agility")].iter().map(|(k, a)| (k.to_string(), (c.attribute(a) as i64).clamp(1, 20))).collect();
        let mut s = Sheet::new(&c.name, "human", base, &self.defs);
        s.money = c.money;
        s.inventory = c.inventory.iter().map(|i| Item { name: i.name.clone(), qty: i.qty, about: String::new() }).collect();
        s
    }
}

/// What only the player may decide (system.py: `choose`, `choose_class`, `spend_bp`), from the status panel.
///   kind "path":  name "class|2", "species|1", "skill:Observation|4"
///   kind "class": name = the class offered
///   kind "bp":    name "stat|STR|5", "skill_tier|Observation", "class_tier", "class_skill|Revive"
pub fn player_choice(rules: &mut crate::rpg::Rules, character: &str, kind: &str, name: &str) -> Result<(), String> {
    let w = &mut rules.ad;
    let key = w.sheet_key(character).ok_or("No such character.")?;
    let defs = &w.defs;
    let s = w.sheets.get_mut(&key).ok_or("No such character.")?;
    let parts: Vec<&str> = name.split('|').map(str::trim).collect();
    match kind {
        "path" => {
            let pick = parts.get(1).and_then(|p| p.parse().ok()).ok_or("pick a path")?;
            s.choose(parts[0], pick, defs)?;
        }
        "class" => s.choose_class(name, defs)?,
        "bp" => {
            let what = parts[0];
            let target = parts.get(1).copied().unwrap_or("");
            let target = s.skill_key(target).unwrap_or(target.to_string());
            if s.needs_tier_name(what, &target) {
                // A tier past the last is named by the System, not the player: asked for now, named in the story.
                let cost = BP_COST.iter().find(|(k, _)| *k == what).map_or(0, |(_, c)| *c);
                if cost > s.bp_free {
                    return Err(format!("that costs {cost} bonus points; you have {}", s.bp_free));
                }
                let ask = if what == "skill_tier" { format!("{what}:{target}") } else { what.to_string() };
                if !s.bp_asked.contains(&ask) {
                    s.bp_asked.push(ask);
                }
                s.say("The System is considering what lies past the last tier.");
            } else {
                let points = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(1);
                s.spend_bp(what, &target, points, None, defs)?;
            }
        }
        _ => return Err("unknown choice".into()),
    }
    let msgs: Vec<String> = s.drain().iter().map(|m| relabel(m, &w.labels)).collect();
    rules.log.extend(msgs);
    Ok(())
}

fn short_log10(n: &BigRational) -> f64 {
    let i = n.floor().to_integer().max(BigInt::one());
    if i.bits() < 1000 {
        i.to_f64().unwrap_or(1.0).log10()
    } else {
        (i.bits() as f64 - 1.0) * 0.30102999566398
    }
}

fn finish(key: &str, msg: &str, msgs: Vec<String>) -> String {
    let mut parts = vec![format!("{key}: {msg}")];
    parts.extend(msgs);
    parts.join(" · ")
}

/// feat Name | about | xp 500 | skill Inspect skilled | boost speed 0.1 | stat STR 2 | upgrade class 0.0005 | upgrade skill Inspect 0.004 | beyond Name
fn feat_from(t: &str) -> Result<FeatArgs, String> {
    let parts: Vec<&str> = t.split('|').map(str::trim).collect();
    let name = parts.first().copied().filter(|n| !n.is_empty()).ok_or("a feat needs a name")?.to_string();
    let mut f = FeatArgs {
        name,
        about: parts.get(1).copied().unwrap_or("").to_string(),
        xp: BigInt::zero(),
        skills: Vec::new(),
        boosts: BTreeMap::new(),
        stats: BTreeMap::new(),
        upgrade: None,
        chance: None,
        upgrade_skill: None,
        tier_name: None,
    };
    for p in parts.iter().skip(2) {
        let (field, val) = p.split_once(char::is_whitespace).unwrap_or((p, ""));
        let val = val.trim();
        match field.to_lowercase().as_str() {
            "xp" | "experience" => f.xp = to_xp(val),
            "skill" => {
                let mut w: Vec<&str> = val.split_whitespace().collect();
                let mut kind = "general".to_string();
                if let Some(k) = w.last().filter(|k| ["racial", "class", "general"].contains(&k.to_lowercase().as_str())) {
                    kind = k.to_lowercase();
                    w.pop();
                }
                let tier = w.last().and_then(|t| SKILL_TIERS.iter().position(|x| x.eq_ignore_ascii_case(t)));
                if tier.is_some() {
                    w.pop();
                }
                f.skills.push((w.join(" "), tier.unwrap_or(0), kind));
            }
            "boost" => f.boosts.extend(number_list(val)),
            "stat" | "stats" => f.stats.extend(stat_list(val)),
            "upgrade" => {
                let mut w: Vec<&str> = val.split_whitespace().collect();
                let chance = w.last().and_then(|c| parse_rational(c.trim_start_matches("chance")).and_then(|r| r.to_f64()));
                if chance.is_some() {
                    w.pop();
                }
                if w.last().is_some_and(|x| x.eq_ignore_ascii_case("chance")) {
                    w.pop();
                }
                let kind = w.first().map(|k| k.to_lowercase()).ok_or("upgrade needs: upgrade class 0.0005")?;
                if kind == "skill" && w.len() > 1 {
                    f.upgrade_skill = Some(w[1..].join(" "));
                }
                f.upgrade = Some(kind);
                f.chance = chance;
            }
            "beyond" | "tier_name" => f.tier_name = Some(val.to_string()),
            _ => {}
        }
    }
    Ok(f)
}

/// The crafting arithmetic of his file, for the System alone (item 73: players find it by trying).
fn crafting_calc(line: &str) -> Result<String, String> {
    let w: Vec<&str> = line.split_whitespace().collect();
    let num = |i: usize| w.get(i).and_then(|t| place_from(t).map(|p| p as f64).or_else(|| t.parse::<f64>().ok()));
    let amt = |i: usize| w.get(i).and_then(|t| parse_rational(t));
    match w.first().map(|v| v.to_lowercase()).as_deref() {
        // lens merge 6 9 300 200 | lens divide 9 400 | lens compress 6 1000 2 | lens stretch 4 1000 1 | lens opposite 7 300
        Some("lens") => {
            let op = w.get(1).copied().unwrap_or("").to_lowercase();
            let a = num(2).ok_or("lens needs a frequency")?;
            let (b, amp, amp_b, steps) = match op.as_str() {
                "merge" | "merging" => (num(3), amt(4).unwrap_or_else(|| rat(1)), amt(5), 0),
                "compress" | "compression" | "stretch" | "stretching" => (None, amt(3).unwrap_or_else(|| rat(1)), None, w.get(4).and_then(|s| s.parse().ok()).unwrap_or(1)),
                _ => (None, amt(3).unwrap_or_else(|| rat(1)), None, 0),
            };
            let out = lens(&op, a, b, &amp, amp_b.as_ref(), steps)?;
            if out.is_empty() {
                return Ok("Dissipated: the frequency goes safely back into the world as base frequency.".into());
            }
            let each: Vec<String> = out
                .iter()
                .map(|(p, a)| {
                    if p.fract() == 0.0 {
                        format!("{} ({}) at {} amplitude", label(*p as i32), freq(*p as i32).name, short(a))
                    } else {
                        let (lo, hi) = (p.floor() as i32, p.ceil() as i32);
                        let kept = |u: i32| short(&(a.clone() * BigRational::from_float(1.0 - impurity(*p, u)).unwrap_or_else(|| rat(0))));
                        format!("{p} (impure): used as {} keeps {}, used as {} keeps {} (item 62)", label(lo), kept(lo), label(hi), kept(hi))
                    }
                })
                .collect();
            Ok(format!("Lens {op}: {}", each.join("; ")))
        }
        // impurity 6.25 6
        Some("impurity") => {
            let p = num(1).ok_or("impurity needs: impurity 6.25 6")?;
            let u = num(2).ok_or("impurity needs what it is used as")? as i32;
            Ok(format!("{p} used as {}: {:.0}% lost", label(u), impurity(p, u) * 100.0))
        }
        // crystal destroy 500 | crystal charge 10 60 (cm³, minutes) | crystal time 10 (cm³)
        Some("crystal") => match w.get(1).map(|v| v.to_lowercase()).as_deref() {
            Some("destroy") => {
                let a = amt(2).ok_or("crystal destroy needs what it stored")?;
                Ok(format!("The crystal is destroyed: {} amplitude is released (10x what it stored), can't be stored, and can be funneled through only 2 lenses. What is left decays entirely into lead.", short(&(a * rat(10)))))
            }
            Some("charge") | Some("drain") => {
                let cm = amt(2).ok_or("crystal charge needs: crystal charge <cm³> <minutes>")?;
                let min = amt(3).unwrap_or_else(|| rat(1));
                Ok(format!("{} amplitude (1 amplitude per minute per cm³; the recharge is the decay, and no decay happens without recharge).", short(&(cm * min))))
            }
            Some("time") => {
                let cm = amt(2).ok_or("crystal time needs the crystal's size in cm³")?;
                Ok(format!("Destroying it moves the user {} seconds in time (a 10 cm³ crystal: 10 seconds); only the user keeps memories. At most a minute either way unless class or species deems otherwise.", short(&cm)))
            }
            _ => Err("crystal needs: destroy <stored>, charge <cm³> <minutes>, or time <cm³>".into()),
        },
        Some("battery") => {
            let cm = amt(1).ok_or("battery needs its size in cm³ and minutes: battery 10 60")?;
            let min = amt(2).unwrap_or_else(|| rat(1));
            Ok(format!("An attuned battery near an actinide crystal gains {} amplitude in that time (1 per minute per cm³; very slow).", short(&(cm * min))))
        }
        _ => Err("unknown crafting sum".into()),
    }
}

// ------------------------------------------------------------------ what the GM and the player see

/// The status window for the System: the true values, plus what it needs to judge (system.py `sheet` and `for_system`).
pub fn status_text(s: &Sheet, defs: &Defs) -> String {
    let st = s.stats(defs);
    let growth = s.growth(defs);
    let xp_next = if s.level < MAX_LEVEL { level_xp_needed(s.level) } else { bonus_xp_needed(s.bp) };
    let level = if s.bp > 0 { format!("{} Bp{}", s.level, s.bp) } else { s.level.to_string() };
    let mut t = format!(
        "{} - Level {level} | XP {}/{} | Species: {} ({}) | Class: {} ({})",
        s.name,
        short(&s.xp.0),
        xp_next,
        s.shown_name("species", None, defs),
        s.tier_name("species", None),
        s.shown_name("class", None, defs),
        s.tier_name("class", None)
    );
    t.push_str(&format!(
        "\n{} | per level: {}",
        STATS.iter().map(|k| format!("{k} {}", st[*k])).collect::<Vec<_>>().join(" · "),
        STATS.iter().map(|k| format!("{k} +{}", growth[*k])).collect::<Vec<_>>().join(", ")
    ));
    let derived: Vec<String> = s.derived(defs).into_iter().map(|(n, v)| format!("{n} {}", v.map(|v| short(&v)).unwrap_or_else(|e| e))).collect();
    t.push_str(&format!("\nDerived: {}", derived.join(" · ")));
    let mut prism = s.prism.clone();
    let top = s.amplitude_max(defs);
    prism.fill(&top);
    let specs: Vec<String> = prism
        .held
        .iter()
        .map(|p| {
            let f = freq(*p);
            let mut x = format!("{} {} ({}) {}/{}", label(*p), f.name, f.spectrum, short(&prism.have(*p)), short_int(&prism.top(*p, &top)));
            let d = prism.dec(*p);
            if d.is_positive() {
                x.push_str(&format!(", overused {:.1}%", d.to_f64().unwrap_or(0.0) * 100.0));
            }
            if prism.blocked(*p) {
                x.push_str(", BLOCKED");
            }
            x
        })
        .collect();
    t.push_str(&format!("\nSpectrums: {}", specs.join("; ")));
    t.push_str(&format!("\nEnvironment: {}", if s.env_unbalanced { "unbalanced: all stats -20%, frequency regen halved".into() } else { s.env_why.clone() }));
    if s.pain > 0.0 {
        t.push_str(&format!(" · Overuse pain {:.1}", s.pain));
    }
    let skills: Vec<String> = s
        .skills
        .iter()
        .map(|(k, v)| {
            let called = s.shown_name("skill", Some(k), defs);
            let name = if called != *k { format!("{called} [{k}]") } else { k.clone() };
            let ab = if v.abilities.is_empty() { String::new() } else { format!(", abilities: {}", v.abilities.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", ")) };
            format!("{name} ({}) level {}, {}, xp {}/{}{ab}", v.kind, v.level, s.tier_name("skill", Some(k)), short(&v.xp.0), level_xp_needed(v.level))
        })
        .collect();
    t.push_str(&format!("\nSkills: {}", if skills.is_empty() { "none".into() } else { skills.join("; ") }));
    for (tag, opts) in &s.pending {
        t.push_str(&format!("\nPaths open for {tag} (the player picks): {}", opts.iter().enumerate().map(|(i, o)| format!("{}. {}", i + 1, o.name)).collect::<Vec<_>>().join(" | ")));
    }
    if !s.feats.is_empty() || !s.titles.is_empty() {
        t.push_str(&format!("\nFeats: {} · Titles: {}", s.feats.iter().map(|f| f.name.clone()).collect::<Vec<_>>().join(", "), s.titles.join(", ")));
    }
    if !s.boosts.is_empty() {
        t.push_str(&format!("\nBoosts: {}", s.boosts.iter().map(|(k, v)| format!("{k} {v:+}")).collect::<Vec<_>>().join(", ")));
    }
    if s.bp > 0 {
        t.push_str(&format!("\nBonus points: {} earned, {} to spend", s.bp, s.bp_free));
    }
    let items = if s.inventory.is_empty() { "(nothing)".to_string() } else { s.inventory.iter().map(|i| if i.qty > 1 { format!("{} x{}", i.name, i.qty) } else { i.name.clone() }).collect::<Vec<_>>().join(", ") };
    t.push_str(&format!("\nMoney: {} · Carrying: {items}", s.money));
    if !s.conditions.is_empty() {
        t.push_str(&format!("\nConditions: {}", s.conditions.join(", ")));
    }
    // What the System needs to judge (never told to the player).
    let mut open = Vec::new();
    if s.racial_open() {
        open.push(format!("a racial skill (pick one; revival skills come about {} of the time for them now: {})", g4(revival_share(s.revival_held(defs))), defs.revival().join(", ")));
    }
    if s.class_open() {
        open.push("a class skill".into());
    }
    if s.cls.is_none() && s.offered.is_empty() {
        open.push("no class yet: offer one when their deeds earn it".into());
    }
    if s.can_evolve_class() && !s.pending.contains_key("class") {
        open.push("a class evolution (give five paths)".into());
    }
    if s.can_evolve_species() && !s.pending.contains_key("species") {
        let can: Vec<String> = openings(&s.prism.held).iter().map(|p| format!("{} {}", label(*p), freq(*p).spectrum)).collect();
        open.push(format!("a species evolution (give five paths; one opens a frequency: {})", can.join(", ")));
    }
    for k in s.skills.keys().filter(|k| s.can_tier_skill(k) && !s.pending.contains_key(&format!("skill:{k}"))) {
        open.push(format!("{k} can evolve (give five paths)"));
    }
    for a in &s.bp_asked {
        open.push(format!("they bought a tier past the last ({a}): name it with `beyond`"));
    }
    let used: Vec<String> = s.skills.iter().filter(|(_, v)| v.used > 0.0).map(|(k, v)| format!("{k} {:.1}", v.used)).chain(s.attempts.iter().filter(|(_, v)| **v > 0.0).map(|(k, v)| format!("{k} {v:.1} (not held)"))).collect();
    let next = format!(
        "next class tier at level {}, next species tier at level {}, next racial skill at level {}{}",
        if s.cls.is_some() && s.class_tier < CLASS_TIERS.len() - 1 { class_tier_level(s.class_tier).to_string() } else { "-".into() },
        if s.species_tier < SPECIES_TIERS.len() - 1 { species_tier_level(s.species_tier).to_string() } else { "-".into() },
        racial_skill_level(s.species_tier, s.racial_held()),
        if s.cls.is_some() { format!(", next class skill at level {}", class_skill_level(s.class_tier, s.class_held())) } else { String::new() }
    );
    t.push_str(&format!("\n(System only) Open: {} · {next}", if open.is_empty() { "nothing".into() } else { open.join("; ") }));
    if !used.is_empty() {
        t.push_str(&format!("\n(System only) Skill use since your last look: {}", used.join(", ")));
    }
    t
}

/// The sheet as the player sees it (item 73: only their own spectrums).
pub fn sheet_view(s: &Sheet, defs: &Defs) -> Value {
    let top = s.amplitude_max(defs);
    let mut prism = s.prism.clone();
    prism.fill(&top);
    let spectrums: Vec<Value> = prism
        .held
        .iter()
        .map(|p| {
            let f = freq(*p);
            let semi: Vec<String> = builtin().semi.keys().filter(|k| semi_base(k) == *p).map(|k| semi_name(k)).collect();
            let cap = prism.top(*p, &top);
            let have = prism.have(*p);
            let pct = if cap.is_zero() { 0.0 } else { (have.clone() / BigRational::from_integer(cap.clone())).to_f64().unwrap_or(0.0) * 100.0 };
            json!({"number": label(*p), "name": f.name, "spectrum": f.spectrum, "does": f.does,
                   "amplitude": format!("{}/{}", short(&have), short_int(&cap)), "percent": pct,
                   "semi_skills": semi, "overused_percent": prism.dec(*p).to_f64().unwrap_or(0.0) * 100.0, "blocked": prism.blocked(*p)})
        })
        .collect();
    let xp_next = if s.level < MAX_LEVEL { level_xp_needed(s.level) } else { bonus_xp_needed(s.bp) };
    json!({
        "ruleset": "ad",
        "name": s.name,
        "species": s.shown_name("species", None, defs), "species_tier": s.tier_name("species", None),
        "species_about": defs.species(&s.species).and_then(|v| v["about"].as_str()).unwrap_or(""),
        "class": s.cls.as_ref().map(|_| s.shown_name("class", None, defs)), "class_tier": s.tier_name("class", None),
        "classes_available": if s.cls.is_none() { s.offered.iter().map(|k| json!({"key": k, "name": defs.class(k).and_then(|v| v["name"].as_str()).unwrap_or(k), "about": defs.class(k).and_then(|v| v["about"].as_str()).unwrap_or("")})).collect::<Vec<_>>() } else { vec![] },
        "paths_open": s.pending.iter().map(|(tag, opts)| (tag.clone(), json!(opts.iter().enumerate().map(|(i, o)| json!({"pick": i + 1, "name": o.name, "about": o.about, "yours_alone": o.unique})).collect::<Vec<_>>()))).collect::<Map<String, Value>>(),
        "level": if s.bp > 0 { format!("{} Bp{}", s.level, s.bp) } else { s.level.to_string() }, "max_level": MAX_LEVEL,
        "xp": short(&s.xp.0), "xp_next": xp_next, "xp_percent": (s.xp.0.to_f64().unwrap_or(0.0) / xp_next.max(1) as f64 * 100.0).min(100.0),
        "bonus_points": {"earned": s.bp, "to_spend": s.bp_free, "stat_percent": s.stat_pct.iter().filter(|(_, v)| **v > 0).map(|(k, v)| (k.clone(), json!(*v as f64 / 10.0))).collect::<Map<String, Value>>()},
        "stats": s.stats(defs), "per_level": s.growth(defs),
        "derived": s.derived(defs).into_iter().map(|(n, v)| (n, json!(v.map(|v| short(&v)).unwrap_or_else(|e| e)))).collect::<Map<String, Value>>(),
        "spectrums": spectrums,
        "environment": if s.env_unbalanced { "unbalanced: all stats -20%, frequency regen halved".to_string() } else if s.shares.as_ref().is_some_and(|m| m.is_empty()) { "no frequency here".into() } else { "balanced".into() },
        "pain": s.pain,
        "skills": s.skills.iter().map(|(k, v)| (k.clone(), json!({"called": s.shown_name("skill", Some(k), defs), "does": defs.skill(k).and_then(|d| d["about"].as_str().map(String::from)).unwrap_or_default(),
            "level": v.level, "tier": s.tier_name("skill", Some(k)), "kind": v.kind, "experience": format!("{}/{}", short(&v.xp.0), level_xp_needed(v.level)),
            "abilities": v.abilities.iter().map(|a| json!({"name": a.name, "about": a.about})).collect::<Vec<_>>()}))).collect::<Map<String, Value>>(),
        "feats": s.feats.iter().map(|f| json!({"name": f.name, "about": f.about, "rewards": f.rewards})).collect::<Vec<_>>(),
        "titles": s.titles, "boosts": s.boosts,
        "money": s.money, "inventory": s.inventory, "conditions": s.conditions, "bp_asked": s.bp_asked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> Defs {
        Defs::default()
    }

    fn ten() -> BTreeMap<String, i64> {
        STATS.iter().map(|s| (s.to_string(), 10)).collect()
    }

    fn world_with(name: &str) -> World {
        let mut w = World::default();
        let s = Sheet::new(name, "human", ten(), &w.defs);
        w.sheets.insert(name.into(), s);
        w
    }

    #[test]
    fn the_level_curve_is_his() {
        // "About 438 for level 2, 3,200 around level 100, 12,300 at the cap; it levels off near 14,800."
        assert!((430..445).contains(&level_xp_needed(1)), "{}", level_xp_needed(1));
        assert!((3000..3400).contains(&level_xp_needed(99)));
        assert!((12000..12600).contains(&level_xp_needed(MAX_LEVEL - 1)));
    }

    #[test]
    fn tiers_open_at_his_levels() {
        assert_eq!((0..6).map(class_tier_level).collect::<Vec<_>>(), vec![5, 25, 125, 625, 3125, 15625]);
        assert_eq!((0..3).map(species_tier_level).collect::<Vec<_>>(), vec![25, 625, 15625]);
        assert_eq!(racial_skill_level(0, 2), 100);
        assert_eq!(class_skill_level(1, 3), 75);
        assert_eq!(tier_of_level(4), 0);
        assert_eq!(tier_of_level(5), 1);
        assert_eq!(tier_of_level(15625), 6);
    }

    #[test]
    fn the_difference_multiplier_follows_his_table() {
        assert_eq!(difference_multiplier(10, 10, 0, 0), 1.0);
        // same band (5-24, 20 wide): 5 levels above is a quarter: x2
        assert!((difference_multiplier(10, 15, 0, 0) - 2.0).abs() < 1e-9);
        // and below, a base character gets exactly the half
        assert!((difference_multiplier(15, 10, 0, 0) - 0.5).abs() < 1e-9);
        // another band: at least x4
        assert!(difference_multiplier(3, 30, 0, 0) >= 4.0);
    }

    #[test]
    fn amplitude_is_int_to_the_hundredth_exactly() {
        let w = world_with("Wren");
        let s = &w.sheets["Wren"];
        assert_eq!(s.amplitude_max(&w.defs), pow10(100));
        assert_eq!(short_int(&pow10(100)), "1.00e100");
        assert_eq!(short(&rat(438)), "438");
        assert_eq!(short(&ratio(1, 3)), "0.3333");
    }

    #[test]
    fn the_formula_is_exact_and_refuses_the_unfinishable() {
        let st = ten();
        assert_eq!(formula("dex ^ str * 0.01", &st).unwrap(), BigRational::from_integer(pow10(8)));
        assert_eq!(formula("str ^ hp * 0.1", &st).unwrap(), BigRational::from_integer(pow10(9)));
        assert_eq!(formula("-(2+3)*4/2", &st).unwrap(), rat(-10));
        assert!(formula("int ^ 99999999", &st).is_err());
        assert!(formula("os.system", &st).is_err());
    }

    #[test]
    fn every_prism_holds_5_and_the_roll_never_changes() {
        for n in ["Wren", "Ottilie", "Dunn Hale", "A", "B", "C"] {
            let a = roll(n);
            assert!(a.contains(&5));
            assert_eq!(a, roll(n));
            assert!(a.iter().all(|p| (0..=10).contains(p)));
        }
        // about 1 in 10 hold 4 or 6
        let with = (0..2000).filter(|i| roll(&format!("x{i}")).contains(&6)).count();
        assert!((120..290).contains(&with), "{with}");
    }

    #[test]
    fn runs_and_openings_follow_item_43() {
        assert_eq!(runs(&[2, 4, 5, 6]), vec![(2, 2), (4, 6)]);
        assert_eq!(openings(&[5]), vec![4, 6]);
        assert_eq!(openings(&[2, 5]), vec![1, 3, 4, 6]);
        assert_eq!(openings(&[-3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]), Vec::<i32>::new());
    }

    #[test]
    fn regen_is_one_a_minute_at_int_one() {
        // item 54: a level 0 character with 1 INT gathers 1 a minute
        assert_eq!(rate(1, &BigInt::one(), false), BigInt::one());
        // INT 10: INT^2 x (10^100)^(1/20) = 100 x 10^5
        assert_eq!(rate(10, &pow10(100), false), BigInt::from(10_000_000));
        assert_eq!(rate(10, &pow10(100), true), BigInt::from(20_000_000));
    }

    #[test]
    fn an_unbalanced_environment_halves_regen_and_takes_a_fifth_off_stats() {
        let mut w = world_with("Wren");
        let held = w.sheets["Wren"].prism.held.clone();
        let other = (0..=10).find(|p| !held.contains(p)).unwrap();
        let line = format!("surroundings {} 0.9, base 0.1", freq(other).spectrum);
        let r = w.apply_line(&line, "Wren").unwrap();
        assert!(r.starts_with(SYSTEM_ONLY));
        let s = &w.sheets["Wren"];
        assert!(s.env_unbalanced);
        assert_eq!(s.true_stat("INT", &w.defs), 8);
        w.apply_line("surroundings balanced", "Wren").unwrap();
        assert_eq!(w.sheets["Wren"].true_stat("INT", &w.defs), 10);
        // an alien zone: no frequency, no regen, no debuff (item 56)
        w.apply_line("surroundings alien", "Wren").unwrap();
        assert!(!w.sheets["Wren"].env_unbalanced);
    }

    #[test]
    fn overuse_decreases_the_maximum_and_hurts() {
        let mut w = world_with("Wren");
        let d = w.defs.clone();
        let s = w.sheets.get_mut("Wren").unwrap();
        let top = BigRational::from_integer(s.amplitude_max(&d));
        // spend everything and half as much again
        let out = s.spend(5, &(top.clone() * ratio(3, 2)), &d).unwrap();
        assert!((out.decrease_percent - 50.0).abs() < 1e-6);
        assert!(s.pain >= OVERUSE_PAIN_MIN);
        assert_eq!(s.prism.top(5, &s.amplitude_max(&d)), pow10(100) / 2);
        // overused past its whole maximum, it no longer answers
        s.spend(5, &top, &d).unwrap();
        assert!(s.prism.blocked(5));
        assert!(s.spend(5, &rat(1), &d).is_err());
    }

    #[test]
    fn semi_skills_need_overpower_on_a_person() {
        let mut w = world_with("Wren");
        let s = Sheet::new("Ottilie", "human", ten(), &w.defs);
        w.sheets.insert("Ottilie".into(), s);
        let r = w.apply_line("semi solids 100 at Ottilie", "Wren").unwrap();
        assert!(r.contains("resists"), "{r}");
        w.apply_line("Ottilie: consent Wren", "Wren").unwrap();
        let r = w.apply_line("semi solids 100 at Ottilie", "Wren").unwrap();
        assert!(!r.contains("resists"), "{r}");
        // a level 3 wolf has 30 amplitude: 300 overpowers it
        assert!(!w.apply_line("semi solids 300 at the wolf level 3", "Wren").unwrap().contains("resists"));
        assert!(w.apply_line("semi solids 299 at the wolf level 3", "Wren").unwrap().contains("resists"));
        // 100 amplitude gives the base power
        assert!((semi_power("5", &rat(100), 0) - 3.0 * (101f64).log10() / 2.0).abs() < 1e-9);
    }

    #[test]
    fn levels_grow_stats_by_the_set_amounts() {
        let mut w = world_with("Wren");
        let d = w.defs.clone();
        let s = w.sheets.get_mut("Wren").unwrap();
        s.gain(BigInt::from(level_xp_needed(1)), None, &d);
        assert_eq!(s.level, 2);
        // human growth: HP 2, STR 1, INT 2, DEX 1
        assert_eq!(s.stats(&d)["HP"], 12);
        assert_eq!(s.stats(&d)["INT"], 12);
        assert!(s.drain().iter().any(|m| m.contains("LEVEL UP")));
    }

    #[test]
    fn a_racial_slot_is_open_from_the_start_and_the_system_fills_it() {
        let mut w = world_with("Wren");
        assert!(w.sheets["Wren"].racial_open());
        w.apply_line("skill + Adaptable racial", "Wren").unwrap();
        assert!(!w.sheets["Wren"].racial_open());
        // the next opens at level 50 ^ 1 x 1
        assert_eq!(racial_skill_level(0, 1), 50);
        assert!(w.apply_line("skill + Basket Weaving", "Wren").is_err());
        w.apply_line("skill + Basket Weaving | Making baskets from reeds and grass.", "Wren").unwrap();
        assert!(w.sheets["Wren"].skills.contains_key("Basket Weaving"));
    }

    #[test]
    fn tier_ups_are_five_paths_and_the_basics_are_shared() {
        let mut w = world_with("Wren");
        let s2 = Sheet::new("Ann", "human", ten(), &w.defs);
        w.sheets.insert("Ann".into(), s2);
        for n in ["Wren", "Ann"] {
            w.apply_line(&format!("{n}: skill + Observation"), n).unwrap();
            w.apply_line(&format!("{n}: skill xp Observation 99999"), n).unwrap();
            assert!(w.sheets[n].can_tier_skill("Observation"));
        }
        let five = "paths skill Observation | Hawk Eye: sees twice as far as before | Patient Eye: notices what moves slowly | Wren's Glance: reads tracks at a single look [INT +1] | Tidewatch: knows the tide by the shore's colour | Lens Mind: sees the grain inside a stone";
        assert!(w.apply_line("paths skill Observation | A: too short", "Wren").is_err());
        w.apply_line(five, "Wren").unwrap();
        // Ann's first two are the same basics, whatever the System writes
        w.apply_line("Ann: paths skill Observation | Ann One: her very own first path | Ann Two: her very own second path | Ann Three: her very own third path", "Ann").unwrap();
        let a = &w.sheets["Ann"].pending["skill:Observation"];
        assert_eq!((a[0].name.as_str(), a[1].name.as_str(), a[2].name.as_str()), ("Hawk Eye", "Patient Eye", "Ann One"));
        assert!(!a[1].unique && a[2].unique);
        w.apply_line("choose skill Observation 3", "Wren").unwrap();
        let s = &w.sheets["Wren"];
        assert_eq!(s.skills["Observation"].tier, 1);
        assert_eq!(s.shown_name("skill", Some("Observation"), &w.defs), "Wren's Glance");
        assert_eq!(s.adjust["INT"], 1);
    }

    #[test]
    fn a_feat_carries_an_upgrade_only_past_its_bar() {
        let mut w = world_with("Wren");
        w.apply_line("define class wright | Wright | Makes things that last. | growth HP 1, STR 1, INT 2, DEX 1", "Wren").unwrap();
        w.apply_line("class offer wright", "Wren").unwrap();
        w.apply_line("class choose Wright", "Wren").unwrap();
        let r = w.apply_line("feat Bridge Builder | Raised a bridge alone | xp 50 | upgrade class 0.01", "Wren").unwrap();
        assert!(r.contains("above the bar"), "{r}");
        assert_eq!(w.sheets["Wren"].class_tier, 0);
        w.apply_line("feat Storm Wall | Held back the sea | upgrade class 0.0005", "Wren").unwrap();
        assert_eq!(w.sheets["Wren"].class_tier, 1);
        assert_eq!(w.sheets["Wren"].feats.len(), 2);
    }

    #[test]
    fn species_evolution_opens_a_frequency_next_to_a_run() {
        let mut w = world_with("Wren");
        let d = w.defs.clone();
        let s = w.sheets.get_mut("Wren").unwrap();
        s.level = 25;
        let can = openings(&s.prism.held);
        let mut basics = BTreeMap::new();
        let opts: Vec<PathOpt> = (0..5).map(|i| PathOpt { name: format!("Path {i}"), about: "a path with enough words".into(), frequency: Some(can[0]), ..Default::default() }).collect();
        s.offer("species", None, opts, &mut basics, &d).unwrap();
        s.choose("species", 1, &d).unwrap();
        assert_eq!(s.species_tier, 1);
        assert!(s.prism.held.contains(&can[0]));
    }

    #[test]
    fn past_the_cap_experience_buys_bonus_points() {
        let mut w = world_with("Wren");
        let d = w.defs.clone();
        let s = w.sheets.get_mut("Wren").unwrap();
        s.level = MAX_LEVEL;
        s.gain(BigInt::from(bonus_xp_needed(0) + bonus_xp_needed(1)), None, &d);
        assert_eq!((s.bp, s.bp_free), (2, 2));
        s.spend_bp("stat", "STR", 2, None, &d).unwrap();
        assert_eq!(s.stat_pct["STR"], 2);
        assert!(s.spend_bp("class_tier", "", 1, None, &d).is_err());
    }

    #[test]
    fn defeats_follow_his_formula() {
        let mut w = world_with("Wren");
        let r = w.apply_line("defeat 1 a rat", "Wren").unwrap();
        // a level 1 enemy against a level 1 character: 5 x 11 ^ 0.6
        assert!(r.contains(&format!("+{}", (5.0 * 11f64.powf(0.6)) as u64)), "{r}");
    }

    #[test]
    fn the_crafting_sums_are_his_and_for_the_system_alone() {
        let mut w = world_with("Wren");
        let r = w.apply_line("lens merge 6 9 300 200", "Wren").unwrap();
        assert!(r.starts_with(SYSTEM_ONLY) && r.contains("7.5") && r.contains("250"), "{r}");
        assert!(w.apply_line("lens compress 12 100 1", "Wren").is_err());
        assert!(w.apply_line("impurity 6.25 7", "Wren").unwrap().contains("75%"));
        assert!(w.apply_line("crystal destroy 50", "Wren").unwrap().contains("500"));
        w.apply_line("craft Hand lens | ground from clear quartz", "Wren").unwrap();
        assert!(w.sheets["Wren"].skills.contains_key("Crafting"));
    }

    #[test]
    fn big_numbers_survive_the_file() {
        let w = world_with("Wren");
        let mut s = w.sheets["Wren"].clone();
        s.xp = Big(BigRational::from_integer(pow10(400)));
        s.prism.decrease.insert(5, Big(ratio(1, 3)));
        let back: Sheet = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.xp, s.xp);
        assert_eq!(back.prism.decrease[&5], Big(ratio(1, 3)));
        assert_eq!(to_xp("3.2e5"), BigInt::from(320000));
    }

    #[test]
    fn the_system_can_rename_its_words_and_no_number_moves() {
        let mut w = world_with("Wren");
        let before = w.sheets["Wren"].clone();
        w.apply_line("rename amplitude -> Aether", "Wren").unwrap();
        w.apply_line("rename solids -> Stoneborn", "Wren").unwrap();
        w.apply_line("rename Aether -> Ether", "Wren").unwrap(); // by its new name too
        assert_eq!(w.labels["amplitude"], "Ether");
        assert_eq!(w.sheets["Wren"], before);
        let t = relabel(&status_text(&w.sheets["Wren"], &w.defs), &w.labels);
        assert!(t.contains("Ether") && t.contains("Stoneborn") && !t.contains("amplitude"));
        // the GM may write the story's names back
        assert!(w.apply_line("spend Stoneborn 10", "Wren").is_ok());
        assert!(w.apply_line("rename speed -> Ether", "Wren").is_err()); // taken
        assert!(w.apply_line("rename mana -> Ki", "Wren").is_err()); // not a System word
    }

    #[test]
    fn players_see_only_their_own_spectrums() {
        let w = world_with("Wren");
        let v = sheet_view(&w.sheets["Wren"], &w.defs);
        let shown: Vec<String> = v["spectrums"].as_array().unwrap().iter().map(|x| x["number"].as_str().unwrap().to_string()).collect();
        assert_eq!(shown, w.sheets["Wren"].prism.held.iter().map(|p| label(*p)).collect::<Vec<_>>());
        assert!(!v.to_string().contains("System only"));
    }
}
