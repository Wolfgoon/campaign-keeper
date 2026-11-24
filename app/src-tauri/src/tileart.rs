//! The picture map's tiles and tokens, drawn for each campaign by the helper model as SVG drawings
//! (no generative painting), then put through `pixel`'s k-means in Oklab, down to a 64x64 sprite
//! with a small palette, so every tile shares the same chunky look.
//!
//! Natural ground gets several different drawings (`grass`, `grass~1`, …) and the page also flips
//! and turns them, so a field of grass hardly ever shows the same tile twice in a row.
//!
//! The art is styled after the campaign's world overview, kept in `.keeper/tileset/`, and made one
//! piece at a time in the background: the map shows plain colour until a piece arrives.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine as _;

use crate::campaign::keeper;
use crate::config::Config;
use crate::tilemap::{KINDS, TERRAIN};
use crate::util::atomic_write;
use crate::{engine, pixel};

pub const SIZE: u32 = 64;
const COLORS: usize = 16;

/// What each piece should show. Tiles fill the whole square and repeat; tokens stand on a plain background.
fn subject(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "grass" => ("a patch of meadow grass seen from straight above, small tufts and a few flowers", false),
        "forest" => ("dense tree canopy seen from above, two or three round tree crowns filling the square", false),
        "water" => ("shallow clear water seen from above with small light ripples", false),
        "deep" => ("deep dark water seen from above, a few faint waves", false),
        "sand" => ("sand seen from above, fine grain and a soft ripple", false),
        "road" => ("a cobblestone road surface seen from above", false),
        "dirt" => ("bare packed earth seen from above with a few pebbles", false),
        "floor" => ("wooden floor planks seen from above", false),
        "wall" => ("a stone brick wall face, rows of bricks", false),
        "mountain" => ("a single rocky mountain peak with a snow cap, top-down game map style", false),
        "snow" => ("fresh snow seen from above, soft blue shadows", false),
        "swamp" => ("murky swamp seen from above, dark puddles and reeds", false),
        "lava" => ("molten lava seen from above, glowing cracks", false),
        "bridge" => ("wooden bridge planks running top to bottom over water, seen from above", false),
        "door" => ("a wooden door set in a stone wall, front view", false),
        "building" => ("a small house seen from a top-down game angle: tiled roof covering the top two thirds, a wall with a door and windows below", false),
        "cave" => ("a dark cave mouth in grey rock", false),
        "farm" => ("ploughed farm field rows with green sprouts, seen from above", false),
        "rock" => ("rough grey rocky ground seen from above", false),
        "hero" => ("a brave adventurer hero standing, full body, facing the viewer", true),
        "ally" => ("a friendly companion in green clothes standing, full body, facing the viewer", true),
        "npc" => ("an ordinary villager standing, full body, facing the viewer", true),
        "enemy" => ("a hostile armed bandit in red standing, full body, facing the viewer", true),
        "monster" => ("a menacing monster creature, full body", true),
        "item" => ("a glowing treasure item, a pouch with sparkles", true),
        "chest" => ("a closed wooden treasure chest with gold trim", true),
        "camp" => ("a small tent with a campfire beside it", true),
        "quest" => ("a tall yellow flag with a big exclamation mark", true),
        _ => return None,
    })
}

/// Ground that's drawn several times over, so it doesn't repeat.
pub const NATURAL: &[&str] = &["grass", "forest", "water", "deep", "sand", "dirt", "snow", "swamp", "lava", "rock", "farm", "road", "floor", "wall", "mountain", "cave"];
/// How many different drawings each natural ground gets.
pub const VARIANTS: usize = 4;

/// Every piece the map uses: the terrains (void is drawn as black), their extra drawings
/// ("grass~1"), and the token kinds.
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = TERRAIN.iter().map(|(_, n)| n.to_string()).filter(|n| n != "void").collect();
    for n in NATURAL {
        for k in 1..VARIANTS {
            v.push(format!("{n}~{k}"));
        }
    }
    v.extend(KINDS.iter().map(|k| k.to_string()));
    v
}

/// "grass~2" -> ("grass", 2).
fn base(name: &str) -> (&str, usize) {
    match name.split_once('~') {
        Some((b, k)) => (b, k.parse().unwrap_or(0)),
        None => (name, 0),
    }
}

fn folder(dir: &Path) -> PathBuf {
    keeper(dir).join("tileset")
}

fn data_url(b: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))
}

/// name -> data URL for every piece made so far. A named character's or thing's own sprite is
/// under "token:<slug>" (the page slugs names the same way).
pub fn all(dir: &Path) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = names()
        .into_iter()
        .filter_map(|n| { let b = std::fs::read(folder(dir).join(format!("{n}.png"))).ok()?; Some((n, data_url(&b))) })
        .collect();
    for e in std::fs::read_dir(folder(dir)).into_iter().flatten().flatten() {
        let f = e.file_name().to_string_lossy().into_owned();
        if let Some(s) = f.strip_prefix("t_").and_then(|f| f.strip_suffix(".png")) {
            if let Ok(b) = std::fs::read(e.path()) {
                out.insert(format!("token:{s}"), data_url(&b));
            }
        }
    }
    out
}

