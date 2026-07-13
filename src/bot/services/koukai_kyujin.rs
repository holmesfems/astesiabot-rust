use crate::bot::data::Error;
use poise::serenity_prelude as serenity;

pub async fn handle(_ctx: &serenity::Context, msg: &serenity::Message) -> Result<(), Error> {
    // 画像添付がなければ何もしない
    if msg.attachments.is_empty() {
        return Ok(());
    }
    println!(
        "[koukai_kyujin] {} が画像を投稿: {}",
        msg.author.name, msg.attachments[0].url
    );
    // TODO: OCR APIに投げて公開求人計算
    Ok(())
}