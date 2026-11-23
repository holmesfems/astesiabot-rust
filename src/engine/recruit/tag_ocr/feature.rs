//! 枠の切り抜き → 文字だけの特徴（FW x FH、0..1）と、その類似度（位置ずれを許す正規化相互相関）。
//!
//! - 特徴は「文字だけの 2 値画像」をぼかしたもの。背景色（枠の色）との差で文字を拾うので、
//!   白文字・黄文字・黄地の黒文字を同じ形で扱える。
//! - 学習（fit_tag_templates）と本番（TagOcr）で同じ関数を使う。定数を変えたらテンプレートを作り直す
//!   （meta.json に値を書いてあり、食い違うと読み込みが失敗する）。

use image::{imageops, RgbImage};

/// 特徴の大きさ。枠の縦横比（約 3）に合わせる。
pub const FW: usize = 192;
pub const FH: usize = 64;
pub const NPIX: usize = FW * FH;
/// 類似度で許す位置ずれ（px）。
pub const SHIFT_X: i32 = 4;
pub const SHIFT_Y: i32 = 2;
/// これより小さい連結成分はゴミとして捨てる（縮小後の px 数）。
pub const MIN_AREA: usize = 20;
/// この面積未満で、周り SPECK_DIST px に他の成分が無い点はほこりとして捨てる。
const SPECK_AREA: usize = 80;
const SPECK_DIST: i32 = 8;
/// 文字とみなす差の下限（0..255）。文字の無い枠をノイズで 2 値化しないため。
pub const ABS_MIN_DIFF: f32 = 40.0;
/// 半階調の点（黄色の網点）を文字に入れないよう、しきい値を p99 のこの割合以上にする。
pub const P99_RATIO: f32 = 0.5;

fn median_u8(v: &mut [u8]) -> u8 {
    v.sort_unstable();
    v[v.len() / 2]
}

/// 大津の方法（256 ビン、値域 0..=255）。
fn otsu(values: &[f32]) -> f32 {
    let mut hist = [0f64; 256];
    for &v in values {
        hist[(v.clamp(0.0, 255.0)) as usize] += 1.0;
    }
    let total: f64 = hist.iter().sum();
    let sum_all: f64 = hist.iter().enumerate().map(|(i, h)| i as f64 * h).sum();
    let (mut w0, mut sum0, mut best, mut best_t) = (0f64, 0f64, -1f64, 0usize);
    for (t, &h) in hist.iter().enumerate() {
        w0 += h;
        if w0 == 0.0 {
            continue;
        }
        let w1 = total - w0;
        if w1 == 0.0 {
            break;
        }
        sum0 += t as f64 * h;
        let (m0, m1) = (sum0 / w0, (sum_all - sum0) / w1);
        let between = w0 * w1 * (m0 - m1) * (m0 - m1);
        if between > best {
            best = between;
            best_t = t;
        }
    }
    best_t as f32 + 0.5
}

/// 文字とみなす差のしきい値。大津の値を、網点を弾くために p99 の半分以上に引き上げる。
fn text_threshold(diff: &[f32]) -> f32 {
    let mut s = diff.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let p99 = s[((s.len() - 1) as f32 * 0.99) as usize];
    otsu(diff).max(P99_RATIO * p99).max(ABS_MIN_DIFF)
}

struct Comp {
    cells: Vec<usize>,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

/// 8 近傍の連結成分に分ける。
fn components(mask: &[bool]) -> Vec<Comp> {
    let mut seen = vec![false; NPIX];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    for s in 0..NPIX {
        if !mask[s] || seen[s] {
            continue;
        }
        seen[s] = true;
        stack.push(s);
        let mut c = Comp {
            cells: Vec::new(),
            x0: FW,
            y0: FH,
            x1: 0,
            y1: 0,
        };
        while let Some(i) = stack.pop() {
            c.cells.push(i);
            let (x, y) = (i % FW, i / FW);
            c.x0 = c.x0.min(x);
            c.x1 = c.x1.max(x);
            c.y0 = c.y0.min(y);
            c.y1 = c.y1.max(y);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= FW as i32 || ny >= FH as i32 {
                        continue;
                    }
                    let ni = ny as usize * FW + nx as usize;
                    if mask[ni] && !seen[ni] {
                        seen[ni] = true;
                        stack.push(ni);
                    }
                }
            }
        }
        out.push(c);
    }
    out
}

