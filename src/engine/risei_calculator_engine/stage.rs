use super::values::RiseiValues;
use crate::engine::external_source::ark_matrix::RawDropRecord;
use crate::engine::external_source::ark_stages::RawStage;
use crate::engine::external_source::item_names::ItemNames;
use nalgebra::DVector;
use std::collections::HashMap;

pub const EPSILON: f64 = 1e-6;

#[derive(Clone, Copy)]
pub struct DropItem {
    pub drop_rate: f64,
    pub times: i64,
}

/// ステージ1件分のドロップ集計（Python `DropList`）。
/// Python版の `minTimes()` は常に0を返す未使用の欠陥コードだったため、
/// 意図的に移植していない（`maxTimes()` のみ実際に使われている）。
#[derive(Clone, Default)]
pub struct DropList {
    drop_dict: HashMap<String, DropItem>,
    max_times: i64,
}

impl DropList {
    pub fn from_record(record: &RawDropRecord) -> Self {
        let mut drop_dict = HashMap::new();
        drop_dict.insert(
            record.item_id.clone(),
            DropItem {
                drop_rate: record.quantity / record.times as f64,
                times: record.times,
            },
        );
        Self {
            drop_dict,
            max_times: record.times,
        }
    }

    /// 重複するアイテムIDは新しいものを適用する（Python `DropList.__iadd__`)。
    pub fn merge_from(&mut self, other: DropList) {
        for (key, value) in other.drop_dict {
            self.drop_dict.insert(key, value);
        }
        if other.max_times > self.max_times {
            self.max_times = other.max_times;
        }
    }

    pub fn max_times(&self) -> i64 {
        self.max_times
    }

    pub fn to_drop_array(&self, value_target: &[&str], item_names: &ItemNames) -> DVector<f64> {
        let mut arr = DVector::zeros(value_target.len());
        for (i, zh) in value_target.iter().enumerate() {
            if let Some(id) = item_names.zh_to_id(zh) {
                if let Some(item) = self.drop_dict.get(id) {
                    arr[i] = item.drop_rate;
                }
            }
        }
        arr
    }

    fn to_times_array(&self, value_target: &[&str], item_names: &ItemNames) -> DVector<f64> {
        let mut arr = DVector::from_element(value_target.len(), 2.0);
        for (i, zh) in value_target.iter().enumerate() {
            if let Some(id) = item_names.zh_to_id(zh) {
                if let Some(item) = self.drop_dict.get(id) {
                    arr[i] = item.times as f64;
                }
            }
        }
        arr
    }

    pub fn to_std_dev_array(&self, value_target: &[&str], item_names: &ItemNames) -> DVector<f64> {
        let drop = self.to_drop_array(value_target, item_names);
        let times = self.to_times_array(value_target, item_names);
        DVector::from_iterator(
            value_target.len(),
            (0..value_target.len()).map(|i| {
                let extra = drop[i].rem_euclid(1.0);
                let t = times[i] - 1.0;
                (extra * (1.0 - extra) / t).sqrt()
            }),
        )
    }
}

/// 1マップ分の情報（Python `StageItem`）。理性(Sanity)軸のみを扱い、
/// 時間(Time)軸のコスト分岐は持たない。`minClearTime` は表示用の付随情報として保持する。
pub struct StageItem {
    pub name: String,
    pub zone_id: String,
    pub zone_name: String,
    pub stage_id: String,
    pub ap_cost: f64,
    pub min_clear_time: f64,
    main_drop_ids: Vec<String>,
    drop_list: DropList,
}

impl StageItem {
    pub fn from_raw(
        raw: &RawStage,
        zone_name: String,
        min_clear_time_injection: &HashMap<String, f64>,
    ) -> Self {
        let min_clear_time_ms = raw.min_clear_time.unwrap_or(0.0);
        let min_clear_time = min_clear_time_injection
            .get(&raw.code)
            .copied()
            .unwrap_or(min_clear_time_ms / 1000.0);
        let main_drop_ids = raw
            .drop_infos
            .iter()
            .filter(|d| d.drop_type == "NORMAL_DROP")
            .filter_map(|d| d.item_id.clone())
            .collect();
        Self {
            name: raw.code.clone(),
            zone_id: raw.zone_id.clone(),
            zone_name,
            stage_id: raw.stage_id.clone(),
            ap_cost: raw.ap_cost.unwrap_or(0) as f64,
            min_clear_time,
            main_drop_ids,
            drop_list: DropList::default(),
        }
    }

