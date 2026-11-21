//! The GM's replies carry tags the app acts on and the player never sees: the private notebook,
//! pictures, music, activity tabs, voices, and the LitRPG changes.

use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize, Default, Debug, Clone)]
pub struct Parsed {
    /// What the player reads.
    pub display: String,
    pub notebook: Option<String>,
    pub pictures: Vec<Value>,
    pub music: Option<Value>,
    pub activity: Option<Value>,
    /// Who says what, in order, for reading aloud: {name, attrs, text}.
    pub voices: Vec<Value>,
    pub rpg_lines: Vec<String>,
    pub rpg_sheet: Option<Value>,
    pub ambience: Option<Value>,
    /// {name, body, edit}
    pub maps: Vec<Value>,
    pub save_block: Option<String>,
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("valid pattern")
}

fn attrs(s: &str) -> Value {
    let mut m = serde_json::Map::new();
    for c in re(r#"([a-zA-Z_]+)\s*=\s*"([^"]*)""#).captures_iter(s) {
        m.insert(c[1].to_string(), json!(c[2].to_string()));
    }
    Value::Object(m)
}

/// Take every match of a block out of `text`, handing each (attrs, body) to `f`.
fn take(text: &mut String, pattern: &str, mut f: impl FnMut(Value, &str)) {
    let r = re(pattern);
    let found: Vec<(Value, String)> = r.captures_iter(text).map(|c| (attrs(c.get(1).map_or("", |m| m.as_str())), c.get(2).map_or("", |m| m.as_str()).to_string())).collect();
    for (a, b) in found {
        f(a, &b);
    }
    *text = r.replace_all(text, "").into_owned();
}

pub fn parse(reply: &str) -> Parsed {
    let mut p = Parsed::default();
    let mut t = reply.to_string();
    take(&mut t, r"(?s)<gm_notebook()>(.*?)</gm_notebook>", |_, b| p.notebook = Some(b.trim().to_string()));
    take(&mut t, r"(?s)<rpg_sheet()>(.*?)</rpg_sheet>", |_, b| p.rpg_sheet = serde_json::from_str(b.trim()).ok().or_else(|| crate::util::json_object(b)));
    take(&mut t, r"(?s)<rpg()>(.*?)</rpg>", |_, b| {
        p.rpg_lines.extend(b.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string));
    });
    take(&mut t, r"(?s)<picture\b([^>]*?)>(.*?)</picture>", |a, b| {
        let mut pic = a.clone();
        pic["svg"] = json!(b.trim());
        p.pictures.push(pic);
    });
    take(&mut t, r"(?s)<sprite\b([^>]*)>(.*?)</sprite>", |a, b| {
        let mut pic = a.clone();
        pic["sprite"] = json!(b.trim_matches(['\n', '\r']));
        p.pictures.push(pic);
    });
    // In the order written: a map drawn and then edited in one reply has to go in that order.
    let mut found: Vec<(usize, Value)> = Vec::new();
    for (pattern, edit) in [(r"(?s)<map_edit\b([^>]*)>(.*?)</map_edit>", true), (r"(?s)<map\b([^>]*)>(.*?)</map>", false)] {
        let r = re(pattern);
        for c in r.captures_iter(&t) {
            found.push((c.get(0).map_or(0, |m| m.start()), json!({ "name": attrs(&c[1])["name"], "body": &c[2], "edit": edit })));
        }
        t = r.replace_all(&t, "").into_owned();
    }
    found.sort_by_key(|f| f.0);
    p.maps = found.into_iter().map(|f| f.1).collect();
    take(&mut t, r"<ambience\b([^>]*?)/()>", |a, _| p.ambience = Some(a));
    take(&mut t, r"<picture\b([^>]*?)/()>", |a, _| p.pictures.push(a));
    take(&mut t, r"(?s)<music\b([^>]*?[^/])>(.*?)</music>", |a, b| {
        let mut m = a.clone();
        m["score"] = json!(b.trim());
        p.music = Some(m);
    });
    take(&mut t, r"<music\b([^>]*?)/()>", |a, _| {
        p.music = Some(if a["title"].is_string() { json!({ "title": a["title"], "replay": true }) } else { json!({ "stop": true }) });
    });
    take(&mut t, r"(?s)<activity\b([^>]*)>(.*?)</activity>", |a, b| {
        p.activity = Some(activity(a, b));
    });
    let vr = re(r#"(?s)<voice\b([^>]*)>(.*?)</voice>"#);
    for c in vr.captures_iter(&t) {
        let a = attrs(&c[1]);
        p.voices.push(json!({ "name": a["name"], "attrs": a, "text": c[2].trim() }));
    }
    t = vr.replace_all(&t, "$2").into_owned();
    if let (Some(a), Some(b)) = (t.find("=== SAVE BLOCK ==="), t.find("=== END SAVE ===")) {
        if b > a {
            p.save_block = Some(t[a..b + "=== END SAVE ===".len()].to_string());
        }
    }
    p.display = t.trim().to_string();
    p
}

