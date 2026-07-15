use crate::api::AppState;
use crate::bot::data::Error;
use serde_json::{json, Value};
use std::time::Duration;

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
/// tool-callループの最大往復数。仮実装のdispatchが固定文字列しか返さないため
/// 際限なく呼ばれ続けることは想定しないが、暴走時の安全弁として上限を設ける。
const MAX_TOOL_ROUNDS: usize = 5;

/// OpenAI Responses API への同期的な（background=trueのポーリングをしない）HTTPクライアント。
pub struct OpenAiClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
    /// data/uranai/toolList.yaml から起動時に一度だけ読み込んだtool定義。
    tools: Vec<Value>,
}

impl OpenAiClient {
    pub fn from_env() -> Self {
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| panic!("OPENAI_API_KEY not set"));
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| panic!("OPENAI_MODEL not set"));
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("failed to build reqwest client for OpenAI");
        let tools = super::tools::load_tool_definitions()
            .expect("data/uranai/toolList.yaml の読み込みに失敗しました");
        Self { http, api_key, model, tools }
    }

    async fn call(&self, body: &Value) -> Result<Value, Error> {
        let resp = self
            .http
            .post(RESPONSES_URL)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        let parsed: Value = resp.json().await?;
        if !status.is_success() {
            return Err(format!("OpenAI API error ({status}): {}", error_message(&parsed)).into());
        }
        if parsed.get("status").and_then(Value::as_str) != Some("completed") {
            return Err(format!("response is not completed: {}", error_message(&parsed)).into());
        }
        Ok(parsed)
    }

    /// 1ターン分のやり取り: 直近の会話履歴(`history`)+新規ユーザー発言(`user_message`)を送信し、
    /// function_callが返ってきたら`tools::dispatch`（仮実装）の結果を提出してループ、
    /// 最終的な自然文を1つの文字列にまとめて返す。
    pub async fn run_turn(
        &self,
        instructions: &str,
        history: Vec<Value>,
        user_message: &str,
        state: &AppState,
    ) -> Result<String, Error> {
        let tools = &self.tools;
        let mut input = history;
        input.push(json!({"role": "user", "content": user_message}));

        let mut response = self
            .call(&json!({
                "model": self.model,
                "instructions": instructions,
                "input": input,
                "tools": tools,
            }))
            .await?;

        let mut collected = String::new();
        for round in 0.. {
            let output = response
                .get("output")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            let mut tool_outputs = Vec::new();
            for item in &output {
                match item.get("type").and_then(Value::as_str) {
                    Some("message") => collected.push_str(&extract_message_text(item)),
                    Some("function_call") => tool_outputs.push(run_tool_call(item, state).await),
                    _ => {}
                }
            }

            if tool_outputs.is_empty() || round >= MAX_TOOL_ROUNDS {
                break;
            }

            let response_id = response
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            response = self
                .call(&json!({
                    "model": self.model,
                    "previous_response_id": response_id,
                    "input": tool_outputs,
                    "tools": tools,
                }))
                .await?;
        }

        Ok(collected)
    }
}

fn error_message(parsed: &Value) -> &str {
    parsed
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("unknown error")
}

fn extract_message_text(item: &Value) -> String {
    let mut text = String::new();
    let Some(contents) = item.get("content").and_then(Value::as_array) else {
        return text;
    };
    for content in contents {
        match content.get("type").and_then(Value::as_str) {
            Some("output_text") => {
                if let Some(t) = content.get("text").and_then(Value::as_str) {
                    text.push_str(t);
                }
            }
            Some("refusal") => {
                if let Some(t) = content.get("refusal").and_then(Value::as_str) {
                    text.push_str(t);
                }
            }
            _ => {}
        }
    }
    text
}

async fn run_tool_call(item: &Value, state: &AppState) -> Value {
    let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or_default();
    let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
    let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or("{}");
    println!("[uranai] tool called: {name}({arguments})");
    let result = super::tools::dispatch(name, arguments, state).await;
    json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": result,
    })
}
