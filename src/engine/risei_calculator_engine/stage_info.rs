use super::server::{stage_category_dict, value_target, Server, StageCategoryFile, StageCategoryInfo};
use super::stage::StageItem;
use super::values::RiseiValues;
use crate::engine::external_source::ark_matrix::ArkMatrix;
use crate::engine::external_source::ark_stages::ArkStages;
use crate::engine::external_source::item_names::ItemNames;
use crate::engine::external_source::zones::Zones;
use rand::seq::SliceRandom;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// オートコンプリート等で使うデフォルトの最小試行数（Python `DEFAULT_SHOW_MIN_TIMES`）。
pub const DEFAULT_SHOW_MIN_TIMES: i64 = 1000;

/// `generate_category_seed` の重複リトライ上限。Python版は無条件でリトライし続けるが
/// （`while(True)` + `hasduplicates`）、カテゴリ定義次第では重複が構造的に解消しない
/// 場合があり得るため、テスト・実運用双方が無限ループしないよう上限を設ける。
const MAX_SEED_GENERATION_ATTEMPTS: usize = 10_000;

pub struct CategoryInstance {
    pub info: StageCategoryInfo,
    pub stage_ids: Vec<String>,
}

/// 最大効率のステージとその所属カテゴリ（Python `StageInfo.CategoryMaxEfficiencyItem`）。
pub struct CategoryMaxEfficiencyItem {
    pub max_value: f64,
    pub stage: Arc<StageItem>,
    pub categories: Vec<String>,
}

/// サーバ1つ分のステージ全体像（Python `StageInfo`）。penguin-statsの生データ
/// (`ArkStages`/`ArkMatrix`)から構築する不変のスナップショット。再fetch後は
/// 作り直す（Python版の「ドロップ情報のみ更新」という部分更新最適化は行わず、
/// 毎回フルに再構築する。データ量的にコストは無視できるためシンプルさを優先）。
pub struct StageInfo {
    pub server: Server,
    pub item_names: Arc<ItemNames>,
    pub value_target: Vec<&'static str>,
    pub main_stage_dict: HashMap<String, Arc<StageItem>>,
    pub event_stage_dict: HashMap<String, Arc<StageItem>>,
    pub main_code_to_stage: HashMap<String, Arc<StageItem>>,
    pub event_code_to_stage: HashMap<String, Arc<StageItem>>,
    pub category_instance_dict: BTreeMap<String, CategoryInstance>,
}

