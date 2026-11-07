//! Local embeddings with bge-small-en-v1.5, the model Spirit's memory uses, but in-process and in
//! pure Rust (Candle): no Python, no service, no native library to ship. The model (about 130 MB)
//! downloads the first time and loads in the background. Until then, and if it never can, search
//! simply runs on words alone.

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

use crate::config::local_dir;

const OFF: u8 = 0;
const LOADING: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;

const REPO: &str = "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main";
const FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];
/// What the model's authors say to put in front of a search query.
const QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";
const MAX_TOKENS: usize = 512;
const BATCH: usize = 16;

struct Model {
    bert: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

static STATE: AtomicU8 = AtomicU8::new(OFF);
static MODEL: Mutex<Option<Model>> = Mutex::new(None);
static ERROR: Mutex<String> = Mutex::new(String::new());

/// "off", "loading", "ready", or "failed: why".
pub fn status() -> String {
    match STATE.load(Ordering::Relaxed) {
        OFF => "off".into(),
        LOADING => "loading".into(),
        READY => "ready".into(),
        _ => format!("failed: {}", ERROR.lock().map(|e| e.clone()).unwrap_or_default()),
    }
}

fn download(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for f in FILES {
        let to = dir.join(f);
        if to.exists() {
            continue;
        }
        let resp = ureq::get(&format!("{REPO}/{f}")).call().map_err(|e| format!("couldn't download {f}: {e}"))?;
        let tmp = dir.join(format!("{f}.part"));
        let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        std::io::copy(&mut resp.into_reader(), &mut out).map_err(|e| format!("download of {f} stopped: {e}"))?;
        drop(out);
        std::fs::rename(&tmp, &to).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn load(dir: &Path) -> Result<Model, String> {
    let device = Device::Cpu;
    let cfg: Config = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| e.to_string())?;
    tokenizer.with_padding(Some(PaddingParams::default()));
    tokenizer
        .with_truncation(Some(TruncationParams { max_length: MAX_TOKENS, ..Default::default() }))
        .map_err(|e| e.to_string())?;
    // SAFETY: the file is ours, written once by `download` and never changed while mapped.
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], DType::F32, &device) }.map_err(|e| e.to_string())?;
    let bert = BertModel::load(vb, &cfg).map_err(|e| e.to_string())?;
    Ok(Model { bert, tokenizer, device })
}

/// Start loading the model, once, off the calling thread.
pub fn warm() {
    if std::env::var("CK_NO_EMBED").is_ok() || STATE.compare_exchange(OFF, LOADING, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }
    std::thread::spawn(|| {
        let dir = local_dir().join("models").join("bge-small-en-v1.5");
        match download(&dir).and_then(|_| load(&dir)) {
            Ok(m) => {
                if let Ok(mut g) = MODEL.lock() {
                    *g = Some(m);
                }
                STATE.store(READY, Ordering::SeqCst);
            }
            Err(e) => {
                if let Ok(mut g) = ERROR.lock() {
                    *g = e;
                }
                STATE.store(FAILED, Ordering::SeqCst);
            }
        }
    });
}

impl Model {
    /// The [CLS] token's state, normalized: how bge models are meant to be read.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let enc = self.tokenizer.encode_batch(texts.to_vec(), true).map_err(|e| e.to_string())?;
        let ids: Vec<u32> = enc.iter().flat_map(|e| e.get_ids().to_vec()).collect();
        let mask: Vec<u32> = enc.iter().flat_map(|e| e.get_attention_mask().to_vec()).collect();
        let (n, len) = (enc.len(), enc[0].get_ids().len());
        let e = |x: candle_core::Error| x.to_string();
        let ids = Tensor::from_vec(ids, (n, len), &self.device).map_err(e)?;
        let mask = Tensor::from_vec(mask, (n, len), &self.device).map_err(e)?;
        let types = ids.zeros_like().map_err(e)?;
        let out = self.bert.forward(&ids, &types, Some(&mask)).map_err(e)?;
        let cls = out.narrow(1, 0, 1).map_err(e)?.squeeze(1).map_err(e)?;
        let v: Vec<Vec<f32>> = cls.to_vec2().map_err(e)?;
        Ok(v.into_iter().map(unit).collect())
    }
}

fn run(texts: Vec<String>) -> Option<Vec<Vec<f32>>> {
    if STATE.load(Ordering::Relaxed) != READY {
        return None;
    }
    let g = MODEL.lock().ok()?;
    let m = g.as_ref()?;
    let mut out = Vec::with_capacity(texts.len());
    for batch in texts.chunks(BATCH) {
        out.extend(m.embed(batch).ok()?);
    }
    Some(out)
}

fn unit(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

pub fn passages(texts: &[String]) -> Option<Vec<Vec<f32>>> {
    run(texts.to_vec())
}

pub fn passage(text: &str) -> Option<Vec<f32>> {
    run(vec![text.to_string()])?.pop()
}

pub fn query(text: &str) -> Option<Vec<f32>> {
    run(vec![format!("{QUERY_PREFIX}{text}")])?.pop()
}

/// Run by hand with the model downloaded: `cargo test --lib real_model -- --ignored`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "downloads the model"]
    fn real_model_finds_meaning_not_words() {
        std::env::remove_var("CK_NO_EMBED");
        warm();
        for _ in 0..900 {
            if status() != "loading" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        assert_eq!(status(), "ready");
        let docs: Vec<String> = ["Bram Oakes owes Wren a life-debt after she hauled the rope.", "The tavern serves mutton stew on market days.", "A brass button stamped G.H.L. lay in the mud."].map(String::from).to_vec();
        let d = passages(&docs).unwrap();
        assert_eq!(d[0].len(), 384);
        let cos = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        let q = query("who is indebted to me for saving his life?").unwrap();
        assert!(cos(&q, &d[0]) > cos(&q, &d[1]) && cos(&q, &d[0]) > cos(&q, &d[2]));
        let q = query("what's for dinner at the inn").unwrap();
        assert!(cos(&q, &d[1]) > cos(&q, &d[0]));
    }
}
