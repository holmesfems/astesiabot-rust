//! Discord のモデレーション（スパム検知と処断）。入口と状態はここ、検知は [`detect`]、処断は [`action`]。

use crate::bot::data::Error;
use crate::bot::utils::channel_id_env;
use fancy_regex::Regex;
use poise::serenity_prelude as serenity;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

mod detect;
mod action;
#[cfg(test)]
mod tests;

use detect::{
    URL_PATTERN,
    SpamKind,
    Actor,
    command_invoker,
    Detection,
    first_sight_of_interaction,
    detect,
    detect_rate_spam,
};
use action::dispatch;

/// bot 起動中ずっと保持するモデレーション用の可変状態。AppState から共有される。
pub struct ModerationState {
    autodel_channels: [serenity::ChannelId; 6],
    report_channel: serenity::ChannelId,
    url_re: Regex,
    multi_ch_history: Mutex<HashMap<serenity::UserId, VecDeque<(Instant, serenity::ChannelId)>>>,
    same_ch_history:
        Mutex<HashMap<(serenity::UserId, serenity::ChannelId), VecDeque<Instant>>>,
    /// 数え済みのスラッシュコマンド。1つのコマンドへの応答＋フォローアップを1回と数えるため
    seen_interactions: Mutex<HashMap<serenity::InteractionId, Instant>>,
}

impl ModerationState {
    pub fn from_env() -> Self {
        let autodel_channels = [
            channel_id_env("AUTODEL_1"),
            channel_id_env("AUTODEL_2"),
            channel_id_env("AUTODEL_3"),
            channel_id_env("AUTODEL_4"),
            channel_id_env("AUTODEL_5"),
            channel_id_env("AUTODEL_6"),
        ];
        let report_channel = channel_id_env("REPORT_CHANNEL_ID");
        let url_re = Regex::new(URL_PATTERN).expect("URL_PATTERN must compile");
        Self {
            autodel_channels,
            report_channel,
            url_re,
            multi_ch_history: Mutex::new(HashMap::new()),
            same_ch_history: Mutex::new(HashMap::new()),
            seen_interactions: Mutex::new(HashMap::new()),
        }
    }
}

/// 発言者が ADMINISTRATOR 権限を持つか判定する。DM やメンバー情報未取得なら false。
/// メッセージに同梱される PartialMember から計算するため、追加のAPI呼び出しは発生しない。
pub fn user_is_admin(ctx: &serenity::Context, msg: &serenity::Message) -> bool {
    let Some(guild_id) = msg.guild_id else {
        return false;
    };
    let Some(partial_member) = msg.member.as_deref() else {
        return false;
    };
    let Some(guild) = ctx.cache.guild(guild_id) else {
        return false;
    };
    guild
        .partial_member_permissions(msg.author.id, partial_member)
        .administrator()
}

/// モデレーションの入口。罠チャンネル削除・全体通知BAN・連投/爆撃検知を検知層(detect)で
/// 一括判定し、ディスパッチ層(dispatch)へ渡す。true を返したら、このメッセージは
/// 処断済み（呼び出し側は以降の処理を打ち切ってよい）。
pub async fn handle(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
    user_is_admin: bool,
) -> Result<bool, Error> {
    let detection = detect(state, msg);
    dispatch(ctx, msg, state, Actor::author_of(msg), detection, user_is_admin).await
}

/// bot の発言のうち、スラッシュコマンドへの返信だけを「コマンドを打った人の操作」として
/// 連投/爆撃検知に数える（人間本人の発言と同じ履歴・同じ閾値）。罠チャンネル・全体通知は
/// 人間本人の発言だけが対象なので、ここでは見ない。
pub async fn handle_command_reply(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    state: &ModerationState,
) -> Result<(), Error> {
    let Some(guild_id) = msg.guild_id else {
        return Ok(());
    };
    let Some((interaction_id, invoker)) = command_invoker(msg) else {
        return Ok(());
    };
    if invoker.bot {
        return Ok(());
    }
    let now = Instant::now();
    if !first_sight_of_interaction(&state.seen_interactions, interaction_id, now) {
        return Ok(());
    }
    let kind = detect_rate_spam(state, invoker.id, msg.channel_id, now);
    if kind == SpamKind::None {
        return Ok(());
    }
    // メンバー取得はAPI呼び出しになり得るので、検知が出た時だけ行う
    let invoker_is_admin = member_is_admin(ctx, guild_id, invoker.id).await;
    let actor = Actor { user: invoker, via_bot: Some(&msg.author) };
    let detection = Detection { delete: false, kind };
    dispatch(ctx, msg, state, actor, detection, invoker_is_admin).await?;
    Ok(())
}

/// user_is_admin のメンバーID指定版（メッセージに PartialMember が無い、コマンドを打った人用）。
async fn member_is_admin(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> bool {
    let Ok(member) = guild_id.member(ctx, user_id).await else {
        return false;
    };
    ctx.cache
        .guild(guild_id)
        .is_some_and(|guild| guild.member_permissions(&member).administrator())
}
