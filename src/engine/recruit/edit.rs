use super::calc::{RecruitData, MAX_TAG_COUNT};
use std::collections::HashSet;

/// embedタイトルから逆算したタグ集合。
pub struct ParsedTitle {
    pub tags: Vec<String>,
    pub is_global: bool,
}

/// リプライ編集で起き得るエラー。
/// Python の msgForOCRReply が返すエラーメッセージに対応する。
#[derive(Debug)]
pub enum EditError {
    /// コマンド中に tagList.json に無いタグ名が含まれていた。
    UnknownTags(Vec<String>),
    /// 編集後のタグ数が MAX_TAG_COUNT を超えた。
    TooManyTags(usize),
}

/// Python の msgForOCRReply 冒頭（タイトル→タグ復元部分）に対応。
/// `format::make_title` の出力形式（タグ名をスペース区切りで結合し、
/// 大陸版/タグ不足/タグ過多の装飾を付加したもの）を逆変換する。
/// タイトルに未知のトークンが1つでも混ざっていれば None（呼び出し側は無視する）。
pub fn parse_title(data: &RecruitData, title: &str) -> Option<ParsedTitle> {
    let mut s = title.to_string();
    let is_global = if s.contains("(大陸版)") {
        s = s.replace("(大陸版)", "");
        false
    } else {
        true
    };
    // (タグ不足)/(タグ過多) はスペース無しで直前のタグ名に結合され得るため、
    // 空白分割の前に取り除く。
    s = s.replace("(タグ不足)", "").replace("(タグ過多)", "");

    let tags: Vec<String> = s.split_whitespace().map(str::to_string).collect();
    if tags.is_empty() || tags.iter().any(|t| !data.is_known_tag(t)) {
        return None;
    }
    Some(ParsedTitle { tags, is_global })
}

/// タグの略称→正式名（Python の abbreviations）。
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("上エリ", "上級エリート"),
    ("エリ", "エリート"),
    ("COST", "COST回復"),
    ("コスト", "COST回復"),
    ("コスト回復", "COST回復"),
];

/// Python の formatToTags。末尾の「タイプ」除去→大文字化→略称展開。
fn format_to_tag(command: &str) -> String {
    let normalized = command.replace("タイプ", "").to_uppercase();
    ABBREVIATIONS
        .iter()
        .find(|(k, _)| *k == normalized)
        .map(|(_, v)| v.to_string())
        .unwrap_or(normalized)
}

/// Python の `re.split(r"(?:->)|→", command)` 相当。
/// "→" を "->" に置き換えてから分割することで、同じ分割位置を再現する。
fn split_arrow(command: &str) -> Vec<String> {
    command.replace('→', "->").split("->").map(str::to_string).collect()
}

/// Python の msgForOCRReply のコマンド適用ループに対応。
/// existing（parse_title で復元した既存タグ）にリプライ本文のコマンドを適用し、
/// 編集後のタグ集合を返す。
pub fn apply_edit_commands(
    data: &RecruitData,
    existing: &[String],
    command_text: &str,
) -> Result<Vec<String>, EditError> {
    let mut result_tags: Vec<String> = existing.to_vec();

    for command in command_text.split_whitespace() {
        let command_tags: Vec<String> =
            split_arrow(command).iter().map(|t| format_to_tag(t)).collect();

        let illegal: Vec<String> = command_tags
            .iter()
            .filter(|t| !t.trim().is_empty() && !data.is_known_tag(t))
            .cloned()
            .collect();
        if !illegal.is_empty() {
            return Err(EditError::UnknownTags(illegal));
        }

        match command_tags.as_slice() {
            [single] => result_tags.push(single.clone()),
            [old, new] => {
                for item in result_tags.iter_mut() {
                    if item == old {
                        *item = new.clone();
                    }
                }
            }
            _ => {} // 3要素以上は Python 版同様、意図的なチェック無しで素通り
        }
    }

    result_tags.retain(|t| !t.trim().is_empty());
    let mut seen = HashSet::new();
    result_tags.retain(|t| seen.insert(t.clone()));

    if result_tags.len() > MAX_TAG_COUNT {
        return Err(EditError::TooManyTags(result_tags.len()));
    }
    Ok(result_tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::recruit::model::TagList;

    fn make_data() -> RecruitData {
        RecruitData::for_test(TagList {
            elite_tags: vec!["エリート".to_string(), "上級エリート".to_string()],
            job_tags: vec!["前衛".to_string(), "狙撃".to_string()],
            position_tags: vec!["近距離".to_string()],
            other_tags: vec!["COST回復".to_string(), "特殊".to_string()],
        })
    }

    #[test]
    fn parse_title_strips_mainland_and_tag_loss() {
        let data = make_data();
        let parsed = parse_title(&data, "前衛 狙撃 COST回復 (大陸版)(タグ不足)").unwrap();
        assert!(!parsed.is_global);
        assert_eq!(parsed.tags, vec!["前衛", "狙撃", "COST回復"]);
    }

    #[test]
    fn parse_title_rejects_unknown_token() {
        let data = make_data();
        assert!(parse_title(&data, "前衛 未知タグ").is_none());
    }

    #[test]
    fn apply_edit_commands_adds_single_tag() {
        let data = make_data();
        let result =
            apply_edit_commands(&data, &["前衛".to_string()], "狙撃タイプ").unwrap();
        assert_eq!(result, vec!["前衛", "狙撃"]);
    }

    #[test]
    fn apply_edit_commands_replaces_with_arrow() {
        let data = make_data();
        let result = apply_edit_commands(
            &data,
            &["前衛".to_string(), "狙撃".to_string()],
            "前衛->近距離",
        )
        .unwrap();
        assert_eq!(result, vec!["近距離", "狙撃"]);
    }

    #[test]
    fn apply_edit_commands_expands_abbreviation() {
        let data = make_data();
        let result = apply_edit_commands(&data, &[], "上エリ").unwrap();
        assert_eq!(result, vec!["上級エリート"]);
    }

    #[test]
    fn apply_edit_commands_rejects_unknown_tag() {
        let data = make_data();
        let err = apply_edit_commands(&data, &[], "存在しないタグ").unwrap_err();
        assert!(matches!(err, EditError::UnknownTags(_)));
    }

    #[test]
    fn apply_edit_commands_rejects_too_many_tags() {
        let data = RecruitData::for_test(TagList {
            elite_tags: vec!["エリート".to_string(), "上級エリート".to_string()],
            job_tags: vec!["前衛".to_string(), "狙撃".to_string()],
            position_tags: vec!["近距離".to_string(), "遠距離".to_string()],
            other_tags: vec!["COST回復".to_string(), "特殊".to_string(), "医療".to_string()],
        });
        let existing: Vec<String> = vec![
            "前衛".to_string(),
            "狙撃".to_string(),
            "近距離".to_string(),
            "遠距離".to_string(),
            "COST回復".to_string(),
            "特殊".to_string(),
            "エリート".to_string(),
            "上級エリート".to_string(),
        ];
        assert_eq!(existing.len(), MAX_TAG_COUNT);
        // ちょうど上限(8件)まではOK
        let ok = apply_edit_commands(&data, &existing, "").unwrap();
        assert_eq!(ok.len(), MAX_TAG_COUNT);
        // 新規タグを1件追加して9件になるとNG
        let err = apply_edit_commands(&data, &existing, "医療タイプ").unwrap_err();
        assert!(matches!(err, EditError::TooManyTags(9)));
    }
}
