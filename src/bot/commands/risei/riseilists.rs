use super::{fmt_percent, fmt_value, send_reply, server_from_bool};
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::risei_calculator_engine::TicketEfficiency;

#[derive(Debug, poise::ChoiceParameter)]
pub enum RiseiListTarget {
    #[name = "基準マップ"]
    BaseMaps,
    #[name = "理性価値表"]
    SanValueList,
    #[name = "初級資格証効率表"]
    Te2List,
    #[name = "上級資格証効率表"]
    Te3List,
    #[name = "特別引換証効率表"]
    SpecialList,
}

fn ticket_list_chunks(list: Vec<TicketEfficiency>) -> Vec<String> {
    let lines: Vec<String> = list
        .iter()
        .map(|item| format!("{}: {} ± {}", item.name_ja, fmt_percent(item.efficiency), fmt_percent(item.std_dev * 2.0)))
        .collect();
    vec![format!("```\n{}\n```", lines.join("\n"))]
}

/// 理性効率表を出力します。
#[poise::command(slash_command)]
pub async fn riseilists(
    ctx: Context<'_>,
    #[description = "表示する効率表を選んでください"] target: RiseiListTarget,
    #[description = "True:グローバル版基準の計算(デフォルト)、False:大陸版の新ステージと新素材を入れた計算"] is_global: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let server = server_from_bool(is_global.unwrap_or(true));
    let state = ctx.data().state.clone();
    let engine = &state.risei_calculator;
    let outer_source = &state.outer_source;

    let (title, chunks) = match target {
        RiseiListTarget::BaseMaps => {
            let map = engine.base_maps(server, outer_source).await;
            let body = map
                .iter()
                .map(|(category, stage)| format!("{category}: {stage}"))
                .collect::<Vec<_>>()
                .join("\n");
            ("基準ステージ表示".to_string(), vec![format!("```\n{body}\n```")])
        }
        RiseiListTarget::SanValueList => {
            let values = engine.value_list(server, outer_source).await;
            let lines: Vec<String> = values
                .iter()
                .map(|(name, value, std_dev)| format!("{name}: {} ± {}", fmt_value(*value), fmt_value(*std_dev * 2.0)))
                .collect();
            ("理性価値一覧".to_string(), vec![format!("```\n{}\n```", lines.join("\n"))])
        }
        RiseiListTarget::Te2List => (
            "初級資格証効率".to_string(),
            ticket_list_chunks(engine.te2_list(server, outer_source).await),
        ),
        RiseiListTarget::Te3List => (
            "上級資格証効率".to_string(),
            ticket_list_chunks(engine.te3_list(server, outer_source).await),
        ),
        RiseiListTarget::SpecialList => (
            "特別引換証効率".to_string(),
            ticket_list_chunks(engine.special_list(server, outer_source).await),
        ),
    };

    send_reply(
        ctx,
        EmbedReply {
            title,
            chunks,
            msg_type: MsgType::Ok,
        },
    )
    .await
}
