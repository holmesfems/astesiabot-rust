use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const FORMULA_URL: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/building_data.json";

pub const SEED_PATH: &str = "data/seed/formulas.json";

/// 素材合成レシピ1件の生データ（Python の `FormulaItem` のコンストラクタ引数相当）。
/// 実際の合成換算行列への変換は risei_calculator_engine 側（calculator.rs）が行う。
#[derive(Serialize, Deserialize, Clone)]
pub struct RawFormula {
    #[serde(rename = "itemId")]
    pub item_id: String,
    pub count: f64,
    pub costs: Vec<RawFormulaCost>,
    #[serde(rename = "extraOutcomeGroup", default)]
    pub extra_outcome_group: Vec<RawFormulaOutcome>,
    #[serde(rename = "goldCost")]
    pub gold_cost: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RawFormulaCost {
    pub id: String,
    pub count: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RawFormulaOutcome {
    pub weight: f64,
    #[serde(rename = "itemId")]
    pub item_id: String,
    #[serde(rename = "itemCount")]
    pub item_count: f64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Formulas {
    pub formulas: Vec<RawFormula>,
}

pub fn fetch() -> BoxFuture<'static, Result<Formulas, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<Formulas, FetchError> {
    let client = client();
    let json = fetch_json_with_retry(&client, FORMULA_URL).await?;
    let mut formulas = Vec::new();
    if let Some(Value::Object(map)) = json.get("workshopFormulas") {
        for value in map.values() {
            formulas.push(serde_json::from_value(value.clone())?);
        }
    }
    Ok(Formulas { formulas })
}
