//! 本番の公開求人チャンネルに投稿されたスクショと、bot が返したタグ(embed の title)を
//! 集めるローカル専用ツール。タグ枠のテンプレ画像を作って軽量 OCR に使うための素材集め。
//! main.rs(bot/api本体)とは独立に動き、サーバーでは動かさない。
//!
//! ```text
//! $env:COLLECT_DISCORD_TOKEN = Read-Host "astesiabot token"
//! cargo run --bin collect_recruit_samples -- --channel <チャンネルID> [--limit 500] [--max-pages 50] [--before <メッセージID>] [--out local/recruit_samples]
//! ```
//!
//! - トークンは `COLLECT_DISCORD_TOKEN` だけを読む。.env は読まない
//!   （テスト用と本番用のトークンを取り違えないため）。
//! - Discord REST のみ（gateway には繋がない）。出力は `<out>/images/` と `<out>/index.jsonl`。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API_BASE: &str = "https://discord.com/api/v10";
const USER_AGENT: &str = "DiscordBot (https://astesiabot.com, 1.0)";
/// タグ名の正本。bot のタグ抽出と同じ一覧で「既知のタグか」を判定する。
const TAG_LIST_JSON: &str = include_str!("../../data/recruitment/tagList.json");
/// ページ間の固定待ち。レート制限ヘッダとは別に、控えめに叩くため。
const PAGE_DELAY: Duration = Duration::from_millis(300);

// ---- Discord JSON（必要な項目だけ） ----

#[derive(Debug, Deserialize)]
struct Msg {
    id: String,
    #[serde(default)]
    timestamp: String,
    author: Author,
    #[serde(default)]
    attachments: Vec<Attachment>,
    #[serde(default)]
    embeds: Vec<Embed>,
    #[serde(default)]
    message_reference: Option<MessageRef>,
}

#[derive(Debug, Deserialize)]
struct Author {
    id: String,
    #[serde(default)]
    bot: bool,
}

