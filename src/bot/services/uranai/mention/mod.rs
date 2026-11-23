//! メンション会話: 鯖のどのチャンネルでも、アステシアにメンションすると会話が始まる。
//! 状態遷移は `state.rs`（純粋ロジック）、ここは Discord との出入りとワーカー。
//! OpenAI クライアント・キャラ設定・添付の分類・送信・日次上限は占い館（親モジュール）と共有する。

mod state;

use super::conversation::{speaker_name, PendingInput};
use super::openai_client::ReplyAction;
use super::{lock, membership};
use crate::api::AppState;
use crate::bot::data::{Data, Error};
use crate::bot::services::moderation;
use poise::serenity_prelude as serenity;
use state::{
    CloseReason, FinishAction, Incoming, IncomingAction, Limits, MentionSessions, Notice, Outcome,
    WorkerStep,
};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const MENTION_PROMPT: &str = include_str!("prompt.txt");
const NO_ROLE_MESSAGE: &str =
    "ごめんなさい、ここでのおしゃべりはサーバーブースターかYouTubeメンバーのドクター限定なの。占い館で待っているわね。";
const FULL_MESSAGE: &str =
    "ごめんなさい、今は他のドクターとお話し中で手が離せないの。少し待ってちょうだい。";
const RESET_MESSAGE: &str = "わかったわ。またね、ドクター。";
const CALL_LIMIT_MESSAGE: &str =
    "たくさん話せて楽しかったわ。そろそろ星を見に戻るわね。また呼んでちょうだい、ドクター。";
const ERRORS_MESSAGE: &str = "ごめんなさい、今日は星の巡りが悪いみたい。また後で呼んでちょうだい。";
/// メンションだけの発言（本文が空）を AI に渡すときの本文。
const CALL_ONLY_TEXT: &str = "（アステシアに呼びかけた）";

pub struct MentionChat {
    sessions: Mutex<MentionSessions>,
    /// `CHANNEL_IDS_MENTION_CHAT_DENY`（任意・カンマ区切り）。スレッドは親チャンネルでも照合する。
    deny_channels: HashSet<serenity::ChannelId>,
}

impl MentionChat {
    pub fn from_env() -> Self {
        let deny_channels = match std::env::var("CHANNEL_IDS_MENTION_CHAT_DENY") {
            Ok(v) => parse_channel_ids(&v).unwrap_or_else(|bad| {
                panic!("CHANNEL_IDS_MENTION_CHAT_DENY has an invalid id: {bad}")
            }),
            Err(_) => HashSet::new(),
        };
        Self {
            sessions: Mutex::new(MentionSessions::new(Limits::default())),
            deny_channels,
        }
    }
}

fn parse_channel_ids(v: &str) -> Result<HashSet<serenity::ChannelId>, String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<u64>()
                .ok()
                .filter(|&id| id != 0)
                .map(serenity::ChannelId::new)
                .ok_or_else(|| s.to_string())
        })
        .collect()
}

