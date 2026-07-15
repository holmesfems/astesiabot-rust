use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::recruit::calc::MAX_TAG_COUNT;
use crate::engine::recruit::{format, RecruitEngine};

/// OCR生テキスト → タグ抽出 → 計算 → EmbedReply（bot の embed 表示用）。
/// Python bot の embed 表示に対応。recruit は計算だけを担い、
/// Discord embed への詰め替えは bot 側（koukai_kyujin）の責務とする。
pub fn build_embed_reply(engine: &RecruitEngine, ocr_text: &str) -> EmbedReply {
    let matched = engine.matcher.match_tag(ocr_text);
    if matched.matches.is_empty() {
        return EmbedReply {
            title: "エラー".to_string(),
            chunks: vec!["タグがありません".to_string()],
            msg_type: MsgType::Err,
        };
    }
    let mut matches: Vec<String> = matched.matches.into_iter().collect();
    if matches.len() > MAX_TAG_COUNT {
        matches.truncate(MAX_TAG_COUNT);
    }
    let is_global = matched.is_global;
    let results = engine.data.calculate(&matches, is_global, 4, None);
    let sorted_input = engine.data.normalize_names(&matches);
    let title = format::make_title(&sorted_input, is_global, true);
    let chunks = if results.is_empty() {
        vec!["★4以上になる組み合わせはありません".to_string()]
    } else {
        format::display_chunks(results)
    };
    EmbedReply {
        title,
        chunks,
        msg_type: MsgType::Ok,
    }
}
