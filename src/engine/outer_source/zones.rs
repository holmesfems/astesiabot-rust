use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry_headers};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

const ZONE_TABLE_URL: &str = "https://penguin-stats.io/PenguinStats/api/v2/zones";
const PENGUIN_USER_AGENT: (&str, &str) = ("User-Agent", "ArkPlanner");

pub const SEED_PATH: &str = "data/seed/zones.json";

/// zoneId → 日本語ゾーン名（Python の `infoFromOuterSource.idtoname.ZoneIdToName` 相当）。
#[derive(Serialize, Deserialize, Default)]
pub struct Zones {
    id_to_ja: HashMap<String, String>,
}

impl Zones {
    pub fn get_str(&self, id: &str) -> &str {
        self.id_to_ja.get(id).map(String::as_str).unwrap_or("Missing")
    }
}

pub fn fetch() -> BoxFuture<'static, Result<Zones, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<Zones, FetchError> {
    let client = client();
    let json = fetch_json_with_retry_headers(&client, ZONE_TABLE_URL, &[PENGUIN_USER_AGENT]).await?;
    let mut id_to_ja = HashMap::new();
    if let Value::Array(items) = json {
        for item in items {
            let Some(zone_id) = item.get("zoneId").and_then(Value::as_str) else {
                continue;
            };
            let Some(ja) = item
                .get("zoneName_i18n")
                .and_then(|v| v.get("ja"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            id_to_ja.insert(zone_id.to_string(), ja.to_string());
        }
    }
    Ok(Zones { id_to_ja })
}
