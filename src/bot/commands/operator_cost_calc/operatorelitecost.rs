use super::{build_context, fmt_item_block, send_reply};
use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::operator_cost_calc::calc::operator_elite_cost;
use poise::serenity_prelude as serenity;

const BASE_TITLE: &str = "昇進必要素材検索";

/// Python `OperatorCostsCalculator.operatorEliteCost`の整形込み版。
pub async fn elite_cost_reply(state: &AppState, operator_name: &str) -> EmbedReply {
    let (info, values) = build_context(state).await;
    match operator_elite_cost(&info, &values, operator_name) {
        Err(msg) => EmbedReply {
            title: BASE_TITLE.to_string(),
            chunks: vec![msg],
            msg_type: MsgType::Err,
        },
        Ok(dto) => {
            let mut chunks = Vec::new();
            for (i, phase) in dto.phases.iter().enumerate() {
                chunks.push(format!(
                    "昇進{} 理性価値:{:.2}{}\n",
                    i + 1,
                    phase.risei_value,
                    fmt_item_block(&phase.items, false)
                ));
            }
            chunks.push(format!(
                "合計  理性価値:{:.2}{}\n",
                dto.total.risei_value,
                fmt_item_block(&dto.total.items, false)
            ));
            chunks.push(format!("合計  中級換算{}", fmt_item_block(&dto.total_r2_items, false)));
            if let Some(text) = &dto.ranking_text {
                chunks.push(text.clone());
            }
            EmbedReply {
                title: format!("{BASE_TITLE}: {}", dto.operator_name),
                chunks,
                msg_type: MsgType::Ok,
            }
        }
    }
}

async fn autocomplete_operator_name(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let (info, _) = build_context(&ctx.data().state).await;
    info.autocomplete_elite_cost(partial, 25)
        .into_iter()
        .map(|name| serenity::AutocompleteChoice::new(name.clone(), name))
        .collect()
}

/// オペレーターの昇進消費素材を調べる。
#[poise::command(slash_command)]
pub async fn operatorelitecost(
    ctx: Context<'_>,
    #[description = "オペレーターの名前、大陸先行オペレーターも日本語を入れてください"]
    #[autocomplete = "autocomplete_operator_name"]
    operator_name: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let state = ctx.data().state.clone();
    let reply = elite_cost_reply(&state, &operator_name).await;
    send_reply(ctx, reply).await
}
