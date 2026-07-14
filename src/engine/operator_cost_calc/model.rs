use crate::engine::outer_source::formulas::RawFormula;
use crate::engine::outer_source::item_names::ItemNames;
use crate::engine::outer_source::operator_data::CostEntry;
use crate::engine::risei_calculator_engine::formula::FormulaItem;
use crate::engine::risei_calculator_engine::server::{self, Server};
use crate::engine::risei_calculator_engine::values::RiseiValues;
use indexmap::IndexMap;
use std::collections::HashMap;

/// Python `rcutils.itemArray.ItemArray` の `EPSILON = 0.0001`
/// （`normalize()` でこの絶対値以下の項を除去する。表示整形の丸め判定用の
/// `1e-6`（charmaterials.py側のEPSILON、formatレイヤで使う）とは別物）。
const NORMALIZE_EPSILON: f64 = 0.0001;

/// 素材合成レシピの参照表（アイテムID→レシピ）。Python `Formula.__idToFormula`相当。
/// `risei_calculator_engine::formula::FormulaItem` をそのまま再利用する
/// （合成レシピの構造自体はrisei_calculator_engineと共有のドメインのため）。
pub type FormulaMap = HashMap<String, FormulaItem>;

pub fn build_formula_map(formulas: &[RawFormula], item_names: &ItemNames) -> FormulaMap {
    formulas
        .iter()
        .map(|raw| (raw.item_id.clone(), FormulaItem::from_raw(raw, item_names)))
        .collect()
}

/// アイテムID→個数の消費素材データ（Python `charmaterials.ItemCost`）。
///
/// 内部表現は挿入順を保持する`IndexMap`（Python dictの挿入順保持セマンティクスを
/// 正確に再現するため）。`normalize()`（表示直前の並び替え）はタイになった項の
/// 順序を「その時点の挿入順」で安定ソートするため、挿入順の忠実な再現が
/// Python版と1文字レベルで出力一致するための前提になる
/// （例: スキル特化3段の合計に現れる複数のSoC芯片アイテムの並び順）。
#[derive(Clone, Default, Debug)]
pub struct ItemCost {
    dict: IndexMap<String, f64>,
}

impl ItemCost {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Python `ItemCost.__init__`。同一id重複時は上書き(最後の値が勝つ)、
    /// 位置は初出時のまま（Python dictの`d[key]=value`と同じ挙動）。
    pub fn from_cost_entries(entries: &[CostEntry]) -> Self {
        let mut dict = IndexMap::new();
        for e in entries {
            dict.insert(e.id.clone(), e.count);
        }
        Self { dict }
    }

    /// Python `ItemCost.__iadd__`/`__add__`。既存キーは加算(位置は変えない)、
    /// 新規キーは`other`側の順で末尾に追加。
    pub fn add(&self, other: &ItemCost) -> ItemCost {
        let mut dict = self.dict.clone();
        for (id, value) in &other.dict {
            *dict.entry(id.clone()).or_insert(0.0) += value;
        }
        Self { dict }
    }

    pub fn sum(items: &[ItemCost]) -> ItemCost {
        items.iter().fold(ItemCost::empty(), |acc, item| acc.add(item))
    }

    pub fn scaled(&self, factor: f64) -> ItemCost {
        Self {
            dict: self.dict.iter().map(|(k, v)| (k.clone(), v * factor)).collect(),
        }
    }

    fn get(&self, id: &str) -> f64 {
        self.dict.get(id).copied().unwrap_or(0.0)
    }

    /// Python `ItemArray.normalizeGold`。龍門幣→龍門幣1000(/1000)へ統合し、元キーは削除する。
    fn normalize_gold(&self, item_names: &ItemNames) -> ItemCost {
        let mut dict = self.dict.clone();
        let Some(gold_id) = item_names.ja_to_id("龍門幣") else {
            return Self { dict };
        };
        let gold_value = dict.get(gold_id).copied().unwrap_or(0.0);
        if gold_value == 0.0 {
            return Self { dict };
        }
        let Some(gold1000_id) = item_names.ja_to_id("龍門幣1000").map(str::to_string) else {
            return Self { dict };
        };
        *dict.entry(gold1000_id).or_insert(0.0) += gold_value / 1000.0;
        dict.shift_remove(gold_id);
        Self { dict }
    }

    /// Python `ItemArray.normalize`(gold統合 + `value_target(Mainland)`順の安定ソート + ε除去) +
    /// `toZHStrCountDict`/`toNameCountDict`が呼ぶ`normalize()`まで込み。
    /// 順序基準は常にMainland版基準(`getValueTarget(False)`固定。Python `_ORDERCRITERIA`)。
    fn normalized(&self, item_names: &ItemNames) -> Vec<(String, f64)> {
        let gold_merged = self.normalize_gold(item_names);
        let order = server::value_target(Server::Mainland);
        let index_of = |id: &str| -> usize {
            let zh = item_names.get_zh(id);
            order.iter().position(|x| *x == zh).unwrap_or(order.len())
        };
        let mut items: Vec<(String, f64)> = gold_merged.dict.into_iter().collect();
        items.sort_by_key(|(id, _)| index_of(id));
        items.retain(|(_, v)| v.abs() > NORMALIZE_EPSILON);
        items
    }

