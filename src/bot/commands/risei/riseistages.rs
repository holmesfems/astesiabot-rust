use super::{fmt_percent, send_reply, server_from_bool};
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::risei_calculator_engine::Server;
use poise::serenity_prelude as serenity;

const MAX_ITEMS: usize = 15;

/// Python版と同じく、常に大陸版のステージ一覧からオートコンプリート候補を出す
/// (大陸版が最も先行しておりステージ数が多いため)。
async fn autocomplete_stage(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let state = ctx.data().state.clone();
    let snapshot = state.risei_calculator.snapshot(Server::Mainland, &state.outer_source).await;
    snapshot
        .auto_complete_main_stage(partial, 25)
        .into_iter()
        .map(|(name, value)| serenity::AutocompleteChoice::new(name, value))
        .collect()
}

/// 恒常ステージの理性効率を検索します。恒常サイドストーリーも対象。
#[poise::command(slash_command)]
pub async fn riseistages(
    ctx: Context<'_>,
    #[description = "ステージ名を入力(例:1-7 SV-8 など)"]
    #[autocomplete = "autocomplete_stage"]
    stage: String,
    #[description = "True:グローバル版基準の計算(デフォルト)、False:大陸版の新ステージと新素材を入れた計算"] is_global: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let requested_server = server_from_bool(is_global.unwrap_or(true));
    let state = ctx.data().state.clone();

    let reply = match state.risei_calculator.stage_search(&state.outer_source, requested_server, &stage).await {
        Err(msg) => EmbedReply::error(&msg),
        Ok(result) => {
            let fell_back = result.effective_server == Server::Mainland && requested_server == Server::Global;
            let title = if fell_back {
                "通常ステージ検索(大陸版)".to_string()
            } else {
                "通常ステージ検索".to_string()
            };
            let mut header = format!("検索内容 = {stage}");
            if fell_back {
                header.push_str("\nグロ版未実装につき、大陸版ステージを表示します");
            }
            let mut chunks = vec![header];
            for s in result.stages.iter().take(MAX_ITEMS) {
                let mut lines = vec![
                    format!("マップ名       : {}", s.name),
                    format!("総合効率       : {}", fmt_percent(s.total_efficiency)),
                    format!("99%信頼区間(3σ): {}", fmt_percent(s.confidence_3sigma)),
                ];
                if s.categories.is_empty() {
                    lines.push("主ドロップ情報未登録".to_string());
                } else {
                    for category in &s.categories {
                        lines.push(format!("{}効率: {}", category.category_ja, fmt_percent(category.efficiency)));
                        if let Some(drop_per_minute) = category.drop_per_minute {
                            lines.push(format!("分入手数(中級) : {drop_per_minute:.2}"));
                        }
                    }
                }
                lines.push(format!("理性消費       : {}", s.sanity_cost));
                if let Some(time_cost) = s.time_cost {
                    lines.push(format!("時間消費(倍速) : {time_cost:.2}"));
                }
                lines.push(format!("昇進効率       : {}", fmt_percent(s.promotion_efficiency)));
                lines.push(format!("試行数         : {}", s.max_times));
                chunks.push(format!("```\n{}\n```", lines.join("\n")));
            }
            EmbedReply {
                title,
                chunks,
                msg_type: MsgType::Ok,
            }
        }
    };
    send_reply(ctx, reply).await
}
