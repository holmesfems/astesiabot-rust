//! Python `OperatorCostsCalculator` の各コマンド相当の計算関数。DTOを返すのみで、
//! Discord embed整形は`bot/commands/operator_cost_calc`側の責務。

use super::dto::{
    CostSummaryDto, EliteCostDto, EliteRankingDto, ItemCostView, ModuleCostDto, ModuleEntryDto, ModulePhaseView,
    MasterStatsDto, MasterStatsFullDto, MasterStatsRecentDto, RankedEntry, SkillMasterCostDto,
};
use super::model::ItemCost;
use super::{AllOperatorsInfo, SkillCostInfo, ValueSet, EPSILON};
use crate::engine::outer_source::operator_data::RawOperatorCost;
use super::aggregate;

/// Python `OperatorCostsCalculator.skillMasterCost`。
pub fn skill_master_cost(info: &AllOperatorsInfo, values: &ValueSet, operator_name: &str, skill_num: u32) -> Result<SkillMasterCostDto, String> {
    let Some(op) = info.get_by_name(operator_name) else {
        return Err(format!("オペレーター【{operator_name}】は存在しません"));
    };
    if op.skills.is_empty() {
        return Err(format!("オペレーター【{operator_name}】はスキルが存在しません"));
    }
    if skill_num == 0 || skill_num as usize > op.skills.len() {
        return Err(format!("オペレーター【{operator_name}】のスキル{skill_num}は存在しません"));
    }
    if op.stars <= 3 {
        return Err(format!("オペレーター【{operator_name}】はスキルの特化は存在しません"));
    }

    let skill = &op.skills[(skill_num - 1) as usize];
    let values_for_op = values.for_cn_only(op.cn_only);
    let mastery_costs: Vec<ItemCost> = skill.masteries.iter().map(|m| ItemCost::from_cost_entries(m)).collect();
    let masteries: Vec<ItemCostView> = mastery_costs
        .iter()
        .map(|c| ItemCostView {
            risei_value: c.to_risei_value(values_for_op, &info.item_names),
            items: c.ordered_name_counts(&info.item_names),
        })
        .collect();
    let total_cost = ItemCost::sum(&mastery_costs);
    let total = ItemCostView {
        risei_value: total_cost.to_risei_value(values_for_op, &info.item_names),
        items: total_cost.ordered_name_counts(&info.item_names),
    };
    let total_r2_items = total_cost
        .rare3and4_to_rare2(&info.item_names, &info.formulas)
        .ordered_name_counts(&info.item_names);

    let ranking = info.sorted_skill_cost(op.stars, values);
    let ranking_text = ranking
        .iter()
        .position(|c| c.operator_id == op.id && c.skill_id == skill.skill_id)
        .map(|idx| format!("星{}スキル{}個中、第{}位の消費です", op.stars, ranking.len(), idx + 1));

    Ok(SkillMasterCostDto {
        skill_name: info.skill_names.get_str(&skill.skill_id).to_string(),
        skill_num,
        masteries,
        total,
        total_r2_items,
        ranking_text,
    })
}

/// Python `OperatorCostsCalculator.operatorEliteCost`。
pub fn operator_elite_cost(info: &AllOperatorsInfo, values: &ValueSet, operator_name: &str) -> Result<EliteCostDto, String> {
    let Some(op) = info.get_by_name(operator_name) else {
        return Err(format!("オペレーター【{operator_name}】は存在しません"));
    };
    if op.phases.is_empty() {
        return Err(format!("オペレーター【{operator_name}】の昇進は存在しません"));
    }

    let values_for_op = values.for_cn_only(op.cn_only);
    let phase_costs: Vec<ItemCost> = op.phases.iter().map(|p| ItemCost::from_cost_entries(p)).collect();
    let phases: Vec<ItemCostView> = phase_costs
        .iter()
        .map(|c| ItemCostView {
            risei_value: c.to_risei_value(values_for_op, &info.item_names),
            items: c.ordered_name_counts(&info.item_names),
        })
        .collect();
    let total_cost = ItemCost::sum(&phase_costs);
    let total = ItemCostView {
        risei_value: total_cost.to_risei_value(values_for_op, &info.item_names),
        items: total_cost.ordered_name_counts(&info.item_names),
    };
    let total_r2_items = total_cost
        .rare3and4_to_rare2(&info.item_names, &info.formulas)
        .ordered_name_counts(&info.item_names);

    let ranking_text = if !op.is_patch && (op.stars == 5 || op.stars == 6) {
        let ranking = info.sorted_by_elite_cost(op.stars, values);
        ranking.iter().position(|(candidate, _)| candidate.id == op.id).map(|idx| {
            format!("星{}オペレーター{}名中、第{}位の消費です", op.stars, ranking.len(), idx + 1)
        })
    } else {
        None
    };

    Ok(EliteCostDto {
        operator_name: op.name.clone(),
        phases,
        total,
        total_r2_items,
        ranking_text,
    })
}

