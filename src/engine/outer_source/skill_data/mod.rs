use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

mod description;
mod raw;

use raw::RawSkillLevel;

const SKILL_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/skill_table.json";
const SKILL_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/skill_table.json";

pub const SEED_PATH: &str = "data/seed/skill_data.json";

/// スキル1件分（Python `SkillIdToName.SkillItem`相当）。
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SkillItem {
    pub name: String,
    /// 最大レベルの説明文（ヘッダ込みで組み立て済み）。説明文が無いスキルは空文字列。
    pub description: String,
}

/// スキルID→名前/説明文（Python `SkillIdToName`相当。旧`SkillNames`を統合）。
#[derive(Serialize, Deserialize, Default)]
pub struct SkillData {
    id_to_item: HashMap<String, SkillItem>,
}

impl SkillData {
    /// idから名前を解決する。無ければ"Missing"（Python `SkillIdToName.getStr`と同じフォールバック）。
    pub fn get_str(&self, id: &str) -> &str {
        self.id_to_item.get(id).map(|item| item.name.as_str()).unwrap_or("Missing")
    }

    /// idから説明文を解決する。無ければ空文字列（Python `SkillIdToName.getDescription`と同じフォールバック）。
    pub fn get_description(&self, id: &str) -> &str {
        self.id_to_item.get(id).map(|item| item.description.as_str()).unwrap_or("")
    }

    #[cfg(test)]
    pub fn empty_for_test() -> Self {
        Self::default()
    }
}

pub fn fetch() -> BoxFuture<'static, Result<SkillData, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<SkillData, FetchError> {
    let client = client();
    let (cn_table, jp_table) = tokio::try_join!(
        fetch_json_with_retry(&client, SKILL_TABLE_URL_CN),
        fetch_json_with_retry(&client, SKILL_TABLE_URL_JP),
    )?;

    let mut id_to_item = HashMap::new();
    if let Value::Object(cn_map) = &cn_table {
        for (id, cn_value) in cn_map {
            let jp_value = jp_table.get(id.as_str());
            let source = jp_value.unwrap_or(cn_value);
            // Python版は`levels[-1]`のskillJsonから名前/説明文を組み立てる。
            let Some(last_level) = source.get("levels").and_then(Value::as_array).and_then(|levels| levels.last()) else {
                continue;
            };
            let Ok(level) = serde_json::from_value::<RawSkillLevel>(last_level.clone()) else {
                continue;
            };
            let description = description::build_description(&level);
            id_to_item.insert(
                id.clone(),
                SkillItem {
                    name: level.name,
                    description,
                },
            );
        }
    }
    Ok(SkillData { id_to_item })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ネットワークでスキル名/説明文を取得できるかの疎通確認。`cargo test -- --ignored` で明示実行する。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_gamedata() {
        let data = fetch().await.expect("fetch should succeed against real network");
        assert!(!data.id_to_item.is_empty());
        let has_description = data.id_to_item.values().any(|item| !item.description.is_empty());
        assert!(has_description, "at least some skills should have a non-empty description");
    }
}
