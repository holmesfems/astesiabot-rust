pub mod calculator;
pub mod formula;
pub mod item_array;
pub mod kakin;
pub mod lists;
pub mod search;
pub mod server;
pub mod stage;
pub mod stage_info;
pub mod static_data;
pub mod values;

pub use lists::{TicketEfficiency, ValueEntry, CC_NUMBER};
pub use search::{EventStageInfo, MaterialSearchResult, MaterialStageInfo, StageCategoryEfficiency, StageEfficiencyInfo, StageSearchResult};
pub use server::Server;
pub use stage::StageItem;

use crate::engine::external_source::ExternalSourceRegistry;
use calculator::Calculator;
use chrono::{DateTime, Utc};
use stage_info::{CategoryInstance, DEFAULT_SHOW_MIN_TIMES};
use static_data::StaticData;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use values::RiseiValues;

/// risei_calculator_engine ドメイン共通のエラー型（bot にも api にも依存しない）。
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// 理性価値表をバックグラウンドで再計算する間隔（120分）。
/// 更新と応答を切り分け、main.rsのループがこの間隔で[`RiseiCalculatorEngine::refresh`]を呼ぶ。
/// リクエスト側([`RiseiCalculatorEngine::snapshot`])は常にキャッシュを読むだけ。
pub const REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(120 * 60);
/// 基準マップとして選ばれるために必要な最小試行数。
const DEFAULT_BASE_MIN_TIMES: i64 = 3000;

/// 理性価値表の計算エンジン（facade）。
/// bot/api には依存しない。グローバル版・大陸版それぞれの計算結果を
/// 内部で保持する。参照([`Self::snapshot`])と更新([`Self::refresh`]。main.rsが
/// [`REFRESH_INTERVAL`]毎に呼ぶ)は切り分けてあり、参照側は再計算を起こさない。
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

    /// ark_stages/ark_matrix を再fetchしてから両サーバ分を丸ごと再計算する
    /// （main.rsの定期ループから[`REFRESH_INTERVAL`]毎に呼ぶ）。
    /// 再計算に失敗したサーバは既存の値を保持したまま継続する（`outer_source::Source::refresh`と
    /// 同じ方針）。計算中もリクエストは旧スナップショットを読み続け、完成後に一瞬で差し替える。
    pub async fn refresh(&self, outer_source: &ExternalSourceRegistry) {
        tokio::join!(outer_source.ark_stages.refresh(), outer_source.ark_matrix.refresh());
        for server in [Server::Global, Server::Mainland] {
            match build_calculator(server, outer_source, &self.static_data).await {
                Ok(new_calc) => {
                    *self.lock_for(server).write().await = new_calc;
                }
                Err(e) => {
                    eprintln!("[risei_calculator_engine] {server:?}の再計算に失敗、既存の理性価値表を保持します: {e}");
                }
            }
        }
    }

    /// 表示・検索に必要な情報一式のスナップショットを取得する。常にキャッシュを読むだけで、
    /// fetchや再計算は行わない（更新は[`Self::refresh`]の責務）。
    pub async fn snapshot(&self, server: Server) -> EngineSnapshot {
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
            last_updated: calc.last_updated,
        }
    }

    /// 指定サーバの理性価値表を計算した時刻（REST APIの`updated_at`用）。
    pub async fn last_updated(&self, server: Server) -> DateTime<Utc> {
        self.lock_for(server).read().await.last_updated
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
    /// この理性価値表を計算した時刻。
    pub last_updated: DateTime<Utc>,
}

/// カテゴリのmainItem/subItemドロップ率を`120秒`あたりの入手数に換算する。
/// riseimaterials/riseistagesの両方が使う汎用計算のためengineに残す。
pub fn drop_per_minute(stage: &StageItem, info: &server::StageCategoryInfo, values: &RiseiValues) -> f64 {
    let mut drop_values = stage.get_drop_rate(&info.main_item, &values.value_target, &values.item_names);
    for (item, order) in info.sub_item.iter().zip(info.sub_order.iter()) {
        drop_values += stage.get_drop_rate(item, &values.value_target, &values.item_names) / (*order as f64);
    }
    drop_values / stage.min_clear_time * 120.0
}

