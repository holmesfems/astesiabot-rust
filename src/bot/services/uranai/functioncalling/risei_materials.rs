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
                "description": "Category of the material",
                "enum": [
                    "Orirock", "Device", "Polyester", "Sugar", "Ori-iron", "Aketon", "Kohl",
                    "Manganese", "Grindstone", "RMA", "Gel", "Incandescent Alloy", "Crystalline",
                    "Solvent", "Cutting Fluid", "Transmuted Salt", "Fiber", "Hydrocarbon",
                    "Condensation-like nuclei"
                ],
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["target"]
    }
}

pub struct RiseiMaterials;

#[async_trait]
impl ToolFunction for RiseiMaterials {
    fn name(&self) -> &'static str {
        "riseiMaterials"
    }

    fn description(&self) -> &'static str {
        "Get the information (e.g. sanity efficiency, sanity cost, time efficiency, time cost, etc.) of stages to farm a kind of material in Arknights"
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
