//! メンション会話の状態遷移（純粋ロジック。Discord・OpenAI を触らない）。
//!
//! - メンションで会話を始める（課金ロール持ちだけ）。1チャンネルに会話は1つ、鯖全体で同時 `max_sessions` まで。
//! - 会話中は「参加者」（始めた人＋途中でメンションして加わった課金ロール持ち）の発言をメンション無しで拾う。
//! - 考え中の発言の貯め方・まとめ方は占い館と同じ `Conversation` を使う。
//! - 閉じる理由: AI の end・呼び出し上限・silent の連続・エラーの連続・無操作・送信失敗・reset。
//!
//! ID はすべて素の `u64`（ChannelId・UserId・MessageId）で持ち、単体テストで Discord の型を作らなくて済むようにする。

use super::super::conversation::{Batch, Conversation, Enqueue, PendingInput};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Limits {
    /// 鯖全体で同時に開ける会話の数。
    pub max_sessions: usize,
    /// 1つの会話の参加者の最大数。
    pub max_participants: usize,
    /// 1つの会話で AI を呼べる回数。
    pub max_calls: u32,
    /// silent がこの回数続いたら黙って閉じる。
    pub max_consecutive_silent: u32,
    /// エラーがこの回数続いたら閉じる。
    pub max_consecutive_errors: u32,
    /// 1つの会話で画像を作れる回数。
    pub max_images: u32,
    /// 参加者の最後の発言からこれだけ過ぎたら閉じる。
    pub idle_timeout: Duration,
    /// 課金ロールが無い人のメンションを無視する時間。
    pub denied_cooldown: Duration,
    /// 満席の案内を同じ人に出さない時間。
    pub busy_notice_cooldown: Duration,
    /// 案内をチャンネル単位で連投しない時間。
    pub channel_notice_cooldown: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sessions: 3,
            max_participants: 5,
            max_calls: 20,
            max_consecutive_silent: 5,
            max_consecutive_errors: 3,
            max_images: 1,
            idle_timeout: Duration::from_secs(10 * 60),
            denied_cooldown: Duration::from_secs(10 * 60),
            busy_notice_cooldown: Duration::from_secs(10 * 60),
            channel_notice_cooldown: Duration::from_secs(30),
        }
    }
}

/// 来た発言（入口で guild・チャンネル・メッセージの種類の絞り込みを済ませたもの）。
pub struct Incoming {
    pub channel: u64,
    pub user: u64,
    /// 本文に bot へのメンションがある（リプライの自動メンションだけなら false）。
    pub mentions_bot: bool,
    pub has_paid_role: bool,
    pub is_admin: bool,
    /// メンション付きの `reset`（会話を閉じる）。
    pub is_reset: bool,
    pub input: PendingInput,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Notice {
    /// 課金ロールが無い。
    NoRole,
    /// 同時に開ける会話の数を超えた。
    Full,
}

#[derive(Debug, PartialEq, Eq)]
pub enum IncomingAction {
    Ignore,
    /// 決まった文面の案内を送る（AI は呼ばない）。
    Notice(Notice),
    /// ワーカーを spawn する。`started` は新しく会話を作ったか（ログ用）。
    StartWorker {
        session_id: u64,
        started: bool,
    },
    /// 考え中なので貯めた。
    Queued,
    /// reset で閉じた。決まった文面を送る。
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    End,
    CallLimit,
    Silent,
    Errors,
    DailyLimit,
    SendFailed,
}

pub enum WorkerStep {
    Run {
        batch: Batch,
        history: Vec<Value>,
        allow_image: bool,
    },
    /// 会話が無い・別の会話に替わった・貯まった発言が無い。ワーカーは黙って終わる。
    Done,
    /// 閉じた。決まった文面を送って終わる。
    Closed(CloseReason),
}

/// AI の返事（画像を作ったのに silent なら、呼び出し側で Reply に直してから渡す）。
pub enum Outcome {
    Reply { message: String, made_image: bool },
    Silent,
    End { message: String },
    Error,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FinishAction {
    /// 返事を送って続ける。
    Send(String),
    /// 何も送らずに続ける。
    Nothing,
    /// 返事（お別れ）を送ってから閉じた。メッセージが空なら何も送らない。
    SendAndClosed(String, CloseReason),
    /// 決まった文面で閉じた（silent 続きは文面なし）。
    Closed(CloseReason),
    /// エラーの一言を送って続ける。
    SendError,
}

struct ChannelSession {
    id: u64,
    participants: HashSet<u64>,
    conv: Conversation,
    last_participant_at: Instant,
    calls: u32,
    consecutive_silent: u32,
    consecutive_errors: u32,
    images: u32,
}

pub struct MentionSessions {
    limits: Limits,
    sessions: HashMap<u64, ChannelSession>,
    next_id: u64,
    denied_until: HashMap<u64, Instant>,
    busy_notice_until: HashMap<u64, Instant>,
    channel_notice_until: HashMap<u64, Instant>,
}

impl MentionSessions {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            sessions: HashMap::new(),
            next_id: 1,
            denied_until: HashMap::new(),
            busy_notice_until: HashMap::new(),
            channel_notice_until: HashMap::new(),
        }
    }

