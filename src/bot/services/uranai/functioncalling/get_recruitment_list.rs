use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    #[allow(dead_code)]
    star: i64,
    #[allow(dead_code)]
    is_global: bool,
}

impl ToolArgs for Args {
    fn schema_properties() -> Value {
        json!({
            "star": {
                "type": "number",
                "description": "The star to specify",
                "enum": [4, 5],
            },
            "isGlobal": {
                "type": "boolean",
                "description": "True for the Global Server, and false for the CN Server. Default is True.",
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["star", "isGlobal"]
    }
}

pub struct GetRecruitmentList;

#[async_trait]
impl ToolFunction for GetRecruitmentList {
    fn name(&self) -> &'static str {
        "getRecruitmentList"
    }

    fn description(&self) -> &'static str {
        "Get a tag combination that will ensure that only characters of the specified star appear"
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
