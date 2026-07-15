use super::{build_context, fmt_item_block, send_reply};
use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::operator_cost_calc::calc::operator_module_cost;
use poise::serenity_prelude as serenity;

const BASE_TITLE: &str = "モジュール必要素材検索";

/// Python `OperatorCostsCalculator.operatorModuleCost`の整形込み版。
/// Python版と異なりモジュール1種＝1チャンク（`bodyMsg + lastMsg + "\n"`を1要素として
/// `embbedContents`に積むのと同じ構造）。
pub async fn module_cost_reply(state: &AppState, operator_name: &str) -> EmbedReply {
    let (info, values) = build_context(state).await;
    match operator_module_cost(&info, &values, operator_name) {
        Err(msg) => EmbedReply {
            title: BASE_TITLE.to_string(),
            chunks: vec![msg],
            msg_type: MsgType::Err,
            reply_marker: None,
        },
        Ok(dto) => {
            let chunks: Vec<String> = dto
                .modules
                .iter()
                .map(|module| {
                    let mut body = String::new();
                    for phase in &module.phases {
                        body += &format!(
                            "{} Stage.{} 理性価値:{:.2}{}\n",
                            module.header,
                            phase.stage,
                            phase.risei_value,
                            fmt_item_block(&phase.items, false)
                        );
                    }
                    let last = format!(
                        "合計 理性価値:{:.2}{}\n合計 中級換算:{}",
                        module.total_risei_value,
                        fmt_item_block(&module.total_items, false),
                        fmt_item_block(&module.total_r2_items, false)
                    );
                    format!("{body}{last}\n")
                })
                .collect();
            EmbedReply {
                title: format!("{BASE_TITLE}: {}", dto.operator_name),
                chunks,
                msg_type: MsgType::Ok,
                reply_marker: None,
            }
        }
    }
}

async fn autocomplete_operator_name(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let (info, _) = build_context(&ctx.data().state).await;
    info.autocomplete_module_cost(partial, 25)
        .into_iter()
        .map(|name| serenity::AutocompleteChoice::new(name.clone(), name))
        .collect()
}

/// オペレーターのモジュール消費素材を調べる。
#[poise::command(slash_command)]
pub async fn operatormodulecost(
    ctx: Context<'_>,
    #[description = "オペレーターの名前、大陸先行オペレーターも日本語を入れてください"]
    #[autocomplete = "autocomplete_operator_name"]
    operator_name: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let state = ctx.data().state.clone();
    let reply = module_cost_reply(&state, &operator_name).await;
    send_reply(ctx, reply).await
}
