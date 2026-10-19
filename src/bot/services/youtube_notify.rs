//! YouTube 新着動画のお知らせ。チャンネルの RSS（Atom）を定期的に読み、公開から
//! [`RECENT_WINDOW`] 以内でまだお知らせしていない動画（ショートは除く）を、
//! アステシアの口調の宣伝文（占い館と同じ OpenAI クライアント・キャラ設定）を添えて
//! お知らせ用チャンネルに投稿する。
//!
//! 配信（ライブ）も RSS に載るので、動画ページの `ytInitialPlayerResponse` で見分ける。
//! 配信中になったら、配信告知チャンネルに決まった文面で投稿する（AI は通さない）。
//! 配信予定・プレミア公開前のものは、始まるまで待つ。過去の配信はお知らせしない。
//!
//! 二重投稿の防止はファイルに状態を持たず、お知らせ用チャンネルの直近
//! [`HISTORY_LIMIT`] 件に bot 自身がその動画IDを貼っていないかで判定する
//! （Heroku 等は実行時のファイル書き込みが再起動で消えるため。Seed と同じ理由）。

use crate::api::AppState;
use crate::bot::data::Error;
use crate::bot::utils::{channel_id_env, role_id_env};
use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// RSS を読みに行く間隔。
const POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// これより前に公開された動画はお知らせしない（初回起動時に過去の動画を流さないため）。
const RECENT_WINDOW: chrono::Duration = chrono::Duration::hours(24);
/// 二重投稿の判定で読む、お知らせ用チャンネルの直近のメッセージ数（Discord API の上限）。
const HISTORY_LIMIT: u8 = 50;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// 宣伝文を AI に頼むときに添える概要欄の長さ（文字数）。
const DESCRIPTION_PROMPT_CHARS: usize = 400;
/// AI の宣伝文がこれより長ければ使わず、決まった文面にする。
const MAX_PITCH_CHARS: usize = 300;
const FALLBACK_PITCH: &str = "ふぉめさんからの新しい動画よ！ぜひ見に来てちょうだい。";
/// 配信のお知らせの文面（AI は通さない）。
const STREAM_PITCH: &str = "ふぉめさんの配信が始まったわ！みんなで見に行きましょ。";
/// YouTube はブラウザ以外の User-Agent だと応答を変えることがあるので、ブラウザを名乗る。
const USER_AGENT: &str = "Mozilla/5.0 (compatible; astesiabot)";
const PITCH_TASK: &str = "\
# 今回のお願い
このDiscordサーバーの主、ふぉめさんのYouTubeチャンネルに新しい動画が公開されたわ。\
サーバーのみんなに、動画を見に来てもらえるよう、アステシアとして短い宣伝の言葉を書いて。
- 1〜2文。動画の内容に軽く触れて、見たくなるように誘うこと。
- 「ふぉめさんからの新しい動画よ！」のように、ふぉめさんの新しい動画だと必ず分かるようにすること。
- 動画のタイトル・URL・メンションは書かないこと（あとで添える）。
- 宣伝の言葉だけを返すこと。";

/// お知らせの設定。起動時に一度だけ環境変数から解決する。
pub struct YoutubeNotifyConfig {
    youtube_channel_id: String,
    discord_channel: serenity::ChannelId,
    mention_role: Option<serenity::RoleId>,
    /// 配信告知チャンネル。None なら配信はお知らせしない。
    stream_channel: Option<serenity::ChannelId>,
    stream_role: Option<serenity::RoleId>,
}

impl YoutubeNotifyConfig {
    /// `YOUTUBE_CHANNEL_ID` と `CHANNEL_ID_YOUTUBE_NOTIFY` が両方あれば有効。どちらかが
    /// 未設定なら None（機能ごと止める。環境変数を足す前にデプロイしても起動できるように）。
    /// 次は任意。値が不正なら panic。
    /// - `ROLE_ID_YOUTUBE_NOTIFY`: 動画のお知らせでメンションするロール
    /// - `CHANNEL_ID_STREAM_NOTIFY`: 配信告知チャンネル（未設定なら配信はお知らせしない）
    /// - `ROLE_ID_STREAM_NOTIFY`: 配信のお知らせでメンションするロール
    pub fn from_env() -> Option<Self> {
        let youtube_channel_id = std::env::var("YOUTUBE_CHANNEL_ID").ok()?;
        std::env::var("CHANNEL_ID_YOUTUBE_NOTIFY").ok()?;
        let optional_channel = |key: &str| std::env::var(key).ok().map(|_| channel_id_env(key));
        let optional_role = |key: &str| std::env::var(key).ok().map(|_| role_id_env(key));
        Some(Self {
            youtube_channel_id,
            discord_channel: channel_id_env("CHANNEL_ID_YOUTUBE_NOTIFY"),
            mention_role: optional_role("ROLE_ID_YOUTUBE_NOTIFY"),
            stream_channel: optional_channel("CHANNEL_ID_STREAM_NOTIFY"),
            stream_role: optional_role("ROLE_ID_STREAM_NOTIFY"),
        })
    }

