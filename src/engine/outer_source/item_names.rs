use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

const ITEM_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/item_table.json";
const ITEM_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/item_table.json";

/// Seedの保存先。`cargo run --bin regen_seeds` で手動生成し、git commitして
/// リポジトリに含めておく（起動時fetchが失敗した場合のフォールバック用）。
pub const SEED_PATH: &str = "data/seed/item_names.json";

/// アイテムの id / 中国語名(zh) / 日本語名(ja) 相互変換
/// （Python の `infoFromOuterSource.idtoname.ItemIdToName` 相当）。
/// risei_calculator_engine が理性価値表を組み立てる際の名前解決に使う。
#[derive(Serialize, Deserialize, Default)]
pub struct ItemNames {
    id_to_ja: HashMap<String, String>,
    id_to_zh: HashMap<String, String>,
    zh_to_ja: HashMap<String, String>,
    zh_to_id: HashMap<String, String>,
    ja_to_id: HashMap<String, String>,
}

impl ItemNames {
    /// id→表示名。日本語名が無ければ中国語名、どちらも無ければ"Missing"。
    pub fn get_str(&self, id: &str) -> &str {
        self.id_to_ja
            .get(id)
            .or_else(|| self.id_to_zh.get(id))
            .map(String::as_str)
            .unwrap_or("Missing")
    }

    /// 中国語名→日本語名。対応が無ければ中国語名をそのまま返す。
    pub fn zh_to_ja<'a>(&'a self, zh: &'a str) -> &'a str {
        self.zh_to_ja.get(zh).map(String::as_str).unwrap_or(zh)
    }

    pub fn zh_to_id(&self, zh: &str) -> Option<&str> {
        self.zh_to_id.get(zh).map(String::as_str)
    }

    pub fn ja_to_id(&self, ja: &str) -> Option<&str> {
        self.ja_to_id.get(ja).map(String::as_str)
    }

    /// id→中国語名。無ければ"Missing"。
    pub fn get_zh(&self, id: &str) -> &str {
        self.id_to_zh.get(id).map(String::as_str).unwrap_or("Missing")
    }
}

/// [`super::Source`] に登録するfetch関数。
pub fn fetch() -> BoxFuture<'static, Result<ItemNames, FetchError>> {
    Box::pin(fetch_impl())
}

/// Seedの手動更新ツール（`cargo run --bin regen_seeds`）に登録するジョブ。
pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

#[derive(Deserialize)]
struct CustomIdEntry {
    id: String,
    zh: String,
    ja: String,
}

async fn fetch_impl() -> Result<ItemNames, FetchError> {
    let client = client();
    let (cn_table, jp_table) = tokio::try_join!(
        fetch_json_with_retry(&client, ITEM_TABLE_URL_CN),
        fetch_json_with_retry(&client, ITEM_TABLE_URL_JP),
    )?;
    let cn_items = cn_table.get("items").and_then(Value::as_object).cloned().unwrap_or_default();
    let jp_items = jp_table.get("items").and_then(Value::as_object).cloned().unwrap_or_default();

    let custom_zh_to_ja: HashMap<String, String> = match std::fs::read_to_string("data/customItemZhToJa.yaml") {
        Ok(s) => serde_yaml::from_str(&s).unwrap_or_default(),
        Err(_) => HashMap::new(),
    };

    let mut names = ItemNames::default();

    for (id, cn_value) in cn_items.iter() {
        let Some(cn_name) = cn_value.get("name").and_then(Value::as_str) else {
            continue;
        };
        names.zh_to_id.insert(cn_name.to_string(), id.clone());
        names.id_to_zh.insert(id.clone(), cn_name.to_string());

        let jp_value = jp_items.get(id);
        if let Some(ja_name) = jp_value.and_then(|v| v.get("name")).and_then(Value::as_str) {
            names.zh_to_ja.insert(cn_name.to_string(), ja_name.to_string());
            names.id_to_ja.insert(id.clone(), ja_name.to_string());
            names.ja_to_id.insert(ja_name.to_string(), id.clone());
        } else if let Some(ja_name) = custom_zh_to_ja.get(cn_name) {
            names.zh_to_ja.insert(cn_name.to_string(), ja_name.clone());
            names.id_to_ja.insert(id.clone(), ja_name.clone());
            names.ja_to_id.insert(ja_name.clone(), id.clone());
        }
    }

    // 中国語情報での追加漏れを確認(JPにのみ存在するアイテム)
    for (id, jp_value) in jp_items.iter() {
        if names.id_to_zh.contains_key(id) {
            continue;
        }
        if let Some(ja_name) = jp_value.get("name").and_then(Value::as_str) {
            names.id_to_ja.insert(id.clone(), ja_name.to_string());
            names.ja_to_id.insert(ja_name.to_string(), id.clone());
        }
    }

    apply_custom_item_ids(&mut names);

    Ok(names)
}