/// メンション会話の入口。占い館・公開求人のチャンネルには来ない（handler がその前に振り分ける）が、
/// その中のスレッドは来るので、`excluded` に渡されたチャンネルを親として照合する。
pub async fn handle(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
    excluded: &[serenity::ChannelId],
) -> Result<(), Error> {
    let uranai = &data.state.uranai;
    let mention = &uranai.mention;
    // 1. システムメッセージ・スタンプだけ等、DM・別の鯖は相手にしない（ロールの案内も出さない）。
    if !super::is_conversational(msg.kind, &msg.content, msg.attachments.len())
        || !membership::in_guild(&uranai.membership, msg)
    {
        return Ok(());
    }
    let bot_id = ctx.cache.current_user().id;
    // Discord の入力補完では bot 本人のほかに、bot に自動で付くロール（同じような名前）も候補に出る。
    // ロールのほうを選んでも呼び出しとして扱う。
    let bot_role = bot_managed_role(ctx, msg.guild_id, bot_id);
    let mentions_bot = (msg.mentions.iter().any(|u| u.id == bot_id)
        && content_mentions_user(&msg.content, bot_id.get()))
        || bot_role.is_some_and(|r| {
            msg.mention_roles.contains(&r) && content_mentions_role(&msg.content, r.get())
        });
    let channel_id = msg.channel_id;

    // 2. 会話の無いチャンネルでは、メンションだけを、除外チャンネル以外で受ける
    //    （チャンネルの取得は HTTP になりうるので、ここまで絞ってから）。
    if !lock(&mention.sessions).has_session(channel_id.get()) {
        if !mentions_bot {
            return Ok(());
        }
        if !channel_allowed(ctx, channel_id, excluded, &mention.deny_channels).await {
            return Ok(());
        }
    }

    // 3. AI に渡す形に整える。
    let names: Vec<(u64, String)> = msg
        .mentions
        .iter()
        .map(|u| {
            let nick = u.member.as_ref().and_then(|m| m.nick.as_deref());
            (
                u.id.get(),
                speaker_name(nick, u.global_name.as_deref(), &u.name),
            )
        })
        .collect();
    let body = clean_content(
        &msg.content,
        bot_id.get(),
        bot_role.map(|r| r.get()),
        &names,
    );
    let is_reset = mentions_bot && matches!(body.as_str(), "reset" | "clear");
    let (attachments, rejected) = super::split_attachments(&msg.attachments);
    let body = if !body.is_empty() || !attachments.is_empty() {
        body
    } else if mentions_bot {
        CALL_ONLY_TEXT.to_string()
    } else {
        // 渡せない添付だけの発言（会話相手でない人への動画など）は黙って見送る。
        return Ok(());
    };
    let name = speaker_name(
        msg.member.as_ref().and_then(|m| m.nick.as_deref()),
        msg.author.global_name.as_deref(),
        &msg.author.name,
    );
    let incoming = Incoming {
        channel: channel_id.get(),
        user: msg.author.id.get(),
        mentions_bot,
        has_paid_role: membership::check(&uranai.membership, msg),
        is_admin: moderation::user_is_admin(ctx, msg),
        is_reset,
        input: PendingInput {
            text: format!("from {name}:\n {body}"),
            attachments,
            reply_to: Some(msg.id.get()),
        },
    };

    let action = lock(&mention.sessions).on_message(incoming, Instant::now());
    let accepted = matches!(
        action,
        IncomingAction::StartWorker { .. } | IncomingAction::Queued
    );
    match action {
        IncomingAction::Ignore | IncomingAction::Queued => {}
        IncomingAction::Notice(notice) => {
            let text = match notice {
                Notice::NoRole => NO_ROLE_MESSAGE,
                Notice::Full => FULL_MESSAGE,
            };
            send_fixed(ctx, channel_id, text, Some(msg.id)).await;
        }
        IncomingAction::Closed => {
            println!("[mention_chat] 閉じた: channel={channel_id} 理由=reset");
            send_fixed(ctx, channel_id, RESET_MESSAGE, Some(msg.id)).await;
        }
        IncomingAction::StartWorker {
            session_id,
            started,
        } => {
            if started {
                println!("[mention_chat] 会話を始めた: channel={channel_id} session={session_id}");
            }
            tokio::spawn(run_worker(
                ctx.clone(),
                channel_id,
                session_id,
                data.state.clone(),
            ));
        }
    }
    // 読めない添付の一言は、アステシアに向けた発言で、会話に受け付けたときだけ。
    if accepted && mentions_bot && !rejected.is_empty() {
        send_fixed(
            ctx,
            channel_id,
            &super::rejection_message(&rejected),
            Some(msg.id),
        )
        .await;
    }
    Ok(())
}

/// 除外チャンネル（占い館・公開求人・`CHANNEL_IDS_MENTION_CHAT_DENY`）でないか。スレッドは親も見る
/// （ふつうのチャンネルの親はカテゴリなので、カテゴリの ID を DENY に書けばカテゴリごと止められる）。
async fn channel_allowed(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    excluded: &[serenity::ChannelId],
    deny: &HashSet<serenity::ChannelId>,
) -> bool {
    let denied = |id: serenity::ChannelId| excluded.contains(&id) || deny.contains(&id);
    if denied(channel_id) {
        return false;
    }
    match channel_id.to_channel(ctx).await {
        Ok(serenity::Channel::Guild(gc)) => !gc.parent_id.is_some_and(denied),
        Ok(_) => false,
        Err(e) => {
            eprintln!("[mention_chat] チャンネルの取得に失敗しました: {e}");
            false
        }
    }
}

/// ワーカーが panic・早期リターンで抜けたときに busy を下ろす。
struct WorkerGuard {
    state: Arc<AppState>,
    channel: u64,
    session_id: u64,
    armed: bool,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if self.armed {
            eprintln!(
                "[mention_chat] ワーカーが途中で止まったので busy を下ろします: channel={}",
                self.channel
            );
            lock(&self.state.uranai.mention.sessions).abort_worker(self.channel, self.session_id);
        }
    }
}

