//! CNで公開求人に追加されたオペレーターのJP反映日（`data/recruitment/jp_release.yaml`）。
//! 書き方・運用はYAML冒頭のコメント参照。

use super::Error;
use crate::engine::external_source::recruit_pool::PoolOperator;
use chrono::NaiveDate;
use serde::Deserialize;
use std::sync::OnceLock;

const JP_RELEASE_YAML: &str = include_str!("../../../data/recruitment/jp_release.yaml");

#[derive(Deserialize)]
struct RawEntry {
    date: String,
    // human-readable label; not used by code
    #[serde(default)]
    #[allow(dead_code)]
    comment: Option<String>,
    operators: Vec<String>,
}

pub struct JpReleaseEntry {
    pub date: NaiveDate,
    /// 表示名またはCN名。
    pub operators: Vec<String>,
}

pub struct JpReleaseSchedule {
    entries: Vec<JpReleaseEntry>,
}

impl JpReleaseSchedule {
    pub fn parse(yaml: &str) -> Result<Self, Error> {
        // コメントだけのファイル（エントリ0件）は空のドキュメントになるのでOptionで受ける。
        let raw: Option<Vec<RawEntry>> = serde_yaml::from_str(yaml)?;
        let entries = raw
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                let date = NaiveDate::parse_from_str(&e.date, "%Y-%m-%d")
                    .map_err(|err| format!("jp_release.yaml: 日付「{}」を解釈できません（YYYY-MM-DD）: {err}", e.date))?;
                Ok(JpReleaseEntry { date, operators: e.operators })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self { entries })
    }

    /// ビルド時に埋め込んだYAML。プロセス内で1回だけパースする（壊れていれば
    /// `embedded_schedule_parses`のテストで落ちる）。
    pub fn global() -> &'static Self {
        static CELL: OnceLock<JpReleaseSchedule> = OnceLock::new();
        CELL.get_or_init(|| Self::parse(JP_RELEASE_YAML).expect("data/recruitment/jp_release.yaml should parse"))
    }

    /// 表示名かCN名が一致する最初のエントリの反映日。
    pub fn date_for(&self, op: &PoolOperator) -> Option<NaiveDate> {
        self.entries
            .iter()
            .find(|e| e.operators.iter().any(|n| *n == op.name || *n == op.cn_name))
            .map(|e| e.date)
    }

    pub fn entries(&self) -> &[JpReleaseEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::recruit_pool::{RecruitPool, SEED_PATH};
    use std::collections::HashSet;

    fn op(name: &str, cn_name: &str) -> PoolOperator {
        PoolOperator {
            char_id: format!("char_{cn_name}"),
            name: name.to_string(),
            cn_name: cn_name.to_string(),
            job: "前衛".to_string(),
            tags: vec![],
            stars: 6,
        }
    }

    #[test]
    fn matches_by_display_name_or_cn_name() {
        let schedule = JpReleaseSchedule::parse(
            "- date: 2026-10-19\n  operators: [パラス]\n- date: 2027-01-20\n  comment: x\n  operators: [水月]\n",
        )
        .unwrap();
        assert_eq!(schedule.date_for(&op("パラス", "帕拉斯")), NaiveDate::from_ymd_opt(2026, 10, 19));
        assert_eq!(schedule.date_for(&op("ミヅキ", "水月")), NaiveDate::from_ymd_opt(2027, 1, 20));
        assert_eq!(schedule.date_for(&op("ラ・プルマ", "羽毛笔")), None);
    }

    #[test]
    fn comment_only_file_is_empty_schedule() {
        let schedule = JpReleaseSchedule::parse("# nothing yet\n").unwrap();
        assert!(schedule.entries().is_empty());
    }

    #[test]
    fn bad_date_is_rejected() {
        assert!(JpReleaseSchedule::parse("- date: 2026/10/19\n  operators: [パラス]\n").is_err());
    }

    #[test]
    fn embedded_schedule_parses() {
        JpReleaseSchedule::parse(JP_RELEASE_YAML).expect("data/recruitment/jp_release.yaml should parse");
    }

    /// jp_release.yamlの名前がSeedのプール（JP分かCN追加分）の誰かに当たること（タイプミス検出）と、
    /// 同じオペレーターを2回書いていないこと。JP側に移った名前はそのまま残っていてもよい。
    #[test]
    fn every_scheduled_name_points_to_pool_operator() {
        let json = std::fs::read_to_string(SEED_PATH)
            .unwrap_or_else(|e| panic!("seed({SEED_PATH})の読み込みに失敗: {e}。先に`cargo run --bin regen_seeds`を実行すること"));
        let pool: RecruitPool = serde_json::from_str(&json).expect("seed should parse as RecruitPool");
        let all: Vec<&PoolOperator> = pool.jp.iter().chain(&pool.cn_only).collect();

        let mut seen_ids: HashSet<&str> = HashSet::new();
        for entry in JpReleaseSchedule::global().entries() {
            for name in &entry.operators {
                let found = all
                    .iter()
                    .find(|o| o.name == *name || o.cn_name == *name)
                    .unwrap_or_else(|| {
                        let pending: Vec<&str> = pool.cn_only.iter().map(|o| o.name.as_str()).collect();
                        panic!("jp_release.yamlの「{name}」が公開求人のプールにいません（CN追加分: {pending:?}）")
                    });
                assert!(seen_ids.insert(&found.char_id), "jp_release.yamlに「{name}」が2回書かれています");
            }
        }
    }
}
