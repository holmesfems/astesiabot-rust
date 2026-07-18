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

/// `engine::operator_cost_calc`のアイテム列(`Vec<(日本語名, 個数)>`)をAI向けJSON配列に変換する。
/// operatorEliteCost/operatorSkillInfo/operatorModuleCostの3関数が共通で使う。
pub fn items_to_json(items: &[(String, f64)]) -> Value {
    json!(items
        .iter()
        .map(|(name, count)| json!({ "name": name, "count": count }))
        .collect::<Vec<_>>())
}

/// GPTが誤って生成しがちなオペレーター名の誤字補正(Python版`toolCalling`内の
/// `operator_typo_correction_dict`相当)。オペレーター名を引数に取る4関数
/// (operatorEliteCost/operatorSkillInfo/operatorModuleCost/operatorFKInfo)の
/// autocomplete解決の後段で共通して使う。
pub fn operator_typo_correction(name: &str) -> String {
    match name {
        "メラニート" => "メラナイト".to_string(),
        other => other.to_string(),
    }
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

    // --- 要件4: 疑似入力でfunction callingの出力を検証する統合テスト ---
    // 実ネットワーク(outer_source起動時fetch/RiseiCalculatorEngine構築)に依存するため
    // `#[ignore]`。`cargo test -- --ignored`で明示実行する
    // (`bot/commands/operator_cost_calc/mod.rs`のgolden_testsと同じ方針・同じ構築手順)。
    use crate::bot::services::moderation::ModerationState;
    use crate::engine;

    async fn build_state() -> AppState {
        dotenvy::dotenv().ok();
        let recruit = engine::recruit::RecruitEngine::load().expect("recruit data should load");
        let moderation = ModerationState::from_env();
        let outer_source = engine::external_source::ExternalSourceRegistry::load().await;
        let risei_calculator = engine::risei_calculator_engine::RiseiCalculatorEngine::load(&outer_source)
            .await
            .expect("risei engine should build against real network data");
        let fk_data_search = engine::fk_data_search::FkDataSearchEngine::new();
        let uranai = crate::bot::services::uranai::UranaiState::from_env();
        AppState {
            recruit,
            moderation,
            external_source: outer_source,
            risei_calculator,
            fk_data_search,
            uranai,
        }
    }

    fn to_args(value: Value) -> Map<String, Value> {
        value.as_object().expect("test args must be a JSON object").clone()
    }

    async fn run(name: &str, arguments: Value, state: &AppState) -> ToolResponse {
        let tools = all_tools();
        let func = tools
            .into_iter()
            .find(|f| f.name() == name)
            .unwrap_or_else(|| panic!("tool {name} not registered"));
        func.execute(&to_args(arguments), state).await
    }

    fn unwrap_ok(resp: ToolResponse) -> Value {
        match resp {
            ToolResponse::Ok(v) => v,
            ToolResponse::Error(e) => panic!("expected Ok, got Error: {e}"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn risei_materials_returns_stages() {
        let state = build_state().await;
        let value = unwrap_ok(run("riseiMaterials", json!({ "target": "糖" }), &state).await);
        let stages = value.get("top_stages").and_then(Value::as_array).expect("top_stages array");
        assert!(!stages.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn risei_stages_returns_stages() {
        let state = build_state().await;
        let value = unwrap_ok(run("riseiStages", json!({ "target": "1-7" }), &state).await);
        let stages = value.get("stages").and_then(Value::as_array).expect("stages array");
        assert!(!stages.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn risei_lists_returns_contents() {
        let state = build_state().await;
        let value = unwrap_ok(run("riseiLists", json!({ "target": "理性価値表" }), &state).await);
        let contents = value.get("contents").and_then(Value::as_array).expect("contents array");
        assert!(!contents.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn operator_elite_cost_returns_phases() {
        let state = build_state().await;
        let value = unwrap_ok(run("operatorEliteCost", json!({ "target": "ケルシー・エスペランタ" }), &state).await);
        let phases = value.get("phases").and_then(Value::as_array).expect("phases array");
        assert!(!phases.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn operator_skill_info_returns_masteries() {
        let state = build_state().await;
        let value = unwrap_ok(run("operatorSkillInfo", json!({ "target": "アーミヤ(前衛)", "skillnum": 1 }), &state).await);
        let masteries = value.get("masteries").and_then(Value::as_array).expect("masteries array");
        assert!(!masteries.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn operator_module_cost_returns_modules() {
        let state = build_state().await;
        let value = unwrap_ok(run("operatorModuleCost", json!({ "target": "アステシア" }), &state).await);
        let modules = value.get("modules").and_then(Value::as_array).expect("modules array");
        assert!(!modules.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn operator_fk_info_resolves_single_skill_operator() {
        let state = build_state().await;
        let value = unwrap_ok(run("operatorFKInfo", json!({ "target": "アイリス", "skillnum": "" }), &state).await);
        assert_eq!(value.get("status").and_then(Value::as_str), Some("found"));
    }

    #[tokio::test]
    #[ignore]
    async fn get_recruitment_list_returns_combos_for_star5() {
        let state = build_state().await;
        let value = unwrap_ok(run("getRecruitmentList", json!({ "star": 5, "isGlobal": true }), &state).await);
        let combos = value.get("combos").and_then(Value::as_array).expect("combos array");
        assert!(!combos.is_empty());
    }
}
