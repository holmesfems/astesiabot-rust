use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct Args {
    #[allow(dead_code)]
    target: String,
    #[allow(dead_code)]
    skillnum: i64,
}

impl ToolArgs for Args {
    fn schema_properties() -> Value {
        json!({
            "target": {
                "type": "string",
                "description": "Name of the operator. e.g. アーミヤ アステシア エイヤフィヤトラ",
            },
            "skillnum": {
                "type": "number",
                "description": "The skill number",
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["target", "skillnum"]
    }
}

pub struct OperatorSkillInfo;

#[async_trait]
impl ToolFunction for OperatorSkillInfo {
    fn name(&self) -> &'static str {
        "operatorSkillInfo"
    }

    fn description(&self) -> &'static str {
        "Get the material cost list to specialize one of the skill of an operator. Each operator has up to 3 skills."
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
