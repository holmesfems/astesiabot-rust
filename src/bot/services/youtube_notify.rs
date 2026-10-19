//! YouTube 新着動画のお知らせ。チャンネルの RSS（Atom）を定期的に読み、公開から
//! [`RECENT_WINDOW`] 以内でまだお知らせしていない動画（ショートは除く）を、
//! アステシアの口調の宣伝文（占い館と同じ OpenAI クライアント・キャラ設定）を添えて
//! お知らせ用チャンネルに投稿する。
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
const FALLBACK_PITCH: &str = "ドクター、新しい動画が公開されたわ。ぜひ見に来てちょうだい。";
/// YouTube はブラウザ以外の User-Agent だと応答を変えることがあるので、ブラウザを名乗る。
const USER_AGENT: &str = "Mozilla/5.0 (compatible; astesiabot)";
const PITCH_TASK: &str = "\
# 今回のお願い
ドクターのYouTubeチャンネルに新しい動画が公開されたわ。Discordサーバーのみんなに、\
動画を見に来てもらえるよう、アステシアとして短い宣伝の言葉を書いて。
- 1〜2文。動画の内容に軽く触れて、見たくなるように誘うこと。
- 動画のタイトル・URL・メンションは書かないこと（あとで添える）。
- 宣伝の言葉だけを返すこと。";

/// お知らせの設定。起動時に一度だけ環境変数から解決する。
pub struct YoutubeNotifyConfig {
    youtube_channel_id: String,
    discord_channel: serenity::ChannelId,
    mention_role: Option<serenity::RoleId>,
}

impl YoutubeNotifyConfig {
    /// `YOUTUBE_CHANNEL_ID` と `CHANNEL_ID_YOUTUBE_NOTIFY` が両方あれば有効。どちらかが
    /// 未設定なら None（機能ごと止める。環境変数を足す前にデプロイしても起動できるように）。
    /// `ROLE_ID_YOUTUBE_NOTIFY` は任意（未設定ならメンションしない）。値が不正なら panic。
    pub fn from_env() -> Option<Self> {
        let youtube_channel_id = std::env::var("YOUTUBE_CHANNEL_ID").ok()?;
        std::env::var("CHANNEL_ID_YOUTUBE_NOTIFY").ok()?;
        Some(Self {
            youtube_channel_id,
            discord_channel: channel_id_env("CHANNEL_ID_YOUTUBE_NOTIFY"),
            mention_role: std::env::var("ROLE_ID_YOUTUBE_NOTIFY")
                .ok()
                .map(|_| role_id_env("ROLE_ID_YOUTUBE_NOTIFY")),
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

/// 1回分の確認。新しい動画があればお知らせする。
async fn check_once(
    ctx: &serenity::Context,
    config: &YoutubeNotifyConfig,
    feed_client: &reqwest::Client,
    no_redirect: &reqwest::Client,
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
    let since = Utc::now() - RECENT_WINDOW;
    videos.retain(|v| v.published >= since);
    // 古い順にお知らせする
    videos.sort_by_key(|v| v.published);

    for video in videos {
        if already_posted(ctx, config.discord_channel, &video.video_id).await? {
            continue;
        }
        if is_short(no_redirect, &video.video_id).await? {
            continue;
        }
        let pitch = match state.uranai.write_in_character(PITCH_TASK, &pitch_input(&video)).await {
            Ok(raw) => clean_pitch(&raw).unwrap_or_else(|| FALLBACK_PITCH.to_string()),
            Err(e) => {
                eprintln!("[youtube_notify] 宣伝文の生成に失敗したので決まった文面にします: {e}");
                FALLBACK_PITCH.to_string()
            }
        };
        // 宣伝文は AI の出力なので、通知は設定したロールにだけ飛ばす（@everyone などは通知させない）
        let allowed = serenity::CreateAllowedMentions::new().roles(config.mention_role.into_iter().collect::<Vec<_>>());
        let message = serenity::CreateMessage::new()
            .content(build_message(config.mention_role, &pitch, &video))
            .allowed_mentions(allowed);
        config.discord_channel.send_message(&ctx.http, message).await?;
        println!("[youtube_notify] お知らせしました: {} ({})", video.title, video.video_id);
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
    loop {
        if let Err(e) = check_once(&ctx, &config, &feed_client, &no_redirect, &state).await {
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
    }

    #[test]
    fn pitch_input_truncates_description() {
        let mut v = video();
        v.description = "い".repeat(DESCRIPTION_PROMPT_CHARS + 50);
        let input = pitch_input(&v);
        assert_eq!(input.matches('い').count(), DESCRIPTION_PROMPT_CHARS);
    }
}
