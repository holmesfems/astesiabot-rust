//! YouTube 新着動画のお知らせ。チャンネルのアップロード一覧（YouTube Data API v3 の
//! `playlistItems`）を定期的に読み、公開から [`RECENT_WINDOW`] 以内でまだお知らせしていない
//! 動画（ショートは除く）を、アステシアの口調の宣伝文（占い館と同じ OpenAI クライアント・
//! キャラ設定）を添えてお知らせ用チャンネルに投稿する。
//!
//! RSS（`/feeds/videos.xml`）は使わない。Heroku（AWS）の IP からはどのチャンネルでも 404 が
//! 返り、読めなくなったため（動画ページ・チャンネルページは読める。2026-10 に確認）。
//!
//! 配信（ライブ）もアップロード一覧に載るので、動画ページの `ytInitialPlayerResponse` で見分ける。
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
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// アップロード一覧を読みに行く間隔（1回で Data API の割り当てを 1 使う。1日144。無料枠は1日10000）。
const POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// 1回に読むアップロード一覧の件数（新しい順）。
const PLAYLIST_MAX_RESULTS: &str = "15";
const PLAYLIST_ITEMS_URL: &str = "https://www.googleapis.com/youtube/v3/playlistItems";
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
    /// チャンネルのアップロード一覧のプレイリストID（チャンネルIDの `UC` を `UU` にしたもの）。
    uploads_playlist_id: String,
    api_key: String,
    discord_channel: serenity::ChannelId,
    mention_role: Option<serenity::RoleId>,
    /// 配信告知チャンネル。None なら配信はお知らせしない。
    stream_channel: Option<serenity::ChannelId>,
    stream_role: Option<serenity::RoleId>,
}

impl YoutubeNotifyConfig {
    /// `YOUTUBE_CHANNEL_ID`・`YOUTUBE_API_KEY`・`CHANNEL_ID_YOUTUBE_NOTIFY` が揃っていれば有効。
    /// どれかが未設定なら None（機能ごと止める。環境変数を足す前にデプロイしても起動できるように）。
    /// 次は任意。値が不正なら panic。
    /// - `ROLE_ID_YOUTUBE_NOTIFY`: 動画のお知らせでメンションするロール
    /// - `CHANNEL_ID_STREAM_NOTIFY`: 配信告知チャンネル（未設定なら配信はお知らせしない）
    /// - `ROLE_ID_STREAM_NOTIFY`: 配信のお知らせでメンションするロール
    pub fn from_env() -> Option<Self> {
        let youtube_channel_id = std::env::var("YOUTUBE_CHANNEL_ID").ok()?;
        let api_key = std::env::var("YOUTUBE_API_KEY").ok()?;
        std::env::var("CHANNEL_ID_YOUTUBE_NOTIFY").ok()?;
        let uploads_playlist_id = uploads_playlist_id(&youtube_channel_id)
            .unwrap_or_else(|| panic!("YOUTUBE_CHANNEL_ID は UC で始まるチャンネルIDにしてください: {youtube_channel_id}"));
        let optional_channel = |key: &str| std::env::var(key).ok().map(|_| channel_id_env(key));
        let optional_role = |key: &str| std::env::var(key).ok().map(|_| role_id_env(key));
        Some(Self {
            uploads_playlist_id,
            api_key,
            discord_channel: channel_id_env("CHANNEL_ID_YOUTUBE_NOTIFY"),
            mention_role: optional_role("ROLE_ID_YOUTUBE_NOTIFY"),
            stream_channel: optional_channel("CHANNEL_ID_STREAM_NOTIFY"),
            stream_role: optional_role("ROLE_ID_STREAM_NOTIFY"),
        })
    }
}

/// チャンネルID（`UC…`）からアップロード一覧のプレイリストID（`UU…`）を作る。
fn uploads_playlist_id(channel_id: &str) -> Option<String> {
    channel_id.strip_prefix("UC").map(|rest| format!("UU{rest}"))
}

/// アップロード一覧の1動画分。
#[derive(Debug, PartialEq)]
pub struct FeedVideo {
    pub video_id: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub published: DateTime<Utc>,
}

