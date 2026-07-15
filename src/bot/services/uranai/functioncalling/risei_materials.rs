use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::engine::risei_calculator_engine::server::load_stage_category_file;
use crate::engine::risei_calculator_engine::Server;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// GPT向けの表示件数上限(Python版`toolCalling`の`maxItems=5`相当。Discordコマンド版の
/// `MAX_ITEMS=15`よりトークン消費を抑えるため小さくしている)。
const MAX_ITEMS_FOR_AI: usize = 5;

#[derive(Deserialize)]
struct Args {
    target: String,
}

/// カテゴリのenum値(`data/risei/stage_category.json`の`main`+`new`の`to_ja`)。
/// 以前は誤選択対策で英語表記に固定していたが、GPT側の精度向上を踏まえて
/// 日本語の`to_ja`表記に切り替える(`main`のみ19件だった旧enumには大陸先行の
/// `new`2件が含まれていなかったため、こちらの方が実際のカテゴリ一覧と一致する)。
fn material_categories() -> Vec<String> {
    let file = load_stage_category_file().expect("data/risei/stage_category.json should parse");
    file.main.values().chain(file.new.values()).map(|c| c.to_ja.clone()).collect()
}

/// enumの`to_ja`表記からカテゴリキー(`main_x`/`new_x`)を解決する。完全一致優先、
/// 無ければ部分一致(Python版`estimateCategoryFromJPName`の`current in to_ja or to_ja in current`
/// 相当)でGPTの表記ゆれを吸収する。
fn resolve_category_key(ctx: &AppState, target: &str) -> Option<String> {
    let file = ctx.risei_calculator.stage_category();
    let entries: Vec<(&String, &str)> = file
        .main
        .iter()
        .chain(file.new.iter())
        .map(|(key, info)| (key, info.to_ja.as_str()))
        .collect();
    entries
        .iter()
        .find(|(_, to_ja)| *to_ja == target)
        .or_else(|| entries.iter().find(|(_, to_ja)| target.contains(*to_ja) || to_ja.contains(target)))
        .map(|(key, _)| (*key).clone())
}

impl ToolArgs for Args {
    fn schema_properties() -> Value {
        json!({
            "target": {
                "type": "string",
                "description": "Category of the material",
                "enum": material_categories(),
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

    async fn execute(&self, args: &Map<String, Value>, ctx: &AppState) -> ToolResponse {
        let parsed = match Args::validate_json(args) {
            Ok(a) => a,
            Err(e) => return ToolResponse::Error(e),
        };
        let Some(key) = resolve_category_key(ctx, &parsed.target) else {
            return ToolResponse::Error(format!("不明な素材カテゴリです: {}", parsed.target));
        };

        // Python版と同じくGPTからの呼び出しは常にグローバル版基準
        // (newカテゴリ指定時は material_search 内部で自動的に大陸版へ切り替わる)。
        match ctx.risei_calculator.material_search(&ctx.outer_source, Server::Global, &key).await {
            Err(msg) => ToolResponse::Error(msg),
            Ok(result) => {
                let stages: Vec<Value> = result
                    .stages
                    .iter()
                    .take(MAX_ITEMS_FOR_AI)
                    .map(|s| {
                        json!({
                            "stage_name": s.name,
                            "overall_efficiency": s.efficiency,
                            "sanity_cost": s.sanity_cost,
                            "time_cost_seconds_double_speed": s.time_cost,
                            "drop_per_minute": s.drop_per_minute,
                            "main_item_efficiency": s.main_item_efficiency,
                            "confidence_interval_3sigma": s.confidence_3sigma,
                            "promotion_material_efficiency": s.promotion_efficiency,
                            "sample_size": s.max_times,
                        })
                    })
                    .collect();
                ToolResponse::Ok(json!({
                    "category": result.category_ja,
                    "main_item_risei_value": result.main_item_value,
                    "main_item_risei_value_std_dev": result.main_item_std_dev,
                    "top_stages": stages,
                }))
            }
        }
    }
}