    fn feed_url(&self) -> String {
        format!(
            "https://www.youtube.com/feeds/videos.xml?channel_id={}",
            self.youtube_channel_id
        )
    }
}

/// RSS の1動画分。
#[derive(Debug, PartialEq)]
pub struct FeedVideo {
    pub video_id: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub published: DateTime<Utc>,
}

/// 読み取り途中の entry。
#[derive(Default)]
struct Partial {
    video_id: String,
    title: String,
    url: String,
    description: String,
    published: String,
}

/// YouTube の RSS（Atom）から動画の一覧を取り出す。`<entry>` 直下の `yt:videoId`・
/// `title`・`published`・`link rel="alternate"` と、`media:group` の `media:description` を読む。
/// 必要な項目が欠けた entry は飛ばす。
pub fn parse_feed(xml: &str) -> Result<Vec<FeedVideo>, String> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut current: Option<Partial> = None;
    let mut videos = Vec::new();

    loop {
        let event = reader.read_event().map_err(|e| format!("RSS の読み取りに失敗: {e}"))?;
        match event {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "entry" {
                    current = Some(Partial::default());
                }
                path.push(name);
            }
            Event::End(_) => {
                if path.pop().as_deref() == Some("entry") {
                    if let Some(p) = current.take() {
                        let Ok(published) = DateTime::parse_from_rfc3339(p.published.trim()) else {
                            continue;
                        };
                        if p.video_id.is_empty() || p.url.is_empty() {
                            continue;
                        }
                        videos.push(FeedVideo {
                            video_id: p.video_id.trim().to_string(),
                            title: p.title.trim().to_string(),
                            url: p.url,
                            description: p.description.trim().to_string(),
                            published: published.with_timezone(&Utc),
                        });
                    }
                }
            }
            Event::Empty(e) => {
                let in_entry = path.last().map(String::as_str) == Some("entry");
                if let (true, Some(p)) = (in_entry && e.name().as_ref() == b"link", current.as_mut()) {
                    if attribute(&e, "rel").as_deref() == Some("alternate") {
                        if let Some(href) = attribute(&e, "href") {
                            p.url = href;
                        }
                    }
                }
            }
            Event::Text(t) => {
                let text = t.decode().map_err(|e| format!("RSS の文字が読めない: {e}"))?;
                push_text(&path, current.as_mut(), &text);
            }
            Event::GeneralRef(r) => {
                // quick-xml 0.38 以降は `&amp;` などの参照が Text と別のイベントで来る
                let resolved = match r.resolve_char_ref().map_err(|e| format!("文字参照が読めない: {e}"))? {
                    Some(c) => c.to_string(),
                    None => {
                        let name = r.decode().map_err(|e| format!("実体参照が読めない: {e}"))?;
                        quick_xml::escape::resolve_predefined_entity(&name)
                            .unwrap_or_default()
                            .to_string()
                    }
                };
                push_text(&path, current.as_mut(), &resolved);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(videos)
}

/// 属性の値（参照は解決済み）。無い・読めなければ None。
fn attribute(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    let attr = e.try_get_attribute(name).ok()??;
    let value = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?;
    Some(value.into_owned())
}

/// 今いる要素が読みたい項目なら、その項目に文字を足す。
fn push_text(path: &[String], current: Option<&mut Partial>, text: &str) {
    let Some(p) = current else { return };
    let n = path.len();
    if n < 2 {
        return;
    }
    let (parent, leaf) = (path[n - 2].as_str(), path[n - 1].as_str());
    let field = match (parent, leaf) {
        ("entry", "yt:videoId") => &mut p.video_id,
        ("entry", "title") => &mut p.title,
        ("entry", "published") => &mut p.published,
        ("media:group", "media:description") => &mut p.description,
        _ => return,
    };
    field.push_str(text);
}

/// お知らせの本文。メンションの行（あれば）・宣伝文・タイトル（太字）・URL の順。
/// URL を最後に置くと Discord が動画のカード（サムネと再生ボタン）を出す。
pub fn build_message(mention_role: Option<serenity::RoleId>, pitch: &str, video: &FeedVideo) -> String {
    let mut lines = Vec::new();
    if let Some(role) = mention_role {
        lines.push(format!("<@&{role}>"));
    }
    lines.push(pitch.to_string());
    lines.push(format!("**{}**", video.title));
    lines.push(video.url.clone());
    lines.join("\n")
}

/// AI の宣伝文を整える。空・長すぎる場合は None（呼び出し側で決まった文面にする）。
fn clean_pitch(raw: &str) -> Option<String> {
    let pitch = raw.trim();
    let len = pitch.chars().count();
    (len > 0 && len <= MAX_PITCH_CHARS).then(|| pitch.to_string())
}

fn pitch_input(video: &FeedVideo) -> String {
    let description: String = video.description.chars().take(DESCRIPTION_PROMPT_CHARS).collect();
    format!("動画のタイトル: {}\n概要欄の冒頭:\n{description}", video.title)
}

/// 動画の種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoKind {
    Video,
    Short,
    /// 配信中。
    LiveNow,
    /// 配信予定・プレミア公開前。始まるまで待つ。
    Upcoming,
    /// 終わった配信（アーカイブ）。
    PastStream,
}

