//! 各コマンドに対応する計算関数。DTOを返すのみで、
//! Discord embed整形は`bot/commands/operator_cost_calc`側の責務。

use super::aggregate;
use super::dto::{
    CostSummaryDto, EliteCostDto, EliteRankingDto, ItemCostView, MasterStatsDto,
    MasterStatsFullDto, MasterStatsRecentDto, ModuleCostDto, ModuleEntryDto, ModulePhaseView,
    OperatorCostError, RankedEntry, RankingPosition, SkillMasterCostDto,
};
use super::model::ItemCost;
use super::{AllOperatorsInfo, SkillCostInfo, ValueSet, EPSILON};
use crate::engine::external_source::operator_data::RawOperatorCost;

/// 特化コスト（指定スキルの特化1〜3と合計）。
pub fn skill_master_cost(
    info: &AllOperatorsInfo,
    values: &ValueSet,
    operator_name: &str,
    skill_num: u32,
) -> Result<SkillMasterCostDto, OperatorCostError> {
    let Some(op) = info.get_by_name(operator_name) else {
        return Err(OperatorCostError::OperatorNotFound(format!(
            "オペレーター【{operator_name}】は存在しません"
        )));
    };
    if op.skills.is_empty() {
        return Err(OperatorCostError::Other(format!(
            "オペレーター【{operator_name}】はスキルが存在しません"
        )));
    }
    if skill_num == 0 || skill_num as usize > op.skills.len() {
        return Err(OperatorCostError::Other(format!(
            "オペレーター【{operator_name}】のスキル{skill_num}は存在しません"
        )));
    }
    if op.stars <= 3 {
        return Err(OperatorCostError::Other(format!(
            "オペレーター【{operator_name}】はスキルの特化は存在しません"
        )));
    }

    let skill = &op.skills[(skill_num - 1) as usize];
    let values_for_op = values.for_cn_only(op.cn_only);
    let mastery_costs: Vec<ItemCost> = skill
        .masteries
        .iter()
        .map(|m| ItemCost::from_cost_entries(m))
        .collect();
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
    let ranking = ranking
        .iter()
        .position(|c| c.operator_id == op.id && c.skill_id == skill.skill_id)
        .map(|idx| RankingPosition {
            star: op.stars,
            rank: idx + 1,
            total: ranking.len(),
        });

    let description = info.skill_data.get_description(&skill.skill_id).to_string();
    Ok(SkillMasterCostDto {
        operator_name: op.name.clone(),
        cn_only: op.cn_only,
        skill_name: info.skill_data.get_str(&skill.skill_id).to_string(),
        skill_num,
        description,
        masteries,
        total,
        total_r2_items,
        ranking,
    })
}

/// 昇進コスト（昇進1・2と合計）。
pub fn operator_elite_cost(
    info: &AllOperatorsInfo,
    values: &ValueSet,
    operator_name: &str,
) -> Result<EliteCostDto, OperatorCostError> {
    let Some(op) = info.get_by_name(operator_name) else {
        return Err(OperatorCostError::OperatorNotFound(format!(
            "オペレーター【{operator_name}】は存在しません"
        )));
    };
    if op.phases.is_empty() {
        return Err(OperatorCostError::Other(format!(
            "オペレーター【{operator_name}】の昇進は存在しません"
        )));
    }

    let values_for_op = values.for_cn_only(op.cn_only);
    let phase_costs: Vec<ItemCost> = op
        .phases
        .iter()
        .map(|p| ItemCost::from_cost_entries(p))
        .collect();
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

    let ranking = if !op.is_patch && (op.stars == 5 || op.stars == 6) {
        let ranking = info.sorted_by_elite_cost(op.stars, values);
        ranking
            .iter()
            .position(|(candidate, _)| candidate.id == op.id)
            .map(|idx| RankingPosition {
                star: op.stars,
                rank: idx + 1,
                total: ranking.len(),
            })
    } else {
        None
    };

    Ok(EliteCostDto {
        operator_name: op.name.clone(),
        cn_only: op.cn_only,
        phases,
        total,
        total_r2_items,
        ranking,
    })
}

