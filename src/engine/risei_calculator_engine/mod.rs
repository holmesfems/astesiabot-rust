pub mod calculator;
pub mod formula;
pub mod item_array;
pub mod server;
pub mod stage;
pub mod stage_info;
pub mod static_data;
pub mod values;

pub use server::Server;
pub use stage::StageItem;

use crate::engine::outer_source::OuterSourceRegistry;
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
    pub async fn load(outer_source: &OuterSourceRegistry) -> Result<Self, Error> {
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
    async fn ensure_fresh(&self, server: Server, outer_source: &OuterSourceRegistry) {
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
    pub async fn snapshot(&self, server: Server, outer_source: &OuterSourceRegistry) -> EngineSnapshot {
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

    /// riseimaterials相当。`new`カテゴリを指定した場合は自動的に大陸版基準になる。
    pub async fn material_search(
        &self,
        server: Server,
        category_key: &str,
        outer_source: &OuterSourceRegistry,
    ) -> Result<(Server, MaterialSearchResult), String> {
        let effective_server = if self.static_data.stage_category.new.contains_key(category_key) {
            Server::Mainland
        } else {
            server
        };
        let snapshot = self.snapshot(effective_server, outer_source).await;
        match snapshot.material_search(category_key) {
            Some(result) => Ok((effective_server, result)),
            None => Err(format!("無効なカテゴリ:{category_key}")),
        }
    }

    /// riseistages相当。グローバル版に該当ステージが無ければ大陸版にフォールバックする。
    pub async fn stage_search(
        &self,
        server: Server,
        target_code: &str,
        outer_source: &OuterSourceRegistry,
    ) -> Result<(Server, Vec<StageEfficiencyInfo>), String> {
        let mut snapshot = self.snapshot(server, outer_source).await;
        let mut stages = snapshot.search_main_stage(target_code);
        let mut effective_server = server;
        if stages.is_empty() && server == Server::Global {
            snapshot = self.snapshot(Server::Mainland, outer_source).await;
            stages = snapshot.search_main_stage(target_code);
            effective_server = Server::Mainland;
        }
        if stages.is_empty() {
            return Err(format!("無効なステージ指定{target_code}"));
        }
        Ok((effective_server, snapshot.stage_search_results(stages)))
    }

    /// riseievents相当。
    pub async fn event_search(
        &self,
        server: Server,
        target_code: &str,
        outer_source: &OuterSourceRegistry,
    ) -> Result<Vec<EventStageInfo>, String> {
        let snapshot = self.snapshot(server, outer_source).await;
        let stages = snapshot.search_event_stage(target_code);
        if stages.is_empty() {
            return Err(format!("無効なステージ指定{target_code}"));
        }
        Ok(snapshot.event_search_results(stages))
    }

    /// riseilists(basemaps)相当。
    pub async fn base_maps(&self, server: Server, outer_source: &OuterSourceRegistry) -> BTreeMap<String, String> {
        self.snapshot(server, outer_source).await.base_stage_display
    }

    /// riseilists(san_value_lists)相当。(ja名, 価値, 標準偏差)のリスト。
    pub async fn value_list(&self, server: Server, outer_source: &OuterSourceRegistry) -> Vec<(String, f64, f64)> {
        self.snapshot(server, outer_source).await.value_list()
    }

    /// riseilists(te2list)相当。
    pub async fn te2_list(&self, server: Server, outer_source: &OuterSourceRegistry) -> Vec<TicketEfficiency> {
        let snapshot = self.snapshot(server, outer_source).await;
        snapshot.ticket_efficiency_list(&server::item_rarity2(server), &self.static_data.price)
    }

    /// riseilists(te3list)相当。
    pub async fn te3_list(&self, server: Server, outer_source: &OuterSourceRegistry) -> Vec<TicketEfficiency> {
        let snapshot = self.snapshot(server, outer_source).await;
        snapshot.ticket_efficiency_list(&server::item_rarity3(server), &self.static_data.price)
    }

    /// riseilists(special_list)相当。初級・上級両方の資格証対象アイテムを特別引換証価格で評価する。
    pub async fn special_list(&self, server: Server, outer_source: &OuterSourceRegistry) -> Vec<TicketEfficiency> {
        let snapshot = self.snapshot(server, outer_source).await;
        let mut items = server::item_rarity2(server);
        items.extend(server::item_rarity3(server));
        snapshot.ticket_efficiency_list(&items, &self.static_data.price_special)
    }

    /// riseimaterials/riseicalculator の target_item 選択肢（Python版は常に大陸版の
    /// 全カテゴリ(main+new)を選択肢に使う）。(表示名, カテゴリキー) のリスト。
    pub fn category_choices(&self) -> Vec<(String, String)> {
        self.static_data
            .stage_category
            .main
            .iter()
            .chain(self.static_data.stage_category.new.iter())
            .map(|(key, info)| (info.to_ja.clone(), key.clone()))
            .collect()
    }
}

async fn build_calculator(
    server: Server,
    outer_source: &OuterSourceRegistry,
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
    base_stage_display: BTreeMap<String, String>,
}

pub struct TicketEfficiency {
    pub name_ja: String,
    pub efficiency: f64,
    pub std_dev: f64,
}

pub struct MaterialStageInfo {
    pub name: String,
    pub efficiency: f64,
    pub sanity_cost: f64,
    /// 倍速時のクリア時間(秒)。ステージにクリア時間データが無ければ`None`。
    pub time_cost: Option<f64>,
    pub drop_per_minute: Option<f64>,
    pub main_item_efficiency: f64,
    pub confidence_3sigma: f64,
    pub promotion_efficiency: f64,
    pub max_times: i64,
}

pub struct MaterialSearchResult {
    pub category_ja: String,
    pub main_item_value: f64,
    pub main_item_std_dev: f64,
    /// 総合効率の降順でソート済み。件数の絞り込みはbot/commands側で行う。
    pub stages: Vec<MaterialStageInfo>,
}

pub struct StageCategoryEfficiency {
    pub category_ja: String,
    pub efficiency: f64,
    pub drop_per_minute: Option<f64>,
}

pub struct StageEfficiencyInfo {
    pub name: String,
    pub total_efficiency: f64,
    pub confidence_3sigma: f64,
    pub categories: Vec<StageCategoryEfficiency>,
    pub sanity_cost: f64,
    pub time_cost: Option<f64>,
    pub promotion_efficiency: f64,
    pub max_times: i64,
}

pub struct EventStageInfo {
    pub name: String,
    pub zone_name: String,
    pub total_efficiency: f64,
    pub main_drop_name: String,
    pub main_drop_rate: f64,
    pub max_times: i64,
    pub sanity_cost: f64,
    pub time_cost: Option<f64>,
    pub drop_per_minute: Option<f64>,
}

/// カテゴリのmainItem/subItemドロップ率を`120秒`あたりの入手数に換算する
/// （Python版riseimaterials/riseistagesの`dropPerMin`計算と同じ式）。
fn drop_per_minute(
    stage: &StageItem,
    info: &server::StageCategoryInfo,
    values: &RiseiValues,
) -> f64 {
    let mut drop_values = stage.get_drop_rate(&info.main_item, &values.value_target, &values.item_names);
    for (item, order) in info.sub_item.iter().zip(info.sub_order.iter()) {
        drop_values += stage.get_drop_rate(item, &values.value_target, &values.item_names) / (*order as f64);
    }
    drop_values / stage.min_clear_time * 120.0
}

/// 試行数が足りないステージを除外する。全滅した場合は無条件で全部返す
/// （Python `CalculatorManager.filterStagesByShowMinTimes`）。
fn filter_stages_by_show_min_times(stages: Vec<Arc<StageItem>>, show_min_times: i64) -> Vec<Arc<StageItem>> {
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

    fn category(&self, key: &str) -> Option<&CategoryInstance> {
        self.stage_info.category_instance_dict.get(key)
    }

    fn category_stages(&self, key: &str) -> Vec<Arc<StageItem>> {
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

    pub fn value_list(&self) -> Vec<(String, f64, f64)> {
        self.values
            .value_target
            .iter()
            .map(|zh| {
                let ja = self.values.item_names.zh_to_ja(zh).to_string();
                (ja, self.values.get_value_from_zh(zh), self.values.get_std_dev_from_zh(zh))
            })
            .collect()
    }

    pub fn ticket_efficiency_list(
        &self,
        items: &[&str],
        price: &std::collections::HashMap<String, f64>,
    ) -> Vec<TicketEfficiency> {
        let mut list: Vec<TicketEfficiency> = items
            .iter()
            .filter_map(|zh| {
                let p = price.get(*zh)?;
                if *p == 0.0 {
                    return None;
                }
                Some(TicketEfficiency {
                    name_ja: self.values.item_names.zh_to_ja(zh).to_string(),
                    efficiency: self.values.get_value_from_zh(zh) / p,
                    std_dev: self.values.get_std_dev_from_zh(zh) / p,
                })
            })
            .collect();
        list.sort_by(|a, b| b.efficiency.total_cmp(&a.efficiency));
        list
    }

    pub fn material_search(&self, category_key: &str) -> Option<MaterialSearchResult> {
        let cat = self.category(category_key)?;
        let info = cat.info.clone();
        let stages = filter_stages_by_show_min_times(self.category_stages(category_key), DEFAULT_SHOW_MIN_TIMES);
        let promotion_items: Vec<&str> = self.values.value_target[4..].to_vec();

        let mut list: Vec<MaterialStageInfo> = stages
            .iter()
            .map(|stage| {
                let items: Vec<&str> = info.items.iter().map(String::as_str).collect();
                let (time_cost, drop_per_minute_value) = if stage.min_clear_time > 0.0 {
                    (Some(stage.min_clear_time / 2.0), Some(drop_per_minute(stage, &info, &self.values)))
                } else {
                    (None, None)
                };
                MaterialStageInfo {
                    name: stage.name_with_replicate(),
                    efficiency: stage.get_efficiency(&self.values),
                    sanity_cost: stage.ap_cost,
                    time_cost,
                    drop_per_minute: drop_per_minute_value,
                    main_item_efficiency: stage.get_partial_efficiency(&self.values, &items),
                    confidence_3sigma: self.stage_dev(stage) * 3.0,
                    promotion_efficiency: stage.get_partial_efficiency(&self.values, &promotion_items),
                    max_times: stage.max_times(),
                }
            })
            .collect();
        list.sort_by(|a, b| b.efficiency.total_cmp(&a.efficiency));

        Some(MaterialSearchResult {
            category_ja: info.to_ja.clone(),
            main_item_value: self.values.get_value_from_zh(&info.main_item),
            main_item_std_dev: self.values.get_std_dev_from_zh(&info.main_item),
            stages: list,
        })
    }

    pub fn stage_search_results(&self, mut stages: Vec<Arc<StageItem>>) -> Vec<StageEfficiencyInfo> {
        stages.sort_by(|a, b| a.name.cmp(&b.name));
        let promotion_items: Vec<&str> = self.values.value_target[4..].to_vec();
        stages
            .iter()
            .map(|stage| {
                let categories = self
                    .stage_info
                    .stage_to_categories(stage)
                    .into_iter()
                    .filter_map(|key| {
                        let info = self.category(&key)?.info.clone();
                        let items: Vec<&str> = info.items.iter().map(String::as_str).collect();
                        let efficiency = stage.get_partial_efficiency(&self.values, &items);
                        let drop_per_minute_value =
                            (stage.min_clear_time > 0.0).then(|| drop_per_minute(stage, &info, &self.values));
                        Some(StageCategoryEfficiency {
                            category_ja: info.to_ja,
                            efficiency,
                            drop_per_minute: drop_per_minute_value,
                        })
                    })
                    .collect();
                StageEfficiencyInfo {
                    name: stage.name_with_replicate(),
                    total_efficiency: stage.get_efficiency(&self.values),
                    confidence_3sigma: self.stage_dev(stage) * 3.0,
                    categories,
                    sanity_cost: stage.ap_cost,
                    time_cost: (stage.min_clear_time > 0.0).then_some(stage.min_clear_time / 2.0),
                    promotion_efficiency: stage.get_partial_efficiency(&self.values, &promotion_items),
                    max_times: stage.max_times(),
                }
            })
            .collect()
    }

    pub fn event_search_results(&self, mut stages: Vec<Arc<StageItem>>) -> Vec<EventStageInfo> {
        stages.sort_by(|a, b| a.name.cmp(&b.name));
        stages
            .iter()
            .filter_map(|stage| {
                let (main_drop_name, main_drop_rate) = stage.get_max_efficiency_item(&self.values.item_names)?;
                let time_cost = (stage.min_clear_time > 0.0).then_some(stage.min_clear_time / 2.0);
                let drop_per_minute_value =
                    (stage.min_clear_time > 0.0).then(|| main_drop_rate / stage.min_clear_time * 120.0);
                Some(EventStageInfo {
                    name: stage.name_with_replicate(),
                    zone_name: stage.zone_name.clone(),
                    total_efficiency: stage.get_efficiency(&self.values),
                    main_drop_name,
                    main_drop_rate,
                    max_times: stage.max_times(),
                    sanity_cost: stage.ap_cost,
                    time_cost,
                    drop_per_minute: drop_per_minute_value,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::outer_source::OuterSourceRegistry;

    /// 実ネットワークに対して起動時ロード〜線形連立方程式の解決〜基準マップ収束まで
    /// 一通り動くかの疎通確認。`cargo test -- --ignored` で明示実行する。
    #[tokio::test]
    #[ignore]
    async fn load_and_solve_against_real_network() {
        let outer_source = OuterSourceRegistry::load().await;
        let engine = RiseiCalculatorEngine::load(&outer_source)
            .await
            .expect("engine should build against real network data");

        for server in [Server::Global, Server::Mainland] {
            let snapshot = engine.snapshot(server, &outer_source).await;
            let values = snapshot.value_list();
            assert!(!values.is_empty(), "value list should not be empty for {server:?}");
            for (name, value, _) in &values {
                assert!(value.is_finite(), "{name} value should be finite, got {value}");
            }
            println!("[{server:?}] 龙门币1000 value = {}", snapshot.values.get_value_from_zh("龙门币1000"));
        }

        let (_server, materials) = engine
            .material_search(Server::Global, "源岩", &outer_source)
            .await
            .expect("源岩 category should resolve");
        assert!(!materials.stages.is_empty(), "源岩 category should have candidate stages");
        println!("源岩 top stage: {}", materials.stages[0].name);
    }
}
