//! 公開求人のローカル OCR（タグ枠のテンプレート照合）の bot 側。
//!
//! 計算は `engine::recruit::tag_ocr`。ここは、添付のダウンロード・安全な読み込み・モード（LOCAL_OCR_MODE）・
//! shadow モードの比較と記録（ログ・オーナーへの DM・日次まとめ）を担う。DM などの文面はここで組み立てる。
//!
//! - off（既定）: 何もしない / shadow: ローカルも読むが返信は常に Google の結果 /
//!   on: ローカルが読み切れたら Google を呼ばない。
//! - ローカルの失敗はどれも「読めなかった」（Unsure）扱いにして Google に回す。返信には影響させない。
//! - 日本語版の読めない画像の監視: ゲームの画面が変わるとローカルが日本語の画面を読めなくなり Google に落ちる回数が増えるので、
//!   「ローカルが OCR の理由（グリッド無し・低スコア・重複）で読めなかったが、Google のテキストは日本語で 5 タグ取れた」画像を数える
//!   （ダウンロード・デコードなどの失敗は数えない）。連続 3 枚（同じ画像は 1 回だけ数える。画像のハッシュで判定）で DM し、
//!   ローカルが 1 枚読めたら連続を 0 に戻して再び DM できるようにする。読めた枚数とあわせて日次まとめに出す。
//!   shadow でも on でも動く（off は何もしない）。状態はメモリだけで、再起動で消える。
//! - ログは全部 `[local_ocr]` で始める（Heroku のログは約 1500 行しか残らないので、shadow の結果は DM でも残す）。

use crate::engine::recruit::tag_ocr::{BoxScore, TagOcr, TagOcrResult, UnsureReason};
use poise::serenity_prelude as serenity;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;

/// この大きさを超える添付はローカルで読まない（Google に回す）。
const MAX_BYTES: u64 = 10 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);
/// 画像の幅・高さ・画素数・デコード時のメモリの上限。悪意ある画像でメモリを溢れさせないため
/// （Heroku は 512MB。RGB8 に直すのでデコード後の 2 倍ほど要る）。
const MAX_DIMENSION: u32 = 8000;
const MAX_PIXELS: u64 = 20_000_000;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;
/// 同時にローカルで読む枚数（1 枚が CPU を数百 ms 使うので、連投されても詰まらせない）。
const MAX_CONCURRENT: usize = 2;
/// Discord のメッセージの文字数の上限。
const DISCORD_LIMIT: usize = 2000;
/// 日本語版の読めない画像がこの枚数続いたら DM する。
const MISS_STREAK_ALERT: usize = 3;
/// 重複判定のために覚えておく、日本語版の読めなかった画像のハッシュの数。
const MISS_RECENT: usize = 20;

// ---- モード ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalOcrMode {
    Off,
    Shadow,
    On,
}

impl LocalOcrMode {
    pub fn as_str(self) -> &'static str {
        match self {
            LocalOcrMode::Off => "off",
            LocalOcrMode::Shadow => "shadow",
            LocalOcrMode::On => "on",
        }
    }

    /// 環境変数の値から。未設定は off、未知の値も off（2 つ目の戻りは「未知の値だった」）。
    pub fn parse(raw: Option<&str>) -> (Self, bool) {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") | Some("off") => (LocalOcrMode::Off, false),
            Some("shadow") => (LocalOcrMode::Shadow, false),
            Some("on") => (LocalOcrMode::On, false),
            Some(_) => (LocalOcrMode::Off, true),
        }
    }

    pub fn from_env() -> Self {
        let raw = std::env::var("LOCAL_OCR_MODE").ok();
        let (mode, unknown) = Self::parse(raw.as_deref());
        if unknown {
            eprintln!(
                "[local_ocr] LOCAL_OCR_MODE の値が不正なので off にします（off / shadow / on）"
            );
        }
        mode
    }
}

// ---- 1 枚の結果 ----

/// ローカルで読めなかった理由（OCR 自身の理由と、その前後の失敗）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalUnsure {
    Ocr(UnsureReason),
    TooLarge,
    Download,
    Decode,
    Internal,
}

impl LocalUnsure {
    pub fn as_str(self) -> &'static str {
        match self {
            LocalUnsure::Ocr(r) => r.as_str(),
            LocalUnsure::TooLarge => "too_large",
            LocalUnsure::Download => "download_failed",
            LocalUnsure::Decode => "decode_failed",
            LocalUnsure::Internal => "internal_error",
        }
    }
}