impl StageInfo {
    /// Python `StageInfo.init`/`initMatrix` に相当。
    ///
    /// `eventMainList`（新章実装直後の期間限定ドロップ倍率ウィンドウを`(Event)`ステージへ
    /// 退避する仕組み）自体は移植していない（対象の新章はとうに実装済みで
    /// `eventMainDict`に一致するウィンドウが来ることはない）。
    /// ただし、この仕組みが担っていた「MAIN/SUBステージ向けの`end`付き（期間限定）
    /// matrixレコードは通常のdropListに混ぜない」という前提は今も有効。
    /// penguin-statsの`/result/matrix`は本編ステージであっても、新章実装直後の
    /// 一時的なブースト期間のレコード（`end`が設定され、他アイテムと桁違いの
    /// `times`を持つ）を返し続けることがある（例: 14章実装時の酮凝集組ドロップ、
    /// `end=1715716800000`のレコードが実装から1年以上経った現在も残っている）。
    /// これをそのまま合算するとそのステージの`maxTimes`や特定アイテムのドロップ率が
    /// 汚染され、基準マップ差し替えループが収束しなくなる実害があったため、
    /// `end`付きレコードはMAIN/SUBステージについては読み飛ばす
    /// （Python版が`(Event)`ステージへ退避して本編ステージのdropListに
    /// 混ぜないのと同じ効果）。ACTIVITYステージ（`event_stage_dict`）は
    /// 元々期間限定運用が前提のため対象外（Python版と同じ）。
    pub fn build(
        server: Server,
        stage_category_file: &StageCategoryFile,
        ark_stages: &ArkStages,
        ark_matrix: &ArkMatrix,
        zones: &Zones,
        item_names: Arc<ItemNames>,
        min_clear_time_injection: &HashMap<String, f64>,
    ) -> Self {
        let value_target_list = value_target(server);

        // グローバル版のみ、大陸先行実装ゾーンを除外する（Python `new_zone if isGlobal else []`）。
        let exclusion_zones: &[&str] = if server == Server::Global {
            super::server::NEW_ZONE_MAINLAND_ONLY
        } else {
            &[]
        };

        let mut main_stage_dict: HashMap<String, StageItem> = HashMap::new();
        let mut event_stage_dict: HashMap<String, StageItem> = HashMap::new();

        for raw in &ark_stages.stages {
            let is_main_or_sub = raw.stage_type == "MAIN" || raw.stage_type == "SUB";
            let is_permanent_activity = raw.stage_type == "ACTIVITY" && raw.zone_id.contains("permanent");
            if is_main_or_sub || is_permanent_activity {
                let excluded = exclusion_zones.contains(&raw.zone_id.as_str())
                    || raw.zone_id == "recruit"
                    || raw.zone_id.contains("tough");
                if !excluded {
                    let zone_name = zones.get_str(&raw.zone_id).to_string();
                    main_stage_dict.insert(
                        raw.stage_id.clone(),
                        StageItem::from_raw(raw, zone_name, min_clear_time_injection),
                    );
                    continue;
                }
            }
            let is_event = raw.stage_type == "ACTIVITY"
                && !raw.zone_id.contains("permanent")
                && raw.zone_id.contains("act")
                && !raw.stage_id.contains("gacha")
                && !raw.zone_id.contains("act10d5");
            if is_event {
                let zone_name = zones.get_str(&raw.zone_id).to_string();
                event_stage_dict.insert(
                    raw.stage_id.clone(),
                    StageItem::from_raw(raw, zone_name, min_clear_time_injection),
                );
            }
        }

        for record in &ark_matrix.matrix {
            if let Some(stage) = main_stage_dict.get_mut(&record.stage_id) {
                // 期間限定（`end`付き）レコードはMAIN/SUBの通常dropListに混ぜない
                // （このimpl冒頭のコメント参照）。
                if record.end.is_none() {
                    stage.add_drop_record(record, &value_target_list, &item_names);
                }
            } else if let Some(stage) = event_stage_dict.get_mut(&record.stage_id) {
                stage.add_drop_record(record, &value_target_list, &item_names);
            }
        }

        let main_stage_dict: HashMap<String, Arc<StageItem>> =
            main_stage_dict.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        let event_stage_dict: HashMap<String, Arc<StageItem>> =
            event_stage_dict.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();

        let main_code_to_stage: HashMap<String, Arc<StageItem>> = main_stage_dict
            .values()
            .map(|s| (s.name_with_replicate(), s.clone()))
            .collect();
        let event_code_to_stage: HashMap<String, Arc<StageItem>> = event_stage_dict
            .values()
            .map(|s| (s.name_with_replicate(), s.clone()))
            .collect();

        let category_dict = stage_category_dict(stage_category_file, server);
        let category_instance_dict: BTreeMap<String, CategoryInstance> = category_dict
            .into_iter()
            .map(|(key, info)| {
                let stage_ids = info
                    .stages
                    .iter()
                    .filter_map(|code| main_code_to_stage.get(code).map(|s| s.stage_id.clone()))
                    .collect();
                (key, CategoryInstance { info, stage_ids })
            })
            .collect();

        Self {
            server,
            item_names,
            value_target: value_target_list,
            main_stage_dict,
            event_stage_dict,
            main_code_to_stage,
            event_code_to_stage,
            category_instance_dict,
        }
    }

    /// カテゴリに属する有効ステージ（試行数フィルター後、全滅した場合は無条件で全部返す）。
    pub fn category_valid_stages(&self, category: &str, valid_base_min_times: i64) -> Vec<Arc<StageItem>> {
        let Some(cat) = self.category_instance_dict.get(category) else {
            return Vec::new();
        };
        let all: Vec<Arc<StageItem>> = cat
            .stage_ids
            .iter()
            .filter_map(|id| self.main_stage_dict.get(id).cloned())
            .collect();
        let valid: Vec<Arc<StageItem>> = all
            .iter()
            .filter(|s| s.max_times() >= valid_base_min_times)
            .cloned()
            .collect();
        if valid.is_empty() {
            all
        } else {
            valid
        }
    }

