//! テンプレートの保存形式（`data/recruitment/tag_templates/`）。
//!
//! - 代表（prototype）1 つにつきグレースケール PNG 1 枚（FW x FH、画素値 = 特徴値 0..1 を 0..255 に量子化）。
//! - `meta.json` に、しきい値 T・M、特徴の大きさ・前処理と再判定の定数（読み込み時にコードの定数と照合する）、
//!   タグごとの PNG の一覧、紛らわしいタグの組（全組の相関を起動時に計算すると重いので書き出し時に済ませる）。
//! - 書き出しは手元ツール（fit_tag_templates --export）だけが使う。実行時（bot）は読むだけ。

use super::feature::{ABS_MIN_DIFF, FH, FW, MIN_AREA, NPIX, P99_RATIO, SHIFT_X, SHIFT_Y};
use super::score::{Model, Template, DIFF_DILATE, DIFF_THR, PAIR_SIM};
use crate::engine::recruit::Error;
use image::{GrayImage, Luma};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 形式を変えたら上げる。
pub const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Prototype {
    file: String,
    /// 学習に使ったサンプル数（参考）
    count: usize,
}

#[derive(Serialize, Deserialize)]
struct TagEntry {
    name: String,
    prototypes: Vec<Prototype>,
}

#[derive(Serialize, Deserialize)]
struct Meta {
    format_version: u32,
    feature_width: usize,
    feature_height: usize,
    center: bool,
    threshold_t: f32,
    threshold_m: f32,
    shift_x: i32,
    shift_y: i32,
    min_area: usize,
    abs_min_diff: f32,
    p99_ratio: f32,
    pair_sim: f32,
    diff_threshold: f32,
    diff_dilate: i32,
    note: String,
    tags: Vec<TagEntry>,
    /// 相関が pair_sim 以上のタグの組: [名前, 名前, 相関]
    confusable_pairs: Vec<(String, String, f32)>,
}

/// 読み込んだテンプレート一式。
pub struct Loaded {
    /// テンプレートの添字 → タグ名（tagList.json の正式名）
    pub names: Vec<String>,
    pub model: Model<'static>,
    pub t: f32,
    pub m: f32,
    pub center: bool,
}

fn file_stem(tag: &str) -> String {
    tag.chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn quantize(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-6
}

/// 量子化した特徴（PNG の画素）を特徴値に戻す。
fn dequantize(px: &[u8]) -> Vec<f32> {
    px.iter().map(|&p| p as f32 / 255.0).collect()
}

/// `tags`（全タグ。テンプレートの添字と同じ並び）の `templates` を `dir` に書き出す。
/// 量子化したあとの特徴で紛らわしい組を計算して meta.json に書く（読み込んだ結果と一致させるため）。
/// `dir` の既存の PNG と meta.json は消す（他のファイルには触らない）。
pub fn write(
    dir: &Path,
    tags: &[String],
    templates: &[Vec<Template>],
    t: f32,
    m: f32,
    center: bool,
    note: &str,
) -> Result<(), Error> {
    std::fs::create_dir_all(dir)?;
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "png")
            || p.file_name().is_some_and(|n| n == "meta.json")
        {
            std::fs::remove_file(p)?;
        }
    }
    let mut entries = Vec::new();
    let mut quantized: Vec<Vec<Template>> = Vec::new();
    for (name, protos) in tags.iter().zip(templates) {
        if protos.is_empty() {
            continue;
        }
        let mut files = Vec::new();
        let mut qs = Vec::new();
        for (k, tp) in protos.iter().enumerate() {
            let px: Vec<u8> = tp.raw.iter().map(|&v| quantize(v)).collect();
            let img = GrayImage::from_fn(FW as u32, FH as u32, |x, y| {
                Luma([px[y as usize * FW + x as usize]])
            });
            let file = format!("{}_{}.png", file_stem(name), k + 1);
            img.save(dir.join(&file))?;
            files.push(Prototype {
                file,
                count: tp.count,
            });
            qs.push(Template::new(dequantize(&px), tp.count));
        }
        entries.push(TagEntry {
            name: name.clone(),
            prototypes: files,
        });
        quantized.push(qs);
    }
    let model = Model::new(&quantized);
    let confusable_pairs = model
        .confusable_pairs()
        .into_iter()
        .map(|(a, b, v)| {
            (
                entries[a].name.clone(),
                entries[b].name.clone(),
                (v * 1000.0).round() / 1000.0,
            )
        })
        .collect();
    let meta = Meta {
        format_version: FORMAT_VERSION,
        feature_width: FW,
        feature_height: FH,
        center,
        threshold_t: t,
        threshold_m: m,
        shift_x: SHIFT_X,
        shift_y: SHIFT_Y,
        min_area: MIN_AREA,
        abs_min_diff: ABS_MIN_DIFF,
        p99_ratio: P99_RATIO,
        pair_sim: PAIR_SIM,
        diff_threshold: DIFF_THR,
        diff_dilate: DIFF_DILATE,
        note: note.to_string(),
        tags: entries,
        confusable_pairs,
    };
    std::fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;
    Ok(())
}

/// `dir` からテンプレートを読む。形式・定数がコードと合わない、PNG が欠けている等はエラー。
pub fn load(dir: &Path) -> Result<Loaded, Error> {
    let meta: Meta = serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json"))?)?;
    let mismatch = meta.format_version != FORMAT_VERSION
        || meta.feature_width != FW
        || meta.feature_height != FH
        || meta.shift_x != SHIFT_X
        || meta.shift_y != SHIFT_Y
        || meta.min_area != MIN_AREA
        || !near(meta.abs_min_diff, ABS_MIN_DIFF)
        || !near(meta.p99_ratio, P99_RATIO)
        || !near(meta.pair_sim, PAIR_SIM)
        || !near(meta.diff_threshold, DIFF_THR)
        || meta.diff_dilate != DIFF_DILATE;
    if mismatch {
        return Err(
            "tag_templates/meta.json の形式・定数がコードと合わない（テンプレートを書き出し直す）"
                .into(),
        );
    }
    let mut names = Vec::new();
    let mut templates = Vec::new();
    for tag in &meta.tags {
        if tag.prototypes.is_empty() {
            return Err(format!("タグ {} の代表が無い", tag.name).into());
        }
        let mut protos = Vec::new();
        for p in &tag.prototypes {
            if p.file.contains(['/', '\\']) || p.file.contains("..") {
                return Err(format!("不正なファイル名: {}", p.file).into());
            }
            let img = image::open(dir.join(&p.file))?.to_luma8();
            if img.dimensions() != (FW as u32, FH as u32) {
                return Err(format!("{} の大きさが {FW}x{FH} でない", p.file).into());
            }
            debug_assert_eq!(img.as_raw().len(), NPIX);
            protos.push(Template::new(dequantize(img.as_raw()), p.count));
        }
        names.push(tag.name.clone());
        templates.push(protos);
    }
    let index = |n: &str| {
        names
            .iter()
            .position(|x| x == n)
            .ok_or_else(|| format!("confusable_pairs に未知のタグ: {n}"))
    };
    let mut pairs = Vec::new();
    for (a, b, v) in &meta.confusable_pairs {
        pairs.push((index(a)?, index(b)?, *v));
    }
    Ok(Loaded {
        names,
        model: Model::with_confusable(templates, &pairs),
        t: meta.threshold_t,
        m: meta.threshold_m,
        center: meta.center,
    })
}
