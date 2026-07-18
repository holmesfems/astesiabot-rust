use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry_headers};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};

const MATRIX_URL: &str =
    "https://penguin-stats.io/PenguinStats/api/v2/result/matrix?server=CN&show_closed_zones=true";
const PENGUIN_USER_AGENT: (&str, &str) = ("User-Agent", "ArkPlanner");

pub const SEED_PATH: &str = "data/seed/ark_matrix.json";

/// penguin-stats `/result/matrix` の生データ1件（Python の `getMatrix()` が
/// 返す配列の要素相当）。`server` は常にCN固定
/// （Python版のコメント「matrix代入, targetServerはCNで固定」を踏襲）。
#[derive(Serialize, Deserialize, Clone)]
pub struct RawDropRecord {
    #[serde(rename = "stageId")]
    pub stage_id: String,
    #[serde(rename = "itemId")]
    pub item_id: String,
    pub times: i64,
    pub quantity: f64,
    pub start: i64,
    pub end: Option<i64>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ArkMatrix {
    pub matrix: Vec<RawDropRecord>,
}

pub fn fetch() -> BoxFuture<'static, Result<ArkMatrix, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<ArkMatrix, FetchError> {
    let client = client();
    let json = fetch_json_with_retry_headers(&client, MATRIX_URL, &[PENGUIN_USER_AGENT]).await?;
    let matrix: Vec<RawDropRecord> = json
        .get("matrix")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    Ok(ArkMatrix { matrix })
}
