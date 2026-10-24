// エンドフィールド レシピ計算機の計算エンジン（純粋関数。DOM 非依存。表示は render.js）。
// RecipeSet + CalcRequest -> CalcResult のみを扱う。アルゴリズムの詳細は EFRecipeCalculator.md §4。
// フィールド名は JSON エクスポート・presets.json と同じ snake_case。

export const MAX_ITER = 100;
const EPSILON = 1e-6;

function perMin(qty, cycleSeconds) {
  return (qty / cycleSeconds) * 60.0;
}

function addTo(map, key, value) {
  map.set(key, (map.get(key) ?? 0.0) + value);
}

// アイテム→産出レシピの索引。selected_recipe_ids の順に構築し、同一アイテムを
// 産出するレシピが複数選択されていれば先頭採用＋warning（代替レシピ非対応）。
function buildItemRecipeIndex(set, warnings) {
  const index = new Map();
  for (const id of set.selected_recipe_ids) {
    const recipe = set.recipes.find((r) => r.id === id);
    if (!recipe) continue;
    for (const output of recipe.outputs) {
      const existing = index.get(output.item);
      if (existing) {
        if (existing.id !== recipe.id) {
          warnings.push(
            `アイテム『${output.item}』を産出するレシピが複数選択されています(『${existing.name}』と『${recipe.name}』)。『${existing.name}』を採用します。`
          );
        }
        continue;
      }
      index.set(output.item, recipe);
    }
  }
  return index;
}

function buildExternalSupplyMap(set) {
  const map = new Map();
  for (const supply of set.external_supplies) {
    map.set(supply.item, supply.max_rate_per_min);
  }
  return map;
}

// 1反復分の需要展開（仕様§4.2 手順1）。
function expandDemand(demand, rawItems, externalSupply, itemRecipe) {
  const rawMaterials = new Map();
  // item -> {used, cap}
  const minedUsage = new Map();
  // recipe_id -> 必要台数(浮動)。同一レシピを要求する複数アイテムがあれば max。
  const requiredFloat = new Map();
  // item -> その item の需要のうちレシピ生産で充当された残余(=レシピへの要求量)。
  const outputUsage = new Map();
  const warnings = [];

  for (const [item, r] of demand) {
    if (rawItems.has(item)) {
      addTo(rawMaterials, item, r);
      continue;
    }

    let residual = r;
    if (externalSupply.has(item)) {
      const cap = externalSupply.get(item);
      const used = Math.min(r, cap);
      if (!minedUsage.has(item)) minedUsage.set(item, { used: 0.0, cap });
      minedUsage.get(item).used += used;
      residual = Math.max(r - used, 0.0);
    }

    if (residual <= 0.0) continue;

    const recipe = itemRecipe.get(item);
    if (recipe) {
      const output = recipe.outputs.find((o) => o.item === item);
      const perMachine = perMin(output.qty, recipe.cycle_seconds);
      if (perMachine <= 0.0) {
        warnings.push(`レシピ『${recipe.name}』の産出『${item}』の量が0以下のため計算できません。`);
        continue;
      }
      const need = residual / perMachine;
      if (!requiredFloat.has(recipe.id)) requiredFloat.set(recipe.id, 0.0);
      if (need > requiredFloat.get(recipe.id)) requiredFloat.set(recipe.id, need);
      outputUsage.set(item, residual);
    } else {
      warnings.push(`『${item}』は原料指定もレシピもありません。原料として扱います。`);
      addTo(rawMaterials, item, residual);
    }
  }

  return { rawMaterials, minedUsage, requiredFloat, outputUsage, warnings };
}