/// The same slug the page uses to find a named sprite.
pub fn slug(name: &str) -> String {
    name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).take(60).collect()
}

/// A named sprite is drawn once and kept: the same Wren on every turn and every player's screen.
const MAX_NAMED: usize = 120;

fn named_path(dir: &Path, name: &str) -> PathBuf {
    folder(dir).join(format!("t_{}.png", slug(name)))
}

/// What the campaign's files say about someone or something, so their sprite looks like them.
fn described(dir: &Path, name: &str) -> String {
    let want = slug(name);
    let first = want.split('_').find(|w| w.len() > 2).unwrap_or(&want).to_string();
    let mut found = String::new();
    for sub in ["01_Character", "03_NPCs", "02_World/Items_and_Lore", "02_World/Factions"] {
        for e in std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten() {
            let stem = slug(&e.path().file_stem().unwrap_or_default().to_string_lossy());
            let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
            if stem == want || (!first.is_empty() && stem.contains(&first)) {
                return text.chars().take(700).collect();
            }
            // A line in a shared file like minor_npcs.md.
            if found.is_empty() {
                if let Some(l) = text.lines().find(|l| l.to_lowercase().contains(&name.trim().to_lowercase())) {
                    found = l.chars().take(300).collect();
                }
            }
        }
    }
    found
}

fn make_named(cfg: &Config, dir: &Path, name: &str, kind: &str) -> Result<(), String> {
    let (base, _) = subject(kind).ok_or_else(|| format!("unknown kind {kind}"))?;
    let about = described(dir, name);
    let what = if about.is_empty() { format!("{name}: {base}") } else { format!("{name}, {base}. From the campaign's notes: {about}") };
    let world = style(dir);
    let png = from_model(cfg, name, &what, true, &world, "")?;
    atomic_write(&named_path(dir, name), &png)
}

pub fn missing(dir: &Path) -> Vec<String> {
    names().into_iter().filter(|n| !folder(dir).join(format!("{n}.png")).exists()).collect()
}

/// The campaign's own look, from its world overview, so a desert world gets desert grass.
fn style(dir: &Path) -> String {
    let world = std::fs::read_to_string(dir.join("02_World/world_overview.md")).unwrap_or_default();
    let world: String = world.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#') && !l.contains("_TEMPLATE")).collect::<Vec<_>>().join(" ").chars().take(500).collect();
    if world.trim().is_empty() { "classic fantasy adventure".into() } else { world }
}

const SVG_PROMPT: &str = "\
You are a pixel artist making one 64x64 sprite for a top-down tile map in a story game. Reply with ONE complete SVG and nothing \
else: <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\" width=\"64\" height=\"64\">…</svg>. Use only rect, circle, \
ellipse, polygon, path and g with solid fill colours; no text, no images, no gradients, no filters. Bold, readable shapes with \
a darker outline and a light highlight side. A TILE must cover the whole 64x64 square edge to edge and sit well next to itself: keep its \
edges the plain base colour and scatter details (tufts, stones, ripples, cracks, leaves) unevenly, never in a grid. A TOKEN stands alone on a transparent background, centred, feet near the bottom, filling most of the height.";

fn from_model(cfg: &Config, name: &str, what: &str, token: bool, world: &str, extra: &str) -> Result<Vec<u8>, String> {
    let user = format!("Make the {} '{name}': {what}.{extra}\nThe game's world, for colours and mood: {world}", if token { "TOKEN" } else { "TILE" });
    let reply = engine::helper(cfg, SVG_PROMPT, &user)?;
    let start = reply.find("<svg").ok_or("the model sent no picture")?;
    let end = reply.rfind("</svg>").ok_or("the model's picture was cut off")? + 6;
    pixel::from_svg(&reply[start..end], &pixel::Opts { size: SIZE, colors: COLORS, dither: false, palette: None })
}

