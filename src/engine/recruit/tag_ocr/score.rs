//! テンプレートとの採点。1 位・2 位の類似度と、紛らわしい 2 タグの再判定（違う画素だけで測り直す）。
//!
//! 近距離/遠距離・「〜タイプ」・エリート/上級エリートのように文字を共有するタグは、全体の相関だと
//! 差が小さい。1 位と 2 位のテンプレートが似ているとき、2 つが違う画素だけで測り直して差を取る。

use super::feature::{normalize, similarity, similarity_arg, FH, FW, NPIX};
use std::borrow::Cow;

/// 2 つのテンプレートが似ているとみなす相関の下限（この組だけ再判定する）。
pub const PAIR_SIM: f32 = 0.85;
/// 2 テンプレートの差がこの値を超える画素を「違う画素」とし、DIFF_DILATE px 太らせる。
pub const DIFF_THR: f32 = 0.25;
pub const DIFF_DILATE: i32 = 2;

#[derive(Clone)]
pub struct Template {
    pub raw: Vec<f32>,
    pub norm: Vec<f32>,
    /// このテンプレートに割り当てられた学習サンプル数
    pub count: usize,
}

impl Template {
    pub fn new(raw: Vec<f32>, count: usize) -> Self {
        let norm = normalize(&raw);
        Template { raw, norm, count }
    }
}

/// そのタグの代表のうち一番似ているもの: (類似度, 代表の添字, ずれ dx, ずれ dy)。代表が無ければ最低値。
pub fn tag_best(norm: &[f32], protos: &[Template]) -> (f32, usize, i32, i32) {
    let mut best = (f32::MIN, 0, 0, 0);
    for (k, p) in protos.iter().enumerate() {
        let (s, dx, dy) = similarity_arg(norm, &p.norm);
        if s > best.0 {
            best = (s, k, dx, dy);
        }
    }
    best
}

/// 2 テンプレートの違う画素（太らせてある）。
pub fn diff_mask(ta: &[f32], tb: &[f32]) -> Vec<bool> {
    let raw: Vec<bool> = ta.iter().zip(tb).map(|(a, b)| (a - b).abs() > DIFF_THR).collect();
    let mut out = vec![false; NPIX];
    for (i, &m) in raw.iter().enumerate() {
        if !m {
            continue;
        }
        let (x, y) = ((i % FW) as i32, (i / FW) as i32);
        for dy in -DIFF_DILATE..=DIFF_DILATE {
            for dx in -DIFF_DILATE..=DIFF_DILATE {
                let (nx, ny) = (x + dx, y + dy);
                if (0..FW as i32).contains(&nx) && (0..FH as i32).contains(&ny) {
                    out[ny as usize * FW + nx as usize] = true;
                }
            }
        }
    }
    out
}

/// マスク内の画素だけのピアソン相関（枠の特徴 `f` とテンプレート `t`。t は (dx, dy) ずらして重ねる）。
pub fn masked_sim(f: &[f32], t: &[f32], mask: &[bool], dx: i32, dy: i32) -> f32 {
    let (mut n, mut sf, mut st, mut sff, mut stt, mut sft) = (0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
    for (i, &m) in mask.iter().enumerate() {
        if !m {
            continue;
        }
        let (x, y) = ((i % FW) as i32 - dx, (i / FW) as i32 - dy);
        if x < 0 || y < 0 || x >= FW as i32 || y >= FH as i32 {
            continue;
        }
        let (a, b) = (f[i], t[y as usize * FW + x as usize]);
        n += 1.0;
        sf += a;
        st += b;
        sff += a * a;
        stt += b * b;
        sft += a * b;
    }
    if n < 20.0 {
        return 0.0;
    }
    let (vf, vt) = (sff - sf * sf / n, stt - st * st / n);
    if vf < 1e-6 || vt < 1e-6 {
        return 0.0;
    }
    (sft - sf * st / n) / (vf * vt).sqrt()
}

/// テンプレート一式と、タグ同士の似かた（代表どうしの相関の最大値）。
pub struct Model<'a> {
    pub templates: Cow<'a, [Vec<Template>]>,
    pub gate: Vec<Vec<f32>>,
}

