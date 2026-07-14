use super::server::{load_stage_category_file, StageCategoryFile};
use super::Error;
use std::collections::HashMap;
use std::sync::Arc;

pub const PRICE_PATH: &str = "data/risei/price.yaml";
pub const PRICE_SPECIAL_PATH: &str = "data/risei/price_special.yaml";
pub const CONST_VALUES_PATH: &str = "data/risei/const_values.yaml";
pub const MIN_CLEAR_TIME_INJECTION_PATH: &str = "data/risei/min_clear_time_injection.yaml";

/// `data/risei/` 配下の静的データ一式。起動時に一度だけ読み込み、以後は不変。
pub struct StaticData {
    /// 初級資格証(te2list)の交換レート（zh名→価格）。Python `price.yaml`。
    pub price: HashMap<String, f64>,
    /// 特別引換証(special_list)の交換レート。Python `price_special.yaml`。
    pub price_special: HashMap<String, f64>,
    /// value_targetに含まれないアイテムのフォールバック定数値（ja名→値）。
    pub const_values: Arc<HashMap<String, f64>>,
    /// 一部ステージの実測クリア時間の手動補正（作戦コード→秒）。
    pub min_clear_time_injection: HashMap<String, f64>,
    pub stage_category: StageCategoryFile,
}

impl StaticData {
    pub fn load() -> Result<Self, Error> {
        Ok(Self {
            price: load_yaml_map(PRICE_PATH)?,
            price_special: load_yaml_map(PRICE_SPECIAL_PATH)?,
            const_values: Arc::new(load_yaml_map(CONST_VALUES_PATH)?),
            min_clear_time_injection: load_yaml_map(MIN_CLEAR_TIME_INJECTION_PATH)?,
            stage_category: load_stage_category_file()?,
        })
    }
}

fn load_yaml_map(path: &str) -> Result<HashMap<String, f64>, Error> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&s)?)
}