/// ゴミの連結成分を消す: 小さすぎるもの、離れて孤立した小さな点（ほこり）、
/// 上下の縁に触れるもの（文字は枠の縦中央にあるので、枠線の残りやバッジ）、縁に張り付いた細長いもの。
fn remove_junk(mask: &mut [bool]) {
    let comps = components(mask);
    let mut label = vec![usize::MAX; NPIX];
    for (k, c) in comps.iter().enumerate() {
        for &i in &c.cells {
            label[i] = k;
        }
    }
    for (k, c) in comps.iter().enumerate() {
        let touches_tb = c.y0 == 0 || c.y1 == FH - 1;
        let touches = touches_tb || c.x0 == 0 || c.x1 == FW - 1;
        let spans = (c.x1 - c.x0 + 1) >= FW * 9 / 10 || (c.y1 - c.y0 + 1) >= FH * 9 / 10;
        let isolated = c.cells.len() < SPECK_AREA
            && c.cells.iter().all(|&i| {
                let (x, y) = ((i % FW) as i32, (i / FW) as i32);
                (-SPECK_DIST..=SPECK_DIST).all(|dy| {
                    (-SPECK_DIST..=SPECK_DIST).all(|dx| {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= FW as i32 || ny >= FH as i32 {
                            return true;
                        }
                        let l = label[ny as usize * FW + nx as usize];
                        l == usize::MAX || l == k
                    })
                })
            });
        if c.cells.len() < MIN_AREA || isolated || touches_tb || (touches && spans) {
            for &i in &c.cells {
                mask[i] = false;
            }
        }
    }
}

/// 2 値マスクを、外接矩形の中心が特徴の中心に来るよう整数 px だけ動かす。
fn center_mask(mask: &[bool]) -> Vec<bool> {
    let (mut x0, mut y0, mut x1, mut y1) = (FW, FH, 0usize, 0usize);
    let mut any = false;
    for (i, &m) in mask.iter().enumerate() {
        if m {
            any = true;
            let (x, y) = (i % FW, i / FW);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    if !any {
        return mask.to_vec();
    }
    let sx = FW as i32 / 2 - (x0 + x1) as i32 / 2;
    let sy = FH as i32 / 2 - (y0 + y1) as i32 / 2;
    let mut out = vec![false; NPIX];
    for (i, &m) in mask.iter().enumerate() {
        if m {
            let (nx, ny) = ((i % FW) as i32 + sx, (i / FW) as i32 + sy);
            if (0..FW as i32).contains(&nx) && (0..FH as i32).contains(&ny) {
                out[ny as usize * FW + nx as usize] = true;
            }
        }
    }
    out
}

/// 3x3 の平均（位置ずれと線の太さの揺れをならす）。
fn blur3(mask: &[bool]) -> Vec<f32> {
    let mut out = vec![0f32; NPIX];
    for y in 0..FH {
        for x in 0..FW {
            let mut s = 0;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if (0..FW as i32).contains(&nx)
                        && (0..FH as i32).contains(&ny)
                        && mask[ny as usize * FW + nx as usize]
                    {
                        s += 1;
                    }
                }
            }
            out[y * FW + x] = s as f32 / 9.0;
        }
    }
    out
}

/// 枠の切り抜き → 文字だけの特徴（FW x FH、0..1）。
/// 背景色（枠内の各チャンネルの中央値）との差で文字を拾うので、色の組み合わせに依らない。
pub fn preprocess(img: &RgbImage, center: bool) -> Vec<f32> {
    let small = imageops::resize(img, FW as u32, FH as u32, imageops::FilterType::Triangle);
    let mut med = [0u8; 3];
    for (c, m) in med.iter_mut().enumerate() {
        let mut v: Vec<u8> = small.pixels().map(|p| p[c]).collect();
        *m = median_u8(&mut v);
    }
    let diff: Vec<f32> = small
        .pixels()
        .map(|p| {
            (0..3)
                .map(|c| (p[c] as i32 - med[c] as i32).abs())
                .max()
                .unwrap() as f32
        })
        .collect();
    let thr = text_threshold(&diff);
    let mut mask: Vec<bool> = diff.iter().map(|&d| d >= thr).collect();
    remove_junk(&mut mask);
    if center {
        mask = center_mask(&mask);
    }
    blur3(&mask)
}

// ---- 類似度 ----

/// 平均を引いて長さ 1 にする。全部同じ値（文字なし）は零ベクトル（= 誰とも似ない）。
pub fn normalize(raw: &[f32]) -> Vec<f32> {
    let mean = raw.iter().sum::<f32>() / raw.len() as f32;
    let norm = raw
        .iter()
        .map(|v| (v - mean) * (v - mean))
        .sum::<f32>()
        .sqrt();
    if norm < 1e-6 {
        return vec![0.0; raw.len()];
    }
    raw.iter().map(|v| (v - mean) / norm).collect()
}

pub fn dot_shift(a: &[f32], b: &[f32], dx: i32, dy: i32) -> f32 {
    let mut s = 0f32;
    let (y0, y1) = (dy.max(0) as usize, (FH as i32 + dy.min(0)) as usize);
    let (x0, x1) = (dx.max(0) as usize, (FW as i32 + dx.min(0)) as usize);
    for y in y0..y1 {
        let yb = (y as i32 - dy) as usize;
        let ra = &a[y * FW + x0..y * FW + x1];
        let xb0 = (x0 as i32 - dx) as usize;
        let rb = &b[yb * FW + xb0..yb * FW + xb0 + (x1 - x0)];
        s += ra.iter().zip(rb).map(|(p, q)| p * q).sum::<f32>();
    }
    s
}

