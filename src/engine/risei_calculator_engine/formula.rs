use super::item_array::ItemArray;
use crate::engine::external_source::formulas::RawFormula;
use crate::engine::external_source::item_names::ItemNames;
use std::collections::HashMap;

/// 素材合成レシピ1件。
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
    pub fn to_outcome_array(
        &self,
        drop_rate: f64,
        value_target: &[&str],
        item_names: &ItemNames,
    ) -> ItemArray {
        let filtered = self.outcome_array.filter_by_zh(value_target, item_names);
        let total = filtered.total_count();
        if total.abs() < f64::EPSILON {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::formulas::RawFormulaOutcome;

    fn item_names() -> ItemNames {
        serde_json::from_value(serde_json::json!({
            "id_to_ja": {"a": "ja_a", "b": "ja_b"},
            "id_to_zh": {"a": "zh_a", "b": "zh_b"},
            "zh_to_id": {"zh_a": "a", "zh_b": "b"},
            "ja_to_id": {"ja_a": "a", "ja_b": "b"},
        }))
        .unwrap()
    }

    fn formula(outcomes: &[(&str, f64, f64)]) -> FormulaItem {
        let raw = RawFormula {
            item_id: "a".to_string(),
            count: 1.0,
            costs: Vec::new(),
            extra_outcome_group: outcomes
                .iter()
                .map(|(id, weight, count)| RawFormulaOutcome {
                    weight: *weight,
                    item_id: id.to_string(),
                    item_count: *count,
                })
                .collect(),
            gold_cost: 0.0,
        };
        FormulaItem::from_raw(&raw, &item_names())
    }

    #[test]
    fn outcome_array_sums_to_drop_rate_and_keeps_ratios() {
        let names = item_names();
        let f = formula(&[("a", 30.0, 1.0), ("b", 10.0, 1.0)]);
        let out = f.to_outcome_array(0.4, &["zh_a", "zh_b"], &names);
        assert!((out.total_count() - 0.4).abs() < 1e-12);
        let dict = out.to_zh_count_dict(&names);
        assert!((dict["zh_a"] / dict["zh_b"] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn outcome_array_with_zero_total_has_no_nan_or_inf() {
        let names = item_names();
        let out = formula(&[("a", 0.0, 1.0)]).to_outcome_array(0.4, &["zh_a"], &names);
        assert!(out.to_id_count_dict().values().all(|v| v.is_finite()));
        assert_eq!(out.total_count(), 0.0);
        // value_target outside items only
        let out = formula(&[("b", 1.0, 1.0)]).to_outcome_array(0.4, &["zh_a"], &names);
        assert!(out.to_id_count_dict().values().all(|v| v.is_finite()));
        assert_eq!(out.total_count(), 0.0);
    }
}