    pub fn valid_base_stages(&self, valid_base_min_times: i64) -> Vec<Arc<StageItem>> {
        self.category_instance_dict
            .keys()
            .flat_map(|cat| self.category_valid_stages(cat, valid_base_min_times))
            .collect()
    }

    pub fn stage_to_categories(&self, stage: &Arc<StageItem>) -> Vec<String> {
        self.category_instance_dict
            .iter()
            .filter(|(_, inst)| inst.stage_ids.contains(&stage.stage_id))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// カテゴリごとにランダムなステージを選び、基準マップの初期seedを作る
    /// （重複した場合はやり直す。Python `generateCategorySeed`）。
    /// `MAX_SEED_GENERATION_ATTEMPTS` 回リトライしても重複が解消しない場合はエラーを返す
    /// （カテゴリ定義が競合していて構造的に解消不可能なケースの安全弁。詳細は定数コメント参照）。
    pub fn generate_category_seed(
        &self,
        valid_base_min_times: i64,
    ) -> Result<HashMap<String, Arc<StageItem>>, super::Error> {
        let mut rng = rand::thread_rng();
        for _ in 0..MAX_SEED_GENERATION_ATTEMPTS {
            let mut ret = HashMap::new();
            for key in self.category_instance_dict.keys() {
                let candidates = self.category_valid_stages(key, valid_base_min_times);
                if let Some(choice) = candidates.choose(&mut rng) {
                    ret.insert(key.clone(), choice.clone());
                }
            }
            let mut seen = HashSet::new();
            let has_dup = ret.values().any(|stage| !seen.insert(stage.stage_id.clone()));
            if !has_dup {
                return Ok(ret);
            }
        }
        Err(format!(
            "基準マップの初期seed生成が{MAX_SEED_GENERATION_ATTEMPTS}回のリトライでも重複を解消できませんでした。\
             カテゴリ定義（data/risei/stage_category.json）で複数カテゴリが同じステージしか候補に持てない状態になっている可能性があります"
        )
        .into())
    }

    /// 全基準候補ステージの中で最大効率のものを探す（Python `getCategoryMaxEfficiency`）。
    pub fn get_category_max_efficiency(
        &self,
        values: &RiseiValues,
        valid_base_min_times: i64,
    ) -> Option<CategoryMaxEfficiencyItem> {
        let mut best: Option<(f64, Arc<StageItem>)> = None;
        for stage in self.valid_base_stages(valid_base_min_times) {
            let eff = stage.get_efficiency(values);
            if best.as_ref().map_or(true, |(v, _)| eff > *v) {
                best = Some((eff, stage));
            }
        }
        let (max_value, max_stage) = best?;
        let categories = self.stage_to_categories(&max_stage);
        Some(CategoryMaxEfficiencyItem {
            max_value,
            stage: max_stage,
            categories,
        })
    }

    pub fn search_main_stage(&self, target_code: &str, show_min_times: i64) -> Vec<Arc<StageItem>> {
        self.main_code_to_stage
            .iter()
            .filter(|(code, _)| code.starts_with(target_code))
            .map(|(_, s)| s.clone())
            .filter(|s| s.is_valid_for_show(show_min_times, &self.value_target, &self.item_names))
            .collect()
    }

    pub fn search_event_stage(&self, target_code: &str, show_min_times: i64) -> Vec<Arc<StageItem>> {
        self.event_code_to_stage
            .iter()
            .filter(|(code, _)| code.starts_with(target_code))
            .map(|(_, s)| s.clone())
            .filter(|s| s.is_valid_for_show(show_min_times, &self.value_target, &self.item_names))
            .collect()
    }

    pub fn auto_complete_main_stage(&self, target_code: &str, limit: usize) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = self
            .search_main_stage(target_code, DEFAULT_SHOW_MIN_TIMES)
            .into_iter()
            .map(|s| {
                let code = s.name_with_replicate();
                (format!("{code}{}", s.get_main_drop_ja_str(&self.item_names)), code)
            })
            .collect();
        result.truncate(limit);
        result
    }

    pub fn auto_complete_event_stage(&self, target_code: &str, limit: usize) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = self
            .search_event_stage(target_code, DEFAULT_SHOW_MIN_TIMES)
            .into_iter()
            .map(|s| {
                let code = s.name_with_replicate();
                (format!("{code}{}", s.get_main_drop_ja_str(&self.item_names)), code)
            })
            .collect();
        result.truncate(limit);
        result
    }
}
