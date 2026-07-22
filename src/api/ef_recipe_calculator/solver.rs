//! エンドフィールド レシピ計算機の計算エンジン（純粋関数）。
//! `RecipeSet` + `CalcRequest` -> `CalcResult` のみを扱い、UI/IOには触れない。
//! アルゴリズムの詳細は `EFRecipeCalculator.md` §4 を参照。

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

const MAX_ITER: usize = 100;
const EPSILON: f64 = 1e-6;

/// 産出物（主産出・副産物を区別せず outputs に並べる）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub item: String,
    pub qty: f64,
}

/// 材料（生産に直接消費される）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Input {
    pub item: String,
    pub qty: f64,
}

/// 稼働コスト（材料とは別枠。自己消費もここで表現し、循環扱いしない）。
/// 材料消費/産出とは異なり稼働率を考慮せず、常に台数×rate_per_minの固定費として計算する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatingCost {
    pub item: String,
    pub rate_per_min: f64,
}

/// 1レシピ = 1設備が回す1種類の生産プロセス。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    /// このレシピを回す設備・建物の名称(表示用。計算には使わない)。
    #[serde(default)]
    pub equipment_name: String,
    pub cycle_seconds: f64,
    pub outputs: Vec<Output>,
    pub inputs: Vec<Input>,
    #[serde(default)]
    pub operating_costs: Vec<OperatingCost>,
}

/// 採掘など、上限レートのある外部固定供給。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalSupply {
    pub item: String,
    pub max_rate_per_min: f64,
}

