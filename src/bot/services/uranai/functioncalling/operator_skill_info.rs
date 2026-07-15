use super::{items_to_json, operator_typo_correction, wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::bot::commands::operator_cost_calc::build_context;
use crate::engine::operator_cost_calc::calc::skill_master_cost;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct Args {
    target: String,
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

    async fn execute(&self, args: &Map<String, Value>, ctx: &AppState) -> ToolResponse {
        let parsed = match Args::validate_json(args) {
            Ok(a) => a,
            Err(e) => return ToolResponse::Error(e),
        };
        if parsed.skillnum < 1 {
            return ToolResponse::Error(format!("スキル番号は1以上を指定してください: {}", parsed.skillnum));
        }

        let (info, values) = build_context(ctx).await;
        let resolved = info
            .autocomplete_master_cost(&parsed.target, 1)
            .into_iter()
            .next()
            .unwrap_or(parsed.target);
        let resolved = operator_typo_correction(&resolved);

        match skill_master_cost(&info, &values, &resolved, parsed.skillnum as u32) {
            Err(msg) => ToolResponse::Error(msg),
            Ok(dto) => {
                let masteries: Vec<Value> = dto
                    .masteries
                    .iter()
                    .enumerate()
                    .map(|(i, m)| json!({ "mastery_level": i + 1, "risei_value": m.risei_value, "items": items_to_json(&m.items) }))
                    .collect();
                ToolResponse::Ok(json!({
                    "skill_name": dto.skill_name,
                    "skill_num": dto.skill_num,
                    "skill_description": dto.description,
                    "masteries": masteries,
                    "total_risei_value": dto.total.risei_value,
                    "total_items": items_to_json(&dto.total.items),
                    "total_items_converted_to_medium_grade": items_to_json(&dto.total_r2_items),
                    "ranking_note": dto.ranking_text,
                }))
            }
        }
    }
}