#[derive(Debug, Deserialize, Clone)]
struct Attachment {
    id: String,
    url: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct Embed {
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageRef {
    #[serde(default)]
    message_id: Option<String>,
}

// ---- 出力 ----

#[derive(Debug, Serialize, Deserialize)]
struct Label {
    tags: Vec<String>,
    is_global: bool,
    usable: bool,
    raw_title: String,
}

/// 作者名・本文は個人情報なので持たない。
#[derive(Debug, Serialize, Deserialize)]
struct Record {
    message_id: String,
    attachment_id: String,
    posted_at: String,
    file: String,
    width: Option<u32>,
    height: Option<u32>,
    content_type: Option<String>,
    bot_titles: Vec<String>,
    label: Option<Label>,
}

// ---- 純粋関数 ----

#[derive(Debug, PartialEq)]
struct TitleInfo {
    tags: Vec<String>,
    is_global: bool,
    all_known: bool,
    usable: bool,
}

/// `tagList.json` の全タグ名。
fn known_tags() -> HashSet<String> {
    let v: serde_json::Value =
        serde_json::from_str(TAG_LIST_JSON).expect("tagList.json should be valid JSON");
    let obj = v.as_object().expect("tagList.json should be an object");
    ["eliteTags", "jobTags", "positionTags", "otherTags"]
        .iter()
        .flat_map(|k| {
            obj.get(*k)
                .and_then(|a| a.as_array())
                .unwrap_or_else(|| panic!("tagList.json に {k} が無い"))
                .iter()
                .filter_map(|s| s.as_str().map(str::to_string))
        })
        .collect()
}

/// bot の embed title(`make_title` の出力)をタグに戻す。
/// 末尾の `(大陸版)` `(タグ不足)` `(タグ過多)` を外し、空白で割る。順序はそのまま残す
/// （画面上の枠の並び＝行優先の 3+2 と一致するため）。
fn parse_title(title: &str, known: &HashSet<String>) -> TitleInfo {
    let mut is_global = true;
    let mut lacking = false;
    let mut s = title.to_string();
    for (marker, kind) in [("(大陸版)", 0), ("(タグ不足)", 1), ("(タグ過多)", 1)] {
        if s.contains(marker) {
            s = s.replace(marker, " ");
            match kind {
                0 => is_global = false,
                _ => lacking = true,
            }
        }
    }
    let tags: Vec<String> = s.split_whitespace().map(str::to_string).collect();
    let all_known = !tags.is_empty() && tags.iter().all(|t| known.contains(t));
    // エラー返信の title は「エラー」。既知タグではないので all_known で落ちる
    let usable = tags.len() == 5 && all_known && !lacking;
    TitleInfo {
        tags,
        is_global,
        all_known,
        usable,
    }
}

fn is_image_attachment(a: &Attachment) -> bool {
    if a.content_type
        .as_deref()
        .is_some_and(|c| c.starts_with("image/"))
    {
        return true;
    }
    matches!(
        file_ext(&a.filename).as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp")
    )
}

fn file_ext(filename: &str) -> Option<String> {
    let name = filename.split(['?', '#']).next().unwrap_or(filename);
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty() && ext.len() <= 5).then(|| ext.to_ascii_lowercase())
}

/// 保存時の拡張子。ファイル名優先、無ければ content_type から。
fn pick_ext(a: &Attachment) -> String {
    if let Some(e) =
        file_ext(&a.filename).filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp"))
    {
        return e;
    }
    match a.content_type.as_deref() {
        Some("image/jpeg") => "jpg",
        Some("image/webp") => "webp",
        _ => "png",
    }
    .to_string()
}

/// bot は画像1枚につき1返信を添付順に返す。画像数とタイトル付き返信数が同じときだけ
/// 添付 i ↔ タイトル i と対応づける（タグ無しの画像は返信が省かれ、ずれるため）。
fn assign_titles(image_count: usize, titles: &[String]) -> Vec<Option<&String>> {
    if image_count == titles.len() {
        titles.iter().map(Some).collect()
    } else {
        vec![None; image_count]
    }
}

/// 次のリクエスト前に待つ時間。`X-RateLimit-Remaining` が 0 なら Reset-After 秒待つ。
fn rate_limit_wait(remaining: Option<&str>, reset_after: Option<&str>) -> Option<Duration> {
    if remaining?.trim() != "0" {
        return None;
    }
    let secs: f64 = reset_after?.trim().parse().ok()?;
    secs.is_finite()
        .then(|| Duration::from_secs_f64(secs.clamp(0.0, 60.0)))
}

/// 返信メッセージから (トリガーID → [(返信ID, title)]) を集める。
fn collect_replies(msgs: &[Msg], bot_id: &str, replies: &mut HashMap<String, Vec<(u64, String)>>) {
    for m in msgs.iter().filter(|m| m.author.id == bot_id) {
        let Some(trigger) = m
            .message_reference
            .as_ref()
            .and_then(|r| r.message_id.clone())
        else {
            continue;
        };
        let Ok(rid) = m.id.parse::<u64>() else {
            continue;
        };
        for t in m.embeds.iter().filter_map(|e| e.title.clone()) {
            replies.entry(trigger.clone()).or_default().push((rid, t));
        }
    }
}

/// 返信を投稿順(snowflake 昇順)に並べたタイトル。
fn ordered_titles(replies: &HashMap<String, Vec<(u64, String)>>, trigger_id: &str) -> Vec<String> {
    let mut v = replies.get(trigger_id).cloned().unwrap_or_default();
    v.sort_by_key(|(id, _)| *id);
    v.into_iter().map(|(_, t)| t).collect()
}

// ---- 引数 ----

struct Args {
    channel: String,
    limit: usize,
    max_pages: usize,
    before: Option<String>,
    out: PathBuf,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut channel = None;
    let mut limit = 500usize;
    let mut max_pages = 50usize;
    let mut before = None;
    let mut out = PathBuf::from("local/recruit_samples");
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut val = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} に値がありません"))
        };
        match flag.as_str() {
            "--channel" => channel = Some(val()?),
            "--limit" => {
                limit = val()?
                    .parse()
                    .map_err(|_| "--limit は整数で指定してください".to_string())?
            }
            "--max-pages" => {
                max_pages = val()?
                    .parse()
                    .map_err(|_| "--max-pages は整数で指定してください".to_string())?
            }
            "--before" => before = Some(val()?),
            "--out" => out = PathBuf::from(val()?),
            other => return Err(format!("知らない引数です: {other}")),
        }
    }
    let channel = channel.ok_or("--channel <チャンネルID> が必要です")?;
    if !channel.chars().all(|c| c.is_ascii_digit()) || channel.is_empty() {
        return Err("--channel は数字のIDで指定してください".into());
    }
    Ok(Args {
        channel,
        limit,
        max_pages,
        before,
        out,
    })
}

