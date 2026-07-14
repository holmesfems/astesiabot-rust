pub mod riseievents;
pub mod riseilists;
pub mod riseimaterials;
pub mod riseistages;

use crate::bot::data::{Context, Error};
use crate::bot::reply::{to_embed_batches, EmbedReply};
use crate::engine::risei_calculator_engine::Server;

/// `is_global: bool` スラッシュコマンド引数から [`Server`] へ変換する
/// （Python版の各コマンドの `is_global` 引数と同じ意味。true=グローバル版基準）。
pub fn server_from_bool(is_global: bool) -> Server {
    if is_global {
        Server::Global
    } else {
        Server::Mainland
    }
}

/// EmbedReply をスラッシュコマンドの応答として送信する。
/// 1メッセージに収まらない場合は複数回に分けて送る（`to_embed_batches` 参照）。
pub async fn send_reply(ctx: Context<'_>, reply: EmbedReply) -> Result<(), Error> {
    for batch in to_embed_batches(&reply) {
        let mut created = poise::CreateReply::default();
        created.embeds = batch;
        ctx.send(created).await?;
    }
    Ok(())
}

/// パーセント表示（小数点以下1桁）。Python `"{0:.1%}"`.
pub fn fmt_percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

/// 効率などの小数表示（小数点以下3桁）。Python `"{0:.3f}"`.
pub fn fmt_value(value: f64) -> String {
    format!("{value:.3}")
}
