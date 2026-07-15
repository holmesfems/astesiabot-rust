use crate::bot::data::{Data, Error};
use crate::bot::reply::send_embed_reply;
use crate::bot::services::{anniversary, recruit, moderation, uranai};
use crate::bot::utils::channel_id_env;
use poise::serenity_prelude as serenity;

/// チャンネル別振り分け先のID。起動時に一度だけ環境変数から解決し、Data に載せて
/// bot 起動中ずっと使い回す。新しい振り分け先を増やす時はここにフィールドと
/// from_env() の解決を足し、event_handler の振り分けに分岐を足すだけでよい
/// （Data・bot/mod.rs 側の変更は不要）。
pub struct ChannelRouting {
    koukai_kyujin: serenity::ChannelId,
    uranai: serenity::ChannelId,
}

impl ChannelRouting {
    pub fn from_env() -> Self {
        Self {
            koukai_kyujin: channel_id_env("CHANNEL_ID_KOUKAI_KYUJIN"),
            uranai: channel_id_env("CHANNEL_ID_URANAI"),
        }
    }
}

pub async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    if let serenity::FullEvent::Message { new_message: msg } = event {
        // 0. 自分の発言は必ず無視（自己ループ防止）
        if msg.author.id == ctx.cache.current_user().id {
            return Ok(());
        }

        // 1. モデレーション（罠チャンネル削除・全体通知BAN・連投/爆撃検知・最優先）
        let user_is_admin = moderation::user_is_admin(ctx, msg);
        let discarded = moderation::handle(ctx, msg, &data.state.moderation, user_is_admin).await?;
        if discarded {
            return Ok(()); // 処断済みなら以降に渡さない（1周年ロールも付けない）
        }

        // 2. bot の発言はここから先は無視
        if msg.author.bot {
            return Ok(());
        }

        // 3. 1周年ロール付与（モデレーションとは無関係の独立機能）
        anniversary::handle(ctx, msg).await?;

        // 4. チャンネル別のサービス振り分け（計算 → 返ってきたら送信）
        if msg.channel_id == data.channel_routing.koukai_kyujin {
            if let Some(reply) = recruit::handle(ctx, msg, data).await? {
                send_embed_reply(ctx, msg.channel_id, &reply).await?;
            }
        } else if msg.channel_id == data.channel_routing.uranai {
            if let Some(reply) = uranai::handle(ctx, msg, data).await? {
                send_embed_reply(ctx, msg.channel_id, &reply).await?;
            }
        } else {
            // どのサービスにも該当しないチャンネル。今はログのみ。
            println!("[msg] {}: {}", msg.author.name, msg.content);
        }
    }
    Ok(())
}