impl<'a> Model<'a> {
    /// 全タグの組の相関を計算して作る（重い。学習側が使う）。
    pub fn new(templates: &'a [Vec<Template>]) -> Self {
        let n = templates.len();
        let mut gate = vec![vec![0f32; n]; n];
        for a in 0..n {
            for b in a + 1..n {
                let mut best = 0f32;
                for pa in &templates[a] {
                    for pb in &templates[b] {
                        best = best.max(similarity(&pa.norm, &pb.norm));
                    }
                }
                gate[a][b] = best;
                gate[b][a] = best;
            }
        }
        Model { templates: Cow::Borrowed(templates), gate }
    }

    /// 紛らわしい組（相関が PAIR_SIM 以上）だけを与えて作る（本番。全組の計算は起動が重いので
    /// テンプレートの書き出し時に済ませて meta.json に持つ）。他の組は 0 とみなす。
    pub fn with_confusable(templates: Vec<Vec<Template>>, pairs: &[(usize, usize, f32)]) -> Model<'static> {
        let n = templates.len();
        let mut gate = vec![vec![0f32; n]; n];
        for &(a, b, v) in pairs {
            gate[a][b] = v;
            gate[b][a] = v;
        }
        Model { templates: Cow::Owned(templates), gate }
    }

    /// 相関が PAIR_SIM 以上の組: (a, b, 相関)。a < b。
    pub fn confusable_pairs(&self) -> Vec<(usize, usize, f32)> {
        let mut out = Vec::new();
        for (a, row) in self.gate.iter().enumerate() {
            for (b, &v) in row.iter().enumerate().skip(a + 1) {
                if v >= PAIR_SIM {
                    out.push((a, b, v));
                }
            }
        }
        out
    }
}

/// 1 枠の 1 位・2 位。
#[derive(Debug, Clone, Copy)]
pub struct TopScore {
    pub best_tag: usize,
    pub best: f32,
    pub second_tag: usize,
    pub second: f32,
    /// 受理の条件に使う 1 位と 2 位の差。再判定した組では、違う画素だけの相関の差。
    pub margin: f32,
}

/// 全テンプレートとの類似度から、1 位と 2 位を取る。
pub fn top2(scores: &[f32]) -> TopScore {
    let mut b = TopScore { best_tag: 0, best: f32::MIN, second_tag: 0, second: f32::MIN, margin: 0.0 };
    for (t, &s) in scores.iter().enumerate() {
        if s > b.best {
            b.second = b.best;
            b.second_tag = b.best_tag;
            b.best = s;
            b.best_tag = t;
        } else if s > b.second {
            b.second = s;
            b.second_tag = t;
        }
    }
    b.margin = b.best - b.second;
    b
}

/// 1 枠の採点結果。
pub struct BoxResult {
    /// 全タグとの類似度
    pub scores: Vec<f32>,
    /// 全体の相関だけの 1 位・2 位
    pub plain: TopScore,
    /// 紛らわしい組を再判定した後の 1 位・2 位
    pub refined: TopScore,
    /// 1 位のタグのどの代表に一番似ていたか
    pub best_proto: usize,
    /// 再判定した組: (全体の相関での 1 位, 2 位, 違う画素での相関 1 位, 2 位)
    pub pair: Option<(usize, usize, f32, f32)>,
}

pub fn score_box(raw: &[f32], norm: &[f32], model: &Model) -> BoxResult {
    let per: Vec<(f32, usize, i32, i32)> = model.templates.iter().map(|ps| tag_best(norm, ps)).collect();
    let scores: Vec<f32> = per.iter().map(|p| p.0).collect();
    let plain = top2(&scores);
    let mut refined = plain;
    let mut pair = None;
    let (a, c) = (plain.best_tag, plain.second_tag);
    if plain.second > f32::MIN && model.gate[a][c] >= PAIR_SIM {
        let (ta, tc) = (&model.templates[a][per[a].1].raw, &model.templates[c][per[c].1].raw);
        let mask = diff_mask(ta, tc);
        let sa = masked_sim(raw, ta, &mask, per[a].2, per[a].3);
        let sc = masked_sim(raw, tc, &mask, per[c].2, per[c].3);
        pair = Some((a, c, sa, sc));
        if sc > sa {
            refined = TopScore { best_tag: c, best: scores[c], second_tag: a, second: scores[a], margin: sc - sa };
        } else {
            refined.margin = sa - sc;
        }
    }
    BoxResult { scores, plain, refined, best_proto: per[plain.best_tag].1, pair }
}

