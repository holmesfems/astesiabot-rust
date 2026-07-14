use super::formula::FormulaItem;
use super::server::{value_target, Server, StageCategoryFile};
use super::stage::{StageItem, EPSILON};
use super::stage_info::StageInfo;
use super::values::RiseiValues;
use crate::engine::outer_source::ark_matrix::ArkMatrix;
use crate::engine::outer_source::ark_stages::ArkStages;
use crate::engine::outer_source::formulas::RawFormula;
use crate::engine::outer_source::item_names::ItemNames;
use crate::engine::outer_source::zones::Zones;
use chrono::{DateTime, Utc};
use nalgebra::{DMatrix, DVector};
use rand::seq::SliceRandom;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::Error;

/// 合成レシピの副産物ドロップ率（Python `ConvertionMatrix.__init__` のデフォルト値）。
const CONVERTION_DROP_RATE: f64 = 0.18;

/// 基準マップ差し替え収束ループ(`BaseStageMatrix::update`)の最大反復回数。
/// Python版は`while(baseMatrix.update(values))`で無条件に回し続けるが、
/// カテゴリ数（数十件）に対して十分すぎるほど大きい値を安全弁として設定し、
/// 収束しないケース（データ不整合等）でテスト・実運用が無限ループしないようにする。
const MAX_BASE_STAGE_UPDATE_ITERATIONS: usize = 1_000;

/// 換算行列・基準マップ行列などの1行分（Python `Calculator.ConvertionItem`）。
struct ConvertionItem {
    #[allow(dead_code)]
    name: String,
    value_array: DVector<f64>,
}

impl ConvertionItem {
    fn new(len: usize, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value_array: DVector::zeros(len),
        }
    }

    fn set_value(&mut self, dict: &HashMap<String, f64>, value_target: &[&str]) {
        for (zh, value) in dict {
            if let Some(idx) = value_target.iter().position(|x| x == zh) {
                self.value_array[idx] = *value;
            }
        }
    }
}

fn convertion_item_from_map(len: usize, value_target: &[&str], name: &str, dict: HashMap<&str, f64>) -> ConvertionItem {
    let dict: HashMap<String, f64> = dict.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    let mut item = ConvertionItem::new(len, name);
    item.set_value(&dict, value_target);
    item
}

/// 素材合成・経験値・純金換算の行列（Python `Calculator.ConvertionMatrix`）。
fn build_convertion_matrix(server: Server, item_names: &ItemNames, formulas: &[RawFormula]) -> Vec<ConvertionItem> {
    let value_target_list = value_target(server);
    let n = value_target_list.len();
    let mut items = Vec::new();

    items.push(convertion_item_from_map(
        n,
        &value_target_list,
        "経験値換算1",
        HashMap::from([("基础作战记录", -2.0), ("初级作战记录", 1.0)]),
    ));
    items.push(convertion_item_from_map(
        n,
        &value_target_list,
        "経験値換算2",
        HashMap::from([("初级作战记录", -2.5), ("中级作战记录", 1.0)]),
    ));
    items.push(convertion_item_from_map(
        n,
        &value_target_list,
        "経験値換算3",
        HashMap::from([("中级作战记录", -2.0), ("高级作战记录", 1.0)]),
    ));
    items.push(convertion_item_from_map(
        n,
        &value_target_list,
        "純金換算",
        HashMap::from([("赤金", -2.0), ("龙门币1000", 1.0)]),
    ));

    for raw in formulas {
        let zh = item_names.get_zh(&raw.item_id);
        if !value_target_list.contains(&zh) {
            continue;
        }
        let formula_item = FormulaItem::from_raw(raw, item_names);
        let array = formula_item.to_formula_array_with_outcome(CONVERTION_DROP_RATE, &value_target_list, item_names);
        let mut item = ConvertionItem::new(n, format!("合成-{}", formula_item.name));
        item.set_value(&array.to_zh_count_dict(item_names), &value_target_list);
        items.push(item);
    }

    items
}