impl VideoKind {
    /// これ以上変わらない種類か（覚えておいて、次からは動画ページを読まない）。
    fn is_final(self) -> bool {
        matches!(self, VideoKind::Video | VideoKind::Short | VideoKind::PastStream)
    }
}

/// 配信まわりの状態（動画ページの `ytInitialPlayerResponse` から読む）。
#[derive(Debug, PartialEq)]
pub struct LiveStatus {
    pub is_live_content: bool,
    pub is_upcoming: bool,
    pub is_live_now: bool,
}

/// 動画ページの HTML から `ytInitialPlayerResponse` を取り出して、配信まわりの状態を読む。
/// - `videoDetails.isLiveContent`: 配信（予定・配信中・アーカイブ）なら true
/// - `videoDetails.isUpcoming`: 配信予定・プレミア公開前なら true
/// - `microformat.playerMicroformatRenderer.liveBroadcastDetails.isLiveNow`: 配信中なら true
pub fn parse_live_status(html: &str) -> Result<LiveStatus, String> {
    const MARKER: &str = "var ytInitialPlayerResponse = ";
    let start = html
        .find(MARKER)
        .ok_or("動画ページに ytInitialPlayerResponse が無い")?
        + MARKER.len();
    // 後ろに JS が続くので、先頭の JSON の値1つだけを読む
    let player: Value = serde_json::Deserializer::from_str(&html[start..])
        .into_iter::<Value>()
        .next()
        .ok_or("ytInitialPlayerResponse が空")?
        .map_err(|e| format!("ytInitialPlayerResponse が読めない: {e}"))?;
    let flag = |pointer: &str| player.pointer(pointer).and_then(Value::as_bool).unwrap_or(false);
    Ok(LiveStatus {
        is_live_content: flag("/videoDetails/isLiveContent"),
        is_upcoming: flag("/videoDetails/isUpcoming"),
        is_live_now: flag("/microformat/playerMicroformatRenderer/liveBroadcastDetails/isLiveNow"),
    })
}

/// 配信まわりの状態から種類を決める。配信でもプレミアでもなければ None（ショートかどうかは別に確かめる）。
fn live_kind(status: &LiveStatus) -> Option<VideoKind> {
    if status.is_upcoming {
        Some(VideoKind::Upcoming)
    } else if status.is_live_content {
        Some(if status.is_live_now { VideoKind::LiveNow } else { VideoKind::PastStream })
    } else {
        None
    }
}

