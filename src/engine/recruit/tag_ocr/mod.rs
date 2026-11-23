//! ローカルのタグ枠 OCR（公開求人のスクショの一次判定）。
//!
//! 画面と軸が揃った日本語のスクショなら、タグ枠 5 個を検出し、テンプレート照合で 5 個全部のタグを
//! 読む（ほぼ誤読なし）。読み切れなければ [`TagOcrResult::Unsure`] を返し、呼び出し側が Google Vision に回す。
//! 英語・大陸版・モニターを撮った写真は枠が見つからない／自信が出ないので自然に Unsure になる。
//!
//! bot にも api にも依存しない。テンプレートは `data/recruitment/tag_templates/`（形式は `store.rs`、
//! 作り方は `src/engine/recruit/CLAUDE.md`）。

pub mod detect;
pub mod feature;
pub mod score;
pub mod store;

use crate::engine::recruit::Error;
use image::DynamicImage;
use std::collections::HashSet;
use std::path::Path;

/// 本番のテンプレートの置き場（カレントディレクトリ基準）。
pub const TEMPLATE_DIR: &str = "data/recruitment/tag_templates";

/// 1 枠の採点結果（ログ・比較レポート用）。
#[derive(Debug, Clone, PartialEq)]
pub struct BoxScore {
    pub best_tag: String,
    pub best: f32,
    pub second_tag: String,
    pub second: f32,
    /// 1 位と 2 位の差（紛らわしい組は違う画素だけでの差）
    pub margin: f32,
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsureReason {
    /// 枠 5 個のグリッドが見つからない
    GridNotFound,
    /// 5 枠のうち自信が足りないものがある
    LowScore,
    /// 5 枠とも受理したが同じタグが重複した（必ずどれかが誤り）
    Duplicate,
}

impl UnsureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            UnsureReason::GridNotFound => "grid_not_found",
            UnsureReason::LowScore => "low_score",
            UnsureReason::Duplicate => "duplicate",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TagOcrResult {
    /// 5 枠とも受理し、全部違うタグ。`tags` は画面の枠の順（行優先）で、tagList.json の正式名。
    Read { tags: Vec<String>, boxes: Vec<BoxScore> },
    Unsure { reason: UnsureReason, boxes: Vec<BoxScore> },
}

pub struct TagOcr {
    names: Vec<String>,
    model: score::Model<'static>,
    t: f32,
    m: f32,
    center: bool,
}

impl TagOcr {
    /// テンプレートのディレクトリから読む。
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, Error> {
        let l = store::load(dir.as_ref())?;
        Ok(Self { names: l.names, model: l.model, t: l.t, m: l.m, center: l.center })
    }

    /// 受理のしきい値 (T, M)。
    pub fn thresholds(&self) -> (f32, f32) {
        (self.t, self.m)
    }

    /// 枠 5 個の採点。グリッドが見つからなければ None。
    pub fn analyze(&self, img: &DynamicImage) -> Option<Vec<BoxScore>> {
        let rgb = img.to_rgb8();
        let (iw, ih) = rgb.dimensions();
        let (det, _) = detect::detect(&rgb);
        let full = det.full_boxes(iw, ih)?;
        let mut out = Vec::with_capacity(5);
        for b in &full {
            let crop = detect::crop_box(&rgb, b)?;
            let raw = feature::preprocess(&crop, self.center);
            let norm = feature::normalize(&raw);
            let r = score::score_box(&raw, &norm, &self.model);
            let s = r.refined;
            out.push(BoxScore {
                best_tag: self.names[s.best_tag].clone(),
                best: s.best,
                second_tag: self.names[s.second_tag].clone(),
                second: s.second,
                margin: s.margin,
                accepted: score::accepts(&s, self.t, self.m),
            });
        }
        Some(out)
    }