/// 理性価値計算で使う特殊なアイテムのIDを補足する
/// （Python の `ItemIdToName.init` 後半、`customItemId.yaml` 適用部分と同じ分岐）。
fn apply_custom_item_ids(names: &mut ItemNames) {
    let entries: Vec<CustomIdEntry> = match std::fs::read_to_string("data/customItemId.yaml") {
        Ok(s) => serde_yaml::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    for item in entries {
        let has_zh = names.id_to_zh.contains_key(&item.id);
        let has_ja = names.id_to_ja.contains_key(&item.id);

        if has_zh && has_ja {
            continue;
        }
        if has_ja {
            // 中国語情報のみ欠損
            let ja_name = names.id_to_ja.get(&item.id).cloned().unwrap();
            names.zh_to_ja.insert(item.zh.clone(), ja_name);
            names.zh_to_id.insert(item.zh.clone(), item.id.clone());
            names.id_to_zh.insert(item.id.clone(), item.zh);
            continue;
        }
        if has_zh {
            // 日本語情報のみ欠損
            let zh_name = names.id_to_zh.get(&item.id).cloned().unwrap();
            names.zh_to_ja.insert(zh_name, item.ja.clone());
            names.id_to_ja.insert(item.id.clone(), item.ja.clone());
            names.ja_to_id.insert(item.ja, item.id);
            continue;
        }
        // 同じ情報(zh jaのセット)が既にある場合、更新によってcustomIDが古くなっているのでスルー
        // 違うIDならこの項目に問題があるのでスルー
        if names.zh_to_id.contains_key(&item.zh) && names.ja_to_id.contains_key(&item.ja) {
            continue;
        }
        if let Some(id_str) = names.zh_to_id.get(&item.zh).cloned() {
            // 中国語で対応するIDがある
            if names.id_to_ja.contains_key(&id_str) {
                continue; // 正しい日本語が既にある
            }
            names.id_to_ja.insert(id_str.clone(), item.ja.clone());
            names.ja_to_id.insert(item.ja.clone(), id_str);
            names.zh_to_ja.insert(item.zh, item.ja);
            continue;
        }
        if let Some(id_str) = names.ja_to_id.get(&item.ja).cloned() {
            // 日本語で対応するIDがある
            if names.id_to_zh.contains_key(&id_str) {
                continue; // 正しい中国語が既にある
            }
            names.zh_to_id.insert(item.zh.clone(), id_str.clone());
            names.zh_to_ja.insert(item.zh.clone(), item.ja);
            names.id_to_zh.insert(id_str, item.zh);
            continue;
        }

        // 完全オリジナルモノ
        names.id_to_ja.insert(item.id.clone(), item.ja.clone());
        names.zh_to_ja.insert(item.zh.clone(), item.ja.clone());
        names.zh_to_id.insert(item.zh.clone(), item.id.clone());
        names.id_to_zh.insert(item.id.clone(), item.zh);
        names.ja_to_id.insert(item.ja, item.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ネットワークでアイテムテーブルを取得できるかの疎通確認。
    /// `cargo test -- --ignored` で明示実行する（通常のCIでは走らせない）。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_gamedata_and_translates_known_item() {
        let names = fetch().await.expect("fetch should succeed against real network");
        let ja = names.zh_to_ja("龙门币");
        assert_ne!(ja, "龙门币", "fetch/突き合わせに失敗している可能性: got {ja}");
        println!("龙门币 -> {ja}");
    }
}
