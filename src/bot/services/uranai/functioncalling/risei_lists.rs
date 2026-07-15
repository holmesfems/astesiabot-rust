use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::bot::commands::risei::riseilists::RiseiListTarget;
use async_trait::async_trait;
use poise::ChoiceParameter;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct Args {
    #[allow(dead_code)]
    target: String,
}

/// `/riseilists`の`RiseiListTarget`（poise::ChoiceParameter）の`#[name]`一覧をenum値として使う。
/// 以前は手書きの英語8件だったが、結晶交換所(Pinch Out)相当の2件は実装されておらず
/// `RiseiListTarget`には存在しないため実態と食い違っていた。コマンド側の選択肢と
/// 単一の情報源にすることでズレを防ぐ。
fn risei_list_targets() -> Vec<String> {
    RiseiListTarget::list().into_iter().map(|c| c.name).collect()
}

impl ToolArgs for Args {
    fn schema_properties() -> Value {
        json!({
            "target": {
                "type": "string",
                "description": "Name of the table",
                "enum": risei_list_targets(),
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["target"]
    }
}

pub struct RiseiLists;

#[async_trait]
impl ToolFunction for RiseiLists {
    fn name(&self) -> &'static str {
        "riseiLists"
    }

    fn description(&self) -> &'static str {
        "Get the contents of a table about sanity efficiency"
    }

    fn parameters_schema(&self) -> Value {
        wrap_parameters(Args::schema_properties(), Args::required_fields())
    }

    async fn execute(&self, args: &Map<String, Value>, _ctx: &AppState) -> ToolResponse {
        if let Err(e) = Args::validate_json(args) {
            return ToolResponse::Error(e);
        }
        ToolResponse::Ok(json!({ "message": format!("この機能はまだ実装されていません: {}", self.name()) }))
    }
}
