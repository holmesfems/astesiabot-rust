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
                "description": "The code name of the stage. e.g. 1-7 8-3 GA-8 JT8-2",
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["target"]
    }
}

pub struct RiseiStages;

#[async_trait]
impl ToolFunction for RiseiStages {
    fn name(&self) -> &'static str {
        "riseiStages"
    }

    fn description(&self) -> &'static str {
        "Get the information (e.g. efficiency, sanity cost, time cost, main drop, etc.) of constant stages in Arknights"
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