/// 理性消費のみで換算される定番周回マップ（Python `Calculator.ConstStageMatrix`）。
struct StageDropItem {
    #[allow(dead_code)]
    name: String,
    value_array: DVector<f64>,
    ap_cost: f64,
}

fn build_const_stage_matrix(server: Server) -> Vec<StageDropItem> {
    let value_target_list = value_target(server);
    let n = value_target_list.len();

    let make = |name: &str, dict: HashMap<&str, f64>, ap_cost: f64| -> StageDropItem {
        let dict: HashMap<String, f64> = dict.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let mut item = ConvertionItem::new(n, name);
        item.set_value(&dict, &value_target_list);
        StageDropItem {
            name: name.to_string(),
            value_array: item.value_array,
            ap_cost,
        }
    };

    vec![
        make(
            "LS-6",
            HashMap::from([("中级作战记录", 2.0), ("高级作战记录", 4.0), ("龙门币1000", 0.432)]),
            36.0,
        ),
        make("CE-6", HashMap::from([("龙门币1000", 10.0)]), 36.0),
        make(
            "CA-5",
            HashMap::from([
                ("技巧概要·卷1", 1.5),
                ("技巧概要·卷2", 1.5),
                ("技巧概要·卷3", 2.0),
                ("龙门币1000", 0.36),
            ]),
            30.0,
        ),
    ]
}

/// 基準マップ1件（カテゴリ→選ばれたステージ。Python `Calculator.BaseStageDropItem`）。
pub struct BaseStageDropItem {
    pub category: String,
    pub name: String,
    pub stage: Arc<StageItem>,
}

/// カテゴリごとの基準マップ集合（Python `Calculator.BaseStageMatrix`）。
pub struct BaseStageMatrix {
    pub valid_base_min_times: i64,
    pub items: BTreeMap<String, BaseStageDropItem>,
}

impl BaseStageMatrix {
    fn from_seed(valid_base_min_times: i64, seed: HashMap<String, Arc<StageItem>>, stage_info: &StageInfo) -> Self {
        let items = seed
            .into_iter()
            .filter_map(|(category, stage)| {
                let to_ja = stage_info.category_instance_dict.get(&category)?.info.to_ja.clone();
                let name = format!("{to_ja}{}", stage.name);
                Some((category.clone(), BaseStageDropItem { category, name, stage }))
            })
            .collect();
        Self {
            valid_base_min_times,
            items,
        }
    }

    pub fn contains(&self, stage_id: &str) -> bool {
        self.items.values().any(|item| item.stage.stage_id == stage_id)
    }

    /// カテゴリ`to_ja`名→選択中のステージコード（基準マップ表示用。Python `toString`）。
    pub fn to_ja_display_map(&self) -> BTreeMap<String, String> {
        self.items
            .values()
            .map(|item| (item.category.clone(), item.stage.name.clone()))
            .collect()
    }

    /// 基準マップより明らかに効率の良いステージがあれば1件だけ入れ替える
    /// （Python `BaseStageMatrix.update`）。入れ替えが起きなければ `Ok(false)`。
    fn update(&mut self, stage_info: &StageInfo, values: &RiseiValues) -> Result<bool, Error> {
        let max_item = match stage_info.get_category_max_efficiency(values, self.valid_base_min_times) {
            Some(item) => item,
            None => return Err("有効な基準ステージ候補が見つかりませんでした".into()),
        };
        if max_item.categories.is_empty() {
            return Err(format!(
                "カテゴリから外れたマップを検出、計算を中断します。マップ{}は、何を稼ぐステージですか？",
                max_item.stage.name
            )
            .into());
        }
        if max_item.max_value <= 1.0 + EPSILON {
            return Ok(false);
        }
        let target_category = max_item
            .categories
            .choose(&mut rand::thread_rng())
            .expect("categories is checked non-empty above")
            .clone();
        println!(
            "[risei_calculator_engine] stage {}: {target_category} category eff={} replacing",
            max_item.stage.name, max_item.max_value
        );
        let to_ja = stage_info
            .category_instance_dict
            .get(&target_category)
            .map(|c| c.info.to_ja.clone())
            .unwrap_or_default();
        let name = format!("{to_ja}{}", max_item.stage.name);
        self.items.insert(
            target_category.clone(),
            BaseStageDropItem {
                category: target_category,
                name,
                stage: max_item.stage,
            },
        );
        Ok(true)
    }
}