/// After k-means the backdrop is one or two palette colours: whatever matches the corners goes clear.
#[allow(dead_code)]
fn cut_background(png: &[u8]) -> Result<Vec<u8>, String> {
    let mut img = image::load_from_memory(png).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = img.dimensions();
    let corners: HashSet<[u8; 3]> = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)].iter().map(|&(x, y)| { let p = img.get_pixel(x, y).0; [p[0], p[1], p[2]] }).collect();
    for p in img.pixels_mut() {
        if corners.contains(&[p.0[0], p.0[1], p.0[2]]) {
            p.0[3] = 0;
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Make one piece and keep it.
pub fn make(cfg: &Config, dir: &Path, name: &str) -> Result<(), String> {
    let (b, k) = base(name);
    let (what, token) = subject(b).ok_or_else(|| format!("unknown piece {name}"))?;
    let world = style(dir);
    // Each extra drawing of the same ground is told to differ, so neighbours don't match.
    let extra = if k == 0 { String::new() } else {
        let looks = ["details clustered toward one corner", "a sparse, mostly plain patch", "one larger feature off-centre and small bits elsewhere"];
        format!(" This is drawing {} of {VARIANTS} of the same ground: same colours, but {}, so it doesn't look like the others.", k + 1, looks[(k - 1) % looks.len()])
    };
    let png = from_model(cfg, b, what, token, &world, &extra)?;
    atomic_write(&folder(dir).join(format!("{name}.png")), &png)
}

/// One filler at a time, and a piece that failed isn't retried until the app restarts.
static FILLING: Mutex<()> = Mutex::new(());
static FAILED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Make whatever is missing, calling `made` after each piece so the page can show it.
pub fn fill(cfg: &Config, dir: &Path, mut made: impl FnMut(&str), mut failed: impl FnMut(&str, &str)) {
    let Ok(_g) = FILLING.try_lock() else { return };
    // Tiles made before 3.2.0 may be paintings: clear them once so they're all drawings.
    let mark = folder(dir).join("drawn-v2");
    if !mark.exists() {
        let _ = std::fs::remove_dir_all(folder(dir));
        let _ = std::fs::create_dir_all(folder(dir));
        let _ = std::fs::write(&mark, "");
    }
    for name in missing(dir) {
        let key = format!("{}|{name}", dir.display());
        if FAILED.lock().map(|f| f.as_ref().is_some_and(|s| s.contains(&key))).unwrap_or(false) {
            continue;
        }
        match make(cfg, dir, &name) {
            Ok(()) => made(&name),
            Err(e) => {
                if let Ok(mut f) = FAILED.lock() {
                    f.get_or_insert_with(HashSet::new).insert(key);
                }
                failed(&name, &e);
            }
        }
    }
    // Then everyone and everything on the map gets their own sprite, once.
    let Some(area) = crate::tilemap::current(dir) else { return };
    for (name, t) in &area.tokens {
        let count = std::fs::read_dir(folder(dir)).map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("t_")).count()).unwrap_or(0);
        if count >= MAX_NAMED {
            break;
        }
        let key = format!("{}|t_{}", dir.display(), slug(name));
        if named_path(dir, name).exists() || FAILED.lock().map(|f| f.as_ref().is_some_and(|s| s.contains(&key))).unwrap_or(false) {
            continue;
        }
        match make_named(cfg, dir, name, &t.kind) {
            Ok(()) => made(name),
            Err(e) => {
                if let Ok(mut f) = FAILED.lock() {
                    f.get_or_insert_with(HashSet::new).insert(key);
                }
                failed(name, &e);
            }
        }
    }
}

/// Throw the art away so it's all drawn again (after the world changes, or if it came out badly).
pub fn reset(dir: &Path) {
    let _ = std::fs::remove_dir_all(folder(dir));
    if let Ok(mut f) = FAILED.lock() {
        *f = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_has_a_subject() {
        for n in names() {
            assert!(subject(base(&n).0).is_some(), "{n}");
        }
    }

    #[test]
    fn a_model_svg_becomes_a_64px_reduced_sprite() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64"><rect width="64" height="64" fill="#3a7"/><circle cx="32" cy="32" r="20" fill="#153"/><rect x="10" y="10" width="8" height="8" fill="#fe5"/></svg>"##;
        let png = pixel::from_svg(svg, &pixel::Opts { size: SIZE, colors: COLORS, dither: false, palette: None }).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (64, 64));
        let colours: HashSet<[u8; 4]> = img.pixels().map(|p| p.0).collect();
        assert!(colours.len() <= COLORS);
    }

    #[test]
    fn a_named_sprite_is_found_by_its_name_and_its_file_describes_it() {
        let d = std::env::temp_dir().join(format!("ck-art-{}", uuid::Uuid::new_v4())).join("c");
        crate::campaign::create(&d, "T").unwrap();
        std::fs::write(d.join("03_NPCs/old_maren.md"), "# Old Maren
A one-eyed fisherwoman in a yellow oilskin.").unwrap();
        assert!(described(&d, "Old Maren").contains("yellow oilskin"));
        std::fs::create_dir_all(folder(&d)).unwrap();
        std::fs::write(named_path(&d, "Old Maren"), [1u8, 2, 3]).unwrap();
        assert!(all(&d).contains_key("token:old_maren"));
    }

    #[test]
    fn token_backgrounds_go_clear() {
        let mut img = image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 255, 255]));
        img.put_pixel(32, 32, image::Rgba([10, 20, 30, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let out = image::load_from_memory(&cut_background(png.get_ref()).unwrap()).unwrap().to_rgba8();
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
        assert_eq!(out.get_pixel(32, 32).0[3], 255);
    }
}
