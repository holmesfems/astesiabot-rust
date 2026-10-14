//! オペレーターがJPのゲームデータに初めて現れた日（直近実装の判定用）。
//!
//! JPのゲームデータのリポジトリ（ArknightsAssets）にある `char_meta_table.json`（約75KB。
//! JPで入手可能なオペレーターが全員載っている）のコミット履歴を二分探索して、
//! キャラごとに初めて載ったコミットの日付（UTC）を `data/seed/jp_first_seen.json` に記録する。
//! 記録の更新は `regen_seeds`（[`super::SEED_JOBS`]）だけが行い、bot の実行中は
//! `include_str!` で埋め込んだ記録を読むだけ（GitHubへアクセスしない）。
//!
//! 更新は差分だけ: 前回調べたコミット（`checked_commit`）より新しいコミットの範囲で、
//! 最新版に載っていて記録に無いキャラだけを探す。ダウンロードはキャラ間で使い回す。
//! 履歴の最初のコミット（2023-12-24）から載っているキャラはその日付で記録する
//! （直近実装の判定にはそれより前の正確な日付は要らない）。
//! 昇格オペレーター（前衛アーミヤ等）は `char_meta_table.json` に載らないので記録されない。

use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry_headers, fetch_typed_with_retry};
use super::{BoxFuture, FetchError};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, OnceLock};

const REPO: &str = "ArknightsAssets/ArknightsGamedata";
const CHAR_META_PATH: &str = "jp/gamedata/excel/char_meta_table.json";

/// 記録の保存先。`cargo run --bin regen_seeds` で更新し、git commitしてリポジトリに含める。
pub const SEED_PATH: &str = "data/seed/jp_first_seen.json";
const EMBEDDED_JSON: &str = include_str!("../../../data/seed/jp_first_seen.json");

/// `data/seed/jp_first_seen.json` の中身。
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct JpFirstSeenRecord {
    /// ここまで調べたコミット。次回はこれより新しいコミットだけを探す。
    pub checked_commit: Option<String>,
    /// `checked_commit` のコミット日時（GitHub APIの`since`に渡す）。
    pub checked_at: Option<String>,
    /// charId → 初めて載ったコミットの日付（YYYY-MM-DD、UTC）。
    pub first_seen: BTreeMap<String, String>,
}

/// 実行時に引く用の、日付を解釈済みの記録。
pub struct JpFirstSeen {
    dates: HashMap<String, NaiveDate>,
    /// `checked_at`の日付（UTC）。記録に無い非昇格のJP実装済みオペレーターは、この日以降に
    /// 現れたことになる（記録は`regen_seeds`でしか更新されないため）。
    checked_on: Option<NaiveDate>,
}

impl JpFirstSeen {
    /// ビルド時に埋め込んだ記録。プロセス内で1回だけパースする（壊れていれば
    /// `embedded_record_parses`のテストで落ちる）。
    pub fn embedded() -> &'static Self {
        static CELL: OnceLock<JpFirstSeen> = OnceLock::new();
        CELL.get_or_init(|| {
            let record: JpFirstSeenRecord = serde_json::from_str(EMBEDDED_JSON).expect("data/seed/jp_first_seen.json should parse");
            Self::from_record(&record).expect("data/seed/jp_first_seen.json should have YYYY-MM-DD dates")
        })
    }

    fn from_record(record: &JpFirstSeenRecord) -> Result<Self, FetchError> {
        let dates = record
            .first_seen
            .iter()
            .map(|(id, date)| {
                let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|e| format!("{id}: 日付「{date}」を解釈できません: {e}"))?;
                Ok((id.clone(), date))
            })
            .collect::<Result<HashMap<_, _>, FetchError>>()?;
        let checked_on = record
            .checked_at
            .as_deref()
            .map(|at| {
                chrono::DateTime::parse_from_rfc3339(at)
                    .map(|t| t.with_timezone(&chrono::Utc).date_naive())
                    .map_err(|e| format!("checked_at「{at}」を解釈できません: {e}"))
            })
            .transpose()?;
        Ok(Self { dates, checked_on })
    }

    /// JPのゲームデータに初めて現れた日。記録に無い（JP未実装・昇格オペレーター）ならNone。
    pub fn first_seen(&self, char_id: &str) -> Option<NaiveDate> {
        self.dates.get(char_id).copied()
    }

    /// 記録を最後に更新した日（`checked_at`の日付。UTC）。
    pub fn checked_on(&self) -> Option<NaiveDate> {
        self.checked_on
    }
}