/// モジュールコスト。
pub fn operator_module_cost(
    info: &AllOperatorsInfo,
    values: &ValueSet,
    operator_name: &str,
) -> Result<ModuleCostDto, OperatorCostError> {
    let Some(op) = info.get_by_name(operator_name) else {
        return Err(OperatorCostError::OperatorNotFound(format!(
            "オペレーター【{operator_name}】は存在しません"
        )));
    };
    if op.modules.is_empty() {
        return Err(OperatorCostError::Other(format!(
            "オペレーター【{operator_name}】のモジュールは存在しません"
        )));
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
            let phase_costs: Vec<ItemCost> = module
                .phase_costs
                .iter()
                .map(|c| ItemCost::from_cost_entries(c))
                .collect();
            let phases: Vec<ModulePhaseView> = phase_costs
                .iter()
                .enumerate()
                .map(|(i, c)| ModulePhaseView {
                    stage: (i + 1) as u32,
                    risei_value: c
                        .to_risei_value_only_value_target(values_for_module, &info.item_names),
                    items: c.ordered_name_counts(&info.item_names),
                })
                .collect();
            let total_cost = ItemCost::sum(&phase_costs);
            ModuleEntryDto {
                header,
                cn_only: module.cn_only,
                phases,
                total_risei_value: total_cost
                    .to_risei_value_only_value_target(values_for_module, &info.item_names),
                total_items: total_cost.ordered_name_counts(&info.item_names),
                total_r2_items: total_cost
                    .rare3and4_to_rare2(&info.item_names, &info.formulas)
                    .ordered_name_counts(&info.item_names),
            }
        })
        .collect();

    Ok(ModuleCostDto {
        operator_name: op.name.clone(),
        cn_only: op.cn_only,
        modules,
    })
}

/// 星4/5/6の昇進コストランキング。
pub fn cost_list_by_elite(
    info: &AllOperatorsInfo,
    values: &ValueSet,
    star: u32,
    only_recent: bool,
) -> EliteRankingDto {
    let ranking = info.sorted_by_elite_cost(star, values);
    let total_count = ranking.len();
    let entries = ranking
        .iter()
        .enumerate()
        .filter(|(_, (op, risei))| (!only_recent || info.is_recent(op)) && *risei > EPSILON)
        .map(|(idx, (op, risei))| RankedEntry {
            rank: idx + 1,
            name: op.name.clone(),
            risei_value: *risei,
        })
        .collect();
    EliteRankingDto {
        star,
        total_count,
        entries,
    }
}

/// 大陸限定オペレーターの消費合計。
pub fn cost_list_cost_of_cn_only(info: &AllOperatorsInfo, values: &ValueSet) -> CostSummaryDto {
    let cn_only_ops: Vec<&RawOperatorCost> = info
        .data
        .operators
        .values()
        .filter(|op| op.cn_only)
        .collect();
    let operator_names = cn_only_ops.iter().map(|op| op.name.clone()).collect();

    let total_cost = ItemCost::sum(
        &cn_only_ops
            .iter()
            .map(|op| aggregate::all_cost_except_eq(op))
            .collect::<Vec<_>>(),
    );
    let eq_cost = ItemCost::sum(
        &info
            .data
            .operators
            .values()
            .filter(|op| aggregate::has_cn_only_ueq(op))
            .map(aggregate::total_unique_eq_cost_cn_only)
            .collect::<Vec<_>>(),
    );
    let combined_r2 = total_cost
        .add(&eq_cost)
        .rare3and4_to_rare2(&info.item_names, &info.formulas);
    let total_risei_value = total_cost.to_risei_value(&values.mainland, &info.item_names)
        + eq_cost.to_risei_value(&values.mainland, &info.item_names);

    CostSummaryDto {
        operator_names,
        total_items: total_cost.ordered_name_counts(&info.item_names),
        eq_items: eq_cost.ordered_name_counts(&info.item_names),
        combined_r2_items: combined_r2.ordered_name_counts(&info.item_names),
        total_risei_value,
    }
}