/// Python `OperatorCostsCalculator.operatorModuleCost`。
pub fn operator_module_cost(info: &AllOperatorsInfo, values: &ValueSet, operator_name: &str) -> Result<ModuleCostDto, String> {
    let Some(op) = info.get_by_name(operator_name) else {
        return Err(format!("オペレーター【{operator_name}】は存在しません"));
    };
    if op.modules.is_empty() {
        return Err(format!("オペレーター【{operator_name}】のモジュールは存在しません"));
    }

    let modules = op
        .modules
        .iter()
        .map(|module| {
            // モジュールごとにcnOnlyで参照サーバを切り替える(オペレーター自身のcnOnlyではない)。
            let values_for_module = values.for_cn_only(module.cn_only);
            let header = if module.cn_only {
                format!("{}(大陸版)", module.eq_type)
            } else {
                module.eq_type.clone()
            };
            let phase_costs: Vec<ItemCost> = module.phase_costs.iter().map(|c| ItemCost::from_cost_entries(c)).collect();
            let phases: Vec<ModulePhaseView> = phase_costs
                .iter()
                .enumerate()
                .map(|(i, c)| ModulePhaseView {
                    stage: (i + 1) as u32,
                    risei_value: c.to_risei_value_only_value_target(values_for_module, &info.item_names),
                    items: c.ordered_name_counts(&info.item_names),
                })
                .collect();
            let total_cost = ItemCost::sum(&phase_costs);
            ModuleEntryDto {
                header,
                phases,
                total_risei_value: total_cost.to_risei_value_only_value_target(values_for_module, &info.item_names),
                total_items: total_cost.ordered_name_counts(&info.item_names),
                total_r2_items: total_cost
                    .rare3and4_to_rare2(&info.item_names, &info.formulas)
                    .ordered_name_counts(&info.item_names),
            }
        })
        .collect();

    Ok(ModuleCostDto {
        operator_name: op.name.clone(),
        modules,
    })
}

/// Python `operatorCostList`のSTAR4/5/6ELITE分岐（`printCostRanking`）。
pub fn cost_list_by_elite(info: &AllOperatorsInfo, values: &ValueSet, star: u32, only_recent: bool) -> EliteRankingDto {
    let ranking = info.sorted_by_elite_cost(star, values);
    let total_count = ranking.len();
    let entries = ranking
        .iter()
        .enumerate()
        .filter(|(_, (op, risei))| (!only_recent || op.is_recent) && *risei > EPSILON)
        .map(|(idx, (op, risei))| RankedEntry {
            rank: idx + 1,
            name: op.name.clone(),
            risei_value: *risei,
        })
        .collect();
    EliteRankingDto { star, total_count, entries }
}

/// Python `operatorCostList`のCOSTOFCNONLY分岐。
pub fn cost_list_cost_of_cn_only(info: &AllOperatorsInfo, values: &ValueSet) -> CostSummaryDto {
    let cn_only_ops: Vec<&RawOperatorCost> = info.data.operators.values().filter(|op| op.cn_only).collect();
    let operator_names = cn_only_ops.iter().map(|op| op.name.clone()).collect();

    let total_cost = ItemCost::sum(&cn_only_ops.iter().map(|op| aggregate::all_cost_except_eq(op)).collect::<Vec<_>>());
    let eq_cost = ItemCost::sum(
        &info
            .data
            .operators
            .values()
            .filter(|op| aggregate::has_cn_only_ueq(op))
            .map(aggregate::total_unique_eq_cost_cn_only)
            .collect::<Vec<_>>(),
    );
    let combined_r2 = total_cost.add(&eq_cost).rare3and4_to_rare2(&info.item_names, &info.formulas);
    let total_risei_value =
        total_cost.to_risei_value(&values.mainland, &info.item_names) + eq_cost.to_risei_value(&values.mainland, &info.item_names);

    CostSummaryDto {
        operator_names,
        total_items: total_cost.ordered_name_counts(&info.item_names),
        eq_items: eq_cost.ordered_name_counts(&info.item_names),
        combined_r2_items: combined_r2.ordered_name_counts(&info.item_names),
        total_risei_value,
    }
}