// 仕様§4.2 手順2〜4: 台数確定 + 実効算出 + 次期demand構築。
function resolveMachines(targetItem, targetRate, requiredFloat, outputUsage, recipesById) {
  const machines = new Map();
  for (const [rid, f] of requiredFloat) {
    machines.set(rid, Math.max(Math.ceil(f), 1.0));
  }

  const nextDemand = new Map([[targetItem, targetRate]]);
  const byproductSurplus = new Map();

  for (const [rid, count] of machines) {
    const recipe = recipesById.get(rid);
    // オンデマンド生産: 切り上げで余裕のある工程は稼働率(<=1)ぶんだけしか回らない。
    // 消費・産出はすべて台数×フルレート×稼働率で計算する(仕様: patch-utilization-consumption)。
    const required = requiredFloat.get(rid) ?? 0.0;
    const utilization = count > 0 ? required / count : 0.0;

    for (const output of recipe.outputs) {
      const effective = count * perMin(output.qty, recipe.cycle_seconds) * utilization;
      const charged = outputUsage.get(output.item) ?? 0.0;
      const surplus = Math.max(effective - charged, 0.0);
      if (surplus > EPSILON) addTo(byproductSurplus, output.item, surplus);
    }
    for (const input of recipe.inputs) {
      addTo(nextDemand, input.item, count * perMin(input.qty, recipe.cycle_seconds) * utilization);
    }
    for (const oc of recipe.operating_costs) {
      // 稼働コストは入力材料と同じ扱い(台数×フルレート×稼働率)で計算する。
      addTo(nextDemand, oc.item, count * oc.rate_per_min * utilization);
    }
  }

  return { machines, nextDemand, byproductSurplus };
}

function mapsClose(a, b, eps) {
  const closeOneWay = (x, y) => {
    for (const [k, v] of x) {
      if (!(Math.abs((y.get(k) ?? 0.0) - v) < eps)) return false;
    }
    return true;
  };
  return closeOneWay(a, b) && closeOneWay(b, a);
}

// 順序を見ない比較（Rust の IndexMap の == と同じ）。
function mapsEqual(a, b) {
  if (a.size !== b.size) return false;
  for (const [k, v] of a) {
    if (!b.has(k) || b.get(k) !== v) return false;
  }
  return true;
}

// target_item を産出するレシピを根に、実際に使用中(machines>0)のレシピだけを
// 逆方向BFSして段数を振る(根=0)。自己消費による循環はBFSの既訪問チェックで自然に止まる。
function computeDepths(targetItem, itemRecipe, machines) {
  const depths = new Map();
  const queue = [];

  const root = itemRecipe.get(targetItem);
  if (root && machines.has(root.id)) {
    depths.set(root.id, 0);
    queue.push([root.id, 0]);
  }

  while (queue.length > 0) {
    const [rid, depth] = queue.shift();
    const recipe = [...itemRecipe.values()].find((r) => r.id === rid);
    if (!recipe) continue;
    const downstreamItems = [
      ...recipe.inputs.map((i) => i.item),
      ...recipe.operating_costs.map((o) => o.item),
    ];
    for (const item of downstreamItems) {
      const next = itemRecipe.get(item);
      if (next && machines.has(next.id) && !depths.has(next.id)) {
        depths.set(next.id, depth + 1);
        queue.push([next.id, depth + 1]);
      }
    }
  }

  return depths;
}

function buildSteps(machines, requiredFloat, outputUsage, recipesById, depths) {
  return [...machines].map(([rid, count]) => {
    const recipe = recipesById.get(rid);
    const required = requiredFloat.get(rid) ?? 0.0;

    let limitingOutput = "";
    let best = -1.0;
    for (const output of recipe.outputs) {
      const charged = outputUsage.get(output.item) ?? 0.0;
      const perMachine = perMin(output.qty, recipe.cycle_seconds);
      const need = perMachine > 0.0 ? charged / perMachine : 0.0;
      if (need > best) {
        best = need;
        limitingOutput = output.item;
      }
    }

    const utilization = count > 0 ? required / count : 0.0;

    return {
      recipe_id: rid,
      recipe_name: recipe.name,
      equipment_name: recipe.equipment_name ?? "",
      machine_count: count,
      limiting_output: limitingOutput,
      utilization,
      outputs_effective: recipe.outputs.map((o) => ({
        item: o.item,
        rate_per_min: count * perMin(o.qty, recipe.cycle_seconds) * utilization,
      })),
      inputs_demand: recipe.inputs.map((i) => ({
        item: i.item,
        rate_per_min: count * perMin(i.qty, recipe.cycle_seconds) * utilization,
      })),
      operating_demand: recipe.operating_costs.map((oc) => ({
        item: oc.item,
        rate_per_min: count * oc.rate_per_min * utilization,
      })),
      depth: depths.get(rid) ?? 0,
    };
  });
}

