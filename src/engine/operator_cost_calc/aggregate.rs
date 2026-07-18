use super::model::ItemCost;
use crate::engine::external_source::operator_data::{RawModule, RawOperatorCost};

/// Python `OperatorCosts.totalPhaseCost`。昇進1,2の消費素材合計。
pub fn total_phase_cost(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op.phases.iter().map(|p| ItemCost::from_cost_entries(p)).collect();
    ItemCost::sum(&items)
}

/// Python `OperatorCosts.totalSkillMasterCost`。全スキルの特化1〜3の消費素材合計。
pub fn total_skill_master_cost(op: &RawOperatorCost) -> ItemCost {
    let per_skill: Vec<ItemCost> = op
        .skills
        .iter()
        .map(|s| {
            let costs: Vec<ItemCost> = s.masteries.iter().map(|m| ItemCost::from_cost_entries(m)).collect();
            ItemCost::sum(&costs)
        })
        .collect();
    ItemCost::sum(&per_skill)
}

/// Python `OperatorCosts.totalSkillLv7Cost`。スキルLv1→7の消費素材合計。
pub fn total_skill_lv7_cost(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op.all_skill_lvlup.iter().map(|c| ItemCost::from_cost_entries(c)).collect();
    ItemCost::sum(&items)
}

fn module_total_cost(module: &RawModule) -> ItemCost {
    let items: Vec<ItemCost> = module.phase_costs.iter().map(|c| ItemCost::from_cost_entries(c)).collect();
    ItemCost::sum(&items)
}

/// Python `EQCost.allUEQCost`。全モジュール(大陸限定含む)の合計。
pub fn total_unique_eq_cost(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op.modules.iter().map(module_total_cost).collect();
    ItemCost::sum(&items)
}

/// Python `EQCost.cnOnlyUEQCost`。大陸限定モジュールのみの合計。
pub fn total_unique_eq_cost_cn_only(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op.modules.iter().filter(|m| m.cn_only).map(module_total_cost).collect();
    ItemCost::sum(&items)
}

/// Python `EQCost.globalUEQCost`。実装済み(非大陸限定)モジュールのみの合計。
pub fn total_unique_eq_cost_global(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op.modules.iter().filter(|m| !m.cn_only).map(module_total_cost).collect();
    ItemCost::sum(&items)
}

pub fn has_unique_eq(op: &RawOperatorCost) -> bool {
    !op.modules.is_empty()
}

pub fn has_cn_only_ueq(op: &RawOperatorCost) -> bool {
    op.modules.iter().any(|m| m.cn_only)
}

/// Python `OperatorCosts.allCostExceptEq`。昇格オペレーターは元オペレーターと
/// 昇進/スキルLv7素材を共有するため、ここでは計上しない。
pub fn all_cost_except_eq(op: &RawOperatorCost) -> ItemCost {
    let mut ret = total_skill_master_cost(op);
    if !op.is_patch {
        ret = ret.add(&total_phase_cost(op));
        ret = ret.add(&total_skill_lv7_cost(op));
    }
    ret
}
