use crate::engine::outer_source::item_names::ItemNames;
use std::collections::{HashMap, HashSet};

/// アイテムID→個数のベクトル。素材合成レシピの換算計算（calculator.rs の
/// ConvertionMatrix）でのみ使う、Python `rcutils.itemArray.ItemArray` の
/// 必要最小限の移植。課金パック(kakin)・ガチャ数換算まわりのメソッドは
/// 今回のスコープ外のため意図的に省略している。
#[derive(Clone, Default)]
pub struct ItemArray {
    dict: HashMap<String, f64>,
}

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
            dict: self.dict.iter().map(|(k, v)| (k.clone(), v * factor)).collect(),
        }
    }

    pub fn sub(&self, other: &ItemArray) -> ItemArray {
        let mut result = self.clone();
        result.add_assign(&other.scaled(-1.0));
        result
    }

    pub fn filter_by_zh(&self, zh_list: &[&str], item_names: &ItemNames) -> ItemArray {
        let ids: HashSet<&str> = zh_list.iter().filter_map(|zh| item_names.zh_to_id(zh)).collect();
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

    pub fn to_zh_count_dict(&self, item_names: &ItemNames) -> HashMap<String, f64> {
        self.dict
            .iter()
            .map(|(id, v)| (item_names.get_zh(id).to_string(), *v))
            .collect()
    }
}
