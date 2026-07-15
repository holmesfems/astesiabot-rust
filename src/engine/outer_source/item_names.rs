use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

const ITEM_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/item_table.json";
const ITEM_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/item_table.json";

/// Seedの保存先。`cargo run --bin regen_seeds` で手動生成し、git commitして
/// リポジトリに含めておく（起動時fetchが失敗した場合のフォールバック用）。
pub const SEED_PATH: &str = "data/seed/item_names.json";

/// アイテムの id / 中国語名(zh) / 日本語名(ja) 相互変換
/// （Python の `infoFromOuterSource.idtoname.ItemIdToName` 相当）。
/// risei_calculator_engine が理性価値表を組み立てる際の名前解決に使う。
///
/// `zh_to_ja` は独立したマップとして持たず `zh_to_id` → `id_to_ja` の連鎖で導出する
/// （id/zh/jaが常に対で登録される不変条件があるため、導出結果は専用マップを
/// 持っていた場合と一致する）。各マップの文字列は `Arc<str>` で共有し、id/zh/jaの
/// 実体が複数マップに重複してヒープ確保されるのを避けている（4マップ全部O(1)のまま）。
#[derive(Serialize, Deserialize, Default)]
pub struct ItemNames {
    id_to_ja: HashMap<Arc<str>, Arc<str>>,
    id_to_zh: HashMap<Arc<str>, Arc<str>>,
    zh_to_id: HashMap<Arc<str>, Arc<str>>,
    ja_to_id: HashMap<Arc<str>, Arc<str>>,
}

impl ItemNames {
    /// id→表示名。日本語名が無ければ中国語名、どちらも無ければ"Missing"。
    pub fn get_str(&self, id: &str) -> &str {
        self.id_to_ja
            .get(id)
            .or_else(|| self.id_to_zh.get(id))
            .map(AsRef::as_ref)
            .unwrap_or("Missing")
    }

    /// 中国語名→日本語名。対応が無ければ中国語名をそのまま返す。
    pub fn zh_to_ja<'a>(&'a self, zh: &'a str) -> &'a str {
        self.zh_to_id
            .get(zh)
            .and_then(|id| self.id_to_ja.get(id.as_ref()))
            .map(AsRef::as_ref)
            .unwrap_or(zh)
    }

    pub fn zh_to_id(&self, zh: &str) -> Option<&str> {
        self.zh_to_id.get(zh).map(AsRef::as_ref)
    }

    pub fn ja_to_id(&self, ja: &str) -> Option<&str> {
        self.ja_to_id.get(ja).map(AsRef::as_ref)
    }

    /// id→中国語名。無ければ"Missing"。
    pub fn get_zh(&self, id: &str) -> &str {
        self.id_to_zh.get(id).map(AsRef::as_ref).unwrap_or("Missing")
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
        let id: Arc<str> = Arc::from(id.as_str());
        let cn_name: Arc<str> = Arc::from(cn_name);
        names.zh_to_id.insert(cn_name.clone(), id.clone());
        names.id_to_zh.insert(id.clone(), cn_name.clone());

        let jp_value = jp_items.get(id.as_ref());
        if let Some(ja_name) = jp_value.and_then(|v| v.get("name")).and_then(Value::as_str) {
            let ja_name: Arc<str> = Arc::from(ja_name);
            names.id_to_ja.insert(id.clone(), ja_name.clone());
            names.ja_to_id.insert(ja_name, id);
        } else if let Some(ja_name) = custom_zh_to_ja.get(cn_name.as_ref()) {
            let ja_name: Arc<str> = Arc::from(ja_name.as_str());
            names.id_to_ja.insert(id.clone(), ja_name.clone());
            names.ja_to_id.insert(ja_name, id);
        }
    }

    // 中国語情報での追加漏れを確認(JPにのみ存在するアイテム)
    for (id, jp_value) in jp_items.iter() {
        if names.id_to_zh.contains_key(id.as_str()) {
            continue;
        }
        if let Some(ja_name) = jp_value.get("name").and_then(Value::as_str) {
            let id: Arc<str> = Arc::from(id.as_str());
            let ja_name: Arc<str> = Arc::from(ja_name);
            names.id_to_ja.insert(id.clone(), ja_name.clone());
            names.ja_to_id.insert(ja_name, id);
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
        let id: Arc<str> = Arc::from(item.id.as_str());
        let zh: Arc<str> = Arc::from(item.zh.as_str());
        let ja: Arc<str> = Arc::from(item.ja.as_str());

        let has_zh = names.id_to_zh.contains_key(id.as_ref());
        let has_ja = names.id_to_ja.contains_key(id.as_ref());

        if has_zh && has_ja {
            continue;
        }
        if has_ja {
            // 中国語情報のみ欠損（zh_to_ja(zh)はzh_to_id→id_to_jaの連鎖で導出されるため、
            // 既存のid_to_ja[id]と対応させるにはzh_to_id/id_to_zhの登録だけで足りる）
            names.zh_to_id.insert(zh.clone(), id.clone());
            names.id_to_zh.insert(id, zh);
            continue;
        }
        if has_zh {
            // 日本語情報のみ欠損（同様にid_to_ja/ja_to_idの登録だけで足りる）
            names.id_to_ja.insert(id.clone(), ja.clone());
            names.ja_to_id.insert(ja, id);
            continue;
        }
        // 同じ情報(zh jaのセット)が既にある場合、更新によってcustomIDが古くなっているのでスルー
        // 違うIDならこの項目に問題があるのでスルー
        if names.zh_to_id.contains_key(zh.as_ref()) && names.ja_to_id.contains_key(ja.as_ref()) {
            continue;
        }
        if let Some(existing_id) = names.zh_to_id.get(zh.as_ref()).cloned() {
            // 中国語で対応するIDがある
            if names.id_to_ja.contains_key(existing_id.as_ref()) {
                continue; // 正しい日本語が既にある
            }
            names.id_to_ja.insert(existing_id.clone(), ja.clone());
            names.ja_to_id.insert(ja, existing_id);
            continue;
        }
        if let Some(existing_id) = names.ja_to_id.get(ja.as_ref()).cloned() {
            // 日本語で対応するIDがある
            if names.id_to_zh.contains_key(existing_id.as_ref()) {
                continue; // 正しい中国語が既にある
            }
            names.zh_to_id.insert(zh.clone(), existing_id.clone());
            names.id_to_zh.insert(existing_id, zh);
            continue;
        }

        // 完全オリジナルモノ
        names.id_to_ja.insert(id.clone(), ja.clone());
        names.zh_to_id.insert(zh.clone(), id.clone());
        names.id_to_zh.insert(id.clone(), zh);
        names.ja_to_id.insert(ja, id);
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
