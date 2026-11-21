//! Pixel art. The GM's SVG pictures are rendered in pure Rust, shrunk, and reduced to a small
//! palette with k-means, in the Oklab colour space so the palette looks right to the eye. The
//! GM can also draw a sprite pixel by pixel.
//!
//! No randomness here either: k-means starts from a farthest-first seed, so the same picture
//! always gives the same image.

use std::sync::{Arc, OnceLock};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

const MAX_SVG: usize = 96 * 1024;
const OVERSAMPLE: u32 = 4;
const ITERATIONS: usize = 14;
pub const MAX_SIZE: u32 = 256;
pub const MIN_SIZE: u32 = 16;

#[derive(Clone, Debug)]
pub struct Opts {
    /// Width of the finished image in pixels.
    pub size: u32,
    pub colors: usize,
    pub dither: bool,
    /// Use exactly these colours instead of finding them.
    pub palette: Option<Vec<[u8; 3]>>,
}

fn named_palette(name: &str) -> Option<&'static [&'static str]> {
    Some(match name.to_lowercase().as_str() {
        "gameboy" => &["#0f380f", "#306230", "#8bac0f", "#9bbc0f"],
        "sepia" => &["#2b1d0e", "#5a3e22", "#8c6a3f", "#c2a26f", "#efe4c8"],
        "night" => &["#0b1026", "#1d2b53", "#3a4f8a", "#7e9ccc", "#e6edf7", "#f0b43c"],
        "ember" => &["#14080a", "#4a1414", "#a02c1a", "#e0651c", "#f7b73c", "#fbecc0"],
        "forest" => &["#0d1b12", "#1f3d2a", "#3f7a4a", "#8fb56a", "#e3e0b0"],
        "ice" => &["#0c1a2b", "#2a4d6e", "#5c93b8", "#a9d3e6", "#f2fbff"],
        _ => return None,
    })
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

/// A palette attribute: a name, or colours separated by commas or spaces.
pub fn palette_from(attr: &str) -> Option<Vec<[u8; 3]>> {
    if let Some(n) = named_palette(attr.trim()) {
        return Some(n.iter().filter_map(|c| parse_hex(c)).collect());
    }
    let v: Vec<[u8; 3]> = attr.split([',', ' ']).filter(|x| !x.is_empty()).filter_map(parse_hex).collect();
    (v.len() >= 2).then_some(v)
}

fn fonts() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

/// RGBA at the given width. A model wrote this SVG, so it may not reach for files or the network.
fn rasterize(svg: &str, width: u32) -> Result<(Vec<u8>, u32, u32), String> {
    if svg.len() > MAX_SVG {
        return Err("the picture is too large".into());
    }
    let mut opt = usvg::Options::default();
    opt.fontdb = fonts();
    opt.image_href_resolver.resolve_string = Box::new(|_, _| None);
    opt.image_href_resolver.resolve_data = Box::new(|_, _, _| None);
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let scale = width as f32 / size.width().max(1.0);
    let height = ((size.height() * scale).ceil() as u32).clamp(1, width.saturating_mul(4));
    let mut map = Pixmap::new(width, height).ok_or("bad picture size")?;
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut map.as_mut());
    Ok((map.data().to_vec(), width, height))
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(srgb_to_linear);
    let l = (0.41222147 * r + 0.53633254 * g + 0.05144599 * b).cbrt();
    let m = (0.2119035 * r + 0.6806995 * g + 0.10739696 * b).cbrt();
    let s = (0.08830246 * r + 0.28171884 * g + 0.6299787 * b).cbrt();
    [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s]
}

fn from_oklab(lab: [f32; 3]) -> [u8; 3] {
    let [l, a, b] = lab;
    let lp = l + 0.3963377774 * a + 0.2158037573 * b;
    let mp = l - 0.1055613458 * a - 0.0638541728 * b;
    let sp = l - 0.0894841775 * a - 1.2914855480 * b;
    let (x, y, z) = (lp * lp * lp, mp * mp * mp, sp * sp * sp);
    let rgb = [4.0767416621 * x - 3.3077115913 * y + 0.2309699292 * z, -1.2684380046 * x + 2.6097574011 * y - 0.3413193965 * z, -0.0041960863 * x - 0.7034186147 * y + 1.707614701 * z];
    rgb.map(|c| (linear_to_srgb(c) * 255.0).round() as u8)
}

fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

fn nearest(p: &[f32; 3], c: &[[f32; 3]]) -> usize {
    let mut best = (0, f32::MAX);
    for (i, x) in c.iter().enumerate() {
        let d = dist(p, x);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

/// K-means with a fixed start: the mean colour, then each next seed is the colour farthest from
/// the seeds so far. Same pixels, same palette.
pub fn kmeans(points: &[[f32; 3]], k: usize) -> Vec<[f32; 3]> {
    if points.is_empty() {
        return Vec::new();
    }
    let n = points.len() as f32;
    let mean = points.iter().fold([0.0; 3], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n, a[2] + p[2] / n]);
    let mut centres = vec![mean];
    while centres.len() < k {
        let far = points
            .iter()
            .map(|p| (p, centres.iter().map(|c| dist(p, c)).fold(f32::MAX, f32::min)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|x| x.1 > 1e-6);
        match far {
            Some((p, _)) => centres.push(*p),
            None => break, // fewer distinct colours than asked for
        }
    }
    for _ in 0..ITERATIONS {
        let mut sum = vec![[0.0f32; 3]; centres.len()];
        let mut count = vec![0u32; centres.len()];
        for p in points {
            let i = nearest(p, &centres);
            for d in 0..3 {
                sum[i][d] += p[d];
            }
            count[i] += 1;
        }
        let mut moved = 0.0;
        for i in 0..centres.len() {
            if count[i] > 0 {
                let c = [sum[i][0] / count[i] as f32, sum[i][1] / count[i] as f32, sum[i][2] / count[i] as f32];
                moved += dist(&c, &centres[i]);
                centres[i] = c;
            }
        }
        if moved < 1e-7 {
            break;
        }
    }
    // A seed nothing settled on (the mean, between two clear groups) isn't a colour in the picture.
    let mut used = vec![false; centres.len()];
    for p in points {
        used[nearest(p, &centres)] = true;
    }
    let mut kept: Vec<[f32; 3]> = centres.into_iter().zip(used).filter(|x| x.1).map(|x| x.0).collect();
    kept.sort_by(|a, b| a[0].total_cmp(&b[0]));
    kept
}

const BAYER: [[f32; 4]; 4] = [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];

fn encode(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut map = Pixmap::new(w, h).ok_or("bad picture size")?;
    map.data_mut().copy_from_slice(rgba);
    map.encode_png().map_err(|e| e.to_string())
}

/// A finished low-resolution PNG from an SVG.
pub fn from_svg(svg: &str, o: &Opts) -> Result<Vec<u8>, String> {
    let (big, bw, bh) = rasterize(svg, o.size.clamp(MIN_SIZE, MAX_SIZE) * OVERSAMPLE)?;
    reduce(&big, bw, bh, o)
}

/// Pixel art from a painting (PNG, JPEG or WebP bytes).
pub fn from_image(bytes: &[u8], o: &Opts) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("the picture couldn't be read: {e}"))?.to_rgba8();
    let (w, h) = img.dimensions();
    reduce(img.as_raw(), w, h, o)
}