/// 試行数が足りないステージを除外する。全滅した場合は無条件で全部返す。
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
    use serde::{Deserialize, Serialize};
    use tokio::sync::OnceCell;

    const SERVERS: [Server; 2] = [Server::Global, Server::Mainland];

    async fn seed_source() -> &'static ExternalSourceRegistry {
        static SOURCE: OnceCell<ExternalSourceRegistry> = OnceCell::const_new();
        SOURCE.get_or_init(|| ExternalSourceRegistry::load(true)).await
    }

    async fn seed_engine() -> &'static RiseiCalculatorEngine {
        static ENGINE: OnceCell<RiseiCalculatorEngine> = OnceCell::const_new();
        ENGINE
            .get_or_init(|| async { RiseiCalculatorEngine::load(seed_source().await).await.expect("seedから理性価値表を計算できる") })
            .await
    }

    /// 起動時に組み立てた計算結果が、Seedに依存しない性質（`calculator::check_invariants`）を満たすこと。
    async fn assert_invariants(engine: &RiseiCalculatorEngine, source: &ExternalSourceRegistry) {
        let item_names = source.item_names.get().await;
        let formulas = source.formulas.get().await;
        let mut bad = Vec::new();
        for server in SERVERS {
            let calc = engine.lock_for(server).read().await;
            bad.extend(calculator::check_invariants(&calc, &item_names, &formulas.formulas));
        }
        assert!(bad.is_empty(), "理性価値表の不変条件が破れている:\n{}", bad.join("\n"));
    }

    #[tokio::test]
    async fn seed_calculation_satisfies_invariants() {
        assert_invariants(seed_engine().await, seed_source().await).await;
    }

    /// 理性価値表の計算結果の写し。`testdata/golden_<server>.json` に置き、コードの退行を見張る。
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Golden {
        /// カテゴリ → 基準マップの作戦コード
        base_stages: BTreeMap<String, String>,
        /// 素材(中国語名) → 理性価値
        values: BTreeMap<String, f64>,
        /// 素材(中国語名) → 理性価値の誤差
        std_devs: BTreeMap<String, f64>,
        /// stage_id → 理性効率（試行数が基準マップの条件を満たす通常ステージだけ）
        stage_efficiencies: BTreeMap<String, f64>,
    }

    impl Golden {
        fn from_snapshot(snapshot: &EngineSnapshot) -> Self {
            let values = &snapshot.values;
            let target = || values.value_target.iter().enumerate();
            Self {
                base_stages: snapshot.base_stage_display.clone(),
                values: target().map(|(i, zh)| (zh.to_string(), values.value_array[i])).collect(),
                std_devs: target().map(|(i, zh)| (zh.to_string(), values.dev_array[i])).collect(),
                stage_efficiencies: snapshot
                    .stage_info
                    .main_stage_dict
                    .values()
                    .filter(|s| s.ap_cost > 0.0 && s.max_times() >= DEFAULT_BASE_MIN_TIMES)
                    .map(|s| (s.stage_id.clone(), s.get_efficiency(values)))
                    .collect(),
            }
        }
    }

    fn golden_path(server: Server) -> std::path::PathBuf {
        let name = match server {
            Server::Global => "global",
            Server::Mainland => "mainland",
        };
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/engine/risei_calculator_engine/testdata")
            .join(format!("golden_{name}.json"))
    }

    /// 2つの表のキーが同じで、値が相対誤差1e-9以内で一致すること。違いは`bad`に足す。
    fn diff_numbers(label: &str, expected: &BTreeMap<String, f64>, actual: &BTreeMap<String, f64>, bad: &mut Vec<String>) {
        for key in expected.keys().filter(|k| !actual.contains_key(*k)) {
            bad.push(format!("{label}: {key} が無くなった"));
        }
        for (key, a) in actual {
            match expected.get(key) {
                None => bad.push(format!("{label}: {key} が増えた ({a})")),
                Some(e) if (a - e).abs() > 1e-9 * e.abs().max(a.abs()) => {
                    bad.push(format!("{label}: {key} {e} → {a} ({:+.4}%)", (a - e) / e * 100.0));
                }
                Some(_) => {}
            }
        }
    }

    /// `UPDATE_GOLDEN=1 cargo test risei_golden` で期待値を書き直す（Seedを作り直したとき）。
    async fn check_golden(server: Server) {
        let actual = Golden::from_snapshot(&seed_engine().await.snapshot(server).await);
        let path = golden_path(server);
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
            println!("{} を書き直しました", path.display());
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("{} を読めない（初回は UPDATE_GOLDEN=1 cargo test risei_golden で作る）: {e}", path.display())
        });
        let expected: Golden = serde_json::from_str(&text).unwrap();

        let mut bad = Vec::new();
        if expected.base_stages != actual.base_stages {
            bad.push(format!("基準マップ: {:?} → {:?}", expected.base_stages, actual.base_stages));
        }
        diff_numbers("理性価値", &expected.values, &actual.values, &mut bad);
        diff_numbers("誤差", &expected.std_devs, &actual.std_devs, &mut bad);
        diff_numbers("ステージ効率", &expected.stage_efficiencies, &actual.stage_efficiencies, &mut bad);
        assert!(
            bad.is_empty(),
            "{server:?} の理性価値表が golden と違う（{}件。先頭20件）:\n{}\n\
             Seedを作り直したのなら、差分を確かめてから UPDATE_GOLDEN=1 cargo test risei_golden で更新する",
            bad.len(),
            bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    #[tokio::test]
    async fn risei_golden_global() {
        check_golden(Server::Global).await;
    }

    #[tokio::test]
    async fn risei_golden_mainland() {
        check_golden(Server::Mainland).await;
    }

    /// 基準マップの初期値と差し替え先は乱数で選ぶが、収束先は1つに決まっていること。
    /// 揺れると、2時間ごとの再計算のたびに理性価値が変わってしまう。
    #[tokio::test]
    async fn calculation_does_not_depend_on_random_choices() {
        let source = seed_source().await;
        let first = seed_engine().await;
        for _ in 0..2 {
            let other = RiseiCalculatorEngine::load(source).await.expect("seedから理性価値表を計算できる");
            for server in SERVERS {
                assert_eq!(
                    Golden::from_snapshot(&first.snapshot(server).await),
                    Golden::from_snapshot(&other.snapshot(server).await),
                    "{server:?} の計算結果が実行ごとに変わる"
                );
            }
        }
    }

    /// 実ネットワークに対して起動時ロード〜線形連立方程式の解決〜基準マップ収束・
    /// ステージ検索まで一通り動くかの疎通確認。`cargo test -- --ignored` で明示実行する。
    /// 今の penguin-stats のデータでも不変条件が成り立つかを見る（データ側の不整合の検知）。
    /// riseiXXX相当のDTO構築ロジックはbot層(`bot/commands/risei/*.rs`)に移ったため
    /// ここでは検証しない。
    #[tokio::test]
    #[ignore]
    async fn load_and_solve_against_real_network() {
        let outer_source = ExternalSourceRegistry::load(false).await;
        let engine = RiseiCalculatorEngine::load(&outer_source)
            .await
            .expect("engine should build against real network data");
        assert_invariants(&engine, &outer_source).await;

        for server in SERVERS {
            let snapshot = engine.snapshot(server).await;
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
