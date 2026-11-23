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

/// ステージ1件分のドロップ集計。
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

/// 1マップ分の情報。理性(Sanity)軸のみを扱い、
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

    /// 作戦コード（Re版のサフィックス付き）。
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

    pub fn add_drop_record(
        &mut self,
        record: &RawDropRecord,
        value_target: &[&str],
        item_names: &ItemNames,
    ) {
        let zh = item_names.get_zh(&record.item_id);
        if value_target.contains(&zh) {
            self.drop_list.merge_from(DropList::from_record(record));
        }
    }

    pub fn max_times(&self) -> i64 {
        self.drop_list.max_times()
    }

    pub fn is_valid_for_show(
        &self,
        show_min_times: i64,
        value_target: &[&str],
        item_names: &ItemNames,
    ) -> bool {
        if self.max_times() < show_min_times {
            return false;
        }
        self.to_drop_array(value_target, item_names).sum() >= EPSILON
    }

    /// ドロップ配列（龍門幣1000に理性消費分のボーナスを加算済み）。
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

    pub fn get_partial_efficiency(&self, values: &RiseiValues, items: &[&str]) -> f64 {
        let total_value: f64 = items
            .iter()
            .map(|zh| {
                values.get_value_from_zh(zh)
                    * self.get_drop_rate(zh, &values.value_target, &values.item_names)
            })
            .sum();
        total_value / self.ap_cost
    }

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
    pub fn get_max_efficiency_item(&self, item_names: &ItemNames) -> Option<(String, f64)> {
        self.drop_list
            .drop_dict
            .iter()
            .max_by(|a, b| a.1.drop_rate.total_cmp(&b.1.drop_rate))
            .map(|(id, item)| (item_names.get_str(id).to_string(), item.drop_rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[(&str, f64, i64)], max_times: i64) -> DropList {
        DropList {
            drop_dict: entries
                .iter()
                .map(|(id, rate, times)| {
                    (
                        id.to_string(),
                        DropItem {
                            drop_rate: *rate,
                            times: *times,
                        },
                    )
                })
                .collect(),
            max_times,
        }
    }

    #[test]
    fn merge_from_takes_newer_value_for_duplicate_item() {
        let mut a = list(&[("x", 0.1, 100), ("y", 0.2, 100)], 100);
        a.merge_from(list(&[("x", 0.5, 50)], 50));
        assert_eq!(a.drop_dict["x"].drop_rate, 0.5);
        assert_eq!(a.drop_dict["x"].times, 50);
        assert_eq!(a.drop_dict["y"].drop_rate, 0.2);
    }

    #[test]
    fn merge_from_keeps_larger_max_times_in_both_directions() {
        let mut a = list(&[("x", 0.1, 100)], 100);
        a.merge_from(list(&[("y", 0.1, 500)], 500));
        assert_eq!(a.max_times(), 500);

        let mut b = list(&[("x", 0.1, 500)], 500);
        b.merge_from(list(&[("y", 0.1, 100)], 100));
        assert_eq!(b.max_times(), 500);
    }

    use crate::engine::external_source::ark_stages::RawDropInfo;
    use crate::engine::risei_calculator_engine::server::{value_target, Server};
    use std::sync::Arc;

    const ID_A: &str = "item_a";
    const ID_B: &str = "item_b";
    const ZH_A: &str = "基础作战记录";
    const ZH_B: &str = "初级作战记录";

    fn names() -> ItemNames {
        serde_json::from_value(serde_json::json!({
            "id_to_ja": {ID_A: "ja_a", ID_B: "ja_b"},
            "id_to_zh": {ID_A: ZH_A, ID_B: ZH_B},
            "zh_to_id": {ZH_A: ID_A, ZH_B: ID_B},
            "ja_to_id": {"ja_a": ID_A, "ja_b": ID_B},
        }))
        .unwrap()
    }

    fn raw(min_clear_time: Option<f64>, drops: &[(Option<&str>, &str)]) -> RawStage {
        RawStage {
            code: "1-7".to_string(),
            zone_id: "main_1".to_string(),
            stage_id: "main_01-07".to_string(),
            ap_cost: Some(10),
            stage_type: "MAIN".to_string(),
            min_clear_time,
            drop_infos: drops
                .iter()
                .map(|(id, ty)| RawDropInfo {
                    item_id: id.map(str::to_string),
                    drop_type: ty.to_string(),
                })
                .collect(),
        }
    }

    fn stage_with(drops: DropList) -> StageItem {
        let mut s = StageItem::from_raw(&raw(None, &[]), "zone".to_string(), &HashMap::new());
        s.drop_list = drops;
        s
    }

    fn values(a: f64, b: f64, dev_a: f64, dev_b: f64) -> RiseiValues {
        let target = value_target(Server::Mainland);
        let pos = |zh: &str| target.iter().position(|x| *x == zh).unwrap();
        let mut value_array = DVector::zeros(target.len());
        value_array[pos(ZH_A)] = a;
        value_array[pos(ZH_B)] = b;
        let mut dev_array = DVector::zeros(target.len());
        dev_array[pos(ZH_A)] = dev_a;
        dev_array[pos(ZH_B)] = dev_b;
        let mut v = RiseiValues::new(
            Server::Mainland,
            value_array,
            Arc::new(names()),
            Arc::new(HashMap::new()),
        );
        v.set_dev_array(dev_array);
        v
    }

    #[test]
    fn from_raw_converts_min_clear_time_from_milliseconds_to_seconds() {
        let s = StageItem::from_raw(&raw(Some(5000.0), &[]), "z".to_string(), &HashMap::new());
        assert!((s.min_clear_time - 5.0).abs() < 1e-9);
        let none = StageItem::from_raw(&raw(None, &[]), "z".to_string(), &HashMap::new());
        assert_eq!(none.min_clear_time, 0.0);
    }

    #[test]
    fn from_raw_prefers_injected_min_clear_time_for_the_stage_code() {
        let inj = HashMap::from([("1-7".to_string(), 42.0)]);
        let s = StageItem::from_raw(&raw(Some(5000.0), &[]), "z".to_string(), &inj);
        assert_eq!(s.min_clear_time, 42.0);
    }

    #[test]
    fn main_drop_string_lists_only_normal_drops_with_ids() {
        let drops = [
            (Some(ID_A), "NORMAL_DROP"),
            (Some(ID_B), "SPECIAL_DROP"),
            (None, "NORMAL_DROP"),
        ];
        let s = StageItem::from_raw(&raw(None, &drops), "z".to_string(), &HashMap::new());
        assert_eq!(s.get_main_drop_ja_str(&names()), " ja_a");
    }

    #[test]
    fn main_drop_string_joins_several_items_and_is_empty_without_any() {
        let drops = [(Some(ID_A), "NORMAL_DROP"), (Some(ID_B), "NORMAL_DROP")];
        let s = StageItem::from_raw(&raw(None, &drops), "z".to_string(), &HashMap::new());
        assert_eq!(s.get_main_drop_ja_str(&names()), " ja_a ja_b");

        let empty = StageItem::from_raw(&raw(None, &[]), "z".to_string(), &HashMap::new());
        assert_eq!(empty.get_main_drop_ja_str(&names()), "");
    }

    #[test]
    fn is_valid_for_show_requires_max_times_at_least_the_minimum() {
        let target = value_target(Server::Mainland);
        let s = stage_with(list(&[(ID_A, 0.5, 2000)], 2000));
        assert!(!s.is_valid_for_show(2001, &target, &names()));
        assert!(s.is_valid_for_show(2000, &target, &names()));
        assert!(s.is_valid_for_show(1999, &target, &names()));
    }

    #[test]
    fn partial_efficiency_sums_value_times_drop_rate_over_ap_cost() {
        let s = stage_with(list(&[(ID_A, 0.5, 100), (ID_B, 0.25, 100)], 100));
        let v = values(2.0, 3.0, 0.0, 0.0);
        // (2*0.5 + 3*0.25) / 10
        let both = s.get_partial_efficiency(&v, &[ZH_A, ZH_B]);
        assert!((both - 0.175).abs() < 1e-9, "both = {both}");
        let only_a = s.get_partial_efficiency(&v, &[ZH_A]);
        assert!((only_a - 0.1).abs() < 1e-9, "only_a = {only_a}");
    }

    #[test]
    fn std_dev_combines_item_variances_weighted_by_value() {
        // 端数 0.5 / 試行 101 -> sigma^2 = 0.25/100、端数 0.2 / 試行 51 -> sigma^2 = 0.16/50。
        let s = stage_with(list(&[(ID_A, 0.5, 101), (ID_B, 1.2, 51)], 101));
        let v = values(2.0, 3.0, 0.4, 0.1);
        // dev1 = 0.0025*2^2 + 0.0032*3^2、dev2 = 0.5^2*0.4^2 + 1.2^2*0.1^2
        let expected = (0.0025 * 4.0 + 0.0032 * 9.0 + 0.25 * 0.16 + 1.44 * 0.01_f64).sqrt();
        let got = s.get_std_dev(&v);
        assert!(
            (got - expected).abs() < 1e-9,
            "got = {got}, expected = {expected}"
        );
    }

    #[test]
    fn max_efficiency_item_is_the_highest_drop_rate_with_its_display_name() {
        let s = stage_with(list(&[(ID_A, 0.5, 100), (ID_B, 0.2, 100)], 100));
        let (name, rate) = s.get_max_efficiency_item(&names()).unwrap();
        assert_eq!(name, "ja_a");
        assert!((rate - 0.5).abs() < 1e-9);

        let s = stage_with(list(&[(ID_A, 0.1, 100), (ID_B, 0.2, 100)], 100));
        let (name, rate) = s.get_max_efficiency_item(&names()).unwrap();
        assert_eq!(name, "ja_b");
        assert!((rate - 0.2).abs() < 1e-9);
    }

    #[test]
    fn max_efficiency_item_is_none_without_drops() {
        assert!(stage_with(DropList::default())
            .get_max_efficiency_item(&names())
            .is_none());
    }
}
