use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Google Sheets上のFK情報シート名（Python `SSNAME`）。reqwest/url が非ASCII文字を
/// URLパスとして自動で%エンコードするため、手動エンコードは不要。
const SHEET_NAME: &str = "スキル一覧";

/// Seedの保存先。`cargo run --bin regen_seeds` で手動生成し、git commitして
/// リポジトリに含めておく（起動時fetchが失敗した場合のフォールバック用）。
pub const SEED_PATH: &str = "data/seed/fk_data.json";

/// FKスプレッドシート1行分の生データ（Python `SkillFKInfo`からスキル名解決を除いた部分）。
/// スキル名解決（`skillNum`をオペレーターの`skillIds`と突き合わせて`SkillData`から引く）は
/// このデータ層の責務ではなく`engine::fk_data_search`が行う。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FkSheetRow {
    /// シート上の生値（"1"〜"3"の他、"素質1"のような数値以外の指定もありうる）。
    pub skill_num: String,
    pub fk_num: String,
    pub fk_err: String,
    pub detail: String,
    pub last_edited: String,
    pub state: String,
}

/// FKスプレッドシート全体（Python `FKInfo.fkData`相当）。
#[derive(Serialize, Deserialize, Default)]
pub struct FkSheetData {
    /// オペレーター名(シート列E) -> そのオペレーターのFK行一覧。
    /// `IndexMap`はシート行の挿入順(=Python dictの挿入順)を保持するため。
    pub by_operator: IndexMap<String, Vec<FkSheetRow>>,
}

pub fn fetch() -> BoxFuture<'static, Result<FkSheetData, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<FkSheetData, FetchError> {
    let api_key = std::env::var("FK_SHEETS_API_KEY").map_err(|_| "FK_SHEETS_API_KEY not set")?;
    let spreadsheet_id = std::env::var("FK_SHEETS_SPREADSHEET_ID").map_err(|_| "FK_SHEETS_SPREADSHEET_ID not set")?;
    let url = format!("https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values/{SHEET_NAME}?key={api_key}");

    let client = client();
    let json = fetch_json_with_retry(&client, &url).await?;

    let mut by_operator: IndexMap<String, Vec<FkSheetRow>> = IndexMap::new();
    let Some(rows) = json.get("values").and_then(Value::as_array) else {
        return Ok(FkSheetData { by_operator });
    };

    // 先頭2行はヘッダのため除外(Python `fkList = fkJson["values"][2:]`)。
    for row in rows.iter().skip(2) {
        let Some(cells) = row.as_array() else { continue };
        // 列L(index11)までアクセスするため、それ未満の行は対象外(Python `len(item)<12`)。
        if cells.len() < 12 {
            continue;
        }
        let cell = |i: usize| cells[i].as_str().unwrap_or("").to_string();
        let operator_name = cell(4);
        let sheet_row = FkSheetRow {
            skill_num: cell(6),
            fk_num: cell(7),
            fk_err: cell(8),
            detail: cell(9),
            last_edited: cell(10),
            state: cell(11),
        };
        // 最終更新が空のものは未整備データとして除外(Python `SkillFKInfo.isAvailable`)。
        if sheet_row.last_edited.is_empty() {
            continue;
        }
        by_operator.entry(operator_name).or_default().push(sheet_row);
    }

    Ok(FkSheetData { by_operator })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ネットワークでFKスプレッドシートを取得できるかの疎通確認。
    /// `cargo test -- --ignored` で明示実行する（通常のCIでは走らせない）。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_sheet_data() {
        dotenvy::dotenv().ok();
        let data = fetch().await.expect("fetch should succeed against real network");
        assert!(!data.by_operator.is_empty());
    }
}
