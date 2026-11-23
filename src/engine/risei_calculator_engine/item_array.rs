use crate::engine::external_source::item_names::ItemNames;
use std::collections::{HashMap, HashSet};

/// アイテムID→個数のベクトル。素材合成レシピの換算計算（calculator.rs の
/// ConvertionMatrix）でのみ使う最小限の配列型。
#[derive(Clone, Default)]
pub struct ItemArray {
    dict: HashMap<String, f64>,
}

/// ゼロ近傍とみなす閾値。`stage.rs` の `EPSILON`（収束判定用）とは別物。
const NORMALIZE_EPSILON: f64 = 0.0001;

impl ItemArray {
    pub fn from_id_count(dict: HashMap<String, f64>) -> Self {
        Self { dict }
    }

    pub fn add_assign(&mut self, other: &ItemArray) {
        for (key, value) in &other.dict {
            *self.dict.entry(key.clone()).or_insert(0.0) += value;
        }
    }

    pub fn scaled(&self, factor: f64) -> ItemArray {
        ItemArray {
            dict: self
                .dict
                .iter()
                .map(|(k, v)| (k.clone(), v * factor))
                .collect(),
        }
    }

    pub fn sub(&self, other: &ItemArray) -> ItemArray {
        let mut result = self.clone();
        result.add_assign(&other.scaled(-1.0));
        result
    }

    pub fn filter_by_zh(&self, zh_list: &[&str], item_names: &ItemNames) -> ItemArray {
        let ids: HashSet<&str> = zh_list
            .iter()
            .filter_map(|zh| item_names.zh_to_id(zh))
            .collect();
        ItemArray {
            dict: self
                .dict
                .iter()
                .filter(|(id, _)| ids.contains(id.as_str()))
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        }
    }

    pub fn total_count(&self) -> f64 {
        self.dict.values().sum()
    }

    /// 中国語名→個数の辞書に変換する。ゼロ近傍の項は除く。
    pub fn to_zh_count_dict(&self, item_names: &ItemNames) -> HashMap<String, f64> {
        self.dict
            .iter()
            .filter(|(_, v)| v.abs() > NORMALIZE_EPSILON)
            .map(|(id, v)| (item_names.get_zh(id).to_string(), *v))
            .collect()
    }

    /// id→個数の中身を複製する（`operator_cost_calc`が`ItemCost`へ持ち替えるために使う）。
    pub fn to_id_count_dict(&self) -> HashMap<String, f64> {
        self.dict.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_names() -> ItemNames {
        serde_json::from_value(serde_json::json!({
            "id_to_ja": {"a": "ja_a", "b": "ja_b"},
            "id_to_zh": {"a": "zh_a", "b": "zh_b"},
            "zh_to_id": {"zh_a": "a", "zh_b": "b"},
            "ja_to_id": {"ja_a": "a", "ja_b": "b"},
        }))
        .unwrap()
    }

    fn array(entries: &[(&str, f64)]) -> ItemArray {
        ItemArray::from_id_count(entries.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }

    #[test]
    fn to_zh_count_dict_drops_entries_within_epsilon() {
        let names = item_names();
        let dict = array(&[("a", NORMALIZE_EPSILON), ("b", -NORMALIZE_EPSILON * 0.5)])
            .to_zh_count_dict(&names);
        assert!(dict.is_empty(), "{dict:?}");
    }

    #[test]
    fn to_zh_count_dict_keeps_entries_beyond_epsilon_with_both_signs() {
        let names = item_names();
        let dict = array(&[
            ("a", NORMALIZE_EPSILON * 1.01),
            ("b", -NORMALIZE_EPSILON * 1.01),
        ])
        .to_zh_count_dict(&names);
        assert_eq!(dict.len(), 2);
        assert!(dict["zh_a"] > 0.0 && dict["zh_b"] < 0.0);
    }
}