/// `regen_seeds` 用。既存の記録を読み、新しいコミットの分だけ追記して書き出す。
pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let mut record: JpFirstSeenRecord = match std::fs::read_to_string(SEED_PATH) {
            Ok(s) => serde_json::from_str(&s)?,
            Err(_) => JpFirstSeenRecord::default(),
        };
        update_record(&mut record).await?;
        write_seed_file(SEED_PATH, &record)
    })
}

#[derive(Deserialize)]
struct RawCommit {
    sha: String,
    commit: RawCommitDetail,
}

#[derive(Deserialize)]
struct RawCommitDetail {
    committer: RawCommitter,
}

#[derive(Deserialize)]
struct RawCommitter {
    date: String,
}

#[derive(Deserialize)]
struct RawCharMeta {
    #[serde(rename = "spCharGroups")]
    sp_char_groups: HashMap<String, Vec<String>>,
}

/// コミット1件（古い順に並べて使う）。
struct Commit {
    sha: String,
    date: String,
}

async fn update_record(record: &mut JpFirstSeenRecord) -> Result<(), FetchError> {
    let client = client();
    let commits = fetch_new_commits(&client, record).await?;
    let Some(last) = commits.last() else {
        println!("新しいコミットはありません");
        return Ok(());
    };

    let dates: Vec<String> = commits.iter().map(|c| c.date.clone()).collect();
    let fetch = |idx: usize| {
        let client = client.clone();
        let url = format!("https://raw.githubusercontent.com/{REPO}/{}/{CHAR_META_PATH}", commits[idx].sha);
        async move {
            let meta: RawCharMeta = fetch_typed_with_retry(&client, &url).await?;
            Ok(meta.sp_char_groups.into_values().flatten().collect())
        }
    };
    let mut snapshots = Snapshots::new(fetch);
    let added = assign_first_seen(&mut record.first_seen, &dates, &mut snapshots).await?;
    println!("新しいコミット{}件 / 追加{}体 / ダウンロード{}回", commits.len(), added, snapshots.cache.len());

    record.checked_commit = Some(last.sha.clone());
    record.checked_at = Some(last.date.clone());
    Ok(())
}

/// `char_meta_table.json` のコミットのうち、前回調べたコミットより新しいものを古い順に返す。
async fn fetch_new_commits(client: &reqwest::Client, record: &JpFirstSeenRecord) -> Result<Vec<Commit>, FetchError> {
    // GitHub APIはUser-Agent必須。GITHUB_TOKENがあれば使う（無くても1時間60回まで使える）。
    let token = std::env::var("GITHUB_TOKEN").ok().map(|t| format!("Bearer {t}"));
    let mut headers: Vec<(&str, &str)> = vec![("User-Agent", "astesiabot-rust"), ("Accept", "application/vnd.github+json")];
    if let Some(token) = &token {
        headers.push(("Authorization", token.as_str()));
    }

    let mut newest_first: Vec<Commit> = Vec::new();
    for page in 1.. {
        let mut url = format!("https://api.github.com/repos/{REPO}/commits?path={CHAR_META_PATH}&per_page=100&page={page}");
        if let Some(since) = &record.checked_at {
            url.push_str(&format!("&since={since}"));
        }
        let json = fetch_json_with_retry_headers(client, &url, &headers).await?;
        let batch: Vec<RawCommit> = serde_json::from_value(json)?;
        let len = batch.len();
        newest_first.extend(batch.into_iter().map(|c| Commit { sha: c.sha, date: c.commit.committer.date }));
        if len < 100 {
            break;
        }
    }

    // `since`は境界を含むので、前回調べたコミット自身とそれより前は落とす。
    let mut commits: Vec<Commit> = newest_first.into_iter().rev().collect();
    if let Some(checked) = &record.checked_commit {
        if let Some(pos) = commits.iter().position(|c| &c.sha == checked) {
            commits.drain(..=pos);
        }
    }
    Ok(commits)
}

/// コミットごとの「載っているcharId一覧」をダウンロードして使い回すキャッシュ。
struct Snapshots<F> {
    fetch: F,
    cache: HashMap<usize, Arc<HashSet<String>>>,
}

impl<F, Fut> Snapshots<F>
where
    F: Fn(usize) -> Fut,
    Fut: Future<Output = Result<HashSet<String>, FetchError>>,
{
    fn new(fetch: F) -> Self {
        Self { fetch, cache: HashMap::new() }
    }

    async fn ids_at(&mut self, idx: usize) -> Result<Arc<HashSet<String>>, FetchError> {
        if let Some(ids) = self.cache.get(&idx) {
            return Ok(ids.clone());
        }
        let ids = Arc::new((self.fetch)(idx).await?);
        self.cache.insert(idx, ids.clone());
        Ok(ids)
    }
}

