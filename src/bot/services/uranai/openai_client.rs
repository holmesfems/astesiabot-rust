use crate::api::AppState;
use crate::bot::data::Error;
use base64::Engine;
use serde_json::{json, Value};
use std::time::Duration;

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
/// tool-callループの最大往復数。仮実装のdispatchが固定文字列しか返さないため
/// 際限なく呼ばれ続けることは想定しないが、暴走時の安全弁として上限を設ける。
const MAX_TOOL_ROUNDS: usize = 5;

/// タイムアウト時間
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// ユーザーがDiscordに添付した画像/PDFを表すDTO。呼び出し元(`uranai::mod`)が
/// `msg.attachments`から分類して渡す。Discordの添付URLは`file_url`/`image_url`として
/// そのままResponses APIに渡せる（OpenAI側がサーバーサイドでfetchするため、こちら側での
/// ダウンロード/base64化/Files APIへの再アップロードは不要）。
pub struct UranaiAttachment {
    pub url: String,
    pub filename: String,
    pub kind: AttachmentKind,
    /// バイト数（まとめて送るときの合計サイズの上限に使う）。
    pub size: u64,
}

pub enum AttachmentKind {
    Image,
    Pdf,
}

/// 1ターン分のやり取りの結果。`text`は自然文の応答、`images`はGPTが
/// `image_generation`ツールで生成した画像のデコード済みバイト列（PNG）。
pub struct TurnOutput {
    pub text: String,
    pub images: Vec<Vec<u8>>,
}

/// ユーザー発言+添付をResponses APIの`content`形式に組み立てる。
/// 添付が無ければ従来通りプレーン文字列（会話履歴に積む形式と揃える）。
fn build_user_content(user_message: &str, attachments: &[UranaiAttachment]) -> Value {
    if attachments.is_empty() {
        return json!(user_message);
    }
    let mut parts = vec![json!({"type": "input_text", "text": user_message})];
    for a in attachments {
        parts.push(match a.kind {
            AttachmentKind::Image => json!({"type": "input_image", "image_url": a.url}),
            AttachmentKind::Pdf => {
                json!({"type": "input_file", "file_url": a.url, "filename": a.filename})
            }
        });
    }
    json!(parts)
}

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
            .timeout(REQUEST_TIMEOUT)
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

    /// 会話履歴もtoolも使わない、1往復だけの文章生成（YouTube新着のお知らせの宣伝文など）。
    pub async fn generate_text(&self, instructions: &str, user_message: &str) -> Result<String, Error> {
        let response = self
            .call(&json!({
                "model": self.model,
                "instructions": instructions,
                "input": [{"role": "user", "content": user_message}],
            }))
            .await?;
        let text = response
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
            .map(extract_message_text)
            .collect();
        Ok(text)
    }

    /// 1ターン分のやり取り: 直近の会話履歴(`history`)+新規ユーザー発言(`user_message`)を送信し、
    /// function_callが返ってきたら`tools::dispatch`の結果を提出してループ、
    /// 最終的な自然文を1つの文字列にまとめて返す（占い館）。
    pub async fn run_turn(
        &self,
        instructions: &str,
        history: Vec<Value>,
        user_message: &str,
        attachments: &[UranaiAttachment],
        state: &AppState,
    ) -> Result<TurnOutput, Error> {
        let out = self
            .run_loop(instructions, history, user_message, attachments, &self.tools, None, state)
            .await?;
        Ok(TurnOutput { text: out.all_text, images: out.images })
    }

    /// 返事の要否を AI に決めさせる1ターン（メンション会話）。出力を `REPLY_SCHEMA` の JSON に
    /// 縛り、最後のラウンドの message だけを読む（ツールの前置きの文を混ぜない）。
    /// `allow_image` が false なら画像生成をツール一覧から外す。
    /// JSON が読めなければ `Err`（生の JSON を Discord に出さないため、呼び出し側はエラーとして扱う）。
    pub async fn run_structured_turn(
        &self,
        instructions: &str,
        history: Vec<Value>,
        user_message: &str,
        attachments: &[UranaiAttachment],
        allow_image: bool,
        state: &AppState,
    ) -> Result<StructuredTurn, Error> {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .filter(|t| allow_image || t.get("type").and_then(Value::as_str) != Some("image_generation"))
            .cloned()
            .collect();
        let format = json!({
            "format": {
                "type": "json_schema",
                "name": "astesia_reply",
                "strict": true,
                "schema": reply_schema(),
            }
        });
        let out = self
            .run_loop(instructions, history, user_message, attachments, &tools, Some(&format), state)
            .await?;
        let reply = parse_structured_reply(&out.last_text)
            .ok_or_else(|| format!("structured output could not be parsed ({} chars)", out.last_text.chars().count()))?;
        Ok(StructuredTurn { reply, images: out.images })
    }

    /// ツールの往復を含む1ターン。2回目以降も `instructions`・`tools`・`text`（出力の形）を毎回送る
    /// （`previous_response_id` ではどれも引き継がれない）。
    #[allow(clippy::too_many_arguments)]
    async fn run_loop(
        &self,
        instructions: &str,
        history: Vec<Value>,
        user_message: &str,
        attachments: &[UranaiAttachment],
        tools: &[Value],
        text_format: Option<&Value>,
        state: &AppState,
    ) -> Result<LoopOutput, Error> {
        let mut input = history;
        input.push(json!({"role": "user", "content": build_user_content(user_message, attachments)}));

        let mut body = json!({
            "model": self.model,
            "instructions": instructions,
            "input": input,
            "tools": tools,
        });
        if let Some(format) = text_format {
            body["text"] = format.clone();
        }
        let mut response = self.call(&body).await?;

        let mut all_text = String::new();
        let mut last_text = String::new();
        let mut images_b64 = Vec::new();
        for round in 0.. {
            let output = response
                .get("output")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            let mut round_text = String::new();
            let mut tool_outputs = Vec::new();
            for item in &output {
                match item.get("type").and_then(Value::as_str) {
                    Some("message") => round_text.push_str(&extract_message_text(item)),
                    Some("function_call") => tool_outputs.push(run_tool_call(item, state).await),
                    Some("image_generation_call") => {
                        if let Some(b64) = item.get("result").and_then(Value::as_str) {
                            images_b64.push(b64.to_string());
                        }
                    }
                    _ => {}
                }
            }
            all_text.push_str(&round_text);
            last_text = round_text;

            if tool_outputs.is_empty() || round >= MAX_TOOL_ROUNDS {
                break;
            }

            let response_id = response
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            // `previous_response_id` では `instructions` 等は引き継がれないので、毎回送り直す
            // （送らないとツールを使った後の返事でキャラ設定・出力の形が抜ける）。
            let mut body = json!({
                "model": self.model,
                "instructions": instructions,
                "previous_response_id": response_id,
                "input": tool_outputs,
                "tools": tools,
            });
            if let Some(format) = text_format {
                body["text"] = format.clone();
            }
            response = self.call(&body).await?;
        }

        let images = images_b64
            .into_iter()
            .filter_map(|b64| match base64::engine::general_purpose::STANDARD.decode(&b64) {
                Ok(bytes) => Some(bytes),
                Err(e) => {
                    eprintln!("[uranai] image_generation_call の画像デコードに失敗しました: {e}");
                    None
                }
            })
            .collect();

        Ok(LoopOutput { all_text, last_text, images })
    }
}

