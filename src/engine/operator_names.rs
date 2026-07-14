use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

const CHAR_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/character_table.json";
const CHAR_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/character_table.json";

const FETCH_RETRIES: usize = 3;

/// オペレーターの中国語名→日本語名 変換。起動時に一度、中国語版/日本語版の
/// character_table.json を突き合わせて構築する（Python の
/// AllOperatorsInfo.cnNameToJaName 相当）。
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

    /// CN/JP の character_table.json を fetch して変換辞書を構築する。
    /// fetch に失敗した場合はエラーを出力し、空の辞書（中国語名そのまま表示）で継続する。
    pub async fn load() -> Self {
        let custom: BTreeMap<String, String> =
            match std::fs::read_to_string("data/customZhToJa.yaml") {
                Ok(s) => serde_yaml::from_str(&s).unwrap_or_default(),
                Err(_) => BTreeMap::new(),
            };

        let cn_to_ja = match Self::fetch_cn_to_ja(&custom).await {
            Ok(map) => map,
            Err(e) => {
                eprintln!("[operator_names] キャラテーブルの取得に失敗しました: {e}");
                custom.into_iter().collect()
            }
        };

        Self { cn_to_ja }
    }

    async fn fetch_cn_to_ja(
        custom: &BTreeMap<String, String>,
    ) -> Result<HashMap<String, String>, reqwest::Error> {
        let client = reqwest::Client::new();
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
}

/// character_table.json のキーが `char_xxx_yyy` 形式（オペレーター）かどうか
/// （Python の `re.match(r"([^_]+)_(\d+)_([^_]+)", key).group(1) == "char"` 相当）。
fn is_char_key(key: &str) -> bool {
    key.split('_').next() == Some("char")
}

async fn fetch_json_with_retry(client: &reqwest::Client, url: &str) -> Result<Value, reqwest::Error> {
    let mut last_err = None;
    for _ in 0..FETCH_RETRIES {
        match client.get(url).send().await {
            Ok(resp) => match resp.error_for_status() {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(json) => return Ok(json),
                    Err(e) => last_err = Some(e),
                },
                Err(e) => last_err = Some(e),
            },
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.expect("FETCH_RETRIES must be >= 1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ネットワークでキャラテーブルを取得できるかの疎通確認。
    /// `cargo test -- --ignored` で明示実行する（通常のCIでは走らせない）。
    #[tokio::test]
    #[ignore]
    async fn load_fetches_real_gamedata_and_translates_known_name() {
        let names = OperatorNames::load().await;
        // data/birthdayRev.yaml (1月2日) に登場するCN名。fetchが成功すれば
        // 日本語名に変換され、CN名のままではなくなるはず。
        let ja = names.to_ja("夜烟");
        assert_ne!(ja, "夜烟", "fetch/突き合わせに失敗している可能性: got {ja}");
        println!("夜烟 -> {ja}");
    }
}