/// `dates`（古い順のコミット日時）の範囲で、最新コミットに載っていて`first_seen`に無いキャラが
/// 初めて載ったコミットを二分探索し、その日付を記録する。追加した数を返す。
async fn assign_first_seen<F, Fut>(
    first_seen: &mut BTreeMap<String, String>,
    dates: &[String],
    snapshots: &mut Snapshots<F>,
) -> Result<usize, FetchError>
where
    F: Fn(usize) -> Fut,
    Fut: Future<Output = Result<HashSet<String>, FetchError>>,
{
    let last = dates.len() - 1;
    let latest = snapshots.ids_at(last).await?;
    let mut new_ids: Vec<&String> = latest.iter().filter(|id| !first_seen.contains_key(*id)).collect();
    new_ids.sort();

    for id in &new_ids {
        let idx = if snapshots.ids_at(0).await?.contains(*id) {
            0
        } else {
            // 不変条件: lo には載っていない、hi には載っている。
            let (mut lo, mut hi) = (0, last);
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                if snapshots.ids_at(mid).await?.contains(*id) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            hi
        };
        first_seen.insert(id.to_string(), dates[idx].chars().take(10).collect());
    }
    Ok(new_ids.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 偽の履歴（32コミット）: "old"は最初から、"a"は3番目、"b"は20番目、"c"は最後のコミットで登場。
    fn history() -> Vec<HashSet<String>> {
        (0..32)
            .map(|i| {
                let mut ids = vec!["old"];
                if i >= 2 {
                    ids.push("a");
                }
                if i >= 19 {
                    ids.push("b");
                }
                if i >= 31 {
                    ids.push("c");
                }
                ids.into_iter().map(str::to_string).collect()
            })
            .collect()
    }

    fn dates(n: usize) -> Vec<String> {
        // 日付だけでコミットを見分けられるよう、1コミット1年にずらす。
        (0..n).map(|i| format!("{}-01-01T00:00:00Z", 2000 + i)).collect()
    }

    #[tokio::test]
    async fn finds_first_commit_for_each_new_id_sharing_downloads() {
        let history = history();
        let fetch = |idx: usize| {
            let ids = history[idx].clone();
            async move { Ok::<_, FetchError>(ids) }
        };
        let mut snapshots = Snapshots::new(fetch);
        let mut first_seen = BTreeMap::new();
        let added = assign_first_seen(&mut first_seen, &dates(history.len()), &mut snapshots).await.unwrap();

        assert_eq!(added, 4);
        assert_eq!(first_seen["old"], dates(32)[0][..10], "最初から載っているものは最初のコミットの日付");
        assert_eq!(first_seen["a"], dates(32)[2][..10]);
        assert_eq!(first_seen["b"], dates(32)[19][..10]);
        assert_eq!(first_seen["c"], dates(32)[31][..10]);
        assert!(snapshots.cache.len() < history.len(), "全コミットをダウンロードしないこと: {}", snapshots.cache.len());
    }

    #[tokio::test]
    async fn only_searches_ids_missing_from_record() {
        let history = history();
        let fetch = |idx: usize| {
            let ids = history[idx].clone();
            async move { Ok::<_, FetchError>(ids) }
        };
        let mut snapshots = Snapshots::new(fetch);
        let mut first_seen = BTreeMap::from([
            ("old".to_string(), "2025-12-01".to_string()),
            ("a".to_string(), "2025-12-02".to_string()),
            ("b".to_string(), "2025-12-03".to_string()),
        ]);
        let added = assign_first_seen(&mut first_seen, &dates(history.len()), &mut snapshots).await.unwrap();

        assert_eq!(added, 1);
        assert_eq!(first_seen["a"], "2025-12-02", "記録済みの日付は変えない");
        assert_eq!(first_seen["c"], dates(32)[31][..10]);
    }

    #[test]
    fn embedded_record_parses() {
        let record: JpFirstSeenRecord = serde_json::from_str(EMBEDDED_JSON).expect("should parse");
        JpFirstSeen::from_record(&record).expect("dates should parse");
        assert!(record.checked_commit.is_some());
    }

    /// 実ネットワークで記録を一から作れることの確認（ダウンロード回数も表示する）。
    /// `cargo test -- --ignored` で明示実行する。
    #[tokio::test]
    #[ignore]
    async fn builds_record_from_scratch_against_real_history() {
        let mut record = JpFirstSeenRecord::default();
        update_record(&mut record).await.expect("should build against real network");
        assert!(record.first_seen.len() > 400);
        assert_eq!(record.first_seen.get("char_4223_botany").map(String::as_str), Some("2026-09-16"));
    }
}
