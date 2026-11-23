pub mod conversation;
pub mod functioncalling;
mod membership;
pub mod mention;
mod openai_client;
mod session;
mod tools;

use crate::api::AppState;
use crate::bot::data::{Data, Error};
use crate::bot::reply::EmbedReply;
use conversation::{Conversation, DailyLimit, Enqueue, PendingInput};
use membership::MembershipConfig;
use openai_client::{AttachmentKind, OpenAiClient, UranaiAttachment};
use poise::serenity_prelude as serenity;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// 会話メモリの無操作タイムアウト（10分）。
const SESSION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Discordの1メッセージあたりの文字数上限。
const MAX_MESSAGE_LEN: usize = 2000;
/// 1回の返事で送るメッセージの最大数（AI に誘導されて長文の壁を作らせない）。
const MAX_REPLY_MESSAGES: usize = 2;
const SYSTEM_PROMPT: &str = include_str!("system_prompt.txt");
/// OpenAI Responses APIの1リクエストあたりファイル合計サイズ上限（50MB）に合わせる。
const MAX_ATTACHMENT_TOTAL_BYTES: u64 = conversation::MAX_BATCH_ATTACHMENT_BYTES;
/// AI を呼ぶ回数の1日（JST）の上限の既定値。`CHAT_DAILY_CALL_LIMIT` で変えられる。
const DEFAULT_DAILY_CALL_LIMIT: u32 = 200;
const DAILY_LIMIT_MESSAGE: &str =
    "ごめんなさい、今日はもう星がよく見えないみたい。また明日話しかけてちょうだい。";
const ERROR_MESSAGE: &str = "ごめんなさい、エラーが発生したみたい。";

/// 占い館（OpenAIチャット）のbot起動中ずっと保持する状態。AppStateから共有される。
pub struct UranaiState {
    membership: MembershipConfig,
    client: OpenAiClient,
    /// ユーザー区別なしの単一グローバル会話（要件定義で確定済み）。`.await` をまたいでロックを握らない。
    conversation: Mutex<Conversation>,
    /// AI を呼んだ回数の日次上限（占い館とメンション会話で共有する）。
    daily: Mutex<DailyLimit>,
    /// メンション会話（チャンネルごとの会話）。
    mention: mention::MentionChat,
}

impl UranaiState {
    pub fn from_env() -> Self {
        let daily_limit = match std::env::var("CHAT_DAILY_CALL_LIMIT") {
            Ok(v) => v
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("CHAT_DAILY_CALL_LIMIT is not a number: {v}")),
            Err(_) => DEFAULT_DAILY_CALL_LIMIT,
        };
        Self {
            membership: MembershipConfig::from_env(),
            client: OpenAiClient::from_env(),
            conversation: Mutex::new(Conversation::new(SESSION_TIMEOUT)),
            daily: Mutex::new(DailyLimit::new(daily_limit)),
            mention: mention::MentionChat::from_env(),
        }
    }

    /// 占い館と同じキャラ設定（system_prompt.txt）に`task`を足して、会話履歴なしで
    /// 1回だけアステシアに文章を書いてもらう。占い館の会話セッションには影響しない。
    pub async fn write_in_character(
        &self,
        task: &str,
        user_message: &str,
    ) -> Result<String, Error> {
        let instructions = format!("{SYSTEM_PROMPT}\n\n{task}");
        self.client.generate_text(&instructions, user_message).await
    }

    /// 今日（JST）の AI 呼び出し枠を1回使う。残っていなければ `false`。
    fn try_acquire_daily(&self) -> bool {
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Asia::Tokyo)
            .date_naive();
        lock(&self.daily).try_acquire(today)
    }
}