/// `read_attachment` の結果。ダウンロードできたときだけ、バイト列のハッシュ（重複画像の判定用）が付く。
#[derive(Debug, Clone, PartialEq)]
pub struct LocalRead {
    pub outcome: LocalOutcome,
    pub digest: Option<u64>,
}

impl LocalRead {
    fn without_digest(outcome: LocalOutcome) -> Self {
        Self {
            outcome,
            digest: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocalOutcome {
    Read {
        tags: Vec<String>,
        boxes: Vec<BoxScore>,
    },
    Unsure {
        reason: LocalUnsure,
        boxes: Vec<BoxScore>,
    },
}

impl LocalOutcome {
    fn unsure(reason: LocalUnsure) -> Self {
        LocalOutcome::Unsure {
            reason,
            boxes: Vec::new(),
        }
    }

    pub fn boxes(&self) -> &[BoxScore] {
        match self {
            LocalOutcome::Read { boxes, .. } | LocalOutcome::Unsure { boxes, .. } => boxes,
        }
    }

    /// 5 枠の 1 位の類似度のうち最小のもの（ログ用）。枠が無ければ None。
    pub fn min_best(&self) -> Option<f32> {
        self.boxes().iter().map(|b| b.best).reduce(f32::min)
    }
}

// ---- shadow の比較・集計 ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comparison {
    Agree,
    Disagree,
    /// ローカルは読めたが、Google の結果からはタグが 1 つも取れなかった
    GoogleNoTags,
    LocalUnsure(LocalUnsure),
}

/// ローカルの結果と、Google の OCR テキストから取れたタグ集合の比較。
pub fn compare(local: &LocalOutcome, google_tags: &HashSet<String>) -> Comparison {
    match local {
        LocalOutcome::Unsure { reason, .. } => Comparison::LocalUnsure(*reason),
        LocalOutcome::Read { tags, .. } => {
            if google_tags.is_empty() {
                Comparison::GoogleNoTags
            } else if tags.iter().cloned().collect::<HashSet<_>>() == *google_tags {
                Comparison::Agree
            } else {
                Comparison::Disagree
            }
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShadowStats {
    pub agree: u64,
    pub disagree: u64,
    pub google_no_tags: u64,
    pub unsure: BTreeMap<&'static str, u64>,
    /// 日本語版の画像でローカルが読めた枚数
    pub jp_read: u64,
    /// 日本語版の画像でローカルが（OCR の理由で）読めなかった枚数
    pub jp_miss: u64,
}

impl ShadowStats {
    pub fn record(&mut self, c: &Comparison) {
        match c {
            Comparison::Agree => self.agree += 1,
            Comparison::Disagree => self.disagree += 1,
            Comparison::GoogleNoTags => self.google_no_tags += 1,
            Comparison::LocalUnsure(r) => *self.unsure.entry(r.as_str()).or_default() += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.agree + self.disagree + self.google_no_tags + self.unsure.values().sum::<u64>()
    }

    /// まとめに出すものが何も無い（shadow の比較も日本語版の集計も 0）。
    pub fn is_empty(&self) -> bool {
        self.total() == 0 && self.jp_read + self.jp_miss == 0
    }
}

// ---- 日本語版の読めない画像の監視 ----

/// `MissWatch::miss` の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissVerdict {
    /// 直近に同じ画像が来ていたので、何も数えない
    Duplicate,
    /// 数えた。`alert` が Some なら DM する（連続した画像へのリンク）
    Counted {
        streak: usize,
        alert: Option<Vec<String>>,
    },
}

#[derive(Debug)]
pub struct MissWatch {
    recent: VecDeque<u64>,
    streak: usize,
    armed: bool,
    links: Vec<String>,
}

/// 起動直後は DM を出せる状態（armed）で始める。
impl Default for MissWatch {
    fn default() -> Self {
        Self {
            recent: VecDeque::new(),
            streak: 0,
            armed: true,
            links: Vec::new(),
        }
    }
}

impl MissWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// ローカルが 1 枚読めた。連続を 0 に戻し、DM を再び出せるようにする。
    pub fn read(&mut self) {
        self.streak = 0;
        self.armed = true;
        self.links.clear();
    }

    /// 日本語版の画像をローカルが読めなかった。`digest` が直近の読めなかった画像と同じなら無視する。
    pub fn miss(&mut self, digest: Option<u64>, link: String) -> MissVerdict {
        if let Some(d) = digest {
            if self.recent.contains(&d) {
                return MissVerdict::Duplicate;
            }
            self.recent.push_back(d);
            if self.recent.len() > MISS_RECENT {
                self.recent.pop_front();
            }
        }
        self.streak += 1;
        if self.links.len() < MISS_STREAK_ALERT {
            self.links.push(link);
        }
        let alert = if self.streak >= MISS_STREAK_ALERT && self.armed {
            self.armed = false;
            Some(self.links.clone())
        } else {
            None
        };
        MissVerdict::Counted {
            streak: self.streak,
            alert,
        }
    }
}

// ---- 文面（純粋関数） ----

fn sorted(tags: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = tags.into_iter().collect();
    v.sort();
    v
}

/// 文字数（バイトでなく）で切り詰める。
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// 元メッセージへのリンク。
pub fn jump_link(
    guild: Option<serenity::GuildId>,
    channel: serenity::ChannelId,
    message: serenity::MessageId,
) -> String {
    let g = guild.map_or("@me".to_string(), |g| g.to_string());
    format!("https://discord.com/channels/{g}/{channel}/{message}")
}

/// shadow で local と Google のタグ集合が食い違ったときの DM の本文。画像・投稿者の情報は入れない。
pub fn disagreement_message(
    local_tags: &[String],
    google_tags: &HashSet<String>,
    boxes: &[BoxScore],
    link: &str,
) -> String {
    let mut s = String::from("[local_ocr] shadow: ローカルと Google のタグが食い違いました\n");
    s += &format!(
        "local : {}\n",
        sorted(local_tags.iter().cloned()).join("、")
    );
    s += &format!(
        "google: {}\n",
        sorted(google_tags.iter().cloned()).join("、")
    );
    for (i, b) in boxes.iter().enumerate() {
        s += &format!(
            "枠{}: {} {:.2} / {} {:.2}\n",
            i + 1,
            b.best_tag,
            b.best,
            b.second_tag,
            b.second
        );
    }
    s += link;
    truncate_chars(&s, DISCORD_LIMIT)
}

/// 日本語版の画像が連続で読めなかったときの DM の本文。リンクは連続した画像のもの。
pub fn miss_alert_message(streak: usize, links: &[String]) -> String {
    let mut s = format!("[local_ocr] 日本語版の画像をローカルで読めないのが {streak} 枚続きました。画面が変わったかもしれません。");
    for l in links {
        s += &format!("\n{l}");
    }
    truncate_chars(&s, DISCORD_LIMIT)
}

/// 日本語版を読めなかったときのログ行（URL は入れない）。
pub fn jp_miss_log_line(streak: usize, reason: LocalUnsure) -> String {
    format!(
        "[local_ocr] jp_miss streak={streak} reason={}",
        reason.as_str()
    )
}

/// 日次まとめの 1 行。shadow は比較の集計に日本語版の行を足し、on は日本語版だけ。
pub fn summary_message(stats: &ShadowStats, mode: LocalOcrMode) -> String {
    let jp = format!(
        "日本語版 読めた {}/{}",
        stats.jp_read,
        stats.jp_read + stats.jp_miss
    );
    if mode != LocalOcrMode::Shadow {
        return truncate_chars(
            &format!("[local_ocr] 直近の集計（{}）: {jp}", mode.as_str()),
            DISCORD_LIMIT,
        );
    }
    let unsure_total: u64 = stats.unsure.values().sum();
    let mut s = format!(
        "[local_ocr] 直近の shadow 集計: {} 枚 / 一致 {} / 不一致 {} / Googleがタグ無し {} / ローカル不確か {}",
        stats.total(),
        stats.agree,
        stats.disagree,
        stats.google_no_tags,
        unsure_total
    );
    if unsure_total > 0 {
        let detail: Vec<String> = stats
            .unsure
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        s += &format!("（{}）", detail.join(", "));
    }
    s += &format!(" / {jp}");
    truncate_chars(&s, DISCORD_LIMIT)
}

fn fmt_min(min_best: Option<f32>) -> String {
    min_best.map_or("-".to_string(), |v| format!("{v:.3}"))
}

/// shadow の 1 枚ぶんのログ行。
pub fn shadow_log_line(
    c: &Comparison,
    local_tags: Option<&[String]>,
    google_tags: &HashSet<String>,
    min_best: Option<f32>,
) -> String {
    let min = fmt_min(min_best);
    match c {
        Comparison::Agree => format!("[local_ocr] shadow agree min_best={min}"),
        Comparison::Disagree => format!(
            "[local_ocr] shadow DISAGREE min_best={min} local={:?} google={:?}",
            sorted(local_tags.unwrap_or_default().iter().cloned()),
            sorted(google_tags.iter().cloned())
        ),
        Comparison::GoogleNoTags => {
            format!("[local_ocr] shadow local_read_google_no_tags min_best={min}")
        }
        Comparison::LocalUnsure(r) => format!(
            "[local_ocr] shadow local_unsure({}) min_best={min}",
            r.as_str()
        ),
    }
}

/// on モードの 1 枚ぶんのログ行（どちらの経路を取ったか）。
pub fn on_log_line(local: &LocalOutcome) -> String {
    let min = fmt_min(local.min_best());
    match local {
        LocalOutcome::Read { .. } => {
            format!("[local_ocr] on read min_best={min} -> local (google skipped)")
        }
        LocalOutcome::Unsure { reason, .. } => format!(
            "[local_ocr] on unsure({}) min_best={min} -> google",
            reason.as_str()
        ),
    }
}

// ---- 状態 ----

pub struct LocalOcr {
    pub mode: LocalOcrMode,
    tag_ocr: Arc<TagOcr>,
    client: reqwest::Client,
    permits: Arc<Semaphore>,
    /// shadow の不一致・日次まとめを DM する相手（LOCAL_OCR_REPORT_USER_ID。無ければログだけ）
    pub report_user: Option<serenity::UserId>,
    stats: Mutex<ShadowStats>,
    watch: Mutex<MissWatch>,
}

impl LocalOcr {
    /// 起動時に作る。テンプレートが無い・壊れていれば panic（コミット済みの静的データ）。
    pub fn from_env() -> Self {
        let mode = LocalOcrMode::from_env();
        let report_user = crate::bot::utils::optional_user_id_env("LOCAL_OCR_REPORT_USER_ID");
        let tag_ocr = TagOcr::load(crate::engine::recruit::tag_ocr::TEMPLATE_DIR)
            .expect("タグ枠のテンプレートのロードに失敗");
        println!(
            "[local_ocr] mode={} report_dm={}",
            mode.as_str(),
            report_user.is_some()
        );
        Self::new(mode, tag_ocr, report_user)
    }

    pub fn new(mode: LocalOcrMode, tag_ocr: TagOcr, report_user: Option<serenity::UserId>) -> Self {
        Self {
            mode,
            tag_ocr: Arc::new(tag_ocr),
            client: reqwest::Client::builder()
                .timeout(DOWNLOAD_TIMEOUT)
                .build()
                .expect("reqwest client"),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            report_user,
            stats: Mutex::new(ShadowStats::default()),
            watch: Mutex::new(MissWatch::new()),
        }
    }

    /// 添付 1 枚をローカルで読む。失敗はすべて Unsure（呼び出し側は Google に回す）。
    pub async fn read_attachment(&self, url: &str, size: u32) -> LocalRead {
        if size as u64 > MAX_BYTES {
            return LocalRead::without_digest(LocalOutcome::unsure(LocalUnsure::TooLarge));
        }
        let Ok(_permit) = self.permits.clone().acquire_owned().await else {
            return LocalRead::without_digest(LocalOutcome::unsure(LocalUnsure::Internal));
        };
        let bytes = match self.download(url).await {
            Ok(b) => b,
            Err(reason) => return LocalRead::without_digest(LocalOutcome::unsure(reason)),
        };
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        let digest = Some(hasher.finish());
        let ocr = self.tag_ocr.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let img = decode_limited(&bytes)?;
            Ok::<_, LocalUnsure>(ocr.read(&img))
        })
        .await;
        let outcome = match joined {
            Ok(Ok(TagOcrResult::Read { tags, boxes })) => LocalOutcome::Read { tags, boxes },
            Ok(Ok(TagOcrResult::Unsure { reason, boxes })) => LocalOutcome::Unsure {
                reason: LocalUnsure::Ocr(reason),
                boxes,
            },
            Ok(Err(reason)) => LocalOutcome::unsure(reason),
            Err(_) => LocalOutcome::unsure(LocalUnsure::Internal),
        };
        LocalRead { outcome, digest }
    }

    async fn download(&self, url: &str) -> Result<Vec<u8>, LocalUnsure> {
        // URL には CDN の署名が入るので、エラーはそのまま出さず URL を外してから記録する
        let fail = |e: reqwest::Error| {
            eprintln!("[local_ocr] download failed: {}", e.without_url());
            LocalUnsure::Download
        };
        let mut resp = self
            .client
            .get(url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(fail)?;
        if resp.content_length().is_some_and(|l| l > MAX_BYTES) {
            return Err(LocalUnsure::TooLarge);
        }
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(fail)? {
            if (buf.len() + chunk.len()) as u64 > MAX_BYTES {
                return Err(LocalUnsure::TooLarge);
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf)
    }

    /// shadow の結果を集計に足す。
    pub fn record(&self, c: &Comparison) {
        self.stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(c);
    }

    /// ローカルが 1 枚読めた（日本語版として数える）。監視の連続も戻す。
    pub fn record_read(&self) {
        self.stats.lock().unwrap_or_else(|e| e.into_inner()).jp_read += 1;
        self.watch.lock().unwrap_or_else(|e| e.into_inner()).read();
    }

    /// 日本語版の画像をローカルが読めなかった。重複なら何も数えない。
    pub fn record_jp_miss(&self, digest: Option<u64>, link: String) -> MissVerdict {
        let verdict = self
            .watch
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .miss(digest, link);
        if matches!(verdict, MissVerdict::Counted { .. }) {
            self.stats.lock().unwrap_or_else(|e| e.into_inner()).jp_miss += 1;
        }
        verdict
    }

    /// 集計を取り出して 0 に戻す。
    pub fn take_stats(&self) -> ShadowStats {
        std::mem::take(&mut *self.stats.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// 大きさ・メモリの上限つきでデコードする。
fn decode_limited(bytes: &[u8]) -> Result<image::DynamicImage, LocalUnsure> {
    let reader = || {
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| LocalUnsure::Decode)
    };
    // 先に大きさだけ読んで、画素数が多すぎるものはデコードしない
    let (w, h) = reader()?
        .into_dimensions()
        .map_err(|_| LocalUnsure::Decode)?;
    if w > MAX_DIMENSION || h > MAX_DIMENSION || (w as u64) * (h as u64) > MAX_PIXELS {
        return Err(LocalUnsure::TooLarge);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOC);
    let mut r = reader()?;
    r.limits(limits);
    r.decode().map_err(|_| LocalUnsure::Decode)
}

// ---- DM ----

/// オーナーに DM を送る。失敗（DM を閉じている等）は警告のログだけで、呼び出し側には返さない。
/// 本文に他人の文字列は入らないが、念のためメンション通知は一切させない。
pub async fn send_dm(http: &serenity::Http, user: serenity::UserId, text: String) {
    if let Err(e) = try_send_dm(http, user, text).await {
        eprintln!("[local_ocr] DM の送信に失敗しました: {e}");
    }
}

async fn try_send_dm(
    http: &serenity::Http,
    user: serenity::UserId,
    text: String,
) -> Result<(), crate::bot::data::Error> {
    let dm = user.create_dm_channel(http).await?;
    let builder = serenity::CreateMessage::new()
        .content(text)
        .allowed_mentions(serenity::CreateAllowedMentions::new());
    dm.send_message(http, builder).await?;
    Ok(())
}

/// 毎日 JST 0:00 に集計（shadow の比較・日本語版の読めた数）を DM する。前回から何も数えていなければ送らない。
/// 集計は送るたびに 0 に戻す。bot 起動中ずっと動くので `tokio::spawn` すること。
pub async fn run_daily_summary(ctx: serenity::Context, state: Arc<crate::api::AppState>) {
    let Some(user) = state.local_ocr.report_user else {
        return;
    };
    loop {
        tokio::time::sleep(crate::bot::services::birthday::duration_until_next_midnight_jst())
            .await;
        let stats = state.local_ocr.take_stats();
        if !stats.is_empty() {
            send_dm(
                &ctx.http,
                user,
                summary_message(&stats, state.local_ocr.mode),
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(best: &str, b: f32, second: &str, s: f32) -> BoxScore {
        BoxScore {
            best_tag: best.into(),
            best: b,
            second_tag: second.into(),
            second: s,
            margin: b - s,
            accepted: true,
        }
    }

    fn set(v: &[&str]) -> HashSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn read(tags: &[&str]) -> LocalOutcome {
        LocalOutcome::Read {
            tags: tags.iter().map(|s| s.to_string()).collect(),
            boxes: tags.iter().map(|t| bx(t, 0.9, "x", 0.3)).collect(),
        }
    }

    #[test]
    fn mode_parsing() {
        assert_eq!(LocalOcrMode::parse(None), (LocalOcrMode::Off, false));
        assert_eq!(LocalOcrMode::parse(Some("")), (LocalOcrMode::Off, false));
        assert_eq!(LocalOcrMode::parse(Some("off")), (LocalOcrMode::Off, false));
        assert_eq!(
            LocalOcrMode::parse(Some("shadow")),
            (LocalOcrMode::Shadow, false)
        );
        assert_eq!(LocalOcrMode::parse(Some(" ON ")), (LocalOcrMode::On, false));
        assert_eq!(LocalOcrMode::parse(Some("yes")), (LocalOcrMode::Off, true));
    }

    #[test]
    fn compare_cases() {
        let five = ["近距離", "遠距離", "治療", "火力", "牽制"];
        assert_eq!(compare(&read(&five), &set(&five)), Comparison::Agree);
        assert_eq!(
            compare(
                &read(&five),
                &set(&["近距離", "遠距離", "治療", "火力", "支援"])
            ),
            Comparison::Disagree
        );
        assert_eq!(compare(&read(&five), &set(&[])), Comparison::GoogleNoTags);
        let u = LocalOutcome::Unsure {
            reason: LocalUnsure::Ocr(UnsureReason::LowScore),
            boxes: vec![],
        };
        assert_eq!(
            compare(&u, &set(&five)),
            Comparison::LocalUnsure(LocalUnsure::Ocr(UnsureReason::LowScore))
        );
    }

    #[test]
    fn stats_count_and_summary() {
        let mut s = ShadowStats::default();
        s.record(&Comparison::Agree);
        s.record(&Comparison::Agree);
        s.record(&Comparison::Disagree);
        s.record(&Comparison::GoogleNoTags);
        s.record(&Comparison::LocalUnsure(LocalUnsure::Ocr(
            UnsureReason::GridNotFound,
        )));
        s.record(&Comparison::LocalUnsure(LocalUnsure::Ocr(
            UnsureReason::GridNotFound,
        )));
        s.record(&Comparison::LocalUnsure(LocalUnsure::Download));
        assert_eq!(s.total(), 7);
        let text = summary_message(&s, LocalOcrMode::Shadow);
        assert!(text.ends_with("日本語版 読めた 0/0"), "{text}");
        assert!(
            text.contains("7 枚") && text.contains("一致 2") && text.contains("不一致 1"),
            "{text}"
        );
        assert!(
            text.contains("grid_not_found=2") && text.contains("download_failed=1"),
            "{text}"
        );
    }

    #[test]
    fn summary_in_both_modes() {
        let mut s = ShadowStats::default();
        assert!(s.is_empty());
        s.jp_read = 8;
        s.jp_miss = 2;
        assert!(!s.is_empty());
        s.record(&Comparison::Agree);
        let shadow = summary_message(&s, LocalOcrMode::Shadow);
        assert!(
            shadow.contains("1 枚") && shadow.ends_with(" / 日本語版 読めた 8/10"),
            "{shadow}"
        );
        assert_eq!(
            summary_message(&s, LocalOcrMode::On),
            "[local_ocr] 直近の集計（on）: 日本語版 読めた 8/10"
        );
    }

    fn sample_stats() -> ShadowStats {
        let mut s = ShadowStats::default();
        for c in [
            Comparison::Agree,
            Comparison::Agree,
            Comparison::Disagree,
            Comparison::GoogleNoTags,
            Comparison::LocalUnsure(LocalUnsure::Ocr(UnsureReason::GridNotFound)),
            Comparison::LocalUnsure(LocalUnsure::Download),
        ] {
            s.record(&c);
        }
        s.jp_read = 8;
        s.jp_miss = 2;
        s
    }

    #[test]
    fn snapshot_summary_messages() {
        insta::assert_snapshot!(
            "local_ocr_summary_shadow",
            summary_message(&sample_stats(), LocalOcrMode::Shadow)
        );
        insta::assert_snapshot!(
            "local_ocr_summary_off",
            summary_message(&sample_stats(), LocalOcrMode::Off)
        );
    }

    #[test]
    fn snapshot_miss_alert_message() {
        let links = vec![
            "https://discord.com/channels/1/2/3".to_string(),
            "https://discord.com/channels/1/2/4".to_string(),
        ];
        insta::assert_snapshot!("local_ocr_miss_alert", miss_alert_message(3, &links));
    }

    #[test]
    fn miss_watch_alerts_once_per_streak() {
        let mut w = MissWatch::new();
        let l = |i: u64| format!("link{i}");
        assert_eq!(
            w.miss(Some(1), l(1)),
            MissVerdict::Counted {
                streak: 1,
                alert: None
            }
        );
        assert_eq!(
            w.miss(Some(2), l(2)),
            MissVerdict::Counted {
                streak: 2,
                alert: None
            }
        );
        assert_eq!(
            w.miss(Some(3), l(3)),
            MissVerdict::Counted {
                streak: 3,
                alert: Some(vec![l(1), l(2), l(3)])
            }
        );
        // 以降は DM しない
        assert_eq!(
            w.miss(Some(4), l(4)),
            MissVerdict::Counted {
                streak: 4,
                alert: None
            }
        );
        // 読めたら再び出せる
        w.read();
        assert_eq!(
            w.miss(Some(5), l(5)),
            MissVerdict::Counted {
                streak: 1,
                alert: None
            }
        );
        w.miss(Some(6), l(6));
        assert_eq!(
            w.miss(Some(7), l(7)),
            MissVerdict::Counted {
                streak: 3,
                alert: Some(vec![l(5), l(6), l(7)])
            }
        );
    }

    #[test]
    fn miss_watch_ignores_duplicates() {
        let mut w = MissWatch::new();
        assert_eq!(
            w.miss(Some(1), "a".into()),
            MissVerdict::Counted {
                streak: 1,
                alert: None
            }
        );
        assert_eq!(w.miss(Some(1), "b".into()), MissVerdict::Duplicate);
        assert_eq!(
            w.miss(Some(2), "c".into()),
            MissVerdict::Counted {
                streak: 2,
                alert: None
            }
        );
        // 読めても、覚えている重複は覚えたまま
        w.read();
        assert_eq!(w.miss(Some(2), "d".into()), MissVerdict::Duplicate);
        // 古いものは忘れる
        for i in 10..10 + MISS_RECENT as u64 {
            w.miss(Some(i), "x".into());
        }
        assert!(matches!(
            w.miss(Some(1), "e".into()),
            MissVerdict::Counted { .. }
        ));
    }

    #[test]
    fn record_jp_miss_counts_only_non_duplicates() {
        let ocr = TagOcr::load(crate::engine::recruit::tag_ocr::TEMPLATE_DIR).unwrap();
        let o = LocalOcr::new(LocalOcrMode::On, ocr, None);
        o.record_read();
        o.record_jp_miss(Some(1), "a".into());
        assert_eq!(
            o.record_jp_miss(Some(1), "a".into()),
            MissVerdict::Duplicate
        );
        let s = o.take_stats();
        assert_eq!((s.jp_read, s.jp_miss), (1, 1));
    }

    #[test]
    fn miss_alert_message_has_count_and_links() {
        let links = vec![
            "https://discord.com/channels/1/2/3".to_string(),
            "https://discord.com/channels/1/2/4".to_string(),
        ];
        let m = miss_alert_message(3, &links);
        assert!(
            m.starts_with("[local_ocr] 日本語版の画像をローカルで読めないのが 3 枚続きました")
                && m.ends_with(&links[1])
                && m.contains(&links[0]),
            "{m}"
        );
        assert!(
            jp_miss_log_line(2, LocalUnsure::Ocr(UnsureReason::LowScore))
                .starts_with("[local_ocr] ")
        );
    }

    #[test]
    fn take_stats_resets() {
        let ocr = TagOcr::load(crate::engine::recruit::tag_ocr::TEMPLATE_DIR).unwrap();
        let o = LocalOcr::new(LocalOcrMode::Shadow, ocr, None);
        o.record(&Comparison::Agree);
        assert_eq!(o.take_stats().total(), 1);
        assert_eq!(o.take_stats().total(), 0);
    }

    #[test]
    fn disagreement_message_has_both_sets_boxes_and_link() {
        let local: Vec<String> = ["近距離", "遠距離", "治療", "火力", "牽制"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let boxes: Vec<BoxScore> = local.iter().map(|t| bx(t, 0.912, "支援", 0.5)).collect();
        let link = jump_link(
            Some(serenity::GuildId::new(1)),
            serenity::ChannelId::new(2),
            serenity::MessageId::new(3),
        );
        assert_eq!(link, "https://discord.com/channels/1/2/3");
        let m = disagreement_message(
            &local,
            &set(&["近距離", "遠距離", "治療", "火力", "支援"]),
            &boxes,
            &link,
        );
        assert!(
            m.contains("local :")
                && m.contains("google:")
                && m.contains("枠5: 牽制 0.91 / 支援 0.50")
                && m.ends_with(&link),
            "{m}"
        );
        assert!(m.chars().count() <= DISCORD_LIMIT);
    }

    #[test]
    fn message_is_capped_at_2000_chars() {
        let boxes: Vec<BoxScore> = (0..400)
            .map(|_| bx("あああああああああ", 0.9, "いいいいいいいいいい", 0.1))
            .collect();
        let m = disagreement_message(&[], &set(&[]), &boxes, "https://discord.com/channels/1/2/3");
        assert_eq!(m.chars().count(), DISCORD_LIMIT);
        assert!(m.ends_with('…'));
        assert_eq!(truncate_chars("あいう", 3), "あいう");
        assert_eq!(truncate_chars("あいうえ", 3), "あい…");
    }

    #[test]
    fn log_lines_are_prefixed() {
        let g = set(&["a"]);
        for (c, tags) in [
            (Comparison::Agree, None),
            (Comparison::Disagree, Some(&["b".to_string()][..])),
            (Comparison::GoogleNoTags, None),
            (Comparison::LocalUnsure(LocalUnsure::TooLarge), None),
        ] {
            let l = shadow_log_line(&c, tags, &g, Some(0.7));
            assert!(l.starts_with("[local_ocr] "), "{l}");
        }
        assert!(on_log_line(&read(&["a"])).starts_with("[local_ocr] on read"));
        assert!(on_log_line(&LocalOutcome::unsure(LocalUnsure::Download)).contains("-> google"));
    }

    #[test]
    fn decode_rejects_garbage_and_oversized() {
        assert_eq!(
            decode_limited(b"not an image").unwrap_err(),
            LocalUnsure::Decode
        );
        // 幅が上限を超える PNG はデコード前に弾く
        let mut png = Vec::new();
        image::DynamicImage::ImageLuma8(image::GrayImage::new(8001, 1))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(decode_limited(&png).unwrap_err(), LocalUnsure::TooLarge);
        let mut ok = Vec::new();
        image::DynamicImage::ImageLuma8(image::GrayImage::new(10, 10))
            .write_to(&mut std::io::Cursor::new(&mut ok), image::ImageFormat::Png)
            .unwrap();
        assert!(decode_limited(&ok).is_ok());
    }

    /// 本物の Discord で、食い違いの DM とまとめの DM が届くかを確かめる（手動）。
    /// .env の DISCORD_TOKEN の bot から LOCAL_OCR_REPORT_USER_ID の人へ送る。送り先は bot と同じ鯖にいる必要がある。
    /// `$env:LOCAL_OCR_REPORT_USER_ID = "<ID>"; cargo test --lib -- --ignored local_ocr_dm_live`
    #[tokio::test]
    #[ignore = "本物の Discord に DM を送る"]
    async fn local_ocr_dm_live() {
        dotenvy::dotenv().ok();
        let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN が要る");
        let user: u64 = std::env::var("LOCAL_OCR_REPORT_USER_ID")
            .expect("LOCAL_OCR_REPORT_USER_ID が要る")
            .parse()
            .expect("LOCAL_OCR_REPORT_USER_ID は数値");
        let http = serenity::Http::new(&token);
        let user = serenity::UserId::new(user);

        let local = vec![
            "前衛".to_string(),
            "近距離".into(),
            "火力".into(),
            "生存".into(),
            "防御".into(),
        ];
        let google: HashSet<String> = ["前衛", "遠距離", "火力", "生存", "防御"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let boxes = vec![
            bx("前衛", 0.98, "重装", 0.80),
            bx("近距離", 0.86, "遠距離", 0.83),
            bx("火力", 0.97, "生存", 0.78),
            bx("生存", 0.96, "火力", 0.77),
            bx("防御", 0.95, "減速", 0.79),
        ];
        let text = disagreement_message(&local, &google, &boxes, "（テスト送信のためリンクなし）");
        try_send_dm(&http, user, format!("【テスト】{text}"))
            .await
            .expect("食い違いの DM を送れなかった");

        let mut stats = ShadowStats::default();
        stats.record(&Comparison::Agree);
        stats.record(&Comparison::Disagree);
        stats.record(&Comparison::LocalUnsure(LocalUnsure::Ocr(
            UnsureReason::GridNotFound,
        )));
        stats.jp_read = 1;
        stats.jp_miss = 1;
        try_send_dm(
            &http,
            user,
            format!(
                "【テスト】{}",
                summary_message(&stats, LocalOcrMode::Shadow)
            ),
        )
        .await
        .expect("まとめの DM を送れなかった");
        try_send_dm(
            &http,
            user,
            format!("【テスト】{}", summary_message(&stats, LocalOcrMode::On)),
        )
        .await
        .expect("on のまとめの DM を送れなかった");

        let links = vec![
            "（テスト送信のためリンクなし 1）".to_string(),
            "（2）".into(),
            "（3）".into(),
        ];
        try_send_dm(
            &http,
            user,
            format!("【テスト】{}", miss_alert_message(3, &links)),
        )
        .await
        .expect("連続の DM を送れなかった");
    }
}
