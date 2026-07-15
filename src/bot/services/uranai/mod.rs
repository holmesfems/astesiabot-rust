pub mod functioncalling;
mod membership;
mod openai_client;
mod session;
mod tools;

use crate::bot::data::{Data, Error};
use crate::bot::reply::EmbedReply;
use membership::MembershipConfig;
use openai_client::{AttachmentKind, OpenAiClient, UranaiAttachment};
use poise::serenity_prelude as serenity;
use session::ChatSession;
use std::time::Duration;

/// 会話メモリの無操作タイムアウト（10分）。
const SESSION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Discordの1メッセージあたりの文字数上限。
const MAX_MESSAGE_LEN: usize = 2000;
const SYSTEM_PROMPT: &str = include_str!("system_prompt.txt");
/// OpenAI Responses APIの1リクエストあたりファイル合計サイズ上限（50MB）に合わせる。
const MAX_ATTACHMENT_TOTAL_BYTES: u64 = 50 * 1024 * 1024;

/// 占い館（OpenAIチャット）のbot起動中ずっと保持する状態。AppStateから共有される。
pub struct UranaiState {
    membership: MembershipConfig,
    client: OpenAiClient,
    /// ユーザー区別なしの単一グローバル会話セッション（要件定義で確定済み）。
    session: tokio::sync::Mutex<ChatSession>,
}

impl UranaiState {
    pub fn from_env() -> Self {
        Self {
            membership: MembershipConfig::from_env(),
            client: OpenAiClient::from_env(),
            session: tokio::sync::Mutex::new(ChatSession::new()),
        }
    }
}

/// 占い館チャンネルの入口。membershipチェック→OpenAI呼び出しをバックグラウンドで行う。
/// 実際の返信はこの関数内（正確には spawn したタスク内）で直接送信するため、
/// 呼び出し元への戻り値は常に `Ok(None)`。
pub async fn handle(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
) -> Result<Option<EmbedReply>, Error> {
    if !membership::check(&data.state.uranai.membership, msg) {
        // チャンネル自体がロール制限されている想定の二重チェックなので、無反応でよい
        return Ok(None);
    }

    let ctx = ctx.clone();
    let channel_id = msg.channel_id;
    let content = msg.content.trim().to_string();
    let raw_attachments = msg.attachments.clone();
    let state = data.state.clone();

    tokio::spawn(async move {
        let uranai = &state.uranai;
        // 既にリクエスト処理中（GPTが思考中）なら、新規メッセージはキューイングせず無視する。
        // ロックは reset/continue の即時応答も含め、この処理が終わるまで保持し続ける。
        let Ok(mut session) = uranai.session.try_lock() else {
            return;
        };

        if content == "reset" || content == "clear" {
            session.reset();
            drop(session);
            if let Err(e) = channel_id.say(&ctx.http, "会話履歴をリセットしたわ。").await {
                eprintln!("[uranai] 返信の送信に失敗しました: {e}");
            }
            return;
        }
        if content == "continue" {
            session.touch();
            drop(session);
            if let Err(e) = channel_id.say(&ctx.http, "会話を延長したわ。").await {
                eprintln!("[uranai] 返信の送信に失敗しました: {e}");
            }
            return;
        }

        // 添付に非対応形式(画像/PDF以外)やサイズ超過が含まれる場合は、GPTを呼ばずにその場で
        // 打ち切る。会話履歴にも積まない（要件定義で確定済み）。
        let attachments = match classify_attachments(&raw_attachments) {
            Ok(attachments) => attachments,
            Err(rejected) => {
                drop(session);
                if let Err(e) = channel_id.say(&ctx.http, rejection_message(&rejected)).await {
                    eprintln!("[uranai] 返信の送信に失敗しました: {e}");
                }
                return;
            }
        };

        if session.is_expired(SESSION_TIMEOUT) {
            session.reset();
        }
        let history_input = session.history_input();

        let typing = channel_id.start_typing(&ctx.http);
        let result = uranai
            .client
            .run_turn(SYSTEM_PROMPT, history_input, &content, &attachments, &state)
            .await;
        typing.stop();

        let (reply_text, images) = match result {
            Ok(turn) => {
                session.push_exchange(content, turn.text.clone());
                (turn.text, turn.images)
            }
            Err(e) => {
                eprintln!("[uranai] OpenAI呼び出しに失敗しました: {e}");
                ("ごめんなさい、エラーが発生したみたい。".to_string(), Vec::new())
            }
        };
        drop(session);

        if let Err(e) = send_reply(&ctx, channel_id, &reply_text, images).await {
            eprintln!("[uranai] 返信の送信に失敗しました: {e}");
        }
    });

    Ok(None)
}

