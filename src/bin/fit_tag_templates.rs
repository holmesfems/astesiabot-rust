//! タグ枠の切り抜き（cut_tag_boxes の出力）から、タグごとのテンプレートを EM で学習し、
//! 「全部の枠をローカルで読み切れるか」を評価するローカル専用ツール。
//!
//! ```text
//! cargo run --release --bin fit_tag_templates -- [--in local/recruit_samples] [--tags data/recruitment/tagList.json] [--no-center] [--export data/recruitment/tag_templates]
//! ```
//!
//! - ラベル（過去の bot の返信）は「5 個の集合」としてしか信用できない。画面の枠の順とタイトルの順は
//!   一致しないことがあるので、枠ごとのタグは EM（最良の順列の割り当て → テンプレートを平均で作り直す）で推定する。
//! - 特徴は「文字だけの 2 値画像」。背景色（枠の色）との差で文字を拾うので、白文字・黄文字・黄地の黒文字を
//!   同じ形で扱える。
//! - 時系列で古い 80% を学習、新しい 20% を評価に使う。しきい値（T, M）は学習側だけで決める。
//! - 出力は `<in>/templates/`（タグごとの PNG、templates.json、eval_report.txt、mistakes.jsonl）。
//! - 学習側だけのテンプレートを本番の形式（量子化した PNG + meta.json）で書き出して `TagOcr::load` し、
//!   テスト画像の元のスクショから `TagOcr::analyze` で読み直した評価も出す（本番と同じ経路の確認）。
//! - `--export <dir>` は、英語を除く全データで学習し直したテンプレートを本番の形式で <dir> に書く
//!   （bot が読む `data/recruitment/tag_templates/` を作る。時系列分割のときだけ）。

use astesiabot_rust::engine::recruit::tag_ocr::feature::{dot_shift, normalize, preprocess, similarity, ABS_MIN_DIFF, FH, FW, MIN_AREA, NPIX, P99_RATIO, SHIFT_X, SHIFT_Y};
use astesiabot_rust::engine::recruit::tag_ocr::score::{accepts, score_box, BoxResult, Model, Template, TopScore, DIFF_DILATE, DIFF_THR, PAIR_SIM};
use astesiabot_rust::engine::recruit::tag_ocr::{store, BoxScore, TagOcr};
use image::{GrayImage, Luma};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const TRIM: f32 = 0.1;
const EM_MAX_ITER: usize = 10;
/// 1 タグを分ける代表の最大数、代表として残す最小のサンプル数、新しい代表を作る類似度の上限。
const K_MAX: usize = 6;
const MIN_CLUSTER: usize = 5;
const SPLIT_T: f32 = 0.8;
const CLUSTER_ATTEMPTS: usize = 12;
/// 代表を作って再割り当てするラウンド数。
const PROTO_ROUNDS: usize = 3;
const TRAIN_RATIO: f64 = 0.8;
/// T, M を選ぶ検証側の枠の誤読率の上限（オーナーの仕様。余裕は取らない）。
const SPEC_VAL_MISID: f64 = 0.005;

// ---- 入力の形 ----

#[derive(Debug, Deserialize)]
struct Label {
    tags: Vec<String>,
    is_global: bool,
    usable: bool,
}

#[derive(Debug, Deserialize)]
struct Record {
    posted_at: String,
    file: String,
    label: Option<Label>,
}

#[derive(Debug, Deserialize)]
struct CropRec {
    file: String,
    source: String,
    pos: usize,
    color: String,
}

#[derive(Debug, Deserialize)]
struct DetectRec {
    file: String,
    found: bool,
}

#[derive(Debug, Deserialize)]
struct TagList {
    #[serde(rename = "eliteTags")]
    elite: Vec<String>,
    #[serde(rename = "jobTags")]
    job: Vec<String>,
    #[serde(rename = "positionTags")]
    position: Vec<String>,
    #[serde(rename = "otherTags")]
    other: Vec<String>,
}


// ---- テンプレート ----

/// 画素ごとの刈り込み平均（両端を TRIM ずつ捨てる）。誤った割り当ての混入に強くする。
fn build_template(feats: &[&[f32]]) -> Vec<f32> {
    let n = feats.len();
    let cut = if n >= 10 { (n as f32 * TRIM) as usize } else { 0 };
    let mut col = vec![0f32; n];
    (0..NPIX)
        .map(|i| {
            for (k, f) in feats.iter().enumerate() {
                col[k] = f[i];
            }
            col.sort_by(|a, b| a.total_cmp(b));
            let kept = &col[cut..n - cut];
            kept.iter().sum::<f32>() / kept.len() as f32
        })
        .collect()
}


/// タグの類似度は、そのタグの代表（複数）のうち一番似ているもの。代表が無いタグは最低値。
fn tag_sim(norm: &[f32], protos: &[Template]) -> f32 {
    protos.iter().map(|p| similarity(norm, &p.norm)).fold(f32::MIN, f32::max)
}

// ---- 1 タグの中を複数の代表に分ける ----
// 英語表示の画面・「タイプ」付きの表記・ぼやけた写真など、同じタグでも見た目が別系統のものがあるので、
// 1 タグ 1 テンプレートだと平均が崩れる。タグごとに少数の代表へ分ける（k-means 風）。

/// 代表を決めるときの類似度は、位置ずれを ±1 に絞って軽くする。
fn similarity_fast(a: &[f32], b: &[f32]) -> f32 {
    let mut best = f32::MIN;
    for dy in -1..=1 {
        for dx in -2..=2 {
            best = best.max(dot_shift(a, b, dx, dy));
        }
    }
    best
}

/// 各サンプルを一番似ている代表へ。(代表の添字, その類似度)
fn assign_protos(samples: &[(&[f32], &[f32])], protos: &[Template]) -> Vec<(usize, f32)> {
    samples
        .iter()
        .map(|(_, norm)| {
            let mut best = (0, f32::MIN);
            for (k, p) in protos.iter().enumerate() {
                let s = similarity_fast(norm, &p.norm);
                if s > best.1 {
                    best = (k, s);
                }
            }
            best
        })
        .collect()
}

/// 代表を所属サンプルの刈り込み平均で作り直す。所属が無い代表はそのまま。
fn lloyd_step(samples: &[(&[f32], &[f32])], protos: &mut [Template]) -> Vec<(usize, f32)> {
    let a = assign_protos(samples, protos);
    for (k, p) in protos.iter_mut().enumerate() {
        let members: Vec<&[f32]> = samples.iter().zip(&a).filter(|(_, x)| x.0 == k).map(|(s, _)| s.0).collect();
        if !members.is_empty() {
            *p = Template::new(build_template(&members), members.len());
        }
    }
    a
}

/// 小さい塊（MIN_CLUSTER 未満）を落とす。全部落ちる場合は一番大きいものを残す。
/// 落とした代表の種は `banned` に入れて二度と種にしない。
fn drop_small(protos: &mut Vec<Template>, seeds: &mut Vec<Option<usize>>, banned: &mut [bool], sizes: &[usize]) {
    let keep_max = (0..protos.len()).max_by_key(|&k| sizes[k]).unwrap();
    let keep: Vec<bool> = (0..protos.len()).map(|k| sizes[k] >= MIN_CLUSTER || k == keep_max).collect();
    for (k, sd) in seeds.iter().enumerate() {
        if !keep[k] {
            if let Some(sd) = sd {
                banned[*sd] = true;
            }
        }
    }
    let mut k = 0;
    protos.retain(|_| {
        k += 1;
        keep[k - 1]
    });
    let mut k = 0;
    seeds.retain(|_| {
        k += 1;
        keep[k - 1]
    });
}

