// エンドフィールド レシピ計算機の計算層（static/solver.js）の検証スクリプト。
// Rust 版 solver.rs のテストを同じ入力・同じ期待値で移したもの＋同梱プリセットの計算確認。
//
// 実行方法（node は PATH に無いので絶対パスで）:
//   & "C:\Program Files\nodejs\node.exe" src/api/ef_recipe_calculator/verify.mjs

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { calculate } from "./static/solver.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));

let fail = 0;
let pass = 0;
function ok(name, cond, extra) {
  if (cond) pass++;
  else {
    fail++;
    console.log("FAIL  " + name + (extra !== undefined ? "  -> " + JSON.stringify(extra) : ""));
  }
}
function near(a, b, eps) {
  return Math.abs(a - b) < eps;
}

const output = (item, qty) => ({ item, qty });
const input = (item, qty) => ({ item, qty });
const opcost = (item, rate_per_min) => ({ item, rate_per_min });
const recipe = (id, cycle_seconds, outputs, inputs) => ({
  id,
  name: id,
  equipment_name: "",
  cycle_seconds,
  outputs,
  inputs,
  operating_costs: [],
});
const recipeSet = (recipes) => ({
  recipes,
  selected_recipe_ids: recipes.map((r) => r.id),
  raw_items: [],
  external_supplies: [],
});
const req = (target_item, rate) => ({ target_item, target_rate_per_min: rate });
function run(set, request) {
  const { result, error } = calculate(set, request);
  if (error) throw new Error("unexpected error: " + error);
  return result;
}
const stepFor = (result, id) => {
  const s = result.steps.find((s) => s.recipe_id === id);
  if (!s) throw new Error("step not found: " + id);
  return s;
};
const needFor = (needs, item) => {
  const n = needs.find((n) => n.item === item);
  if (!n) throw new Error("material need not found: " + item);
  return n;
};
const hasNotConverged = (result) => result.warnings.some((w) => w.includes("収束しませんでした"));

// 1. 単段: 原料→最終製品1段。台数・原料レートが正しい。
{
  const set = recipeSet([recipe("r1", 2.0, [output("製品", 1.0)], [input("原料", 1.0)])]);
  set.raw_items.push("原料");
  const result = run(set, req("製品", 30.0));
  ok("1 single_stage machine_count", stepFor(result, "r1").machine_count === 1);
  ok("1 single_stage raw", near(needFor(result.raw_materials, "原料").rate_per_min, 30.0, 1e-9));
  ok("1 single_stage no warnings", result.warnings.length === 0, result.warnings);
}

// 2. 多段: 3段以上。中間素材の需要伝播が正しい。
{
  const set = recipeSet([
    recipe("c", 2.0, [output("完成品", 1.0)], [input("中間B", 1.0)]),
    recipe("b", 2.0, [output("中間B", 1.0)], [input("中間A", 1.0)]),
    recipe("a", 2.0, [output("中間A", 1.0)], [input("原料", 1.0)]),
  ]);
  const result = run(set, req("完成品", 30.0));
  for (const id of ["c", "b", "a"]) {
    ok(`2 multi_stage ${id}`, stepFor(result, id).machine_count === 1);
  }
  ok("2 multi_stage raw", near(needFor(result.raw_materials, "原料").rate_per_min, 30.0, 1e-6));
}

// 3. 合流: 複数製品(A,B)が同一中間素材Cを要求 → 需要が合算される。
{
  const set = recipeSet([
    recipe("final", 2.0, [output("完成品", 1.0)], [input("A", 1.0), input("B", 1.0)]),
    recipe("a", 2.0, [output("A", 1.0)], [input("C", 1.0)]),
    recipe("b", 2.0, [output("B", 1.0)], [input("C", 2.0)]),
  ]);
  const result = run(set, req("完成品", 30.0));
  ok("3 merging_demand", near(needFor(result.raw_materials, "C").rate_per_min, 90.0, 1e-6));
}

// 4. 切り上げ: 端数が出るケースで machine_count が ceil・最小1になる。
{
  const set = recipeSet([recipe("r1", 2.0, [output("製品", 1.0)], [input("原料", 1.0)])]);
  ok("4 ceiling", stepFor(run(set, req("製品", 31.0)), "r1").machine_count === 2);
  ok("4 minimum one", stepFor(run(set, req("製品", 0.001)), "r1").machine_count === 1);
}