fn activity(mut a: Value, body: &str) -> Value {
    let mut body = body.to_string();
    let mut art = serde_json::Map::new();
    for part in ["surface", "icon", "brush"] {
        take(&mut body, &format!(r"(?s)<{part}()>(.*?)</{part}>"), |_, b| {
            art.insert(part.to_string(), json!(b.trim()));
        });
    }
    let mut item_art = serde_json::Map::new();
    take(&mut body, r#"(?s)<art\b([^>]*)>(.*?)</art>"#, |x, b| {
        if let Some(n) = x["for"].as_str() {
            item_art.insert(n.to_string(), json!(b.trim()));
        }
    });
    let (mut items, mut tools, mut prompt) = (Vec::new(), Vec::new(), Vec::new());
    for l in body.lines().map(str::trim) {
        if let Some(i) = l.strip_prefix("- item:") {
            items.push(i.trim().to_string());
        } else if let Some(i) = l.strip_prefix("- tool:") {
            tools.push(i.trim().to_string());
        } else if !l.is_empty() {
            prompt.push(l.to_string());
        }
    }
    a["prompt"] = json!(prompt.join("\n"));
    a["items"] = json!(items);
    a["tools"] = json!(tools);
    a["art"] = Value::Object(art);
    a["item_art"] = Value::Object(item_art);
    a
}