/// 実装済みオペレーターの消費合計。
pub fn cost_list_cost_of_global(info: &AllOperatorsInfo, values: &ValueSet) -> CostSummaryDto {
    let global_ops: Vec<&RawOperatorCost> = info
        .data
        .operators
        .values()
        .filter(|op| !op.cn_only)
        .collect();

    let total_cost = ItemCost::sum(
        &global_ops
            .iter()
            .map(|op| aggregate::all_cost_except_eq(op))
            .collect::<Vec<_>>(),
    );
    let eq_cost = ItemCost::sum(
        &global_ops
            .iter()
            .filter(|op| aggregate::has_unique_eq(op))
            .map(|op| aggregate::total_unique_eq_cost_global(op))
            .collect::<Vec<_>>(),
    );
    let combined_r2 = total_cost
        .add(&eq_cost)
        .rare3and4_to_rare2(&info.item_names, &info.formulas);
    let total_risei_value = total_cost.to_risei_value(&values.global, &info.item_names)
        + eq_cost.to_risei_value(&values.global, &info.item_names);

    CostSummaryDto {
        operator_names: Vec::new(),
        total_items: total_cost.ordered_name_counts(&info.item_names),
        eq_items: eq_cost.ordered_name_counts(&info.item_names),
        combined_r2_items: combined_r2.ordered_name_counts(&info.item_names),
        total_risei_value,
    }
}

fn skill_cost_item_view(
    item: &SkillCostInfo,
    values: &ValueSet,
    info: &AllOperatorsInfo,
) -> ItemCostView {
    ItemCostView {
        risei_value: item.total_risei(values, &info.item_names),
        items: item.total_cost.ordered_name_counts(&info.item_names),
    }
}