fn cluster_sizes(a: &[(usize, f32)], k: usize) -> Vec<usize> {
    let mut sizes = vec![0usize; k];
    for x in a {
        sizes[x.0] += 1;
    }
    sizes
}

/// 1 タグのサンプル（(生の特徴, 正規化済み)）を代表に分ける。
/// いちばん遠いサンプルを新しい代表の種にし、小さすぎる塊（ゴミ）は捨てる。
fn cluster_tag(samples: &[(&[f32], &[f32])]) -> Vec<Template> {
    let n = samples.len();
    let all: Vec<&[f32]> = samples.iter().map(|s| s.0).collect();
    let mut protos = vec![Template::new(build_template(&all), n)];
    if n < 2 * MIN_CLUSTER {
        return protos;
    }
    let mut seeds: Vec<Option<usize>> = vec![None];
    let mut banned = vec![false; n];
    for _ in 0..CLUSTER_ATTEMPTS {
        if protos.len() >= K_MAX {
            break;
        }
        let a = assign_protos(samples, &protos);
        let far = (0..n).filter(|&i| !banned[i]).min_by(|&i, &j| a[i].1.total_cmp(&a[j].1));
        let Some(far) = far else { break };
        if a[far].1 >= SPLIT_T {
            break;
        }
        protos.push(Template::new(samples[far].0.to_vec(), 1));
        seeds.push(Some(far));
        let mut a = a;
        for _ in 0..3 {
            a = lloyd_step(samples, &mut protos);
        }
        let sizes = cluster_sizes(&a, protos.len());
        drop_small(&mut protos, &mut seeds, &mut banned, &sizes);
    }
    let mut a = Vec::new();
    for _ in 0..3 {
        a = lloyd_step(samples, &mut protos);
    }
    let sizes = cluster_sizes(&a, protos.len());
    for (p, &c) in protos.iter_mut().zip(&sizes) {
        p.count = c;
    }
    drop_small(&mut protos, &mut seeds, &mut banned, &sizes);
    protos
}

// ---- 順列の割り当て ----

fn perms5() -> Vec<[usize; 5]> {
    fn rec(cur: &mut Vec<usize>, used: &mut [bool; 5], out: &mut Vec<[usize; 5]>) {
        if cur.len() == 5 {
            out.push([cur[0], cur[1], cur[2], cur[3], cur[4]]);
            return;
        }
        for i in 0..5 {
            if !used[i] {
                used[i] = true;
                cur.push(i);
                rec(cur, used, out);
                cur.pop();
                used[i] = false;
            }
        }
    }
    let mut out = Vec::with_capacity(120);
    rec(&mut Vec::new(), &mut [false; 5], &mut out);
    out
}

/// `s[box][label]` の合計が最大になる割り当て。`perm[box] = label の添字`。
fn best_perm(s: &[[f32; 5]; 5], perms: &[[usize; 5]]) -> [usize; 5] {
    let mut best = (f32::MIN, perms[0]);
    for p in perms {
        let t: f32 = (0..5).map(|i| s[i][p[i]]).sum();
        if t > best.0 {
            best = (t, *p);
        }
    }
    best.1
}


/// 正解（truth）から見た差。再判定した組に正解が入っていれば違う画素での差、そうでなければ全体の相関での差。
fn truth_margin(r: &BoxResult, truth: usize, refined: bool) -> f32 {
    if refined {
        if let Some((a, c, sa, sc)) = r.pair {
            if truth == a {
                return sa - sc;
            }
            if truth == c {
                return sc - sa;
            }
        }
    }
    let w = r.scores.iter().enumerate().filter(|(i, _)| *i != truth).map(|(_, &s)| s).fold(f32::MIN, f32::max);
    r.scores[truth] - w
}


// ---- しきい値の評価と選択 ----

/// 評価の 1 枚ぶん。`boxes` が None は枠が見つからなかった画像（Google 行き）。
struct ImgEval {
    label: [usize; 5],
    english: bool,
    boxes: Option<[TopScore; 5]>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Metrics {
    images: usize,
    accepted: usize,
    misid: usize,
    fully_read: usize,
    /// 全部読めたが、読んだ集合がラベルと違う画像
    wrong_images: usize,
}

impl Metrics {
    fn misid_rate(&self) -> f64 {
        if self.accepted == 0 { 0.0 } else { self.misid as f64 / self.accepted as f64 }
    }
    fn full_rate(&self) -> f64 {
        if self.images == 0 { 0.0 } else { self.fully_read as f64 / self.images as f64 }
    }
    fn wrong_rate(&self) -> f64 {
        if self.fully_read == 0 { 0.0 } else { self.wrong_images as f64 / self.fully_read as f64 }
    }
}

/// 1 枚を (T, M) で判定する。(受理した枠, 誤読の枠, 全部読めたか, 集合が違うか)
fn judge(e: &ImgEval, t: f32, m: f32) -> (usize, usize, bool, bool) {
    let Some(bs) = &e.boxes else { return (0, 0, false, false) };
    let acc: Vec<usize> = (0..5).filter(|&i| accepts(&bs[i], t, m)).map(|i| bs[i].best_tag).collect();
    let mut counts: HashMap<usize, usize> = HashMap::new();
    for &t in &acc {
        *counts.entry(t).or_default() += 1;
    }
    // ラベルに無い枠 + 同じタグの 2 個目以降（ラベルは 5 個が全部違うので、重複は必ずどれかが誤り）
    let outside = acc.iter().filter(|t| !e.label.contains(t)).count();
    let dups: usize = counts.iter().filter(|(t, _)| e.label.contains(t)).map(|(_, c)| c - 1).sum();
    let misid = outside + dups;
    let full = acc.len() == 5 && counts.len() == 5;
    (acc.len(), misid, full, full && misid > 0)
}

fn evaluate_filtered(imgs: &[ImgEval], t: f32, m: f32, keep: impl Fn(&ImgEval) -> bool) -> Metrics {
    let mut r = Metrics::default();
    for e in imgs.iter().filter(|e| keep(e)) {
        let (a, mi, full, wrong) = judge(e, t, m);
        r.images += 1;
        r.accepted += a;
        r.misid += mi;
        r.fully_read += full as usize;
        r.wrong_images += wrong as usize;
    }
    r
}

fn evaluate(imgs: &[ImgEval], t: f32, m: f32) -> Metrics {
    evaluate_filtered(imgs, t, m, |_| true)
}

/// 英語画面の画像についての安全の数字（読まれてはいけない）。
#[derive(Debug, Default, Clone, Copy)]
struct EnStats {
    images: usize,
    grid_not_found: usize,
    fully_read: usize,
    boxes: usize,
    accepted_boxes: usize,
}

fn english_stats(imgs: &[ImgEval], t: f32, m: f32) -> EnStats {
    let mut s = EnStats::default();
    for e in imgs.iter().filter(|e| e.english) {
        s.images += 1;
        match &e.boxes {
            None => s.grid_not_found += 1,
            Some(bs) => {
                s.boxes += 5;
                s.accepted_boxes += bs.iter().filter(|b| accepts(b, t, m)).count();
                s.fully_read += judge(e, t, m).2 as usize;
            }
        }
    }
    s
}

/// 誤読率が上限以内で全部読める画像が最多になる (T, M)。同点なら緩すぎない（T+M が大きい）方。
fn select_tm(imgs: &[ImgEval], max_misid: f64) -> Option<(f32, f32, Metrics)> {
    let mut best: Option<(f32, f32, Metrics)> = None;
    for ti in 30..=99 {
        for mi in 0..=60 {
            let (t, m) = (ti as f32 / 100.0, mi as f32 / 100.0);
            let r = evaluate(imgs, t, m);
            if r.misid_rate() > max_misid {
                continue;
            }
            let better = match &best {
                None => true,
                Some((bt, bm, br)) => r.fully_read > br.fully_read || (r.fully_read == br.fully_read && t + m > bt + bm),
            };
            if better {
                best = Some((t, m, r));
            }
        }
    }
    best
}

fn percentile(sorted: &[f32], q: f64) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

// ---- 並列 ----

fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() {
                    break;
                }
                let r = f(&items[i]);
                out.lock().unwrap()[i] = Some(r);
            });
        }
    });
    out.into_inner().unwrap().into_iter().map(|o| o.expect("par_map")).collect()
}

