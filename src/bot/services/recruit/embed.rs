use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::recruit::calc::MAX_TAG_COUNT;
use crate::engine::recruit::{format, RecruitEngine};

/// build_embed_reply の結果。
pub struct OcrEmbedResult {
    pub reply: EmbedReply,
    /// Python の `tagMatch.isIllegal()` 相当（OCRで拾ったタグの生の個数が5個
    /// ちょうどでない）。true の場合、呼び出し側（ocr_flow）がリプライ誘導の
    /// メッセージを追加送信する。
    pub tag_count_illegal: bool,
}

/// OCR生テキスト → タグ抽出 → 計算 → EmbedReply（bot の embed 表示用）。
/// Python bot の embed 表示に対応。recruit は計算だけを担い、
/// Discord embed への詰め替えは bot 側（koukai_kyujin）の責務とする。
pub fn build_embed_reply(engine: &RecruitEngine, ocr_text: &str) -> OcrEmbedResult {
    let matched = engine.matcher.match_tag(ocr_text);
    if matched.matches.is_empty() {
        return OcrEmbedResult {
            reply: EmbedReply {
                title: "エラー".to_string(),
                chunks: vec!["タグがありません".to_string()],
                msg_type: MsgType::Err,
                reply_marker: None,
            },
            tag_count_illegal: false,
        };
    }
    // truncate 前の生の個数で判定する（Python の isIllegal は truncate 相当の処理を持たない）。
    let tag_count_illegal = matched.matches.len() != 5;
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
    OcrEmbedResult {
        reply: EmbedReply {
            title,
            chunks,
            msg_type: MsgType::Ok,
            reply_marker: None,
        },
        tag_count_illegal,
    }
}
