use super::server::{value_target, Server};
use crate::engine::outer_source::item_names::ItemNames;
use nalgebra::DVector;
use std::collections::HashMap;
use std::sync::Arc;

/// 理性消費ベースの理性価値表（Python `RiseiOrTimeValues` からTIME軸を除いたもの）。
/// 時間軸(`CalculateMode.TIME`)は使われていなかったため移植していない。
#[derive(Clone)]
pub struct RiseiValues {
    pub server: Server,
    pub value_target: Vec<&'static str>,
    pub value_array: DVector<f64>,
    pub dev_array: DVector<f64>,
    pub item_names: Arc<ItemNames>,
    /// value_target に含まれないアイテム(スカウト券・合成玉など)のフォールバック定数値
    /// （`data/risei/const_values.yaml`。Python `RiseiOrTimeValues.__constValueDict`）。
    pub const_values: Arc<HashMap<String, f64>>,
}

impl RiseiValues {
    pub fn new(
        server: Server,
        value_array: DVector<f64>,
        item_names: Arc<ItemNames>,
        const_values: Arc<HashMap<String, f64>>,
    ) -> Self {
        let target = value_target(server);
        let dev_array = DVector::zeros(target.len());
        Self {
            server,
            value_target: target,
            value_array,
            dev_array,
            item_names,
            const_values,
        }
    }

    pub fn set_dev_array(&mut self, dev_array: DVector<f64>) {
        self.dev_array = dev_array;
    }

    fn index_of(&self, zh: &str) -> Option<usize> {
        self.value_target.iter().position(|x| *x == zh)
    }

    /// 中国語名から理性価値を取得。value_target外なら定数辞書にフォールバック
    /// （Python `getValueFromZH`）。
    pub fn get_value_from_zh(&self, zh: &str) -> f64 {
        match self.index_of(zh) {
            Some(idx) => self.value_array[idx],
            None => {
                let ja = self.item_names.zh_to_ja(zh);
                self.const_values.get(ja).copied().unwrap_or(0.0)
            }
        }
    }

    pub fn get_value_from_ja(&self, ja: &str) -> f64 {
        match self.item_names.ja_to_id(ja) {
            Some(id) => {
                let zh = self.item_names.get_zh(id).to_string();
                self.get_value_from_zh(&zh)
            }
            None => self.const_values.get(ja).copied().unwrap_or(0.0),
        }
    }

    pub fn get_std_dev_from_zh(&self, zh: &str) -> f64 {
        match self.index_of(zh) {
            Some(idx) => self.dev_array[idx],
            None => 0.0,
        }
    }
}