/// `playlistItems` の応答（使う所だけ）。
#[derive(Deserialize)]
struct PlaylistItems {
    #[serde(default)]
    items: Vec<PlaylistItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistItem {
    snippet: PlaylistSnippet,
    content_details: PlaylistContentDetails,
    status: Option<PlaylistStatus>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistStatus {
    /// `public` / `unlisted` / `private`。
    privacy_status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistSnippet {
    /// プレイリストに入った時刻。`videoPublishedAt` が無いとき（公開前など）に使う。
    published_at: Option<DateTime<Utc>>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistContentDetails {
    video_id: String,
    video_published_at: Option<DateTime<Utc>>,
}

/// `playlistItems`（`part=snippet,contentDetails,status`）の応答から動画の一覧を取り出す。
/// 公開（`public`）でない項目・公開時刻が読めない項目は飛ばす。
pub fn parse_playlist_items(json: &str) -> Result<Vec<FeedVideo>, String> {
    let parsed: PlaylistItems =
        serde_json::from_str(json).map_err(|e| format!("アップロード一覧が読めない: {e}"))?;
    Ok(parsed
        .items
        .into_iter()
        .filter_map(|item| {
            // 非公開・限定公開（公開予約の待ちを含む）はお知らせしない。公開されれば次の回に拾う
            let privacy = item.status.as_ref().and_then(|s| s.privacy_status.as_deref());
            if privacy != Some("public") {
                return None;
            }
            let published = item.content_details.video_published_at.or(item.snippet.published_at)?;
            let video_id = item.content_details.video_id;
            Some(FeedVideo {
                url: format!("https://www.youtube.com/watch?v={video_id}"),
                video_id,
                title: item.snippet.title.trim().to_string(),
                description: item.snippet.description.trim().to_string(),
                published,
            })
        })
        .collect())
}

/// アップロード一覧を Data API から読む。URL に API キーが入るので、エラーには URL を含めない
/// （reqwest のエラーは既定で URL を表示し、ログにキーが残るため）。
async fn fetch_uploads(client: &reqwest::Client, config: &YoutubeNotifyConfig) -> Result<Vec<FeedVideo>, Error> {
    let resp = client
        .get(PLAYLIST_ITEMS_URL)
        .query(&[
            ("part", "snippet,contentDetails,status"),
            ("playlistId", config.uploads_playlist_id.as_str()),
            ("maxResults", PLAYLIST_MAX_RESULTS),
            ("key", config.api_key.as_str()),
        ])
        .send()
        .await
        .map_err(|e| e.without_url())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.without_url())?;
    if !status.is_success() {
        return Err(format!("YouTube Data API が {status} を返した: {}", api_error_message(&body)).into());
    }
    Ok(parse_playlist_items(&body)?)
}

/// Data API のエラー応答（`{"error":{"message":…}}`）から理由を取り出す。読めなければ本文の先頭。
fn api_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(200).collect())
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
    let mut videos = fetch_uploads(feed_client, config).await?;
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

/// Data API・動画ページ用と、ショート判定用（リダイレクトを追わない）の HTTP クライアント。
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

    /// 実際の `playlistItems` の応答から使う所だけ残したもの（2件目は公開前で videoPublishedAt が無い。
    /// 4・5件目は非公開・限定公開）。
    const SAMPLE: &str = r#"{
  "kind": "youtube#playlistItemListResponse",
  "items": [
    {
      "snippet": {
        "publishedAt": "2026-10-19T07:40:00Z",
        "title": "【天井交換 誰を選ぶ】ウィシャデルか新約エクシアか？ Q&A ★ ",
        "description": "おすすめを解説します！\n・新約エクシア\n",
        "resourceId": { "kind": "youtube#video", "videoId": "XeOiPXCJyFA" }
      },
      "contentDetails": { "videoId": "XeOiPXCJyFA", "videoPublishedAt": "2026-10-19T07:34:29Z" },
      "status": { "privacyStatus": "public" }
    },
    {
      "snippet": { "publishedAt": "2026-10-13T10:56:06Z", "title": "配信の枠" },
      "contentDetails": { "videoId": "jgo3tHu1dZQ" },
      "status": { "privacyStatus": "public" }
    },
    {
      "snippet": { "title": "時刻なし" },
      "contentDetails": { "videoId": "noTimeAtAll" },
      "status": { "privacyStatus": "public" }
    },
    {
      "snippet": { "publishedAt": "2026-10-21T00:00:00Z", "title": "Private video" },
      "contentDetails": { "videoId": "privateOne1" },
      "status": { "privacyStatus": "private" }
    },
    {
      "snippet": { "publishedAt": "2026-10-21T00:00:00Z", "title": "限定公開" },
      "contentDetails": { "videoId": "unlisted111" },
      "status": { "privacyStatus": "unlisted" }
    }
  ]
}"#;