/// 正規化済み 2 ベクトルの相互相関の、位置ずれ（±SHIFT）での最大値と、そのずれ。[-1, 1]。
/// ずれ (dx, dy) は `a[y][x] * b[y-dy][x-dx]` の向き。
pub fn similarity_arg(a: &[f32], b: &[f32]) -> (f32, i32, i32) {
    let mut best = (f32::MIN, 0, 0);
    for dy in -SHIFT_Y..=SHIFT_Y {
        for dx in -SHIFT_X..=SHIFT_X {
            let s = dot_shift(a, b, dx, dy);
            if s > best.0 {
                best = (s, dx, dy);
            }
        }
    }
    best
}

pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    similarity_arg(a, b).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    /// 枠の色の上に、縦棒の文字風パターンを描く。
    fn synth_box(bg: [u8; 3], fg: [u8; 3], bars: &[(u32, u32)], w: u32, h: u32) -> RgbImage {
        let mut img = RgbImage::from_pixel(w, h, Rgb(bg));
        for &(x, bw) in bars {
            for y in h / 4..h * 3 / 4 {
                for xx in x..x + bw {
                    img.put_pixel(xx, y, Rgb(fg));
                }
            }
        }
        img
    }

    const BARS_A: [(u32, u32); 3] = [(40, 12), (80, 12), (120, 12)];
    const BARS_B: [(u32, u32); 4] = [(30, 10), (60, 30), (110, 10), (140, 20)];

    #[test]
    fn polarity_does_not_matter() {
        let white_on_dark = preprocess(
            &synth_box([48, 48, 48], [255, 255, 255], &BARS_A, 200, 64),
            true,
        );
        let dark_on_yellow = preprocess(
            &synth_box([255, 216, 0], [20, 20, 20], &BARS_A, 200, 64),
            true,
        );
        let yellow_on_dark = preprocess(
            &synth_box([48, 48, 48], [255, 216, 0], &BARS_A, 200, 64),
            true,
        );
        let a = normalize(&white_on_dark);
        assert!(similarity(&a, &normalize(&dark_on_yellow)) > 0.95);
        assert!(similarity(&a, &normalize(&yellow_on_dark)) > 0.95);
    }

    #[test]
    fn different_text_is_dissimilar() {
        let a = normalize(&preprocess(
            &synth_box([48, 48, 48], [255, 255, 255], &BARS_A, 200, 64),
            true,
        ));
        let b = normalize(&preprocess(
            &synth_box([48, 48, 48], [255, 255, 255], &BARS_B, 200, 64),
            true,
        ));
        assert!(similarity(&a, &b) < 0.6, "{}", similarity(&a, &b));
    }

    #[test]
    fn dust_speck_is_removed() {
        let clean = synth_box([48, 48, 48], [255, 255, 255], &BARS_A, 200, 64);
        let mut dusty = clean.clone();
        for y in 3..7 {
            for x in 3..7 {
                dusty.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let (a, b) = (preprocess(&clean, true), preprocess(&dusty, true));
        let diff: f32 = a.iter().zip(&b).map(|(p, q)| (p - q).abs()).sum();
        assert!(diff < 1.0, "{diff}");
    }

    #[test]
    fn blank_box_gives_empty_feature() {
        let f = preprocess(&RgbImage::from_pixel(200, 64, Rgb([48, 48, 48])), true);
        assert!(f.iter().all(|&v| v == 0.0));
        assert!(normalize(&f).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn small_offset_is_tolerated_without_centering() {
        let a = normalize(&preprocess(
            &synth_box([48, 48, 48], [255, 255, 255], &BARS_A, 200, 64),
            false,
        ));
        let shifted: Vec<(u32, u32)> = BARS_A.iter().map(|&(x, w)| (x + 3, w)).collect(); // 縮小後 約 1.4px
        let b = normalize(&preprocess(
            &synth_box([48, 48, 48], [255, 255, 255], &shifted, 200, 64),
            false,
        ));
        assert!(similarity(&a, &b) > 0.9);
    }

    #[test]
    fn similarity_properties() {
        let a = normalize(&preprocess(
            &synth_box([48, 48, 48], [255, 255, 255], &BARS_A, 200, 64),
            true,
        ));
        let b = normalize(&preprocess(
            &synth_box([48, 48, 48], [255, 255, 255], &BARS_B, 200, 64),
            true,
        ));
        assert!((similarity(&a, &a) - 1.0).abs() < 1e-4);
        assert!((similarity(&a, &b) - similarity(&b, &a)).abs() < 1e-5);
        assert!(similarity(&a, &b) <= 1.0 + 1e-5);
        assert!(similarity(&a, &vec![0.0; NPIX]).abs() < 1e-6);
    }
}