/// `msg.attachments`を画像/PDFに分類する。非対応形式(画像/application-pdf以外)、または
/// 合計サイズがOpenAI側の上限(50MB)を超える添付が1つでもあれば、そのファイル名一覧を
/// `Err`で返す（呼び出し元はGPT呼び出しをせずファイル名を添えた一言で打ち切る）。
fn classify_attachments(attachments: &[serenity::Attachment]) -> Result<Vec<UranaiAttachment>, Vec<String>> {
    let mut valid = Vec::new();
    let mut unsupported = Vec::new();
    let mut total_size: u64 = 0;

    for a in attachments {
        let is_image = a.content_type.as_deref().is_some_and(|ct| ct.starts_with("image/"));
        let is_pdf = a.content_type.as_deref() == Some("application/pdf")
            || a.filename.to_lowercase().ends_with(".pdf");

        if is_image {
            total_size += a.size as u64;
            valid.push(UranaiAttachment {
                url: a.url.clone(),
                filename: a.filename.clone(),
                kind: AttachmentKind::Image,
            });
        } else if is_pdf {
            total_size += a.size as u64;
            valid.push(UranaiAttachment {
                url: a.url.clone(),
                filename: a.filename.clone(),
                kind: AttachmentKind::Pdf,
            });
        } else {
            unsupported.push(a.filename.clone());
        }
    }

    if total_size > MAX_ATTACHMENT_TOTAL_BYTES {
        unsupported.extend(valid.into_iter().map(|a| a.filename));
        return Err(unsupported);
    }

    if unsupported.is_empty() {
        Ok(valid)
    } else {
        Err(unsupported)
    }
}

/// アステシアの口調で、非対応だった添付ファイル名を添えて一言返す。
fn rejection_message(rejected_filenames: &[String]) -> String {
    format!(
        "ごめんなさい、{}は読めないみたい。画像かPDFにしてもらえるかしら？",
        rejected_filenames.join("、")
    )
}

/// 自然文を Discord の1メッセージ文字数上限に収まるよう分割して送信する。
/// `images`（GPTが`image_generation`ツールで生成したPNGバイト列）があれば、最初のチャンクに
/// まとめて添付する。
async fn send_reply(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    text: &str,
    images: Vec<Vec<u8>>,
) -> Result<(), Error> {
    let mut chunks = chunk_text(text, MAX_MESSAGE_LEN);
    if chunks.is_empty() && !images.is_empty() {
        chunks.push(String::new());
    }
    if chunks.is_empty() {
        return Ok(());
    }

    let mut attachments = Some(to_attachments(images)).filter(|a| !a.is_empty());
    for chunk in chunks {
        if let Some(files) = attachments.take() {
            channel_id
                .send_files(&ctx.http, files, serenity::CreateMessage::new().content(chunk))
                .await?;
        } else {
            channel_id.say(&ctx.http, chunk).await?;
        }
    }
    Ok(())
}

fn to_attachments(images: Vec<Vec<u8>>) -> Vec<serenity::CreateAttachment> {
    images
        .into_iter()
        .enumerate()
        .map(|(i, bytes)| serenity::CreateAttachment::bytes(bytes, format!("astesia_{i}.png")))
        .collect()
}

fn chunk_text(text: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= limit {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + limit).min(chars.len());
        chunks.push(chars[start..end].iter().collect());
        start = end;
    }
    chunks
}