    #[test]
    fn parse_playlist_items_reads_items() {
        let videos = parse_playlist_items(SAMPLE).expect("sample should parse");
        assert_eq!(videos.len(), 2, "公開でない項目・公開時刻が読めない項目は飛ばす");
        let v = &videos[0];
        assert_eq!(v.video_id, "XeOiPXCJyFA");
        assert_eq!(v.title, "【天井交換 誰を選ぶ】ウィシャデルか新約エクシアか？ Q&A ★");
        assert_eq!(v.url, "https://www.youtube.com/watch?v=XeOiPXCJyFA");
        assert_eq!(v.description, "おすすめを解説します！\n・新約エクシア");
        // プレイリストに入った時刻ではなく、動画の公開時刻を使う
        assert_eq!(v.published.to_rfc3339(), "2026-10-19T07:34:29+00:00");
        assert_eq!(videos[1].video_id, "jgo3tHu1dZQ");
        assert_eq!(videos[1].published.to_rfc3339(), "2026-10-13T10:56:06+00:00");
        assert_eq!(videos[1].description, "");
    }

    #[test]
    fn parse_playlist_items_accepts_empty_and_rejects_garbage() {
        assert_eq!(parse_playlist_items(r#"{"items":[]}"#).unwrap(), vec![]);
        assert_eq!(parse_playlist_items(r#"{"kind":"youtube#playlistItemListResponse"}"#).unwrap(), vec![]);
        assert!(parse_playlist_items("<html>").is_err());
    }

    #[test]
    fn uploads_playlist_id_swaps_prefix() {
        assert_eq!(uploads_playlist_id("UCqaaoz71ygYPrTrtApHGkZg").as_deref(), Some("UUqaaoz71ygYPrTrtApHGkZg"));
        assert_eq!(uploads_playlist_id("@holmesfems"), None);
    }

    #[test]
    fn api_error_message_reads_reason() {
        let body = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","errors":[]}}"#;
        assert_eq!(api_error_message(body), "API key not valid. Please pass a valid API key.");
        assert_eq!(api_error_message("Not Found"), "Not Found");
    }

    fn video() -> FeedVideo {
        parse_playlist_items(SAMPLE).unwrap().remove(0)
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

    /// 本物の YouTube に繋いで、アップロード一覧が読めることとショート判定が効くことを確かめる
    /// （ネットワークと `.env` の `YOUTUBE_API_KEY` が要るので普段は走らせない。
    /// `cargo test -- --ignored youtube_live`）。
    #[tokio::test]
    #[ignore]
    async fn youtube_live() {
        dotenvy::dotenv().ok();
        let config = YoutubeNotifyConfig {
            uploads_playlist_id: uploads_playlist_id("UCqaaoz71ygYPrTrtApHGkZg").unwrap(),
            api_key: std::env::var("YOUTUBE_API_KEY").expect("YOUTUBE_API_KEY が要る"),
            discord_channel: serenity::ChannelId::new(1),
            mention_role: None,
            stream_channel: None,
            stream_role: None,
        };
        let (feed_client, no_redirect) = http_clients();
        let videos = fetch_uploads(&feed_client, &config).await.unwrap();
        assert!(!videos.is_empty());
        assert!(videos.iter().any(|v| v.video_id == "XeOiPXCJyFA"));
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
