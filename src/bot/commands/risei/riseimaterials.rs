use super::{fmt_percent, fmt_value, send_reply, server_from_bool};
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use poise::serenity_prelude as serenity;

const MAX_ITEMS: usize = 15;

async fn autocomplete_target_item(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    ctx.data()
        .state
        .risei_calculator
        .category_choices()
        .into_iter()
        .filter(|(name, _)| name.contains(partial))
        .take(25)
        .map(|(name, key)| serenity::AutocompleteChoice::new(name, key))
        .collect()
}

/// 昇進素材の効率の良い恒常ステージを調べます。
#[poise::command(slash_command)]
pub async fn riseimaterials(
    ctx: Context<'_>,
    #[description = "昇進素材カテゴリを選択"]
    #[autocomplete = "autocomplete_target_item"]
    target_item: String,
    #[description = "True:グローバル版基準の計算(デフォルト)、False:大陸版の新ステージと新素材を入れた計算"] is_global: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let server = server_from_bool(is_global.unwrap_or(true));
    let state = ctx.data().state.clone();

    let reply = match state
        .risei_calculator
        .material_search(server, &target_item, &state.outer_source)
        .await
    {
        Err(msg) => EmbedReply::error(&msg),
        Ok((_effective_server, result)) => {
            let mut chunks = vec![format!(
                "{}: 理性価値(中級)={}±{}\n",
                result.category_ja,
                fmt_value(result.main_item_value),
                fmt_value(result.main_item_std_dev)
            )];
            for stage in result.stages.iter().take(MAX_ITEMS) {
                let mut lines = vec![
                    format!("マップ名       : {}", stage.name),
                    format!("理性効率       : {}", fmt_percent(stage.efficiency)),
                    format!("理性消費       : {}", stage.sanity_cost),
                ];
                if let Some(time_cost) = stage.time_cost {
                    lines.push(format!("時間消費(倍速) : {time_cost:.2}"));
                }
                if let Some(drop_per_minute) = stage.drop_per_minute {
                    lines.push(format!("分入手数(中級) : {drop_per_minute:.2}"));
                }
                lines.push(format!("主素材効率     : {}", fmt_percent(stage.main_item_efficiency)));
                lines.push(format!("99%信頼区間(3σ): {}", fmt_percent(stage.confidence_3sigma)));
                lines.push(format!("昇進効率       : {}", fmt_percent(stage.promotion_efficiency)));
                lines.push(format!("試行数         : {}", stage.max_times));
                chunks.push(format!("```\n{}\n```", lines.join("\n")));
            }
            EmbedReply {
                title: "昇進素材検索".to_string(),
                chunks,
                msg_type: MsgType::Ok,
            }
        }
    };
    send_reply(ctx, reply).await
}
