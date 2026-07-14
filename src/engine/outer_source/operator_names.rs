use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

const CHAR_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/character_table.json";
const CHAR_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/character_table.json";

/// Seedの保存先。`cargo run --bin regen_seeds` で手動生成し、git commitして
/// リポジトリに含めておく（起動時fetchが失敗した場合のフォールバック用）。
pub const SEED_PATH: &str = "data/seed/operator_names.json";

/// オペレーターの中国語名→日本語名 変換。[`super::OuterSourceRegistry`] が
/// 起動時と定期fetchでメモリに保持し、機能側（birthday.rs など）はそこから
/// 参照する（Python の AllOperatorsInfo.cnNameToJaName 相当）。
#[derive(Serialize, Deserialize)]
pub struct OperatorNames {
    cn_to_ja: HashMap<String, String>,
}

impl OperatorNames {
    /// 中国語名を日本語名に変換する。対応が無ければ中国語名をそのまま返す
    /// （Python の `__cnToJa.get(cnName, cnName)` 相当）。
    pub fn to_ja<'a>(&'a self, cn_name: &'a str) -> &'a str {
        self.cn_to_ja.get(cn_name).map(String::as_str).unwrap_or(cn_name)
    }

    /// テスト用: 変換辞書が空の状態（＝常にCN名をそのまま返す）を作る。
    #[cfg(test)]
    pub fn empty_for_test() -> Self {
        Self {
            cn_to_ja: HashMap::new(),
        }
    }
}

/// [`super::Source`] に登録するfetch関数。CN/JP の character_table.json を
/// fetchして変換辞書を構築する。
pub fn fetch() -> BoxFuture<'static, Result<OperatorNames, FetchError>> {
    Box::pin(fetch_impl())
}

/// Seedの手動更新ツール（`cargo run --bin regen_seeds`）に登録するジョブ。
/// fetchし直して [`SEED_PATH`] に書き出す。
pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<OperatorNames, FetchError> {
    let custom: BTreeMap<String, String> = match std::fs::read_to_string("data/customZhToJa.yaml") {
        Ok(s) => serde_yaml::from_str(&s).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    };

    let cn_to_ja = fetch_cn_to_ja(&custom).await?;
    Ok(OperatorNames { cn_to_ja })
}

async fn fetch_cn_to_ja(
    custom: &BTreeMap<String, String>,
) -> Result<HashMap<String, String>, FetchError> {
    let client = client();
    let (cn_table, jp_table) = tokio::try_join!(
        fetch_json_with_retry(&client, CHAR_TABLE_URL_CN),
        fetch_json_with_retry(&client, CHAR_TABLE_URL_JP),
    )?;

    let mut cn_to_ja = HashMap::new();
    if let Value::Object(cn_map) = cn_table {
        for (key, cn_value) in cn_map {
            if !is_char_key(&key) {
                continue;
            }
            if cn_value
                .get("isNotObtainable")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let Some(cn_name) = cn_value.get("name").and_then(Value::as_str) else {
                continue;
            };
            let ja_name = jp_table
                .get(key.as_str())
                .and_then(|jp_value| jp_value.get("name"))
                .and_then(Value::as_str)
                .or_else(|| custom.get(cn_name).map(String::as_str));
            if let Some(ja_name) = ja_name {
                cn_to_ja.insert(cn_name.to_string(), ja_name.to_string());
            }
        }
    }
    Ok(cn_to_ja)
}

/// character_table.json のキーが `char_xxx_yyy` 形式（オペレーター）かどうか
/// （Python の `re.match(r"([^_]+)_(\d+)_([^_]+)", key).group(1) == "char"` 相当）。
fn is_char_key(key: &str) -> bool {
    key.split('_').next() == Some("char")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ネットワークでキャラテーブルを取得できるかの疎通確認。
    /// `cargo test -- --ignored` で明示実行する（通常のCIでは走らせない）。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_gamedata_and_translates_known_name() {
        let names = fetch().await.expect("fetch should succeed against real network");
        // data/birthdayRev.yaml (1月2日) に登場するCN名。fetchが成功すれば
        // 日本語名に変換され、CN名のままではなくなるはず。
        let ja = names.to_ja("夜烟");
        assert_ne!(ja, "夜烟", "fetch/突き合わせに失敗している可能性: got {ja}");
        println!("夜烟 -> {ja}");
    }
}
