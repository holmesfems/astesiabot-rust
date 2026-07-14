use super::server::{load_stage_category_file, StageCategoryFile};
use super::Error;
use std::collections::HashMap;
use std::sync::Arc;

pub const CONST_VALUES_PATH: &str = "data/risei/const_values.yaml";
pub const MIN_CLEAR_TIME_INJECTION_PATH: &str = "data/risei/min_clear_time_injection.yaml";

/// `data/risei/` 配下の静的データのうち、`Calculator::build`が構造的に必要とするもの一式
/// （価格表など特定のriseiXXXコマンドだけが使うデータは各commands/risei/*.rsが自前で
/// 読み込む。`price.yaml`/`price_special.yaml`/`price_cc.yaml`は`riseilists.rs`へ移設済み）。
/// 起動時に一度だけ読み込み、以後は不変。
pub struct StaticData {
    /// value_targetに含まれないアイテムのフォールバック定数値（ja名→値）。
    pub const_values: Arc<HashMap<String, f64>>,
    /// 一部ステージの実測クリア時間の手動補正（作戦コード→秒）。
    pub min_clear_time_injection: HashMap<String, f64>,
    pub stage_category: StageCategoryFile,
}

impl StaticData {
    pub fn load() -> Result<Self, Error> {
        Ok(Self {
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