/// A painting shrunk to a sensible size, for when pixel art is off.
pub fn shrink_image(bytes: &[u8], width: u32) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("the picture couldn't be read: {e}"))?;
    let small = img.resize(width, width * 2, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut out = std::io::Cursor::new(Vec::new());
    small.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn reduce(big: &[u8], bw: u32, bh: u32, o: &Opts) -> Result<Vec<u8>, String> {
    let size = o.size.clamp(MIN_SIZE, MAX_SIZE).min(bw);
    let w = size;
    let h = ((bh as u64 * size as u64 / bw as u64) as u32).max(1);
    // Average each block, so thin lines survive shrinking.
    let mut cells: Vec<([f32; 3], f32)> = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        let (y0, y1) = (y * bh / h, ((y + 1) * bh / h).max(y * bh / h + 1));
        for x in 0..w {
            let (x0, x1) = (x * bw / w, ((x + 1) * bw / w).max(x * bw / w + 1));
            let (mut c, mut alpha) = ([0.0f32; 3], 0.0f32);
            for sy in y0..y1.min(bh) {
                for sx in x0..x1.min(bw) {
                    let i = ((sy * bw + sx) * 4) as usize;
                    let a = big[i + 3] as f32 / 255.0;
                    // Weighted by coverage, so a transparent corner adds no colour. Rendered SVGs
                    // are premultiplied already; paintings are opaque, where both forms agree.
                    for d in 0..3 {
                        c[d] += big[i + d] as f32 / 255.0;
                    }
                    alpha += a;
                }
            }
            let n = ((y1.min(bh) - y0) * (x1.min(bw) - x0)) as f32;
            if alpha > 0.0 {
                c = c.map(|v| v / alpha);
            }
            cells.push((c, alpha / n));
        }
    }
    let solid: Vec<[f32; 3]> = cells.iter().filter(|c| c.1 >= 0.5).map(|c| to_oklab(c.0)).collect();
    let centres = match &o.palette {
        Some(p) => p.iter().map(|c| to_oklab(c.map(|v| v as f32 / 255.0))).collect(),
        None => kmeans(&solid, o.colors.clamp(2, 64)),
    };
    if centres.is_empty() {
        return Err("the picture came out empty".into());
    }
    let spread = 0.05;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (i, (c, a)) in cells.iter().enumerate() {
        if *a < 0.5 {
            continue;
        }
        let (x, y) = (i as u32 % w, i as u32 / w);
        let mut lab = to_oklab(*c);
        if o.dither {
            lab[0] += (BAYER[(y % 4) as usize][(x % 4) as usize] / 16.0 - 0.47) * spread;
        }
        let rgb = from_oklab(centres[nearest(&lab, &centres)]);
        rgba[i * 4..i * 4 + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    encode(&rgba, w, h)
}

/// A sprite the GM drew by hand: one character per pixel, with a key like "a:#112233 b:#ffcc00".
/// Space and `.` are see-through.
pub fn sprite(rows: &str, key: &str) -> Result<Vec<u8>, String> {
    let colours: Vec<(char, [u8; 3])> = key
        .split_whitespace()
        .filter_map(|p| {
            let (c, hex) = p.split_once(':')?;
            Some((c.chars().next()?, parse_hex(hex)?))
        })
        .collect();
    if colours.is_empty() {
        return Err("a sprite needs a palette, like a:#112233 b:#ffcc00".into());
    }
    let lines: Vec<Vec<char>> = rows.lines().map(|l| l.trim_end().chars().collect::<Vec<_>>()).filter(|l| !l.is_empty()).collect();
    let w = lines.iter().map(Vec::len).max().unwrap_or(0) as u32;
    let h = lines.len() as u32;
    if w == 0 || w > MAX_SIZE || h > MAX_SIZE {
        return Err("a sprite must be 1 to 256 pixels each way".into());
    }
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (y, l) in lines.iter().enumerate() {
        for (x, ch) in l.iter().enumerate() {
            if let Some((_, c)) = colours.iter().find(|(k, _)| k == ch) {
                let i = (y * w as usize + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
    }
    encode(&rgba, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 300">
      <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#16324a"/><stop offset="1" stop-color="#e8a33d"/></linearGradient></defs>
      <rect width="400" height="300" fill="url(#g)"/>
      <circle cx="300" cy="80" r="40" fill="#fff6e0"/>
      <path d="M0 300 L120 140 L240 300 Z" fill="#102015"/></svg>"##;

    fn opts(colors: usize) -> Opts {
        Opts { size: 64, colors, dither: false, palette: None }
    }

    fn pixels(png: &[u8]) -> Pixmap {
        Pixmap::decode_png(png).unwrap()
    }

    fn distinct(p: &Pixmap) -> usize {
        let mut s = std::collections::HashSet::new();
        for c in p.data().chunks_exact(4) {
            s.insert([c[0], c[1], c[2], c[3]]);
        }
        s.len()
    }

    #[test]
    fn the_palette_really_is_reduced() {
        for k in [4, 8, 16] {
            let p = pixels(&from_svg(SVG, &opts(k)).unwrap());
            assert!(distinct(&p) <= k, "{k} colours asked, {} used", distinct(&p));
            assert_eq!((p.width(), p.height()), (64, 48));
        }
    }

    #[test]
    fn the_same_picture_always_gives_the_same_image() {
        let a = from_svg(SVG, &opts(12)).unwrap();
        for _ in 0..5 {
            assert_eq!(a, from_svg(SVG, &opts(12)).unwrap());
        }
    }

    #[test]
    fn kmeans_finds_clear_groups() {
        let mut pts = vec![[0.1, 0.0, 0.0]; 50];
        pts.extend(vec![[0.9, 0.1, 0.0]; 50]);
        let c = kmeans(&pts, 2);
        assert_eq!(c.len(), 2);
        assert!(dist(&c[0], &pts[0]) < 1e-4 && dist(&c[1], &pts[99]) < 1e-4);
        assert_eq!(kmeans(&pts, 40).len(), 2); // can't invent colours
        assert!(kmeans(&[], 4).is_empty());
    }

    #[test]
    fn named_and_custom_palettes_are_obeyed() {
        let want: Vec<[u8; 3]> = palette_from("gameboy").unwrap();
        let o = Opts { palette: Some(want.clone()), ..opts(16) };
        let p = pixels(&from_svg(SVG, &o).unwrap());
        for c in p.data().chunks_exact(4) {
            assert!(want.contains(&[c[0], c[1], c[2]]));
        }
        assert_eq!(palette_from("#ff0000, #00ff00").unwrap().len(), 2);
        assert!(palette_from("not a palette").is_none());
    }

    #[test]
    fn dithering_is_deterministic_too() {
        let o = Opts { dither: true, ..opts(6) };
        assert_eq!(from_svg(SVG, &o).unwrap(), from_svg(SVG, &o).unwrap());
    }

    #[test]
    fn transparency_survives() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="0" y="0" width="5" height="10" fill="#ff0000"/></svg>"##;
        let p = pixels(&from_svg(svg, &Opts { size: 16, ..opts(4) }).unwrap());
        assert_eq!(p.data()[3], 255);
        assert_eq!(p.data()[(15 * 4 + 3) as usize], 0);
    }

    #[test]
    fn a_picture_cannot_reach_for_files() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10"><rect width="10" height="10" fill="red"/><image href="C:\Windows\win.ini" width="10" height="10"/></svg>"#;
        assert!(from_svg(svg, &opts(4)).is_ok());
    }

    #[test]
    fn a_painting_becomes_pixel_art_the_same_way_every_time() {
        // A smooth 512x384 gradient: thousands of colours going in.
        let img = image::RgbaImage::from_fn(512, 384, |x, y| image::Rgba([(x / 2) as u8, (y * 255 / 384) as u8, ((x + y) / 4) as u8, 255]));
        let mut b = std::io::Cursor::new(Vec::new());
        img.write_to(&mut b, image::ImageFormat::Png).unwrap();
        let png = b.into_inner();
        let a = from_image(&png, &opts(8)).unwrap();
        assert_eq!(a, from_image(&png, &opts(8)).unwrap());
        let p = pixels(&a);
        assert_eq!((p.width(), p.height()), (64, 48));
        assert!(distinct(&p) <= 8);
        assert!(from_image(b"not an image", &opts(8)).is_err());
        assert_eq!(pixels(&shrink_image(&png, 256).unwrap()).width(), 256);
    }

    #[test]
    fn bad_input_is_an_error_not_a_crash() {
        assert!(from_svg("not svg", &opts(4)).is_err());
        assert!(from_svg(&"x".repeat(MAX_SVG + 1), &opts(4)).is_err());
    }

    #[test]
    fn sprites_draw_exactly_what_was_written() {
        let p = pixels(&sprite("ab.\n.ab", "a:#ff0000 b:#0000ff").unwrap());
        assert_eq!((p.width(), p.height()), (3, 2));
        assert_eq!(&p.data()[0..4], &[255, 0, 0, 255]);
        assert_eq!(&p.data()[4..8], &[0, 0, 255, 255]);
        assert_eq!(p.data()[11], 0);
        assert!(sprite("ab", "nonsense").is_err());
    }
}
