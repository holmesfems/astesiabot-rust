use crate::bot::channels;
use crate::bot::data::{Data, Error};
use crate::bot::reply::send_embed_reply;
use crate::bot::services::{anniversary, koukai_kyujin, moderation, uranai};
use poise::serenity_prelude as serenity;

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
        match msg.channel_id {
            channels::KOUKAI_KYUJIN => {
                if let Some(reply) = koukai_kyujin::handle(ctx, msg, data).await? {
                    send_embed_reply(ctx, msg.channel_id, &reply).await?;
                }
            }
            channels::URANAI => {
                if let Some(reply) = uranai::handle(ctx, msg).await? {
                    send_embed_reply(ctx, msg.channel_id, &reply).await?;
                }
            }
            _ => {
                // どのサービスにも該当しないチャンネル。今はログのみ。
                println!("[msg] {}: {}", msg.author.name, msg.content);
            }
        }
    }
    Ok(())
}