/// 動画ページと `/shorts/` を読んで種類を決める。
async fn classify(
    feed_client: &reqwest::Client,
    no_redirect: &reqwest::Client,
    video_id: &str,
) -> Result<VideoKind, Error> {
    let html = feed_client
        .get(format!("https://www.youtube.com/watch?v={video_id}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if let Some(kind) = live_kind(&parse_live_status(&html)?) {
        return Ok(kind);
    }
    Ok(if is_short(no_redirect, video_id).await? { VideoKind::Short } else { VideoKind::Video })
}

/// ショート動画か。`/shorts/<ID>` がショートなら 200、ふつうの動画なら `/watch` への
/// リダイレクト（303）を返すことを使う。どちらでもなければ Err（次の回にまた確かめる）。
async fn is_short(no_redirect: &reqwest::Client, video_id: &str) -> Result<bool, Error> {
    let resp = no_redirect
        .get(format!("https://www.youtube.com/shorts/{video_id}"))
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() {
        Ok(true)
    } else if status.is_redirection() {
        Ok(false)
    } else {
        Err(format!("ショート判定で想定外の応答: {status}").into())
    }
}

/// お知らせ用チャンネルの直近のメッセージに、bot がこの動画IDをもう貼っているか。
async fn already_posted(ctx: &serenity::Context, channel: serenity::ChannelId, video_id: &str) -> Result<bool, Error> {
    let me = ctx.cache.current_user().id;
    let recent = channel
        .messages(&ctx.http, serenity::GetMessages::new().limit(HISTORY_LIMIT))
        .await?;
    Ok(recent
        .iter()
        .any(|m| m.author.id == me && m.content.contains(video_id)))
}

/// お知らせを投稿する。本文には AI の出力も入るので、通知は設定したロールにだけ飛ばす
/// （@everyone などは通知させない）。
async fn post(
    ctx: &serenity::Context,
    channel: serenity::ChannelId,
    role: Option<serenity::RoleId>,
    pitch: &str,
    video: &FeedVideo,
) -> Result<(), Error> {
    let allowed = serenity::CreateAllowedMentions::new().roles(role.into_iter().collect::<Vec<_>>());
    let message = serenity::CreateMessage::new()
        .content(build_message(role, pitch, video))
        .allowed_mentions(allowed);
    channel.send_message(&ctx.http, message).await?;
    println!("[youtube_notify] お知らせしました: {} ({}) → {channel}", video.title, video.video_id);
    Ok(())
}

/// 1回分の確認。新しい動画・始まった配信があればお知らせする。
/// `kinds` は種類が決まった動画の覚え書き（bot 起動中だけ持つ）。
async fn check_once(
    ctx: &serenity::Context,
    config: &YoutubeNotifyConfig,
    feed_client: &reqwest::Client,
    no_redirect: &reqwest::Client,
    kinds: &mut HashMap<String, VideoKind>,
    state: &AppState,
) -> Result<(), Error> {
    let xml = feed_client
        .get(config.feed_url())
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let mut videos = parse_feed(&xml)?;
    // 古い順にお知らせする
    videos.sort_by_key(|v| v.published);
    let since = Utc::now() - RECENT_WINDOW;

    for video in videos {
        // 配信は枠を立てた時（published）より後に始まるので、RECENT_WINDOW で絞る前に種類を決める
        let kind = match kinds.get(&video.video_id) {
            Some(&kind) => kind,
            None => {
                let kind = match classify(feed_client, no_redirect, &video.video_id).await {
                    Ok(kind) => kind,
                    Err(e) => {
                        // この動画だけ次の回に回す
                        eprintln!("[youtube_notify] {} の種類が分からないので次の回にまた確かめます: {e}", video.video_id);
                        continue;
                    }
                };
                if kind.is_final() {
                    kinds.insert(video.video_id.clone(), kind);
                }
                kind
            }
        };
        match kind {
            VideoKind::Video if video.published >= since => {
                if already_posted(ctx, config.discord_channel, &video.video_id).await? {
                    continue;
                }
                let pitch = match state.uranai.write_in_character(PITCH_TASK, &pitch_input(&video)).await {
                    Ok(raw) => clean_pitch(&raw).unwrap_or_else(|| FALLBACK_PITCH.to_string()),
                    Err(e) => {
                        eprintln!("[youtube_notify] 宣伝文の生成に失敗したので決まった文面にします: {e}");
                        FALLBACK_PITCH.to_string()
                    }
                };
                post(ctx, config.discord_channel, config.mention_role, &pitch, &video).await?;
            }
            VideoKind::LiveNow => {
                let Some(channel) = config.stream_channel else { continue };
                if already_posted(ctx, channel, &video.video_id).await? {
                    continue;
                }
                post(ctx, channel, config.stream_role, STREAM_PITCH, &video).await?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// RSS 用と、ショート判定用（リダイレクトを追わない）の HTTP クライアント。
fn http_clients() -> (reqwest::Client, reqwest::Client) {
    let builder = || reqwest::Client::builder().timeout(HTTP_TIMEOUT).user_agent(USER_AGENT);
    let feed_client = builder().build().expect("reqwest client should build");
    let no_redirect = builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("reqwest client should build");
    (feed_client, no_redirect)
}

/// [`POLL_INTERVAL`] ごとに新着を確かめ続けるループ。bot 起動中ずっと動く想定なので、
/// 呼び出し側は `tokio::spawn` すること。
pub async fn run(ctx: serenity::Context, config: YoutubeNotifyConfig, state: Arc<AppState>) {
    let (feed_client, no_redirect) = http_clients();
    let mut kinds = HashMap::new();
    loop {
        if let Err(e) = check_once(&ctx, &config, &feed_client, &no_redirect, &mut kinds, &state).await {
            eprintln!("[youtube_notify] 新着の確認に失敗しました（次の回にまた確かめます）: {e}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
 <link rel="self" href="http://www.youtube.com/feeds/videos.xml?channel_id=UCqaaoz71ygYPrTrtApHGkZg"/>
 <title>ふぉめ holmesfems</title>
 <link rel="alternate" href="https://www.youtube.com/channel/UCqaaoz71ygYPrTrtApHGkZg"/>
 <published>2016-08-28T14:09:55+00:00</published>
 <entry>
  <id>yt:video:XeOiPXCJyFA</id>
  <yt:videoId>XeOiPXCJyFA</yt:videoId>
  <title>【天井交換 誰を選ぶ】ウィシャデルか新約エクシアか？ Q&amp;A &#x2605;</title>
  <link rel="alternate" href="https://www.youtube.com/watch?v=XeOiPXCJyFA"/>
  <published>2026-10-19T07:34:29+00:00</published>
  <updated>2026-10-19T09:42:39+00:00</updated>
  <media:group>
   <media:title>media側のタイトルは読まない</media:title>
   <media:description>おすすめを解説します！
・新約エクシア</media:description>
  </media:group>
 </entry>
 <entry>
  <yt:videoId>jgo3tHu1dZQ</yt:videoId>
  <title>前の動画</title>
  <link rel="alternate" href="https://www.youtube.com/watch?v=jgo3tHu1dZQ"/>
  <published>2026-10-13T10:56:06+00:00</published>
 </entry>
</feed>"#;

    #[test]
    fn parse_feed_reads_entries() {
        let videos = parse_feed(SAMPLE).expect("sample should parse");
        assert_eq!(videos.len(), 2);
        let v = &videos[0];
        assert_eq!(v.video_id, "XeOiPXCJyFA");
        assert_eq!(v.title, "【天井交換 誰を選ぶ】ウィシャデルか新約エクシアか？ Q&A ★");
        assert_eq!(v.url, "https://www.youtube.com/watch?v=XeOiPXCJyFA");
        assert_eq!(v.description, "おすすめを解説します！\n・新約エクシア");
        assert_eq!(v.published.to_rfc3339(), "2026-10-19T07:34:29+00:00");
        assert_eq!(videos[1].video_id, "jgo3tHu1dZQ");
        assert_eq!(videos[1].description, "");
    }

    #[test]
    fn parse_feed_skips_entries_without_required_fields() {
        let xml = r#"<feed xmlns:yt="y"><entry><title>IDなし</title><published>2026-10-19T07:34:29+00:00</published></entry></feed>"#;
        assert_eq!(parse_feed(xml).unwrap(), vec![]);
    }

    fn video() -> FeedVideo {
        parse_feed(SAMPLE).unwrap().remove(0)
    }

    #[test]
    fn build_message_puts_url_last() {
        let msg = build_message(Some(serenity::RoleId::new(42)), "見に来てね", &video());
        assert_eq!(
            msg,
            "<@&42>\n見に来てね\n**【天井交換 誰を選ぶ】ウィシャデルか新約エクシアか？ Q&A ★**\nhttps://www.youtube.com/watch?v=XeOiPXCJyFA"
        );
        let msg = build_message(None, "見に来てね", &video());
        assert!(msg.starts_with("見に来てね\n"));
    }

    #[test]
    fn clean_pitch_rejects_empty_and_too_long() {
        assert_eq!(clean_pitch("  見に来てね \n").as_deref(), Some("見に来てね"));
        assert_eq!(clean_pitch("   "), None);
        assert_eq!(clean_pitch(&"あ".repeat(MAX_PITCH_CHARS + 1)), None);
    }

    /// 本物の YouTube に繋いで、RSS が読めることとショート判定が効くことを確かめる
    /// （ネットワークが要るので普段は走らせない。`cargo test -- --ignored youtube_live`）。
    #[tokio::test]
    #[ignore]
    async fn youtube_live() {
        let (feed_client, no_redirect) = http_clients();
        let xml = feed_client
            .get("https://www.youtube.com/feeds/videos.xml?channel_id=UCqaaoz71ygYPrTrtApHGkZg")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(!parse_feed(&xml).unwrap().is_empty());
        assert!(!is_short(&no_redirect, "XeOiPXCJyFA").await.unwrap());
        assert!(is_short(&no_redirect, "ebu6YObXPiA").await.unwrap());
        let kind = |id: &'static str| classify(&feed_client, &no_redirect, id);
        assert_eq!(kind("XeOiPXCJyFA").await.unwrap(), VideoKind::Video);
        assert_eq!(kind("ebu6YObXPiA").await.unwrap(), VideoKind::Short);
        assert_eq!(kind("0rLwW1TIu1k").await.unwrap(), VideoKind::PastStream);
    }

    fn player_page(player_json: &str) -> String {
        format!(r#"<html><script>var ytInitialPlayerResponse = {player_json};var meta = document.createElement('meta');</script></html>"#)
    }

    #[test]
    fn parse_live_status_reads_player_response() {
        // 実際のページから必要な所だけ残したもの
        let video = player_page(r#"{"videoDetails":{"videoId":"XeOiPXCJyFA","isLiveContent":false}}"#);
        let past = player_page(
            r#"{"videoDetails":{"isLiveContent":true},"microformat":{"playerMicroformatRenderer":{"liveBroadcastDetails":{"isLiveNow":false,"startTimestamp":"2026-07-20T11:53:04+00:00","endTimestamp":"2026-07-20T16:13:42+00:00"}}}}"#,
        );
        let live = player_page(
            r#"{"videoDetails":{"isLiveContent":true,"isLive":true},"microformat":{"playerMicroformatRenderer":{"liveBroadcastDetails":{"isLiveNow":true,"startTimestamp":"2026-10-19T10:47:32+00:00"}}}}"#,
        );
        let upcoming = player_page(r#"{"videoDetails":{"isLiveContent":true,"isUpcoming":true}}"#);
        let kind = |html: &str| live_kind(&parse_live_status(html).unwrap());
        assert_eq!(kind(&video), None);
        assert_eq!(kind(&past), Some(VideoKind::PastStream));
        assert_eq!(kind(&live), Some(VideoKind::LiveNow));
        assert_eq!(kind(&upcoming), Some(VideoKind::Upcoming));
        // プレミア公開前（配信ではない）も待つ
        let premiere = player_page(r#"{"videoDetails":{"isLiveContent":false,"isUpcoming":true}}"#);
        assert_eq!(kind(&premiere), Some(VideoKind::Upcoming));
        assert!(parse_live_status("<html></html>").is_err());
    }

    #[test]
    fn only_settled_kinds_are_remembered() {
        assert!(VideoKind::Video.is_final());
        assert!(VideoKind::Short.is_final());
        assert!(VideoKind::PastStream.is_final());
        assert!(!VideoKind::LiveNow.is_final());
        assert!(!VideoKind::Upcoming.is_final());
    }

    #[test]
    fn pitch_input_truncates_description() {
        let mut v = video();
        v.description = "い".repeat(DESCRIPTION_PROMPT_CHARS + 50);
        let input = pitch_input(&v);
        assert_eq!(input.matches('い').count(), DESCRIPTION_PROMPT_CHARS);
    }
}
