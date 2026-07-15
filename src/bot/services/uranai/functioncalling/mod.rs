//! uranai(占い館)のfunction calling本実装。各関数は1ファイル1関数で
//! `functioncalling/`直下に置き、共通のトレイト`ToolFunction`を実装する。
//!
//! - `ToolFunction`: 関数の名前・description・JSON Schema(parameters)・実行ロジックを強制する
//!   （object-safeにするため`async-trait`でasync実行ロジックを持たせる）。
//! - `ToolArgs`: 各関数の入力形式を定義する。`validate_json`はuranaiがJSON文字列から
//!   パースした`Map`を受け取り、serde経由で自身の型に変換する（エラーならメッセージを返す）。
//!   小規模プロジェクトのため、フィールドごとの手書き検証はせずserde Deserializeに委譲する。
//! - `ToolResponse`: 実行ロジックが返す型。呼び出し元(`uranai::tools::dispatch`)がこれを
//!   JSONにエンコードしてGPTへの`function_call_output`として送信する。
//!
//! `data/uranai/toolList.yaml`は`build_tool_list_entries()`が組み立てる内容と一致している
//! 必要がある（`tests::tool_list_yaml_matches_registered_functions`で検証）。実際のファイルの
//! 再生成は`src/bin/regen_uranai_tools.rs`（main.rs非依存）で行う。

mod get_recruitment_list;
mod operator_elite_cost;
mod operator_fk_info;
mod operator_module_cost;
mod operator_skill_info;
mod risei_lists;
mod risei_materials;
mod risei_stages;

use crate::api::AppState;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};

/// 各関数の実行ロジックが返す型（要件2の「Responseでラベル付けしたクラス」）。
/// 呼び出し元がJSON文字列にエンコードしてGPTへ送信する。
pub enum ToolResponse {
    Ok(Value),
    Error(String),
}

impl ToolResponse {
    pub fn to_json_string(&self) -> String {
        match self {
            ToolResponse::Ok(v) => v.to_string(),
            ToolResponse::Error(msg) => json!({ "error": msg }).to_string(),
        }
    }
}

/// 各関数の入力形式を定義する。`schema_properties`/`required_fields`は
/// `data/uranai/toolList.yaml`の`parameters.properties`/`parameters.required`に対応する。
pub trait ToolArgs: DeserializeOwned {
    fn schema_properties() -> Value;
    fn required_fields() -> &'static [&'static str];

    /// uranaiがJSON文字列からパースした`Map`を受け取り、内容を自身の型に反映する。
    /// 型不一致や必須フィールド欠如はErrに文字列で反映される（呼び出し元がエラーレスポンスにする）。
    fn validate_json(map: &Map<String, Value>) -> Result<Self, String> {
        serde_json::from_value(Value::Object(map.clone())).map_err(|e| e.to_string())
    }
}

/// `parameters`オブジェクト全体（type/properties/required/additionalProperties）を組み立てる。
/// `additionalProperties: false`を全関数共通で強制するため、ここに一本化する。
pub fn wrap_parameters(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// 各functionが実装する共通トレイト（要件2）。`Vec<Box<dyn ToolFunction>>`として
/// レジストリに積めるようobject-safeにしている。
#[async_trait]
pub trait ToolFunction: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// `data/uranai/toolList.yaml`の`parameters`に対応するJSON Schema。
    fn parameters_schema(&self) -> Value;
    /// 引数は`Map`のまま受け取り(要件4)、実装側の冒頭で自分のArgs型のvalidate_jsonを呼んで解釈する。
    async fn execute(&self, args: &Map<String, Value>, ctx: &AppState) -> ToolResponse;
}

/// 登録済み関数一覧。dispatch側の名前引き・toolList.yaml生成の両方がここを走査する。
pub fn all_tools() -> Vec<Box<dyn ToolFunction>> {
    vec![
        Box::new(risei_materials::RiseiMaterials),
        Box::new(risei_stages::RiseiStages),
        Box::new(risei_lists::RiseiLists),
        Box::new(operator_elite_cost::OperatorEliteCost),
        Box::new(operator_skill_info::OperatorSkillInfo),
        Box::new(operator_module_cost::OperatorModuleCost),
        Box::new(operator_fk_info::OperatorFkInfo),
        Box::new(get_recruitment_list::GetRecruitmentList),
    ]
}

/// `data/uranai/toolList.yaml`の内容を`all_tools()`から組み立てる。
/// 組み込みツール(`web_search`/`image_generation`)は登録関数ではないため固定エントリとして
/// 末尾に追記する。`regen_uranai_tools`（生成bin）とテスト
/// (`tool_list_yaml_matches_registered_functions`)の両方がこれを使うことで、生成ロジックを
/// 1箇所に保つ。
pub fn build_tool_list_entries() -> Vec<Value> {
    let mut entries: Vec<Value> = all_tools()
        .iter()
        .map(|f| {
            json!({
                "type": "function",
                "name": f.name(),
                "description": f.description(),
                "parameters": f.parameters_schema(),
                "strict": true,
            })
        })
        .collect();
    entries.push(json!({ "type": "web_search" }));
    entries.push(json!({ "type": "image_generation" }));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_yaml_matches_registered_functions() {
        let file_tools = super::super::tools::load_tool_definitions()
            .expect("data/uranai/toolList.yaml should parse");
        let expected = build_tool_list_entries();
        assert_eq!(
            file_tools, expected,
            "data/uranai/toolList.yaml is stale — run `cargo run --bin regen_uranai_tools` and commit the diff"
        );
    }
}
