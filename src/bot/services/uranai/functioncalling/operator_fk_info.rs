use super::{operator_typo_correction, wrap_parameters, ToolArgs, ToolFunction, ToolResponse};
use crate::api::AppState;
use crate::bot::commands::fksearch::build_view;
use crate::engine::fk_data_search::FkSearchResult;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct Args {
    target: String,
    skillnum: String,
}

impl ToolArgs for Args {
    fn schema_properties() -> Value {
        json!({
            "target": {
                "type": "string",
                "description": "Name of the operator. e.g. アーミヤ アステシア エイヤフィヤトラ",
            },
            "skillnum": {
                "type": "string",
                "description": "The id to specify the skill. It can be omitted if the skill is not specified",
                "enum": ["1", "2", "3", "素質1", "素質2", ""],
            }
        })
    }

    fn required_fields() -> &'static [&'static str] {
        &["target", "skillnum"]
    }
}

pub struct OperatorFkInfo;

#[async_trait]
impl ToolFunction for OperatorFkInfo {
    fn name(&self) -> &'static str {
        "operatorFKInfo"
    }

    fn description(&self) -> &'static str {
        "Get the Frame-Kill(FK) info of a skill of the operator."
    }

    fn parameters_schema(&self) -> Value {
        wrap_parameters(Args::schema_properties(), Args::required_fields())
    }

    async fn execute(&self, args: &Map<String, Value>, ctx: &AppState) -> ToolResponse {
        let parsed = match Args::validate_json(args) {
            Ok(a) => a,
            Err(e) => return ToolResponse::Error(e),
        };

        let view = build_view(ctx).await;
        let resolved = view.autocomplete(&parsed.target, 1).into_iter().next().unwrap_or(parsed.target);
        let resolved = operator_typo_correction(&resolved);

        match view.search(&resolved, &parsed.skillnum) {
            FkSearchResult::OperatorNotFound => ToolResponse::Error(format!("オペレーター【{resolved}】のFK情報は見つかりませんでした")),
            FkSearchResult::NeedsSkillSelection { choices } => ToolResponse::Ok(json!({
                "status": "needs_skill_selection",
                "operator_name": resolved,
                "choices": choices.iter().map(|(num, name)| json!({ "skill_num": num, "skill_name": name })).collect::<Vec<_>>(),
            })),
            FkSearchResult::SkillNotFound { candidates } => ToolResponse::Ok(json!({
                "status": "skill_not_found",
                "operator_name": resolved,
                "candidates": candidates.iter().map(|c| json!({ "skill_num": c.skill_num, "skill_name": c.skill_name })).collect::<Vec<_>>(),
            })),
            FkSearchResult::Found(skill) => ToolResponse::Ok(json!({
                "status": "found",
                "operator_name": resolved,
                "skill_name": skill.skill_name,
                "requested_skill_num": skill.requested_skill_num,
                "fastest_fk_count": skill.fk_num,
                "fk_margin_of_error": skill.fk_err,
                "detail": skill.detail,
            })),
        }
    }
}
