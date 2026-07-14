use super::kakin::{CCExchangeItem, KakinPackDef};
use super::server::{load_stage_category_file, StageCategoryFile};
use super::Error;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

pub const PRICE_PATH: &str = "data/risei/price.yaml";
pub const PRICE_SPECIAL_PATH: &str = "data/risei/price_special.yaml";
pub const CONST_VALUES_PATH: &str = "data/risei/const_values.yaml";
pub const MIN_CLEAR_TIME_INJECTION_PATH: &str = "data/risei/min_clear_time_injection.yaml";
pub const PRICE_CC_PATH: &str = "data/risei/price_cc.yaml";
pub const KAKIN_LIST_PATH: &str = "data/risei/price_kakin.yaml";
pub const CONST_GACHA_PATH: &str = "data/risei/const_gacha.yaml";

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
    /// 契約賞金引換証(cclist)の交換レート。Python `price_cc4.yaml`。
    pub price_cc: Vec<CCExchangeItem>,
    /// 課金パック一覧(riseikakin)。グローバル版のみ（Mainland版は元々オミット済み。`Design.md`参照）。
    /// YAML記載順を表示順として使うため`IndexMap`。
    pub kakin_list: IndexMap<String, KakinPackDef>,
    /// ガチャ数換算係数（ja名→1個あたりのガチャ数換算値）。Python `rcutils/constGacha.yaml`。
    pub const_gacha: HashMap<String, f64>,
}

impl StaticData {
    pub fn load() -> Result<Self, Error> {
        Ok(Self {
            price: load_yaml_map(PRICE_PATH)?,
            price_special: load_yaml_map(PRICE_SPECIAL_PATH)?,
            const_values: Arc::new(load_yaml_map(CONST_VALUES_PATH)?),
            min_clear_time_injection: load_yaml_map(MIN_CLEAR_TIME_INJECTION_PATH)?,
            stage_category: load_stage_category_file()?,
            price_cc: load_yaml(PRICE_CC_PATH)?,
            kakin_list: load_yaml(KAKIN_LIST_PATH)?,
            const_gacha: load_yaml_map(CONST_GACHA_PATH)?,
        })
    }
}

fn load_yaml_map(path: &str) -> Result<HashMap<String, f64>, Error> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&s)?)
}

fn load_yaml<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, Error> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&s)?)
}