// ---- REST ----

struct Api {
    http: reqwest::Client,
    wait_next: Option<Duration>,
}

impl Api {
    /// GET してJSONを返す。429 は retry_after 待ちで再試行する。
    /// エラー文にはヘッダを入れない（トークンを出さないため）。
    async fn get_json(
        &mut self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<serde_json::Value, String> {
        for _ in 0..8 {
            if let Some(d) = self.wait_next.take() {
                tokio::time::sleep(d).await;
            }
            let resp = self
                .http
                .get(format!("{API_BASE}{path}"))
                .query(query)
                .send()
                .await
                .map_err(|e| format!("GET {path} に失敗: {}", e.without_url()))?;
            let h = |k: &str| {
                resp.headers()
                    .get(k)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            };
            self.wait_next = rate_limit_wait(
                h("x-ratelimit-remaining").as_deref(),
                h("x-ratelimit-reset-after").as_deref(),
            );
            let status = resp.status();
            if status.as_u16() == 429 {
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                let secs = body["retry_after"]
                    .as_f64()
                    .unwrap_or(5.0)
                    .clamp(0.0, 120.0);
                eprintln!("レート制限: {secs:.1} 秒待ちます");
                tokio::time::sleep(Duration::from_secs_f64(secs)).await;
                continue;
            }
            if !status.is_success() {
                return Err(format!(
                    "GET {path} が {status} を返しました（トークン・チャンネルID・権限を確認）"
                ));
            }
            return resp
                .json()
                .await
                .map_err(|e| format!("GET {path} の応答を読めません: {}", e.without_url()));
        }
        Err(format!("GET {path} がレート制限で進みません"))
    }
}

async fn download(plain: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = plain
        .get(url)
        .send()
        .await
        .map_err(|e| format!("画像の取得に失敗: {}", e.without_url()))?;
    if !resp.status().is_success() {
        return Err(format!("画像の取得が {} を返しました", resp.status()));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("画像の読み込みに失敗: {}", e.without_url()))
}

fn load_index_keys(path: &Path) -> HashSet<(String, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashSet::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            Some((
                v["message_id"].as_str()?.to_string(),
                v["attachment_id"].as_str()?.to_string(),
            ))
        })
        .collect()
}

