use fancy_regex::Regex;
use std::fs;
use std::path::Path;

const SOURCE_DIRS: &[&str] = &["src", "tests"];
const SOURCE_EXTENSIONS: &[&str] = &["rs", "js", "mjs"];

fn collect_sources(root: &Path, rel: &str, out: &mut Vec<String>) {
    for entry in fs::read_dir(root.join(rel)).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = format!("{rel}/{name}");
        if entry.file_type().unwrap().is_dir() {
            collect_sources(root, &child, out);
        } else if !name.ends_with(".min.js")
            && Path::new(&name)
                .extension()
                .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext.to_str().unwrap()))
        {
            out.push(child);
        }
    }
}

fn comment_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("/*") || trimmed.starts_with('*') {
        return Some(trimmed);
    }
    let mut search_from = 0;
    while let Some(pos) = line[search_from..].find("//") {
        let at = search_from + pos;
        let before = &line[..at];
        let quotes = before
            .match_indices('"')
            .filter(|&(i, _)| !before[..i].ends_with('\\'));
        let inside_string = quotes.count() % 2 == 1;
        if !before.ends_with(':') && !inside_string {
            return Some(&line[at..]);
        }
        search_from = at + 2;
    }
    None
}

fn without_code_spans(text: &str) -> String {
    text.split('`').step_by(2).collect::<Vec<_>>().join(" ")
}

fn history_pattern() -> Regex {
    Regex::new(
        r"20\d{2}-\d{2}|20\d{2}年|(?<![0-9A-Za-z&])#\d{1,4}(?![0-9A-Za-z])|(?i:\bissue\b)|以前は|昔は",
    )
    .unwrap()
}

fn has_history(comment: &str, pattern: &Regex) -> bool {
    pattern.is_match(&without_code_spans(comment)).unwrap()
}

#[test]
fn comments_do_not_record_history() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    for dir in SOURCE_DIRS {
        collect_sources(root, dir, &mut sources);
    }
    sources.sort();
    assert!(!sources.is_empty());

    let pattern = history_pattern();
    let mut found = Vec::new();
    for source in &sources {
        let text = fs::read_to_string(root.join(source)).unwrap();
        for (i, line) in text.lines().enumerate() {
            if let Some(comment) = comment_text(line) {
                if has_history(comment, &pattern) {
                    found.push(format!("{source}:{} {}", i + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "コメントに経緯（日付・issue / PR 番号・「以前は」など）が書かれている。\
         経緯は commit と PR に書き、コメントには今のコードの「なぜ」だけを書く:\n{}",
        found.join("\n")
    );
}

#[test]
fn history_detection_rules() {
    let pattern = history_pattern();
    let detects = |line: &str| comment_text(line).is_some_and(|c| has_history(c, &pattern));

    assert!(detects("// 2026-10-26 に直した"));
    assert!(detects("/// 2024年時点のデータ"));
    assert!(detects("    // 縁を光らせる(#31)"));
    assert!(detects("/// issue #20 の対策"));
    assert!(detects("//! 以前は英語だった"));
    assert!(detects(" * 昔は毎回描き直していた"));

    assert!(!detects("/// 販売期間（`2026-10-28 16:00` の形）"));
    assert!(!detects("let url = \"https://example.com/#1\";"));
    assert!(!detects("// 色は #fff"));
    assert!(!detects("let date = \"2026-10-26\"; // 日付の書式"));
    assert!(!detects("let d = \"2026-10-26\";"));
    assert!(!detects("assert!(detects(\"// 2026-10-26 に直した\"));"));
}