async fn run_worker(
    ctx: serenity::Context,
    channel_id: serenity::ChannelId,
    session_id: u64,
    state: Arc<AppState>,
) {
    let channel = channel_id.get();
    let mut guard = WorkerGuard {
        state: state.clone(),
        channel,
        session_id,
        armed: true,
    };
    let uranai = &state.uranai;
    let sessions = &uranai.mention.sessions;
    let instructions = format!("{}\n\n{MENTION_PROMPT}", super::SYSTEM_PROMPT);
    loop {
        let step = lock(sessions).take_batch(channel, session_id);
        let (batch, history, allow_image) = match step {
            WorkerStep::Done => break,
            WorkerStep::Closed(reason) => {
                log_close(channel_id, reason);
                if let Some(text) = close_message(reason) {
                    send_fixed(&ctx, channel_id, text, None).await;
                }
                break;
            }
            WorkerStep::Run {
                batch,
                history,
                allow_image,
            } => (batch, history, allow_image),
        };
        if batch.merged > 1 || batch.dropped_messages > 0 || batch.dropped_attachments > 0 {
            println!(
                "[mention_chat] {}件をまとめて渡す（落とした発言: {}件・添付: {}件）",
                batch.merged, batch.dropped_messages, batch.dropped_attachments
            );
        }
        let reply_to = batch.reply_to.map(serenity::MessageId::new);

        if !uranai.try_acquire_daily() {
            lock(sessions).close(channel, session_id);
            log_close(channel_id, CloseReason::DailyLimit);
            send_fixed(&ctx, channel_id, super::DAILY_LIMIT_MESSAGE, reply_to).await;
            break;
        }

        let typing = channel_id.start_typing(&ctx.http);
        let think_start = Instant::now();
        let result = uranai
            .client
            .run_structured_turn(
                &instructions,
                history,
                &batch.text,
                &batch.attachments,
                allow_image,
                &state,
            )
            .await;
        typing.stop();

        let (outcome, images) = match result {
            Ok(turn) => {
                let made_image = !turn.images.is_empty();
                let message = turn.reply.message;
                let outcome = match turn.reply.action {
                    // 画像を作ったのに silent なら、お金を払った画像を捨てずに送る。
                    ReplyAction::Reply | ReplyAction::Silent if made_image => Outcome::Reply {
                        message,
                        made_image,
                    },
                    ReplyAction::Reply => Outcome::Reply {
                        message,
                        made_image,
                    },
                    ReplyAction::Silent => Outcome::Silent,
                    ReplyAction::End => Outcome::End { message },
                };
                (outcome, turn.images)
            }
            Err(e) => {
                eprintln!("[mention_chat] OpenAI呼び出しに失敗しました: {e}");
                (Outcome::Error, Vec::new())
            }
        };
        println!(
            "[mention_chat] channel={channel_id} {} ({:.2}秒)",
            outcome_label(&outcome),
            think_start.elapsed().as_secs_f64()
        );

        let action =
            lock(sessions).finish(channel, session_id, batch.generation, batch.text, outcome);
        let sent = match action {
            FinishAction::Nothing => Ok(()),
            FinishAction::Send(text) => {
                super::send_reply(&ctx, channel_id, &text, images, reply_to).await
            }
            FinishAction::SendError => {
                super::send_reply(&ctx, channel_id, super::ERROR_MESSAGE, Vec::new(), reply_to)
                    .await
            }
            FinishAction::SendAndClosed(text, reason) => {
                log_close(channel_id, reason);
                super::send_reply(&ctx, channel_id, &text, images, reply_to).await
            }
            FinishAction::Closed(reason) => {
                log_close(channel_id, reason);
                match close_message(reason) {
                    Some(text) => {
                        super::send_reply(&ctx, channel_id, text, Vec::new(), reply_to).await
                    }
                    None => Ok(()),
                }
            }
        };
        if let Err(e) = sent {
            eprintln!("[mention_chat] 返信の送信に失敗しました: {e}");
            if is_unrecoverable_send_error(&e) {
                lock(sessions).close(channel, session_id);
                log_close(channel_id, CloseReason::SendFailed);
                break;
            }
        }
    }
    guard.armed = false;
}

/// 決まった文面を送る（失敗はログだけ）。
async fn send_fixed(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    text: &str,
    reply_to: Option<serenity::MessageId>,
) {
    if let Err(e) = super::send_reply(ctx, channel_id, text, Vec::new(), reply_to).await {
        eprintln!("[mention_chat] 返信の送信に失敗しました: {e}");
    }
}

/// 権限が無い（403）・チャンネルが消えた（404）なら、続けても送れないので会話を閉じる。
fn is_unrecoverable_send_error(e: &Error) -> bool {
    match e.downcast_ref::<serenity::Error>() {
        Some(serenity::Error::Http(serenity::HttpError::UnsuccessfulRequest(r))) => {
            matches!(r.status_code.as_u16(), 403 | 404)
        }
        _ => false,
    }
}

fn close_message(reason: CloseReason) -> Option<&'static str> {
    match reason {
        CloseReason::CallLimit => Some(CALL_LIMIT_MESSAGE),
        CloseReason::Errors => Some(ERRORS_MESSAGE),
        CloseReason::DailyLimit => Some(super::DAILY_LIMIT_MESSAGE),
        // End は AI のお別れを送る。silent 続き・送信失敗は黙って閉じる。
        CloseReason::End | CloseReason::Silent | CloseReason::SendFailed => None,
    }
}

