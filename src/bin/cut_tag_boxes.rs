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

use image::{imageops, GenericImageView, Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// 検出に使う縮小後の高さ。これより小さい画像は拡大しない。
const DETECT_HEIGHT: u32 = 540;
/// 閉じ処理の半径（縮小後 px）。枠の中の白い文字で枠が割れないようにする。
const CLOSE_RADIUS: usize = 2;
/// 候補にする塗りつぶし率の下限（文字ぶんの穴があるので 1 にはならない）。
const MIN_FILL: f32 = 0.6;
/// 候補の縦横比（幅/高さ）の範囲。タグ枠は約 3.1。
const MIN_ASPECT: f32 = 2.2;
const MAX_ASPECT: f32 = 4.6;
/// 切り抜きで枠の縁を落とす量（元画像の枠の高さに対する比）。
const CROP_INSET_RATIO: f32 = 0.04;

// ---- 入力・出力の形 ----

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

// ---- 色の分類 ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Dark,
    Blue,
    Yellow,
}

impl Color {
    fn name(self) -> &'static str {
        match self {
            Color::Dark => "dark",
            Color::Blue => "blue",
            Color::Yellow => "yellow",
        }
    }
}

/// (色相 0..360, 彩度 0..1, 明度 0..1)
fn hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

/// 枠の色（暗灰・青・黄）なら Some。背景の明るい灰色やパネルは None。
fn classify(r: u8, g: u8, b: u8) -> Option<Color> {
    let (h, s, v) = hsv(r, g, b);
    if v <= 0.40 && s <= 0.45 {
        Some(Color::Dark)
    } else if (185.0..=220.0).contains(&h) && s >= 0.6 && v >= 0.55 {
        Some(Color::Blue)
    } else if (38.0..=65.0).contains(&h) && s >= 0.6 && v >= 0.6 {
        Some(Color::Yellow)
    } else {
        None
    }
}

// ---- 画像処理（縮小後の bool マスク） ----

struct Mask {
    w: usize,
    h: usize,
    v: Vec<bool>,
}

/// 正方形窓の膨張。積分画像で O(w*h)。範囲外は false 扱い。
fn dilate(m: &Mask, r: usize) -> Mask {
    let (w, h) = (m.w, m.h);
    let mut integ = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0;
        for x in 0..w {
            row += m.v[y * w + x] as u32;
            integ[(y + 1) * (w + 1) + x + 1] = integ[y * (w + 1) + x + 1] + row;
        }
    }
    let mut out = vec![false; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let s = integ[y1 * (w + 1) + x1] + integ[y0 * (w + 1) + x0]
                - integ[y0 * (w + 1) + x1]
                - integ[y1 * (w + 1) + x0];
            out[y * w + x] = s > 0;
        }
    }
    Mask { w, h, v: out }
}

/// 閉じ（膨張→収縮）。収縮は反転マスクの膨張で作る。
fn close(m: &Mask, r: usize) -> Mask {
    let d = dilate(m, r);
    let inv = Mask { w: d.w, h: d.h, v: d.v.iter().map(|b| !b).collect() };
    let e = dilate(&inv, r);
    Mask { w: e.w, h: e.h, v: e.v.iter().map(|b| !b).collect() }
}

/// 外側につながっていない穴（枠の中の文字など）を埋める。画像の大きさに依らず効く。
fn fill_holes(m: &Mask) -> Mask {
    let (w, h) = (m.w, m.h);
    let mut outside = vec![false; w * h];
    let mut stack = Vec::new();
    let mut seed = |x: usize, y: usize, stack: &mut Vec<(usize, usize)>| {
        let i = y * w + x;
        if !m.v[i] && !outside[i] {
            outside[i] = true;
            stack.push((x, y));
        }
    };
    for x in 0..w {
        seed(x, 0, &mut stack);
        seed(x, h - 1, &mut stack);
    }
    for y in 0..h {
        seed(0, y, &mut stack);
        seed(w - 1, y, &mut stack);
    }
    while let Some((x, y)) = stack.pop() {
        if x > 0 {
            seed(x - 1, y, &mut stack);
        }
        if x + 1 < w {
            seed(x + 1, y, &mut stack);
        }
        if y > 0 {
            seed(x, y - 1, &mut stack);
        }
        if y + 1 < h {
            seed(x, y + 1, &mut stack);
        }
    }
    Mask { w, h, v: outside.iter().map(|o| !o).collect() }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
}

