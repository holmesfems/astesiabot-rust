pub mod calculator;
pub mod formula;
pub mod item_array;
pub mod lists;
pub mod search;
pub mod server;
pub mod stage;
pub mod stage_info;
pub mod static_data;
pub mod values;

pub use lists::{TicketEfficiency, ValueEntry, CC_NUMBER};
pub use search::{MaterialSearchResult, MaterialStageInfo, StageCategoryEfficiency, StageEfficiencyInfo, StageSearchResult};
pub use server::Server;
pub use stage::StageItem;

use crate::engine::external_source::ExternalSourceRegistry;
use calculator::Calculator;
use chrono::Utc;
use stage_info::{CategoryInstance, DEFAULT_SHOW_MIN_TIMES};
use static_data::StaticData;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use values::RiseiValues;

/// risei_calculator_engine ドメイン共通のエラー型（bot にも api にも依存しない）。
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// 理性価値表の再計算に使う基準（Python `DEFAULT_CACHE_TIME`）。
const CACHE_MINUTES: i64 = 120;
/// 基準マップとして選ばれるために必要な最小試行数（Python版コマンドのデフォルト値）。
const DEFAULT_BASE_MIN_TIMES: i64 = 3000;

/// 理性価値表の計算エンジン（Python `CalculatorManager`相当のfacade）。
/// bot/api には依存しない。グローバル版・大陸版それぞれの計算結果を
/// 内部で保持し、キャッシュ期限切れ時に自動で再計算する。
pub struct RiseiCalculatorEngine {
    static_data: StaticData,
    global: RwLock<Calculator>,
    mainland: RwLock<Calculator>,
}

impl RiseiCalculatorEngine {
    /// 起動時に一度だけ両サーバ分をロード＆計算する。
    pub async fn load(outer_source: &ExternalSourceRegistry) -> Result<Self, Error> {
        let static_data = StaticData::load()?;
        let global = build_calculator(Server::Global, outer_source, &static_data).await?;
        let mainland = build_calculator(Server::Mainland, outer_source, &static_data).await?;
        Ok(Self {
            static_data,
            global: RwLock::new(global),
            mainland: RwLock::new(mainland),
        })
    }

    fn lock_for(&self, server: Server) -> &RwLock<Calculator> {
        match server {
            Server::Global => &self.global,
            Server::Mainland => &self.mainland,
        }
    }

    /// キャッシュが古ければ ark_stages/ark_matrix を再fetchしてから丸ごと再計算する
    /// （Python `Calculator.tryReInit`相当。再計算に失敗した場合は既存の値を
    /// 保持したまま継続する。`outer_source::Source::refresh`と同じ方針）。
    async fn ensure_fresh(&self, server: Server, outer_source: &ExternalSourceRegistry) {
        let stale = {
            let calc = self.lock_for(server).read().await;
            Utc::now() - calc.last_updated > chrono::Duration::minutes(CACHE_MINUTES)
        };
        if !stale {
            return;
        }
        tokio::join!(outer_source.ark_stages.refresh(), outer_source.ark_matrix.refresh());
        match build_calculator(server, outer_source, &self.static_data).await {
            Ok(new_calc) => {
                *self.lock_for(server).write().await = new_calc;
            }
            Err(e) => {
                eprintln!("[risei_calculator_engine] 再計算に失敗、既存の理性価値表を保持します: {e}");
            }
        }
    }

    /// 表示・検索に必要な情報一式のスナップショットを取得する
    /// （必要ならキャッシュ更新してから）。
    pub async fn snapshot(&self, server: Server, outer_source: &ExternalSourceRegistry) -> EngineSnapshot {
        self.ensure_fresh(server, outer_source).await;
        let calc = self.lock_for(server).read().await;
        EngineSnapshot {
            stage_info: calc.stage_info.clone(),
            values: calc.values.clone(),
            base_stage_ids: calc
                .base_stage_matrix
                .items
                .values()
                .map(|item| item.stage.stage_id.clone())
                .collect(),
            base_stage_display: calc.base_stage_matrix.to_ja_display_map(),
        }
    }

    /// カテゴリ定義一式（`new`カテゴリを含むかどうかの判定・オートコンプリート等に使う
    /// 構造データそのもの）。加工はbot/commands側の責務。
    pub fn stage_category(&self) -> &server::StageCategoryFile {
        &self.static_data.stage_category
    }
}

async fn build_calculator(
    server: Server,
    outer_source: &ExternalSourceRegistry,
    static_data: &StaticData,
) -> Result<Calculator, Error> {
    let item_names = outer_source.item_names.get().await;
    let zones = outer_source.zones.get().await;
    let ark_stages = outer_source.ark_stages.get().await;
    let ark_matrix = outer_source.ark_matrix.get().await;
    let formulas = outer_source.formulas.get().await;
    Calculator::build(
        server,
        &static_data.stage_category,
        &ark_stages,
        &ark_matrix,
        &zones,
        item_names,
        &formulas.formulas,
        &static_data.min_clear_time_injection,
        static_data.const_values.clone(),
        DEFAULT_BASE_MIN_TIMES,
    )
}

