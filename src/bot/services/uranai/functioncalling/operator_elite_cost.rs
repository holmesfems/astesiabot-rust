use super::{items_to_json, operator_typo_correction, wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::bot::commands::operator_cost_calc::build_context;
use crate::engine::operator_cost_calc::calc::operator_elite_cost;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct Args {
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

    async fn execute(&self, args: &Map<String, Value>, ctx: &AppState) -> ToolResponse {
        let parsed = match Args::validate_json(args) {
            Ok(a) => a,
            Err(e) => return ToolResponse::Error(e),
        };

        let (info, values) = build_context(ctx).await;
        let resolved = info
            .autocomplete_elite_cost(&parsed.target, 1)
            .into_iter()
            .next()
            .unwrap_or(parsed.target);
        let resolved = operator_typo_correction(&resolved);

        match operator_elite_cost(&info, &values, &resolved) {
            Err(msg) => ToolResponse::Error(msg),
            Ok(dto) => {
                let phases: Vec<Value> = dto
                    .phases
                    .iter()
                    .enumerate()
                    .map(|(i, p)| json!({ "phase": i + 1, "risei_value": p.risei_value, "items": items_to_json(&p.items) }))
                    .collect();
                ToolResponse::Ok(json!({
                    "operator_name": dto.operator_name,
                    "phases": phases,
                    "total_risei_value": dto.total.risei_value,
                    "total_items": items_to_json(&dto.total.items),
                    "total_items_converted_to_medium_grade": items_to_json(&dto.total_r2_items),
                    "ranking_note": dto.ranking_text,
                }))
            }
        }
    }
}