/// 4 近傍の連結成分 → 候補矩形。`colors` は閉じる前の色の分類（枠の色の判定用）。
fn components(closed: &Mask, colors: &[Option<Color>]) -> Vec<Rect> {
    let (w, h) = (closed.w, closed.h);
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for sy in 0..h {
        for sx in 0..w {
            let si = sy * w + sx;
            if !closed.v[si] || seen[si] {
                continue;
            }
            seen[si] = true;
            stack.push((sx, sy));
            let (mut x0, mut y0, mut x1, mut y1) = (sx, sy, sx, sy);
            let mut area = 0usize;
            while let Some((x, y)) = stack.pop() {
                area += 1;
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
                let mut push = |nx: usize, ny: usize| {
                    let ni = ny * w + nx;
                    if closed.v[ni] && !seen[ni] {
                        seen[ni] = true;
                        stack.push((nx, ny));
                    }
                };
                if x > 0 {
                    push(x - 1, y);
                }
                if x + 1 < w {
                    push(x + 1, y);
                }
                if y > 0 {
                    push(x, y - 1);
                }
                if y + 1 < h {
                    push(x, y + 1);
                }
            }
            let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
            if area == 0 || bw < 3 || bh < 3 {
                continue;
            }
            let fill = area as f32 / (bw * bh) as f32;
            if fill < MIN_FILL {
                continue;
            }
            // 色: 枠内で青・黄が目立てばそれ、無ければ暗灰
            let (mut nb, mut ny_) = (0usize, 0usize);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    match colors[y * w + x] {
                        Some(Color::Blue) => nb += 1,
                        Some(Color::Yellow) => ny_ += 1,
                        _ => {}
                    }
                }
            }
            let a = (bw * bh) as f32;
            let color = if nb as f32 / a > 0.3 {
                Color::Blue
            } else if ny_ as f32 / a > 0.015 {
                Color::Yellow
            } else {
                Color::Dark
            };
            out.push(Rect { x: x0 as f32, y: y0 as f32, w: bw as f32, h: bh as f32, color });
        }
    }
    out
}

/// 黄色く光る枠になるタグ。ラベルの順が画面の位置と合っているかの検算に使う。
const YELLOW_TAGS: [&str; 2] = ["上級エリート", "エリート"];

/// タグと枠の色の食い違い（エリート系なのに黄色でない／そうでないのに黄色）が 1 つでもあるか。
fn order_suspect(tags: &[String], colors: &[Color]) -> bool {
    tags.iter().zip(colors).any(|(t, c)| YELLOW_TAGS.contains(&t.as_str()) != (*c == Color::Yellow))
}

/// サイズ・縦横比が枠として妥当な候補だけ残す。
fn filter_candidates(rects: Vec<Rect>, img_w: f32, img_h: f32) -> Vec<Rect> {
    rects
        .into_iter()
        .filter(|r| {
            let aspect = r.w / r.h;
            (MIN_ASPECT..=MAX_ASPECT).contains(&aspect)
                && r.w >= img_w * 0.04
                && r.w <= img_w * 0.32
                && r.h >= img_h * 0.025
                && r.h <= img_h * 0.5
        })
        .collect()
}

// ---- グリッド探し ----

#[derive(Debug, Clone, PartialEq)]
struct Grid {
    /// 行優先: [r1c1, r1c2, r1c3, r2c1, r2c2]
    boxes: [Rect; 5],
    /// 小さいほど良い
    score: f32,
}

fn similar_size(a: &Rect, b: &Rect) -> bool {
    (a.w - b.w).abs() <= 0.12 * a.w.max(b.w) && (a.h - b.h).abs() <= 0.2 * a.h.max(b.h)
}

/// 3 列 x 2 行（2 行目は 2 個）の等間隔グリッドを探す。無ければ None。
fn find_grid(c: &[Rect]) -> Option<Grid> {
    let mut best: Option<Grid> = None;
    for (ia, a) in c.iter().enumerate() {
        for (ib, b) in c.iter().enumerate() {
            if ia == ib || b.x <= a.x || !similar_size(a, b) {
                continue;
            }
            let (w, h) = ((a.w + b.w) / 2.0, (a.h + b.h) / 2.0);
            let pitch = b.x - a.x;
            let dy = (b.y - a.y).abs();
            if !(1.05 * w..=2.0 * w).contains(&pitch) || dy > 0.2 * h {
                continue;
            }
            // 1 行目の 3 つ目: x = b.x + pitch 付近
            let Some((c3, d3)) = pick(c, |r| similar_size(a, r), |r| (r.x - (b.x + pitch)).abs() / w + (r.y - a.y).abs() / h, |r| {
                (r.x - (b.x + pitch)).abs() <= 0.2 * w && (r.y - a.y).abs() <= 0.2 * h
            }) else {
                continue;
            };
            // 2 行目: a・b の真下
            let below = |r: &Rect, col: &Rect| {
                (r.x - col.x).abs() <= 0.2 * w && r.y >= a.y + 1.15 * h && r.y <= a.y + 2.5 * h
            };
            let Some((d, dd)) = pick(c, |r| similar_size(a, r), |r| (r.x - a.x).abs() / w, |r| below(r, a)) else {
                continue;
            };
            let Some((e, de)) = pick(c, |r| similar_size(a, r), |r| (r.x - b.x).abs() / w + (r.y - d.y).abs() / h, |r| {
                below(r, b) && (r.y - d.y).abs() <= 0.2 * h
            }) else {
                continue;
            };
            let score = dy / h + d3 + dd + de + (c3.w - w).abs() / w + (d.w - w).abs() / w + (e.w - w).abs() / w;
            if best.as_ref().is_none_or(|g| score < g.score) {
                best = Some(Grid { boxes: [*a, *b, *c3, *d, *e], score });
            }
        }
    }
    best
}

