use super::calc::MatchItem;
use std::collections::BTreeSet;

/// 5段ソート（chunks と responseForAI で共通）。
/// Python の searchMapToStringChunks と同じ順序を、安定ソートの重ねがけで再現する。
/// 適用順（最後が最優先）:
///   keyLen desc → valueLen asc → maxstar desc → minstar desc → containsPickup desc
fn sort_items(items: &mut [MatchItem]) {
    items.sort_by(|a, b| b.combo.len().cmp(&a.combo.len())); // keyLen desc
    items.sort_by(|a, b| a.operators.len().cmp(&b.operators.len())); // valueLen asc
    items.sort_by(|a, b| {
        let ma = a.star_set.iter().max().copied().unwrap_or(0);
        let mb = b.star_set.iter().max().copied().unwrap_or(0);
        mb.cmp(&ma) // maxstar desc
    });
    items.sort_by(|a, b| b.min_star.cmp(&a.min_star)); // minstar desc
    items.sort_by(|a, b| b.contains_pickup.cmp(&a.contains_pickup)); // containsPickup desc
}

fn star_set_str(set: &BTreeSet<u8>) -> String {
    set.iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// embbedContents 相当。各組み合わせを1チャンク文字列にして Vec で返す。
/// bot の embed で使う。
/// Python: key + " -> ★{minStar}" + "```\n" + "★{star}{name},..." + "```\n"
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

/// Python の responseForAI（"".join(aiChunks)）を完全再現する。
pub fn response_for_ai(mut items: Vec<MatchItem>) -> String {
    sort_items(&mut items);
    let mut out = String::new();
    for it in &items {
        let key_msg = it.combo.join("+");
        let mut chunk = format!("{key_msg} ->★{}", star_set_str(&it.star_set));

        if it.min_star >= 5 || it.min_star == 1 {
            // 名前を「, 」区切りで列挙
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

/// title 生成（Python の recruitDoProcess のタイトル部分）。
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

// ---- 分割ユーティリティ（Python の rcReply.py 相当）----
// 注意: Python の len() は文字数。Rust の String::len() はバイト数なので、
// 日本語で挙動がズレないよう chars() ベースで文字数を数える。

/// Python の chunk_text。limit超のテキストを改行/空白優先で分割。
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

/// Python の arrangementChunks。chunk群を maxLength に収まるよう結合。
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
