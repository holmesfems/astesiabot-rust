use super::{build_context, dump_to_print, fmt_item_block, send_reply};
use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::operator_cost_calc::calc::{
    cost_list_by_elite, cost_list_cost_of_cn_only, cost_list_cost_of_global, cost_list_master_stats,
};
use crate::engine::operator_cost_calc::dto::{
    CostSummaryDto, EliteRankingDto, MasterStatsDto, MasterStatsFullDto, MasterStatsRecentDto,
    RankedEntry,
};

/// Python `OperatorCostsCalculator.CostListSelection`。`#[name]`はRiseiCalculator.pyの
/// `Choice(name=...)`表示文字列と一致させること。
#[derive(Debug, poise::ChoiceParameter)]
pub enum CostListSelection {
    #[name = "星6昇進素材価値表"]
    Star6Elite,
    #[name = "星5昇進素材価値表"]
    Star5Elite,
    #[name = "星4昇進素材価値表"]
    Star4Elite,
    #[name = "未実装オペレーターの消費素材合計"]
    CostOfCnOnly,
    #[name = "実装済オペレーターの消費素材合計"]
    CostOfGlobal,
    #[name = "星6特化統計"]
    MasterStar6,
    #[name = "星5特化統計"]
    MasterStar5,
    #[name = "星4特化統計"]
    MasterStar4,
}

/// Python `printCostRanking`。
fn elite_ranking_chunks(dto: &EliteRankingDto) -> Vec<String> {
    let mut chunks = vec![format!(
        "オペレーター総数:{}\nSoCは以下の計算に含まれません:",
        dto.total_count
    )];
    for group in dto.entries.chunks(50) {
        let lines: Vec<String> = group
            .iter()
            .map(|e| format!("{}. {} : {:.3}", e.rank, e.name, e.risei_value))
            .collect();
        chunks.push(dump_to_print(&lines));
    }
    chunks
}

fn cost_summary_chunks(dto: &CostSummaryDto, unimplemented: bool) -> Vec<String> {
    let mut chunks = Vec::new();
    if unimplemented {
        chunks.push(format!(
            "未実装オペレーター一覧：{}\n",
            dump_to_print(&dto.operator_names)
        ));
    }
    let (label_total, label_eq) = if unimplemented {
        ("全昇進、全特化の合計消費:", "未実装モジュールの合計消費:")
    } else {
        ("全昇進、全特化の合計消費:", "実装済モジュールの合計消費:")
    };
    chunks.push(format!(
        "{label_total}{}\n",
        fmt_item_block(&dto.total_items, false)
    ));
    chunks.push(format!(
        "{label_eq}{}\n",
        fmt_item_block(&dto.eq_items, false)
    ));
    chunks.push(format!(
        "全合計の中級素材換算:{}\n",
        fmt_item_block(&dto.combined_r2_items, true)
    ));
    chunks.push(format!(
        "合計理性価値(補完チップ系抜き):{:.3}\n",
        dto.total_risei_value
    ));
    chunks.push(format!("源石換算 : {:.3}\n", dto.total_risei_value / 135.0));
    chunks.push(format!(
        "日本円換算 : {:.0} 円",
        dto.total_risei_value / 135.0 / 175.0 * 10000.0
    ));
    chunks
}

/// Python `msg = "```\n" + ... + "```\n"`（`dumpToPrint`とは末尾の改行位置が異なる専用整形）。
fn ranked_block_with_trailing_newline(entries: &[RankedEntry]) -> String {
    let mut msg = String::from("```\n");
    for e in entries {
        msg += &format!("{}.{}: {:.2}\n", e.rank, e.name, e.risei_value);
    }
    msg += "```\n";
    msg
}