fn log_close(channel_id: serenity::ChannelId, reason: CloseReason) {
    println!("[mention_chat] 閉じた: channel={channel_id} 理由={reason:?}");
}

fn outcome_label(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Reply {
            made_image: true, ..
        } => "reply+image",
        Outcome::Reply { .. } => "reply",
        Outcome::Silent => "silent",
        Outcome::End { .. } => "end",
        Outcome::Error => "error",
    }
}

/// bot に自動で付く（integration の）ロール。キャッシュに鯖が無ければ `None`。
fn bot_managed_role(
    ctx: &serenity::Context,
    guild_id: Option<serenity::GuildId>,
    bot_id: serenity::UserId,
) -> Option<serenity::RoleId> {
    let guild = ctx.cache.guild(guild_id?)?;
    guild
        .roles
        .values()
        .find(|r| r.tags.bot_id == Some(bot_id))
        .map(|r| r.id)
}

/// 本文に `<@&id>` があるか。
fn content_mentions_role(content: &str, role_id: u64) -> bool {
    content.contains(&format!("<@&{role_id}>"))
}

/// 本文に `<@id>` か `<@!id>` があるか（リプライの自動メンションは本文に出ないので、これで区別する）。
fn content_mentions_user(content: &str, user_id: u64) -> bool {
    content.contains(&format!("<@{user_id}>")) || content.contains(&format!("<@!{user_id}>"))
}

/// AI に渡す本文: bot（とその自動ロール）へのメンションは消し、他の人へのメンションは `@表示名` にする。
/// 前後の空白は削る。
fn clean_content(
    content: &str,
    bot_id: u64,
    bot_role: Option<u64>,
    names: &[(u64, String)],
) -> String {
    let mut s = content
        .replace(&format!("<@{bot_id}>"), "")
        .replace(&format!("<@!{bot_id}>"), "");
    if let Some(role) = bot_role {
        s = s.replace(&format!("<@&{role}>"), "");
    }
    for (id, name) in names {
        if *id == bot_id {
            continue;
        }
        let at = format!("@{name}");
        s = s
            .replace(&format!("<@{id}>"), &at)
            .replace(&format!("<@!{id}>"), &at);
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT: u64 = 999;

    #[test]
    fn mention_in_content_is_required() {
        assert!(content_mentions_user("<@999> こんにちは", BOT));
        assert!(content_mentions_user("ねえ <@!999>", BOT));
        // リプライの自動メンションは本文に出ない
        assert!(!content_mentions_user("こんにちは", BOT));
        assert!(!content_mentions_user("<@9990>", BOT));
        // ロールのメンションはユーザーのメンションとは別
        assert!(!content_mentions_user("<@&999>", BOT));
        assert!(content_mentions_role("<@&555> はろー！", 555));
        assert!(!content_mentions_role("<@555>", 555));
    }

    #[test]
    fn clean_content_removes_bot_role_mention() {
        assert_eq!(
            clean_content("<@&555>\nはろー！", BOT, Some(555), &[]),
            "はろー！"
        );
        // 他のロールのメンションは残す
        assert_eq!(
            clean_content("<@&556> どう？", BOT, Some(555), &[]),
            "<@&556> どう？"
        );
    }

    #[test]
    fn clean_content_removes_bot_and_names_others() {
        let names = vec![
            (BOT, "アステシア".to_string()),
            (42, "ドクターA".to_string()),
        ];
        assert_eq!(
            clean_content("<@999> <@42> と話してたの", BOT, None, &names),
            "@ドクターA と話してたの"
        );
        assert_eq!(clean_content("<@!999>", BOT, None, &names), "");
        assert_eq!(clean_content("  reset <@999> ", BOT, None, &names), "reset");
        // 知らない ID はそのまま
        assert_eq!(clean_content("<@7>", BOT, None, &names), "<@7>");
    }

    #[test]
    fn parse_deny_list() {
        let ids = parse_channel_ids(" 1, 2 ,,3").unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&serenity::ChannelId::new(2)));
        assert_eq!(parse_channel_ids("1,abc"), Err("abc".to_string()));
        assert_eq!(parse_channel_ids("0"), Err("0".to_string()));
        assert!(parse_channel_ids("").unwrap().is_empty());
    }

    #[test]
    fn close_messages() {
        assert!(close_message(CloseReason::Silent).is_none());
        assert!(close_message(CloseReason::End).is_none());
        assert_eq!(
            close_message(CloseReason::CallLimit),
            Some(CALL_LIMIT_MESSAGE)
        );
    }
}