/// ある時点の計算結果一式のスナップショット（`Arc`ベースで安価にclone可能）。
/// bot/commands はこれを介してステージ検索・効率計算を行う。
#[derive(Clone)]
pub struct EngineSnapshot {
    pub stage_info: Arc<stage_info::StageInfo>,
    pub values: RiseiValues,
    base_stage_ids: HashSet<String>,
    pub base_stage_display: BTreeMap<String, String>,
}

/// カテゴリのmainItem/subItemドロップ率を`120秒`あたりの入手数に換算する
/// （Python版riseimaterials/riseistagesの`dropPerMin`計算と同じ式）。
/// riseimaterials/riseistagesの両方が使う汎用計算のためengineに残す。
pub fn drop_per_minute(stage: &StageItem, info: &server::StageCategoryInfo, values: &RiseiValues) -> f64 {
    let mut drop_values = stage.get_drop_rate(&info.main_item, &values.value_target, &values.item_names);
    for (item, order) in info.sub_item.iter().zip(info.sub_order.iter()) {
        drop_values += stage.get_drop_rate(item, &values.value_target, &values.item_names) / (*order as f64);
    }
    drop_values / stage.min_clear_time * 120.0
}

/// 試行数が足りないステージを除外する。全滅した場合は無条件で全部返す
/// （Python `CalculatorManager.filterStagesByShowMinTimes`）。
pub fn filter_stages_by_show_min_times(stages: Vec<Arc<StageItem>>, show_min_times: i64) -> Vec<Arc<StageItem>> {
    let filtered: Vec<Arc<StageItem>> = stages.iter().filter(|s| s.max_times() >= show_min_times).cloned().collect();
    if filtered.is_empty() {
        stages
    } else {
        filtered
    }
}

impl EngineSnapshot {
    pub fn stage_dev(&self, stage: &StageItem) -> f64 {
        if self.base_stage_ids.contains(&stage.stage_id) {
            return 0.0;
        }
        let cost = if stage.ap_cost > 0.0 { stage.ap_cost } else { 1.0 };
        stage.get_std_dev(&self.values) / cost
    }

    pub fn category(&self, key: &str) -> Option<&CategoryInstance> {
        self.stage_info.category_instance_dict.get(key)
    }

    pub fn category_stages(&self, key: &str) -> Vec<Arc<StageItem>> {
        self.category(key)
            .map(|cat| {
                cat.stage_ids
                    .iter()
                    .filter_map(|id| self.stage_info.main_stage_dict.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn search_main_stage(&self, target_code: &str) -> Vec<Arc<StageItem>> {
        self.stage_info.search_main_stage(target_code, DEFAULT_SHOW_MIN_TIMES)
    }

    pub fn search_event_stage(&self, target_code: &str) -> Vec<Arc<StageItem>> {
        self.stage_info.search_event_stage(target_code, DEFAULT_SHOW_MIN_TIMES)
    }

    pub fn auto_complete_main_stage(&self, target_code: &str, limit: usize) -> Vec<(String, String)> {
        self.stage_info.auto_complete_main_stage(target_code, limit)
    }

    pub fn auto_complete_event_stage(&self, target_code: &str, limit: usize) -> Vec<(String, String)> {
        self.stage_info.auto_complete_event_stage(target_code, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::ExternalSourceRegistry;

    /// 実ネットワークに対して起動時ロード〜線形連立方程式の解決〜基準マップ収束・
    /// ステージ検索まで一通り動くかの疎通確認。`cargo test -- --ignored` で明示実行する。
    /// riseiXXX相当のDTO構築ロジックはbot層(`bot/commands/risei/*.rs`)に移ったため
    /// ここでは検証しない。
    #[tokio::test]
    #[ignore]
    async fn load_and_solve_against_real_network() {
        let outer_source = ExternalSourceRegistry::load().await;
        let engine = RiseiCalculatorEngine::load(&outer_source)
            .await
            .expect("engine should build against real network data");

        for server in [Server::Global, Server::Mainland] {
            let snapshot = engine.snapshot(server, &outer_source).await;
            assert!(
                snapshot.values.get_value_from_zh("龙门币1000").is_finite(),
                "龙门币1000 value should be finite for {server:?}"
            );
            println!("[{server:?}] 龙门币1000 value = {}", snapshot.values.get_value_from_zh("龙门币1000"));

            let stages = snapshot.search_main_stage("1-7");
            assert!(!stages.is_empty(), "1-7 stage search should have candidates for {server:?}");
            println!("[{server:?}] 1-7 top hit: {}", stages[0].name);
        }
    }
}
