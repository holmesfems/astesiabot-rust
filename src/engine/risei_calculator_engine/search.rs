//! riseimaterials/riseistages 相当のステージ検索DTO+計算(Python `CalculatorManager.riseimaterials`/
//! `riseistages`の計算部相当)。整形(Discord embed化/GPT向けJSON化)は呼び出し側
//! (`bot/commands/risei/*.rs` / `bot/services/uranai/functioncalling/*.rs`)の責務。
//! 元は`bot/commands/risei/riseimaterials.rs`/`riseistages.rs`にあったが、bot/apiの両方
//! (Discordコマンド・GPT function calling)から参照するためengineへ移した。

use super::stage_info::DEFAULT_SHOW_MIN_TIMES;
use super::{drop_per_minute, filter_stages_by_show_min_times, RiseiCalculatorEngine, Server, StageItem};
use crate::engine::outer_source::OuterSourceRegistry;
use std::sync::Arc;

/// 昇進素材カテゴリの1ステージ分の効率情報(Python `riseimaterials`のjsonForAI各項目相当)。
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

/// riseimaterials の計算結果一式(Discord/GPT function calling共通)。
pub struct MaterialSearchResult {
    /// `new`カテゴリ指定時は自動的にMainlandへ切り替わる。
    pub effective_server: Server,
    pub category_ja: String,
    pub main_item_value: f64,
    pub main_item_std_dev: f64,
    /// 総合効率の降順でソート済み。件数の絞り込みは呼び出し側の責務。
    pub stages: Vec<MaterialStageInfo>,
}

/// ステージが属する1カテゴリ分の効率情報。
pub struct StageCategoryEfficiency {
    pub category_ja: String,
    pub efficiency: f64,
    pub drop_per_minute: Option<f64>,
}

/// riseistages の1ステージ分の効率情報(Python `riseistages`のjsonForAI各項目相当)。
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

/// riseistages の計算結果一式(Discord/GPT function calling共通)。
pub struct StageSearchResult {
    /// グローバル版に該当ステージが無く大陸版へフォールバックした場合はMainlandになる。
    pub effective_server: Server,
    /// 名前順にソート済み。
    pub stages: Vec<StageEfficiencyInfo>,
}

impl RiseiCalculatorEngine {
    /// riseimaterials相当の計算のみを行う共通部。`new`カテゴリを指定した場合は
    /// 自動的に大陸版基準になる。整形は呼び出し側の責務。
    pub async fn material_search(
        &self,
        outer_source: &OuterSourceRegistry,
        server: Server,
        category_key: &str,
    ) -> Result<MaterialSearchResult, String> {
        let effective_server = if self.stage_category().new.contains_key(category_key) {
            Server::Mainland
        } else {
            server
        };
        let snapshot = self.snapshot(effective_server, outer_source).await;
        let cat = snapshot.category(category_key).ok_or_else(|| format!("無効なカテゴリ:{category_key}"))?;
        let info = cat.info.clone();
        let stages = filter_stages_by_show_min_times(snapshot.category_stages(category_key), DEFAULT_SHOW_MIN_TIMES);
        let promotion_items: Vec<&str> = snapshot.values.value_target[4..].to_vec();

        let mut list: Vec<MaterialStageInfo> = stages
            .iter()
            .map(|stage| {
                let items: Vec<&str> = info.items.iter().map(String::as_str).collect();
                let (time_cost, drop_per_minute_value) = if stage.min_clear_time > 0.0 {
                    (Some(stage.min_clear_time / 2.0), Some(drop_per_minute(stage, &info, &snapshot.values)))
                } else {
                    (None, None)
                };
                MaterialStageInfo {
                    name: stage.name_with_replicate(),
                    efficiency: stage.get_efficiency(&snapshot.values),
                    sanity_cost: stage.ap_cost,
                    time_cost,
                    drop_per_minute: drop_per_minute_value,
                    main_item_efficiency: stage.get_partial_efficiency(&snapshot.values, &items),
                    confidence_3sigma: snapshot.stage_dev(stage) * 3.0,
                    promotion_efficiency: stage.get_partial_efficiency(&snapshot.values, &promotion_items),
                    max_times: stage.max_times(),
                }
            })
            .collect();
        list.sort_by(|a, b| b.efficiency.total_cmp(&a.efficiency));

        Ok(MaterialSearchResult {
            effective_server,
            category_ja: info.to_ja.clone(),
            main_item_value: snapshot.values.get_value_from_zh(&info.main_item),
            main_item_std_dev: snapshot.values.get_std_dev_from_zh(&info.main_item),
            stages: list,
        })
    }

    /// riseistages相当の計算のみを行う共通部。グローバル版に該当ステージが無ければ
    /// 大陸版にフォールバックする。整形は呼び出し側の責務。
    pub async fn stage_search(
        &self,
        outer_source: &OuterSourceRegistry,
        server: Server,
        target_code: &str,
    ) -> Result<StageSearchResult, String> {
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

        stages.sort_by(|a, b| a.name.cmp(&b.name));
        let promotion_items: Vec<&str> = snapshot.values.value_target[4..].to_vec();
        let infos: Vec<StageEfficiencyInfo> = stages
            .iter()
            .map(|stage: &Arc<StageItem>| {
                let categories = snapshot
                    .stage_info
                    .stage_to_categories(stage)
                    .into_iter()
                    .filter_map(|key| {
                        let info = snapshot.category(&key)?.info.clone();
                        let items: Vec<&str> = info.items.iter().map(String::as_str).collect();
                        let efficiency = stage.get_partial_efficiency(&snapshot.values, &items);
                        let drop_per_minute_value =
                            (stage.min_clear_time > 0.0).then(|| drop_per_minute(stage, &info, &snapshot.values));
                        Some(StageCategoryEfficiency {
                            category_ja: info.to_ja,
                            efficiency,
                            drop_per_minute: drop_per_minute_value,
                        })
                    })
                    .collect();
                StageEfficiencyInfo {
                    name: stage.name_with_replicate(),
                    total_efficiency: stage.get_efficiency(&snapshot.values),
                    confidence_3sigma: snapshot.stage_dev(stage) * 3.0,
                    categories,
                    sanity_cost: stage.ap_cost,
                    time_cost: (stage.min_clear_time > 0.0).then_some(stage.min_clear_time / 2.0),
                    promotion_efficiency: stage.get_partial_efficiency(&snapshot.values, &promotion_items),
                    max_times: stage.max_times(),
                }
            })
            .collect();

        Ok(StageSearchResult {
            effective_server,
            stages: infos,
        })
    }
}
