use super::server::{
    stage_category_dict, value_target, Server, StageCategoryFile, StageCategoryInfo,
};
use super::stage::StageItem;
use super::values::RiseiValues;
use crate::engine::external_source::ark_matrix::ArkMatrix;
use crate::engine::external_source::ark_stages::ArkStages;
use crate::engine::external_source::item_names::ItemNames;
use crate::engine::external_source::zones::Zones;
use rand::seq::IndexedRandom;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// オートコンプリート等で使うデフォルトの最小試行数。
pub const DEFAULT_SHOW_MIN_TIMES: i64 = 1000;

/// `generate_category_seed` の重複リトライ上限。カテゴリ定義次第では重複が構造的に解消しない
/// 場合があり得るため、テスト・実運用双方が無限ループしないよう上限を設ける。
const MAX_SEED_GENERATION_ATTEMPTS: usize = 10_000;

pub struct CategoryInstance {
    pub info: StageCategoryInfo,
    pub stage_ids: Vec<String>,
}

/// 最大効率のステージとその所属カテゴリ。
pub struct CategoryMaxEfficiencyItem {
    pub max_value: f64,
    pub stage: Arc<StageItem>,
    pub categories: Vec<String>,
}

/// サーバ1つ分のステージ全体像。penguin-statsの生データ
/// (`ArkStages`/`ArkMatrix`)から構築する不変のスナップショット。再fetch後は
/// 作り直す（ドロップ情報だけを差分更新する最適化は行わず、
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
    /// 生データからステージ情報を構築する。
    ///
    /// penguin-statsの`/result/matrix`は本編ステージでも、新章実装直後の一時的なブースト期間の
    /// レコード（`end`が設定され、他アイテムと桁違いの`times`を持つ）を返し続けることがある
    /// （例: 14章の酮凝集組、`end=1715716800000`）。合算すると`max_times`やドロップ率が汚染され
    /// 基準マップ差し替えが収束しなくなるため、`end`付きレコードはMAIN/SUBでは読み飛ばす。
    /// ACTIVITYステージは元々期間限定運用なので対象外。
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

        // グローバル版のみ、大陸先行実装ゾーンを除外する。
        let exclusion_zones: &[&str] = if server == Server::Global {
            super::server::NEW_ZONE_MAINLAND_ONLY
        } else {
            &[]
        };

        let mut main_stage_dict: HashMap<String, StageItem> = HashMap::new();
        let mut event_stage_dict: HashMap<String, StageItem> = HashMap::new();

        for raw in &ark_stages.stages {
            let is_main_or_sub = raw.stage_type == "MAIN" || raw.stage_type == "SUB";
            let is_permanent_activity =
                raw.stage_type == "ACTIVITY" && raw.zone_id.contains("permanent");
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
                if record.end.is_none() {
                    stage.add_drop_record(record, &value_target_list, &item_names);
                }
            } else if let Some(stage) = event_stage_dict.get_mut(&record.stage_id) {
                stage.add_drop_record(record, &value_target_list, &item_names);
            }
        }

        let main_stage_dict: HashMap<String, Arc<StageItem>> = main_stage_dict
            .into_iter()
            .map(|(k, v)| (k, Arc::new(v)))
            .collect();
        let event_stage_dict: HashMap<String, Arc<StageItem>> = event_stage_dict
            .into_iter()
            .map(|(k, v)| (k, Arc::new(v)))
            .collect();

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
    pub fn category_valid_stages(
        &self,
        category: &str,
        valid_base_min_times: i64,
    ) -> Vec<Arc<StageItem>> {
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
    /// （重複した場合はやり直す）。
    /// `MAX_SEED_GENERATION_ATTEMPTS` 回リトライしても重複が解消しない場合はエラーを返す
    /// （カテゴリ定義が競合していて構造的に解消不可能なケースの安全弁。詳細は定数コメント参照）。
    pub fn generate_category_seed(
        &self,
        valid_base_min_times: i64,
    ) -> Result<HashMap<String, Arc<StageItem>>, super::Error> {
        let mut rng = rand::rng();
        for _ in 0..MAX_SEED_GENERATION_ATTEMPTS {
            let mut ret = HashMap::new();
            for key in self.category_instance_dict.keys() {
                let candidates = self.category_valid_stages(key, valid_base_min_times);
                if let Some(choice) = candidates.choose(&mut rng) {
                    ret.insert(key.clone(), choice.clone());
                }
            }
            let mut seen = HashSet::new();
            let has_dup = ret
                .values()
                .any(|stage| !seen.insert(stage.stage_id.clone()));
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

    /// 全基準候補ステージの中で最大効率のものを探す。
    pub fn get_category_max_efficiency(
        &self,
        values: &RiseiValues,
        valid_base_min_times: i64,
    ) -> Option<CategoryMaxEfficiencyItem> {
        let mut best: Option<(f64, Arc<StageItem>)> = None;
        for stage in self.valid_base_stages(valid_base_min_times) {
            let eff = stage.get_efficiency(values);
            if best.as_ref().is_none_or(|(v, _)| eff > *v) {
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

    pub fn search_event_stage(
        &self,
        target_code: &str,
        show_min_times: i64,
    ) -> Vec<Arc<StageItem>> {
        self.event_code_to_stage
            .iter()
            .filter(|(code, _)| code.starts_with(target_code))
            .map(|(_, s)| s.clone())
            .filter(|s| s.is_valid_for_show(show_min_times, &self.value_target, &self.item_names))
            .collect()
    }

    pub fn auto_complete_main_stage(
        &self,
        target_code: &str,
        limit: usize,
    ) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = self
            .search_main_stage(target_code, DEFAULT_SHOW_MIN_TIMES)
            .into_iter()
            .map(|s| {
                let code = s.name_with_replicate();
                (
                    format!("{code}{}", s.get_main_drop_ja_str(&self.item_names)),
                    code,
                )
            })
            .collect();
        result.truncate(limit);
        result
    }

    pub fn auto_complete_event_stage(
        &self,
        target_code: &str,
        limit: usize,
    ) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = self
            .search_event_stage(target_code, DEFAULT_SHOW_MIN_TIMES)
            .into_iter()
            .map(|s| {
                let code = s.name_with_replicate();
                (
                    format!("{code}{}", s.get_main_drop_ja_str(&self.item_names)),
                    code,
                )
            })
            .collect();
        result.truncate(limit);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::ark_matrix::RawDropRecord;
    use crate::engine::external_source::ark_stages::{RawDropInfo, RawStage};

    const ITEM_ID: &str = "item_a";
    const ITEM_ZH: &str = "基础作战记录";

    fn item_names() -> Arc<ItemNames> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id_to_ja": {ITEM_ID: "ja_a"},
                "id_to_zh": {ITEM_ID: ITEM_ZH},
                "zh_to_id": {ITEM_ZH: ITEM_ID},
                "ja_to_id": {"ja_a": ITEM_ID},
            }))
            .unwrap(),
        )
    }

    fn raw_stage(stage_id: &str, stage_type: &str, zone_id: &str) -> RawStage {
        RawStage {
            code: stage_id.to_string(),
            zone_id: zone_id.to_string(),
            stage_id: stage_id.to_string(),
            ap_cost: Some(10),
            stage_type: stage_type.to_string(),
            min_clear_time: None,
            drop_infos: Vec::new(),
        }
    }

    fn record(stage_id: &str, times: i64, quantity: f64, end: Option<i64>) -> RawDropRecord {
        RawDropRecord {
            stage_id: stage_id.to_string(),
            item_id: ITEM_ID.to_string(),
            times,
            quantity,
            start: 0,
            end,
        }
    }

    fn build(stages: Vec<RawStage>, records: Vec<RawDropRecord>) -> StageInfo {
        let category_file = StageCategoryFile {
            main: BTreeMap::new(),
            new: BTreeMap::new(),
        };
        StageInfo::build(
            Server::Mainland,
            &category_file,
            &ArkStages { stages },
            &ArkMatrix { matrix: records },
            &Zones::default(),
            item_names(),
            &HashMap::new(),
        )
    }

    /// 本編ステージに期間限定（`end`付き）レコードを混ぜると、桁違いの`times`が
    /// `max_times`やドロップ率を汚染して基準マップの差し替えが収束しなくなる。
    #[test]
    fn main_stage_ignores_period_limited_records() {
        let info = build(
            vec![raw_stage("main_x", "MAIN", "zone1")],
            vec![
                record("main_x", 1_000_000, 900_000.0, Some(1_715_716_800_000)),
                record("main_x", 2000, 1000.0, None),
            ],
        );
        let stage = &info.main_stage_dict["main_x"];
        assert_eq!(stage.max_times(), 2000);
        let rate = stage.get_drop_rate(ITEM_ZH, &info.value_target, &info.item_names);
        assert!((rate - 0.5).abs() < 1e-9, "rate = {rate}");
    }

    #[test]
    fn sub_stage_ignores_period_limited_records() {
        let info = build(
            vec![raw_stage("sub_x", "SUB", "zone1")],
            vec![record("sub_x", 1_000_000, 900_000.0, Some(1))],
        );
        assert_eq!(info.main_stage_dict["sub_x"].max_times(), 0);
    }

    #[test]
    fn event_stage_accepts_period_limited_records() {
        let info = build(
            vec![raw_stage("act_ev", "ACTIVITY", "act1side")],
            vec![
                record("act_ev", 1_000_000, 900_000.0, Some(1)),
                record("act_ev", 2000, 1000.0, None),
            ],
        );
        let stage = &info.event_stage_dict["act_ev"];
        assert_eq!(stage.max_times(), 1_000_000);
        // 同じアイテムは後から来たレコードで上書きされる。
        let rate = stage.get_drop_rate(ITEM_ZH, &info.value_target, &info.item_names);
        assert!((rate - 0.5).abs() < 1e-9, "rate = {rate}");
    }

    #[test]
    fn event_stage_uses_period_limited_record_when_alone() {
        let info = build(
            vec![raw_stage("act_ev", "ACTIVITY", "act1side")],
            vec![record("act_ev", 1000, 900.0, Some(1))],
        );
        let stage = &info.event_stage_dict["act_ev"];
        let rate = stage.get_drop_rate(ITEM_ZH, &info.value_target, &info.item_names);
        assert!((rate - 0.9).abs() < 1e-9, "rate = {rate}");
    }

    fn build_with_categories(
        categories: &[(&str, &[&str])],
        stages: Vec<RawStage>,
        records: Vec<RawDropRecord>,
    ) -> StageInfo {
        let main = categories
            .iter()
            .map(|(key, codes)| {
                (
                    key.to_string(),
                    StageCategoryInfo {
                        stages: codes.iter().map(|c| c.to_string()).collect(),
                        items: Vec::new(),
                        main_item: String::new(),
                        to_ja: String::new(),
                        sub_item: Vec::new(),
                        sub_order: Vec::new(),
                    },
                )
            })
            .collect();
        StageInfo::build(
            Server::Mainland,
            &StageCategoryFile {
                main,
                new: BTreeMap::new(),
            },
            &ArkStages { stages },
            &ArkMatrix { matrix: records },
            &Zones::default(),
            item_names(),
            &HashMap::new(),
        )
    }

    fn event_stage_ids(stage_type: &str, zone_id: &str, stage_id: &str) -> Vec<String> {
        let info = build(vec![raw_stage(stage_id, stage_type, zone_id)], Vec::new());
        info.event_stage_dict.keys().cloned().collect()
    }

    #[test]
    fn event_stage_must_be_an_activity_stage() {
        assert!(event_stage_ids("SUB", "tough_act1", "s").is_empty());
        assert_eq!(event_stage_ids("ACTIVITY", "act1side", "s"), ["s"]);
    }

    #[test]
    fn permanent_activity_zone_is_never_an_event_stage() {
        assert!(event_stage_ids("ACTIVITY", "tough_permanent_act", "s").is_empty());
    }

    #[test]
    fn event_stage_zone_must_contain_act() {
        assert!(event_stage_ids("ACTIVITY", "zone_x", "s").is_empty());
    }

    #[test]
    fn gacha_stage_is_never_an_event_stage() {
        assert!(event_stage_ids("ACTIVITY", "act1side", "act_gacha_1").is_empty());
    }

    #[test]
    fn act10d5_zone_is_never_an_event_stage() {
        assert!(event_stage_ids("ACTIVITY", "act10d5side", "s").is_empty());
    }

    #[test]
    fn category_max_efficiency_keeps_the_first_stage_on_a_tie() {
        let info = build_with_categories(
            &[("a_cat", &["s1"]), ("b_cat", &["s2"])],
            vec![
                raw_stage("s1", "MAIN", "zone1"),
                raw_stage("s2", "MAIN", "zone1"),
            ],
            vec![
                record("s1", 2000, 1000.0, None),
                record("s2", 2000, 1000.0, None),
            ],
        );
        let target = value_target(Server::Mainland);
        let mut value_array = nalgebra::DVector::zeros(target.len());
        value_array[target.iter().position(|x| *x == ITEM_ZH).unwrap()] = 2.0;
        let values = RiseiValues::new(
            Server::Mainland,
            value_array,
            item_names(),
            Arc::new(HashMap::new()),
        );
        let best = info.get_category_max_efficiency(&values, 1000).unwrap();
        assert_eq!(best.stage.stage_id, "s1");
        assert_eq!(best.categories, ["a_cat"]);
        // 2.0 * 0.5 / 10
        assert!((best.max_value - 0.1).abs() < 1e-9);
    }

    fn with_main_drop(mut stage: RawStage, id: &str) -> RawStage {
        stage.drop_infos.push(RawDropInfo {
            item_id: Some(id.to_string()),
            drop_type: "NORMAL_DROP".to_string(),
        });
        stage
    }

    #[test]
    fn auto_complete_main_stage_returns_label_with_main_drop_and_code() {
        let mut re = raw_stage("main_re", "MAIN", "main_re_1");
        re.code = "1-7".to_string();
        let info = build(
            vec![with_main_drop(re, ITEM_ID)],
            vec![record("main_re", 2000, 1000.0, None)],
        );
        assert_eq!(
            info.auto_complete_main_stage("1-", 10),
            [("1-7(Re) ja_a".to_string(), "1-7(Re)".to_string())]
        );
        assert!(info.auto_complete_main_stage("9-", 10).is_empty());
    }

    #[test]
    fn auto_complete_main_stage_truncates_to_limit() {
        let info = build(
            vec![
                raw_stage("m1", "MAIN", "zone1"),
                raw_stage("m2", "MAIN", "zone1"),
            ],
            vec![
                record("m1", 2000, 1000.0, None),
                record("m2", 2000, 1000.0, None),
            ],
        );
        assert_eq!(info.auto_complete_main_stage("m", 10).len(), 2);
        assert_eq!(info.auto_complete_main_stage("m", 1).len(), 1);
    }

    #[test]
    fn auto_complete_event_stage_returns_label_with_main_drop_and_code() {
        let info = build(
            vec![with_main_drop(
                raw_stage("act_ev", "ACTIVITY", "act1side"),
                ITEM_ID,
            )],
            vec![record("act_ev", 2000, 1000.0, None)],
        );
        assert_eq!(
            info.auto_complete_event_stage("act", 10),
            [("act_ev ja_a".to_string(), "act_ev".to_string())]
        );
        assert!(info.auto_complete_event_stage("zzz", 10).is_empty());
    }

    #[test]
    fn auto_complete_event_stage_truncates_to_limit() {
        let info = build(
            vec![
                raw_stage("act_1", "ACTIVITY", "act1side"),
                raw_stage("act_2", "ACTIVITY", "act1side"),
            ],
            vec![
                record("act_1", 2000, 1000.0, None),
                record("act_2", 2000, 1000.0, None),
            ],
        );
        assert_eq!(info.auto_complete_event_stage("act", 10).len(), 2);
        assert_eq!(info.auto_complete_event_stage("act", 1).len(), 1);
    }
}
