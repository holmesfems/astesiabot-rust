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

/// コストランキングの種別。`#[name]`がDiscordの選択肢の表示文字列になる。
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

/// 昇進コストランキングの表示チャンク。
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

/// コードブロックで囲み、閉じの```の後にも改行を入れる専用整形（`dump_to_print`とは末尾の改行位置が異なる）。
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
                Err(e) => EmbedReply {
                    title,
                    chunks: vec![e.to_string()],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::operator_cost_calc::dto::ItemCostView;

    const SEP: &str = "\n----- chunk -----\n";

    fn items(list: &[(&str, f64)]) -> Vec<(String, f64)> {
        list.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    fn entry(rank: usize, name: &str, risei_value: f64) -> RankedEntry {
        RankedEntry {
            rank,
            name: name.to_string(),
            risei_value,
        }
    }

    fn view(risei_value: f64, list: &[(&str, f64)]) -> ItemCostView {
        ItemCostView {
            risei_value,
            items: items(list),
        }
    }

    fn summary() -> CostSummaryDto {
        CostSummaryDto {
            operator_names: vec![
                "アーミヤ".into(),
                "ロスモンティス".into(),
                "ケルシー".into(),
            ],
            total_items: items(&[
                ("龍門幣", 12000.0),
                ("上級作戦記録", 24.5),
                ("技巧概論", 0.0),
            ]),
            eq_items: items(&[("モジュールデータ", 3.0), ("データ増幅装置", 1.5)]),
            combined_r2_items: items(&[("RMA70-12", 9.0), ("ポリケトン", 18.25), ("糖", 4.0)]),
            total_risei_value: 12345.678,
        }
    }

    fn ranking() -> Vec<RankedEntry> {
        vec![
            entry(1, "アーミヤ S3", 812.3456),
            entry(2, "とても長い名前のオペレーターのスキル三", 700.0),
            entry(3, "ケルシー S2", 0.0),
        ]
    }

    #[test]
    fn snapshot_elite_ranking_chunks() {
        let dto = EliteRankingDto {
            star: 6,
            total_count: 3,
            entries: ranking(),
        };
        insta::assert_snapshot!(
            "operatorcostlist_elite_ranking",
            elite_ranking_chunks(&dto).join(SEP)
        );
    }

    #[test]
    fn snapshot_cost_summary_chunks() {
        insta::assert_snapshot!(
            "operatorcostlist_cost_summary_unimplemented",
            cost_summary_chunks(&summary(), true).join(SEP)
        );
        insta::assert_snapshot!(
            "operatorcostlist_cost_summary_implemented",
            cost_summary_chunks(&summary(), false).join(SEP)
        );
    }

    #[test]
    fn snapshot_master_stats_full_chunks() {
        let dto = MasterStatsFullDto {
            star: 6,
            skill_nums: 3,
            heaviest_name: "アーミヤ S3".into(),
            heaviest: view(812.3456, &[("技巧概論", 6.0), ("龍門幣", 2.5)]),
            top10_heaviest: ranking(),
            lightest_name: "ケルシー S2".into(),
            lightest: view(10.0, &[("戦術演習", 3.0)]),
            top10_lightest: vec![
                entry(3, "ケルシー S2", 10.0),
                entry(2, "ホシグマ S1", 20.55),
            ],
            average_risei: 340.125,
        };
        insta::assert_snapshot!(
            "operatorcostlist_master_stats_full",
            master_stats_full_chunks(&dto).join(SEP)
        );
    }

    #[test]
    fn snapshot_master_stats_recent_chunks() {
        let dto = MasterStatsRecentDto {
            star: 5,
            skill_nums: 51,
            entries: (1..=51)
                .map(|i| entry(i, &format!("オペレーター{i} S1"), 1000.0 / i as f64))
                .collect(),
        };
        insta::assert_snapshot!(
            "operatorcostlist_master_stats_recent",
            master_stats_recent_chunks(&dto).join(SEP)
        );
    }
}