    /// 作戦コード（Re版のサフィックス付き）。Python `nameWithReplicate`。
    pub fn name_with_replicate(&self) -> String {
        if self.zone_id.contains("re_") {
            format!("{}(Re)", self.name)
        } else {
            self.name.clone()
        }
    }

    pub fn get_main_drop_ja_str(&self, item_names: &ItemNames) -> String {
        let msg = self
            .main_drop_ids
            .iter()
            .map(|id| item_names.get_str(id))
            .collect::<Vec<_>>()
            .join(" ");
        if msg.is_empty() {
            String::new()
        } else {
            format!(" {msg}")
        }
    }

    /// matrix由来のドロップ記録を1件追加する（value_targetに含まれる場合のみ。
    /// Python `StageItem.addDropList`）。
    pub fn add_drop_record(&mut self, record: &RawDropRecord, value_target: &[&str], item_names: &ItemNames) {
        let zh = item_names.get_zh(&record.item_id);
        if value_target.contains(&zh) {
            self.drop_list.merge_from(DropList::from_record(record));
        }
    }

    pub fn max_times(&self) -> i64 {
        self.drop_list.max_times()
    }

    pub fn is_valid_for_show(&self, show_min_times: i64, value_target: &[&str], item_names: &ItemNames) -> bool {
        if self.max_times() < show_min_times {
            return false;
        }
        self.to_drop_array(value_target, item_names).sum() >= EPSILON
    }

    /// ドロップ配列（理性消費に応じた龍門幣1000のボーナスを加算済み）。
    pub fn to_drop_array(&self, value_target: &[&str], item_names: &ItemNames) -> DVector<f64> {
        let mut arr = self.drop_list.to_drop_array(value_target, item_names);
        if let Some(lmd_index) = value_target.iter().position(|x| *x == "龙门币1000") {
            arr[lmd_index] += self.ap_cost * 0.012;
        }
        arr
    }

    pub fn get_drop_rate(&self, zh: &str, value_target: &[&str], item_names: &ItemNames) -> f64 {
        match value_target.iter().position(|x| *x == zh) {
            Some(idx) => self.to_drop_array(value_target, item_names)[idx],
            None => 0.0,
        }
    }

    pub fn to_std_dev_array(&self, value_target: &[&str], item_names: &ItemNames) -> DVector<f64> {
        self.drop_list.to_std_dev_array(value_target, item_names)
    }

    /// 理性効率（総合ドロップ価値 / 理性消費）。
    pub fn get_efficiency(&self, values: &RiseiValues) -> f64 {
        let drop_array = self.to_drop_array(&values.value_target, &values.item_names);
        let total_value: f64 = drop_array
            .iter()
            .zip(values.value_array.iter())
            .map(|(d, v)| d * v)
            .sum();
        total_value / self.ap_cost
    }

    /// 指定アイテム群だけに絞った効率（Python `getPartialEfficiency`）。
    pub fn get_partial_efficiency(&self, values: &RiseiValues, items: &[&str]) -> f64 {
        let total_value: f64 = items
            .iter()
            .map(|zh| values.get_value_from_zh(zh) * self.get_drop_rate(zh, &values.value_target, &values.item_names))
            .sum();
        total_value / self.ap_cost
    }

    /// 理性効率の誤差項（Python `getStdDev`）。
    pub fn get_std_dev(&self, values: &RiseiValues) -> f64 {
        let std_dev = self.to_std_dev_array(&values.value_target, &values.item_names);
        let drop_array = self.to_drop_array(&values.value_target, &values.item_names);
        let dev1: f64 = std_dev
            .iter()
            .zip(values.value_array.iter())
            .map(|(s, v)| s * s * v * v)
            .sum();
        let dev2: f64 = drop_array
            .iter()
            .zip(values.dev_array.iter())
            .map(|(d, e)| d * d * e * e)
            .sum();
        (dev1 + dev2).sqrt()
    }

    /// 最もドロップ率の高い主ドロップアイテム(名前, ドロップ率)。
    /// `is_valid_for_show` を通過済みのステージ（ドロップが1件以上ある）でのみ呼ぶ想定。
    pub fn get_max_efficiency_item(&self, item_names: &ItemNames) -> Option<(String, f64)> {
        self.drop_list
            .drop_dict
            .iter()
            .max_by(|a, b| a.1.drop_rate.total_cmp(&b.1.drop_rate))
            .map(|(id, item)| (item_names.get_str(id).to_string(), item.drop_rate))
    }
}