pub fn accepts(b: &TopScore, t: f32, m: f32) -> bool {
    b.best >= t && b.margin >= m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_feat(rects: &[(usize, usize, usize, usize)]) -> Vec<f32> {
        let mut v = vec![0f32; NPIX];
        for &(x, y, w, h) in rects {
            for yy in y..y + h {
                for xx in x..x + w {
                    v[yy * FW + xx] = 1.0;
                }
            }
        }
        v
    }

    /// 共通の大きな塊 + 違う位置の細い縦棒（近距離/遠距離のように、ほとんど同じで一部だけ違う 2 タグ）。
    fn pair_feats() -> (Vec<f32>, Vec<f32>) {
        let common = (60, 16, 70, 32);
        (rect_feat(&[common, (140, 16, 4, 32)]), rect_feat(&[common, (40, 16, 4, 32)]))
    }

    #[test]
    fn diff_mask_covers_only_differing_strokes() {
        let (a, b) = pair_feats();
        let m = diff_mask(&a, &b);
        assert!(m[30 * FW + 141] && m[30 * FW + 41]); // 違う棒の上
        assert!(!m[30 * FW + 100]); // 共通の塊の真ん中
        assert!(m[30 * FW + 145]); // 太らせた縁
        assert!(!m[30 * FW + 148] && !m[30 * FW + 175]);
    }

    #[test]
    fn masked_sim_separates_what_plain_ncc_cannot() {
        let (a, b) = pair_feats();
        let (na, nb) = (normalize(&a), normalize(&b));
        // 全体の相関では A と B はよく似ている
        assert!(similarity(&na, &nb) > 0.7);
        let plain_margin = similarity(&na, &na) - similarity(&na, &nb);
        let mask = diff_mask(&a, &b);
        // A の特徴を、違う画素だけで A・B と比べる
        let (sa, sb) = (masked_sim(&a, &a, &mask, 0, 0), masked_sim(&a, &b, &mask, 0, 0));
        assert!(sa > 0.99 && sb < -0.2, "{sa} {sb}");
        assert!(sa - sb > 3.0 * plain_margin);
    }

    #[test]
    fn masked_sim_degenerate_is_zero() {
        let (a, _) = pair_feats();
        assert_eq!(masked_sim(&a, &a, &vec![false; NPIX], 0, 0), 0.0); // マスクが空
        let flat = vec![0.5f32; NPIX];
        assert_eq!(masked_sim(&flat, &a, &vec![true; NPIX], 0, 0), 0.0); // 分散ゼロ
    }

    #[test]
    fn pair_rescoring_flips_a_wrong_plain_winner() {
        let (a, b) = pair_feats();
        // 3 つ目のタグは全然違う
        let other = rect_feat(&[(10, 5, 20, 10)]);
        let templates: Vec<Vec<Template>> = vec![vec![Template::new(a.clone(), 10)], vec![Template::new(b.clone(), 10)], vec![Template::new(other, 10)]];
        let model = Model::new(&templates);
        assert!(model.gate[0][1] >= PAIR_SIM && model.gate[0][2] < PAIR_SIM);
        // B の棒が薄く、A の棒も薄く出ている枠 → 全体の相関では差がほとんど付かないが、違う画素では B が勝つ
        let mut f = b.clone();
        for y in 16..48 {
            for x in 40..44 {
                f[y * FW + x] = 0.6;
            }
            for x in 140..144 {
                f[y * FW + x] = 0.5;
            }
        }
        let norm = normalize(&f);
        let r = score_box(&f, &norm, &model);
        assert!(r.pair.is_some());
        assert_eq!(r.refined.best_tag, 1, "{:?} {:?}", r.plain, r.pair);
        assert!(r.refined.margin > r.plain.margin);
    }
}
