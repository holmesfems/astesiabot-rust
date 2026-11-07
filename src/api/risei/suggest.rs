//! 間違った入力への候補（did_you_mean）。

use crate::engine::risei_calculator_engine::{RiseiCalculatorEngine, Server};
use axum::http::StatusCode;
use super::spec::{STAGES, EVENTS};
use super::params::ApiError;

const MAX_SUGGESTIONS: usize = 5;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum StageKind {
    Main,
    Event,
}

/// ステージが見つからなかったときの404。候補は常に大陸版(最も先行していてステージ数が
/// 最多。Discordのオートコンプリートと同じ)から出す。恒常/イベントを取り違えている場合は
/// 正しい方のエンドポイントを`hint`で案内する。
pub(super) async fn stage_not_found(engine: &RiseiCalculatorEngine, stage: &str, kind: StageKind, msg: String) -> ApiError {
    let snapshot = engine.snapshot(Server::Mainland).await;
    let info = &snapshot.stage_info;
    let (own, other) = match kind {
        StageKind::Main => (&info.main_code_to_stage, &info.event_code_to_stage),
        StageKind::Event => (&info.event_code_to_stage, &info.main_code_to_stage),
    };
    let mut error = ApiError::new(StatusCode::NOT_FOUND, msg)
        .with("did_you_mean", suggest(stage, own.keys().map(String::as_str)));
    let found_in_other = match kind {
        StageKind::Main => !snapshot.search_event_stage(stage).is_empty(),
        StageKind::Event => !snapshot.search_main_stage(stage).is_empty(),
    };
    if found_in_other {
        let (label, path) = match kind {
            StageKind::Main => ("イベントステージ", EVENTS.path),
            StageKind::Event => ("恒常ステージ", STAGES.path),
        };
        error = error.with("hint", format!("「{stage}」は{label}にあります: {path}?stage={stage}"));
    } else if !other.is_empty() {
        let other_suggestions = suggest(stage, other.keys().map(String::as_str));
        if !other_suggestions.is_empty() {
            let label = match kind {
                StageKind::Main => "did_you_mean_event",
                StageKind::Event => "did_you_mean_main",
            };
            error = error.with(label, other_suggestions);
        }
    }
    error
}

/// 近い候補を最大[`MAX_SUGGESTIONS`]件返す。大文字小文字を無視した前方一致を最優先し、
/// 残りは編集距離(入力長の半分か2の大きい方まで)の近い順。
///
/// 入力は認証なしのクエリ/パスそのもので長さに上限が無いため、文字数の差(編集距離の下限)が
/// 閾値を超える候補は編集距離を計算せずに落とす。結果は変わらず、計算量は入力長ではなく
/// 候補側の長さで抑えられる(長大な入力でワーカースレッドを占有させないため)。
pub(super) fn suggest<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let query = input.to_lowercase();
    let query_len = query.chars().count();
    let threshold = (query_len / 2).max(2);
    let mut scored: Vec<(usize, usize, &str)> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let lower = candidate.to_lowercase();
            let distance = if !query.is_empty() && lower.starts_with(&query) {
                0
            } else if query_len.abs_diff(lower.chars().count()) > threshold {
                return None;
            } else {
                levenshtein(&query, &lower)
            };
            (distance <= threshold).then_some((distance, candidate.chars().count(), candidate))
        })
        .collect();
    scored.sort();
    scored.dedup_by(|a, b| a.2 == b.2);
    scored.into_iter().take(MAX_SUGGESTIONS).map(|(_, _, c)| c.to_string()).collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}
