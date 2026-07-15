use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::engine::risei_calculator_engine::Server;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// GPT向けの表示件数上限(Python版`toolCalling`の`maxItems=5`相当)。
const MAX_ITEMS_FOR_AI: usize = 5;

#[derive(Deserialize)]
struct Args {
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

    async fn execute(&self, args: &Map<String, Value>, ctx: &AppState) -> ToolResponse {
        let parsed = match Args::validate_json(args) {
            Ok(a) => a,
            Err(e) => return ToolResponse::Error(e),
        };

        // Python版と同じく、常に大陸版のオートコンプリートで表記ゆれ(全角/半角等)を解決してから
        // グローバル版基準で検索する(該当が無ければstage_search内部で大陸版へフォールバックする)。
        let mainland_snapshot = ctx.risei_calculator.snapshot(Server::Mainland, &ctx.outer_source).await;
        let resolved_code = mainland_snapshot
            .auto_complete_main_stage(&parsed.target, 1)
            .into_iter()
            .next()
            .map(|(_, code)| code)
            .unwrap_or(parsed.target);

        match ctx.risei_calculator.stage_search(&ctx.outer_source, Server::Global, &resolved_code).await {
            Err(msg) => ToolResponse::Error(msg),
            Ok(result) => {
                let stages: Vec<Value> = result
                    .stages
                    .iter()
                    .take(MAX_ITEMS_FOR_AI)
                    .map(|s| {
                        let categories: Vec<Value> = s
                            .categories
                            .iter()
                            .map(|c| {
                                json!({
                                    "category": c.category_ja,
                                    "efficiency": c.efficiency,
                                    "drop_per_minute": c.drop_per_minute,
                                })
                            })
                            .collect();
                        json!({
                            "stage_name": s.name,
                            "total_efficiency": s.total_efficiency,
                            "confidence_interval_3sigma": s.confidence_3sigma,
                            "categories": categories,
                            "sanity_cost": s.sanity_cost,
                            "time_cost_seconds_double_speed": s.time_cost,
                            "promotion_material_efficiency": s.promotion_efficiency,
                            "sample_size": s.max_times,
                        })
                    })
                    .collect();
                ToolResponse::Ok(json!({
                    "fell_back_to_mainland": result.effective_server == Server::Mainland,
                    "stages": stages,
                }))
            }
        }
    }
}