/// ユーザーが編集する作業セット全体。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecipeSet {
    pub recipes: Vec<Recipe>,
    pub selected_recipe_ids: Vec<String>,
    #[serde(default)]
    pub raw_items: Vec<String>,
    #[serde(default)]
    pub external_supplies: Vec<ExternalSupply>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalcRequest {
    pub target_item: String,
    pub target_rate_per_min: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialNeed {
    pub item: String,
    pub rate_per_min: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinedUsage {
    pub item: String,
    pub used_rate: f64,
    pub surplus_rate: f64,
    pub cap_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalcStep {
    pub recipe_id: String,
    pub recipe_name: String,
    pub equipment_name: String,
    pub machine_count: u64,
    pub limiting_output: String,
    pub utilization: f64,
    pub outputs_effective: Vec<MaterialNeed>,
    pub inputs_demand: Vec<MaterialNeed>,
    pub operating_demand: Vec<MaterialNeed>,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalcResult {
    pub steps: Vec<CalcStep>,
    pub raw_materials: Vec<MaterialNeed>,
    pub mined_usage: Vec<MinedUsage>,
    pub byproduct_surplus: Vec<MaterialNeed>,
    pub bottleneck: Option<String>,
    pub warnings: Vec<String>,
}

fn per_min(qty: f64, cycle_seconds: f64) -> f64 {
    qty / cycle_seconds * 60.0
}

/// アイテム→産出レシピの索引。`selected_recipe_ids` の順に構築し、同一アイテムを
/// 産出するレシピが複数選択されていれば先頭採用＋warning（代替レシピ非対応）。
fn build_item_recipe_index<'a>(
    set: &'a RecipeSet,
    warnings: &mut Vec<String>,
) -> IndexMap<String, &'a Recipe> {
    let mut index: IndexMap<String, &Recipe> = IndexMap::new();
    for id in &set.selected_recipe_ids {
        let Some(recipe) = set.recipes.iter().find(|r| &r.id == id) else {
            continue;
        };
        for output in &recipe.outputs {
            if let Some(&existing) = index.get(&output.item) {
                if existing.id != recipe.id {
                    warnings.push(format!(
                        "アイテム『{}』を産出するレシピが複数選択されています(『{}』と『{}』)。『{}』を採用します。",
                        output.item, existing.name, recipe.name, existing.name
                    ));
                }
                continue;
            }
            index.insert(output.item.clone(), recipe);
        }
    }
    index
}

fn build_external_supply_map(set: &RecipeSet) -> IndexMap<String, f64> {
    let mut map = IndexMap::new();
    for supply in &set.external_supplies {
        map.insert(supply.item.clone(), supply.max_rate_per_min);
    }
    map
}

/// 1反復分の需要展開結果（仕様§4.2 手順1）。
struct Expansion {
    raw_materials: IndexMap<String, f64>,
    /// item -> (used, cap)
    mined_usage: IndexMap<String, (f64, f64)>,
    /// recipe_id -> 必要台数(浮動)。同一レシピを要求する複数アイテムがあれば max。
    required_float: IndexMap<String, f64>,
    /// item -> その item の需要のうちレシピ生産で充当された残余(=レシピへの要求量)。
    output_usage: IndexMap<String, f64>,
    warnings: Vec<String>,
}

fn expand_demand(
    demand: &IndexMap<String, f64>,
    raw_items: &HashSet<&str>,
    external_supply: &IndexMap<String, f64>,
    item_recipe: &IndexMap<String, &Recipe>,
) -> Expansion {
    let mut raw_materials = IndexMap::new();
    let mut mined_usage = IndexMap::new();
    let mut required_float: IndexMap<String, f64> = IndexMap::new();
    let mut output_usage = IndexMap::new();
    let mut warnings = Vec::new();

    for (item, &r) in demand {
        if raw_items.contains(item.as_str()) {
            *raw_materials.entry(item.clone()).or_insert(0.0) += r;
            continue;
        }

        let residual = if let Some(&cap) = external_supply.get(item) {
            let used = r.min(cap);
            let entry = mined_usage.entry(item.clone()).or_insert((0.0, cap));
            entry.0 += used;
            (r - used).max(0.0)
        } else {
            r
        };

        if residual <= 0.0 {
            continue;
        }

        if let Some(&recipe) = item_recipe.get(item) {
            let output = recipe
                .outputs
                .iter()
                .find(|o| &o.item == item)
                .expect("item_recipe は該当レシピの outputs から構築される");
            let per_machine = per_min(output.qty, recipe.cycle_seconds);
            if per_machine <= 0.0 {
                warnings.push(format!(
                    "レシピ『{}』の産出『{}』の量が0以下のため計算できません。",
                    recipe.name, item
                ));
                continue;
            }
            let need = residual / per_machine;
            let entry = required_float.entry(recipe.id.clone()).or_insert(0.0);
            if need > *entry {
                *entry = need;
            }
            output_usage.insert(item.clone(), residual);
        } else {
            warnings.push(format!(
                "『{item}』は原料指定もレシピもありません。原料として扱います。"
            ));
            *raw_materials.entry(item.clone()).or_insert(0.0) += residual;
        }
    }

    Expansion {
        raw_materials,
        mined_usage,
        required_float,
        output_usage,
        warnings,
    }
}

/// 仕様§4.2 手順2〜4: 台数確定 + 実効算出 + 次期demand構築。
struct Resolution {
    machines: IndexMap<String, u64>,
    next_demand: IndexMap<String, f64>,
    byproduct_surplus: IndexMap<String, f64>,
}

fn resolve_machines(
    target_item: &str,
    target_rate: f64,
    required_float: &IndexMap<String, f64>,
    output_usage: &IndexMap<String, f64>,
    recipes_by_id: &IndexMap<String, &Recipe>,
) -> Resolution {
    let mut machines = IndexMap::new();
    for (rid, &f) in required_float {
        machines.insert(rid.clone(), (f.ceil().max(1.0)) as u64);
    }

    let mut next_demand = IndexMap::new();
    next_demand.insert(target_item.to_string(), target_rate);
    let mut byproduct_surplus: IndexMap<String, f64> = IndexMap::new();

    for (rid, &count) in &machines {
        let recipe = recipes_by_id[rid];
        // オンデマンド生産: 切り上げで余裕のある工程は稼働率(<=1)ぶんだけしか回らない。
        // 消費・産出はすべて台数×フルレート×稼働率で計算する(仕様: patch-utilization-consumption)。
        let required = required_float.get(rid).copied().unwrap_or(0.0);
        let utilization = if count > 0 { required / count as f64 } else { 0.0 };

        for output in &recipe.outputs {
            let effective = count as f64 * per_min(output.qty, recipe.cycle_seconds) * utilization;
            let charged = output_usage.get(&output.item).copied().unwrap_or(0.0);
            let surplus = (effective - charged).max(0.0);
            if surplus > EPSILON {
                *byproduct_surplus.entry(output.item.clone()).or_insert(0.0) += surplus;
            }
        }
        for input in &recipe.inputs {
            *next_demand.entry(input.item.clone()).or_insert(0.0) +=
                count as f64 * per_min(input.qty, recipe.cycle_seconds) * utilization;
        }
        for oc in &recipe.operating_costs {
            // 稼働コストは稼働率を考慮せず、配置した台数分だけ常に発生する固定費として扱う
            // (材料消費/産出とは異なり、稼働率で按分しない)。
            *next_demand.entry(oc.item.clone()).or_insert(0.0) += count as f64 * oc.rate_per_min;
        }
    }

    Resolution {
        machines,
        next_demand,
        byproduct_surplus,
    }
}

fn maps_close(a: &IndexMap<String, f64>, b: &IndexMap<String, f64>, eps: f64) -> bool {
    let close_one_way = |x: &IndexMap<String, f64>, y: &IndexMap<String, f64>| {
        x.iter()
            .all(|(k, v)| (y.get(k).copied().unwrap_or(0.0) - v).abs() < eps)
    };
    close_one_way(a, b) && close_one_way(b, a)
}

/// `target_item` を産出するレシピを根に、実際に使用中(machines>0)のレシピだけを
/// 逆方向BFSして段数を振る(根=0)。自己消費による循環はBFSの既訪問チェックで自然に止まる。
fn compute_depths(
    target_item: &str,
    item_recipe: &IndexMap<String, &Recipe>,
    machines: &IndexMap<String, u64>,
) -> IndexMap<String, u32> {
    let mut depths: IndexMap<String, u32> = IndexMap::new();
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();

    if let Some(&root) = item_recipe.get(target_item) {
        if machines.contains_key(&root.id) {
            depths.insert(root.id.clone(), 0);
            queue.push_back((root.id.clone(), 0));
        }
    }

    while let Some((rid, depth)) = queue.pop_front() {
        let Some(recipe) = item_recipe.values().find(|r| r.id == rid) else {
            continue;
        };
        let downstream_items = recipe
            .inputs
            .iter()
            .map(|i| &i.item)
            .chain(recipe.operating_costs.iter().map(|o| &o.item));
        for item in downstream_items {
            if let Some(&next) = item_recipe.get(item) {
                if machines.contains_key(&next.id) && !depths.contains_key(&next.id) {
                    depths.insert(next.id.clone(), depth + 1);
                    queue.push_back((next.id.clone(), depth + 1));
                }
            }
        }
    }

    depths
}

fn build_steps(
    machines: &IndexMap<String, u64>,
    required_float: &IndexMap<String, f64>,
    output_usage: &IndexMap<String, f64>,
    recipes_by_id: &IndexMap<String, &Recipe>,
    depths: &IndexMap<String, u32>,
) -> Vec<CalcStep> {
    machines
        .iter()
        .map(|(rid, &count)| {
            let recipe = recipes_by_id[rid];
            let required = required_float.get(rid).copied().unwrap_or(0.0);

            let mut limiting_output = String::new();
            let mut best = -1.0_f64;
            for output in &recipe.outputs {
                let charged = output_usage.get(&output.item).copied().unwrap_or(0.0);
                let per_machine = per_min(output.qty, recipe.cycle_seconds);
                let need = if per_machine > 0.0 { charged / per_machine } else { 0.0 };
                if need > best {
                    best = need;
                    limiting_output = output.item.clone();
                }
            }

            let utilization = if count > 0 { required / count as f64 } else { 0.0 };

            let outputs_effective = recipe
                .outputs
                .iter()
                .map(|o| MaterialNeed {
                    item: o.item.clone(),
                    rate_per_min: count as f64 * per_min(o.qty, recipe.cycle_seconds) * utilization,
                })
                .collect();
            let inputs_demand = recipe
                .inputs
                .iter()
                .map(|i| MaterialNeed {
                    item: i.item.clone(),
                    rate_per_min: count as f64 * per_min(i.qty, recipe.cycle_seconds) * utilization,
                })
                .collect();
            let operating_demand = recipe
                .operating_costs
                .iter()
                .map(|oc| MaterialNeed {
                    item: oc.item.clone(),
                    rate_per_min: count as f64 * oc.rate_per_min,
                })
                .collect();

            CalcStep {
                recipe_id: rid.clone(),
                recipe_name: recipe.name.clone(),
                equipment_name: recipe.equipment_name.clone(),
                machine_count: count,
                limiting_output,
                utilization,
                outputs_effective,
                inputs_demand,
                operating_demand,
                depth: depths.get(rid).copied().unwrap_or(0),
            }
        })
        .collect()
}

/// エンジンのエントリポイント。`Err` は明確な入力異常のみ。それ以外(未選択レシピで
/// 経路が切れる・代替レシピ衝突・反復収束失敗等)は `CalcResult.warnings` に積んで
/// `Ok` を返す(黙って誤答しない = 警告付きで最終反復時点の値を返す、という意味)。
pub fn calculate(set: &RecipeSet, req: &CalcRequest) -> Result<CalcResult, String> {
    if !req.target_rate_per_min.is_finite() || req.target_rate_per_min <= 0.0 {
        return Err("目標レートは正の数で指定してください。".to_string());
    }

    let mut warnings = Vec::new();
    let item_recipe = build_item_recipe_index(set, &mut warnings);
    let external_supply = build_external_supply_map(set);
    let raw_items: HashSet<&str> = set.raw_items.iter().map(|s| s.as_str()).collect();
    let recipes_by_id: IndexMap<String, &Recipe> =
        set.recipes.iter().map(|r| (r.id.clone(), r)).collect();

    let mut demand: IndexMap<String, f64> = IndexMap::new();
    demand.insert(req.target_item.clone(), req.target_rate_per_min);
    let mut machines: IndexMap<String, u64> = IndexMap::new();

    let mut last_expansion = expand_demand(&demand, &raw_items, &external_supply, &item_recipe);
    let mut last_resolution = resolve_machines(
        &req.target_item,
        req.target_rate_per_min,
        &last_expansion.required_float,
        &last_expansion.output_usage,
        &recipes_by_id,
    );
    let mut converged = maps_close(&demand, &last_resolution.next_demand, EPSILON)
        && machines == last_resolution.machines;
    demand = last_resolution.next_demand.clone();
    machines = last_resolution.machines.clone();

    let mut iterations_used = 1;
    while !converged && iterations_used < MAX_ITER {
        last_expansion = expand_demand(&demand, &raw_items, &external_supply, &item_recipe);
        last_resolution = resolve_machines(
            &req.target_item,
            req.target_rate_per_min,
            &last_expansion.required_float,
            &last_expansion.output_usage,
            &recipes_by_id,
        );
        converged = maps_close(&demand, &last_resolution.next_demand, EPSILON)
            && machines == last_resolution.machines;
        demand = last_resolution.next_demand.clone();
        machines = last_resolution.machines.clone();
        iterations_used += 1;
    }

    warnings.extend(last_expansion.warnings);
    if !converged {
        warnings.push(format!(
            "反復が収束しませんでした(最大{MAX_ITER}回)。表示値は最終反復時点の近似値です。"
        ));
    }

    let depths = compute_depths(&req.target_item, &item_recipe, &machines);
    let steps = build_steps(
        &machines,
        &last_expansion.required_float,
        &last_expansion.output_usage,
        &recipes_by_id,
        &depths,
    );

    let mut bottleneck: Option<&CalcStep> = None;
    for step in &steps {
        if bottleneck.map_or(true, |b| step.utilization > b.utilization) {
            bottleneck = Some(step);
        }
    }
    let bottleneck = bottleneck.map(|s| s.recipe_name.clone());

    let raw_materials = last_expansion
        .raw_materials
        .into_iter()
        .map(|(item, rate_per_min)| MaterialNeed { item, rate_per_min })
        .collect();
    let mined_usage = last_expansion
        .mined_usage
        .into_iter()
        .map(|(item, (used, cap))| MinedUsage {
            item,
            used_rate: used,
            surplus_rate: (cap - used).max(0.0),
            cap_rate: cap,
        })
        .collect();
    let byproduct_surplus = last_resolution
        .byproduct_surplus
        .into_iter()
        .map(|(item, rate_per_min)| MaterialNeed { item, rate_per_min })
        .collect();

    Ok(CalcResult {
        steps,
        raw_materials,
        mined_usage,
        byproduct_surplus,
        bottleneck,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(item: &str, qty: f64) -> Output {
        Output { item: item.to_string(), qty }
    }
    fn input(item: &str, qty: f64) -> Input {
        Input { item: item.to_string(), qty }
    }
    fn opcost(item: &str, rate_per_min: f64) -> OperatingCost {
        OperatingCost { item: item.to_string(), rate_per_min }
    }
    fn recipe(id: &str, cycle_seconds: f64, outputs: Vec<Output>, inputs: Vec<Input>) -> Recipe {
        Recipe {
            id: id.to_string(),
            name: id.to_string(),
            equipment_name: String::new(),
            cycle_seconds,
            outputs,
            inputs,
            operating_costs: vec![],
        }
    }
    fn recipe_set(recipes: Vec<Recipe>) -> RecipeSet {
        let selected_recipe_ids = recipes.iter().map(|r| r.id.clone()).collect();
        RecipeSet {
            recipes,
            selected_recipe_ids,
            raw_items: vec![],
            external_supplies: vec![],
        }
    }
    fn req(target_item: &str, rate: f64) -> CalcRequest {
        CalcRequest { target_item: target_item.to_string(), target_rate_per_min: rate }
    }
    fn step_for<'a>(result: &'a CalcResult, recipe_id: &str) -> &'a CalcStep {
        result.steps.iter().find(|s| s.recipe_id == recipe_id).expect("step exists")
    }
    fn need_for<'a>(needs: &'a [MaterialNeed], item: &str) -> &'a MaterialNeed {
        needs.iter().find(|n| n.item == item).expect("material need exists")
    }

    /// 1. 単段: 原料→最終製品1段。台数・原料レートが正しい。
    #[test]
    fn single_stage() {
        // 2s cycle, 1個産出。目標30/min -> 1個/2s = 30/min ちょうど1台。
        let mut set = recipe_set(vec![recipe(
            "r1",
            2.0,
            vec![output("製品", 1.0)],
            vec![input("原料", 1.0)],
        )]);
        set.raw_items.push("原料".to_string());
        let result = calculate(&set, &req("製品", 30.0)).unwrap();
        let step = step_for(&result, "r1");
        assert_eq!(step.machine_count, 1);
        assert!((need_for(&result.raw_materials, "原料").rate_per_min - 30.0).abs() < 1e-9);
        assert!(result.warnings.is_empty());
    }

    /// 2. 多段: 3段以上。中間素材の需要伝播が正しい。
    #[test]
    fn multi_stage() {
        let set = recipe_set(vec![
            recipe("c", 2.0, vec![output("完成品", 1.0)], vec![input("中間B", 1.0)]),
            recipe("b", 2.0, vec![output("中間B", 1.0)], vec![input("中間A", 1.0)]),
            recipe("a", 2.0, vec![output("中間A", 1.0)], vec![input("原料", 1.0)]),
        ]);
        let result = calculate(&set, &req("完成品", 30.0)).unwrap();
        assert_eq!(step_for(&result, "c").machine_count, 1);
        assert_eq!(step_for(&result, "b").machine_count, 1);
        assert_eq!(step_for(&result, "a").machine_count, 1);
        assert!((need_for(&result.raw_materials, "原料").rate_per_min - 30.0).abs() < 1e-6);
    }

    /// 3. 合流: 複数製品(A,B)が同一中間素材Cを要求 → 需要が合算される。
    #[test]
    fn merging_demand() {
        let set = recipe_set(vec![
            recipe("final", 2.0, vec![output("完成品", 1.0)], vec![input("A", 1.0), input("B", 1.0)]),
            recipe("a", 2.0, vec![output("A", 1.0)], vec![input("C", 1.0)]),
            recipe("b", 2.0, vec![output("B", 1.0)], vec![input("C", 2.0)]),
        ]);
        // final 1台 -> A,B ともに30/min要求。a: 30/min(1台) -> C 30/min。b: 30/min(1台) -> C 60/min。
        let result = calculate(&set, &req("完成品", 30.0)).unwrap();
        assert!((need_for(&result.raw_materials, "C").rate_per_min - 90.0).abs() < 1e-6);
    }

    /// 4. 切り上げ: 端数が出るケースで machine_count が ceil・最小1になる。
    #[test]
    fn ceiling_and_minimum_one() {
        let set = recipe_set(vec![recipe(
            "r1",
            2.0,
            vec![output("製品", 1.0)],
            vec![input("原料", 1.0)],
        )]);
        // 1台=30/min。要求31/min -> 1.033...台 -> ceil=2台。
        let result = calculate(&set, &req("製品", 31.0)).unwrap();
        assert_eq!(step_for(&result, "r1").machine_count, 2);

        // ごく少量の要求でも最低1台。
        let result_small = calculate(&set, &req("製品", 0.001)).unwrap();
        assert_eq!(step_for(&result_small, "r1").machine_count, 1);
    }

    /// 5. サイクル混在: 2s/10s/20s・産出1/2個の混在で毎分換算が正しい。
    #[test]
    fn mixed_cycles() {
        let set = recipe_set(vec![
            recipe("r2s", 2.0, vec![output("製品", 1.0)], vec![input("原料2s", 1.0)]),
            recipe("r10s", 10.0, vec![output("原料2s", 2.0)], vec![input("原料10s", 1.0)]),
            recipe("r20s", 20.0, vec![output("原料10s", 1.0)], vec![input("原料20s", 2.0)]),
        ]);
        // 製品 目標: r2s 1台分=30/min のちょうど(util=1.0)。
        let result = calculate(&set, &req("製品", 30.0)).unwrap();
        assert_eq!(step_for(&result, "r2s").machine_count, 1);
        // 原料2s needed = 30/min。r10s per machine = 2/10*60=12/min -> ceil(30/12)=3台(util=30/36=0.8333)。
        assert_eq!(step_for(&result, "r10s").machine_count, 3);
        // r10s消費(稼働率按分後) = 3*(1/10*60)*0.8333=15/min(フルレート18/minではない)。
        // r20s per machine=1/20*60=3/min -> ceil(15/3)=5台(util=15/15=1.0ちょうど)。
        assert_eq!(step_for(&result, "r20s").machine_count, 5);
        // r20s 5台・util=1.0 -> 原料20s消費 = 5*(2/20*60)*1.0=30/min(旧フルレート計算では36/minだった)。
        assert!((need_for(&result.raw_materials, "原料20s").rate_per_min - 30.0).abs() < 1e-4);
    }

    /// 6. 採掘控除: 採掘上限<需要のとき不足分だけレシピ生産、採掘は使い切り。
    ///    採掘上限>需要のとき余剰報告、レシピ台数0。
    #[test]
    fn external_supply_deduction() {
        let mut set = recipe_set(vec![recipe(
            "r1",
            2.0,
            vec![output("素材", 1.0)],
            vec![input("原料", 1.0)],
        )]);
        set.external_supplies.push(ExternalSupply { item: "素材".to_string(), max_rate_per_min: 10.0 });
        set.raw_items.push("原料".to_string());

        // 需要30/min、採掘上限10/min -> 採掘10使い切り、レシピは残り20/min分(1台=30/min出力だが
        // need=20/30=0.6667台分 -> ceil=1台・util=0.6667で稼働)。原料消費は稼働率按分後の20/min
        // (フルレートの30/minではない。切り上げ由来の過剰原料所要が出ないことの確認)。
        let result = calculate(&set, &req("素材", 30.0)).unwrap();
        let mined = result.mined_usage.iter().find(|m| m.item == "素材").unwrap();
        assert!((mined.used_rate - 10.0).abs() < 1e-6);
        assert!((mined.surplus_rate - 0.0).abs() < 1e-6);
        assert!(result.steps.iter().any(|s| s.recipe_id == "r1"));
        assert!((need_for(&result.raw_materials, "原料").rate_per_min - 20.0).abs() < 1e-4);
        assert!(result.byproduct_surplus.is_empty());

        // 需要5/min、採掘上限10/min -> 採掘のみで充足、レシピ台数0(steps未登場)。
        let result2 = calculate(&set, &req("素材", 5.0)).unwrap();
        let mined2 = result2.mined_usage.iter().find(|m| m.item == "素材").unwrap();
        assert!((mined2.used_rate - 5.0).abs() < 1e-6);
        assert!((mined2.surplus_rate - 5.0).abs() < 1e-6);
        assert!(!result2.steps.iter().any(|s| s.recipe_id == "r1"));
    }

    /// 7. 稼働コスト(自己消費): 息壌1→息壌ガス1、稼働に息壌ガス6/min。反復で正しく収束。
    #[test]
    fn operating_cost_self_consumption() {
        let mut r = recipe("gas", 2.0, vec![output("息壌ガス", 1.0)], vec![input("息壌", 1.0)]);
        r.operating_costs.push(opcost("息壌ガス", 6.0));
        let mut set = recipe_set(vec![r]);
        set.raw_items.push("息壌".to_string());

        let result = calculate(&set, &req("息壌ガス", 24.0)).unwrap();
        assert!(result.warnings.iter().all(|w| !w.contains("収束しませんでした")));
        let step = step_for(&result, "gas");
        // per machine 出力=30/min。理論解: count*30 = 24 + count*6 -> count=1(ceil(1.0))
        // 24/(30-6)=1.0 ちょうど。
        assert_eq!(step.machine_count, 1);
        let raw = need_for(&result.raw_materials, "息壌");
        assert!((raw.rate_per_min - 30.0).abs() < 1e-4);
    }

    /// 8. 反復収束: 稼働コスト連鎖で数回反復して収束。収束値が理論値と一致。
    #[test]
    fn iterative_convergence_matches_theory() {
        let mut r = recipe("gas", 2.0, vec![output("息壌ガス", 1.0)], vec![input("息壌", 1.0)]);
        r.operating_costs.push(opcost("息壌ガス", 6.0));
        let mut set = recipe_set(vec![r]);
        set.raw_items.push("息壌".to_string());

        // 目標50/min。理論: count*30 - count*6 = 50 -> count = 50/24 = 2.0833... -> ceil=3。
        let result = calculate(&set, &req("息壌ガス", 50.0)).unwrap();
        assert_eq!(step_for(&result, "gas").machine_count, 3);
        assert!(result.warnings.iter().all(|w| !w.contains("収束しませんでした")));
    }

    /// 9. 収束失敗: 意図的に発散する構成でN回打ち切り+warningが出る。
    #[test]
    fn divergent_configuration_warns_without_panicking() {
        // 稼働コストが出力そのものを上回るため、需要が反復ごとに増大し続ける。
        let mut r = recipe("gas", 2.0, vec![output("息壌ガス", 1.0)], vec![input("息壌", 1.0)]);
        r.operating_costs.push(opcost("息壌ガス", 40.0)); // per machine output=30/minを上回る
        let mut set = recipe_set(vec![r]);
        set.raw_items.push("息壌".to_string());

        let result = calculate(&set, &req("息壌ガス", 10.0)).unwrap();
        assert!(result.warnings.iter().any(|w| w.contains("収束しませんでした")));
    }

    /// 10. 副産物max合わせ: 銅鉱石+水→銅塊+汚水。
    #[test]
    fn byproduct_max_matching() {
        let cu = recipe(
            "cu",
            2.0,
            vec![output("銅塊", 1.0), output("汚水", 1.0)],
            vec![input("銅鉱石", 1.0), input("水", 1.0)],
        );
        let set = recipe_set(vec![cu]);

        // 下流が銅塊のみ要求 -> 銅塊需要で台数決定、汚水は全量surplus。
        let only_ingot = calculate(&set, &req("銅塊", 30.0)).unwrap();
        assert_eq!(step_for(&only_ingot, "cu").machine_count, 1);
        assert!((need_for(&only_ingot.byproduct_surplus, "汚水").rate_per_min - 30.0).abs() < 1e-6);

        // 下流が汚水のみ要求 -> 汚水需要で台数決定、銅塊がsurplus。
        let only_sewage = calculate(&set, &req("汚水", 30.0)).unwrap();
        assert_eq!(step_for(&only_sewage, "cu").machine_count, 1);
        assert!((need_for(&only_sewage.byproduct_surplus, "銅塊").rate_per_min - 30.0).abs() < 1e-6);

        // 両方要求(consumer recipe が銅塊・汚水を両方入力に取る) -> 各需要のmaxで決定。
        let consumer = recipe(
            "consumer",
            2.0,
            vec![output("最終品", 1.0)],
            vec![input("銅塊", 1.0), input("汚水", 2.0)],
        );
        let mut both_set = recipe_set(vec![cu_recipe_clone(&set), consumer]);
        both_set.selected_recipe_ids = both_set.recipes.iter().map(|r| r.id.clone()).collect();
        let both = calculate(&both_set, &req("最終品", 30.0)).unwrap();
        // consumer 1台 -> 銅塊需要30/min(cu 1台で足りる)、汚水需要60/min(cu 2台必要)。
        // max合わせで cu は2台になり、銅塊は60-30=30がsurplus。
        assert_eq!(step_for(&both, "cu").machine_count, 2);
        assert!((need_for(&both.byproduct_surplus, "銅塊").rate_per_min - 30.0).abs() < 1e-6);
    }

    fn cu_recipe_clone(set: &RecipeSet) -> Recipe {
        set.recipes.iter().find(|r| r.id == "cu").unwrap().clone()
    }

    /// 11. 原料の底指定: raw_items に息壌を入れると展開停止し、原料として所要レート報告。
    #[test]
    fn raw_item_override_stops_expansion() {
        let mut r = recipe("gas", 2.0, vec![output("息壌ガス", 1.0)], vec![input("息壌", 1.0)]);
        let producer = recipe("soil", 2.0, vec![output("息壌", 1.0)], vec![input("何か", 1.0)]);
        r.operating_costs.clear();
        let mut set = recipe_set(vec![r, producer]);
        set.raw_items.push("息壌".to_string());

        let result = calculate(&set, &req("息壌ガス", 30.0)).unwrap();
        // 息壌の産出レシピ(soil)があっても展開停止し、原料として報告される。
        assert!((need_for(&result.raw_materials, "息壌").rate_per_min - 30.0).abs() < 1e-6);
        assert!(!result.steps.iter().any(|s| s.recipe_id == "soil"));
    }

    /// 12. レシピ選択: selected_recipe_ids に含まれないレシピは索引に入らない。
    ///     未選択で経路が切れる場合は未定義warning。
    #[test]
    fn unselected_recipe_breaks_path_with_warning() {
        let r1 = recipe("r1", 2.0, vec![output("製品", 1.0)], vec![input("中間", 1.0)]);
        let r2 = recipe("r2", 2.0, vec![output("中間", 1.0)], vec![input("原料", 1.0)]);
        let mut set = recipe_set(vec![r1, r2]);
        set.selected_recipe_ids = vec!["r1".to_string()]; // r2 を選択から外す

        let result = calculate(&set, &req("製品", 30.0)).unwrap();
        assert!(result.warnings.iter().any(|w| w.contains("中間") && w.contains("原料として扱います")));
        assert!(!result.steps.iter().any(|s| s.recipe_id == "r2"));
        // 中間素材は経路切れのため原料扱いで報告される。
        assert!((need_for(&result.raw_materials, "中間").rate_per_min - 30.0).abs() < 1e-6);
    }

    /// 実例: 焔銅塊の6レシピチェーン。息壌ガスが3レシピ(r1/r4/r6)から稼働コストとして
    /// 要求され、かつr6自身の稼働コストでもある(自己消費)+r3の入力材料でもある、という
    /// 多重フィードバックのある実データに近い構成。cycle_secondsはユーザー未指定のため
    /// 全レシピ2秒と仮定したダミー値。
    #[test]
    fn complex_multi_stage_chain_with_shared_self_consumption() {
        let mut r1 = recipe(
            "r1_akadou_gas",
            2.0,
            vec![output("赤銅ガス", 1.0)],
            vec![input("銅塊", 2.0)],
        );
        r1.name = "赤銅ガス生成".to_string();
        r1.equipment_name = "ガス固体変換".to_string();
        r1.operating_costs.push(opcost("息壌ガス", 6.0));

        let mut r2 = recipe(
            "r2_hidou_gas",
            2.0,
            vec![output("緋銅ガス", 2.0)],
            vec![input("赤銅ガス", 2.0), input("分離コア", 1.0)],
        );
        r2.name = "緋銅ガス精錬".to_string();
        r2.equipment_name = "精錬機(安定環境)".to_string();

        let mut r3 = recipe(
            "r3_enrou_gas",
            2.0,
            vec![output("焔銅ガス", 1.0)],
            vec![input("緋銅ガス", 2.0), input("息壌ガス", 1.0)],
        );
        r3.name = "焔銅ガス反応".to_string();
        r3.equipment_name = "反応装置(酸性環境)".to_string();

        let mut r4 = recipe(
            "r4_enrou_katamari",
            2.0,
            vec![output("焔銅塊", 1.0)],
            vec![input("焔銅ガス", 1.0)],
        );
        r4.name = "焔銅塊固体化".to_string();
        r4.equipment_name = "ガス固体変換".to_string();
        r4.operating_costs.push(opcost("息壌ガス", 6.0));

        let mut r5 = recipe(
            "r5_bunri_core",
            2.0,
            vec![output("分離コア", 2.0)],
            vec![input("銅塊", 2.0), input("息壌", 1.0)],
        );
        r5.name = "分離コア成形".to_string();
        r5.equipment_name = "成形機包装機".to_string();

        let mut r6 = recipe(
            "r6_sokujou_gas",
            2.0,
            vec![output("息壌ガス", 1.0)],
            vec![input("息壌", 1.0)],
        );
        r6.name = "息壌ガス化".to_string();
        r6.equipment_name = "ガス固体変換".to_string();
        r6.operating_costs.push(opcost("息壌ガス", 6.0));

        let mut set = recipe_set(vec![r1, r2, r3, r4, r5, r6]);
        set.raw_items.push("銅塊".to_string());
        set.raw_items.push("息壌".to_string());

        let result = calculate(&set, &req("焔銅塊", 12.0)).unwrap();
        assert!(result.warnings.is_empty(), "warnings: {:?}", result.warnings);

        // 手計算で追跡した理論値(全レシピ2s。材料消費/産出は稼働率按分後=台数×フルレート×
        // utilizationで計算(仕様: patch-utilization-consumption)だが、稼働コストは稼働率を
        // 考慮せず台数×フルレートの固定費として計算する(仕様: 稼働コストは稼働率非依存)):
        // r4: 焔銅塊12/min要求 -> 1台(util 12/30=0.4)。入力焔銅ガス需要=1*30*0.4=12/min。
        //     稼働コスト息壌ガス=1*6=6/min(稼働率非依存)。
        // r3: 焔銅ガス12/min要求 -> 1台(util 12/30=0.4)。入力: 緋銅ガス=1*60*0.4=24/min、
        //     息壌ガス=1*30*0.4=12/min。
        // r2: 緋銅ガス24/min要求 -> 1台(util 24/60=0.4)。入力: 赤銅ガス=1*60*0.4=24/min、
        //     分離コア=1*30*0.4=12/min。
        // r1: 赤銅ガス24/min要求 -> 1台(util 24/30=0.8)。入力: 銅塊=1*60*0.8=48/min。
        //     稼働コスト息壌ガス=1*6=6/min(稼働率非依存)。
        // r5: 分離コア12/min要求、1台あたり産出60/min(2個/2s) -> need=12/60=0.2->1台(util 0.2)。
        //     入力: 銅塊=1*60*0.2=12/min、息壌=1*30*0.2=6/min。
        // r6: 息壌ガス需要(r1:6+r3:12+r4:6=24) + 自己消費(1台*6=6、稼働率非依存) -> 収束点は
        //     demand=30, required_float=30/30=1.0 -> 1台(util 1.0、ちょうど満稼働)。
        //     入力: 息壌=1*30*1.0=30/min。稼働コスト息壌ガス=1*6=6/min。
        // 銅塊総需要=48(r1)+12(r5)=60/min。息壌総需要=6(r5)+30(r6)=36/min。
        assert_eq!(step_for(&result, "r4_enrou_katamari").machine_count, 1);
        assert_eq!(step_for(&result, "r3_enrou_gas").machine_count, 1);
        assert_eq!(step_for(&result, "r2_hidou_gas").machine_count, 1);
        assert_eq!(step_for(&result, "r1_akadou_gas").machine_count, 1);
        assert_eq!(step_for(&result, "r5_bunri_core").machine_count, 1);
        assert!((step_for(&result, "r5_bunri_core").utilization - 0.2).abs() < 1e-4);
        assert_eq!(step_for(&result, "r6_sokujou_gas").machine_count, 1);
        assert!((step_for(&result, "r6_sokujou_gas").utilization - 1.0).abs() < 1e-4);

        assert!((need_for(&result.raw_materials, "銅塊").rate_per_min - 60.0).abs() < 1e-4);
        assert!((need_for(&result.raw_materials, "息壌").rate_per_min - 36.0).abs() < 1e-4);
        assert!(result.byproduct_surplus.is_empty());

        // 律速はr6(util=1.0、稼働コストが稼働率非依存の固定費になったことでちょうど満稼働になる)。
        assert_eq!(result.bottleneck.as_deref(), Some("息壌ガス化"));

        // equipment_nameがCalcStepまで伝播していること(表示用フィールドの配線確認)。
        assert_eq!(step_for(&result, "r1_akadou_gas").equipment_name, "ガス固体変換");
        assert_eq!(step_for(&result, "r2_hidou_gas").equipment_name, "精錬機(安定環境)");
    }

    /// 13. 稼働率消費: 切り上げで台数に余裕がある多段チェーンで、下流消費が
    ///     「台数×フルレート」ではなく「×utilization」で計算されること。律速産出の
    ///     実効産出がrequiredにちょうど一致し、切り上げ由来の過剰原料所要が出ないこと。
    #[test]
    fn utilization_scales_downstream_consumption() {
        let set = recipe_set(vec![
            recipe("down", 2.0, vec![output("製品", 1.0)], vec![input("中間", 1.0)]),
            recipe("up", 2.0, vec![output("中間", 1.0)], vec![input("原料", 1.0)]),
        ]);
        // per machine出力=30/min。目標31/min -> need=31/30=1.0333 -> ceil=2台、util=(31/30)/2=31/60。
        let result = calculate(&set, &req("製品", 31.0)).unwrap();
        let down = step_for(&result, "down");
        assert_eq!(down.machine_count, 2);
        assert!((down.utilization - 31.0 / 60.0).abs() < 1e-6);
        // 律速産出(製品)の実効産出はrequired(31)にちょうど一致する。
        assert!((need_for(&down.outputs_effective, "製品").rate_per_min - 31.0).abs() < 1e-4);
        // 下流(down)がupに要求する中間消費は、フルレート(2*30=60)ではなくutilization按分後の31。
        assert!((need_for(&down.inputs_demand, "中間").rate_per_min - 31.0).abs() < 1e-4);

        // upも同じ需要31/minを受けて同様にceil=2台・util=0.5相当で稼働し、原料所要は
        // フルレート(60)ではなく31になる(切り上げ由来の過剰原料所要が出ない)。
        let up = step_for(&result, "up");
        assert_eq!(up.machine_count, 2);
        assert!((need_for(&result.raw_materials, "原料").rate_per_min - 31.0).abs() < 1e-4);
        assert!(result.byproduct_surplus.is_empty());
    }

    /// 14. 回帰確認: 産出比由来の副産物余剰は、律速側がutilization<1で稼働していても
    ///     消えずに残ること(ゼロ除算や誤った消去をしていないことの確認)。
    #[test]
    fn byproduct_surplus_persists_with_partial_utilization() {
        let cu = recipe(
            "cu",
            2.0,
            vec![output("銅塊", 1.0), output("汚水", 3.0)],
            vec![input("銅鉱石", 1.0), input("水", 1.0)],
        );
        let set = recipe_set(vec![cu]);
        // 銅塊のみ31/min要求。per machine=30/min -> ceil(31/30)=2台、util=31/60=0.51666...
        let result = calculate(&set, &req("銅塊", 31.0)).unwrap();
        let step = step_for(&result, "cu");
        assert_eq!(step.machine_count, 2);
        assert!((step.utilization - 31.0 / 60.0).abs() < 1e-6);
        // 銅塊(律速産出)はちょうどrequiredに一致し余剰0。
        assert!((need_for(&step.outputs_effective, "銅塊").rate_per_min - 31.0).abs() < 1e-4);
        // 汚水は誰も要求していないので全量surplusだが、値はutilization按分後
        // (2台*90/min*31/60 = 93.0)であり、フルレート(180.0)でもゼロでもない。
        let surplus = need_for(&result.byproduct_surplus, "汚水");
        assert!((surplus.rate_per_min - 93.0).abs() < 1e-4);
    }
}