/// 特化コストの統計（`only_recent`で直近実装のオペレーターに絞る）。
pub fn cost_list_master_stats(
    info: &AllOperatorsInfo,
    values: &ValueSet,
    star: u32,
    only_recent: bool,
) -> Result<MasterStatsDto, OperatorCostError> {
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
        return Err(OperatorCostError::Other(format!(
            "星{star}の特化データがありません"
        )));
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
    let average_risei = ranking
        .iter()
        .map(|item| item.total_risei(values, &info.item_names))
        .sum::<f64>()
        / skill_nums as f64;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::item_names::ItemNames;
    use crate::engine::external_source::operator_data::{
        CostEntry, OperatorData, RawModule, RawSkill,
    };
    use crate::engine::external_source::skill_data::SkillData;
    use crate::engine::risei_calculator_engine::server::{self, Server};
    use crate::engine::risei_calculator_engine::values::RiseiValues;
    use chrono::NaiveDate;
    use nalgebra::DVector;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn item_names() -> ItemNames {
        serde_json::from_value(serde_json::json!({
            "id_to_ja": {"a": "素材A", "b": "素材B", "c": "素材C", "x": "素材X"},
            "id_to_zh": {"a": "固源岩组", "b": "提纯源岩", "c": "异铁组", "x": "无价值"},
            "zh_to_id": {"固源岩组": "a", "提纯源岩": "b", "异铁组": "c", "无价值": "x"},
            "ja_to_id": {"素材A": "a", "素材B": "b", "素材C": "c", "素材X": "x"},
        }))
        .unwrap()
    }

    fn values_for(server: Server, prices: &[(&str, f64)], names: &Arc<ItemNames>) -> RiseiValues {
        let target = server::value_target(server);
        let mut array = DVector::zeros(target.len());
        for (zh, price) in prices {
            array[target.iter().position(|t| t == zh).unwrap()] = *price;
        }
        RiseiValues::new(server, array, names.clone(), Arc::new(HashMap::new()))
    }

    /// 素材Aの価値は グローバル1・大陸10、素材Bは グローバル3・大陸30、素材Cは
    /// どちらも `EPSILON` ちょうど。素材Xは価値表に無い。
    fn value_set(names: &Arc<ItemNames>) -> ValueSet {
        ValueSet {
            global: values_for(
                Server::Global,
                &[("固源岩组", 1.0), ("提纯源岩", 3.0), ("异铁组", EPSILON)],
                names,
            ),
            mainland: values_for(
                Server::Mainland,
                &[("固源岩组", 10.0), ("提纯源岩", 30.0), ("异铁组", EPSILON)],
                names,
            ),
        }
    }

    fn entries(pairs: &[(&str, f64)]) -> Vec<CostEntry> {
        pairs
            .iter()
            .map(|(id, count)| CostEntry {
                id: id.to_string(),
                count: *count,
            })
            .collect()
    }

    fn op(id: &str, stars: u32) -> RawOperatorCost {
        RawOperatorCost {
            id: id.to_string(),
            name: id.to_string(),
            cn_name: id.to_string(),
            stars,
            cn_only: false,
            is_patch: false,
            phases: Vec::new(),
            skills: Vec::new(),
            all_skill_lvlup: Vec::new(),
            modules: Vec::new(),
        }
    }

    fn with_phases(mut o: RawOperatorCost, phases: &[&[(&str, f64)]]) -> RawOperatorCost {
        o.phases = phases.iter().map(|p| entries(p)).collect();
        o
    }

    fn with_skill(
        mut o: RawOperatorCost,
        skill_id: &str,
        masteries: &[&[(&str, f64)]],
    ) -> RawOperatorCost {
        o.skills.push(RawSkill {
            skill_id: skill_id.to_string(),
            masteries: masteries.iter().map(|m| entries(m)).collect(),
        });
        o
    }

    fn with_module(
        mut o: RawOperatorCost,
        cn_only: bool,
        phases: &[&[(&str, f64)]],
    ) -> RawOperatorCost {
        o.modules.push(RawModule {
            eq_type: "X".to_string(),
            cn_only,
            phase_costs: phases.iter().map(|p| entries(p)).collect(),
        });
        o
    }

    fn cn(mut o: RawOperatorCost) -> RawOperatorCost {
        o.cn_only = true;
        o
    }

    fn patch(mut o: RawOperatorCost) -> RawOperatorCost {
        o.is_patch = true;
        o
    }

    /// 直近実装の期限を遠い未来にして、大陸版限定だけが直近実装になるようにする。
    fn build(ops: Vec<RawOperatorCost>) -> (AllOperatorsInfo, ValueSet) {
        let names = Arc::new(item_names());
        let values = value_set(&names);
        let mut data = OperatorData::default();
        for o in ops {
            data.name_to_id.insert(o.name.clone(), o.id.clone());
            data.operators.insert(o.id.clone(), o);
        }
        let info = AllOperatorsInfo {
            data: Arc::new(data),
            item_names: names,
            skill_data: Arc::new(SkillData::default()),
            formulas: HashMap::new(),
            recent_since: NaiveDate::from_ymd_opt(2999, 1, 1).unwrap(),
        };
        (info, values)
    }

    fn expect(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    fn position(p: Option<RankingPosition>) -> Option<(u32, usize, usize)> {
        p.map(|p| (p.star, p.rank, p.total))
    }

    fn ranked(entries: &[RankedEntry]) -> Vec<(usize, &str, f64)> {
        entries
            .iter()
            .map(|e| (e.rank, e.name.as_str(), e.risei_value))
            .collect()
    }

    fn error_text<T>(r: Result<T, OperatorCostError>) -> Option<String> {
        r.err().map(|e| e.to_string())
    }

    fn skill_ops() -> Vec<RawOperatorCost> {
        let three = |n: f64| -> Vec<Vec<(&'static str, f64)>> {
            vec![vec![("a", n)], vec![("a", n)], vec![("a", n)]]
        };
        let mut s_a = op("s_a", 6);
        let rows = three(10.0);
        let refs: Vec<&[(&str, f64)]> = rows.iter().map(|r| r.as_slice()).collect();
        s_a = with_skill(s_a, "sk_a1", &refs);
        let mut s_b = op("s_b", 6);
        let rows = three(5.0);
        let refs: Vec<&[(&str, f64)]> = rows.iter().map(|r| r.as_slice()).collect();
        s_b = with_skill(s_b, "sk_b1", &refs);
        s_b = with_skill(s_b, "sk_b2", &[&[("x", 1.0)], &[("x", 2.0)], &[("x", 3.0)]]);
        vec![s_a, s_b]
    }

    #[test]
    fn skill_master_cost_rejects_skill_numbers_outside_the_operators_skills() {
        let (info, values) = build(skill_ops());
        for n in [0, 3] {
            assert_eq!(
                error_text(skill_master_cost(&info, &values, "s_b", n)),
                Some(format!("オペレーター【s_b】のスキル{n}は存在しません")),
                "skill {n}"
            );
        }
    }

    #[test]
    fn skill_master_cost_uses_the_skill_with_the_given_number() {
        let (info, values) = build(skill_ops());
        let first = skill_master_cost(&info, &values, "s_b", 1).ok().unwrap();
        assert_eq!(first.skill_num, 1);
        assert_eq!(first.total.items, expect(&[("素材A", 15.0)]));
        assert_eq!(first.total.risei_value, 15.0);
        assert_eq!(first.masteries.len(), 3);
        assert_eq!(first.masteries[0].items, expect(&[("素材A", 5.0)]));
        assert_eq!(first.masteries[0].risei_value, 5.0);

        let second = skill_master_cost(&info, &values, "s_b", 2).ok().unwrap();
        assert_eq!(second.skill_num, 2);
        assert_eq!(second.total.items, expect(&[("素材X", 6.0)]));
        assert_eq!(second.masteries[2].items, expect(&[("素材X", 3.0)]));
    }

    #[test]
    fn skill_master_cost_ranks_the_skill_among_its_stars_skills_by_risei() {
        let (info, values) = build(skill_ops());
        let rank_of = |name: &str, n: u32| {
            position(
                skill_master_cost(&info, &values, name, n)
                    .ok()
                    .unwrap()
                    .ranking,
            )
        };
        assert_eq!(rank_of("s_a", 1), Some((6, 1, 2)));
        assert_eq!(rank_of("s_b", 1), Some((6, 2, 2)));
        assert_eq!(
            rank_of("s_b", 2),
            None,
            "理性価値が0のスキルはランキング対象外"
        );
    }

    fn elite_ops() -> Vec<RawOperatorCost> {
        vec![
            with_phases(op("g1", 5), &[&[("a", 100.0)], &[("b", 10.0)]]),
            cn(with_phases(op("cn1", 5), &[&[("a", 5.0)]])),
            with_phases(op("g2", 5), &[&[("b", 10.0)]]),
            with_phases(op("d", 5), &[&[("c", 1.0)]]),
            with_phases(op("z", 5), &[&[("x", 5.0)]]),
            patch(with_phases(op("p5", 5), &[&[("a", 999.0)]])),
            with_phases(op("f4", 4), &[&[("a", 1.0)]]),
            with_phases(op("g6", 6), &[&[("a", 1.0)]]),
        ]
    }

    #[test]
    fn operator_elite_cost_ranks_non_patch_five_and_six_star_operators() {
        let (info, values) = build(elite_ops());
        let rank_of = |name: &str| {
            position(
                operator_elite_cost(&info, &values, name)
                    .ok()
                    .unwrap()
                    .ranking,
            )
        };
        assert_eq!(rank_of("g1"), Some((5, 1, 5)));
        assert_eq!(
            rank_of("cn1"),
            Some((5, 2, 5)),
            "大陸限定は大陸版の価値で順位が付く"
        );
        assert_eq!(rank_of("g2"), Some((5, 3, 5)));
        assert_eq!(rank_of("g6"), Some((6, 1, 1)));
        assert_eq!(rank_of("p5"), None, "昇格オペレーターは順位なし");
        assert_eq!(rank_of("f4"), None, "星4は順位なし");
    }

    #[test]
    fn operator_elite_cost_sums_phases_into_total() {
        let (info, values) = build(elite_ops());
        let dto = operator_elite_cost(&info, &values, "g1").ok().unwrap();
        assert_eq!(dto.phases.len(), 2);
        assert_eq!(dto.phases[1].items, expect(&[("素材B", 10.0)]));
        assert_eq!(
            dto.total.items,
            expect(&[("素材A", 100.0), ("素材B", 10.0)])
        );
        assert_eq!(dto.total.risei_value, 130.0);
    }

    #[test]
    fn cost_list_by_elite_lists_operators_above_epsilon_with_overall_rank() {
        let (info, values) = build(elite_ops());
        let dto = cost_list_by_elite(&info, &values, 5, false);
        assert_eq!(dto.star, 5);
        assert_eq!(dto.total_count, 5, "価値0のものも母数に数える");
        assert_eq!(
            ranked(&dto.entries),
            vec![(1, "g1", 130.0), (2, "cn1", 50.0), (3, "g2", 30.0)],
            "EPSILONちょうどの d と価値0の z は載らない"
        );
    }

    #[test]
    fn cost_list_by_elite_only_recent_keeps_overall_rank_of_recent_operators() {
        let (info, values) = build(elite_ops());
        let dto = cost_list_by_elite(&info, &values, 5, true);
        assert_eq!(dto.total_count, 5);
        assert_eq!(ranked(&dto.entries), vec![(2, "cn1", 50.0)]);
    }

    #[test]
    fn cost_list_cost_of_cn_only_adds_cn_only_modules_of_any_operator() {
        let (info, values) = build(vec![
            with_module(
                cn(with_phases(op("cn1", 6), &[&[("a", 5.0)]])),
                true,
                &[&[("b", 2.0)]],
            ),
            with_module(
                with_module(
                    with_phases(op("gx", 6), &[&[("a", 1000.0)]]),
                    true,
                    &[&[("a", 1.0)]],
                ),
                false,
                &[&[("b", 100.0)]],
            ),
        ]);
        let dto = cost_list_cost_of_cn_only(&info, &values);
        assert_eq!(dto.operator_names, vec!["cn1".to_string()]);
        assert_eq!(dto.total_items, expect(&[("素材A", 5.0)]));
        assert_eq!(dto.eq_items, expect(&[("素材A", 1.0), ("素材B", 2.0)]));
        assert_eq!(dto.combined_r2_items, expect(&[("素材A", 6.0)]));
        assert_eq!(
            dto.total_risei_value, 120.0,
            "大陸版の価値で 本体5*10 + モジュール(1*10 + 2*30)"
        );
    }

    #[test]
    fn cost_list_cost_of_global_excludes_cn_only_operators_and_modules() {
        let gl = with_module(
            with_module(
                with_skill(
                    with_phases(op("gl", 6), &[&[("a", 4.0)]]),
                    "sk",
                    &[&[("b", 1.0)]],
                ),
                false,
                &[&[("b", 2.0)]],
            ),
            true,
            &[&[("a", 100.0)]],
        );
        let gp = patch(with_skill(
            with_phases(op("gp", 6), &[&[("a", 50.0)]]),
            "sk_p",
            &[&[("a", 2.0)]],
        ));
        let cn_op = with_module(
            cn(with_phases(op("cn1", 6), &[&[("a", 1000.0)]])),
            false,
            &[&[("b", 1000.0)]],
        );
        let (info, values) = build(vec![gl, gp, cn_op]);
        let dto = cost_list_cost_of_global(&info, &values);
        assert!(dto.operator_names.is_empty());
        assert_eq!(
            dto.total_items,
            expect(&[("素材A", 6.0), ("素材B", 1.0)]),
            "昇格オペレーターは特化だけ数える"
        );
        assert_eq!(dto.eq_items, expect(&[("素材B", 2.0)]));
        assert_eq!(
            dto.total_risei_value, 15.0,
            "グローバル版の価値で 本体(6*1 + 1*3) + モジュール(2*3)"
        );
    }

    fn stats_ops() -> Vec<RawOperatorCost> {
        vec![
            with_skill(op("g_big", 6), "sk1", &[&[("a", 30.0)]]),
            cn(with_skill(op("cn_mid", 6), "sk2", &[&[("a", 2.0)]])),
            with_skill(op("g_small", 6), "sk3", &[&[("a", 10.0)]]),
        ]
    }

    #[test]
    fn cost_list_master_stats_full_reports_heaviest_lightest_and_average() {
        let (info, values) = build(stats_ops());
        let Ok(MasterStatsDto::Full(dto)) = cost_list_master_stats(&info, &values, 6, false) else {
            panic!("full stats expected");
        };
        assert_eq!(dto.star, 6);
        assert_eq!(dto.skill_nums, 3);
        assert_eq!(dto.heaviest_name, "g_big(S1)");
        assert_eq!(dto.heaviest.risei_value, 30.0);
        assert_eq!(dto.lightest_name, "g_small(S1)");
        assert_eq!(dto.lightest.risei_value, 10.0);
        let expected = vec![
            (1, "g_big(S1)", 30.0),
            (2, "cn_mid(S1)", 20.0),
            (3, "g_small(S1)", 10.0),
        ];
        assert_eq!(ranked(&dto.top10_heaviest), expected);
        assert_eq!(
            ranked(&dto.top10_lightest),
            expected,
            "3件なら両方とも全件で、順位は全体の順位"
        );
        assert_eq!(
            dto.average_risei, 20.0,
            "大陸限定は大陸版の価値で換算して平均する"
        );
    }

    #[test]
    fn cost_list_master_stats_only_recent_keeps_overall_rank_of_recent_skills() {
        let (info, values) = build(stats_ops());
        let Ok(MasterStatsDto::Recent(dto)) = cost_list_master_stats(&info, &values, 6, true)
        else {
            panic!("recent stats expected");
        };
        assert_eq!(dto.star, 6);
        assert_eq!(dto.skill_nums, 3, "母数は絞り込み前");
        assert_eq!(ranked(&dto.entries), vec![(2, "cn_mid(S1)", 20.0)]);
    }
}