// ---- データ ----

struct BoxData {
    raw: Vec<f32>,
    norm: Vec<f32>,
    color: String,
    crop_file: String,
}

struct ImgData {
    file: String,
    posted_at: String,
    /// 英語クライアントの画面（ローカル OCR の対象外。学習に使わず、読まれないことを確かめる）
    english: bool,
    /// ラベルのタグ（タイトルの順。画面の順とは限らない）
    label: [usize; 5],
    /// 画面の位置 0..4 の枠。枠が見つからなかった画像は None。
    boxes: Option<Vec<BoxData>>,
}

struct Args {
    root: PathBuf,
    tags: PathBuf,
    center: bool,
    /// 時系列でなく、ファイル名のハッシュ順で 80/20 に分ける（表示言語などの経時変化を混ぜた参考値用）。
    shuffle: bool,
    out_dir: String,
    /// 英語を除く全データで学習し直した本番テンプレートを、この場所に PNG + meta.json で書き出す。
    export: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args { root: PathBuf::from("local/recruit_samples"), tags: PathBuf::from("data/recruitment/tagList.json"), center: true, shuffle: false, out_dir: "templates".into(), export: None };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--in" => a.root = PathBuf::from(it.next().expect("--in に値が要る")),
            "--tags" => a.tags = PathBuf::from(it.next().expect("--tags に値が要る")),
            "--no-center" => a.center = false,
            "--shuffle" => a.shuffle = true,
            "--out-dir" => a.out_dir = it.next().expect("--out-dir に値が要る"),
            "--export" => a.export = Some(PathBuf::from(it.next().expect("--export に値が要る"))),
            other => panic!("知らない引数: {other}"),
        }
    }
    a
}

/// 分割を毎回同じにするための単純なハッシュ。
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &std::path::Path) -> Vec<T> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} が読めない: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("jsonl の行が壊れている"))
        .collect()
}

/// 割り当てから、エリート系タグと黄色い枠の対応を数える。(エリート枠数, 黄色いエリート枠数, 黄色枠数)
fn color_sanity(items: &[(Vec<usize>, Vec<&str>)], elite: &[usize]) -> (usize, usize, usize) {
    let (mut e, mut ey, mut y) = (0, 0, 0);
    for (tags, colors) in items {
        for (t, c) in tags.iter().zip(colors) {
            let is_e = elite.contains(t);
            let is_y = *c == "yellow";
            e += is_e as usize;
            y += is_y as usize;
            ey += (is_e && is_y) as usize;
        }
    }
    (e, ey, y)
}

/// 学習サンプル（枠）をタグごとに集める。
fn groups_of<'a>(set: &[&'a ImgData], assign: &[[usize; 5]], ntags: usize) -> Vec<Vec<&'a BoxData>> {
    let mut groups: Vec<Vec<&BoxData>> = vec![Vec::new(); ntags];
    for (img, a) in set.iter().zip(assign) {
        for (b, &t) in img.boxes.as_ref().unwrap().iter().zip(a) {
            groups[t].push(b);
        }
    }
    groups
}

/// ラベルの 5 タグを 5 枠に割り当てる（類似度の合計が最大の順列）。戻りは枠ごとのタグ。
fn assign_set(templates: &[Vec<Template>], set: &[&ImgData], perms: &[[usize; 5]]) -> Vec<[usize; 5]> {
    par_map(set, |img| {
        let bs = img.boxes.as_ref().unwrap();
        let mut s = [[f32::MIN; 5]; 5];
        for (i, b) in bs.iter().enumerate() {
            for (j, &t) in img.label.iter().enumerate() {
                s[i][j] = tag_sim(&b.norm, &templates[t]);
            }
        }
        let p = best_perm(&s, perms);
        [img.label[p[0]], img.label[p[1]], img.label[p[2]], img.label[p[3]], img.label[p[4]]]
    })
}

