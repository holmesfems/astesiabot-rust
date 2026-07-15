use serde_json::{json, Value};

/// Python版 `openaichat/toolList.yaml` の7つのcustom functionを移植したJSON Schema定義。
/// 組み込みツール（code_interpreter/image_generation/web_search）は含めない（要件定義で確定済み）。
///
/// Python版の`operatorModuleCost`だけ`additionalProperties`/`strict`が抜けていたが、
/// これはPython側の記述漏れとみなし、他の6つと揃えて normalize してある。
pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "name": "riseiMaterials",
            "description": "Get the information (e.g. sanity efficiency, sanity cost, time efficiency, time cost, etc.) of stages to farm a kind of material in Arknights",
            "parameters": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Category of the material",
                        "enum": [
                            "Orirock", "Device", "Polyester", "Sugar", "Ori-iron", "Aketon", "Kohl",
                            "Manganese", "Grindstone", "RMA", "Gel", "Incandescent Alloy", "Crystalline",
                            "Solvent", "Cutting Fluid", "Transmuted Salt", "Fiber", "Hydrocarbon",
                            "Condensation-like nuclei"
                        ]
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            },
            "strict": true
        }),
        json!({
            "type": "function",
            "name": "riseiStages",
            "description": "Get the information (e.g. efficiency, sanity cost, time cost, main drop, etc.) of constant stages in Arknights",
            "parameters": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "The code name of the stage. e.g. 1-7 8-3 GA-8 JT8-2"
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            },
            "strict": true
        }),
        json!({
            "type": "function",
            "name": "riseiLists",
            "description": "Get the contents of a table about sanity efficiency",
            "parameters": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Name of the table",
                        "enum": [
                            "Base stage table", "Sanity-Value table",
                            "Commendation Certificate Efficiency table", "Distinction Certificate Efficiency table",
                            "Special Exchange Order Efficiency table", "Contract Bounty Efficiency table",
                            "Crystal Exchange Efficiency table", "Pinch-out Exchange Efficiency table"
                        ]
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            },
            "strict": true
        }),
        json!({
            "type": "function",
            "name": "operatorEliteCost",
            "description": "Get the material cost list to promote an operator",
            "parameters": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Name of the operator. e.g. アーミヤ アステシア エイヤフィヤトラ"
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            },
            "strict": true
        }),
        json!({
            "type": "function",
            "name": "operatorSkillInfo",
            "description": "Get the material cost list to specialize one of the skill of an operator. Each operator has up to 3 skills.",
            "parameters": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Name of the operator. e.g. アーミヤ アステシア エイヤフィヤトラ"
                    },
                    "skillnum": {
                        "type": "number",
                        "description": "The skill number"
                    }
                },
                "required": ["target", "skillnum"],
                "additionalProperties": false
            },
            "strict": true
        }),
        json!({
            "type": "function",
            "name": "operatorModuleCost",
            "description": "Get the material cost list to unlock or modify the module of an operator",
            "parameters": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Name of the operator. e.g. アーミヤ アステシア エイヤフィヤトラ"
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            },
            "strict": true
        }),
        json!({
            "type": "function",
            "name": "operatorFKInfo",
            "description": "Get the Frame-Kill(FK) info of a skill of the operator.",
            "parameters": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Name of the operator. e.g. アーミヤ アステシア エイヤフィヤトラ"
                    },
                    "skillnum": {
                        "type": "string",
                        "description": "The id to specify the skill. It can be omitted if the skill is not specified",
                        "enum": ["1", "2", "3", "素質1", "素質2", ""]
                    }
                },
                "required": ["target", "skillnum"],
                "additionalProperties": false
            },
            "strict": true
        }),
        json!({
            "type": "function",
            "name": "getRecruitmentList",
            "description": "Get a tag combination that will ensure that only characters of the specified star appear",
            "parameters": {
                "type": "object",
                "properties": {
                    "star": {
                        "type": "number",
                        "description": "The star to specify",
                        "enum": [4, 5]
                    },
                    "isGlobal": {
                        "type": "boolean",
                        "description": "True for the Global Server, and false for the CN Server. Default is True."
                    }
                },
                "required": ["star", "isGlobal"],
                "additionalProperties": false
            },
            "strict": true
        }),
    ]
}

/// 各functionの内部ロジックは未実装。呼ばれたら固定の仮文字列を返す
/// （呼び出し元のLLMには通常のtool結果として渡るので、会話自体は継続できる）。
pub fn dispatch(name: &str, arguments_json: &str) -> String {
    println!("[uranai] tool called: {name}({arguments_json})");
    format!("この機能はまだ実装されていません: {name}")
}
