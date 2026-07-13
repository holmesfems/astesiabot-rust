use super::AppState;
use crate::recruit::{format, RecruitEngine};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Python の OCRRawData に対応。
#[derive(Deserialize)]
pub(super) struct OcrRawData {
    text: String,
    #[serde(rename = "pickupOperators", default)]
    pickup_operators: Option<Vec<String>>,
}

/// Python の TagReplyData に対応。
#[derive(Serialize)]
pub(super) struct TagReplyData {
    title: String,
    reply: String,
}

/// Web API の doRecruitment と完全一致する処理。
/// OCR生テキスト → タグ抽出 → 計算 → title/reply(responseForAI) の詰め替え。
/// recruit は計算だけを担い、API レスポンス表現への詰め替えは api 側の責務とする。
fn build_tag_reply(engine: &RecruitEngine, ocr_text: &str, pickup: Option<&[String]>) -> TagReplyData {
    let matched = engine.matcher.match_tag(ocr_text);

    // isEmpty チェック（Python: matchTag.isEmpty()）
    if matched.matches.is_empty() {
        return TagReplyData {
            title: "エラー".to_string(),
            reply: "タグがありません".to_string(),
        };
    }

    // matches を Vec 化。8個超なら先頭8個に切り詰め（Python の list(matches)[:8]）
    // 注意: 8個超は OCR 大誤爆時のみ。順序は Python set と一致しない（許容）。
    let mut matches: Vec<String> = matched.matches.into_iter().collect();
    if matches.len() > 8 {
        matches.truncate(8);
    }

    let is_global = matched.is_global;
    let results = engine.data.calculate(&matches, is_global, 4, pickup);

    let sorted_input = engine.data.normalize_names(&matches);
    let title = format::make_title(&sorted_input, is_global, true);

    let reply = if results.is_empty() {
        "★4以上になる組み合わせはありません".to_string()
    } else {
        format::response_for_ai(results)
    };

    TagReplyData { title, reply }
}

pub async fn do_recruitment(
    State(state): State<Arc<AppState>>,
    Json(data): Json<OcrRawData>,
) -> Json<TagReplyData> {
    let pickup = data.pickup_operators.as_deref();
    Json(build_tag_reply(&state.recruit, &data.text, pickup))
}
