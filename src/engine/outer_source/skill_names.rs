use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

const SKILL_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/skill_table.json";
const SKILL_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/skill_table.json";

pub const SEED_PATH: &str = "data/seed/skill_names.json";

/// スキルID→表示名のみ（Python `SkillIdToName`相当だが、説明文(blackboard置換)は
/// スコープ外のため意図的に省略している。skillMasterCostのタイトル表示にのみ使う）。
#[derive(Serialize, Deserialize, Default)]
pub struct SkillNames {
    id_to_name: HashMap<String, String>,
}

impl SkillNames {
    /// idから名前を解決する。無ければ"Missing"（Python `SkillIdToName.getStr`と同じフォールバック）。
    pub fn get_str(&self, id: &str) -> &str {
        self.id_to_name.get(id).map(String::as_str).unwrap_or("Missing")
    }

    #[cfg(test)]
    pub fn empty_for_test() -> Self {
        Self::default()
    }
}

pub fn fetch() -> BoxFuture<'static, Result<SkillNames, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<SkillNames, FetchError> {
    let client = client();
    let (cn_table, jp_table) = tokio::try_join!(
        fetch_json_with_retry(&client, SKILL_TABLE_URL_CN),
        fetch_json_with_retry(&client, SKILL_TABLE_URL_JP),
    )?;

    let mut id_to_name = HashMap::new();
    if let Value::Object(cn_map) = &cn_table {
        for (id, cn_value) in cn_map {
            let jp_value = jp_table.get(id.as_str());
            let source = jp_value.unwrap_or(cn_value);
            // Python版は`levels[-1]`のskillJsonから`name`を読むが、レベル間で名前は
            // 変わらないため`levels`の最初の要素から読んでも同じ結果になる。
            let Some(name) = source
                .get("levels")
                .and_then(Value::as_array)
                .and_then(|levels| levels.first())
                .and_then(|level| level.get("name"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            id_to_name.insert(id.clone(), name.to_string());
        }
    }
    Ok(SkillNames { id_to_name })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ネットワークでスキル名を取得できるかの疎通確認。`cargo test -- --ignored` で明示実行する。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_gamedata() {
        let names = fetch().await.expect("fetch should succeed against real network");
        assert!(!names.id_to_name.is_empty());
    }
}