/// panic で毒された Mutex も中身をそのまま使う（会話の状態は壊れても次の発言で立て直せる）。
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 占い館チャンネルの入口。membershipチェック→会話に積む。考え中でなければワーカーを spawn し、
/// 考え中なら貯めるだけ（返事を送ったあとにまとめて答える）。
/// 実際の返信はワーカーが直接送信するため、呼び出し元への戻り値は常に `Ok(None)`。
pub async fn handle(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
) -> Result<Option<EmbedReply>, Error> {
    if !membership::check(&data.state.uranai.membership, msg) {
        // チャンネル自体がロール制限されている想定の二重チェックなので、無反応でよい
        return Ok(None);
    }
    if !is_conversational(msg.kind, &msg.content, msg.attachments.len()) {
        return Ok(None);
    }

    let uranai = &data.state.uranai;
    let channel_id = msg.channel_id;
    let content_raw = msg.content.trim();

    match session_command(content_raw) {
        Some(SessionCommand::Reset) => {
            lock(&uranai.conversation).reset();
            say(ctx, channel_id, "会話履歴をリセットしたわ。").await;
            return Ok(None);
        }
        Some(SessionCommand::Continue) => {
            lock(&uranai.conversation).touch();
            say(ctx, channel_id, "会話を延長したわ。").await;
            return Ok(None);
        }
        None => {}
    }

    // 添付に非対応形式(画像/PDF以外)やサイズ超過が含まれる場合は、GPTを呼ばずにその場で
    // 打ち切る。会話履歴にも積まない（要件定義で確定済み）。
    let attachments = match classify_attachments(&msg.attachments) {
        Ok(attachments) => attachments,
        Err(rejected) => {
            say(ctx, channel_id, &rejection_message(&rejected)).await;
            return Ok(None);
        }
    };
    if attachments.is_empty() {
        println!("[uranai] GPT accepted");
    } else {
        println!(
            "[uranai] GPT accepted with {} attachment(s)",
            attachments.len()
        );
    }

    let name = conversation::speaker_name(
        msg.member.as_ref().and_then(|m| m.nick.as_deref()),
        msg.author.global_name.as_deref(),
        &msg.author.name,
    );
    let input = PendingInput {
        text: format!("from {name}:\n {content_raw}"),
        attachments,
        reply_to: None,
    };
    let enqueued = lock(&uranai.conversation).enqueue(input);
    match enqueued {
        Enqueue::StartWorker => {
            tokio::spawn(run_worker(ctx.clone(), channel_id, data.state.clone()));
        }
        Enqueue::Queued { dropped } => {
            println!("[uranai] 考え中なので貯めた（上限で捨てた古い発言: {dropped}件）");
        }
    }
    Ok(None)
}

/// AI に渡す発言か。ピン留め・ブースト通知などのシステムメッセージと、本文も添付も無い発言
/// （スタンプだけ等）は渡さない。
fn is_conversational(kind: serenity::MessageType, content: &str, attachment_count: usize) -> bool {
    matches!(
        kind,
        serenity::MessageType::Regular | serenity::MessageType::InlineReply
    ) && (!content.trim().is_empty() || attachment_count > 0)
}

/// ワーカーが panic・早期リターンで抜けたときに busy を下ろす。正常に抜けるときは
/// `take_batch` が busy を下ろしているので `armed = false` にする。
struct WorkerGuard {
    state: Arc<AppState>,
    armed: bool,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if self.armed {
            eprintln!("[uranai] ワーカーが途中で止まったので会話の busy を下ろします");
            lock(&self.state.uranai.conversation).abort_worker();
        }
    }
}

/// 貯まった発言をまとめて AI に渡し、返事を送る。空になるまで繰り返す。
async fn run_worker(ctx: serenity::Context, channel_id: serenity::ChannelId, state: Arc<AppState>) {
    let mut guard = WorkerGuard {
        state: state.clone(),
        armed: true,
    };
    let uranai = &state.uranai;
    loop {
        let (batch, history_input) = {
            let mut conv = lock(&uranai.conversation);
            match conv.take_batch() {
                Some(batch) => {
                    let history = conv.history_input();
                    (batch, history)
                }
                None => break,
            }
        };
        if batch.merged > 1 || batch.dropped_messages > 0 || batch.dropped_attachments > 0 {
            println!(
                "[uranai] {}件をまとめて渡す（落とした発言: {}件・添付: {}件）",
                batch.merged, batch.dropped_messages, batch.dropped_attachments
            );
        }

        if !uranai.try_acquire_daily() {
            println!("[uranai] 1日の呼び出し上限に達しました");
            say(&ctx, channel_id, DAILY_LIMIT_MESSAGE).await;
            continue;
        }

        let typing = channel_id.start_typing(&ctx.http);
        println!("[uranai] ChatGPT start thinking...");
        let think_start = Instant::now();
        let result = uranai
            .client
            .run_turn(
                SYSTEM_PROMPT,
                history_input,
                &batch.text,
                &batch.attachments,
                &state,
            )
            .await;
        println!(
            "[uranai] Thinking completed. ({:.2}秒)",
            think_start.elapsed().as_secs_f64()
        );
        typing.stop();

        let (reply_text, images) = match result {
            Ok(turn) => {
                lock(&uranai.conversation).finish_turn(
                    batch.generation,
                    batch.text,
                    turn.text.clone(),
                );
                (turn.text, turn.images)
            }
            Err(e) => {
                eprintln!("[uranai] OpenAI呼び出しに失敗しました: {e}");
                (ERROR_MESSAGE.to_string(), Vec::new())
            }
        };

        if let Err(e) = send_reply(&ctx, channel_id, &reply_text, images, None).await {
            eprintln!("[uranai] 返信の送信に失敗しました: {e}");
        }
    }
    guard.armed = false;
}

