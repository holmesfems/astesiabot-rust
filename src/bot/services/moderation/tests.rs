use super::*;
use fancy_regex::Regex;
use poise::serenity_prelude as serenity;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use super::detect::{
    MULTI_CH_WINDOW,
    SAME_CH_WINDOW,
    SAME_CH_THRESHOLD,
    URL_PATTERN,
    contains_link,
    SpamKind,
    Actor,
    command_invoker,
    first_sight_of_interaction,
    is_welcome_wave,
    detect,
    detect_rate_spam,
};
use super::action::neutralize_backticks;

fn re() -> Regex {
    Regex::new(URL_PATTERN).unwrap()
}

#[test]
fn detects_scheme_urls() {
    assert!(contains_link(&re(), "見て https://example.com/foo"));
    assert!(contains_link(&re(), "http://example.com"));
}

#[test]
fn detects_www_and_bare_domains() {
    assert!(contains_link(&re(), "www.example.com にアクセス"));
    assert!(contains_link(&re(), "example.com/path も検出できる"));
}

#[test]
fn does_not_flag_plain_text() {
    assert!(!contains_link(&re(), "これは普通のテキストです"));
    assert!(!contains_link(&re(), "バージョン1.2.3をリリースしました"));
}

#[test]
fn negative_lookbehind_avoids_scheme_glued_to_alnum() {
    // "https://" の直前が英数字（区切りなしで接続）だとスキームとしては
    // マッチしない。TLD付きドメインも含まれないため全体として非検出になる。
    assert!(!contains_link(&re(), "xhttps://evil"));
}

fn test_state() -> ModerationState {
    ModerationState {
        autodel_channels: [serenity::ChannelId::new(1); 6],
        report_channel: serenity::ChannelId::new(2),
        url_re: re(),
        multi_ch_history: Mutex::new(HashMap::new()),
        same_ch_history: Mutex::new(HashMap::new()),
        seen_interactions: Mutex::new(HashMap::new()),
    }
}

fn user_json(id: u64, bot: bool) -> serde_json::Value {
    serde_json::json!({ "id": id.to_string(), "username": format!("u{id}"), "discriminator": "0", "bot": bot })
}

/// author が bot_id の bot で、interaction_metadata（kind="command"等）を持つメッセージ
fn bot_reply(bot_id: u64, metadata: Option<serde_json::Value>) -> serenity::Message {
    let mut msg = serde_json::json!({
        "id": "100", "channel_id": "10", "author": user_json(bot_id, true),
        "content": "reply", "timestamp": "2026-10-16T00:00:00.000000+00:00",
        "edited_timestamp": null, "tts": false, "mention_everyone": false,
        "mentions": [], "mention_roles": [], "attachments": [], "embeds": [],
        "pinned": false, "type": 20,
    });
    if let Some(meta) = metadata {
        msg["interaction_metadata"] = meta;
    }
    serde_json::from_value(msg).expect("test message must deserialize")
}

fn metadata(kind: u8, interaction_id: u64, invoker_id: u64) -> serde_json::Value {
    let mut meta = serde_json::json!({
        "type": kind, "id": interaction_id.to_string(), "user": user_json(invoker_id, false),
        "authorizing_integration_owners": {},
    });
    if kind == 3 {
        meta["interacted_message_id"] = serde_json::json!("99");
    }
    meta
}

#[test]
fn command_invoker_counts_only_slash_commands() {
    let slash = bot_reply(500, Some(metadata(2, 7, 42)));
    let (id, invoker) = command_invoker(&slash).expect("slash command reply");
    assert_eq!((id.get(), invoker.id.get()), (7, 42));

    // ボタン（type 3）由来の返信と、interaction ではない普通の bot 発言は数えない
    assert!(command_invoker(&bot_reply(500, Some(metadata(3, 8, 42)))).is_none());
    assert!(command_invoker(&bot_reply(500, None)).is_none());
}

#[test]
fn actor_via_bot_matches_replies_to_its_own_commands_from_any_bot() {
    let reply = bot_reply(500, Some(metadata(2, 7, 42)));
    let other_bot_same_invoker = bot_reply(501, Some(metadata(2, 8, 42)));
    let other_invoker = bot_reply(500, Some(metadata(2, 9, 43)));
    let (_, invoker) = command_invoker(&reply).unwrap();
    let actor = Actor { user: invoker, via_bot: Some(&reply.author) };

    assert!(actor.caused(&reply));
    assert!(actor.caused(&other_bot_same_invoker));
    assert!(!actor.caused(&other_invoker));
    // 発言者本人として見た場合、bot の返信は本人の発言ではない
    assert!(!Actor { user: invoker, via_bot: None }.caused(&reply));
}

