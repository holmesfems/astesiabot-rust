use crate::engine::external_source::item_names::ItemNames;
use std::collections::{HashMap, HashSet};

/// アイテムID→個数のベクトル。素材合成レシピの換算計算（calculator.rs の
/// ConvertionMatrix）でのみ使う、
/// 必要最小限の配列型。課金パック(kakin)・ガチャ数換算まわりのメソッドは
/// スコープ外のため意図的に持たせていない。
#[derive(Clone, Default)]
pub struct ItemArray {
    dict: HashMap<String, f64>,
}

/// `normalize` でゼロ近傍とみなす閾値。
/// `stage.rs` の `EPSILON`（1e-6、理性効率の収束判定用）とは別物なので混同しないこと。
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

    /// 中国語名→個数の辞書に変換する（内部で`normalize()`相当の除去を行う）。
    /// `normalize()`はゼロ近傍(`abs(value) <= NORMALIZE_EPSILON`)の項を除去してから
    /// 辞書化するため、ここでも同じ閾値でフィルタする（順序・龍門幣統合は辞書化に
    /// 無関係のため移植不要）。
    pub fn to_zh_count_dict(&self, item_names: &ItemNames) -> HashMap<String, f64> {
        self.dict
            .iter()
            .filter(|(_, v)| v.abs() > NORMALIZE_EPSILON)
            .map(|(id, v)| (item_names.get_zh(id).to_string(), *v))
            .collect()
    }

    /// id→個数の生の中身をそのまま複製する（`engine::operator_cost_calc`が合成レシピの
    /// 中級換算(`rare3and4ToRare2`)で、この型からアイテムIDベースの`ItemCost`へ持ち替える際に使う）。
    pub fn to_id_count_dict(&self) -> HashMap<String, f64> {
        self.dict.clone()
    }
}
