pub mod dto;
pub mod search;

pub use dto::{FkSearchResult, FkSkillView, SkillCandidate};

use super::external_source::fk_data::FkSheetData;
use super::external_source::operator_data::OperatorData;
use super::external_source::skill_data::SkillData;
use super::external_source::ExternalSourceRegistry;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::RwLock;

/// FK情報スプレッドシートの鮮度管理（Python `FKInfo.getInfoFromName`のTTLチェック相当）。
/// `OuterSourceRegistry::refresh_all`（日次バッチ）とは別軸で、読み取り駆動で1時間毎に
/// 再fetchする。
pub struct FkDataSearchEngine {
    last_checked: RwLock<DateTime<Utc>>,
}

impl FkDataSearchEngine {
    pub fn new() -> Self {
        Self {
            last_checked: RwLock::new(Utc::now()),
        }
    }

    /// 前回チェックから1時間以上経過していれば再fetchしてから、最新のスナップショットを返す。
    /// 再fetchに失敗した場合は`Source::refresh`の方針どおり直前のメモリを保持し続ける。
    pub async fn snapshot(&self, outer_source: &ExternalSourceRegistry) -> Arc<FkSheetData> {
        let stale = Utc::now() - *self.last_checked.read().await > chrono::Duration::hours(1);
        if stale {
            outer_source.fk_data.refresh().await;
            *self.last_checked.write().await = Utc::now();
        }
        outer_source.fk_data.get().await
    }
}

impl Default for FkDataSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 検索・オートコンプリートに必要な情報一式のスナップショット
/// （Python `AllOperatorsInfo` + `SkillIdToName` + `FKInfo.fkData`を束ねたもの）。
pub struct FkDataView {
    pub fk_data: Arc<FkSheetData>,
    pub operator_data: Arc<OperatorData>,
    pub skill_data: Arc<SkillData>,
}

impl FkDataView {
    pub fn search(&self, operator_name: &str, skill_num: &str) -> FkSearchResult {
        search::resolve(&self.fk_data, &self.operator_data, &self.skill_data, operator_name, skill_num)
    }

    /// Python `FKInfo.autoComplete`相当。TTLチェックは行わない
    /// （Python版も`autoComplete`は`fkData`を直接見るだけで鮮度更新をトリガーしない）。
    pub fn autocomplete(&self, partial: &str, limit: usize) -> Vec<String> {
        search::autocomplete(&self.fk_data, partial, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::{fk_data, operator_data, skill_data};

    /// 実ネットワークで実際のFKスプレッドシート+ゲームデータを突き合わせ、
    /// スキルを1つしか持たないオペレーターがskill_num省略で解決できることを確認する
    /// （Python `FKInfo.getReply`のhasOnlyOneSkill分岐の実データ検証）。
    #[tokio::test]
    #[ignore]
    async fn resolves_real_single_skill_operator() {
        dotenvy::dotenv().ok();
        let fk_sheet = fk_data::fetch().await.expect("fk_data fetch should succeed against real network");
        let op_data = operator_data::fetch().await.expect("operator_data fetch should succeed against real network");
        let sk_data = skill_data::fetch().await.expect("skill_data fetch should succeed against real network");
        let view = FkDataView {
            fk_data: Arc::new(fk_sheet),
            operator_data: Arc::new(op_data),
            skill_data: Arc::new(sk_data),
        };
        match view.search("アイリス", "") {
            FkSearchResult::Found(v) => {
                assert_eq!(v.fk_num, "1");
                println!("resolved skill_name={:?} fk_err={:?}", v.skill_name, v.fk_err);
            }
            FkSearchResult::OperatorNotFound => panic!("expected Found, got OperatorNotFound"),
            FkSearchResult::NeedsSkillSelection { .. } => panic!("expected Found, got NeedsSkillSelection"),
            FkSearchResult::SkillNotFound { .. } => panic!("expected Found, got SkillNotFound"),
        }
    }

    /// 実データでの複数スキル分岐の検証（Python `getReply`のスキル未指定+複数候補、
    /// および指定スキルでの解決の両方を1件の実オペレーターで確認する）。
    #[tokio::test]
    #[ignore]
    async fn needs_selection_for_real_multi_skill_operator() {
        dotenvy::dotenv().ok();
        let fk_sheet = fk_data::fetch().await.expect("fk_data fetch should succeed against real network");
        let op_data = operator_data::fetch().await.expect("operator_data fetch should succeed against real network");
        let sk_data = skill_data::fetch().await.expect("skill_data fetch should succeed against real network");
        let view = FkDataView {
            fk_data: Arc::new(fk_sheet),
            operator_data: Arc::new(op_data),
            skill_data: Arc::new(sk_data),
        };
        match view.search("Ela", "") {
            FkSearchResult::NeedsSkillSelection { choices } => {
                println!("choices={:?}", choices.iter().collect::<Vec<_>>());
                assert_eq!(choices.len(), 2);
            }
            _ => panic!("expected NeedsSkillSelection"),
        }
        match view.search("Ela", "2") {
            FkSearchResult::Found(v) => println!("Ela skill2 fk_num={}", v.fk_num),
            _ => panic!("expected Found for skill_num=2"),
        }
    }
}

