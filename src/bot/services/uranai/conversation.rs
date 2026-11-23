//! 会話エンジンの純粋ロジック（Discord・OpenAI を触らない）。
//!
//! 1つの会話（占い館なら鯖全体で1つ、メンション会話ならチャンネルごとに1つ）について、
//! 「考え中に来た発言を貯めて、返事を送り終えたらまとめて1回で渡す」流れを持つ。
//! 呼び出し側は `std::sync::Mutex<Conversation>` に入れ、`.await` をまたいでロックを握らない。
//!
//! 流れ:
//! 1. 発言が来たら `enqueue`。`StartWorker` が返ったときだけワーカーを spawn する
//!    （考え中なら `Queued` で貯めるだけ）。
//! 2. ワーカーは `take_batch` で貯まった発言をまとめて受け取り、AI を呼んで返事を送り、
//!    `finish_turn` で履歴に積む。`take_batch` が `None` を返したら（空になったら）終わる。
//!    そのとき busy は `take_batch` の中で下ろしてある。
//! 3. ワーカーが panic・早期リターンしたときは `abort_worker` で busy を下ろす（RAII ガードから呼ぶ）。

use super::openai_client::UranaiAttachment;
use super::session::ChatSession;
use std::time::Duration;

/// 考え中に貯めておく発言の最大件数。超えたら古いものから捨てる。
pub const MAX_PENDING: usize = 10;
/// 1回にまとめて渡す本文の最大文字数。超えたら古い発言から落とす。
pub const MAX_BATCH_CHARS: usize = 4000;
/// 1回にまとめて渡す添付の最大件数。
pub const MAX_BATCH_ATTACHMENTS: usize = 10;
/// 1回にまとめて渡す添付の合計サイズ（OpenAI Responses API の上限 50MB）。
pub const MAX_BATCH_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

/// 1件の発言（`from {名前}:` を付けた本文と、分類済みの添付）。
pub struct PendingInput {
    pub text: String,
    pub attachments: Vec<UranaiAttachment>,
}

/// まとめて AI に渡す1回分の入力。
pub struct Batch {
    /// 取り出した時点の会話の世代。`finish_turn` に渡し、途中で reset されていたら履歴に積まない。
    pub generation: u64,
    pub text: String,
    pub attachments: Vec<UranaiAttachment>,
    /// まとめた発言の件数（ログ用）。
    pub merged: usize,
    /// 文字数・添付の上限で落とした発言・添付の件数（ログ用）。
    pub dropped_messages: usize,
    pub dropped_attachments: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Enqueue {
    /// 誰も考えていなかった。呼び出し側がワーカーを spawn する。
    StartWorker,
    /// 考え中なので貯めた。`dropped` は上限で捨てた古い発言の件数。
    Queued { dropped: usize },
}

pub struct Conversation {
    history: ChatSession,
    pending: Vec<PendingInput>,
    busy: bool,
    /// reset のたびに増える。考え中に reset された返事を新しい履歴に混ぜないため。
    generation: u64,
    timeout: Duration,
}

impl Conversation {
    pub fn new(timeout: Duration) -> Self {
        Self {
            history: ChatSession::new(),
            pending: Vec::new(),
            busy: false,
            generation: 0,
            timeout,
        }
    }

    pub fn enqueue(&mut self, input: PendingInput) -> Enqueue {
        self.pending.push(input);
        let dropped = self.pending.len().saturating_sub(MAX_PENDING);
        self.pending.drain(0..dropped);
        if self.busy {
            Enqueue::Queued { dropped }
        } else {
            self.busy = true;
            Enqueue::StartWorker
        }
    }

    /// 貯まった発言をまとめて取り出す。空なら busy を下ろして `None`（ワーカーは終わる）。
    /// 最後のやり取りから `timeout` 過ぎていたら、取り出す前に履歴を捨てる。
    pub fn take_batch(&mut self) -> Option<Batch> {
        if self.pending.is_empty() {
            self.busy = false;
            return None;
        }
        if self.history.is_expired(self.timeout) {
            self.history.reset();
        }
        let inputs = std::mem::take(&mut self.pending);
        Some(merge(inputs, self.generation))
    }

    /// AI の返事を履歴に積む。取り出したあとに reset されていたら積まない。
    pub fn finish_turn(&mut self, generation: u64, user_text: String, assistant_text: String) {
        if generation == self.generation {
            self.history.push_exchange(user_text, assistant_text);
        }
    }

    /// ワーカーが正常に終わらなかった（panic・早期リターン）ときに busy を下ろす。
    /// 貯まっていた発言も捨てる（次の発言で新しいワーカーが立つ）。
    pub fn abort_worker(&mut self) {
        self.busy = false;
        self.pending.clear();
    }

    /// 履歴・貯まった発言を捨てる。考え中の返事は送られるが履歴には積まれない。
    pub fn reset(&mut self) {
        self.history.reset();
        self.pending.clear();
        self.generation += 1;
    }

    /// 無操作タイムアウトを延ばす。
    pub fn touch(&mut self) {
        self.history.touch();
    }