/// Python `operatorCostList`のCOSTOFGLOBAL分岐。
pub fn cost_list_cost_of_global(info: &AllOperatorsInfo, values: &ValueSet) -> CostSummaryDto {
    let global_ops: Vec<&RawOperatorCost> = info.data.operators.values().filter(|op| !op.cn_only).collect();

    let total_cost = ItemCost::sum(&global_ops.iter().map(|op| aggregate::all_cost_except_eq(op)).collect::<Vec<_>>());
    let eq_cost = ItemCost::sum(
        &global_ops
            .iter()
            .filter(|op| aggregate::has_unique_eq(op))
            .map(|op| aggregate::total_unique_eq_cost_global(op))
            .collect::<Vec<_>>(),
    );
    let combined_r2 = total_cost.add(&eq_cost).rare3and4_to_rare2(&info.item_names, &info.formulas);
    let total_risei_value =
        total_cost.to_risei_value(&values.global, &info.item_names) + eq_cost.to_risei_value(&values.global, &info.item_names);

    CostSummaryDto {
        operator_names: Vec::new(),
        total_items: total_cost.ordered_name_counts(&info.item_names),
        eq_items: eq_cost.ordered_name_counts(&info.item_names),
        combined_r2_items: combined_r2.ordered_name_counts(&info.item_names),
        total_risei_value,
    }
}

fn skill_cost_item_view(item: &SkillCostInfo, values: &ValueSet, info: &AllOperatorsInfo) -> ItemCostView {
    ItemCostView {
        risei_value: item.total_risei(values, &info.item_names),
        items: item.total_cost.ordered_name_counts(&info.item_names),
    }
}

/// Python `getMasterCostStatistics` / `getMasterCostStatistics_OnlyRecent`。
pub fn cost_list_master_stats(info: &AllOperatorsInfo, values: &ValueSet, star: u32, only_recent: bool) -> Result<MasterStatsDto, String> {
    let ranking = info.sorted_skill_cost(star, values);

    if only_recent {
        let entries = ranking
            .iter()
            .enumerate()
            .filter(|(_, item)| item.is_recent)
            .map(|(idx, item)| RankedEntry {
                rank: idx + 1,
                name: item.operator_name_index(),
                risei_value: item.total_risei(values, &info.item_names),
            })
            .collect();
        return Ok(MasterStatsDto::Recent(MasterStatsRecentDto {
            star,
            skill_nums: ranking.len(),
            entries,
        }));
    }

    let skill_nums = ranking.len();
    if skill_nums == 0 {
        return Err(format!("星{star}の特化データがありません"));
    }

    let heaviest = &ranking[0];
    let lightest = &ranking[skill_nums - 1];
    let top10_heaviest: Vec<RankedEntry> = ranking
        .iter()
        .take(10.min(skill_nums))
        .enumerate()
        .map(|(i, item)| RankedEntry {
            rank: i + 1,
            name: item.operator_name_index(),
            risei_value: item.total_risei(values, &info.item_names),
        })
        .collect();
    let lightest_start = skill_nums.saturating_sub(10);
    let top10_lightest: Vec<RankedEntry> = (lightest_start..skill_nums)
        .map(|i| RankedEntry {
            rank: i + 1,
            name: ranking[i].operator_name_index(),
            risei_value: ranking[i].total_risei(values, &info.item_names),
        })
        .collect();
    let average_risei = ranking.iter().map(|item| item.total_risei(values, &info.item_names)).sum::<f64>() / skill_nums as f64;

    Ok(MasterStatsDto::Full(MasterStatsFullDto {
        star,
        skill_nums,
        heaviest_name: heaviest.operator_name_index(),
        heaviest: skill_cost_item_view(heaviest, values, info),
        top10_heaviest,
        lightest_name: lightest.operator_name_index(),
        lightest: skill_cost_item_view(lightest, values, info),
        top10_lightest,
        average_risei,
    }))
}