struct LoopOutput {
    /// 全ラウンドの message を連結したもの（占い館の返事）。
    all_text: String,
    /// 最後のラウンドの message だけ（構造化出力の JSON）。
    last_text: String,
    images: Vec<Vec<u8>>,
}

/// AI が決めた、このターンの動き。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyAction {
    Reply,
    Silent,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredReply {
    pub action: ReplyAction,
    pub message: String,
}

pub struct StructuredTurn {
    pub reply: StructuredReply,
    pub images: Vec<Vec<u8>>,
}

fn reply_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": ["reply", "silent", "end"]},
            "message": {"type": "string"},
        },
        "required": ["action", "message"],
        "additionalProperties": false,
    })
}

/// 構造化出力の JSON を読む。読めなければ `None`。
fn parse_structured_reply(text: &str) -> Option<StructuredReply> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let action = match v.get("action")?.as_str()? {
        "reply" => ReplyAction::Reply,
        "silent" => ReplyAction::Silent,
        "end" => ReplyAction::End,
        _ => return None,
    };
    let message = v.get("message")?.as_str()?.to_string();
    Some(StructuredReply { action, message })
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
    // 引数はユーザーの発言から作られるので、ログには関数名だけ出す。
    println!("[uranai] tool called: {name}");
    let result = super::tools::dispatch(name, arguments, state).await;
    json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本物の OpenAI で、構造化出力（json_schema）と占い館のツール一式（function・web_search・
    /// image_generation）を一緒に送って受け付けられ、返事が読めるか。.env の OPENAI_API_KEY / OPENAI_MODEL を使う。
    /// `cargo test --lib -- --ignored structured_output_live --nocapture`
    #[tokio::test]
    #[ignore]
    async fn structured_output_live() {
        dotenvy::dotenv().ok();
        let client = OpenAiClient::from_env();
        let instructions = format!(
            "{}\n\n{}",
            include_str!("system_prompt.txt"),
            include_str!("mention/prompt.txt")
        );
        let format = json!({
            "format": {"type": "json_schema", "name": "astesia_reply", "strict": true, "schema": reply_schema()}
        });
        let cases = [
            ("from ドクターA:\n アステシア、今日の星はどう？", Some(ReplyAction::Reply)),
            ("from ドクターA:\n ありがとう、楽しかった！またね", Some(ReplyAction::End)),
            ("from ドクターA:\n @ドクターB このあとご飯行く？\n\nfrom ドクターB:\n 行く行く！", None),
        ];
        for (text, expected) in cases {
            let body = json!({
                "model": client.model,
                "instructions": instructions,
                "input": [{"role": "user", "content": text}],
                "tools": client.tools,
                "text": format,
            });
            let response = client.call(&body).await.expect("request accepted");
            let last: String = response["output"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|i| i["type"] == "message")
                .map(extract_message_text)
                .collect();
            let reply = parse_structured_reply(&last).unwrap_or_else(|| panic!("unparsable: {last}"));
            println!("{text:?} => {reply:?}");
            if let Some(expected) = expected {
                assert_eq!(reply.action, expected, "{text}");
            }
        }
    }

    #[test]
    fn parses_structured_reply() {
        assert_eq!(
            parse_structured_reply(r#"{"action":"reply","message":"こんにちは"}"#),
            Some(StructuredReply { action: ReplyAction::Reply, message: "こんにちは".into() })
        );
        assert_eq!(
            parse_structured_reply(" {\"action\":\"silent\",\"message\":\"\"}\n").map(|r| r.action),
            Some(ReplyAction::Silent)
        );
        assert_eq!(parse_structured_reply(r#"{"action":"end","message":"またね"}"#).map(|r| r.action), Some(ReplyAction::End));
        assert_eq!(parse_structured_reply(r#"{"action":"dance","message":""}"#), None);
        assert_eq!(parse_structured_reply("調べるわね{\"action\":\"reply\",\"message\":\"x\"}"), None);
        assert_eq!(parse_structured_reply(""), None);
    }
}