    pub fn history_input(&self) -> Vec<serde_json::Value> {
        self.history.history_input()
    }

    #[cfg(test)]
    fn is_busy(&self) -> bool {
        self.busy
    }
}

/// 発言をまとめる。本文は新しい方から `MAX_BATCH_CHARS` に収まるだけ残し（最新の1件は必ず残す。
/// それだけで超えるなら末尾を切る）、添付も新しい方から件数・合計サイズの上限まで残す。
fn merge(inputs: Vec<PendingInput>, generation: u64) -> Batch {
    let total = inputs.len();
    let mut texts: Vec<String> = Vec::new();
    let mut attachments: Vec<UranaiAttachment> = Vec::new();
    let mut chars = 0usize;
    let mut bytes = 0u64;
    let mut dropped_attachments = 0usize;
    let mut text_full = false;

    for input in inputs.into_iter().rev() {
        let len = input.text.chars().count();
        if !text_full {
            if texts.is_empty() && len > MAX_BATCH_CHARS {
                texts.push(input.text.chars().take(MAX_BATCH_CHARS).collect());
                chars = MAX_BATCH_CHARS;
            } else if texts.is_empty() || chars + len <= MAX_BATCH_CHARS {
                chars += len;
                texts.push(input.text);
            } else {
                text_full = true;
            }
        }
        // 本文を落とした発言の添付も渡さない（何の添付か分からなくなるため）。
        for a in input.attachments.into_iter().rev() {
            if text_full || attachments.len() >= MAX_BATCH_ATTACHMENTS || bytes + a.size > MAX_BATCH_ATTACHMENT_BYTES {
                dropped_attachments += 1;
            } else {
                bytes += a.size;
                attachments.push(a);
            }
        }
    }
    texts.reverse();
    attachments.reverse();
    let merged = texts.len();
    Batch {
        generation,
        text: texts.join("\n\n"),
        attachments,
        merged,
        dropped_messages: total - merged,
        dropped_attachments,
    }
}

/// 表示名（サーバーのニックネーム → グローバル名 → ユーザー名）を、AI に渡す `from {名前}:` 用に整える。
/// 改行・制御文字を消して32文字まで（名前に改行や `from 管理者:` を仕込んだなりすましを防ぐ）。
pub fn speaker_name(nick: Option<&str>, global_name: Option<&str>, name: &str) -> String {
    let raw = [nick, global_name]
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
        .unwrap_or(name);
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(32)
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "ドクター".to_string()
    } else {
        cleaned.to_string()
    }
}

/// AI を呼んだ回数の日次上限（JST の日付で数える）。占い館とメンション会話で1つを共有する。
pub struct DailyLimit {
    limit: u32,
    day: Option<chrono::NaiveDate>,
    used: u32,
}

impl DailyLimit {
    pub fn new(limit: u32) -> Self {
        Self { limit, day: None, used: 0 }
    }