    /// このチャンネルに会話があるか（入口で、会話の無いチャンネルだけ除外チャンネルを確かめるのに使う）。
    pub fn has_session(&self, channel: u64) -> bool {
        self.sessions.contains_key(&channel)
    }

    pub fn on_message(&mut self, msg: Incoming, now: Instant) -> IncomingAction {
        self.sweep(now);

        if let Some(session) = self.sessions.get_mut(&msg.channel) {
            let is_participant = session.participants.contains(&msg.user);
            if msg.mentions_bot && msg.is_reset && (is_participant || msg.is_admin) {
                self.sessions.remove(&msg.channel);
                return IncomingAction::Closed;
            }
            let joins = !is_participant
                && msg.mentions_bot
                && msg.has_paid_role
                && session.participants.len() < self.limits.max_participants;
            if is_participant || joins {
                session.participants.insert(msg.user);
                session.last_participant_at = now;
                let id = session.id;
                return match session.conv.enqueue(msg.input) {
                    Enqueue::StartWorker => IncomingAction::StartWorker {
                        session_id: id,
                        started: false,
                    },
                    Enqueue::Queued { .. } => IncomingAction::Queued,
                };
            }
            if msg.mentions_bot && !msg.has_paid_role {
                return self.deny(msg.user, msg.channel, now);
            }
            return IncomingAction::Ignore;
        }

        if !msg.mentions_bot || msg.is_reset {
            return IncomingAction::Ignore;
        }
        if self.denied_until.get(&msg.user).is_some_and(|&t| now < t) {
            return IncomingAction::Ignore;
        }
        if !msg.has_paid_role {
            return self.deny(msg.user, msg.channel, now);
        }
        if self.sessions.len() >= self.limits.max_sessions {
            if self
                .busy_notice_until
                .get(&msg.user)
                .is_some_and(|&t| now < t)
            {
                return IncomingAction::Ignore;
            }
            self.busy_notice_until
                .insert(msg.user, now + self.limits.busy_notice_cooldown);
            return self.notice(msg.channel, now, Notice::Full);
        }

        let id = self.next_id;
        self.next_id += 1;
        // 会話の寿命はこちらの idle_timeout で管理するので、Conversation 側の履歴の期限切れは使わない。
        let mut conv = Conversation::new(Duration::MAX);
        let enqueued = conv.enqueue(msg.input);
        debug_assert_eq!(enqueued, Enqueue::StartWorker);
        self.sessions.insert(
            msg.channel,
            ChannelSession {
                id,
                participants: HashSet::from([msg.user]),
                conv,
                last_participant_at: now,
                calls: 0,
                consecutive_silent: 0,
                consecutive_errors: 0,
                images: 0,
            },
        );
        IncomingAction::StartWorker {
            session_id: id,
            started: true,
        }
    }

    /// ワーカーが次にやること。呼び出し上限に来ていたら閉じる。
    pub fn take_batch(&mut self, channel: u64, session_id: u64) -> WorkerStep {
        let Some(session) = self
            .sessions
            .get_mut(&channel)
            .filter(|s| s.id == session_id)
        else {
            return WorkerStep::Done;
        };
        // 上限まで答えたあとに発言が来たら、決まった文面のお別れで閉じる。
        if session.conv.has_pending() && session.calls >= self.limits.max_calls {
            self.sessions.remove(&channel);
            return WorkerStep::Closed(CloseReason::CallLimit);
        }
        let Some(batch) = session.conv.take_batch() else {
            return WorkerStep::Done;
        };
        session.calls += 1;
        WorkerStep::Run {
            history: session.conv.history_input(),
            allow_image: session.images < self.limits.max_images,
            batch,
        }
    }

