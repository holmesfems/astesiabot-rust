use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Args {
    star: i64,
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

    async fn execute(&self, args: &Map<String, Value>, ctx: &AppState) -> ToolResponse {
        let parsed = match Args::validate_json(args) {
            Ok(a) => a,
            Err(e) => return ToolResponse::Error(e),
        };
        if parsed.star != 4 && parsed.star != 5 {
            return ToolResponse::Error(format!("starは4か5を指定してください: {}", parsed.star));
        }

        let combos = ctx.recruit.data.guaranteed_star_tags(parsed.star as u8, parsed.is_global);
        if combos.is_empty() {
            return ToolResponse::Ok(json!({
                "star": parsed.star,
                "combos": [],
                "note": format!("★{}の確定タグはありません", parsed.star),
            }));
        }

        let combos_json: Vec<Value> = combos
            .iter()
            .map(|c| json!({ "tags": c.tags, "operators": c.operators }))
            .collect();
        ToolResponse::Ok(json!({
            "star": parsed.star,
            "combos": combos_json,
        }))
    }
}
