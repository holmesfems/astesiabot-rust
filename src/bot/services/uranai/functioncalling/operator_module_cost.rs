use super::{items_to_json, operator_typo_correction, wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::bot::commands::operator_cost_calc::build_context;
use crate::engine::operator_cost_calc::calc::operator_module_cost;
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

pub struct OperatorModuleCost;

#[async_trait]
impl ToolFunction for OperatorModuleCost {
    fn name(&self) -> &'static str {
        "operatorModuleCost"
    }

    fn description(&self) -> &'static str {
        "Get the material cost list to unlock or modify the module of an operator"
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
            .autocomplete_module_cost(&parsed.target, 1)
            .into_iter()
            .next()
            .unwrap_or(parsed.target);
        let resolved = operator_typo_correction(&resolved);

        match operator_module_cost(&info, &values, &resolved) {
            Err(msg) => ToolResponse::Error(msg),
            Ok(dto) => {
                let modules: Vec<Value> = dto
                    .modules
                    .iter()
                    .map(|m| {
                        let phases: Vec<Value> = m
                            .phases
                            .iter()
                            .map(|p| json!({ "stage": p.stage, "risei_value": p.risei_value, "items": items_to_json(&p.items) }))
                            .collect();
                        json!({
                            "module_name": m.header,
                            "phases": phases,
                            "total_risei_value": m.total_risei_value,
                            "total_items": items_to_json(&m.total_items),
                            "total_items_converted_to_medium_grade": items_to_json(&m.total_r2_items),
                        })
                    })
                    .collect();
                ToolResponse::Ok(json!({
                    "operator_name": dto.operator_name,
                    "modules": modules,
                }))
            }
        }
    }
}
