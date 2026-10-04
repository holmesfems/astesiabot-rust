//! riseimaterials/riseistages/riseievents 相当のステージ検索DTO+計算(Python
//! `CalculatorManager.riseimaterials`/`riseistages`/`riseievents`の計算部相当)。整形(Discord
//! embed化/GPT向けJSON化/REST API)は呼び出し側(`bot/commands/risei/*.rs` /
//! `bot/services/uranai/functioncalling/*.rs` / `api/risei.rs`)の責務。
//! 元は`bot/commands/risei/riseimaterials.rs`等にあったが、bot/apiの両方から参照するため
//! engineへ移した。DTOは`Serialize`を実装し、REST APIはこれをそのままJSONで返す
//! (xlsx出力専用の`raw`は`#[serde(skip)]`)。

use super::stage_info::DEFAULT_SHOW_MIN_TIMES;
use super::{drop_per_minute, filter_stages_by_show_min_times, RiseiCalculatorEngine, Server, StageItem};
use serde::Serialize;
use std::sync::Arc;

/// 昇進素材カテゴリの1ステージ分の効率情報(Python `riseimaterials`のjsonForAI各項目相当)。
#[derive(Serialize)]
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
    /// xlsx出力(`csv_file`オプション)専用。ソート・truncate後も対応関係が
    /// 崩れないよう、表示用フィールドと同じ要素にraw StageItemを畳み込んでいる。
    #[serde(skip)]
    pub raw: Arc<StageItem>,
}

/// riseimaterials の計算結果一式(Discord/GPT function calling共通)。
#[derive(Serialize)]
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
#[derive(Serialize)]
pub struct StageCategoryEfficiency {
    pub category_ja: String,
    pub efficiency: f64,
    pub drop_per_minute: Option<f64>,
}

/// riseistages の1ステージ分の効率情報(Python `riseistages`のjsonForAI各項目相当)。
#[derive(Serialize)]
pub struct StageEfficiencyInfo {
    pub name: String,
    pub total_efficiency: f64,
    pub confidence_3sigma: f64,
    pub categories: Vec<StageCategoryEfficiency>,
    pub sanity_cost: f64,
    pub time_cost: Option<f64>,
    pub promotion_efficiency: f64,
    pub max_times: i64,
    /// xlsx出力(`csv_file`オプション)専用。[`MaterialStageInfo::raw`]と同じ理由で
    /// 表示用フィールドと同じ要素に畳み込んでいる。
    #[serde(skip)]
    pub raw: Arc<StageItem>,
}

/// riseistages の計算結果一式(Discord/GPT function calling共通)。
#[derive(Serialize)]
pub struct StageSearchResult {
    /// グローバル版に該当ステージが無く大陸版へフォールバックした場合はMainlandになる。
    pub effective_server: Server,
    /// 名前順にソート済み。
    pub stages: Vec<StageEfficiencyInfo>,
}

/// riseievents の1ステージ分の効率情報（Python `riseievents`のjsonForAI各項目相当）。
/// 元は`bot/commands/risei/riseievents.rs`にあった。
#[derive(Serialize)]
pub struct EventStageInfo {
    pub name: String,
    pub zone_name: String,
    pub total_efficiency: f64,
    pub main_drop_name: String,
    pub main_drop_rate: f64,
    pub max_times: i64,
    pub sanity_cost: f64,
    /// 倍速時のクリア時間(秒)。ステージにクリア時間データが無ければ`None`。
    pub time_cost: Option<f64>,
    pub drop_per_minute: Option<f64>,
    /// xlsx出力(`csv_file`オプション)専用。[`MaterialStageInfo::raw`]と同じ理由で
    /// 表示用フィールドと同じ要素に畳み込んでいる。
    #[serde(skip)]
    pub raw: Arc<StageItem>,
}

impl RiseiCalculatorEngine {
    /// 素材カテゴリの指定(キー`main_x`/`new_x`、または日本語名`to_ja`)からカテゴリキーを
    /// 解決する。キー完全一致→日本語名完全一致→部分一致(Python版`estimateCategoryFromJPName`の
    /// `current in to_ja or to_ja in current`相当)の順で、AIの表記ゆれを吸収する。
    /// GPT function calling(uranai)とREST API(`api/risei.rs`)の共通部。
    pub fn resolve_category_key(&self, target: &str) -> Option<String> {
        let file = self.stage_category();
        let entries: Vec<(&String, &str)> = file
            .main
            .iter()
            .chain(file.new.iter())
            .map(|(key, info)| (key, info.to_ja.as_str()))
            .collect();
        entries
            .iter()
            .find(|(key, _)| key.as_str() == target)
            .or_else(|| entries.iter().find(|(_, to_ja)| *to_ja == target))
            .or_else(|| {
                entries
                    .iter()
                    .find(|(_, to_ja)| !target.is_empty() && (target.contains(*to_ja) || to_ja.contains(target)))
            })
            .map(|(key, _)| (*key).clone())
    }

    /// riseimaterials相当の計算のみを行う共通部。`new`カテゴリを指定した場合は
    /// 自動的に大陸版基準になる。整形は呼び出し側の責務。
    pub async fn material_search(
        &self,
        server: Server,
        category_key: &str,
    ) -> Result<MaterialSearchResult, String> {
        let effective_server = if self.stage_category().new.contains_key(category_key) {
            Server::Mainland
        } else {
            server
        };
        let snapshot = self.snapshot(effective_server).await;
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
                    raw: stage.clone(),
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
        server: Server,
        target_code: &str,
    ) -> Result<StageSearchResult, String> {
        let mut snapshot = self.snapshot(server).await;
        let mut stages = snapshot.search_main_stage(target_code);
        let mut effective_server = server;
        if stages.is_empty() && server == Server::Global {
            snapshot = self.snapshot(Server::Mainland).await;
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
                    raw: stage.clone(),
                }
            })
            .collect();

        Ok(StageSearchResult {
            effective_server,
            stages: infos,
        })
    }

    /// riseievents相当の計算のみを行う共通部。名前順にソート済み。整形は呼び出し側の責務。
    pub async fn event_search(&self, server: Server, target_code: &str) -> Result<Vec<EventStageInfo>, String> {
        let snapshot = self.snapshot(server).await;
        let mut stages = snapshot.search_event_stage(target_code);
        if stages.is_empty() {
            return Err(format!("無効なステージ指定{target_code}"));
        }

        stages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(stages
            .iter()
            .filter_map(|stage| {
                let (main_drop_name, main_drop_rate) = stage.get_max_efficiency_item(&snapshot.values.item_names)?;
                let time_cost = (stage.min_clear_time > 0.0).then_some(stage.min_clear_time / 2.0);
                let drop_per_minute_value =
                    (stage.min_clear_time > 0.0).then(|| main_drop_rate / stage.min_clear_time * 120.0);
                Some(EventStageInfo {
                    name: stage.name_with_replicate(),
                    zone_name: stage.zone_name.clone(),
                    total_efficiency: stage.get_efficiency(&snapshot.values),
                    main_drop_name,
                    main_drop_rate,
                    max_times: stage.max_times(),
                    sanity_cost: stage.ap_cost,
                    time_cost,
                    drop_per_minute: drop_per_minute_value,
                    raw: stage.clone(),
                })
            })
            .collect())
    }
}