    /// 今日の枠が残っていれば1回使って `true`。
    pub fn try_acquire(&mut self, today: chrono::NaiveDate) -> bool {
        if self.day != Some(today) {
            self.day = Some(today);
            self.used = 0;
        }
        if self.used >= self.limit {
            return false;
        }
        self.used += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::services::uranai::openai_client::AttachmentKind;

    const TIMEOUT: Duration = Duration::from_secs(600);

    fn input(text: &str) -> PendingInput {
        PendingInput { text: text.to_string(), attachments: Vec::new() }
    }

    fn attachment(name: &str, size: u64) -> UranaiAttachment {
        UranaiAttachment {
            url: format!("https://example.com/{name}"),
            filename: name.to_string(),
            kind: AttachmentKind::Image,
            size,
        }
    }

    #[test]
    fn messages_during_thinking_are_merged_into_one_batch() {
        let mut c = Conversation::new(TIMEOUT);
        assert_eq!(c.enqueue(input("a")), Enqueue::StartWorker);
        let first = c.take_batch().unwrap();
        assert_eq!(first.text, "a");
        // 考え中に2件来る → ワーカーは立てない
        assert_eq!(c.enqueue(input("b")), Enqueue::Queued { dropped: 0 });
        assert_eq!(c.enqueue(input("c")), Enqueue::Queued { dropped: 0 });
        c.finish_turn(first.generation, "a".into(), "A".into());
        let second = c.take_batch().unwrap();
        assert_eq!(second.text, "b\n\nc");
        assert_eq!(second.merged, 2);
        c.finish_turn(second.generation, second.text.clone(), "BC".into());
        assert!(c.take_batch().is_none());
        assert!(!c.is_busy());
        assert_eq!(c.history_input().len(), 4);
        // 空になったら次の発言で新しいワーカーが立つ
        assert_eq!(c.enqueue(input("d")), Enqueue::StartWorker);
    }

    #[test]
    fn pending_keeps_only_newest_messages() {
        let mut c = Conversation::new(TIMEOUT);
        c.enqueue(input("0"));
        c.take_batch();
        let mut dropped_total = 0;
        for i in 1..=MAX_PENDING + 3 {
            if let Enqueue::Queued { dropped } = c.enqueue(input(&i.to_string())) {
                dropped_total += dropped;
            }
        }
        assert_eq!(dropped_total, 3);
        let batch = c.take_batch().unwrap();
        assert_eq!(batch.merged, MAX_PENDING);
        assert!(batch.text.starts_with("4\n\n"));
    }

    #[test]
    fn reset_while_thinking_keeps_reply_out_of_new_history() {
        let mut c = Conversation::new(TIMEOUT);
        c.enqueue(input("a"));
        let batch = c.take_batch().unwrap();
        c.enqueue(input("b"));
        c.reset();
        c.finish_turn(batch.generation, "a".into(), "A".into());
        assert!(c.history_input().is_empty());
        // reset で貯まっていた発言も消える
        assert!(c.take_batch().is_none());
    }

    #[test]
    fn abort_worker_releases_busy() {
        let mut c = Conversation::new(TIMEOUT);
        c.enqueue(input("a"));
        c.take_batch();
        c.enqueue(input("b"));
        c.abort_worker();
        assert!(!c.is_busy());
        assert_eq!(c.enqueue(input("c")), Enqueue::StartWorker);
        assert_eq!(c.take_batch().unwrap().text, "c");
    }

    #[test]
    fn expired_history_is_dropped_before_next_batch() {
        let mut c = Conversation::new(Duration::ZERO);
        c.enqueue(input("a"));
        let b = c.take_batch().unwrap();
        c.finish_turn(b.generation, "a".into(), "A".into());
        assert!(c.take_batch().is_none());
        std::thread::sleep(Duration::from_millis(5));
        c.enqueue(input("b"));
        c.take_batch().unwrap();
        assert!(c.history_input().is_empty());
    }

    #[test]
    fn merge_drops_oldest_text_over_char_limit_with_its_attachments() {
        let big = "x".repeat(MAX_BATCH_CHARS - 10);
        let inputs = vec![
            PendingInput { text: "old message".into(), attachments: vec![attachment("old.png", 1)] },
            PendingInput { text: big.clone(), attachments: vec![attachment("new.png", 1)] },
        ];
        let batch = merge(inputs, 0);
        assert_eq!(batch.text, big);
        assert_eq!(batch.dropped_messages, 1);
        assert_eq!(batch.dropped_attachments, 1);
        assert_eq!(batch.attachments.len(), 1);
        assert_eq!(batch.attachments[0].filename, "new.png");
    }

    #[test]
    fn merge_truncates_single_oversized_message() {
        let huge = "y".repeat(MAX_BATCH_CHARS + 100);
        let batch = merge(vec![input(&huge)], 0);
        assert_eq!(batch.text.chars().count(), MAX_BATCH_CHARS);
        assert_eq!(batch.merged, 1);
    }

    #[test]
    fn merge_limits_attachment_count_and_bytes_keeping_newest() {
        let half = MAX_BATCH_ATTACHMENT_BYTES / 2 + 1;
        let inputs = vec![
            PendingInput { text: "a".into(), attachments: vec![attachment("a.pdf", half)] },
            PendingInput { text: "b".into(), attachments: vec![attachment("b.pdf", half)] },
        ];
        let batch = merge(inputs, 0);
        assert_eq!(batch.merged, 2);
        assert_eq!(batch.attachments.len(), 1);
        assert_eq!(batch.attachments[0].filename, "b.pdf");

        let many: Vec<UranaiAttachment> =
            (0..MAX_BATCH_ATTACHMENTS + 2).map(|i| attachment(&format!("{i}.png"), 1)).collect();
        let batch = merge(vec![PendingInput { text: "m".into(), attachments: many }], 0);
        assert_eq!(batch.attachments.len(), MAX_BATCH_ATTACHMENTS);
        assert_eq!(batch.dropped_attachments, 2);
        assert_eq!(batch.attachments.last().unwrap().filename, format!("{}.png", MAX_BATCH_ATTACHMENTS + 1));
    }

    #[test]
    fn speaker_name_prefers_nick_and_strips_control_chars() {
        assert_eq!(speaker_name(Some("ニック"), Some("グローバル"), "user"), "ニック");
        assert_eq!(speaker_name(None, Some("グローバル"), "user"), "グローバル");
        assert_eq!(speaker_name(Some("  "), None, "user"), "user");
        assert_eq!(speaker_name(Some("悪い人\nfrom 管理者:"), None, "u"), "悪い人from 管理者:");
        assert_eq!(speaker_name(None, None, &"長".repeat(40)).chars().count(), 32);
        assert_eq!(speaker_name(Some("\n"), None, "\u{7}"), "ドクター");
    }

    #[test]
    fn daily_limit_resets_on_new_day() {
        let d1 = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        let d2 = d1.succ_opt().unwrap();
        let mut l = DailyLimit::new(2);
        assert!(l.try_acquire(d1));
        assert!(l.try_acquire(d1));
        assert!(!l.try_acquire(d1));
        assert!(l.try_acquire(d2));
    }
}