// 省略可能なフィールドを既定値で埋める（インポートした古い JSON でも動くように）。
function normalizeSet(set) {
  return {
    recipes: (set.recipes ?? []).map((r) => ({
      ...r,
      equipment_name: r.equipment_name ?? "",
      outputs: r.outputs ?? [],
      inputs: r.inputs ?? [],
      operating_costs: r.operating_costs ?? [],
    })),
    selected_recipe_ids: set.selected_recipe_ids ?? [],
    raw_items: set.raw_items ?? [],
    external_supplies: set.external_supplies ?? [],
  };
}

// エンジンのエントリポイント。{ error } は明確な入力異常のみ。それ以外(未選択レシピで
// 経路が切れる・代替レシピ衝突・反復収束失敗等)は result.warnings に積んで { result } を返す
// (黙って誤答しない = 警告付きで最終反復時点の値を返す、という意味)。
export function calculate(recipeSet, request) {
  const rate = request.target_rate_per_min;
  if (typeof rate !== "number" || !Number.isFinite(rate) || rate <= 0.0) {
    return { error: "目標レートは正の数で指定してください。" };
  }
  const set = normalizeSet(recipeSet);
  const targetItem = request.target_item;

  const warnings = [];
  const itemRecipe = buildItemRecipeIndex(set, warnings);
  const externalSupply = buildExternalSupplyMap(set);
  const rawItems = new Set(set.raw_items);
  const recipesById = new Map(set.recipes.map((r) => [r.id, r]));

  let demand = new Map([[targetItem, rate]]);
  let machines = new Map();
  let lastExpansion;
  let lastResolution;
  let converged = false;
  let iterationsUsed = 0;

  do {
    lastExpansion = expandDemand(demand, rawItems, externalSupply, itemRecipe);
    lastResolution = resolveMachines(
      targetItem,
      rate,
      lastExpansion.requiredFloat,
      lastExpansion.outputUsage,
      recipesById
    );
    converged =
      mapsClose(demand, lastResolution.nextDemand, EPSILON) &&
      mapsEqual(machines, lastResolution.machines);
    demand = lastResolution.nextDemand;
    machines = lastResolution.machines;
    iterationsUsed += 1;
  } while (!converged && iterationsUsed < MAX_ITER);

  warnings.push(...lastExpansion.warnings);
  if (!converged) {
    warnings.push(`反復が収束しませんでした(最大${MAX_ITER}回)。表示値は最終反復時点の近似値です。`);
  }

  const depths = computeDepths(targetItem, itemRecipe, machines);
  const steps = buildSteps(
    machines,
    lastExpansion.requiredFloat,
    lastExpansion.outputUsage,
    recipesById,
    depths
  );

  let bottleneck = null;
  for (const step of steps) {
    if (bottleneck === null || step.utilization > bottleneck.utilization) bottleneck = step;
  }

  return {
    result: {
      steps,
      raw_materials: [...lastExpansion.rawMaterials].map(([item, rate_per_min]) => ({
        item,
        rate_per_min,
      })),
      mined_usage: [...lastExpansion.minedUsage].map(([item, { used, cap }]) => ({
        item,
        used_rate: used,
        surplus_rate: Math.max(cap - used, 0.0),
        cap_rate: cap,
      })),
      byproduct_surplus: [...lastResolution.byproductSurplus].map(([item, rate_per_min]) => ({
        item,
        rate_per_min,
      })),
      bottleneck: bottleneck ? bottleneck.recipe_name : null,
      warnings,
    },
  };
}