    /// 表示用: 日本語名×個数（normalize済み順）。Python `ItemArray.toNameCountDict`。
    pub fn ordered_name_counts(&self, item_names: &ItemNames) -> Vec<(String, f64)> {
        self.normalized(item_names)
            .into_iter()
            .map(|(id, v)| (item_names.get_str(&id).to_string(), v))
            .collect()
    }

    /// 職SoC(32xx系chip、8職×3段)→汎用3種(320X_CUSTOM)へ集約する。理性価値計算専用
    /// （表示(`ordered_name_counts`)には使わない）。Python `ItemArray.normalizeSoC`。
    fn normalize_soc(&self) -> ItemCost {
        const GROUPS: [(&[&str], &str); 3] = [
            (&["3211", "3221", "3231", "3241", "3251", "3261", "3271", "3281"], "3201_CUSTOM"),
            (&["3212", "3222", "3232", "3242", "3252", "3262", "3272", "3282"], "3202_CUSTOM"),
            (&["3213", "3223", "3233", "3243", "3253", "3263", "3273", "3283"], "3203_CUSTOM"),
        ];
        let mut result = ItemCost::empty();
        for (id, value) in &self.dict {
            let mapped = GROUPS
                .iter()
                .find(|(ids, _)| ids.contains(&id.as_str()))
                .map(|(_, custom)| *custom)
                .unwrap_or(id.as_str());
            *result.dict.entry(mapped.to_string()).or_insert(0.0) += value;
        }
        result
    }

    /// Python `RiseiOrTimeValues.getValueFromItemArray`
    /// (`normalizeSoC()`→`toZHStrCountDict()`(=normalize込み)→`getValueFromZH`合計)。
    pub fn to_risei_value(&self, values: &RiseiValues, item_names: &ItemNames) -> f64 {
        self.normalize_soc()
            .normalized(item_names)
            .iter()
            .map(|(id, count)| count * values.get_value_from_zh(item_names.get_zh(id)))
            .sum()
    }

    /// Python `RiseiOrTimeValues.getValueFromItemArray_OnlyValueTarget`
    /// (normalizeSoCなし。normalize済みの上で`value_target(values.server)`内のみ合計)。
    pub fn to_risei_value_only_value_target(&self, values: &RiseiValues, item_names: &ItemNames) -> f64 {
        let target = server::value_target(values.server);
        self.normalized(item_names)
            .iter()
            .filter(|(id, _)| target.contains(&item_names.get_zh(id)))
            .map(|(id, count)| count * values.get_value_from_zh(item_names.get_zh(id)))
            .sum()
    }

    /// Python `ItemCost.filterRare2`。常にMainland基準(`getItemRarity2(False)`固定)。
    fn filter_rare2(&self, item_names: &ItemNames) -> ItemCost {
        let list = server::item_rarity2(Server::Mainland);
        Self {
            dict: self
                .dict
                .iter()
                .filter(|(id, _)| list.contains(&item_names.get_zh(id)))
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        }
    }

    /// 上級/中級素材→初級素材への換算（副産物無し）。常にMainland基準の
    /// 素材リストで走査する(Python `rare3and4ToRare2`は`glob`引数を取らず常に`False`固定)。
    /// レシピが存在しない(=合成不可)素材はそのまま残り、最後の`filterRare2`で
    /// 除外される(Python版と同じ挙動)。
    pub fn rare3and4_to_rare2(&self, item_names: &ItemNames, formulas: &FormulaMap) -> ItemCost {
        let mut result = self.clone();
        let mut targets: Vec<&str> = server::item_rarity4(Server::Mainland);
        targets.extend(server::item_rarity3(Server::Mainland));
        for zh in targets {
            let Some(id) = item_names.zh_to_id(zh) else { continue };
            let count = result.get(id);
            if count == 0.0 {
                continue;
            }
            let Some(formula_item) = formulas.get(id) else { continue };
            let scaled: IndexMap<String, f64> = formula_item
                .to_formula_array()
                .to_id_count_dict()
                .into_iter()
                .map(|(k, v)| (k, v * count))
                .collect();
            // Python: `formulaArray.normalize()` を加算前に呼ぶ。
            let normalized = ItemCost { dict: scaled }.normalized(item_names);
            let contribution = ItemCost {
                dict: normalized.into_iter().collect(),
            };
            result = result.add(&contribution);
        }
        result.filter_rare2(item_names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(pairs: &[(&str, f64)]) -> ItemCost {
        ItemCost::from_cost_entries(
            &pairs
                .iter()
                .map(|(id, count)| CostEntry {
                    id: id.to_string(),
                    count: *count,
                })
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn from_cost_entries_last_value_wins_keeping_first_position() {
        let c = cost(&[("a", 1.0), ("b", 2.0), ("a", 5.0)]);
        assert_eq!(c.get("a"), 5.0);
        assert_eq!(c.dict.keys().collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn add_accumulates_existing_and_appends_new_keeping_order() {
        let a = cost(&[("a", 1.0), ("b", 2.0)]);
        let b = cost(&[("b", 3.0), ("c", 4.0)]);
        let sum = a.add(&b);
        assert_eq!(sum.dict.keys().collect::<Vec<_>>(), vec!["a", "b", "c"]);
        assert_eq!(sum.get("b"), 5.0);
    }
}
