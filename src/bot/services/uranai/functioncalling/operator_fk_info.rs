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
    skillnum: String,
}

impl ToolArgs for Args {
    fn schema_properties() -> Value {
        json!({
            "target": {
                "type": "string",
                "description": "Name of the operator. e.g. アーミヤ アステシア エイヤフィヤトラ",
            },
            "skillnum": {
                "type": "string",
                "description": "The id to specify the skill. It can be omitted if the skill is not specified",
                "enum": ["1", "2", "3", "素質1", "素質2", ""],
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["target", "skillnum"]
    }
}

pub struct OperatorFkInfo;

#[async_trait]
impl ToolFunction for OperatorFkInfo {
    fn name(&self) -> &'static str {
        "operatorFKInfo"
    }

    fn description(&self) -> &'static str {
        "Get the Frame-Kill(FK) info of a skill of the operator."
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
