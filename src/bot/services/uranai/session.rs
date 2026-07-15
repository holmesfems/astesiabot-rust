use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// 直近10往復程度を保持する会話メモリ。ユーザー区別なしの単一グローバルセッション
/// （呼び出し側が `tokio::sync::Mutex<ChatSession>` で1個だけ保持する想定）。
/// Python版の `previous_response_id` 無制限チェーンとは異なり、こちらは
/// 明示的に往復数を打ち切る方針を優先する（要件定義で確定済み）。
pub struct ChatSession {
    messages: Vec<(Role, String)>,
    last_updated: Instant,
}

#[derive(Clone, Copy)]
enum Role {
    User,
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

impl ChatSession {
    /// 保持する最大往復数（1往復 = user+assistantの2メッセージ）。
    const MAX_EXCHANGES: usize = 10;

    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            last_updated: Instant::now(),
        }
    }

    pub fn is_expired(&self, timeout: Duration) -> bool {
        self.last_updated.elapsed() > timeout
    }

    pub fn reset(&mut self) {
        self.messages.clear();
        self.last_updated = Instant::now();
    }

    pub fn touch(&mut self) {
        self.last_updated = Instant::now();
    }

    /// 保持している履歴をOpenAI Responses APIの`input`形式に変換する
    /// （新規ユーザー発言はここには含まない。呼び出し側が末尾に追加する）。
    pub fn history_input(&self) -> Vec<Value> {
        self.messages
            .iter()
            .map(|(role, content)| json!({"role": role.as_str(), "content": content}))
            .collect()
    }

    /// 1往復（ユーザー発言+アシスタント応答）を履歴に追加し、上限を超えた分は古い方から捨てる。
    pub fn push_exchange(&mut self, user_message: String, assistant_message: String) {
        self.messages.push((Role::User, user_message));
        self.messages.push((Role::Assistant, assistant_message));
        let max_messages = Self::MAX_EXCHANGES * 2;
        if self.messages.len() > max_messages {
            let excess = self.messages.len() - max_messages;
            self.messages.drain(0..excess);
        }
        self.touch();
    }
}
