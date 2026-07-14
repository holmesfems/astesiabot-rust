use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry_headers};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};

const STAGE_LIST_URL: &str = "https://penguin-stats.io/PenguinStats/api/v2/stages";
const PENGUIN_USER_AGENT: (&str, &str) = ("User-Agent", "ArkPlanner");

pub const SEED_PATH: &str = "data/seed/ark_stages.json";

/// penguin-stats `/stages` の生データ1件（Python の `getStage()` が返す辞書相当）。
/// ここでは生データを保持するのみで、カテゴリ分類やドロップ集計は
/// risei_calculator_engine 側（stage.rs / stage_info.rs）が行う。
#[derive(Serialize, Deserialize, Clone)]
pub struct RawStage {
    pub code: String,
    #[serde(rename = "zoneId")]
    pub zone_id: String,
    #[serde(rename = "stageId")]
    pub stage_id: String,
    #[serde(rename = "apCost")]
    pub ap_cost: Option<i64>,
    #[serde(rename = "stageType")]
    pub stage_type: String,
    #[serde(rename = "minClearTime")]
    pub min_clear_time: Option<f64>,
    #[serde(rename = "dropInfos", default)]
    pub drop_infos: Vec<RawDropInfo>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RawDropInfo {
    #[serde(rename = "itemId")]
    pub item_id: Option<String>,
    #[serde(rename = "dropType")]
    pub drop_type: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ArkStages {
    pub stages: Vec<RawStage>,
}

pub fn fetch() -> BoxFuture<'static, Result<ArkStages, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<ArkStages, FetchError> {
    let client = client();
    let json = fetch_json_with_retry_headers(&client, STAGE_LIST_URL, &[PENGUIN_USER_AGENT]).await?;
    let stages: Vec<RawStage> = serde_json::from_value(json)?;
    Ok(ArkStages { stages })
}
