pub mod calc;
pub mod model;

use crate::bot::data::Error;
use poise::serenity_prelude as serenity;

pub async fn handle(ctx: &serenity::Context, msg: &serenity::Message) -> Result<(), Error> {
    // 画像添付がなければ何もしない
    if msg.attachments.is_empty() {
        return Ok(());
    }

    // TODO: ここで OCR API に msg.attachments[0].url を投げてタグを取得する。
    // いまは OCR 未実装なので、動作確認用に固定タグで計算だけ試す。
    let data = calc::RecruitData::load()?;
    let test_tags = vec![
        "狙撃".to_string(),
        "エリート".to_string(),
        "範囲攻撃".to_string(),
    ];
    let results = data.calculate(&test_tags, true, 4);

    // 最低星が高い順に並べて上位を表示
    let mut sorted = results;
    sorted.sort_by(|a, b| b.min_star.cmp(&a.min_star));

    let mut lines = Vec::new();
    for item in sorted.iter().take(10) {
        lines.push(format!(
            "{} -> ★{} | {}",
            item.combo.join("+"),
            item.min_star,
            item.operators.join(",")
        ));
    }
    let reply = if lines.is_empty() {
        "該当なし".to_string()
    } else {
        lines.join("\n")
    };

    println!("[koukai_kyujin] 計算結果:\n{reply}");
    msg.channel_id.say(&ctx.http, reply).await?;
    Ok(())
}