    pub fn read(&self, img: &DynamicImage) -> TagOcrResult {
        let Some(boxes) = self.analyze(img) else {
            return TagOcrResult::Unsure { reason: UnsureReason::GridNotFound, boxes: Vec::new() };
        };
        if boxes.iter().any(|b| !b.accepted) {
            return TagOcrResult::Unsure { reason: UnsureReason::LowScore, boxes };
        }
        let tags: Vec<String> = boxes.iter().map(|b| b.best_tag.clone()).collect();
        let distinct: HashSet<&String> = tags.iter().collect();
        if distinct.len() != tags.len() {
            return TagOcrResult::Unsure { reason: UnsureReason::Duplicate, boxes };
        }
        TagOcrResult::Read { tags, boxes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    const DARK: [u8; 3] = [0x30, 0x30, 0x30];

    fn ocr() -> TagOcr {
        TagOcr::load(TEMPLATE_DIR).expect("tag_templates should load")
    }

    /// テンプレートの 1 つ目の代表を白文字（暗灰の枠の上）として描いた枠の画像。`tag` が None なら文字なし。
    fn box_image(tag: Option<&str>) -> RgbImage {
        let mut img = RgbImage::from_pixel(192, 64, Rgb(DARK));
        if let Some(tag) = tag {
            let t = image::open(format!("{TEMPLATE_DIR}/{tag}_1.png")).expect("template png").to_luma8();
            for (x, y, p) in t.enumerate_pixels() {
                let v = p[0] as f32 / 255.0;
                let c = DARK[0] as f32 + (255.0 - DARK[0] as f32) * v;
                img.put_pixel(x, y, Rgb([c as u8; 3]));
            }
        }
        img
    }

    /// 明るいパネルに 3 列 x 2 行（2 行目は 2 個）のタグ枠を置いた画面。
    fn screen(tags: [Option<&str>; 5]) -> DynamicImage {
        let mut img = RgbImage::from_pixel(760, 420, Rgb([235, 235, 235]));
        let pos = [(40, 200), (260, 200), (480, 200), (40, 290), (260, 290)];
        for (t, (x, y)) in tags.iter().zip(pos) {
            image::imageops::replace(&mut img, &box_image(*t), x, y);
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn reads_five_tags_from_a_synthetic_screen() {
        // 近距離/遠距離は紛らわしい組（違う画素での再判定が要る）
        let tags = ["近距離", "遠距離", "COST回復", "治療", "火力"];
        let r = ocr().read(&screen(tags.map(Some)));
        match r {
            TagOcrResult::Read { tags: got, boxes } => {
                assert_eq!(got, tags);
                assert!(boxes.iter().all(|b| b.accepted));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn no_grid_is_unsure() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(760, 420, Rgb([235, 235, 235])));
        assert_eq!(ocr().read(&img), TagOcrResult::Unsure { reason: UnsureReason::GridNotFound, boxes: vec![] });
    }

    #[test]
    fn blank_boxes_are_low_score() {
        match ocr().read(&screen([None; 5])) {
            TagOcrResult::Unsure { reason, boxes } => {
                assert_eq!(reason, UnsureReason::LowScore);
                assert_eq!(boxes.len(), 5);
                assert!(boxes.iter().all(|b| !b.accepted));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn duplicate_tag_is_unsure() {
        let tags = [Some("近距離"), Some("近距離"), Some("COST回復"), Some("治療"), Some("火力")];
        match ocr().read(&screen(tags)) {
            TagOcrResult::Unsure { reason, .. } => assert_eq!(reason, UnsureReason::Duplicate),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn template_tag_names_are_canonical() {
        // meta.json のタグ名は tagList.json の正式名であること（calc 側で未知のタグ扱いにならない）
        let list: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("data/recruitment/tagList.json").unwrap()).unwrap();
        let known: HashSet<String> = list.as_object().unwrap().values().flat_map(|v| v.as_array().unwrap()).map(|t| t.as_str().unwrap().to_string()).collect();
        for n in &ocr().names {
            assert!(known.contains(n), "{n}");
        }
    }

    #[test]
    fn every_tag_has_a_template() {
        // 逆向き: tagList.json の全タグにテンプレートがあること（ゲームにタグが増えたら読めなくなるので気づく）
        let list: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("data/recruitment/tagList.json").unwrap()).unwrap();
        let have: HashSet<String> = ocr().names.iter().cloned().collect();
        for t in list.as_object().unwrap().values().flat_map(|v| v.as_array().unwrap()) {
            let t = t.as_str().unwrap();
            assert!(have.contains(t), "タグ「{t}」のテンプレートがありません。fit_tag_templates でテンプレートを作り直してください");
        }
    }

    /// meta.json に持つ紛らわしい組が、PNG から計算し直したものと一致する（重いので release で: `cargo test --release -- --ignored`）。
    #[test]
    #[ignore]
    fn confusable_pairs_match_recomputation() {
        let o = ocr();
        let fresh = score::Model::new(&o.model.templates);
        let mut a = fresh.confusable_pairs().into_iter().map(|(x, y, _)| (x, y)).collect::<Vec<_>>();
        let mut b = o.model.confusable_pairs().into_iter().map(|(x, y, _)| (x, y)).collect::<Vec<_>>();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    /// 手元のスクショでの 1 枚あたりの時間（`cargo test --release -- --ignored --nocapture read_timing`）。
    #[test]
    #[ignore]
    fn read_timing() {
        let o = ocr();
        let dir = "local/recruit_samples/images";
        let files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).take(100).collect();
        let imgs: Vec<DynamicImage> = files.iter().filter_map(|p| image::open(p).ok()).collect();
        let t0 = std::time::Instant::now();
        let reads = imgs.iter().filter(|i| matches!(o.read(i), TagOcrResult::Read { .. })).count();
        let el = t0.elapsed();
        println!("read_timing: {} images, {} read, {:.1} ms/image", imgs.len(), reads, el.as_secs_f64() * 1000.0 / imgs.len() as f64);
    }
}