fn master_stats_full_chunks(dto: &MasterStatsFullDto) -> Vec<String> {
    vec![
        format!("総スキル数: {}\n", dto.skill_nums),
        format!(
            "一番消費が重い特化スキル:\n{}\n{}",
            dto.heaviest_name,
            fmt_item_block(&dto.heaviest.items, false)
        ),
        format!("合計理性価値: {:.2}\n", dto.heaviest.risei_value),
        "消費が重いスキルTop10:".to_string(),
        ranked_block_with_trailing_newline(&dto.top10_heaviest),
        format!(
            "一番消費が軽い特化スキル:\n{}\n{}",
            dto.lightest_name,
            fmt_item_block(&dto.lightest.items, false)
        ),
        format!("合計理性価値: {:.2}\n", dto.lightest.risei_value),
        "消費が軽いスキルTop10:".to_string(),
        ranked_block_with_trailing_newline(&dto.top10_lightest),
        format!("平均理性価値: {:.2}", dto.average_risei),
    ]
}

fn master_stats_recent_chunks(dto: &MasterStatsRecentDto) -> Vec<String> {
    let mut chunks = vec![format!("スキル総数: {}", dto.skill_nums)];
    for group in dto.entries.chunks(50) {
        let lines: Vec<String> = group
            .iter()
            .map(|e| format!("{}. {} : {:.3}", e.rank, e.name, e.risei_value))
            .collect();
        chunks.push(dump_to_print(&lines));
    }
    chunks
}

pub async fn cost_list_reply(
    state: &AppState,
    selection: &CostListSelection,
    only_recent: bool,
) -> EmbedReply {
    let (info, values) = build_context(state).await;
    match selection {
        CostListSelection::Star6Elite
        | CostListSelection::Star5Elite
        | CostListSelection::Star4Elite => {
            let star = match selection {
                CostListSelection::Star6Elite => 6,
                CostListSelection::Star5Elite => 5,
                _ => 4,
            };
            let dto = cost_list_by_elite(&info, &values, star, only_recent);
            EmbedReply {
                title: format!("★{star}昇進素材価値表"),
                chunks: elite_ranking_chunks(&dto),
                msg_type: MsgType::Ok,
                reply_marker: None,
            }
        }
        CostListSelection::CostOfCnOnly => {
            let dto = cost_list_cost_of_cn_only(&info, &values);
            EmbedReply {
                title: "未実装オペレーターの消費素材合計".to_string(),
                chunks: cost_summary_chunks(&dto, true),
                msg_type: MsgType::Ok,
                reply_marker: None,
            }
        }
        CostListSelection::CostOfGlobal => {
            let dto = cost_list_cost_of_global(&info, &values);
            EmbedReply {
                title: "実装済オペレーターの消費素材合計".to_string(),
                chunks: cost_summary_chunks(&dto, false),
                msg_type: MsgType::Ok,
                reply_marker: None,
            }
        }
        CostListSelection::MasterStar6
        | CostListSelection::MasterStar5
        | CostListSelection::MasterStar4 => {
            let star = match selection {
                CostListSelection::MasterStar6 => 6,
                CostListSelection::MasterStar5 => 5,
                _ => 4,
            };
            let title = format!("星{star}の特化統計情報");
            match cost_list_master_stats(&info, &values, star, only_recent) {
                Err(msg) => EmbedReply {
                    title,
                    chunks: vec![msg],
                    msg_type: MsgType::Err,
                    reply_marker: None,
                },
                Ok(MasterStatsDto::Full(dto)) => EmbedReply {
                    title,
                    chunks: master_stats_full_chunks(&dto),
                    msg_type: MsgType::Ok,
                    reply_marker: None,
                },
                Ok(MasterStatsDto::Recent(dto)) => EmbedReply {
                    title,
                    chunks: master_stats_recent_chunks(&dto),
                    msg_type: MsgType::Ok,
                    reply_marker: None,
                },
            }
        }
    }
}

/// オペレーター消費素材の、いくつか役立つリストを出力します。
#[poise::command(slash_command)]
pub async fn operatorcostlist(
    ctx: Context<'_>,
    #[description = "表示するリスト選択"] selection: CostListSelection,
    #[description = "直近実装/将来実装オペレータのみ表示(一部リストに有効)"] only_recent: Option<
        bool,
    >,
) -> Result<(), Error> {
    ctx.defer().await?;
    let state = ctx.data().state.clone();
    let reply = cost_list_reply(&state, &selection, only_recent.unwrap_or(false)).await;
    send_reply(ctx, reply).await
}