// 5. サイクル混在: 2s/10s/20s・産出1/2個の混在で毎分換算が正しい。
{
  const set = recipeSet([
    recipe("r2s", 2.0, [output("製品", 1.0)], [input("原料2s", 1.0)]),
    recipe("r10s", 10.0, [output("原料2s", 2.0)], [input("原料10s", 1.0)]),
    recipe("r20s", 20.0, [output("原料10s", 1.0)], [input("原料20s", 2.0)]),
  ]);
  const result = run(set, req("製品", 30.0));
  ok("5 mixed r2s", stepFor(result, "r2s").machine_count === 1);
  ok("5 mixed r10s", stepFor(result, "r10s").machine_count === 3);
  ok("5 mixed r20s", stepFor(result, "r20s").machine_count === 5);
  ok("5 mixed raw", near(needFor(result.raw_materials, "原料20s").rate_per_min, 30.0, 1e-4));
}

// 6. 採掘控除: 採掘上限<需要のとき不足分だけレシピ生産、採掘は使い切り。
//    採掘上限>需要のとき余剰報告、レシピ台数0。
{
  const set = recipeSet([recipe("r1", 2.0, [output("素材", 1.0)], [input("原料", 1.0)])]);
  set.external_supplies.push({ item: "素材", max_rate_per_min: 10.0 });
  set.raw_items.push("原料");

  const result = run(set, req("素材", 30.0));
  const mined = result.mined_usage.find((m) => m.item === "素材");
  ok("6 mined used", near(mined.used_rate, 10.0, 1e-6));
  ok("6 mined surplus", near(mined.surplus_rate, 0.0, 1e-6));
  ok("6 recipe used", result.steps.some((s) => s.recipe_id === "r1"));
  ok("6 raw scaled by utilization", near(needFor(result.raw_materials, "原料").rate_per_min, 20.0, 1e-4));
  ok("6 no byproduct surplus", result.byproduct_surplus.length === 0);

  const result2 = run(set, req("素材", 5.0));
  const mined2 = result2.mined_usage.find((m) => m.item === "素材");
  ok("6 mined-only used", near(mined2.used_rate, 5.0, 1e-6));
  ok("6 mined-only surplus", near(mined2.surplus_rate, 5.0, 1e-6));
  ok("6 mined-only no step", !result2.steps.some((s) => s.recipe_id === "r1"));
}

// 7. 稼働コスト(自己消費): 息壌1→息壌ガス1、稼働に息壌ガス6/min。反復で正しく収束。
{
  const r = recipe("gas", 2.0, [output("息壌ガス", 1.0)], [input("息壌", 1.0)]);
  r.operating_costs.push(opcost("息壌ガス", 6.0));
  const set = recipeSet([r]);
  set.raw_items.push("息壌");
  const result = run(set, req("息壌ガス", 24.0));
  ok("7 self consumption converged", !hasNotConverged(result));
  ok("7 self consumption machines", stepFor(result, "gas").machine_count === 1);
  ok("7 self consumption raw", near(needFor(result.raw_materials, "息壌").rate_per_min, 30.0, 1e-4));
}

// 8. 反復収束: 稼働コスト連鎖で数回反復して収束。収束値が理論値と一致。
{
  const r = recipe("gas", 2.0, [output("息壌ガス", 1.0)], [input("息壌", 1.0)]);
  r.operating_costs.push(opcost("息壌ガス", 6.0));
  const set = recipeSet([r]);
  set.raw_items.push("息壌");
  const result = run(set, req("息壌ガス", 50.0));
  ok("8 convergence machines", stepFor(result, "gas").machine_count === 3);
  ok("8 convergence converged", !hasNotConverged(result));
}

// 9. 収束失敗: 意図的に発散する構成でN回打ち切り+warningが出る。
{
  const r = recipe("gas", 2.0, [output("息壌ガス", 1.0)], [input("息壌", 1.0)]);
  r.operating_costs.push(opcost("息壌ガス", 40.0)); // per machine output=30/minを上回る
  const set = recipeSet([r]);
  set.raw_items.push("息壌");
  ok("9 divergent warns", hasNotConverged(run(set, req("息壌ガス", 10.0))));
}

// 10. 副産物max合わせ: 銅鉱石+水→銅塊+汚水。
{
  const cu = recipe(
    "cu",
    2.0,
    [output("銅塊", 1.0), output("汚水", 1.0)],
    [input("銅鉱石", 1.0), input("水", 1.0)]
  );
  const set = recipeSet([cu]);

  const onlyIngot = run(set, req("銅塊", 30.0));
  ok("10 only ingot machines", stepFor(onlyIngot, "cu").machine_count === 1);
  ok("10 only ingot surplus", near(needFor(onlyIngot.byproduct_surplus, "汚水").rate_per_min, 30.0, 1e-6));

  const onlySewage = run(set, req("汚水", 30.0));
  ok("10 only sewage machines", stepFor(onlySewage, "cu").machine_count === 1);
  ok("10 only sewage surplus", near(needFor(onlySewage.byproduct_surplus, "銅塊").rate_per_min, 30.0, 1e-6));

  const consumer = recipe(
    "consumer",
    2.0,
    [output("最終品", 1.0)],
    [input("銅塊", 1.0), input("汚水", 2.0)]
  );
  const both = run(recipeSet([structuredClone(cu), consumer]), req("最終品", 30.0));
  ok("10 both machines", stepFor(both, "cu").machine_count === 2);
  ok("10 both surplus", near(needFor(both.byproduct_surplus, "銅塊").rate_per_min, 30.0, 1e-6));
}

