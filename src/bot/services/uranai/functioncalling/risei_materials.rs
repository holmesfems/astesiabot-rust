use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::engine::risei_calculator_engine::server::load_stage_category_file;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct Args {
    #[allow(dead_code)]
    target: String,
}

/// カテゴリのenum値(`data/risei/stage_category.json`の`main`+`new`の`to_ja`)。
/// 以前は誤選択対策で英語表記に固定していたが、GPT側の精度向上を踏まえて
/// 日本語の`to_ja`表記に切り替える（`main`のみ19件だった旧enumには大陸先行の
/// `new`2件が含まれていなかったため、こちらの方が実際のカテゴリ一覧と一致する）。
fn material_categories() -> Vec<String> {
    let file = load_stage_category_file().expect("data/risei/stage_category.json should parse");
    file.main.values().chain(file.new.values()).map(|c| c.to_ja.clone()).collect()
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

    async fn execute(&self, args: &Map<String, Value>, _ctx: &AppState) -> ToolResponse {
        if let Err(e) = Args::validate_json(args) {
            return ToolResponse::Error(e);
        }
        ToolResponse::Ok(json!({ "message": format!("この機能はまだ実装されていません: {}", self.name()) }))
    }
}