/// 条件を満たす候補のうち、コストが最小のもの。
fn pick(
    c: &[Rect],
    size_ok: impl Fn(&Rect) -> bool,
    cost: impl Fn(&Rect) -> f32,
    cond: impl Fn(&Rect) -> bool,
) -> Option<(&Rect, f32)> {
    c.iter()
        .filter(|r| size_ok(r) && cond(r))
        .map(|r| (r, cost(r)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// 枠は全部同じ大きさのはずで、影は枠より大きくしか出ないので、5 つの最小の幅・高さに揃える。
/// 影は下と左右に出るので、上辺は動かさず、左右は中心を保つ。
fn normalize_grid(g: &mut Grid) {
    let w = g.boxes.iter().map(|b| b.w).fold(f32::MAX, f32::min);
    let h = g.boxes.iter().map(|b| b.h).fold(f32::MAX, f32::min);
    for b in &mut g.boxes {
        b.x += (b.w - w) / 2.0;
        b.w = w;
        b.h = h;
    }
}

// ---- 1 枚の検出 ----

struct Detection {
    grid: Option<Grid>,
    candidates: Vec<Rect>,
    /// 縮小後のサイズ
    small: (u32, u32),
}

fn detect(img: &RgbImage) -> (Detection, RgbImage) {
    let (w, h) = img.dimensions();
    let small = if h > DETECT_HEIGHT {
        let nw = ((w as f32) * DETECT_HEIGHT as f32 / h as f32).round().max(1.0) as u32;
        imageops::resize(img, nw, DETECT_HEIGHT, imageops::FilterType::Triangle)
    } else {
        img.clone()
    };
    let (sw, sh) = small.dimensions();
    let colors: Vec<Option<Color>> = small.pixels().map(|p| classify(p[0], p[1], p[2])).collect();
    let mask = Mask { w: sw as usize, h: sh as usize, v: colors.iter().map(|c| c.is_some()).collect() };
    let closed = fill_holes(&close(&mask, CLOSE_RADIUS));
    let cands = filter_candidates(components(&closed, &colors), sw as f32, sh as f32);
    let mut grid = find_grid(&cands);
    if let Some(g) = grid.as_mut() {
        normalize_grid(g);
    }
    (Detection { grid, candidates: cands, small: (sw, sh) }, small)
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
            let (sx, sy) = (iw as f32 / det.small.0 as f32, ih as f32 / det.small.1 as f32);
            let full: Vec<[u32; 4]> = g
                .boxes
                .iter()
                .map(|r| {
                    let x0 = (r.x * sx).round().max(0.0) as u32;
                    let y0 = (r.y * sy).round().max(0.0) as u32;
                    let x1 = (((r.x + r.w) * sx).round() as u32).min(iw);
                    let y1 = (((r.y + r.h) * sy).round() as u32).min(ih);
                    [x0, y0, x1 - x0, y1 - y0]
                })
                .collect();
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
                    let inset = ((b[3] as f32 * CROP_INSET_RATIO).round() as u32).max(2);
                    if b[2] <= inset * 2 + 2 || b[3] <= inset * 2 + 2 {
                        continue;
                    }
                    let crop = img.view(b[0] + inset, b[1] + inset, b[2] - inset * 2, b[3] - inset * 2).to_image();
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

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h, color: Color::Dark }
    }

    #[test]
    fn classify_colors() {
        assert_eq!(classify(0x30, 0x30, 0x30), Some(Color::Dark));
        assert_eq!(classify(0x0a, 0x8f, 0xd8), Some(Color::Blue));
        assert_eq!(classify(0xff, 0xd8, 0x00), Some(Color::Yellow));
        assert_eq!(classify(0xe8, 0xe8, 0xe8), None); // パネルの明るい灰色
        assert_eq!(classify(0xb0, 0xb0, 0xb0), None); // 6 個目の空き枠
        assert_eq!(classify(0xc0, 0x20, 0x20), None); // 赤
    }

    fn grid_rects() -> Vec<Rect> {
        vec![
            r(100.0, 100.0, 60.0, 19.0),
            r(130.0 + 45.0, 100.0, 60.0, 19.0),
            r(100.0 + 70.0 * 2.0, 100.0, 60.0, 19.0),
            r(100.0, 130.0, 60.0, 19.0),
            r(170.0, 130.0, 60.0, 19.0),
        ]
    }

    #[test]
    fn grid_found_in_order() {
        let mut c = grid_rects();
        // 時間の ^ v ボタン（2 列）と ✓✕ ボタンを混ぜる
        c.push(r(20.0, 10.0, 60.0, 19.0));
        c.push(r(90.0, 10.0, 60.0, 19.0));
        c.push(r(20.0, 60.0, 60.0, 19.0));
        c.push(r(90.0, 60.0, 60.0, 19.0));
        c.push(r(300.0, 200.0, 80.0, 24.0));
        let g = find_grid(&c).expect("grid");
        let xs: Vec<(f32, f32)> = g.boxes.iter().map(|b| (b.x, b.y)).collect();
        assert_eq!(xs, vec![(100.0, 100.0), (175.0, 100.0), (240.0, 100.0), (100.0, 130.0), (170.0, 130.0)]);
    }

    #[test]
    fn grid_missing_box_is_none() {
        let mut c = grid_rects();
        c.remove(4);
        assert!(find_grid(&c).is_none());
        // 2 行目がずれている
        let mut c = grid_rects();
        c[4].x += 30.0;
        assert!(find_grid(&c).is_none());
        // サイズが違う
        let mut c = grid_rects();
        c[2].w = 30.0;
        assert!(find_grid(&c).is_none());
    }

    fn fill(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, c: [u8; 3]) {
        for yy in y..y + h {
            for xx in x..x + w {
                img.put_pixel(xx, yy, Rgb(c));
            }
        }
    }

    #[test]
    fn synthetic_image_finds_five_boxes() {
        let mut img = RgbImage::from_pixel(400, 300, Rgb([235, 235, 235]));
        let dark = [0x30, 0x30, 0x30];
        // タグ枠: 80x26、3 列（ピッチ 100）x 2 行。1 つ目の枠には白い文字風の縞
        for (i, (x, y)) in [(60, 150), (160, 150), (260, 150), (60, 200), (160, 200)].iter().enumerate() {
            let c = if i == 1 { [0x0a, 0x8f, 0xd8] } else { dark };
            fill(&mut img, *x, *y, 80, 26, c);
            for k in 0..5 {
                fill(&mut img, x + 12 + k * 12, y + 6, 6, 14, [255, 255, 255]);
            }
        }
        fill(&mut img, 260, 200, 80, 26, [0xb0, 0xb0, 0xb0]); // 6 個目の空き枠
        // 時間ボタン（2 列）と ✓✕
        for (x, y) in [(60, 20), (160, 20), (60, 80), (160, 80)] {
            fill(&mut img, x, y, 80, 26, dark);
        }
        fill(&mut img, 300, 20, 70, 30, [0x0a, 0x8f, 0xd8]);
        fill(&mut img, 300, 60, 70, 30, dark);

        let (det, _) = detect(&img);
        let g = det.grid.expect("grid");
        let pos: Vec<(i32, i32)> = g.boxes.iter().map(|b| (b.x.round() as i32, b.y.round() as i32)).collect();
        let expect = [(60, 150), (160, 150), (260, 150), (60, 200), (160, 200)];
        for (p, e) in pos.iter().zip(expect) {
            assert!((p.0 - e.0).abs() <= 2 && (p.1 - e.1).abs() <= 2, "{pos:?}");
        }
        assert_eq!(g.boxes[1].color, Color::Blue);
        assert_eq!(g.boxes[0].color, Color::Dark);
    }

    #[test]
    fn normalize_uses_smallest_size() {
        let mut c = grid_rects();
        c[3].h = 22.0;
        c[2].w = 64.0;
        let mut g = find_grid(&c).expect("grid");
        normalize_grid(&mut g);
        assert!(g.boxes.iter().all(|b| b.w == 60.0 && b.h == 19.0));
        assert_eq!(g.boxes[2].x, 240.0 + 2.0);
    }

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
