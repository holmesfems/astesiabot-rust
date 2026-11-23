use super::calc::MatchItem;
use std::cmp::Reverse;
use std::collections::BTreeSet;

/// 並び順（display_chunks と response_for_ai で共通）。安定ソートの重ねがけで、最後が最優先。
fn sort_items(items: &mut [MatchItem]) {
    items.sort_by_key(|a| Reverse(a.combo.len()));
    items.sort_by_key(|a| a.operators.len());
    items.sort_by(|a, b| {
        let ma = a.star_set.iter().max().copied().unwrap_or(0);
        let mb = b.star_set.iter().max().copied().unwrap_or(0);
        mb.cmp(&ma)
    });
    items.sort_by_key(|a| Reverse(a.min_star));
    items.sort_by_key(|a| Reverse(a.contains_pickup));
}

fn star_set_str(set: &BTreeSet<u8>) -> String {
    set.iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// 各組み合わせを1チャンク文字列にして Vec で返す。
/// bot の embed で使う。
/// 1チャンクは「タグ -> ★minStar」の後にコードブロックで「★星名前,...」を並べた形式。
pub fn display_chunks(mut items: Vec<MatchItem>) -> Vec<String> {
    sort_items(&mut items);
    let mut chunks = Vec::new();
    for it in &items {
        let key_msg = it.combo.join("+");
        let value_msg = it
            .operators
            .iter()
            .map(|o| format!("★{}{}", o.stars, o.name))
            .collect::<Vec<_>>()
            .join(",");
        let chunk = format!("{key_msg} -> ★{}```\n{value_msg}```\n", it.min_star);
        chunks.push(chunk);
    }
    chunks
}

/// AI 向けのチャンクを区切り無しで連結した文字列を返す。
pub fn response_for_ai(mut items: Vec<MatchItem>) -> String {
    sort_items(&mut items);
    let mut out = String::new();
    for it in &items {
        let key_msg = it.combo.join("+");
        let mut chunk = format!("{key_msg} ->★{}", star_set_str(&it.star_set));

        if it.min_star >= 5 || it.min_star == 1 {
            let names: Vec<String> = it.operators.iter().map(|o| o.name.clone()).collect();
            chunk.push('\n');
            chunk.push_str(&names.join(", "));
        } else if it.contains_pickup {
            // ピックアップの特別表記
            let mut pu_stars: BTreeSet<u8> = BTreeSet::new();
            for o in &it.operators {
                if it.pickup_target.contains(&o.name) {
                    pu_stars.insert(o.stars);
                }
            }
            let mut pu_items = Vec::new();
            for star in &pu_stars {
                let pu_ops: Vec<String> = it
                    .operators
                    .iter()
                    .filter(|o| it.pickup_target.contains(&o.name) && o.stars == *star)
                    .map(|o| o.name.clone())
                    .collect();
                let all_count = it.operators.iter().filter(|o| o.stars == *star).count();
                let other_count = all_count - pu_ops.len();
                let mut s = pu_ops.join(",");
                if other_count > 0 {
                    s.push_str(&format!("(他★{star}が{other_count}人)"));
                }
                pu_items.push(s);
            }
            chunk.push('\n');
            chunk.push_str(&pu_items.join(","));
        }
        // ★4で非ピックアップは名前行なし（星セットのみ）
        chunk.push('\n');
        out.push_str(&chunk);
    }
    out
}

/// title 生成。
/// sorted_input は正規化済み（定義順ソート済み）を渡す。
pub fn make_title(sorted_input: &[String], is_global: bool, show_tag_loss: bool) -> String {
    let mut title = sorted_input.join(" ");
    if !is_global {
        title.push_str(" (大陸版)");
    }
    if show_tag_loss && sorted_input.len() < 5 {
        title.push_str("(タグ不足)");
    }
    if show_tag_loss && sorted_input.len() > 5 {
        title.push_str("(タグ過多)");
    }
    title
}

// ---- 分割ユーティリティ ----
// 長さは chars() で数える（String::len() はバイト数）。

/// limit 超のテキストを分割する。
pub fn chunk_text(text: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n <= limit {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < n {
        let end = std::cmp::min(n, start + limit);
        let split_pos = if end < n {
            let slice = &chars[start..end];
            let nl = slice.iter().rposition(|&c| c == '\n').map(|p| start + p);
            let sp = match nl {
                Some(p) => Some(p),
                None => slice.iter().rposition(|&c| c == ' ').map(|p| start + p),
            };
            match sp {
                Some(p) if p > start => p,
                _ => end,
            }
        } else {
            end
        };
        chunks.push(chars[start..split_pos].iter().collect());
        start = split_pos;
        if start < n && (chars[start] == '\n' || chars[start] == ' ') {
            start += 1;
        }
    }
    chunks
}

/// chunk 群を max_length 以内に結合する。
pub fn arrangement_chunks(msg_list: &[String], max_length: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    for item in msg_list {
        if chunks.is_empty() {
            chunks.push(item.clone());
        } else {
            let last_len = chunks.last().unwrap().chars().count();
            if last_len + item.chars().count() <= max_length {
                chunks.last_mut().unwrap().push_str(item);
            } else {
                let mut safe = chunk_text(item, max_length);
                chunks.append(&mut safe);
            }
        }
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::recruit::calc::OperatorRef;

    fn item(id: &str, combo_len: usize, min_star: u8, stars: &[u8], pickup: bool) -> MatchItem {
        MatchItem {
            combo: vec![id.to_string(); combo_len],
            min_star,
            operators: stars
                .iter()
                .enumerate()
                .map(|(i, &s)| OperatorRef {
                    name: format!("{id}{i}"),
                    stars: s,
                })
                .collect(),
            star_set: stars.iter().copied().collect(),
            contains_pickup: pickup,
            pickup_target: Vec::new(),
        }
    }

    fn order(mut items: Vec<MatchItem>) -> Vec<String> {
        sort_items(&mut items);
        items.iter().map(|i| i.combo[0].clone()).collect()
    }

    #[test]
    fn sort_pickup_beats_min_star() {
        let v = vec![item("A", 1, 5, &[5], false), item("B", 1, 3, &[3], true)];
        assert_eq!(order(v), vec!["B", "A"]);
    }

    #[test]
    fn sort_min_star_desc_beats_max_star() {
        let v = vec![
            item("A", 1, 3, &[3, 5], false),
            item("B", 1, 4, &[4], false),
        ];
        assert_eq!(order(v), vec!["B", "A"]);
    }

    #[test]
    fn sort_max_star_desc_beats_operator_count() {
        let v = vec![
            item("A", 1, 4, &[4], false),
            item("B", 1, 4, &[4, 5, 5], false),
        ];
        assert_eq!(order(v), vec!["B", "A"]);
    }

    #[test]
    fn sort_fewer_operators_first() {
        let v = vec![
            item("A", 1, 4, &[4, 4, 4], false),
            item("B", 1, 4, &[4], false),
        ];
        assert_eq!(order(v), vec!["B", "A"]);
    }

    #[test]
    fn sort_longer_combo_first() {
        let v = vec![item("A", 1, 4, &[4], false), item("B", 2, 4, &[4], false)];
        assert_eq!(order(v), vec!["B", "A"]);
    }

    #[test]
    fn sort_ties_keep_input_order() {
        let v = vec![
            item("A", 1, 4, &[4], false),
            item("B", 1, 4, &[4], false),
            item("C", 1, 4, &[4], false),
        ];
        assert_eq!(order(v), vec!["A", "B", "C"]);
    }

    #[test]
    fn response_for_ai_lists_star_set_ascending() {
        let mut it = item("A", 1, 4, &[4], false);
        it.star_set = [5u8, 4].into_iter().collect();
        assert_eq!(response_for_ai(vec![it]), "A ->★4,5\n");
    }

    #[test]
    fn chunk_text_returns_short_text_as_is() {
        assert_eq!(chunk_text("abc", 3), vec!["abc"]);
    }

    #[test]
    fn chunk_text_counts_chars_not_bytes() {
        assert_eq!(chunk_text("あいうえお", 3), vec!["あいう", "えお"]);
    }

    #[test]
    fn chunk_text_prefers_newline_and_drops_separator() {
        assert_eq!(chunk_text("あい\nうえお", 4), vec!["あい", "うえお"]);
        // 後ろに空白があっても改行を優先する
        assert_eq!(chunk_text("aa bb\nc dd", 8), vec!["aa bb", "c dd"]);
    }

    #[test]
    fn chunk_text_falls_back_to_space_then_hard_cut() {
        assert_eq!(chunk_text("abc def", 5), vec!["abc", "def"]);
        let text = "あ".repeat(10);
        let lens: Vec<usize> = chunk_text(&text, 4)
            .iter()
            .map(|c| c.chars().count())
            .collect();
        assert_eq!(lens, vec![4, 4, 2]);
    }

    #[test]
    fn arrangement_chunks_joins_while_within_max_length() {
        let list: Vec<String> = ["aaa", "bbb", "ccc"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(arrangement_chunks(&list, 6), vec!["aaabbb", "ccc"]);
        // 文字数で数える（バイト数なら結合されない）
        let jp: Vec<String> = ["あああ", "いいい"].iter().map(|s| s.to_string()).collect();
        assert_eq!(arrangement_chunks(&jp, 6), vec!["あああいいい"]);
    }

    #[test]
    fn arrangement_chunks_splits_oversized_item() {
        let list: Vec<String> = ["a", "あいうえお"].iter().map(|s| s.to_string()).collect();
        assert_eq!(arrangement_chunks(&list, 3), vec!["a", "あいう", "えお"]);
    }

    fn named(combo: &[&str], min_star: u8, ops: &[(&str, u8)], pickup: &[&str]) -> MatchItem {
        MatchItem {
            combo: combo.iter().map(|s| s.to_string()).collect(),
            min_star,
            operators: ops
                .iter()
                .map(|&(n, s)| OperatorRef {
                    name: n.to_string(),
                    stars: s,
                })
                .collect(),
            star_set: ops.iter().map(|&(_, s)| s).collect(),
            contains_pickup: !pickup.is_empty(),
            pickup_target: pickup.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn sample_items() -> Vec<MatchItem> {
        vec![
            named(&["上級エリート"], 6, &[("アーミヤ", 6), ("ロスモンティス", 6)], &[]),
            named(&["近距離", "治療"], 4, &[("ホシグマ", 5), ("ミッドナイト", 4), ("ガヴィル", 4)], &[]),
            named(
                &["遠距離", "火力"],
                3,
                &[("ケルシー", 6), ("エクシア", 5), ("ラップランド", 5), ("カーディ", 3), ("ヨモギ", 3)],
                &["エクシア"],
            ),
            named(&["ロボット"], 1, &[("Lancet-2", 1), ("Castle-3", 1)], &[]),
        ]
    }

    #[test]
    fn snapshot_display_chunks() {
        let text = display_chunks(sample_items()).join("
----- chunk -----
");
        insta::assert_snapshot!("recruit_display_chunks", text);
    }

    #[test]
    fn snapshot_response_for_ai() {
        insta::assert_snapshot!("recruit_response_for_ai", response_for_ai(sample_items()));
    }

    #[test]
    fn snapshot_make_title() {
        let tags: Vec<String> = ["上級エリート", "近距離", "治療"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let six: Vec<String> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let text = [
            make_title(&tags, true, true),
            make_title(&tags, false, true),
            make_title(&tags, false, false),
            make_title(&six, true, true),
            make_title(&six, true, false),
        ]
        .join("
");
        insta::assert_snapshot!("recruit_make_title", text);
    }
}