// 11. 原料の底指定: raw_items に息壌を入れると展開停止し、原料として所要レート報告。
{
  const r = recipe("gas", 2.0, [output("息壌ガス", 1.0)], [input("息壌", 1.0)]);
  const producer = recipe("soil", 2.0, [output("息壌", 1.0)], [input("何か", 1.0)]);
  const set = recipeSet([r, producer]);
  set.raw_items.push("息壌");
  const result = run(set, req("息壌ガス", 30.0));
  ok("11 raw override rate", near(needFor(result.raw_materials, "息壌").rate_per_min, 30.0, 1e-6));
  ok("11 raw override no step", !result.steps.some((s) => s.recipe_id === "soil"));
}

// 12. レシピ選択: selected_recipe_ids に含まれないレシピは索引に入らない。
//     未選択で経路が切れる場合は未定義warning。
{
  const r1 = recipe("r1", 2.0, [output("製品", 1.0)], [input("中間", 1.0)]);
  const r2 = recipe("r2", 2.0, [output("中間", 1.0)], [input("原料", 1.0)]);
  const set = recipeSet([r1, r2]);
  set.selected_recipe_ids = ["r1"];
  const result = run(set, req("製品", 30.0));
  ok(
    "12 unselected warns",
    result.warnings.some((w) => w.includes("中間") && w.includes("原料として扱います")),
    result.warnings
  );
  ok("12 unselected no step", !result.steps.some((s) => s.recipe_id === "r2"));
  ok("12 unselected raw", near(needFor(result.raw_materials, "中間").rate_per_min, 30.0, 1e-6));
}

// 実例: 焔銅塊の6レシピチェーン。息壌ガスが3レシピ(r1/r4/r6)から稼働コストとして
// 要求され、かつr6自身の稼働コストでもある(自己消費)+r3の入力材料でもある、という
// 多重フィードバックのある実データに近い構成（理論値の追跡は Rust 版のコメントを参照）。
{
  const named = (r, name, equipment, opcosts = []) => {
    r.name = name;
    r.equipment_name = equipment;
    r.operating_costs.push(...opcosts);
    return r;
  };
  const r1 = named(
    recipe("r1_akadou_gas", 2.0, [output("赤銅ガス", 1.0)], [input("銅塊", 2.0)]),
    "赤銅ガス生成",
    "ガス固体変換",
    [opcost("息壌ガス", 6.0)]
  );
  const r2 = named(
    recipe("r2_hidou_gas", 2.0, [output("緋銅ガス", 2.0)], [input("赤銅ガス", 2.0), input("分離コア", 1.0)]),
    "緋銅ガス精錬",
    "精錬機(安定環境)"
  );
  const r3 = named(
    recipe("r3_enrou_gas", 2.0, [output("焔銅ガス", 1.0)], [input("緋銅ガス", 2.0), input("息壌ガス", 1.0)]),
    "焔銅ガス反応",
    "反応装置(酸性環境)"
  );
  const r4 = named(
    recipe("r4_enrou_katamari", 2.0, [output("焔銅塊", 1.0)], [input("焔銅ガス", 1.0)]),
    "焔銅塊固体化",
    "ガス固体変換",
    [opcost("息壌ガス", 6.0)]
  );
  const r5 = named(
    recipe("r5_bunri_core", 2.0, [output("分離コア", 2.0)], [input("銅塊", 2.0), input("息壌", 1.0)]),
    "分離コア成形",
    "成形機包装機"
  );
  const r6 = named(
    recipe("r6_sokujou_gas", 2.0, [output("息壌ガス", 1.0)], [input("息壌", 1.0)]),
    "息壌ガス化",
    "ガス固体変換",
    [opcost("息壌ガス", 6.0)]
  );
  const set = recipeSet([r1, r2, r3, r4, r5, r6]);
  set.raw_items.push("銅塊", "息壌");

  const result = run(set, req("焔銅塊", 12.0));
  ok("chain no warnings", result.warnings.length === 0, result.warnings);
  for (const id of [
    "r4_enrou_katamari",
    "r3_enrou_gas",
    "r2_hidou_gas",
    "r1_akadou_gas",
    "r5_bunri_core",
    "r6_sokujou_gas",
  ]) {
    ok(`chain ${id} machines`, stepFor(result, id).machine_count === 1);
  }
  ok("chain r5 utilization", near(stepFor(result, "r5_bunri_core").utilization, 0.2, 1e-4));
  ok("chain raw 銅塊", near(needFor(result.raw_materials, "銅塊").rate_per_min, 60.0, 1e-4));
  ok("chain raw 息壌", near(needFor(result.raw_materials, "息壌").rate_per_min, 30.0, 1e-4));
  ok("chain no byproduct surplus", result.byproduct_surplus.length === 0);
  // 律速はr1(util≈0.8)。r6もutil≈0.8で肉薄するが、自己消費の反復収束によるごく僅かな残差で
  // r1よりわずかに低くなるため、r1が採用される。
  ok("chain bottleneck", result.bottleneck === "赤銅ガス生成", result.bottleneck);
  ok("chain equipment r1", stepFor(result, "r1_akadou_gas").equipment_name === "ガス固体変換");
  ok("chain equipment r2", stepFor(result, "r2_hidou_gas").equipment_name === "精錬機(安定環境)");
}