fn build_prob_matrix(
    value_target: &[&str],
    item_names: &ItemNames,
    convertion_items: &[ConvertionItem],
    const_stage_items: &[StageDropItem],
    base_stage_matrix: &BaseStageMatrix,
) -> Result<DMatrix<f64>, Error> {
    let n = value_target.len();
    let mut data = Vec::with_capacity(n * n);
    for item in convertion_items {
        data.extend(item.value_array.iter());
    }
    for item in const_stage_items {
        data.extend(item.value_array.iter());
    }
    for item in base_stage_matrix.items.values() {
        data.extend(item.stage.to_drop_array(value_target, item_names).iter());
    }
    let rows = data.len() / n;
    if data.len() != n * n {
        return Err(format!(
            "理性価値の連立方程式が正方行列になりません(rows={rows}, cols={n})。\
             合成レシピの対応漏れ、もしくはカテゴリ数の不一致の可能性があります"
        )
        .into());
    }
    Ok(DMatrix::from_row_slice(n, n, &data))
}

fn build_cost_array(
    convertion_items: &[ConvertionItem],
    const_stage_items: &[StageDropItem],
    base_stage_matrix: &BaseStageMatrix,
) -> DVector<f64> {
    let mut costs = vec![0.0; convertion_items.len()];
    costs.extend(const_stage_items.iter().map(|i| i.ap_cost));
    costs.extend(base_stage_matrix.items.values().map(|i| i.stage.ap_cost));
    DVector::from_vec(costs)
}

fn build_dev_matrix(
    value_target: &[&str],
    item_names: &ItemNames,
    convertion_items: &[ConvertionItem],
    const_stage_items: &[StageDropItem],
    base_stage_matrix: &BaseStageMatrix,
) -> DMatrix<f64> {
    let n = value_target.len();
    let mut data = Vec::with_capacity(n * n);
    for _ in convertion_items {
        data.extend(std::iter::repeat(0.0).take(n));
    }
    for _ in const_stage_items {
        data.extend(std::iter::repeat(0.0).take(n));
    }
    for item in base_stage_matrix.items.values() {
        data.extend(item.stage.to_std_dev_array(value_target, item_names).iter());
    }
    DMatrix::from_row_slice(n, n, &data)
}

fn solve_values(
    server: Server,
    value_target: &[&str],
    convertion_items: &[ConvertionItem],
    const_stage_items: &[StageDropItem],
    base_stage_matrix: &BaseStageMatrix,
    item_names: Arc<ItemNames>,
    const_values: Arc<HashMap<String, f64>>,
) -> Result<RiseiValues, Error> {
    let prob_matrix = build_prob_matrix(value_target, &item_names, convertion_items, const_stage_items, base_stage_matrix)?;
    let cost_array = build_cost_array(convertion_items, const_stage_items, base_stage_matrix);
    let value_array = prob_matrix
        .lu()
        .solve(&cost_array)
        .ok_or("理性価値の連立方程式が解けませんでした（特異行列）")?;
    Ok(RiseiValues::new(server, value_array, item_names, const_values))
}

