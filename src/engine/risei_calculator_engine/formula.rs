use super::item_array::ItemArray;
use crate::engine::outer_source::formulas::RawFormula;
use crate::engine::outer_source::item_names::ItemNames;
use std::collections::HashMap;

/// 素材合成レシピ1件（Python `formulation.FormulaItem`）。
pub struct FormulaItem {
    pub output_item_id: String,
    pub name: String,
    base_array: ItemArray,
    self_array: ItemArray,
    outcome_array: ItemArray,
    gold_cost_array: ItemArray,
}

impl FormulaItem {
    pub fn from_raw(raw: &RawFormula, item_names: &ItemNames) -> Self {
        let mut base = HashMap::new();
        for cost in &raw.costs {
            *base.entry(cost.id.clone()).or_insert(0.0) += cost.count;
        }

        let mut self_map = HashMap::new();
        self_map.insert(raw.item_id.clone(), -raw.count);

        let mut outcome = HashMap::new();
        for o in &raw.extra_outcome_group {
            *outcome.entry(o.item_id.clone()).or_insert(0.0) += o.weight * o.item_count;
        }

        let mut gold = HashMap::new();
        if let Some(lmd_id) = item_names.ja_to_id("龍門幣1000") {
            gold.insert(lmd_id.to_string(), raw.gold_cost / 1000.0);
        }

        Self {
            output_item_id: raw.item_id.clone(),
            name: item_names.get_str(&raw.item_id).to_string(),
            base_array: ItemArray::from_id_count(base),
            self_array: ItemArray::from_id_count(self_map),
            outcome_array: ItemArray::from_id_count(outcome),
            gold_cost_array: ItemArray::from_id_count(gold),
        }
    }

    pub fn to_formula_array(&self) -> ItemArray {
        let mut result = self.base_array.clone();
        result.add_assign(&self.self_array);
        result.add_assign(&self.gold_cost_array);
        result
    }

    /// 合成時に副産物として得られるアイテムの期待値
    /// （`value_target` に含まれるものだけを対象に、重み比率×ドロップ率で換算）。
    pub fn to_outcome_array(&self, drop_rate: f64, value_target: &[&str], item_names: &ItemNames) -> ItemArray {
        let filtered = self.outcome_array.filter_by_zh(value_target, item_names);
        let total = filtered.total_count();
        if total.abs() < f64::EPSILON {
            // Python版はここでゼロ除算し得るが、現行データでは発生しない前提。
            // 安全側に倒して空配列を返す。
            return filtered;
        }
        filtered.scaled(1.0 / total).scaled(drop_rate)
    }

    pub fn to_formula_array_with_outcome(
        &self,
        drop_rate: f64,
        value_target: &[&str],
        item_names: &ItemNames,
    ) -> ItemArray {
        self.to_formula_array()
            .sub(&self.to_outcome_array(drop_rate, value_target, item_names))
    }
}