// 13. 稼働率消費: 切り上げで台数に余裕がある多段チェーンで、下流消費が
//     「台数×フルレート」ではなく「×utilization」で計算されること。
{
  const set = recipeSet([
    recipe("down", 2.0, [output("製品", 1.0)], [input("中間", 1.0)]),
    recipe("up", 2.0, [output("中間", 1.0)], [input("原料", 1.0)]),
  ]);
  const result = run(set, req("製品", 31.0));
  const down = stepFor(result, "down");
  ok("13 down machines", down.machine_count === 2);
  ok("13 down utilization", near(down.utilization, 31.0 / 60.0, 1e-6));
  ok("13 down output", near(needFor(down.outputs_effective, "製品").rate_per_min, 31.0, 1e-4));
  ok("13 down input", near(needFor(down.inputs_demand, "中間").rate_per_min, 31.0, 1e-4));
  ok("13 up machines", stepFor(result, "up").machine_count === 2);
  ok("13 raw", near(needFor(result.raw_materials, "原料").rate_per_min, 31.0, 1e-4));
  ok("13 no byproduct surplus", result.byproduct_surplus.length === 0);
}

// 14. 回帰確認: 産出比由来の副産物余剰は、律速側がutilization<1で稼働していても
//     消えずに残ること。
{
  const cu = recipe(
    "cu",
    2.0,
    [output("銅塊", 1.0), output("汚水", 3.0)],
    [input("銅鉱石", 1.0), input("水", 1.0)]
  );
  const result = run(recipeSet([cu]), req("銅塊", 31.0));
  const step = stepFor(result, "cu");
  ok("14 machines", step.machine_count === 2);
  ok("14 utilization", near(step.utilization, 31.0 / 60.0, 1e-6));
  ok("14 limiting output", near(needFor(step.outputs_effective, "銅塊").rate_per_min, 31.0, 1e-4));
  ok("14 surplus", near(needFor(result.byproduct_surplus, "汚水").rate_per_min, 93.0, 1e-4));
}

// 入力エラー: 目標レートが正の数でない（空欄の parseFloat は NaN）。
{
  const set = recipeSet([recipe("r1", 2.0, [output("製品", 1.0)], [])]);
  for (const rate of [0, -1, NaN, Infinity, null]) {
    const { result, error } = calculate(set, req("製品", rate));
    ok(`input error rate=${rate}`, result === undefined && error === "目標レートは正の数で指定してください。");
  }
}

// インポートした JSON で省略可能なフィールド（equipment_name・operating_costs・raw_items・
// external_supplies）が無くても計算できる（Rust 版の serde(default) と同じ）。
{
  const set = {
    recipes: [{ id: "r1", name: "r1", cycle_seconds: 2.0, outputs: [output("製品", 1.0)], inputs: [] }],
    selected_recipe_ids: ["r1"],
  };
  const result = run(set, req("製品", 30.0));
  ok("defaults machine_count", stepFor(result, "r1").machine_count === 1);
  ok("defaults equipment_name", stepFor(result, "r1").equipment_name === "");
}

// 同梱プリセット(static/presets.json)が warning なしで計算できること
// （フロントはこのファイルを fetch してそのまま計算に渡す）。
{
  const file = JSON.parse(fs.readFileSync(path.join(HERE, "static", "presets.json"), "utf8"));
  ok("presets not empty", file.presets.length > 0);
  for (const preset of file.presets) {
    const { result, error } = calculate(
      preset.recipe_set,
      req(preset.default_target_item, preset.default_target_rate_per_min)
    );
    ok(`preset ${preset.name} no error`, error === undefined, error);
    ok(`preset ${preset.name} has steps`, result !== undefined && result.steps.length > 0);
    ok(`preset ${preset.name} no warnings`, result !== undefined && result.warnings.length === 0, result?.warnings);
  }
}

console.log(`${pass} passed, ${fail} failed`);
if (fail > 0) process.exit(1);