    /// AI の返事を受けて、履歴に積み、何を送るかを決める。考え中に reset 等で会話が替わっていても、
    /// 返事やお別れは送る（エラーの一言は送らない）。
    pub fn finish(
        &mut self,
        channel: u64,
        session_id: u64,
        generation: u64,
        user_text: String,
        outcome: Outcome,
    ) -> FinishAction {
        let max_silent = self.limits.max_consecutive_silent;
        let max_errors = self.limits.max_consecutive_errors;
        let Some(session) = self
            .sessions
            .get_mut(&channel)
            .filter(|s| s.id == session_id)
        else {
            return match outcome {
                Outcome::Reply { message, .. } if !message.trim().is_empty() => {
                    FinishAction::Send(message)
                }
                Outcome::End { message } => FinishAction::SendAndClosed(message, CloseReason::End),
                _ => FinishAction::Nothing,
            };
        };
        let (action, close) = match outcome {
            // 中身の無い reply は silent と同じに扱う。
            Outcome::Reply {
                message,
                made_image: false,
            } if message.trim().is_empty() => {
                finish_silent(session, generation, user_text, max_silent)
            }
            Outcome::Reply {
                message,
                made_image,
            } => {
                if made_image {
                    session.images += 1;
                }
                session.consecutive_errors = 0;
                session.consecutive_silent = 0;
                session
                    .conv
                    .finish_turn(generation, user_text, message.clone());
                (FinishAction::Send(message), false)
            }
            Outcome::Silent => finish_silent(session, generation, user_text, max_silent),
            Outcome::End { message } => {
                (FinishAction::SendAndClosed(message, CloseReason::End), true)
            }
            Outcome::Error => {
                session.consecutive_errors += 1;
                if session.consecutive_errors >= max_errors {
                    (FinishAction::Closed(CloseReason::Errors), true)
                } else {
                    (FinishAction::SendError, false)
                }
            }
        };
        if close {
            self.sessions.remove(&channel);
        }
        action
    }

    /// 送信失敗・日次上限などで会話を閉じる（別の会話に替わっていたら何もしない）。
    pub fn close(&mut self, channel: u64, session_id: u64) {
        if self
            .sessions
            .get(&channel)
            .is_some_and(|s| s.id == session_id)
        {
            self.sessions.remove(&channel);
        }
    }

    /// ワーカーが panic・早期リターンで抜けたとき。busy を下ろして貯まった発言を捨てる。
    pub fn abort_worker(&mut self, channel: u64, session_id: u64) {
        if let Some(session) = self
            .sessions
            .get_mut(&channel)
            .filter(|s| s.id == session_id)
        {
            session.conv.abort_worker();
        }
    }

    fn deny(&mut self, user: u64, channel: u64, now: Instant) -> IncomingAction {
        if self.denied_until.get(&user).is_some_and(|&t| now < t) {
            return IncomingAction::Ignore;
        }
        self.denied_until
            .insert(user, now + self.limits.denied_cooldown);
        self.notice(channel, now, Notice::NoRole)
    }

    /// 案内はチャンネル単位でも連投しない（大勢が一度にメンションしても1回だけ）。
    fn notice(&mut self, channel: u64, now: Instant, notice: Notice) -> IncomingAction {
        if self
            .channel_notice_until
            .get(&channel)
            .is_some_and(|&t| now < t)
        {
            return IncomingAction::Ignore;
        }
        self.channel_notice_until
            .insert(channel, now + self.limits.channel_notice_cooldown);
        IncomingAction::Notice(notice)
    }

    /// 無操作で期限が来た会話（考え中でないもの）と、期限切れの冷却を捨てる。
    /// タイマーは持たず、発言が来るたびにここで掃除する（放置された会話が同時会話の枠を埋め続けないように）。
    fn sweep(&mut self, now: Instant) {
        let timeout = self.limits.idle_timeout;
        self.sessions.retain(|_, s| {
            s.conv.is_busy() || now.duration_since(s.last_participant_at) <= timeout
        });
        self.denied_until.retain(|_, &mut t| now < t);
        self.busy_notice_until.retain(|_, &mut t| now < t);
        self.channel_notice_until.retain(|_, &mut t| now < t);
    }
}

