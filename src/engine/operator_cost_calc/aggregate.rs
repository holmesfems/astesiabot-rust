use super::model::ItemCost;
use crate::engine::external_source::operator_data::{RawModule, RawOperatorCost};

/// 昇進1,2の消費素材合計。
pub fn total_phase_cost(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op
        .phases
        .iter()
        .map(|p| ItemCost::from_cost_entries(p))
        .collect();
    ItemCost::sum(&items)
}

/// 全スキルの特化1〜3の消費素材合計。
pub fn total_skill_master_cost(op: &RawOperatorCost) -> ItemCost {
    let per_skill: Vec<ItemCost> = op
        .skills
        .iter()
        .map(|s| {
            let costs: Vec<ItemCost> = s
                .masteries
                .iter()
                .map(|m| ItemCost::from_cost_entries(m))
                .collect();
            ItemCost::sum(&costs)
        })
        .collect();
    ItemCost::sum(&per_skill)
}

/// スキルLv1→7の消費素材合計。
pub fn total_skill_lv7_cost(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op
        .all_skill_lvlup
        .iter()
        .map(|c| ItemCost::from_cost_entries(c))
        .collect();
    ItemCost::sum(&items)
}

fn module_total_cost(module: &RawModule) -> ItemCost {
    let items: Vec<ItemCost> = module
        .phase_costs
        .iter()
        .map(|c| ItemCost::from_cost_entries(c))
        .collect();
    ItemCost::sum(&items)
}

/// 全モジュール(大陸限定含む)の合計。
pub fn total_unique_eq_cost(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op.modules.iter().map(module_total_cost).collect();
    ItemCost::sum(&items)
}

/// 大陸限定モジュールのみの合計。
pub fn total_unique_eq_cost_cn_only(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op
        .modules
        .iter()
        .filter(|m| m.cn_only)
        .map(module_total_cost)
        .collect();
    ItemCost::sum(&items)
}

/// 実装済み(非大陸限定)モジュールのみの合計。
pub fn total_unique_eq_cost_global(op: &RawOperatorCost) -> ItemCost {
    let items: Vec<ItemCost> = op
        .modules
        .iter()
        .filter(|m| !m.cn_only)
        .map(module_total_cost)
        .collect();
    ItemCost::sum(&items)
}

pub fn has_unique_eq(op: &RawOperatorCost) -> bool {
    !op.modules.is_empty()
}

pub fn has_cn_only_ueq(op: &RawOperatorCost) -> bool {
    op.modules.iter().any(|m| m.cn_only)
}

