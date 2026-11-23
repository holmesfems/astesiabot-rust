use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const SKIP_DIRS: &[&str] = &["target", ".git", ".claude", "local", "node_modules"];

const PATH_EXTENSIONS: &[&str] = &[
    ".rs", ".json", ".yaml", ".yml", ".html", ".js", ".mjs", ".toml", ".md", ".css", ".png",
    ".txt", ".csv",
];

fn walk(root: &Path, rel: &str, paths: &mut BTreeSet<String>, claude_mds: &mut Vec<String>) {
    let dir = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            if rel.is_empty() && SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            paths.insert(child.clone());
            walk(root, &child, paths, claude_mds);
        } else {
            if name == "CLAUDE.md" {
                claude_mds.push(child.clone());
            }
            paths.insert(child);
        }
    }
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => {
            let Some(tail) = text.strip_prefix(head) else {
                return false;
            };
            (0..=tail.len())
                .filter(|&i| tail.is_char_boundary(i))
                .any(|i| wildcard_match(rest, &tail[i..]))
        }
    }
}

fn ends_with_reference(path: &str, reference: &[&str]) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    segments.len() >= reference.len()
        && segments[segments.len() - reference.len()..]
            .iter()
            .zip(reference)
            .all(|(seg, pat)| wildcard_match(pat, seg))
}

fn looks_like_repo_path(span: &str) -> bool {
    span.contains('/') || PATH_EXTENSIONS.iter().any(|ext| span.ends_with(ext))
}

fn is_checked(span: &str) -> bool {
    if span.chars().any(char::is_whitespace) || !looks_like_repo_path(span) {
        return false;
    }
    // URL のルート、引数、`<名前>` や `…` の置き換え用の書き方はファイルではない
    if span.starts_with('/')
        || span.starts_with("--")
        || span.contains("...")
        || span.contains(['<', '>', '…', '%'])
    {
        return false;
    }
    // 拡張子だけ（「同名の `.json`」など）
    if span.starts_with('.') && !span[1..].contains(['.', '/']) {
        return false;
    }
    // local/ は gitignore（手元専用の置き場）、*_table.json は外から取るゲームデータの表で、リポジトリには無い
    if span.starts_with("local/") || span.ends_with("_table.json") {
        return false;
    }
    true
}

fn backtick_spans(markdown: &str) -> Vec<(usize, String)> {
    let mut spans = Vec::new();
    let mut in_fence = false;
    for (i, line) in markdown.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let parts: Vec<&str> = line.split('`').collect();
        for span in parts.iter().skip(1).step_by(2).take(parts.len() / 2) {
            if !span.is_empty() {
                spans.push((i + 1, span.to_string()));
            }
        }
    }
    spans
}

#[test]
fn paths_written_in_claude_md_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = BTreeSet::new();
    let mut claude_mds = Vec::new();
    walk(root, "", &mut paths, &mut claude_mds);
    assert!(!claude_mds.is_empty(), "CLAUDE.md が1つも見つからない");

    let mut missing = Vec::new();
    for md in &claude_mds {
        let text = fs::read_to_string(root.join(md)).unwrap();
        for (line, span) in backtick_spans(&text) {
            if !is_checked(&span) {
                continue;
            }
            let trimmed = span
                .trim_start_matches("../")
                .trim_start_matches("./")
                .trim_end_matches('/');
            let reference: Vec<&str> = trimmed.split('/').collect();
            if !paths.iter().any(|p| ends_with_reference(p, &reference)) {
                missing.push(format!("{md}:{line} `{span}`"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "CLAUDE.md に書かれたパスがリポジトリに無い（名前を変えた・消したなら CLAUDE.md も直す）:\n{}",
        missing.join("\n")
    );
}

#[test]
fn reference_matching_rules() {
    assert!(ends_with_reference(
        "src/engine/external_source/http.rs",
        &["external_source", "http.rs"]
    ));
    assert!(ends_with_reference(
        "data/seed/ark_matrix.json",
        &["data", "seed", "*.json"]
    ));
    assert!(!ends_with_reference(
        "src/engine/http.rs",
        &["external_source", "http.rs"]
    ));
    assert!(!ends_with_reference("src/a.rs", &["x", "src", "a.rs"]));

    assert!(is_checked("src/api/CLAUDE.md"));
    assert!(is_checked("mod.rs"));
    assert!(!is_checked("/api/risei/lists/kakin"));
    assert!(!is_checked("<タグ>_<番号>.png"));
    assert!(!is_checked(".json"));
    assert!(is_checked(".cargo/audit.toml"));
    assert!(!is_checked("local/recruit_samples/english_images.txt"));
    assert!(!is_checked("gacha_table.json"));
    assert!(!is_checked("cargo run"));
}