/// アステシアが黙っていたターン。続いたら閉じる（戻り値の bool が閉じるか）。
fn finish_silent(
    session: &mut ChannelSession,
    generation: u64,
    user_text: String,
    max_silent: u32,
) -> (FinishAction, bool) {
    session.consecutive_errors = 0;
    session.consecutive_silent += 1;
    if session.consecutive_silent >= max_silent {
        return (FinishAction::Closed(CloseReason::Silent), true);
    }
    session.conv.finish_silent(generation, user_text);
    (FinishAction::Nothing, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CH: u64 = 100;
    const ALICE: u64 = 1;
    const BOB: u64 = 2;
    const CAROL: u64 = 3;

    fn msg(
        channel: u64,
        user: u64,
        text: &str,
        mentions_bot: bool,
        has_paid_role: bool,
    ) -> Incoming {
        Incoming {
            channel,
            user,
            mentions_bot,
            has_paid_role,
            is_admin: false,
            is_reset: false,
            input: PendingInput {
                text: text.to_string(),
                attachments: Vec::new(),
                reply_to: Some(user),
            },
        }
    }

    fn start(s: &mut MentionSessions, now: Instant) -> u64 {
        match s.on_message(msg(CH, ALICE, "hi", true, true), now) {
            IncomingAction::StartWorker {
                session_id,
                started: true,
            } => session_id,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn run(s: &mut MentionSessions, id: u64) -> Batch {
        match s.take_batch(CH, id) {
            WorkerStep::Run { batch, .. } => batch,
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn plain_message_without_session_is_ignored() {
        let mut s = MentionSessions::new(Limits::default());
        assert_eq!(
            s.on_message(msg(CH, ALICE, "hi", false, true), Instant::now()),
            IncomingAction::Ignore
        );
        assert!(!s.has_session(CH));
    }

    #[test]
    fn no_role_gets_one_notice_then_cooldown() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        assert_eq!(
            s.on_message(msg(CH, ALICE, "hi", true, false), now),
            IncomingAction::Notice(Notice::NoRole)
        );
        let later = now + Duration::from_secs(60);
        assert_eq!(
            s.on_message(msg(CH, ALICE, "hi", true, false), later),
            IncomingAction::Ignore
        );
        // 冷却が明けたらまた一言
        let after = now + Duration::from_secs(11 * 60);
        assert_eq!(
            s.on_message(msg(CH, ALICE, "hi", true, false), after),
            IncomingAction::Notice(Notice::NoRole)
        );
    }

    #[test]
    fn notices_are_not_repeated_in_same_channel() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        assert_eq!(
            s.on_message(msg(CH, ALICE, "hi", true, false), now),
            IncomingAction::Notice(Notice::NoRole)
        );
        assert_eq!(
            s.on_message(msg(CH, BOB, "hi", true, false), now),
            IncomingAction::Ignore
        );
        // BOB も冷却に入っている
        let later = now + Duration::from_secs(60);
        assert_eq!(
            s.on_message(msg(CH, BOB, "hi", true, false), later),
            IncomingAction::Ignore
        );
    }

    #[test]
    fn participants_continue_without_mention_others_are_ignored() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        let id = start(&mut s, now);
        run(&mut s, id);
        assert_eq!(
            s.on_message(msg(CH, ALICE, "next", false, true), now),
            IncomingAction::Queued
        );
        // 参加者でない人は、課金ロールがあってもメンション無しなら拾わない
        assert_eq!(
            s.on_message(msg(CH, BOB, "chat", false, true), now),
            IncomingAction::Ignore
        );
        // メンションすれば加わる
        assert_eq!(
            s.on_message(msg(CH, BOB, "join", true, true), now),
            IncomingAction::Queued
        );
        // 課金ロール無しのメンションは一言だけ
        assert_eq!(
            s.on_message(msg(CH, CAROL, "me too", true, false), now),
            IncomingAction::Notice(Notice::NoRole)
        );
        let batch = run(&mut s, id);
        assert_eq!(batch.text, "next\n\njoin");
        assert_eq!(batch.reply_to, Some(BOB));
    }

    #[test]
    fn participant_limit() {
        let limits = Limits {
            max_participants: 2,
            ..Limits::default()
        };
        let mut s = MentionSessions::new(limits);
        let now = Instant::now();
        let id = start(&mut s, now);
        run(&mut s, id);
        assert_eq!(
            s.on_message(msg(CH, BOB, "join", true, true), now),
            IncomingAction::Queued
        );
        assert_eq!(
            s.on_message(msg(CH, CAROL, "join", true, true), now),
            IncomingAction::Ignore
        );
    }

    #[test]
    fn full_server_gets_notice_and_idle_sessions_are_swept() {
        let limits = Limits {
            max_sessions: 1,
            ..Limits::default()
        };
        let mut s = MentionSessions::new(limits);
        let now = Instant::now();
        let id = start(&mut s, now);
        run(&mut s, id);
        assert!(s.take_batch(CH, id).is_done());
        assert_eq!(
            s.on_message(msg(200, BOB, "hi", true, true), now),
            IncomingAction::Notice(Notice::Full)
        );
        // 同じ人には10分出さない
        assert_eq!(
            s.on_message(
                msg(201, BOB, "hi", true, true),
                now + Duration::from_secs(60)
            ),
            IncomingAction::Ignore
        );
        // 最初の会話が放置されて10分過ぎたら、掃除されて枠が空く
        let later = now + Duration::from_secs(10 * 60 + 1);
        assert!(matches!(
            s.on_message(msg(202, CAROL, "hi", true, true), later),
            IncomingAction::StartWorker { started: true, .. }
        ));
        assert!(!s.has_session(CH));
    }

    #[test]
    fn busy_session_is_not_swept_even_after_timeout() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        let id = start(&mut s, now);
        run(&mut s, id); // 考え中のまま
        let later = now + Duration::from_secs(20 * 60);
        s.on_message(msg(999, BOB, "x", false, true), later);
        assert!(s.has_session(CH));
    }

    #[test]
    fn idle_session_ignores_unmentioned_participant() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        let id = start(&mut s, now);
        let b = run(&mut s, id);
        s.finish(
            CH,
            id,
            b.generation,
            b.text,
            Outcome::Reply {
                message: "hello".into(),
                made_image: false,
            },
        );
        assert!(s.take_batch(CH, id).is_done());
        let later = now + Duration::from_secs(10 * 60 + 1);
        assert_eq!(
            s.on_message(msg(CH, ALICE, "still there?", false, true), later),
            IncomingAction::Ignore
        );
        assert!(!s.has_session(CH));
    }

    #[test]
    fn end_closes_and_drops_pending() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        let id = start(&mut s, now);
        let b = run(&mut s, id);
        s.on_message(msg(CH, ALICE, "one more", false, true), now);
        let action = s.finish(
            CH,
            id,
            b.generation,
            b.text,
            Outcome::End {
                message: "またね".into(),
            },
        );
        assert_eq!(
            action,
            FinishAction::SendAndClosed("またね".into(), CloseReason::End)
        );
        assert!(s.take_batch(CH, id).is_done());
        assert!(!s.has_session(CH));
    }

    #[test]
    fn call_limit_closes_session() {
        let limits = Limits {
            max_calls: 1,
            ..Limits::default()
        };
        let mut s = MentionSessions::new(limits);
        let now = Instant::now();
        let id = start(&mut s, now);
        let b = run(&mut s, id);
        s.on_message(msg(CH, ALICE, "more", false, true), now);
        s.finish(
            CH,
            id,
            b.generation,
            b.text,
            Outcome::Reply {
                message: "ok".into(),
                made_image: false,
            },
        );
        assert!(matches!(
            s.take_batch(CH, id),
            WorkerStep::Closed(CloseReason::CallLimit)
        ));
        assert!(!s.has_session(CH));
    }

    #[test]
    fn consecutive_silent_closes_quietly_and_reply_resets_count() {
        let limits = Limits {
            max_consecutive_silent: 2,
            ..Limits::default()
        };
        let mut s = MentionSessions::new(limits);
        let now = Instant::now();
        let id = start(&mut s, now);
        let b = run(&mut s, id);
        assert_eq!(
            s.finish(CH, id, b.generation, b.text, Outcome::Silent),
            FinishAction::Nothing
        );
        s.on_message(msg(CH, ALICE, "a", false, true), now);
        let b = run(&mut s, id);
        assert_eq!(
            s.finish(
                CH,
                id,
                b.generation,
                b.text,
                Outcome::Reply {
                    message: "r".into(),
                    made_image: false
                }
            ),
            FinishAction::Send("r".into())
        );
        s.on_message(msg(CH, ALICE, "b", false, true), now);
        let b = run(&mut s, id);
        assert_eq!(
            s.finish(CH, id, b.generation, b.text, Outcome::Silent),
            FinishAction::Nothing
        );
        s.on_message(msg(CH, ALICE, "c", false, true), now);
        let b = run(&mut s, id);
        assert_eq!(
            s.finish(CH, id, b.generation, b.text, Outcome::Silent),
            FinishAction::Closed(CloseReason::Silent)
        );
        assert!(!s.has_session(CH));
    }

    #[test]
    fn empty_reply_counts_as_silent() {
        let limits = Limits {
            max_consecutive_silent: 1,
            ..Limits::default()
        };
        let mut s = MentionSessions::new(limits);
        let id = start(&mut s, Instant::now());
        let b = run(&mut s, id);
        assert_eq!(
            s.finish(
                CH,
                id,
                b.generation,
                b.text,
                Outcome::Reply {
                    message: " ".into(),
                    made_image: false
                }
            ),
            FinishAction::Closed(CloseReason::Silent)
        );
    }

    #[test]
    fn consecutive_errors_close_session() {
        let limits = Limits {
            max_consecutive_errors: 2,
            ..Limits::default()
        };
        let mut s = MentionSessions::new(limits);
        let now = Instant::now();
        let id = start(&mut s, now);
        let b = run(&mut s, id);
        assert_eq!(
            s.finish(CH, id, b.generation, b.text, Outcome::Error),
            FinishAction::SendError
        );
        s.on_message(msg(CH, ALICE, "again", false, true), now);
        let b = run(&mut s, id);
        assert_eq!(
            s.finish(CH, id, b.generation, b.text, Outcome::Error),
            FinishAction::Closed(CloseReason::Errors)
        );
    }

    #[test]
    fn image_allowed_only_once() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        let id = start(&mut s, now);
        let WorkerStep::Run {
            batch, allow_image, ..
        } = s.take_batch(CH, id)
        else {
            panic!()
        };
        assert!(allow_image);
        s.finish(
            CH,
            id,
            batch.generation,
            batch.text,
            Outcome::Reply {
                message: String::new(),
                made_image: true,
            },
        );
        s.on_message(msg(CH, ALICE, "another", false, true), now);
        let WorkerStep::Run { allow_image, .. } = s.take_batch(CH, id) else {
            panic!()
        };
        assert!(!allow_image);
    }

    #[test]
    fn reset_by_participant_or_admin_only() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        let id = start(&mut s, now);
        run(&mut s, id);
        let mut r = msg(CH, BOB, "reset", true, true);
        r.is_reset = true;
        // 参加者でない BOB の reset は会話を閉じない（参加して reset という発言を渡すだけ）
        assert_eq!(s.on_message(r, now), IncomingAction::Queued);
        let mut r = msg(CH, CAROL, "reset", true, false);
        r.is_reset = true;
        r.is_admin = true;
        assert_eq!(s.on_message(r, now), IncomingAction::Closed);
        assert!(!s.has_session(CH));
    }

    #[test]
    fn reply_after_reset_is_still_sent_but_session_is_new() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        let old = start(&mut s, now);
        let b = run(&mut s, old);
        let mut r = msg(CH, ALICE, "reset", true, true);
        r.is_reset = true;
        assert_eq!(s.on_message(r, now), IncomingAction::Closed);
        let new = start(&mut s, now);
        assert_ne!(old, new);
        // 古いワーカーの結果は送るが、新しい会話には触らない
        assert_eq!(
            s.finish(
                CH,
                old,
                b.generation,
                b.text,
                Outcome::Reply {
                    message: "late".into(),
                    made_image: false
                }
            ),
            FinishAction::Send("late".into())
        );
        assert!(s.take_batch(CH, old).is_done());
        assert_eq!(
            s.finish(CH, old, 0, "x".into(), Outcome::Error),
            FinishAction::Nothing
        );
        let b = run(&mut s, new);
        assert_eq!(b.text, "hi");
    }

    #[test]
    fn abort_worker_lets_next_message_start_worker() {
        let mut s = MentionSessions::new(Limits::default());
        let now = Instant::now();
        let id = start(&mut s, now);
        run(&mut s, id);
        s.abort_worker(CH, id);
        assert_eq!(
            s.on_message(msg(CH, ALICE, "again", false, true), now),
            IncomingAction::StartWorker {
                session_id: id,
                started: false
            }
        );
    }

    impl WorkerStep {
        fn is_done(&self) -> bool {
            matches!(self, WorkerStep::Done)
        }
    }
}
