use super::send_reply;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::risei_calculator_engine::kakin::KakinPack;
use poise::serenity_prelude as serenity;

/// riseikakinのtarget="全体比較(グローバル)"相当（Python `totalJATuple`）。
const TOTAL_TARGETS: [&str; 2] = ["全体比較(グローバル)", "Total_Global"];

async fn autocomplete_kakin_target(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    ctx.data()
        .state
        .risei_calculator
        .kakin_autocomplete(partial, 25)
        .into_iter()
        .map(|(name, value)| serenity::AutocompleteChoice::new(name, value))
        .collect()
}

/// YAMLの個数表示用。整数値なら小数点無しで表示する（Python `str(count)`相当の簡略版）。
fn format_count(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn contents_block(contents: &[(String, f64)]) -> String {
    let lines: Vec<String> = contents
        .iter()
        .map(|(name, count)| format!("{name} × {}", format_count(*count)))
        .collect();
    format!("```\n{}\n```\n", lines.join("\n"))
}

/// Python `KakinPack.strBlock`。
fn value_block(pack: &KakinPack) -> String {
    let lines = [
        format!("総合効率    : {:.2}%", pack.total_efficiency * 100.0),
        format!("ガチャ効率  : {:.2}%", pack.gacha_efficiency * 100.0),
        format!("パック値段  : {:.0}円", pack.price),
        format!("合計理性価値: {:.2}", pack.total_value),
        format!("純正源石換算: {:.2}", pack.total_originium),
        format!("マネー換算  : {:.2}円", pack.total_real_money),
        format!("ガチャ数    : {:.2}", pack.gacha_count),
    ];
    format!("```\n{}\n```\n", lines.join("\n"))
}

/// Python `riseikakin`の`constantStrBlock`（参考用課金効率一覧）。
fn constant_block(constants: &[KakinPack]) -> String {
    let lines: Vec<String> = constants
        .iter()
        .map(|pack| format!("{}: {:.2}%", pack.name, pack.total_efficiency * 100.0))
        .collect();
    format!("参考用課金効率:```\n{}\n```", lines.join("\n"))
}

/// 課金理性効率表を出力します。
#[poise::command(slash_command)]
pub async fn riseikakin(
    ctx: Context<'_>,
    #[description = "表示する効率表を選んでください"]
    #[autocomplete = "autocomplete_kakin_target"]
    target: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let state = ctx.data().state.clone();
    let engine = &state.risei_calculator;
    let outer_source = &state.outer_source;

    let reply = if TOTAL_TARGETS.contains(&target.as_str()) {
        let limited = engine.kakin_limited_sorted(outer_source).await;
        let constants = engine.kakin_constants(outer_source).await;
        let mut chunks: Vec<String> = limited.iter().map(|pack| format!("{}:{}", pack.name, value_block(pack))).collect();
        chunks.push(constant_block(&constants));
        EmbedReply {
            title: "課金パック比較".to_string(),
            chunks,
            msg_type: MsgType::Ok,
        }
    } else {
        match engine.kakin_pack(&target, outer_source).await {
            Err(msg) => EmbedReply::error(&msg),
            Ok(pack) => {
                let constants = engine.kakin_constants(outer_source).await;
                let chunks = vec![
                    format!("内容物:{}", contents_block(&pack.contents)),
                    format!("理性価値情報:{}", value_block(&pack)),
                    constant_block(&constants),
                ];
                EmbedReply {
                    title: pack.name.clone(),
                    chunks,
                    msg_type: MsgType::Ok,
                }
            }
        }
    };
    send_reply(ctx, reply).await
}