/// EM でテンプレートを学習する。戻りは (タグごとの代表, 枠ごとの推定タグ)。経過は `log` に積む。
fn fit(set: &[&ImgData], tags: &[String], perms: &[[usize; 5]], log: &mut Vec<String>) -> (Vec<Vec<Template>>, Vec<[usize; 5]>) {
    let ntags = tags.len();
    // 段階 1: 1 タグ 1 テンプレート。初期値はタイトルの順の位置のまま（多数派は正しい）
    let rebuild = |assign: &[[usize; 5]]| -> Vec<Vec<Template>> {
        par_map(&groups_of(set, assign, ntags), |g| {
            if g.is_empty() {
                Vec::new()
            } else {
                let raws: Vec<&[f32]> = g.iter().map(|b| b.raw.as_slice()).collect();
                vec![Template::new(build_template(&raws), g.len())]
            }
        })
    };
    // 段階 2: 1 タグを複数の代表に分ける
    let rebuild_multi = |assign: &[[usize; 5]]| -> Vec<Vec<Template>> {
        par_map(&groups_of(set, assign, ntags), |g| {
            if g.is_empty() {
                Vec::new()
            } else {
                let samples: Vec<(&[f32], &[f32])> = g.iter().map(|b| (b.raw.as_slice(), b.norm.as_slice())).collect();
                cluster_tag(&samples)
            }
        })
    };
    let diff_count = |a: &[[usize; 5]], b: &[[usize; 5]]| -> (usize, usize) {
        (a.iter().zip(b).map(|(x, y)| x.iter().zip(y).filter(|(p, q)| p != q).count()).sum(), a.iter().zip(b).filter(|(x, y)| x != y).count())
    };

    let mut assign: Vec<[usize; 5]> = set.iter().map(|i| i.label).collect();
    let mut templates = rebuild(&assign);
    log.push(format!("EM stage 1: one template per tag ({} images):", set.len()));
    for it in 1..=EM_MAX_ITER {
        let next = assign_set(&templates, set, perms);
        let (changed, changed_imgs) = diff_count(&next, &assign);
        log.push(format!("  iter {it}: changed boxes {changed} ({changed_imgs} images)"));
        assign = next;
        templates = rebuild(&assign);
        if changed == 0 {
            break;
        }
    }
    log.push(format!("EM stage 2: several prototypes per tag (K<={K_MAX}, min cluster {MIN_CLUSTER}, split below {SPLIT_T}):"));
    for round in 1..=PROTO_ROUNDS {
        templates = rebuild_multi(&assign);
        let next = assign_set(&templates, set, perms);
        let (changed, changed_imgs) = diff_count(&next, &assign);
        log.push(format!("  round {round}: prototypes {}, changed boxes {changed} ({changed_imgs} images)", templates.iter().map(|t| t.len()).sum::<usize>()));
        assign = next;
    }
    templates = rebuild_multi(&assign);
    log.push(format!(
        "  prototypes per tag (samples): {}",
        tags.iter()
            .zip(&templates)
            .map(|(t, ps)| format!("{t}=[{}]", ps.iter().map(|p| p.count.to_string()).collect::<Vec<_>>().join(",")))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    let n_title_diff: usize = assign.iter().zip(set).filter(|(a, i)| **a != i.label).count();
    log.push(format!("  images whose box order differs from title order: {n_title_diff}/{}", set.len()));
    (templates, assign)
}

fn score_boxes(set: &[&ImgData], model: &Model) -> Vec<Option<Vec<BoxResult>>> {
    par_map(set, |img| img.boxes.as_ref().map(|bs| bs.iter().map(|b| score_box(&b.raw, &b.norm, model)).collect()))
}

/// `refined` が true なら紛らわしい組の再判定を使う。
fn to_evals(set: &[&ImgData], res: &[Option<Vec<BoxResult>>], refined: bool) -> Vec<ImgEval> {
    set.iter()
        .zip(res)
        .map(|(img, r)| ImgEval {
            label: img.label,
            english: img.english,
            boxes: r.as_ref().map(|r| {
                let v: Vec<TopScore> = r.iter().map(|x| if refined { x.refined } else { x.plain }).collect();
                [v[0], v[1], v[2], v[3], v[4]]
            }),
        })
        .collect()
}

/// テンプレートの PNG（4 倍）と templates.json を書く。ディレクトリは作り直す。
fn write_templates(dir: &std::path::Path, tags: &[String], templates: &[Vec<Template>], extra: serde_json::Value, center: bool) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("templates dir");
    let mut tmpl_json = BTreeMap::new();
    for (ti, protos) in templates.iter().enumerate() {
        let mut list = Vec::new();
        let name: String = tags[ti].chars().map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
        for (k, tp) in protos.iter().enumerate() {
            let mut g = GrayImage::new(FW as u32 * 4, FH as u32 * 4);
            for (x, y, p) in g.enumerate_pixels_mut() {
                *p = Luma([(tp.raw[(y as usize / 4) * FW + x as usize / 4].clamp(0.0, 1.0) * 255.0).round() as u8]);
            }
            g.save(dir.join(format!("{name}_{}.png", k + 1))).expect("template png");
            list.push(json!({"count": tp.count, "values": tp.raw.iter().map(|v| (v * 1000.0).round() / 1000.0).collect::<Vec<f32>>()}));
        }
        tmpl_json.insert(tags[ti].clone(), list);
    }
    let mut doc = json!({
        "width": FW, "height": FH, "shift_x": SHIFT_X, "shift_y": SHIFT_Y,
        "center": center, "min_area": MIN_AREA, "abs_min_diff": ABS_MIN_DIFF, "p99_ratio": P99_RATIO,
        "similarity": "zero-mean normalized cross-correlation, max over shifts",
        "pair_rescoring": {
            "pair_sim": PAIR_SIM, "diff_threshold": DIFF_THR, "diff_dilate": DIFF_DILATE,
            "note": "if best and second tags' templates have NCC >= pair_sim, re-score both by Pearson correlation on pixels where |tA-tB| > diff_threshold (dilated); margin = difference of those",
        },
        "templates": tmpl_json,
    });
    for (k, v) in extra.as_object().expect("extra object") {
        doc[k] = v.clone();
    }
    std::fs::write(dir.join("templates.json"), serde_json::to_string(&doc).unwrap()).expect("templates.json");
}

fn metrics_line(name: &str, r: &Metrics) -> String {
    format!(
        "{name}: misid {}/{} accepted boxes ({:.3}%), fully read {}/{} ({:.2}%), wrong among fully read {}/{} ({:.3}%)",
        r.misid,
        r.accepted,
        100.0 * r.misid_rate(),
        r.fully_read,
        r.images,
        100.0 * r.full_rate(),
        r.wrong_images,
        r.fully_read,
        100.0 * r.wrong_rate()
    )
}

fn english_line(name: &str, s: &EnStats) -> String {
    format!(
        "{name}: English images {} (grid not found {}), FULLY READ {}, accepted boxes {}/{}",
        s.images, s.grid_not_found, s.fully_read, s.accepted_boxes, s.boxes
    )
}

fn pct_line(name: &str, v: &mut [f32]) -> String {
    v.sort_by(|a, b| a.total_cmp(b));
    format!(
        "{name}: min {:.3}, p1 {:.3}, p5 {:.3}, p25 {:.3}, p50 {:.3}, p95 {:.3}, max {:.3} (n={})",
        percentile(v, 0.0),
        percentile(v, 0.01),
        percentile(v, 0.05),
        percentile(v, 0.25),
        percentile(v, 0.5),
        percentile(v, 0.95),
        percentile(v, 1.0),
        v.len()
    )
}

fn main() {
    let args = parse_args();
    assert!(args.export.is_none() || !args.shuffle, "--export は時系列分割（--shuffle なし）でだけ使える");
    let tl: TagList = serde_json::from_str(&std::fs::read_to_string(&args.tags).expect("tagList.json")).expect("tagList.json の形");
    let tags: Vec<String> = [tl.elite.clone(), tl.job, tl.position, tl.other].concat();
    let tag_idx: HashMap<&str, usize> = tags.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect();
    let elite: Vec<usize> = tl.elite.iter().map(|t| tag_idx[t.as_str()]).collect();

    let mut report = String::new();
    macro_rules! out {
        ($($arg:tt)*) => {{
            let line = format!($($arg)*);
            println!("{line}");
            report.push_str(&line);
            report.push('\n');
        }};
    }

    // ---- 画像の選別と時系列の分割 ----
    let records: Vec<Record> = read_jsonl(&args.root.join("index.jsonl"));
    let detect: HashMap<String, bool> = read_jsonl::<DetectRec>(&args.root.join("detect_report.jsonl")).into_iter().map(|d| (d.file, d.found)).collect();
    let crops: Vec<CropRec> = read_jsonl(&args.root.join("crops/index.jsonl"));
    let mut by_source: HashMap<&str, [Option<&CropRec>; 5]> = HashMap::new();
    for c in &crops {
        if c.pos < 5 {
            by_source.entry(c.source.as_str()).or_insert([None; 5])[c.pos] = Some(c);
        }
    }
    // 英語クライアントの画像（Google に回す対象）。目で確認した手書きの一覧（1 行 1 ファイル、# 始まりはコメント）。
    let english: HashSet<String> = std::fs::read_to_string(args.root.join("english_images.txt"))
        .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect())
        .unwrap_or_default();

    struct Target<'a> {
        rec: &'a Record,
        label: [usize; 5],
        crops: Option<[&'a CropRec; 5]>,
    }
    let mut targets: Vec<Target> = Vec::new();
    let mut skipped_label = 0;
    for r in &records {
        let Some(l) = r.label.as_ref().filter(|l| l.usable && l.is_global) else { continue };
        let idx: Vec<usize> = l.tags.iter().filter_map(|t| tag_idx.get(t.as_str()).copied()).collect();
        if l.tags.len() != 5 || idx.len() != 5 {
            skipped_label += 1;
            continue;
        }
        let cs = by_source.get(r.file.as_str()).and_then(|a| {
            if a.iter().all(|c| c.is_some()) && detect.get(&r.file).copied().unwrap_or(false) {
                Some([a[0].unwrap(), a[1].unwrap(), a[2].unwrap(), a[3].unwrap(), a[4].unwrap()])
            } else {
                None
            }
        });
        targets.push(Target { rec: r, label: [idx[0], idx[1], idx[2], idx[3], idx[4]], crops: cs });
    }
    if args.shuffle {
        targets.sort_by_key(|t| fnv1a(&t.rec.file));
    } else {
        targets.sort_by(|a, b| a.rec.posted_at.cmp(&b.rec.posted_at));
    }
    let n_train = (targets.len() as f64 * TRAIN_RATIO) as usize;
    out!("split: {}", if args.shuffle { "hash order (not chronological)" } else { "chronological (oldest 80% train)" });
    out!("usable JP-label images: {} (label skipped {skipped_label}), train {n_train}, test {}", targets.len(), targets.len() - n_train);
    out!(
        "train {} .. {}, test {} .. {}",
        targets.first().map(|t| t.rec.posted_at.as_str()).unwrap_or(""),
        targets.get(n_train.saturating_sub(1)).map(|t| t.rec.posted_at.as_str()).unwrap_or(""),
        targets.get(n_train).map(|t| t.rec.posted_at.as_str()).unwrap_or(""),
        targets.last().map(|t| t.rec.posted_at.as_str()).unwrap_or("")
    );
    out!("preprocess: {FW}x{FH}, center={}, min_area={MIN_AREA}, shift=±{SHIFT_X},±{SHIFT_Y}", args.center);
    out!("English list: {} files (excluded from fitting; kept in evaluation as must-not-read)", english.len());

    // ---- 前処理 ----
    let root = &args.root;
    let center = args.center;
    let imgs: Vec<ImgData> = par_map(&targets, |t| {
        let boxes = t.crops.map(|cs| {
            cs.iter()
                .map(|c| {
                    let img = image::open(root.join("crops").join(&c.file)).expect("crop が開けない").to_rgb8();
                    let raw = preprocess(&img, center);
                    let norm = normalize(&raw);
                    BoxData { raw, norm, color: c.color.clone(), crop_file: c.file.clone() }
                })
                .collect::<Vec<_>>()
        });
        ImgData { file: t.rec.file.clone(), posted_at: t.rec.posted_at.clone(), english: english.contains(&t.rec.file), label: t.label, boxes }
    });
    let (train, test) = imgs.split_at(n_train);
    let n_en = |s: &[ImgData]| s.iter().filter(|i| i.english).count();
    let found = |s: &[ImgData]| s.iter().filter(|i| i.boxes.is_some()).count();
    out!(
        "grid found: train {}/{}, test {}/{}; English: train {}, test {}",
        found(train),
        train.len(),
        found(test),
        test.len(),
        n_en(train),
        n_en(test)
    );

    let perms = perms5();
    let usable = |i: &&ImgData| i.boxes.is_some() && !i.english;
    let train_set: Vec<&ImgData> = train.iter().collect();
    let test_set: Vec<&ImgData> = test.iter().collect();
    let train_found: Vec<&ImgData> = train_set.iter().copied().filter(usable).collect();
    let test_found: Vec<&ImgData> = test_set.iter().copied().filter(usable).collect();

    // ---- T, M の選び方（結果を見る前に決めた規則）----
    // 学習の古い 80% だけでテンプレートを作り、新しい 20%（検証）で、枠の誤読率 <= 0.5% の範囲で
    // 全部読める画像が最多になる (T, M) を選ぶ。英語の画像は学習に使わないので、学習側にある英語画像も
    // 全部検証に足す（読まれてはいけないものを含めて選ぶ）。テストは 1 回だけ見る。余裕は取らない。
    let n_fit = (train.len() as f64 * TRAIN_RATIO) as usize;
    let (fit_part, val_part) = train.split_at(n_fit);
    let fit_found: Vec<&ImgData> = fit_part.iter().filter(usable).collect();
    let mut val_set: Vec<&ImgData> = val_part.iter().collect();
    val_set.extend(fit_part.iter().filter(|i| i.english));
    let mut val_log = Vec::new();
    let (val_templates, _) = fit(&fit_found, &tags, &perms, &mut val_log);
    let val_model = Model::new(&val_templates);
    let val_res = score_boxes(&val_set, &val_model);
    let val_pair = to_evals(&val_set, &val_res, true);
    let val_plain = to_evals(&val_set, &val_res, false);
    out!(
        "RULE: T, M chosen on validation = newest {} train images + {} English train images (templates from the oldest {} train images, English excluded), max box misid <= {:.1}%, maximize fully read; test looked at once",
        val_part.len(),
        fit_part.iter().filter(|i| i.english).count(),
        fit_part.len(),
        SPEC_VAL_MISID * 100.0
    );

    // ---- 本番用の学習（学習側の全部、英語を除く）----
    let mut log = Vec::new();
    let (templates, assign) = fit(&train_found, &tags, &perms, &mut log);
    for l in &log {
        out!("{l}");
    }
    let model = Model::new(&templates);
    let confusable = {
        let mut n = 0;
        let mut ex = Vec::new();
        for a in 0..tags.len() {
            for b in a + 1..tags.len() {
                if model.gate[a][b] >= PAIR_SIM {
                    n += 1;
                    ex.push(format!("{}/{}={:.2}", tags[a], tags[b], model.gate[a][b]));
                }
            }
        }
        (n, ex)
    };
    out!("confusable tag pairs (template NCC >= {PAIR_SIM}): {} {}", confusable.0, confusable.1.join(" "));

    // ---- 色の検算 ----
    let test_assign = assign_set(&templates, &test_found, &perms);
    for (name, set, asg) in [("train", &train_found, &assign), ("test", &test_found, &test_assign)] {
        let items: Vec<(Vec<usize>, Vec<&str>)> =
            set.iter().zip(asg.iter()).map(|(img, a)| (a.to_vec(), img.boxes.as_ref().unwrap().iter().map(|b| b.color.as_str()).collect())).collect();
        let (e, ey, y) = color_sanity(&items, &elite);
        out!(
            "color sanity ({name}, non-English): elite boxes yellow {ey}/{e} ({:.1}%), yellow boxes elite {ey}/{y} ({:.1}%)",
            100.0 * ey as f64 / e.max(1) as f64,
            100.0 * ey as f64 / y.max(1) as f64
        );
    }

    // ---- 採点 ----
    let tr_res = score_boxes(&train_set, &model);
    let te_res = score_boxes(&test_set, &model);
    let tr_pair = to_evals(&train_set, &tr_res, true);
    let te_pair = to_evals(&test_set, &te_res, true);
    let te_plain = to_evals(&test_set, &te_res, false);

    // ---- T, M の選択（検証側だけ）と評価 ----
    let Some((t, m, va)) = select_tm(&val_pair, SPEC_VAL_MISID) else {
        out!("no (T, M) satisfies validation misidentification <= {:.2}%", SPEC_VAL_MISID * 100.0);
        std::fs::create_dir_all(args.root.join(&args.out_dir)).unwrap();
        std::fs::write(args.root.join(&args.out_dir).join("eval_report.txt"), &report).unwrap();
        return;
    };
    out!("CHOSEN (pair re-scoring on): T={t:.2} M={m:.2}");
    out!("  {}", metrics_line("validation", &va));
    out!("  {}", metrics_line("train (self-fit, optimistic, non-English)", &evaluate_filtered(&tr_pair, t, m, |e| !e.english)));
    out!("  {}", metrics_line("TEST including English", &evaluate(&te_pair, t, m)));
    out!("  {}", metrics_line("TEST excluding English", &evaluate_filtered(&te_pair, t, m, |e| !e.english)));
    out!("  {}", english_line("English safety, train part (never fitted)", &english_stats(&tr_pair, t, m)));
    out!("  {}", english_line("English safety, test part", &english_stats(&te_pair, t, m)));
    if let Some((tp, mp, vap)) = select_tm(&val_plain, SPEC_VAL_MISID) {
        out!("for comparison, same rule WITHOUT pair re-scoring: T={tp:.2} M={mp:.2}");
        out!("  {}", metrics_line("validation", &vap));
        out!("  {}", metrics_line("TEST including English", &evaluate(&te_plain, tp, mp)));
        out!("  {}", metrics_line("TEST excluding English", &evaluate_filtered(&te_plain, tp, mp, |e| !e.english)));
        out!("  {}", english_line("English safety, test part", &english_stats(&te_plain, tp, mp)));
    }
    for (title, v, te) in [("pair re-scoring ON", &val_pair, &te_pair), ("pair re-scoring OFF", &val_plain, &te_plain)] {
        out!("(T, M) alternatives, {title} (validation | test excl. English | test incl. English):");
        out!("  {:>5} {:>5} | {:>8} {:>8} | {:>8} {:>8} | {:>8} {:>8}", "T", "M", "va_misid", "va_full", "te_misid", "te_full", "all_misid", "all_full");
        for (ta, ma) in [(0.6, 0.0), (0.7, 0.0), (0.8, 0.0), (0.6, 0.05), (0.7, 0.05), (0.7, 0.1), (0.7, 0.2), (0.7, 0.3), (0.7, 0.5), (t, m)] {
            let (a, b, c) = (evaluate(v, ta, ma), evaluate_filtered(te, ta, ma, |e| !e.english), evaluate(te, ta, ma));
            out!(
                "  {ta:>5.2} {ma:>5.2} | {:>7.3}% {:>7.2}% | {:>7.3}% {:>7.2}% | {:>7.3}% {:>7.2}%",
                100.0 * a.misid_rate(),
                100.0 * a.full_rate(),
                100.0 * b.misid_rate(),
                100.0 * b.full_rate(),
                100.0 * c.misid_rate(),
                100.0 * c.full_rate()
            );
        }
    }

    // ---- 本番と同じ経路（量子化した PNG を TagOcr::load、元のスクショから analyze）での評価 ----
    // 同じ T, M、同じテスト画像。切り抜きの PNG を経由する上の評価との差は、量子化と、切り抜きから検出までの経路だけ。
    {
        let qdir = args.root.join(&args.out_dir).join("quantized_train");
        store::write(&qdir, &tags, &templates, t, m, args.center, "train-only fit, for the quantized evaluation").expect("量子化テンプレートの書き出し");
        let ocr = TagOcr::load(&qdir).expect("量子化テンプレートの読み込み");
        assert_eq!(ocr.thresholds(), (t, m), "meta.json の T, M が往復で変わった");
        let to_top = |b: &BoxScore| TopScore {
            best_tag: tag_idx[b.best_tag.as_str()],
            best: b.best,
            second_tag: tag_idx[b.second_tag.as_str()],
            second: b.second,
            margin: b.margin,
        };
        let q_evals: Vec<ImgEval> = par_map(&test_set, |img| {
            let boxes = image::open(root.join(&img.file)).ok().and_then(|im| ocr.analyze(&im)).map(|v| {
                let v: Vec<TopScore> = v.iter().map(to_top).collect();
                [v[0], v[1], v[2], v[3], v[4]]
            });
            ImgEval { label: img.label, english: img.english, boxes }
        });
        out!("QUANTIZED templates (PNG via TagOcr::load + analyze on the original screenshots, same T={t:.2} M={m:.2}):");
        out!("  {}", metrics_line("TEST including English", &evaluate(&q_evals, t, m)));
        out!("  {}", metrics_line("TEST excluding English", &evaluate_filtered(&q_evals, t, m, |e| !e.english)));
        out!("  {}", english_line("English safety, test part", &english_stats(&q_evals, t, m)));
    }

    // ---- 類似度の分布（テスト、英語を除く。正解は EM の割り当て）----
    let mut correct = Vec::new();
    let mut wrong = Vec::new();
    let (mut before, mut after) = (Vec::new(), Vec::new());
    let (mut before_c, mut after_c) = (Vec::new(), Vec::new());
    let mut asg_iter = test_assign.iter();
    for (img, r) in test_set.iter().zip(&te_res) {
        let Some(r) = r.as_ref().filter(|_| usable(img)) else { continue };
        let asg = asg_iter.next().unwrap();
        for (b, &truth) in r.iter().zip(asg) {
            let mb = truth_margin(b, truth, false);
            let ma = truth_margin(b, truth, true);
            correct.push(b.scores[truth]);
            wrong.push(b.scores[truth] - mb);
            before.push(mb);
            after.push(ma);
            if b.pair.is_some_and(|(a, c, _, _)| truth == a || truth == c) {
                before_c.push(mb);
                after_c.push(ma);
            }
        }
    }
    out!("test similarity (non-English, EM-assigned truth):");
    out!("  {}", pct_line("correct template   ", &mut correct));
    out!("  {}", pct_line("best wrong template", &mut wrong));
    out!("  {}", pct_line("margin before (plain NCC difference)       ", &mut before.clone()));
    out!("  {}", pct_line("margin after  (pair re-scored where gated) ", &mut after.clone()));
    out!("  boxes with a wrong template beating/tying the truth: before {}, after {} of {}", before.iter().filter(|&&x| x <= 0.0).count(), after.iter().filter(|&&x| x <= 0.0).count(), before.len());
    out!("  confusable-pair boxes only ({} boxes):", before_c.len());
    out!("  {}", pct_line("margin before", &mut before_c.clone()));
    out!("  {}", pct_line("margin after ", &mut after_c.clone()));
    out!(
        "  confusable-pair boxes with margin <= 0: before {}, after {}",
        before_c.iter().filter(|&&x| x <= 0.0).count(),
        after_c.iter().filter(|&&x| x <= 0.0).count()
    );

    // ---- 出力 ----
    let tdir = args.root.join(&args.out_dir);
    write_templates(
        &tdir,
        &tags,
        &templates,
        json!({
            "threshold_t": t, "threshold_m": m,
            "threshold_note": "chosen on validation (newest 20% of train + English train images; templates from older 80%) with box misid <= 0.5%",
            "trained_on": "oldest 80% of usable non-English images",
        }),
        args.center,
    );

    // 枠ごとの最良タグ（英語画像を探すために、全画像ぶん）
    let mut dump = String::new();
    for (split, set, res) in [("train", &train_set, &tr_res), ("test", &test_set, &te_res)] {
        for (img, r) in set.iter().zip(res) {
            let Some(r) = r else { continue };
            let boxes: Vec<_> = r
                .iter()
                .enumerate()
                .map(|(i, b)| json!({"pos": i, "tag": tags[b.plain.best_tag], "proto": b.best_proto, "proto_count": templates[b.plain.best_tag][b.best_proto].count, "score": b.plain.best}))
                .collect();
            dump.push_str(&json!({"file": img.file, "split": split, "english": img.english, "label": img.label.iter().map(|&t| tags[t].as_str()).collect::<Vec<_>>(), "boxes": boxes}).to_string());
            dump.push('\n');
        }
    }
    std::fs::write(tdir.join("box_dump.jsonl"), dump).expect("box_dump.jsonl");

    // 誤読と読み切れなかった画像（テスト）。英語は読まれてしまったものだけ載せる。
    let mut mistakes = String::new();
    let (mut n_mis, mut n_unread, mut n_en_read) = (0, 0, 0);
    let mut asg_iter = test_assign.iter();
    for ((img, ev), r) in test_set.iter().zip(&te_pair).zip(&te_res) {
        let truth = if usable(img) { Some(*asg_iter.next().unwrap()) } else { None };
        let (_, misid, full, _) = judge(ev, t, m);
        if img.english && !full {
            continue;
        }
        if full && misid == 0 && !img.english {
            continue;
        }
        let reason = if img.english {
            n_en_read += 1;
            "ENGLISH FULLY READ"
        } else if ev.boxes.is_none() {
            n_unread += 1;
            "grid not found"
        } else if misid > 0 {
            n_mis += 1;
            "misidentified"
        } else {
            n_unread += 1;
            "not fully read"
        };
        let boxes: Vec<_> = match (&ev.boxes, &img.boxes, r) {
            (Some(bs), Some(bd), Some(r)) => (0..5)
                .map(|i| {
                    json!({
                        "pos": i, "crop": bd[i].crop_file, "color": bd[i].color,
                        "best_tag": tags[bs[i].best_tag], "best": bs[i].best,
                        "second_tag": tags[bs[i].second_tag], "second": bs[i].second, "margin": bs[i].margin,
                        "pair_rescored": r[i].pair.is_some(),
                        "accepted": accepts(&bs[i], t, m), "em_tag": truth.map(|tr| tags[tr[i]].as_str()),
                    })
                })
                .collect(),
            _ => vec![],
        };
        let rec = json!({
            "reason": reason, "file": img.file, "posted_at": img.posted_at,
            "label": img.label.iter().map(|&t| tags[t].as_str()).collect::<Vec<_>>(),
            "boxes": boxes,
        });
        mistakes.push_str(&rec.to_string());
        mistakes.push('\n');
    }
    std::fs::write(tdir.join("mistakes.jsonl"), mistakes).expect("mistakes.jsonl");
    out!("mistakes.jsonl (test, non-English): misidentified {n_mis}, not fully read / grid not found {n_unread}; English fully read {n_en_read}");

    // ---- 本番用: 英語を除く全データ（学習+テスト）で学習し直す ----
    if !args.shuffle {
        let all_found: Vec<&ImgData> = imgs.iter().filter(|i| i.boxes.is_some() && !i.english).collect();
        let mut all_log = Vec::new();
        let (all_templates, _) = fit(&all_found, &tags, &perms, &mut all_log);
        out!("PRODUCTION fit on all {} non-English found images (not evaluable: includes test):", all_found.len());
        for l in all_log.iter().filter(|l| l.contains("prototypes per tag") || l.contains("differs")) {
            out!("{l}");
        }
        let all_model = Model::new(&all_templates);
        let all_set: Vec<&ImgData> = imgs.iter().collect();
        let all_en = to_evals(&all_set, &score_boxes(&all_set, &all_model), true);
        out!("  {}", english_line("English safety with production templates, all splits", &english_stats(&all_en, t, m)));
        if let Some(dir) = &args.export {
            store::write(
                dir,
                &tags,
                &all_templates,
                t,
                m,
                args.center,
                "T, M chosen by the validation rule on the train-only fit (see fit_tag_templates); templates refit on all usable non-English images",
            )
            .expect("--export の書き出し");
            // 書き出したものを本番と同じ読み方で読み直し、英語の画像が読み切られないことを確かめる
            let ocr = TagOcr::load(dir).expect("--export したテンプレートの読み込み");
            let en: Vec<&ImgData> = imgs.iter().filter(|i| i.english).collect();
            let read = par_map(&en, |img| image::open(root.join(&img.file)).ok().is_some_and(|im| matches!(ocr.read(&im), astesiabot_rust::engine::recruit::tag_ocr::TagOcrResult::Read { .. }))).into_iter().filter(|&r| r).count();
            out!("EXPORTED production templates to {} (T={t:.2} M={m:.2}); English images fully read through TagOcr::read: {read}/{}", dir.display(), en.len());
        }
        write_templates(
            &args.root.join("templates_all"),
            &tags,
            &all_templates,
            json!({
                "threshold_t": t, "threshold_m": m,
                "threshold_note": "chosen by the validation rule on the train-only fit (see templates/eval_report.txt); templates here are refit on all non-English usable images",
                "trained_on": "all usable non-English images with a found grid",
            }),
            args.center,
        );
    }
    std::fs::write(tdir.join("eval_report.txt"), &report).expect("eval_report.txt");
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

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
    fn perm_assignment_on_toy() {
        // 枠 0 は label 2、枠 1 は label 0、…と入れ替わっている
        let truth = [2usize, 0, 4, 1, 3];
        let mut s = [[0.1f32; 5]; 5];
        for (b, &l) in truth.iter().enumerate() {
            s[b][l] = 0.9;
        }
        // 1 つだけ紛らわしい（枠 0 が label 1 にもそこそこ似る）が、合計では負けない
        s[0][1] = 0.8;
        assert_eq!(best_perm(&s, &perms5()), truth);
        assert_eq!(perms5().len(), 120);
    }

    #[test]
    fn trimmed_template_resists_outliers() {
        let good = vec![1.0f32; NPIX];
        let bad = vec![0.0f32; NPIX];
        let mut feats: Vec<&[f32]> = vec![&good; 18];
        feats.extend([&bad[..], &bad[..]]);
        let t = build_template(&feats);
        assert!(t.iter().all(|&v| (v - 1.0).abs() < 1e-6));
    }

    fn bs(best_tag: usize, best: f32, second: f32) -> TopScore {
        TopScore { best_tag, best, second_tag: 9, second, margin: best - second }
    }

    #[test]
    fn judge_counts_misid_and_duplicates() {
        let label = [0, 1, 2, 3, 4];
        let ok = ImgEval { english: false, label, boxes: Some([bs(0, 0.9, 0.2), bs(1, 0.9, 0.2), bs(2, 0.9, 0.2), bs(3, 0.9, 0.2), bs(4, 0.9, 0.2)]) };
        assert_eq!(judge(&ok, 0.8, 0.1), (5, 0, true, false));
        // 5 枠目だけ自信がない → 読み切れない
        let weak = ImgEval { english: false, label, boxes: Some([bs(0, 0.9, 0.2), bs(1, 0.9, 0.2), bs(2, 0.9, 0.2), bs(3, 0.9, 0.2), bs(4, 0.5, 0.2)]) };
        assert_eq!(judge(&weak, 0.8, 0.1), (4, 0, false, false));
        // 差が小さい → 受理しない
        let close = ImgEval { english: false, label, boxes: Some([bs(0, 0.9, 0.85), bs(1, 0.9, 0.2), bs(2, 0.9, 0.2), bs(3, 0.9, 0.2), bs(4, 0.9, 0.2)]) };
        assert!(!judge(&close, 0.8, 0.1).2);
        // ラベルに無いタグを受理 → 誤読
        let wrong = ImgEval { english: false, label, boxes: Some([bs(7, 0.9, 0.2), bs(1, 0.9, 0.2), bs(2, 0.9, 0.2), bs(3, 0.9, 0.2), bs(4, 0.9, 0.2)]) };
        assert_eq!(judge(&wrong, 0.8, 0.1), (5, 1, true, true));
        // 同じタグが 2 枠 → 重複（読み切りにしない・誤読 1）
        let dup = ImgEval { english: false, label, boxes: Some([bs(0, 0.9, 0.2), bs(0, 0.9, 0.2), bs(2, 0.9, 0.2), bs(3, 0.9, 0.2), bs(4, 0.9, 0.2)]) };
        assert_eq!(judge(&dup, 0.8, 0.1), (5, 1, false, false));
        // 枠が見つからない画像
        assert_eq!(judge(&ImgEval { english: false, label, boxes: None }, 0.8, 0.1), (0, 0, false, false));
    }

    #[test]
    fn select_tm_prefers_most_reads_within_misid_limit() {
        let label = [0, 1, 2, 3, 4];
        let good = |s: f32| ImgEval { english: false, label, boxes: Some([bs(0, s, 0.1), bs(1, s, 0.1), bs(2, s, 0.1), bs(3, s, 0.1), bs(4, s, 0.1)]) };
        let bad = ImgEval { english: false, label, boxes: Some([bs(7, 0.7, 0.1), bs(1, 0.95, 0.1), bs(2, 0.95, 0.1), bs(3, 0.95, 0.1), bs(4, 0.95, 0.1)]) };
        let mut imgs: Vec<ImgEval> = (0..50).map(|_| good(0.95)).collect();
        imgs.extend((0..50).map(|_| good(0.6)));
        imgs.push(bad);
        // T を 0.7 以下に下げると誤読が入り、率 0.2% > 0.1% になる。T = 0.71 以上なら誤読 0 で 50 枚読める
        let (t, _m, r) = select_tm(&imgs, 0.001).unwrap();
        assert!(t > 0.7 && r.misid == 0 && r.fully_read == 50, "{t} {r:?}");
        // 緩い上限なら 0.6 の 50 枚も、誤読の 1 枚も読む
        let (t2, _, r2) = select_tm(&imgs, 0.05).unwrap();
        assert!(t2 <= 0.6 && r2.fully_read == 101, "{t2} {r2:?}");
    }

    #[test]
    fn percentile_basics() {
        let v = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&v, 0.5), 3.0);
        assert_eq!(percentile(&v, 1.0), 5.0);
    }

    #[test]
    fn color_sanity_counts() {
        let items = vec![(vec![0, 5, 6, 7, 8], vec!["yellow", "dark", "dark", "yellow", "dark"])];
        // タグ 0・1 がエリート。黄色の枠は 0 番目と 3 番目
        assert_eq!(color_sanity(&items, &[0, 1]), (1, 1, 2));
    }

    #[test]
    fn cluster_splits_variants_and_drops_junk() {
        let a = preprocess(&synth_box([48, 48, 48], [255, 255, 255], &BARS_A, 200, 64), true);
        let b = preprocess(&synth_box([48, 48, 48], [255, 255, 255], &BARS_B, 200, 64), true);
        let junk = preprocess(&synth_box([48, 48, 48], [255, 255, 255], &[(10, 40)], 200, 64), true);
        let mut raws: Vec<Vec<f32>> = vec![a.clone(); 20];
        raws.extend(vec![b.clone(); 12]);
        raws.push(junk); // 1 個だけの外れ値は代表にならない
        let norms: Vec<Vec<f32>> = raws.iter().map(|r| normalize(r)).collect();
        let samples: Vec<(&[f32], &[f32])> = raws.iter().zip(&norms).map(|(r, n)| (r.as_slice(), n.as_slice())).collect();
        let protos = cluster_tag(&samples);
        assert_eq!(protos.len(), 2, "{:?}", protos.iter().map(|p| p.count).collect::<Vec<_>>());
        let mut counts: Vec<usize> = protos.iter().map(|p| p.count).collect();
        counts.sort();
        assert_eq!(counts, vec![13, 20]); // 外れ値は近い方の代表に入る
        // どちらの見た目も、どれかの代表と一致する
        assert!(tag_sim(&normalize(&a), &protos) > 0.99);
        assert!(tag_sim(&normalize(&b), &protos) > 0.99);
    }

    #[test]
    fn cluster_keeps_single_prototype_for_uniform_samples() {
        let a = preprocess(&synth_box([48, 48, 48], [255, 255, 255], &BARS_A, 200, 64), true);
        let norm = normalize(&a);
        let samples: Vec<(&[f32], &[f32])> = (0..30).map(|_| (a.as_slice(), norm.as_slice())).collect();
        assert_eq!(cluster_tag(&samples).len(), 1);
    }

    #[test]
    fn hash_split_is_deterministic() {
        assert_eq!(fnv1a("images/1_2.jpg"), fnv1a("images/1_2.jpg"));
        assert_ne!(fnv1a("images/1_2.jpg"), fnv1a("images/1_3.jpg"));
    }


    #[test]
    fn truth_margin_uses_pair_only_when_truth_is_in_it() {
        let r = BoxResult {
            scores: vec![0.9, 0.88, 0.2],
            plain: TopScore { best_tag: 0, best: 0.9, second_tag: 1, second: 0.88, margin: 0.02 },
            refined: TopScore { best_tag: 0, best: 0.9, second_tag: 1, second: 0.88, margin: 0.6 },
            best_proto: 0,
            pair: Some((0, 1, 0.8, 0.2)),
        };
        assert!((truth_margin(&r, 0, false) - 0.02).abs() < 1e-6);
        assert!((truth_margin(&r, 0, true) - 0.6).abs() < 1e-6);
        assert!((truth_margin(&r, 1, true) + 0.6).abs() < 1e-6);
        assert!((truth_margin(&r, 2, true) - (0.2 - 0.9)).abs() < 1e-6); // 組の外は全体の相関
    }

    #[test]
    fn english_stats_counts_reads_and_accepted_boxes() {
        let good = |s: f32| [bs(0, s, 0.1), bs(1, s, 0.1), bs(2, s, 0.1), bs(3, s, 0.1), bs(4, s, 0.1)];
        let label = [0, 1, 2, 3, 4];
        let imgs = vec![
            ImgEval { english: true, label, boxes: Some(good(0.9)) }, // 読まれてしまった英語
            ImgEval { english: true, label, boxes: Some([bs(0, 0.9, 0.1), bs(1, 0.3, 0.1), bs(2, 0.3, 0.1), bs(3, 0.3, 0.1), bs(4, 0.3, 0.1)]) },
            ImgEval { english: true, label, boxes: None },
            ImgEval { english: false, label, boxes: Some(good(0.9)) },
        ];
        let s = english_stats(&imgs, 0.6, 0.0);
        assert_eq!((s.images, s.grid_not_found, s.fully_read, s.boxes, s.accepted_boxes), (3, 1, 1, 10, 6));
        let m = evaluate_filtered(&imgs, 0.6, 0.0, |e| !e.english);
        assert_eq!((m.images, m.fully_read), (1, 1));
    }
}
