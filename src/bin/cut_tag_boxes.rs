//! 公開求人のスクショ（collect_recruit_samples の出力）から、タグ枠 5 個を探して切り抜く
//! ローカル専用ツール。切り抜きは後で軽量 OCR のテンプレになる（その部分はここでは扱わない）。
//!
//! ```text
//! cargo run --release --bin cut_tag_boxes -- [--in local/recruit_samples] [--limit N] [--only <ファイル名の一部>] [--debug-dir <dir>]
//! ```
//!
//! - 対象は画面と軸が揃ったスクショだけ。モニターを撮った写真（遠近）や大陸版は対象外で、
//!   枠が見つからなければ「見つからず」で良い（そちらは Google Vision に回す）。
//! - 検出は縮小コピーで行い、切り抜きは元の解像度から行う。結果は毎回同じ（決定的）。
//! - 出力は `<in>/crops/`（`<タグ>/<message_id>_<attachment_id>_<pos>.png` と index.jsonl）と
//!   `<in>/detect_report.jsonl`。実行のたびに crops/ は作り直す。

use astesiabot_rust::engine::recruit::tag_ocr::detect::{crop_box, detect, Color, Detection, Rect};
use image::{Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug, Deserialize)]
struct Label {
    tags: Vec<String>,
    is_global: bool,
    usable: bool,
}

#[derive(Debug, Deserialize)]
struct Record {
    message_id: String,
    attachment_id: String,
    posted_at: String,
    file: String,
    label: Option<Label>,
}

#[derive(Debug, Serialize)]
struct CropRecord {
    tag: String,
    file: String,
    source: String,
    pos: usize,
    bbox: [u32; 4],
    color: &'static str,
    /// 黄色の枠とエリート系タグの並びが食い違う画像（ラベルの順が画面の順と違う疑い）。
    order_suspect: bool,
    source_width: u32,
    source_height: u32,
    posted_at: String,
}

