use crate::bot::data::{Data, Error};
use poise::serenity_prelude as serenity;

/// 全イベントを受ける汎用ハンドラ。
/// poise の FrameworkContext と serenity の Event が渡ってくる。
pub async fn event_handler(
    _ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    _data: &Data,
) -> Result<(), Error> {
    match event {
        // 新しいメッセージが来たとき
        serenity::FullEvent::Message { new_message } => {
            // bot自身の発言は無視（無限ループ防止）
            if new_message.author.bot {
                return Ok(());
            }
            println!(
                "[msg] {}: {}",
                new_message.author.name, new_message.content
            );
        }
        _ => {}
    }
    Ok(())
}