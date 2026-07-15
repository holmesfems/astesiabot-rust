use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct Args {
    #[allow(dead_code)]
    target: String,
}

impl ToolArgs for Args {
    fn schema_properties() -> Value {
        json!({
            "target": {
                "type": "string",
                "description": "Name of the operator. e.g. アーミヤ アステシア エイヤフィヤトラ",
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["target"]
    }
}

pub struct OperatorEliteCost;

#[async_trait]
impl ToolFunction for OperatorEliteCost {
    fn name(&self) -> &'static str {
        "operatorEliteCost"
    }

    fn description(&self) -> &'static str {
        "Get the material cost list to promote an operator"
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