#[derive(Debug, Serialize)]
struct Report {
    file: String,
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    grid_bbox: Option<[u32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    box_size: Option<[u32; 2]>,
    width: u32,
    height: u32,
    candidates: usize,
}

/// 黄色く光る枠になるタグ。ラベルの順が画面の位置と合っているかの検算に使う。
const YELLOW_TAGS: [&str; 2] = ["上級エリート", "エリート"];

/// タグと枠の色の食い違い（エリート系なのに黄色でない／そうでないのに黄色）が 1 つでもあるか。
fn order_suspect(tags: &[String], colors: &[Color]) -> bool {
    tags.iter().zip(colors).any(|(t, c)| YELLOW_TAGS.contains(&t.as_str()) != (*c == Color::Yellow))
}

// ---- デバッグ描画 ----

fn draw_rect(img: &mut RgbImage, r: &Rect, color: Rgb<u8>) {
    let (w, h) = img.dimensions();
    let (x0, y0) = (r.x as i64, r.y as i64);
    let (x1, y1) = ((r.x + r.w) as i64, (r.y + r.h) as i64);
    for x in x0..=x1 {
        for y in [y0, y1] {
            if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
    for y in y0..=y1 {
        for x in [x0, x1] {
            if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                img.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}

fn write_debug(dir: &Path, name: &str, small: &RgbImage, det: &Detection) {
    let mut canvas = small.clone();
    for r in &det.candidates {
        draw_rect(&mut canvas, r, Rgb([255, 0, 0]));
    }
    if let Some(g) = &det.grid {
        for r in &g.boxes {
            draw_rect(&mut canvas, r, Rgb([0, 255, 0]));
        }
    }
    let _ = canvas.save(dir.join(name));
}

// ---- 切り抜き ----

fn safe_dir_name(tag: &str) -> String {
    tag.chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect()
}

// ---- 実行 ----

struct Args {
    root: PathBuf,
    limit: Option<usize>,
    only: Option<String>,
    debug_dir: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args { root: PathBuf::from("local/recruit_samples"), limit: None, only: None, debug_dir: None };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| panic!("{k} に値が要る"));
        match k.as_str() {
            "--in" => a.root = PathBuf::from(val()),
            "--limit" => a.limit = Some(val().parse().expect("--limit は数")),
            "--only" => a.only = Some(val()),
            "--debug-dir" => a.debug_dir = Some(PathBuf::from(val())),
            other => panic!("知らない引数: {other}"),
        }
    }
    a
}

struct Outcome {
    report: Report,
    crops: Vec<CropRecord>,
    colors: Vec<Color>,
}

fn process(rec: &Record, idx: usize, args: &Args) -> Outcome {
    let path = args.root.join(&rec.file);
    let mut report = Report {
        file: rec.file.clone(),
        found: false,
        reason: None,
        grid_bbox: None,
        box_size: None,
        width: 0,
        height: 0,
        candidates: 0,
    };
    let img = match image::open(&path) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            report.reason = Some(format!("decode: {e}"));
            return Outcome { report, crops: vec![], colors: vec![] };
        }
    };
    let (iw, ih) = img.dimensions();
    report.width = iw;
    report.height = ih;
    let (det, small) = detect(&img);
    report.candidates = det.candidates.len();
    if args.only.is_some() {
        eprintln!("{}: {:?}", rec.file, det.candidates);
    }

    let mut crops = Vec::new();
    let mut colors = Vec::new();
    match &det.grid {
        None => {
            report.reason = Some(if det.candidates.is_empty() { "候補なし" } else { "グリッドなし" }.into());
        }
        Some(g) => {
            report.found = true;
            let full = det.full_boxes(iw, ih).expect("グリッドがあれば枠も取れる");
            let gx0 = full.iter().map(|b| b[0]).min().unwrap();
            let gy0 = full.iter().map(|b| b[1]).min().unwrap();
            let gx1 = full.iter().map(|b| b[0] + b[2]).max().unwrap();
            let gy1 = full.iter().map(|b| b[1] + b[3]).max().unwrap();
            report.grid_bbox = Some([gx0, gy0, gx1 - gx0, gy1 - gy0]);
            report.box_size = Some([full[0][2], full[0][3]]);
            colors = g.boxes.iter().map(|r| r.color).collect();

            if let Some(l) = rec.label.as_ref().filter(|l| l.usable && l.is_global && l.tags.len() == 5) {
                let suspect = order_suspect(&l.tags, &colors);
                for (pos, b) in full.iter().enumerate() {
                    let Some(crop) = crop_box(&img, b) else { continue };
                    let dir = args.root.join("crops").join(safe_dir_name(&l.tags[pos]));
                    std::fs::create_dir_all(&dir).expect("crops dir");
                    let name = format!("{}_{}_{}.png", rec.message_id, rec.attachment_id, pos);
                    crop.save(dir.join(&name)).expect("crop save");
                    crops.push(CropRecord {
                        tag: l.tags[pos].clone(),
                        file: format!("{}/{}", safe_dir_name(&l.tags[pos]), name),
                        source: rec.file.clone(),
                        pos,
                        bbox: *b,
                        color: g.boxes[pos].color.name(),
                        order_suspect: suspect,
                        source_width: iw,
                        source_height: ih,
                        posted_at: rec.posted_at.clone(),
                    });
                }
            }
        }
    }
    if let Some(dir) = &args.debug_dir {
        // 見つからなかった画像は全部、見つかったものは 50 枚に 1 枚
        if !report.found || idx.is_multiple_of(50) {
            let stem = Path::new(&rec.file).file_stem().and_then(|s| s.to_str()).unwrap_or("x");
            let tag = if report.found { "ok" } else { "ng" };
            write_debug(dir, &format!("{tag}_{stem}.png"), &small, &det);
        }
    }
    Outcome { report, crops, colors }
}

fn main() {
    let args = parse_args();
    let index = std::fs::read_to_string(args.root.join("index.jsonl")).expect("index.jsonl が読めない");
    let mut records: Vec<Record> = index
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("index.jsonl の行が壊れている"))
        .collect();
    if let Some(o) = &args.only {
        records.retain(|r| r.file.contains(o.as_str()));
    }
    if let Some(n) = args.limit {
        records.truncate(n);
    }
    if let Some(d) = &args.debug_dir {
        std::fs::create_dir_all(d).expect("debug dir");
    }
    // 派生物なので毎回作り直す
    let crops_root = args.root.join("crops");
    let _ = std::fs::remove_dir_all(&crops_root);
    std::fs::create_dir_all(&crops_root).expect("crops dir");

    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Outcome>>> = Mutex::new((0..records.len()).map(|_| None).collect());
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= records.len() {
                    break;
                }
                let o = process(&records[i], i, &args);
                results.lock().unwrap()[i] = Some(o);
            });
        }
    });
    let outcomes: Vec<Outcome> = results.into_inner().unwrap().into_iter().map(|o| o.expect("outcome")).collect();

    // ---- 書き出し ----
    let mut rep = std::io::BufWriter::new(std::fs::File::create(args.root.join("detect_report.jsonl")).unwrap());
    let mut idx = std::io::BufWriter::new(std::fs::File::create(crops_root.join("index.jsonl")).unwrap());
    for o in &outcomes {
        writeln!(rep, "{}", serde_json::to_string(&o.report).unwrap()).unwrap();
        for c in &o.crops {
            writeln!(idx, "{}", serde_json::to_string(c).unwrap()).unwrap();
        }
    }

    // ---- 集計 ----
    let total = outcomes.len();
    let found = outcomes.iter().filter(|o| o.report.found).count();
    println!("total {total}, found {found}, not found {}", total - found);
    let target = |r: &Record| r.label.as_ref().is_some_and(|l| l.usable && l.is_global);
    let (mut tt, mut tf) = (0, 0);
    let mut by_res: BTreeMap<(u32, u32), (usize, usize)> = BTreeMap::new();
    let mut color_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut tag_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (r, o) in records.iter().zip(&outcomes) {
        if target(r) {
            tt += 1;
            tf += o.report.found as usize;
            let e = by_res.entry((o.report.width, o.report.height)).or_default();
            e.0 += 1;
            e.1 += o.report.found as usize;
        }
        for c in &o.colors {
            *color_counts.entry(c.name()).or_default() += 1;
        }
        for c in &o.crops {
            *tag_counts.entry(c.tag.as_str()).or_default() += 1;
        }
    }
    println!("usable && global: {tt}, found {tf} ({:.1}%)", 100.0 * tf as f32 / tt.max(1) as f32);
    let mut res: Vec<_> = by_res.into_iter().collect();
    res.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
    println!("by resolution (usable && global, top 10):");
    for ((w, h), (n, f)) in res.iter().take(10) {
        println!("  {w}x{h}: {f}/{n} ({:.1}%)", 100.0 * *f as f32 / *n as f32);
    }
    println!("box colors (all found images): {color_counts:?}");
    let ncrops: usize = tag_counts.values().sum();
    let suspect_imgs = outcomes.iter().filter(|o| o.crops.first().is_some_and(|c| c.order_suspect)).count();
    let labeled_imgs = outcomes.iter().filter(|o| !o.crops.is_empty()).count();
    println!("order_suspect images: {suspect_imgs}/{labeled_imgs}");
    println!("crops {ncrops}, tags {}:", tag_counts.len());
    let mut tc: Vec<_> = tag_counts.into_iter().collect();
    tc.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    for (t, n) in tc {
        println!("  {t}: {n}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_suspect_detects_color_mismatch() {
        let tags: Vec<String> = ["近距離", "上級エリート", "牽制", "エリート", "治療"].iter().map(|s| s.to_string()).collect();
        let ok = [Color::Dark, Color::Yellow, Color::Dark, Color::Yellow, Color::Dark];
        assert!(!order_suspect(&tags, &ok));
        let swapped = [Color::Dark, Color::Yellow, Color::Yellow, Color::Dark, Color::Dark];
        assert!(order_suspect(&tags, &swapped));
    }

    #[test]
    fn dir_name_is_sanitized() {
        assert_eq!(safe_dir_name("COST回復"), "COST回復");
        assert_eq!(safe_dir_name("a/b:c"), "a_b_c");
    }
}