/// 決まった文面を送る（失敗はログだけ）。
async fn say(ctx: &serenity::Context, channel_id: serenity::ChannelId, text: &str) {
    if let Err(e) = channel_id
        .send_message(&ctx.http, no_mention_message(text))
        .await
    {
        eprintln!("[uranai] 返信の送信に失敗しました: {e}");
    }
}

/// `msg.attachments`を画像/PDFに分類する。非対応形式(画像/application-pdf以外)、または
/// 合計サイズがOpenAI側の上限(50MB)を超える添付が1つでもあれば、そのファイル名一覧を
/// `Err`で返す（呼び出し元はGPT呼び出しをせずファイル名を添えた一言で打ち切る）。
fn classify_attachments(
    attachments: &[serenity::Attachment],
) -> Result<Vec<UranaiAttachment>, Vec<String>> {
    let (valid, rejected) = split_attachments(attachments);
    if rejected.is_empty() {
        Ok(valid)
    } else {
        Err(rejected)
    }
}

/// 添付を「渡せるもの」と「渡せないもののファイル名」に分ける。合計サイズが上限を超えたら全部渡せない。
fn split_attachments(attachments: &[serenity::Attachment]) -> (Vec<UranaiAttachment>, Vec<String>) {
    let mut valid = Vec::new();
    let mut unsupported = Vec::new();
    let mut total_size: u64 = 0;

    for a in attachments {
        let is_image = a
            .content_type
            .as_deref()
            .is_some_and(|ct| ct.starts_with("image/"));
        let is_pdf = a.content_type.as_deref() == Some("application/pdf")
            || a.filename.to_lowercase().ends_with(".pdf");

        if is_image {
            total_size += a.size as u64;
            valid.push(UranaiAttachment {
                url: a.url.clone(),
                filename: a.filename.clone(),
                kind: AttachmentKind::Image,
                size: a.size as u64,
            });
        } else if is_pdf {
            total_size += a.size as u64;
            valid.push(UranaiAttachment {
                url: a.url.clone(),
                filename: a.filename.clone(),
                kind: AttachmentKind::Pdf,
                size: a.size as u64,
            });
        } else {
            unsupported.push(a.filename.clone());
        }
    }

    if total_size > MAX_ATTACHMENT_TOTAL_BYTES {
        unsupported.extend(valid.into_iter().map(|a| a.filename));
        return (Vec::new(), unsupported);
    }
    (valid, unsupported)
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
/// `reply_to` があれば1通目だけその発言へのリプライにする（相手が消していても送れるように
/// `fail_if_not_exists: false`、リプライのピンは飛ばさない）。
async fn send_reply(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    text: &str,
    images: Vec<Vec<u8>>,
    reply_to: Option<serenity::MessageId>,
) -> Result<(), Error> {
    let mut chunks = chunk_text(text, MAX_MESSAGE_LEN);
    chunks.retain(|c| !c.trim().is_empty());
    chunks.truncate(MAX_REPLY_MESSAGES);
    if chunks.is_empty() && !images.is_empty() {
        chunks.push(String::new());
    }
    if chunks.is_empty() {
        return Ok(());
    }

    let named_images: Vec<(String, Vec<u8>)> = images
        .into_iter()
        .enumerate()
        .map(|(i, bytes)| (format!("astesia_{i}.png"), bytes))
        .collect();

    if named_images.is_empty() {
        println!("[uranai] 送信: channel={channel_id} (添付なし)");
    } else {
        let names: Vec<&str> = named_images.iter().map(|(n, _)| n.as_str()).collect();
        println!(
            "[uranai] 送信: channel={channel_id} 添付=[{}]",
            names.join(", ")
        );
    }

    let mut attachments = Some(to_attachments(named_images)).filter(|a| !a.is_empty());
    let mut reference = reply_to.map(|id| reply_reference(channel_id, id));
    for chunk in chunks {
        let mut builder = no_mention_message(chunk);
        if let Some(r) = reference.take() {
            builder = builder.reference_message(r);
        }
        if let Some(files) = attachments.take() {
            channel_id.send_files(&ctx.http, files, builder).await?;
        } else {
            channel_id.send_message(&ctx.http, builder).await?;
        }
    }
    Ok(())
}

/// 占い館の返信メッセージを組み立てる。本文はGPTの出力や添付ファイル名など、メンバーが
/// 内容を左右できる文字列なので、`@everyone`/`@here`・ロール・ユーザーのメンションを
/// 一切通知させない（Discordはbot自身の権限でメンションを解決するため、そのままだと
/// メンバーがbot経由で全体通知やロール通知を飛ばせてしまう）。本文の表示は変えない。
fn no_mention_message(content: impl Into<String>) -> serenity::CreateMessage {
    serenity::CreateMessage::new()
        .content(content)
        .allowed_mentions(serenity::CreateAllowedMentions::new().replied_user(false))
}

/// 発言へのリプライの参照。発言が消されていてもエラーにせずふつうに送る。
fn reply_reference(
    channel_id: serenity::ChannelId,
    message_id: serenity::MessageId,
) -> serenity::MessageReference {
    let mut r = serenity::MessageReference::from((channel_id, message_id));
    r.fail_if_not_exists = Some(false);
    r
}

fn to_attachments(named_images: Vec<(String, Vec<u8>)>) -> Vec<serenity::CreateAttachment> {
    named_images
        .into_iter()
        .map(|(name, bytes)| serenity::CreateAttachment::bytes(bytes, name))
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

/// 会話履歴を操作する即時コマンド。
#[derive(Debug, PartialEq, Eq)]
enum SessionCommand {
    Reset,
    Continue,
}

/// 発言本文（trim 済み）が即時コマンドか。`from {name}:` を付けた GPT 向けの文字列ではなく、
/// 生の本文と比べる（付けた後だと永久に一致しない）。
fn session_command(raw: &str) -> Option<SessionCommand> {
    match raw.trim() {
        "reset" | "clear" => Some(SessionCommand::Reset),
        "continue" => Some(SessionCommand::Continue),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_commands_match_raw_content_only() {
        assert_eq!(session_command("reset"), Some(SessionCommand::Reset));
        assert_eq!(
            session_command(
                " clear 
"
            ),
            Some(SessionCommand::Reset)
        );
        assert_eq!(session_command("continue"), Some(SessionCommand::Continue));
        assert_eq!(session_command("reset please"), None);
        assert_eq!(
            session_command(
                "from bob:
 reset"
            ),
            None
        );
    }

    #[test]
    fn system_messages_and_empty_messages_are_not_conversational() {
        use serenity::MessageType;
        assert!(is_conversational(MessageType::Regular, "こんにちは", 0));
        assert!(is_conversational(MessageType::InlineReply, "返信", 0));
        assert!(is_conversational(MessageType::Regular, "", 1));
        assert!(!is_conversational(MessageType::Regular, "  ", 0));
        assert!(!is_conversational(MessageType::PinsAdd, "", 0));
        assert!(!is_conversational(MessageType::NitroBoost, "x", 0));
    }

    #[test]
    fn no_mention_message_suppresses_every_mention_kind() {
        let text = "@everyone @here <@&123> <@456>";
        let json = serde_json::to_value(no_mention_message(text)).unwrap();
        assert_eq!(json["content"], text);
        assert_eq!(
            json["allowed_mentions"],
            serde_json::json!({ "parse": [], "users": [], "roles": [], "replied_user": false })
        );
    }
}