async fn run(args: Args, token: String) -> Result<(), String> {
    let mut auth = reqwest::header::HeaderValue::from_str(&format!("Bot {token}"))
        .map_err(|_| "トークンの形式が不正です".to_string())?;
    auth.set_sensitive(true);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::AUTHORIZATION, auth);
    let http = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTPクライアントを作れません: {e}"))?;
    // 画像のCDNには認証ヘッダを送らない
    let plain = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTPクライアントを作れません: {e}"))?;
    let mut api = Api {
        http,
        wait_next: None,
    };

    let me = api.get_json("/users/@me", &[]).await?;
    let bot_id = me["id"]
        .as_str()
        .ok_or("/users/@me に id がありません")?
        .to_string();
    let ch = api
        .get_json(&format!("/channels/{}", args.channel), &[])
        .await?;
    println!(
        "bot: {} / チャンネル: #{}（本番の bot・チャンネルか確認してください）",
        me["username"].as_str().unwrap_or("?"),
        ch["name"].as_str().unwrap_or("?")
    );

    let known = known_tags();
    let img_dir = args.out.join("images");
    std::fs::create_dir_all(&img_dir)
        .map_err(|e| format!("{} を作れません: {e}", img_dir.display()))?;
    let index_path = args.out.join("index.jsonl");
    let mut seen = load_index_keys(&index_path);
    let mut index = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&index_path)
        .map_err(|e| format!("{} を開けません: {e}", index_path.display()))?;

    let mut replies: HashMap<String, Vec<(u64, String)>> = HashMap::new();
    let mut before = args.before.clone();
    let (mut pages, mut messages_seen, mut saved, mut skipped, mut usable_labels) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut oldest: Option<String> = None;

    'pages: while pages < args.max_pages {
        let mut q = vec![("limit", "100".to_string())];
        if let Some(b) = &before {
            q.push(("before", b.clone()));
        }
        let v = api
            .get_json(&format!("/channels/{}/messages", args.channel), &q)
            .await?;
        let msgs: Vec<Msg> = serde_json::from_value(v)
            .map_err(|e| format!("メッセージ一覧を解釈できません: {e}"))?;
        if msgs.is_empty() {
            break;
        }
        pages += 1;
        messages_seen += msgs.len();
        // 返信はトリガーより新しいので、同じページの分も先に拾ってから画像を処理する
        collect_replies(&msgs, &bot_id, &mut replies);
        // 新しい→古い の順なので最後が最古
        if let Some(last) = msgs.last() {
            oldest = Some(last.id.clone());
            before = Some(last.id.clone());
        }

        for m in msgs
            .iter()
            .filter(|m| m.author.id != bot_id && !m.author.bot)
        {
            let images: Vec<&Attachment> = m
                .attachments
                .iter()
                .filter(|a| is_image_attachment(a))
                .collect();
            if images.is_empty() {
                continue;
            }
            let titles = ordered_titles(&replies, &m.id);
            let assigned = assign_titles(images.len(), &titles);
            for (a, title) in images.iter().zip(assigned) {
                if seen.contains(&(m.id.clone(), a.id.clone())) {
                    skipped += 1;
                    continue;
                }
                let rel = format!("images/{}_{}.{}", m.id, a.id, pick_ext(a));
                let bytes = match download(&plain, &a.url).await {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("{}_{} を飛ばします: {e}", m.id, a.id);
                        continue;
                    }
                };
                std::fs::write(args.out.join(&rel), bytes)
                    .map_err(|e| format!("{rel} を保存できません: {e}"))?;
                let label = title.map(|t| {
                    let info = parse_title(t, &known);
                    Label {
                        tags: info.tags,
                        is_global: info.is_global,
                        usable: info.usable,
                        raw_title: t.clone(),
                    }
                });
                if label.as_ref().is_some_and(|l| l.usable) {
                    usable_labels += 1;
                }
                let rec = Record {
                    message_id: m.id.clone(),
                    attachment_id: a.id.clone(),
                    posted_at: m.timestamp.clone(),
                    file: rel,
                    width: a.width,
                    height: a.height,
                    content_type: a.content_type.clone(),
                    bot_titles: titles.clone(),
                    label,
                };
                // 中断しても進捗が残るよう1件ずつ書く
                let line = serde_json::to_string(&rec).map_err(|e| e.to_string())?;
                writeln!(index, "{line}")
                    .and_then(|_| index.flush())
                    .map_err(|e| format!("index.jsonl に書けません: {e}"))?;
                seen.insert((m.id.clone(), a.id.clone()));
                saved += 1;
                if saved >= args.limit {
                    break 'pages;
                }
            }
        }
        tokio::time::sleep(PAGE_DELAY).await;
    }

    println!("読んだページ: {pages} / メッセージ: {messages_seen}");
    println!(
        "画像: 保存 {saved} / 既存で飛ばした {skipped} / うち usable なラベル付き {usable_labels}"
    );
    match oldest {
        Some(id) => println!("到達した最古のメッセージID: {id}（続きは --before {id}）"),
        None => println!("メッセージがありませんでした"),
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = parse_args(&args).unwrap_or_else(|e| {
        eprintln!("{e}\n使い方: cargo run --bin collect_recruit_samples -- --channel <ID> [--limit 500] [--max-pages 50] [--before <ID>] [--out local/recruit_samples]");
        std::process::exit(2);
    });
    let token = std::env::var("COLLECT_DISCORD_TOKEN").unwrap_or_default();
    if token.trim().is_empty() {
        eprintln!("環境変数 COLLECT_DISCORD_TOKEN に本番 bot のトークンを入れてください（.env は読みません）");
        std::process::exit(1);
    }
    if let Err(e) = run(parsed, token.trim().to_string()).await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(v: serde_json::Value) -> Msg {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn title_normal() {
        let k = known_tags();
        let t = parse_title("前衛 近距離 上級エリート 火力 召喚", &k);
        assert_eq!(t.tags, ["前衛", "近距離", "上級エリート", "火力", "召喚"]);
        assert!(t.is_global && t.all_known && t.usable);
    }

    #[test]
    fn title_cn_and_markers() {
        let k = known_tags();
        let t = parse_title("前衛 近距離 上級エリート 火力 召喚 (大陸版)", &k);
        assert!(!t.is_global && t.usable);
        let t = parse_title("前衛 近距離 上級エリート 火力 (タグ不足)", &k);
        assert_eq!(t.tags.len(), 4);
        assert!(!t.usable);
        let t = parse_title("前衛 近距離 上級エリート 火力 召喚 生存(タグ過多)", &k);
        assert_eq!(t.tags.len(), 6);
        assert!(!t.usable);
    }

    #[test]
    fn title_unknown_and_error() {
        let k = known_tags();
        let t = parse_title("前衛 近距離 上級エリート 火力 まちがい", &k);
        assert!(!t.all_known && !t.usable);
        let t = parse_title("エラー", &k);
        assert!(!t.usable);
    }

    #[test]
    fn attachment_filter() {
        let a = |name: &str, ct: Option<&str>| Attachment {
            id: "1".into(),
            url: String::new(),
            filename: name.into(),
            content_type: ct.map(str::to_string),
            width: None,
            height: None,
        };
        assert!(is_image_attachment(&a("x.bin", Some("image/png"))));
        assert!(is_image_attachment(&a("x.JPG", None)));
        assert!(!is_image_attachment(&a("x.txt", Some("text/plain"))));
        assert_eq!(pick_ext(&a("x.JPEG", None)), "jpeg");
        assert_eq!(pick_ext(&a("x", Some("image/webp"))), "webp");
    }

    #[test]
    fn assignment_only_when_counts_match() {
        let titles = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            assign_titles(2, &titles),
            vec![Some(&titles[0]), Some(&titles[1])]
        );
        assert_eq!(assign_titles(3, &titles), vec![None, None, None]);
        assert_eq!(assign_titles(1, &[]), vec![None]);
    }

    #[test]
    fn rate_limit() {
        assert_eq!(
            rate_limit_wait(Some("0"), Some("1.5")),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(rate_limit_wait(Some("3"), Some("1.5")), None);
        assert_eq!(rate_limit_wait(None, Some("1.5")), None);
        assert_eq!(rate_limit_wait(Some("0"), Some("abc")), None);
    }

    #[test]
    fn replies_paired_and_ordered() {
        let msgs = vec![
            msg(
                json!({"id":"30","author":{"id":"B"},"embeds":[{"title":"second"}],"message_reference":{"message_id":"10"}}),
            ),
            msg(
                json!({"id":"20","author":{"id":"B"},"embeds":[{"title":"first"},{"description":"no title"}],"message_reference":{"message_id":"10"}}),
            ),
            msg(
                json!({"id":"21","author":{"id":"U"},"embeds":[{"title":"human"}],"message_reference":{"message_id":"10"}}),
            ),
            msg(json!({"id":"22","author":{"id":"B"},"embeds":[{"title":"orphan"}]})),
        ];
        let mut r = HashMap::new();
        collect_replies(&msgs, "B", &mut r);
        assert_eq!(ordered_titles(&r, "10"), ["first", "second"]);
        assert!(ordered_titles(&r, "99").is_empty());
    }

    #[test]
    fn message_deserializes_with_missing_fields() {
        let m = msg(json!({"id":"1","timestamp":"t","author":{"id":"U"},
            "attachments":[{"id":"9","url":"u","filename":"a.png","content_type":"image/png","width":10,"height":20}]}));
        assert_eq!(m.attachments[0].width, Some(10));
        assert!(!m.author.bot);
    }

    #[test]
    fn args() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let a = parse_args(&s(&["--channel", "123", "--limit", "5"])).unwrap();
        assert_eq!((a.channel.as_str(), a.limit, a.max_pages), ("123", 5, 50));
        assert!(parse_args(&s(&[])).is_err());
        assert!(parse_args(&s(&["--channel", "abc"])).is_err());
        assert!(parse_args(&s(&["--channel", "1", "--bogus"])).is_err());
    }
}
