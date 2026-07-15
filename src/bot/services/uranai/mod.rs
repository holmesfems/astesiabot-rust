pub mod functioncalling;
mod membership;
mod openai_client;
mod session;
mod tools;

use crate::bot::data::{Data, Error};
use crate::bot::reply::EmbedReply;
use membership::MembershipConfig;
use openai_client::OpenAiClient;
use poise::serenity_prelude as serenity;
use session::ChatSession;
use std::time::Duration;

/// 会話メモリの無操作タイムアウト（10分）。
const SESSION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Discordの1メッセージあたりの文字数上限。
const MAX_MESSAGE_LEN: usize = 2000;
const SYSTEM_PROMPT: &str = include_str!("system_prompt.txt");

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

        if session.is_expired(SESSION_TIMEOUT) {
            session.reset();
        }
        let history_input = session.history_input();

        let typing = channel_id.start_typing(&ctx.http);
        let result = uranai
            .client
            .run_turn(SYSTEM_PROMPT, history_input, &content, &state)
            .await;
        typing.stop();

        let reply_text = match result {
            Ok(text) => {
                session.push_exchange(content, text.clone());
                text
            }
            Err(e) => {
                eprintln!("[uranai] OpenAI呼び出しに失敗しました: {e}");
                "ごめんなさい、エラーが発生したみたい。".to_string()
            }
        };
        drop(session);

        if let Err(e) = send_plain_text(&ctx, channel_id, &reply_text).await {
            eprintln!("[uranai] 返信の送信に失敗しました: {e}");
        }
    });

    Ok(None)
}

/// 自然文を Discord の1メッセージ文字数上限に収まるよう分割して送信する。
async fn send_plain_text(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    text: &str,
) -> Result<(), Error> {
    if text.is_empty() {
        return Ok(());
    }
    for chunk in chunk_text(text, MAX_MESSAGE_LEN) {
        channel_id.say(&ctx.http, chunk).await?;
    }
    Ok(())
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
