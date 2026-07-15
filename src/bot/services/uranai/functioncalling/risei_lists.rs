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
                "description": "Name of the table",
                "enum": [
                    "Base stage table",
                    "Sanity-Value table",
                    "Commendation Certificate Efficiency table",
                    "Distinction Certificate Efficiency table",
                    "Special Exchange Order Efficiency table",
                    "Contract Bounty Efficiency table",
                    "Crystal Exchange Efficiency table",
                    "Pinch-out Exchange Efficiency table"
                ],
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
