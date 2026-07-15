use super::{wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::bot::commands::risei::riseilists::RiseiListTarget;
use crate::engine::risei_calculator_engine::{Server, TicketEfficiency, CC_NUMBER};
use async_trait::async_trait;
use poise::ChoiceParameter;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct Args {
    target: String,
}

/// `/riseilists`の`RiseiListTarget`(poise::ChoiceParameter)の`#[name]`一覧をenum値として使う。
/// 以前は手書きの英語8件だったが、結晶交換所(Pinch Out)相当の2件は実装されておらず
/// `RiseiListTarget`には存在しないため実態と食い違っていた。コマンド側の選択肢と
/// 単一の情報源にすることでズレを防ぐ。
fn risei_list_targets() -> Vec<String> {
    RiseiListTarget::list().into_iter().map(|c| c.name).collect()
}

impl ToolArgs for Args {
    fn schema_properties() -> Value {
        json!({
            "target": {
                "type": "string",
                "description": "Name of the table",
                "enum": risei_list_targets(),
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["target"]
    }
}

fn ticket_list_json(list: Vec<TicketEfficiency>) -> Value {
    json!(list
        .iter()
        .map(|item| json!({ "name": item.name_ja, "efficiency": item.efficiency, "std_dev": item.std_dev }))
        .collect::<Vec<_>>())
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

    async fn execute(&self, args: &Map<String, Value>, ctx: &AppState) -> ToolResponse {
        let parsed = match Args::validate_json(args) {
            Ok(a) => a,
            Err(e) => return ToolResponse::Error(e),
        };
        let Some(target) = RiseiListTarget::from_name(&parsed.target) else {
            return ToolResponse::Error(format!("不明な表です: {}", parsed.target));
        };

        // Python版と同じくGPTからの呼び出しは常にグローバル版基準。
        let engine = &ctx.risei_calculator;
        let server = Server::Global;
        let body = match target {
            RiseiListTarget::BaseMaps => {
                let map = engine.base_maps(&ctx.outer_source, server).await;
                json!(map)
            }
            RiseiListTarget::SanValueList => {
                let values = engine.value_list(&ctx.outer_source, server).await;
                json!(values
                    .iter()
                    .map(|v| json!({ "name": v.name_ja, "value": v.value, "std_dev": v.std_dev }))
                    .collect::<Vec<_>>())
            }
            RiseiListTarget::Te2List => ticket_list_json(engine.te2_list(&ctx.outer_source, server).await),
            RiseiListTarget::Te3List => ticket_list_json(engine.te3_list(&ctx.outer_source, server).await),
            RiseiListTarget::SpecialList => ticket_list_json(engine.special_list(&ctx.outer_source, server).await),
            RiseiListTarget::CcList => ticket_list_json(engine.cc_list(&ctx.outer_source, server).await),
        };

        ToolResponse::Ok(json!({
            "table": parsed.target,
            "cc_number": if matches!(target, RiseiListTarget::CcList) { Some(CC_NUMBER) } else { None },
            "contents": body,
        }))
    }
}
