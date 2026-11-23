use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::recruit::calc::MAX_TAG_COUNT;
use crate::engine::recruit::{format, RecruitEngine};

/// build_embed_reply の結果。
pub struct OcrEmbedResult {
    pub reply: EmbedReply,
    /// OCRで拾ったタグの生の個数が5個ちょうどでない。true の場合、呼び出し側（ocr_flow）が
    /// リプライ誘導のメッセージを追加送信する。
    pub tag_count_illegal: bool,
}

/// OCR生テキスト → タグ抽出 → 計算 → EmbedReply（bot の embed 表示用）。
/// recruit は計算だけを担い、Discord embed への詰め替えは bot 側（koukai_kyujin）の責務とする。
pub fn build_embed_reply(engine: &RecruitEngine, ocr_text: &str) -> OcrEmbedResult {
    println!("[recruit OCR]: {} lines", ocr_text.lines().count());
    for (i, line) in ocr_text.lines().enumerate() {
        println!("  [{i:>2}] {line:?}");
    }
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
    // 切り詰め前の生の個数で判定する（8個超に切り詰めた後の個数は使わない）。
    let tag_count_illegal = matched.matches.len() != 5;
    let matches: Vec<String> = matched.matches.into_iter().collect();
    reply_from_matches(engine, matches, matched.is_global, tag_count_illegal)
}

/// ローカル OCR が読んだタグ（tagList.json の正式名）から EmbedReply を作る。
/// ローカル OCR は日本語表示（グローバル版）の画面だけを読むので is_global = true。
/// 5 個ちょうどなら誘導メッセージは不要（tag_count_illegal = false）。
pub fn build_embed_reply_from_tags(engine: &RecruitEngine, tags: &[String]) -> OcrEmbedResult {
    println!("[recruit OCR]: local tags {tags:?}");
    reply_from_matches(engine, tags.to_vec(), true, tags.len() != 5)
}

/// タグ集合 → 計算 → EmbedReply（OCR テキスト経由でもローカル OCR 経由でも共通の後半）。
fn reply_from_matches(
    engine: &RecruitEngine,
    mut matches: Vec<String>,
    is_global: bool,
    tag_count_illegal: bool,
) -> OcrEmbedResult {
    if matches.len() > MAX_TAG_COUNT {
        matches.truncate(MAX_TAG_COUNT);
    }
    let data = engine.data();
    let results = data.calculate(&matches, is_global, 4, None);
    // embedの色分け用: 各組み合わせの確定レアリティ（"-> ★n"の表示に対応するmin_star）
    // のうち最高のもの（該当結果が無ければ0）。star_setには確定レア以外の高レア
    // オペレーターも含まれてしまう（例: ★4確定タグでも★5が混在）ため、それを
    // 使うと実際の表示内容より高いレアリティ色になってしまう。
    let max_star = results.iter().map(|m| m.min_star).max().unwrap_or(0);
    let sorted_input = data.normalize_names(&matches);
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
            msg_type: MsgType::Rarity(max_star),
            reply_marker: None,
        },
        tag_count_illegal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_tags_matches_ocr_text_path() {
        let engine = RecruitEngine::load_from_seed();
        let tags: Vec<String> = ["近距離", "上級エリート", "牽制", "治療", "火力"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let from_tags = build_embed_reply_from_tags(&engine, &tags);
        assert!(!from_tags.tag_count_illegal);
        assert!(!from_tags.reply.chunks.is_empty());
        // 同じタグを OCR テキストで渡した場合と同じ計算結果になる
        let from_text = build_embed_reply(
            &engine,
            &tags.join(
                "
",
            ),
        );
        assert_eq!(from_tags.reply.title, from_text.reply.title);
        assert_eq!(from_tags.reply.chunks, from_text.reply.chunks);
        assert!(!from_text.tag_count_illegal);
    }

    #[test]
    fn from_tags_flags_illegal_count() {
        let engine = RecruitEngine::load_from_seed();
        let tags: Vec<String> = ["近距離", "牽制", "治療", "火力"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(build_embed_reply_from_tags(&engine, &tags).tag_count_illegal);
    }
}