/// What's safe to show of a reply still streaming: finished tags removed, and anything after the
/// start of an unfinished one held back.
pub fn visible(partial: &str) -> String {
    let p = parse(partial);
    let mut t = p.display;
    for tag in ["<gm_notebook", "<rpg", "<picture", "<sprite", "<music", "<activity", "<map", "<ambience"] {
        if let Some(i) = t.find(tag) {
            t.truncate(i);
        }
    }
    // A voice tag being written: hide just the tag.
    t = re(r"</?voice\b[^>]*>?").replace_all(&t, "").into_owned();
    if let Some(i) = t.rfind('<') {
        if !t[i..].contains('>') {
            t.truncate(i);
        }
    }
    t.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_out_every_tag() {
        let r = r#"The door opens.
<voice name="Tobin" gender="male" age="old">"Welcome back."</voice>
You step in.
<picture title="The Key" kind="item">
<svg viewBox="0 0 4 3"></svg>
</picture>
<music title="Yard" tempo="84">
pad: Dm7 - - -
</music>
<activity type="crafting" title="Mend it" tool="Awl">
Fix the wheel.
- item: Broken wheel
- tool: Hammer
<art for="Hammer"><svg/></art>
</activity>
<rpg>
xp +25
item + Rope
</rpg>
<gm_notebook>
secret plans
</gm_notebook>"#;
        let p = parse(r);
        assert_eq!(p.display, "The door opens.\n\"Welcome back.\"\nYou step in.");
        assert_eq!(p.notebook.as_deref(), Some("secret plans"));
        assert_eq!(p.pictures[0]["title"], "The Key");
        assert_eq!(p.music.unwrap()["tempo"], "84");
        let a = p.activity.unwrap();
        assert_eq!(a["type"], "crafting");
        assert_eq!(a["items"][0], "Broken wheel");
        assert_eq!(a["tools"][0], "Hammer");
        assert_eq!(a["prompt"], "Fix the wheel.");
        assert_eq!(p.rpg_lines, vec!["xp +25", "item + Rope"]);
        assert_eq!(p.voices[0]["name"], "Tobin");
    }

    #[test]
    fn replays_and_stops() {
        assert_eq!(parse(r#"x <music title="A"/>"#).music.unwrap()["replay"], true);
        assert_eq!(parse("x <music stop/>").music.unwrap()["stop"], true);
        assert_eq!(parse(r#"x <picture title="K"/>"#).pictures[0]["title"], "K");
    }

    #[test]
    fn streaming_never_leaks_a_secret() {
        for cut in 0.."Hi there.\n<gm_notebook>\nthe butler did it\n</gm_notebook>".len() {
            let s = "Hi there.\n<gm_notebook>\nthe butler did it\n</gm_notebook>";
            if !s.is_char_boundary(cut) {
                continue;
            }
            assert!(!visible(&s[..cut]).contains("butler"), "leaked at {cut}");
        }
        assert_eq!(visible("Hi <voice name=\"A\">\"yo"), "Hi \"yo");
    }

    #[test]
    fn new_art_map_and_sound_tags() {
        let r = r##"Night falls.
<picture title="Key" kind="item" px="64" colors="8" palette="sepia" dither="on"><svg viewBox="0 0 4 3"></svg></picture>
<sprite title="Rat" kind="npc" palette="a:#112233">
.a.
aaa
</sprite>
<map name="Varrow">
+--+
</map>
<map_edit name="Varrow">
marker @ 1 0 Wren
</map_edit>
<ambience sounds="rain, sea" level="0.4"/>
<music title="T" tempo="90" form="A B A" swing="0.1" reverb="0.5" loop="no">
[A]
pad: Dm - - -
</music>"##;
        let p = parse(r);
        assert_eq!(p.display, "Night falls.");
        assert_eq!(p.pictures[0]["px"], "64");
        assert_eq!(p.pictures[0]["palette"], "sepia");
        assert_eq!(p.pictures[1]["sprite"], ".a.\naaa");
        assert_eq!((p.maps.len(), p.maps[0]["edit"].as_bool(), p.maps[1]["edit"].as_bool()), (2, Some(false), Some(true)));
        assert_eq!(p.ambience.unwrap()["sounds"], "rain, sea");
        let m = p.music.unwrap();
        assert_eq!((m["form"].as_str(), m["loop"].as_str()), (Some("A B A"), Some("no")));
        assert!(m["score"].as_str().unwrap().contains("[A]"));
    }

    #[test]
    fn unfinished_new_tags_stay_hidden_while_streaming() {
        for s in ["ok <map name=\"V\">\n+--secret", "ok <sprite title=\"R\">\nsecret", "ok <map_edit name=\"V\">secret"] {
            assert_eq!(visible(s), "ok", "{s}");
        }
    }

    #[test]
    fn a_painted_picture_keeps_its_prompt_and_a_replay_has_none() {
        let p = parse(r#"x <picture title="Harbor" kind="scene" prompt="a wet quay at dusk/night" px="160"/> y <picture title="Key"/>"#);
        assert_eq!(p.pictures[0]["prompt"], "a wet quay at dusk/night");
        assert_eq!(p.pictures[0]["px"], "160");
        assert!(p.pictures[1]["prompt"].is_null());
        assert_eq!(p.display, "x  y");
    }

    #[test]
    fn finds_save_blocks() {
        let p = parse("ok\n=== SAVE BLOCK ===\nStuff\n=== END SAVE ===\nbye");
        assert!(p.save_block.unwrap().starts_with("=== SAVE BLOCK ==="));
    }
}