/// 昇格オペレーターは元オペレーターと
/// 昇進/スキルLv7素材を共有するため、ここでは計上しない。
pub fn all_cost_except_eq(op: &RawOperatorCost) -> ItemCost {
    let mut ret = total_skill_master_cost(op);
    if !op.is_patch {
        ret = ret.add(&total_phase_cost(op));
        ret = ret.add(&total_skill_lv7_cost(op));
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::item_names::ItemNames;
    use crate::engine::external_source::operator_data::{CostEntry, RawSkill};

    fn item_names() -> ItemNames {
        serde_json::from_value(serde_json::json!({
            "id_to_ja": {"a": "素材A", "b": "素材B"},
            "id_to_zh": {"a": "固源岩组", "b": "提纯源岩"},
            "zh_to_id": {"固源岩组": "a", "提纯源岩": "b"},
            "ja_to_id": {"素材A": "a", "素材B": "b"},
        }))
        .unwrap()
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

    fn module(cn_only: bool, phases: &[&[(&str, f64)]]) -> RawModule {
        RawModule {
            eq_type: "X".to_string(),
            cn_only,
            phase_costs: phases.iter().map(|p| entries(p)).collect(),
        }
    }

    fn op() -> RawOperatorCost {
        RawOperatorCost {
            id: "op".to_string(),
            name: "op".to_string(),
            cn_name: "op".to_string(),
            stars: 6,
            cn_only: false,
            is_patch: false,
            phases: Vec::new(),
            skills: Vec::new(),
            all_skill_lvlup: Vec::new(),
            modules: Vec::new(),
        }
    }

    fn items(cost: &ItemCost) -> Vec<(String, f64)> {
        cost.ordered_name_counts(&item_names())
    }

    fn expect(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    #[test]
    fn skill_master_total_sums_every_mastery_of_every_skill() {
        let mut o = op();
        o.skills = vec![
            RawSkill {
                skill_id: "s1".to_string(),
                masteries: vec![
                    entries(&[("a", 1.0)]),
                    entries(&[("a", 2.0), ("b", 1.0)]),
                    entries(&[("b", 4.0)]),
                ],
            },
            RawSkill {
                skill_id: "s2".to_string(),
                masteries: vec![entries(&[("a", 10.0)])],
            },
        ];
        assert_eq!(
            items(&total_skill_master_cost(&o)),
            expect(&[("素材A", 13.0), ("素材B", 5.0)])
        );
    }

    #[test]
    fn skill_lv7_total_sums_every_level() {
        let mut o = op();
        o.all_skill_lvlup = vec![entries(&[("a", 3.0)]), entries(&[("a", 4.0), ("b", 2.0)])];
        assert_eq!(
            items(&total_skill_lv7_cost(&o)),
            expect(&[("素材A", 7.0), ("素材B", 2.0)])
        );
    }

    #[test]
    fn unique_eq_totals_split_into_all_cn_only_and_global() {
        let mut o = op();
        o.modules = vec![
            module(false, &[&[("a", 1.0)], &[("a", 2.0)]]),
            module(true, &[&[("b", 5.0)], &[("b", 7.0)]]),
            module(false, &[&[("a", 100.0)]]),
        ];
        assert_eq!(
            items(&total_unique_eq_cost(&o)),
            expect(&[("素材A", 103.0), ("素材B", 12.0)])
        );
        assert_eq!(
            items(&total_unique_eq_cost_cn_only(&o)),
            expect(&[("素材B", 12.0)])
        );
        assert_eq!(
            items(&total_unique_eq_cost_global(&o)),
            expect(&[("素材A", 103.0)])
        );
    }

    #[test]
    fn has_unique_eq_is_true_only_when_the_operator_has_a_module() {
        let mut o = op();
        assert!(!has_unique_eq(&o));
        o.modules = vec![module(false, &[&[("a", 1.0)]])];
        assert!(has_unique_eq(&o));
    }

    #[test]
    fn has_cn_only_ueq_is_true_only_when_some_module_is_cn_only() {
        let mut o = op();
        assert!(!has_cn_only_ueq(&o), "モジュールが無ければ false");
        o.modules = vec![module(false, &[&[("a", 1.0)]])];
        assert!(!has_cn_only_ueq(&o), "実装済みモジュールだけなら false");
        o.modules.push(module(true, &[&[("a", 1.0)]]));
        assert!(has_cn_only_ueq(&o));
    }

    fn op_with_all_costs() -> RawOperatorCost {
        let mut o = op();
        o.phases = vec![entries(&[("a", 1.0)]), entries(&[("a", 2.0)])];
        o.all_skill_lvlup = vec![entries(&[("b", 10.0)])];
        o.skills = vec![RawSkill {
            skill_id: "s1".to_string(),
            masteries: vec![entries(&[("a", 100.0)])],
        }];
        o.modules = vec![module(false, &[&[("b", 1000.0)]])];
        o
    }

    #[test]
    fn all_cost_except_eq_adds_phase_lv7_and_master_but_not_modules() {
        assert_eq!(
            items(&all_cost_except_eq(&op_with_all_costs())),
            expect(&[("素材A", 103.0), ("素材B", 10.0)])
        );
    }

    #[test]
    fn all_cost_except_eq_of_a_patch_operator_counts_only_master() {
        let mut o = op_with_all_costs();
        o.is_patch = true;
        assert_eq!(items(&all_cost_except_eq(&o)), expect(&[("素材A", 100.0)]));
    }
}