#[test]
fn one_slash_command_is_counted_once() {
    let seen = Mutex::new(HashMap::new());
    let now = Instant::now();
    let id = serenity::InteractionId::new(7);
    assert!(first_sight_of_interaction(&seen, id, now));
    // 同じコマンドへのフォローアップ
    assert!(!first_sight_of_interaction(&seen, id, now + Duration::from_secs(1)));
    // 窓を過ぎた古い記録は捨てる
    let later = now + MULTI_CH_WINDOW.max(SAME_CH_WINDOW) + Duration::from_secs(1);
    assert!(first_sight_of_interaction(&seen, serenity::InteractionId::new(8), later));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn commands_in_three_channels_trip_multi_channel_for_the_invoker() {
    let state = test_state();
    let invoker = serenity::UserId::new(42);
    let now = Instant::now();
    let kinds: Vec<_> = (0..3)
        .map(|i| detect_rate_spam(&state, invoker, serenity::ChannelId::new(10 + i), now + Duration::from_secs(i)))
        .collect();
    assert_eq!(kinds, [SpamKind::None, SpamKind::None, SpamKind::MultiChannel]);
}

/// 人間（id 42）の発言。kind 19（リプライ）なら referenced_type の種類のメッセージへのリプライにする
fn human_msg(id: u64, content: &str, sticker: bool, referenced_type: Option<u8>) -> serenity::Message {
    let mut msg = serde_json::json!({
        "id": id.to_string(), "channel_id": "10", "author": user_json(42, false),
        "content": content, "timestamp": "2026-10-28T00:00:00.000000+00:00",
        "edited_timestamp": null, "tts": false, "mention_everyone": false,
        "mentions": [], "mention_roles": [], "attachments": [], "embeds": [],
        "pinned": false, "type": if referenced_type.is_some() { 19 } else { 0 },
    });
    if sticker {
        msg["sticker_items"] = serde_json::json!([{ "id": "749054660769218631", "name": "Wave", "format_type": 3 }]);
    }
    if let Some(kind) = referenced_type {
        msg["message_reference"] = serde_json::json!({ "message_id": "1", "channel_id": "10" });
        msg["referenced_message"] = serde_json::json!({
            "id": "1", "channel_id": "10", "author": user_json(43, false),
            "content": "", "timestamp": "2026-10-28T00:00:00.000000+00:00",
            "edited_timestamp": null, "tts": false, "mention_everyone": false,
            "mentions": [], "mention_roles": [], "attachments": [], "embeds": [],
            "pinned": false, "type": kind,
        });
    }
    serde_json::from_value(msg).expect("test message must deserialize")
}

#[test]
fn welcome_wave_is_a_sticker_only_reply_to_a_join_message() {
    assert!(is_welcome_wave(&human_msg(1, "", true, Some(7))));
    // 入室メッセージ以外へのスタンプ返信、本文つき、スタンプなし、リプライ先不明は挨拶ではない
    assert!(!is_welcome_wave(&human_msg(1, "", true, Some(0))));
    assert!(!is_welcome_wave(&human_msg(1, "ようこそ", true, Some(7))));
    assert!(!is_welcome_wave(&human_msg(1, "ようこそ", false, Some(7))));
    assert!(!is_welcome_wave(&human_msg(1, "", true, None)));
}

#[test]
fn welcome_waves_do_not_trip_same_channel() {
    let state = test_state();
    let waves: Vec<_> = (0..SAME_CH_THRESHOLD as u64 + 2)
        .map(|i| detect(&state, &human_msg(100 + i, "", true, Some(7))).kind)
        .collect();
    assert!(waves.iter().all(|k| *k == SpamKind::None));

    // 普通の連投はこれまで通り検知する
    let kinds: Vec<_> = (0..SAME_CH_THRESHOLD as u64)
        .map(|i| detect(&state, &human_msg(200 + i, "連投", false, None)).kind)
        .collect();
    assert_eq!(kinds.last(), Some(&SpamKind::SameChannel));
}

#[test]
fn neutralize_backticks_prevents_code_block_breakout() {
    let echoed = neutralize_backticks("x```\n@everyone <@&1> `y`");
    assert!(!echoed.contains('`'));
    assert_eq!(echoed, "x\u{02CB}\u{02CB}\u{02CB}\n@everyone <@&1> \u{02CB}y\u{02CB}");
}
