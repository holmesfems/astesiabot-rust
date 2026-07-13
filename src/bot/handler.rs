use crate::bot::channels;
use crate::bot::data::{Data, Error};
use crate::bot::reply::send_embed_reply;
use crate::bot::services::{koukai_kyujin, spam, uranai};
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

        // 1. スパム検知（全チャンネル対象・最優先）
        if spam::is_spam(msg) {
            spam::handle(ctx, msg).await?;
            return Ok(()); // スパムなら以降に渡さない
        }

        // 2. bot の発言はここから先は無視
        if msg.author.bot {
            return Ok(());
        }

        // 3. チャンネル別のサービス振り分け（計算 → 返ってきたら送信）
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