/// サーバ1つ分の理性価値表計算エンジン全体（Python `Calculator`）。
/// 起動時、およびキャッシュ期限切れ時に [`Calculator::build`] で丸ごと再構築する
/// （Python版の部分更新最適化(`initMatrix`のみ再実行)は行わず、都度フル再構築する。
/// ステージ数・レシピ数の規模ではコスト的に問題にならないため簡潔さを優先した）。
pub struct Calculator {
    pub stage_info: Arc<StageInfo>,
    pub base_stage_matrix: BaseStageMatrix,
    pub values: RiseiValues,
    pub last_updated: DateTime<Utc>,
}

impl Calculator {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        server: Server,
        stage_category_file: &StageCategoryFile,
        ark_stages: &ArkStages,
        ark_matrix: &ArkMatrix,
        zones: &Zones,
        item_names: Arc<ItemNames>,
        formulas: &[RawFormula],
        min_clear_time_injection: &HashMap<String, f64>,
        const_values: Arc<HashMap<String, f64>>,
        valid_base_min_times: i64,
    ) -> Result<Self, Error> {
        println!("[risei_calculator_engine] Risei calculate start ({server:?})");
        let stage_info = Arc::new(StageInfo::build(
            server,
            stage_category_file,
            ark_stages,
            ark_matrix,
            zones,
            item_names.clone(),
            min_clear_time_injection,
        ));
        let convertion_items = build_convertion_matrix(server, &item_names, formulas);
        let const_stage_items = build_const_stage_matrix(server);
        let value_target_list = value_target(server);

        let seed = stage_info.generate_category_seed(valid_base_min_times)?;
        let mut base_stage_matrix = BaseStageMatrix::from_seed(valid_base_min_times, seed, &stage_info);

        let mut values = solve_values(
            server,
            &value_target_list,
            &convertion_items,
            &const_stage_items,
            &base_stage_matrix,
            item_names.clone(),
            const_values.clone(),
        )?;

        // 明らかに現在の基準マップより効率の良いステージが見つからなくなるまで、
        // 基準マップを差し替えて再計算し続ける（Python `solveOptimizedValue`）。
        let mut converged = false;
        for _ in 0..MAX_BASE_STAGE_UPDATE_ITERATIONS {
            if !base_stage_matrix.update(&stage_info, &values)? {
                converged = true;
                break;
            }
            values = solve_values(
                server,
                &value_target_list,
                &convertion_items,
                &const_stage_items,
                &base_stage_matrix,
                item_names.clone(),
                const_values.clone(),
            )?;
        }
        if !converged {
            return Err(format!(
                "基準マップの差し替えが{MAX_BASE_STAGE_UPDATE_ITERATIONS}回反復しても収束しませんでした。\
                 理性効率の計算が振動している可能性があります"
            )
            .into());
        }
        println!("[risei_calculator_engine] Risei calculate end ({server:?})");

        // 誤差(標準偏差)を計算する。
        let prob_matrix = build_prob_matrix(&value_target_list, &item_names, &convertion_items, &const_stage_items, &base_stage_matrix)?;
        let dev_matrix = build_dev_matrix(&value_target_list, &item_names, &convertion_items, &const_stage_items, &base_stage_matrix);
        let prob_inv = prob_matrix
            .try_inverse()
            .ok_or("理性価値の行列が特異で誤差計算に失敗しました")?;
        let prob_inv_sq = prob_inv.map(|x| x * x);
        let dev_matrix_sq = dev_matrix.map(|x| x * x);
        let value_sq = values.value_array.map(|x| x * x);
        let dev_array = (&prob_inv_sq * (&dev_matrix_sq * &value_sq)).map(|x| x.sqrt());
        values.set_dev_array(dev_array);

        Ok(Self {
            stage_info,
            base_stage_matrix,
            values,
            last_updated: Utc::now(),
        })
    }

    pub fn get_stage_dev(&self, target_stage: &StageItem) -> f64 {
        if self.base_stage_matrix.contains(&target_stage.stage_id) {
            return 0.0;
        }
        let cost = if target_stage.ap_cost > 0.0 { target_stage.ap_cost } else { 1.0 };
        target_stage.get_std_dev(&self.values) / cost
    }
}
