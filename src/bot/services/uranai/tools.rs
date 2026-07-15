use super::functioncalling::{self, ToolResponse};
use crate::api::AppState;
use crate::bot::data::Error;
use serde_json::Value;

const TOOL_LIST_PATH: &str = "data/uranai/toolList.yaml";

/// `functioncalling`に登録された8つのcustom functionのJSON Schema定義を
/// `data/uranai/toolList.yaml` から読み込む（起動時に一度だけ呼ぶ想定）。
/// このファイルは`functioncalling::build_tool_list_entries()`の内容と一致している必要があり、
/// ズレは`functioncalling::tests::tool_list_yaml_matches_registered_functions`で検出される
/// （再生成は`cargo run --bin regen_uranai_tools`）。
/// 組み込みツールのうち`web_search`のみ固定エントリとして含む（要件定義で確定済み）。
pub fn load_tool_definitions() -> Result<Vec<Value>, Error> {
    let content = std::fs::read_to_string(TOOL_LIST_PATH)?;
    Ok(serde_yaml::from_str(&content)?)
}

/// `functioncalling::all_tools()`から名前引きして実行する。未登録の名前や不正なJSON引数は
/// エラーレスポンスとして返す（GPT側には通常のtool結果として渡るので、会話自体は継続できる）。
pub async fn dispatch(name: &str, arguments_json: &str, state: &AppState) -> String {
    let tools = functioncalling::all_tools();
    let Some(func) = tools.into_iter().find(|f| f.name() == name) else {
        return ToolResponse::Error(format!("unknown function: {name}")).to_json_string();
    };
    let map = match serde_json::from_str::<Value>(arguments_json) {
        Ok(Value::Object(map)) => map,
        _ => return ToolResponse::Error("invalid arguments JSON".to_string()).to_json_string(),
    };
    func.execute(&map, state).await.to_json_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_all_eight_tools_plus_web_search_from_yaml() {
        let tools = load_tool_definitions().expect("data/uranai/toolList.yaml should parse");
        let functions: Vec<&Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(Value::as_str) == Some("function"))
            .collect();
        let names: Vec<&str> = functions
            .iter()
            .map(|t| t.get("name").and_then(Value::as_str).expect("name field"))
            .collect();
        assert_eq!(
            names,
            vec![
                "riseiMaterials",
                "riseiStages",
                "riseiLists",
                "operatorEliteCost",
                "operatorSkillInfo",
                "operatorModuleCost",
                "operatorFKInfo",
                "getRecruitmentList",
            ]
        );
        for tool in &functions {
            assert_eq!(
                tool.get("parameters")
                    .and_then(|p| p.get("additionalProperties"))
                    .and_then(Value::as_bool),
                Some(false)
            );
            assert_eq!(tool.get("strict").and_then(Value::as_bool), Some(true));
        }
        let fixed_types: Vec<&str> = tools
            .iter()
            .filter(|t| t.get("type").and_then(Value::as_str) != Some("function"))
            .filter_map(|t| t.get("type").and_then(Value::as_str))
            .collect();
        assert_eq!(fixed_types, vec!["web_search", "image_generation"]);
    }
}
