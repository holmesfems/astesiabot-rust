// フレームキル計算機の計算層(engine.js)の検証スクリプト。DOM非依存の純粋関数を
// 直接importして検証する（lod_chest_solver/verify.mjsと同じ手法。実サーバー起動不要）。
//
// 実行方法:
//   & "C:\Program Files\nodejs\node.exe" src/api/fk_kill_calculator/verify.mjs

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import {
  computeRowDamage,
  computeTotal,
  dropStaleRows,
  suggest,
  describeSuggestion,
  findSingleTargetConflicts,
  computeInspireSource,
  computeInspireSelfParts,
  computeInspireForRow,
  defaultInspireSourceCfg,
  computeBuffBreakdown,
  resolveConditionalValue,
  potentialGroups,
  computeBaseAtk,
  resolveAtk,
  moduleUsable,
  effectiveModuleId,
  maxEliteFor,
  maxLevelForElite,
  makeDefaultRow,
  skillUnlockWarning,
  valueAtLevel,
  resolveMultiplierAtLevel,
  resolveSelfAtkPctAtLevel,
  resolveEntryValues,
  maxSkillLevelForElite,
  skillLevelWarning,
  resolveInspireRatioAtLevel,
} from "./static/engine.js";

let allOk = true;
function check(label, cond, extra) {
  console.log(`  [${cond ? "OK" : "NG"}] ${label}${extra !== undefined ? "  -> " + JSON.stringify(extra) : ""}`);
  if (!cond) allOk = false;
}
function approxEqual(actual, expected, tolerance, label) {
  const cond = Math.abs(actual - expected) <= tolerance;
  check(`${label}: ${actual} ≈ ${expected} (±${tolerance})`, cond);
}

const enemyNeutral = { hp: 1_000_000, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 };

function op(atkBase, atkPotential, moduleLv3Atk) {
  return {
    id: "op",
    name: "op",
    tags: [],
    atkBase,
    atkPotential,
    modules: moduleLv3Atk === null ? [] : [{ id: "m", typeName: "X", name: "m", atkByLevel: [0, 0, moduleLv3Atk] }],
    fkEntries: [],
  };
}
// デフォルトは op() が作る唯一のモジュール"m"を使う想定。モジュール無しのopで
// 使うときは呼び出し側で明示的に `moduleId: null` を渡す(spreadで上書きされる)。
function row(overrides) {
  return {
    opId: "op",
    entryIdx: 0,
    dmgType: "true",
    potential: true,
    moduleId: "m",
    moduleLv: 3,
    multiplier: 1,
    selfPct: 0,
    hits: 1,
    buffPct: 0,
    dmgMult: 1,
    ignoreDef: 0,
    ...overrides,
  };
}

console.log("=== オーナー参照スプレッドシートの実測値(最終ATK) ===\n");
{
  const o = op(418, 27, 35);
  const r = row({ multiplier: 0.6, selfPct: 0.09 });
  const { final } = computeRowDamage(o, r, enemyNeutral);
  approxEqual(final, 314, 1, "濁心スカジ S2");
}
{
  const o = op(825, 28, 86);
  const r = row({ multiplier: 4, selfPct: 0.772 });
  const { final } = computeRowDamage(o, r, enemyNeutral);
  approxEqual(final, 6656, 1, "ブレイズ S3");
}
{
  const o = op(624, 27, 40);
  const r = row({ multiplier: 4, buffPct: 1.5 });
  const { final } = computeRowDamage(o, r, enemyNeutral);
  approxEqual(final, 6910, 1, "Ash S3 400%");
}
{
  const o = op(1175, 35, 87);
  const r = row({ multiplier: 4.65, buffPct: 1.86 });
  const { final } = computeRowDamage(o, r, enemyNeutral);
  approxEqual(final, 17249, 1, "FW S2");
}

console.log("\n=== 手計算のダメージケース ===\n");
{
  // 物理: final=1000, 防御300 -> 700 (floorの5%=50より大きいのでfloor未発動)
  const o = op(1000, 0, null);
  const r = row({ dmgType: "physical", moduleId: null });
  const enemy = { ...enemyNeutral, def: 300 };
  const { perHit, atFloor } = computeRowDamage(o, r, enemy);
  approxEqual(perHit, 700, 0.001, "物理 vs 防御300");
  check("物理 vs 防御300: floor未発動", atFloor === false);
}
{
  // 術: final=1000, 術耐性30 -> 700
  const o = op(1000, 0, null);
  const r = row({ dmgType: "arts", moduleId: null });
  const enemy = { ...enemyNeutral, res: 30 };
  const { perHit, atFloor } = computeRowDamage(o, r, enemy);
  approxEqual(perHit, 700, 0.001, "術 vs 術耐性30");
  check("術 vs 術耐性30: floor未発動", atFloor === false);
}
{
  // 5%floor: final=1000, 防御を極端に大きくすると 1000-defEff < 0 なので floor(=50)が効く
  const o = op(1000, 0, null);
  const r = row({ dmgType: "physical", moduleId: null });
  const enemy = { ...enemyNeutral, def: 100000 };
  const { perHit, atFloor } = computeRowDamage(o, r, enemy);
  approxEqual(perHit, 50, 0.001, "5%floor発動時のperHit");
  check("5%floor発動時: atFloor=true", atFloor === true);
}
{
  // 真ダメージ: 防御/術耐性を無視してfinalそのまま
  const o = op(1000, 0, null);
  const r = row({ dmgType: "true", moduleId: null });
  const enemy = { ...enemyNeutral, def: 100000, res: 100 };
  const { perHit, atFloor } = computeRowDamage(o, r, enemy);
  approxEqual(perHit, 1000, 0.001, "真ダメージは防御/術耐性を無視する");
  check("真ダメージ: atFloorは常にfalse", atFloor === false);
}

console.log("\n=== 合計・撃破判定の境界値 ===\n");
{
  const catalog = { operators: [op(1000, 0, null)], buffers: [] };
  catalog.operators[0].id = "a";
  const rows = [
    row({ opId: "a", moduleId: null, dmgType: "true", hits: 5 }), // 1000*5=5000
    row({ opId: "a", moduleId: null, dmgType: "true", hits: 5 }), // 1000*5=5000
  ];
  const enemy = { ...enemyNeutral, hp: 10000 };
  const { total, killed } = computeTotal(catalog, rows, enemy);
  check("multi-row合計が期待通り(10000)", total === 10000, total);
  check("total === hp のとき killed=true(境界値)", killed === true);

  const enemyJustOver = { ...enemyNeutral, hp: 10001 };
  const { killed: killed2 } = computeTotal(catalog, rows, enemyJustOver);
  check("total < hp のとき killed=false", killed2 === false);
}

console.log("\n=== stale row の除去 ===\n");
{
  const catalog = { operators: [{ ...op(1000, 0, null), id: "real", fkEntries: [{}] }], buffers: [] };
  const state = {
    v: 1,
    enemy: enemyNeutral,
    rows: [
      row({ opId: "real", entryIdx: 0, moduleId: null }),
      row({ opId: "ghost", entryIdx: 0, moduleId: null }), // カタログに無いid
      row({ opId: "real", entryIdx: 5, moduleId: null }), // entryIdxが範囲外
    ],
  };
  const { state: cleaned, dropped } = dropStaleRows(state, catalog);
  check("stale rowが2件検出される", dropped === 2, dropped);
  check("残るのは1行だけ", cleaned.rows.length === 1, cleaned.rows.length);
}

console.log("\n=== 撃破提案(suggest) ===\n");
{
  // 撃破できていない状態を作り、提案の1件を適用したら実際に撃破できることを確認する。
  // 上限(バフ+300%/Hit+3)内で解決できる程度の不足にする（現実的なキャップ導入後の前提）。
  const catalog = { operators: [{ ...op(1000, 0, null), id: "a", fkEntries: [{}] }], buffers: [] };
  const rows = [row({ opId: "a", moduleId: null, dmgType: "true", hits: 1 })]; // final=1000
  const enemy = { ...enemyNeutral, hp: 1300 };
  const before = computeTotal(catalog, rows, enemy);
  check("前提: 現状は撃破できていない", before.killed === false, before.total);

  const suggestions = suggest({ v: 1, enemy, rows }, catalog);
  check("提案が1件以上ある", suggestions.length > 0, suggestions.length);
  check("提案は4件以内", suggestions.length <= 4, suggestions.length);

  for (const sug of suggestions) {
    const label = describeSuggestion(sug, catalog, rows);
    check(`提案に説明文がある: ${sug.kind}`, typeof label === "string" && label.length > 0, label);
  }

  // 先頭(最もeffortが小さい)提案を実際に適用してみて、本当に撃破できるかを再計算する。
  const top = suggestions[0];
  let testRows = rows,
    testEnemy = enemy;
  if (top.kind === "rowBuffPct") {
    testRows = rows.map((r2, i) => (i === top.rowIndex ? { ...r2, buffPct: r2.buffPct + top.amount / 100 } : r2));
  } else if (top.kind === "rowHits") {
    testRows = rows.map((r2, i) => (i === top.rowIndex ? { ...r2, hits: r2.hits + top.amount } : r2));
  } else if (top.kind === "enemyDefFlat") {
    testEnemy = { ...enemy, defFlat: enemy.defFlat + top.amount };
  } else if (top.kind === "enemyResFlat") {
    testEnemy = { ...enemy, resFlat: enemy.resFlat + top.amount };
  }
  const after = computeTotal(catalog, testRows, testEnemy);
  check(`提案(${top.kind} +${top.amount})を適用すると撃破できる`, after.killed === true, after.total);

  // effortの昇順に並んでいること。
  const efforts = suggestions.map((s) => s.effort);
  const sorted = [...efforts].sort((a, b) => a - b);
  check("提案はeffort昇順(安い順)", JSON.stringify(efforts) === JSON.stringify(sorted), efforts);
}
console.log("\n=== 提案の現実性キャップ（UIラウンド2: 「Hit数+29」「バフ+4806%」対策） ===\n");
{
  // Hit数キャップ(+3まで): ちょうど+3で足りるケースは提案に出る。
  const catalog = { operators: [{ ...op(1000, 0, null), id: "a", fkEntries: [{}] }], buffers: [] };
  const rows = [row({ opId: "a", moduleId: null, dmgType: "true", hits: 1 })]; // final=1000
  const enemyExact = { ...enemyNeutral, hp: 4000 }; // 1000×(1+3)=4000 ちょうど
  const sugExact = suggest({ v: 1, enemy: enemyExact, rows }, catalog);
  const hitsSugExact = sugExact.find((s) => s.kind === "rowHits");
  check("Hit数+3ちょうどで足りるなら提案に出る(上限=+3)", !!hitsSugExact && hitsSugExact.amount === 3, hitsSugExact);
}
{
  // バフ%キャップ(+300%まで): ちょうど+300%で足りるケースは提案に出る。
  const catalog = { operators: [{ ...op(1000, 0, null), id: "a", fkEntries: [{}] }], buffers: [] };
  const rows = [row({ opId: "a", moduleId: null, dmgType: "true", hits: 1 })]; // final=1000
  const enemyExact = { ...enemyNeutral, hp: 4000 }; // 1000×(1+300%)=4000 ちょうど
  const sugExact = suggest({ v: 1, enemy: enemyExact, rows }, catalog);
  const buffSugExact = sugExact.find((s) => s.kind === "rowBuffPct");
  check("バフ+300%ちょうどで足りるなら提案に出る(上限=+300%)", !!buffSugExact && buffSugExact.amount === 300, buffSugExact);
}
{
  // 上限を超えないと撃破できない場合、suggestは空配列になる
  // （ui.js側の「現実的な補正では届きません（あと N）」表示のトリガー条件）。
  // dmgType=trueの単独行はdef/res系の提案対象にもならないため、Hit/バフの上限超えだけで
  // 空配列になることを確認できる。
  const catalog = { operators: [{ ...op(1000, 0, null), id: "a", fkEntries: [{}] }], buffers: [] };
  const rows = [row({ opId: "a", moduleId: null, dmgType: "true", hits: 1 })]; // final=1000
  const enemy = { ...enemyNeutral, hp: 5000 }; // Hit+4(1000×5=5000)/バフ+400%が必要=どちらも上限超え
  const { total, killed } = computeTotal(catalog, rows, enemy);
  check("前提: 撃破できていない", killed === false, total);
  const suggestions = suggest({ v: 1, enemy, rows }, catalog);
  check("上限内に収まる提案が無い場合はsuggestが空配列", suggestions.length === 0, suggestions);
  // ui.js の「現実的な補正では届きません（あと N）」のNはhp-totalで計算する。そのデータ自体を検証する。
  const deficit = enemy.hp - total;
  check("空配列のときのdeficit(hp-total)データが期待通り", deficit === 4000, deficit);
}
{
  // 防御/術耐性-固定は「敵の現在値を超えて下げる提案をしない」上限を持つ。
  const catalog = { operators: [{ ...op(2000, 0, null), id: "a", fkEntries: [{}] }], buffers: [] };
  const rows = [row({ opId: "a", moduleId: null, dmgType: "physical", hits: 1 })];
  // 防御50をちょうど50下げ(=0)まで許容すれば2000ダメージ、hp=2000ちょうどに届く。
  const enemyExact = { ...enemyNeutral, hp: 2000, def: 50 };
  const sugExact = suggest({ v: 1, enemy: enemyExact, rows }, catalog);
  const defSugExact = sugExact.find((s) => s.kind === "enemyDefFlat");
  check(
    "防御を現在値(50)まで下げればちょうど届く場合、提案の量は現在値以内",
    !!defSugExact && defSugExact.amount <= 50,
    defSugExact,
  );

  // 防御を全部無効化(現在値=50が上限)しても届かないほどの不足では、enemyDefFlat提案は出ない。
  const enemyImpossible = { ...enemyNeutral, hp: 100000, def: 50 };
  const sugImpossible = suggest({ v: 1, enemy: enemyImpossible, rows }, catalog);
  const defSugImpossible = sugImpossible.find((s) => s.kind === "enemyDefFlat");
  check("防御を現在値まで下げても届かない場合はenemyDefFlat提案が出ない", defSugImpossible === undefined, sugImpossible);
}

{
  // 既に撃破できているときは提案が空であること。
  const catalog = { operators: [{ ...op(100000, 0, null), id: "a", fkEntries: [{}] }], buffers: [] };
  const rows = [row({ opId: "a", moduleId: null, dmgType: "true", hits: 1 })];
  const enemy = { ...enemyNeutral, hp: 100 };
  const suggestions = suggest({ v: 1, enemy, rows }, catalog);
  check("撃破済みのときsuggestは空配列", Array.isArray(suggestions) && suggestions.length === 0, suggestions);
}

console.log("\n=== P2: 個別バフ/条件付きバフ/特殊強化 ===\n");
{
  // Ash S3 400%: buffIds([plasma, nasty_s3]=0.9+0.6=1.5)経由でも、手入力buffPctを
  // 使った既存ケースと同じ最終値(6910)に届くこと。
  const catalog = {
    operators: [{ ...op(624, 27, 40), id: "ash", fkEntries: [{ tags: [] }] }],
    buffers: [
      { id: "plasma", name: "血漿", kind: "pct", value: 0.9, scope: { type: "individual" }, singleTarget: false, bonus: null },
      { id: "nasty_s3", name: "ナスティS3", kind: "pct", value: 0.6, scope: { type: "individual" }, singleTarget: false, bonus: null },
    ],
  };
  const r = row({ opId: "ash", entryIdx: 0, multiplier: 4, buffIds: ["plasma", "nasty_s3"] });
  const { results } = computeTotal(catalog, [r], enemyNeutral, []);
  approxEqual(results[0].final, 6910, 1, "Ash S3 400%(buffIds経由)");
}
{
  // FW S2: buffIds([plasma, stainless_2]=0.9+0.96=1.86)経由でも17249に届くこと。
  const catalog = {
    operators: [{ ...op(1175, 35, 87), id: "fw", fkEntries: [{ tags: [] }] }],
    buffers: [
      { id: "plasma", name: "血漿", kind: "pct", value: 0.9, scope: { type: "individual" }, singleTarget: false, bonus: null },
      { id: "stainless_2", name: "ステインレス(2)", kind: "pct", value: 0.96, scope: { type: "individual" }, singleTarget: false, bonus: null },
    ],
  };
  const r = row({ opId: "fw", entryIdx: 0, multiplier: 4.65, buffIds: ["plasma", "stainless_2"] });
  const { results } = computeTotal(catalog, [r], enemyNeutral, []);
  approxEqual(results[0].final, 17249, 1, "FW S2(buffIds経由)");
}
{
  // Horn(X) S2 物理: atk=1136(=1006+30+100)、selfPct=0.31(overrides.yamlのセルフ加算)、
  // multiplier=2.4。異格エクシア(条件付き。弾薬スキル+13%)をON。
  // final = 1136×(1+0.31+0.13)×2.4 = 3926.016 ≈ 3926。
  // 弾薬スキルタグを持たないエントリには適用されない(conditionalPct=0)ことも確認する。
  const exusiaiAlter = {
    id: "exusiai_alter",
    name: "異格エクシア",
    kind: "pct",
    value: 0.13,
    scope: { type: "conditional", targetTags: ["弾薬スキル"] },
    singleTarget: false,
    bonus: { targetTags: ["ラテラーノ"], value: 0.26, note: null },
  };
  const catalogAmmo = {
    operators: [{ ...op(1006, 30, 100), id: "horn", fkEntries: [{ tags: ["弾薬スキル"] }] }],
    buffers: [exusiaiAlter],
  };
  const rAmmo = row({ opId: "horn", entryIdx: 0, dmgType: "physical", multiplier: 2.4, selfPct: 0.31, moduleId: "m" });
  const { results: resultsAmmo } = computeTotal(catalogAmmo, [rAmmo], enemyNeutral, ["exusiai_alter"]);
  approxEqual(resultsAmmo[0].final, 3926, 1, "Horn(X) S2 物理(異格エクシアの弾薬スキルバフ込み)");

  const catalogNonAmmo = {
    operators: [{ ...op(1006, 30, 100), id: "horn_nonammo", fkEntries: [{ tags: [] }] }],
    buffers: [exusiaiAlter],
  };
  const rNonAmmo = row({ opId: "horn_nonammo", entryIdx: 0, dmgType: "physical", multiplier: 2.4, selfPct: 0.31, moduleId: "m" });
  const { results: resultsNonAmmo } = computeTotal(catalogNonAmmo, [rNonAmmo], enemyNeutral, ["exusiai_alter"]);
  check(
    "異格エクシアは弾薬スキルタグの無いエントリには適用されない",
    resultsNonAmmo[0].breakdown.conditionalPct === 0,
    resultsNonAmmo[0].breakdown.conditionalPct,
  );

  // ラテラーノ勢は効果2倍(0.26)。弾薬スキル無しでラテラーノだけの場合は0(基本targetsが
  // 一致しないと適用されない=bonusだけでは発動しない)。
  const mkCatalog = (tags) => ({
    operators: [{ ...op(1000, 0, null), id: "x", fkEntries: [{ tags }] }],
    buffers: [exusiaiAlter],
  });
  const pctFor = (tags) => {
    const r2 = row({ opId: "x", entryIdx: 0, moduleId: null });
    return computeTotal(mkCatalog(tags), [r2], enemyNeutral, ["exusiai_alter"]).results[0].breakdown.conditionalPct;
  };
  check("弾薬スキルのみ: 異格エクシアは13%", Math.abs(pctFor(["弾薬スキル"]) - 0.13) < 1e-9, pctFor(["弾薬スキル"]));
  check("弾薬スキル+ラテラーノ: 異格エクシアは26%(2倍)", Math.abs(pctFor(["弾薬スキル", "ラテラーノ"]) - 0.26) < 1e-9, pctFor(["弾薬スキル", "ラテラーノ"]));
  check("ラテラーノのみ(弾薬スキル無し): 適用されない(0)", pctFor(["ラテラーノ"]) === 0, pctFor(["ラテラーノ"]));
}
{
  // Castle(条件付き、近距離+20%)ONは近距離タグを持つ行にだけ適用される。
  const castle = { id: "castle3", name: "Castle", kind: "pct", value: 0.2, scope: { type: "conditional", targetTags: ["近距離"] }, singleTarget: false, bonus: null };
  const catalog = {
    operators: [
      { ...op(1000, 0, null), id: "melee", fkEntries: [{ tags: ["近距離"] }] },
      { ...op(1000, 0, null), id: "ranged", fkEntries: [{ tags: ["狙撃"] }] },
    ],
    buffers: [castle],
  };
  const rows = [row({ opId: "melee", entryIdx: 0, moduleId: null }), row({ opId: "ranged", entryIdx: 0, moduleId: null })];
  const { results } = computeTotal(catalog, rows, enemyNeutral, ["castle3"]);
  check("CastleONは近距離タグを持つ行に適用される", Math.abs(results[0].breakdown.conditionalPct - 0.2) < 1e-9, results[0].breakdown.conditionalPct);
  check("CastleONは近距離タグを持たない行には適用されない", results[1].breakdown.conditionalPct === 0, results[1].breakdown.conditionalPct);
}
{
  // flat種バフ(個別/条件付きどちらも)はinspireFlat枠に合算される。
  const flatBuffer = { id: "flatbuff", name: "テスト鼓舞", kind: "flat", value: 100, scope: { type: "individual" }, singleTarget: false, bonus: null };
  const catalog = { operators: [{ ...op(1000, 0, null), id: "y", fkEntries: [{ tags: [] }] }], buffers: [flatBuffer] };
  const r = row({ opId: "y", entryIdx: 0, moduleId: null, buffIds: ["flatbuff"] });
  const { results } = computeTotal(catalog, [r], enemyNeutral, []);
  check("flat種バフはinspireFlat(final=(atk×(1+pct)+flat)×multiplier)として加算される", results[0].final === 1100, results[0].final);
}
{
  // single_targetバフを2行で選ぶと検出される(UIの⚠警告用データ)。
  const solo = { id: "solo", name: "ソロバフ", kind: "pct", value: 0.1, scope: { type: "individual" }, singleTarget: true, bonus: null };
  const catalog = { operators: [{ ...op(1000, 0, null), id: "p", fkEntries: [{ tags: [] }] }], buffers: [solo] };
  const conflictRows = [row({ opId: "p", entryIdx: 0, moduleId: null, buffIds: ["solo"] }), row({ opId: "p", entryIdx: 0, moduleId: null, buffIds: ["solo"] })];
  check("single_targetバフが2行で選ばれると検出される", findSingleTargetConflicts(catalog, conflictRows).has("solo"));
  const okRows = [row({ opId: "p", entryIdx: 0, moduleId: null, buffIds: ["solo"] }), row({ opId: "p", entryIdx: 0, moduleId: null, buffIds: [] })];
  check("1行だけならsingle_target警告は出ない", !findSingleTargetConflicts(catalog, okRows).has("solo"));
}
{
  // 撃破提案: 個別バフを1件追加するだけで撃破できる場合、その提案が出て実際に撃破できる。
  const buf = { id: "boost", name: "テストバフ", kind: "pct", value: 0.5, scope: { type: "individual" }, singleTarget: false, bonus: null };
  const catalog = { operators: [{ ...op(1000, 0, null), id: "z", fkEntries: [{ tags: [] }] }], buffers: [buf] };
  const rows = [row({ opId: "z", entryIdx: 0, moduleId: null, dmgType: "true", hits: 1 })]; // final=1000
  const enemy = { ...enemyNeutral, hp: 1400 }; // +50%(=buf)なら1500で撃破、+0%では届かない
  const suggestions = suggest({ v: 1, enemy, rows, globalBuffIds: [] }, catalog);
  const buffSug = suggestions.find((s) => s.kind === "addIndividualBuff" && s.buffId === "boost");
  check("個別バフ追加の提案が出る", !!buffSug, buffSug);
  if (buffSug) {
    const label = describeSuggestion(buffSug, catalog, rows);
    check("提案に説明文がある(addIndividualBuff)", typeof label === "string" && label.length > 0, label);
    const testRows = rows.map((r2, j) => (j === buffSug.rowIndex ? { ...r2, buffIds: [...(r2.buffIds || []), buf.id] } : r2));
    const after = computeTotal(catalog, testRows, enemy, []);
    check("提案の個別バフを適用すると撃破できる", after.killed === true, after.total);
  }
}
{
  // 撃破提案: 条件付きバフを1件ONにするだけで撃破できる場合も同様。
  const buf = { id: "gboost", name: "テスト条件付き", kind: "pct", value: 0.5, scope: { type: "conditional", targetTags: ["近距離"] }, singleTarget: false, bonus: null };
  const catalog = { operators: [{ ...op(1000, 0, null), id: "w", fkEntries: [{ tags: ["近距離"] }] }], buffers: [buf] };
  const rows = [row({ opId: "w", entryIdx: 0, moduleId: null, dmgType: "true", hits: 1 })];
  const enemy = { ...enemyNeutral, hp: 1400 };
  const suggestions = suggest({ v: 1, enemy, rows, globalBuffIds: [] }, catalog);
  const gSug = suggestions.find((s) => s.kind === "toggleGlobalBuff" && s.buffId === "gboost");
  check("条件付きバフONの提案が出る", !!gSug, gSug);
  if (gSug) {
    const label = describeSuggestion(gSug, catalog, rows);
    check("提案に説明文がある(toggleGlobalBuff)", typeof label === "string" && label.length > 0, label);
    const after = computeTotal(catalog, rows, enemy, [gSug.buffId]);
    check("提案の条件付きバフをONにすると撃破できる", after.killed === true, after.total);
  }
}
{
  // 旧(P1)形のstate(buffIds/specialOn/globalBuffIds無し)もdropStaleRows経由で
  // 補われて計算できること(後方互換)。
  const catalog = { operators: [{ ...op(1000, 0, null), id: "old", fkEntries: [{ tags: [] }] }], buffers: [] };
  const oldRow = {
    opId: "old", entryIdx: 0, dmgType: "true", potential: true, moduleId: null, moduleLv: 3,
    multiplier: 1, selfPct: 0, hits: 1, buffPct: 0, dmgMult: 1, ignoreDef: 0,
  }; // buffIds/specialOnフィールドが無い旧形
  const oldState = { v: 1, enemy: { ...enemyNeutral, hp: 500 }, rows: [oldRow] }; // globalBuffIdsも無い
  const { state: cleaned } = dropStaleRows(oldState, catalog);
  check(
    "旧形のstateもdropStaleRows後にbuffIds/specialOn/globalBuffIdsが補われる",
    Array.isArray(cleaned.rows[0].buffIds) && cleaned.rows[0].specialOn === true && Array.isArray(cleaned.globalBuffIds),
    cleaned,
  );
  const { total, killed } = computeTotal(catalog, cleaned.rows, cleaned.enemy, cleaned.globalBuffIds);
  check("旧形のstateでも計算できる(撃破)", killed === true, total);
}

console.log("\n=== P2 follow-up: 特殊強化(モジュール依存の加算系/乗算系) ===\n");
{
  // ファイヤーウォッチS2「遠距離特効」: 乗算系(mul_multiplier)。
  // モジュールY(実データではuniequip_002_milu。ここではダミーid"m")装備時は
  // Lv1=1.45/Lv2=1.5/Lv3=1.55、未装備はbase=1.45(素質「暗殺者」E2最大潜在)。
  const fwSpecial = {
    label: "遠距離特効",
    description: "遠隔武器を持つ、または攻撃しない敵を攻撃した時、攻撃力が上昇する",
    requiresModule: null,
    addSelfAtkPctByModuleLevel: null,
    mulMultiplier: { base: 1.45, module: "m", byModuleLevel: [1.45, 1.5, 1.55] },
  };
  const catalog = {
    operators: [{ ...op(1175, 35, 87), id: "fw", fkEntries: [{ tags: [], special: fwSpecial }] }],
    buffers: [
      { id: "plasma", name: "血漿", kind: "pct", value: 0.9, scope: { type: "individual" }, singleTarget: false, bonus: null },
      { id: "stainless_2", name: "ステインレス(2)", kind: "pct", value: 0.96, scope: { type: "individual" }, singleTarget: false, bonus: null },
    ],
  };
  const rLv3On = row({ opId: "fw", entryIdx: 0, multiplier: 3.0, buffIds: ["plasma", "stainless_2"], moduleId: "m", moduleLv: 3, specialOn: true });
  const lv3On = computeTotal(catalog, [rLv3On], enemyNeutral, []).results[0];
  check("FW S2 モジュールYのLv3+特殊強化ONでmul係数1.55", lv3On.specialMulFactor === 1.55, lv3On.specialMulFactor);
  approxEqual(lv3On.final, 17249, 1, "FW S2 モジュールYのLv3+特殊強化ON (3.0×1.55=4.65)");

  const off = computeTotal(catalog, [{ ...rLv3On, specialOn: false, buffIds: [] }], enemyNeutral, []).results[0];
  check("FW S2 特殊強化OFFはmul係数1(=素のmultiplier 3.0のまま)", off.specialMulFactor === 1, off.specialMulFactor);
  approxEqual(off.final, 1297 * 3.0, 1, "FW S2 特殊強化OFF final(atk1297×multiplier3.0)");

  const noModule = computeTotal(catalog, [{ ...rLv3On, moduleId: null, buffIds: [] }], enemyNeutral, []).results[0];
  check("FW S2 モジュール未装備はbase(1.45)を使う", noModule.specialMulFactor === 1.45, noModule.specialMulFactor);

  const lv1 = computeTotal(catalog, [{ ...rLv3On, moduleLv: 1, buffIds: [] }], enemyNeutral, []).results[0];
  check("FW S2 モジュールYのLv1はbaseと同値1.45(Lv1では素質強化が付かない)", lv1.specialMulFactor === 1.45, lv1.specialMulFactor);

  const lv2 = computeTotal(catalog, [{ ...rLv3On, moduleLv: 2, buffIds: [] }], enemyNeutral, []).results[0];
  check("FW S2 モジュールYのLv2は1.5", lv2.specialMulFactor === 1.5, lv2.specialMulFactor);
}
{
  // ウィーディS3「蓄水砲配置バフ」: 加算系(requires_module)。実データでは
  // uniequip_002_weedy、Lv1=0/Lv2=0.15/Lv3=0.20(ここではダミーid"m")。
  const weedySpecial = {
    label: "蓄水砲配置バフ",
    description: "蓄水砲(召喚物)が周囲4マス以内にある時、攻撃力が追加upする",
    requiresModule: "m",
    addSelfAtkPctByModuleLevel: [0, 0.15, 0.2],
    mulMultiplier: null,
  };
  const catalog = { operators: [{ ...op(1000, 0, 50), id: "weedy", fkEntries: [{ tags: [], special: weedySpecial }] }], buffers: [] };
  const mk = (moduleId, moduleLv, specialOn) => row({ opId: "weedy", entryIdx: 0, moduleId, moduleLv, specialOn });

  const lv3 = computeTotal(catalog, [mk("m", 3, true)], enemyNeutral, []).results[0];
  check("ウィーディ モジュールX Lv3+ONで+20%", Math.abs(lv3.specialAddPct - 0.2) < 1e-9, lv3.specialAddPct);

  const lv2 = computeTotal(catalog, [mk("m", 2, true)], enemyNeutral, []).results[0];
  check("ウィーディ モジュールX Lv2+ONで+15%", Math.abs(lv2.specialAddPct - 0.15) < 1e-9, lv2.specialAddPct);

  const lv1 = computeTotal(catalog, [mk("m", 1, true)], enemyNeutral, []).results[0];
  check("ウィーディ モジュールX Lv1+ONは+0%(素質強化が付かない)", lv1.specialAddPct === 0, lv1.specialAddPct);

  const noModule = computeTotal(catalog, [mk(null, 3, true)], enemyNeutral, []).results[0];
  check("ウィーディ モジュールX未装備では+0%(適用されない)", noModule.specialAddPct === 0, noModule.specialAddPct);

  const off = computeTotal(catalog, [mk("m", 3, false)], enemyNeutral, []).results[0];
  check("ウィーディ 特殊強化OFFでは+0%", off.specialAddPct === 0, off.specialAddPct);
}
{
  // ブレイズS3「待機ボーナス」: 加算系。実データではuniequip_002_huang、
  // Lv1=0/Lv2=0.04/Lv3=0.06。self_atk_pct基礎値0.712に加算され、Lv3+ONで0.772
  // (939×1.772×4=6656、参照シート実測値と一致)。
  const blazeSpecial = {
    label: "待機ボーナス",
    description: "モジュールXを装備し配置から30秒経過すると、攻撃力が追加upする",
    requiresModule: "m",
    addSelfAtkPctByModuleLevel: [0, 0.04, 0.06],
    mulMultiplier: null,
  };
  const catalog = { operators: [{ ...op(825, 28, 86), id: "blaze", fkEntries: [{ tags: [], special: blazeSpecial }] }], buffers: [] };
  const mk = (moduleLv) => row({ opId: "blaze", entryIdx: 0, multiplier: 4, selfPct: 0.712, moduleId: "m", moduleLv, specialOn: true });

  const lv1 = computeTotal(catalog, [mk(1)], enemyNeutral, []).results[0];
  check("ブレイズ モジュールX Lv1+ONは+0%(素質強化が付かない)", lv1.specialAddPct === 0, lv1.specialAddPct);

  const lv2 = computeTotal(catalog, [mk(2)], enemyNeutral, []).results[0];
  check("ブレイズ モジュールX Lv2+ONは+4%", Math.abs(lv2.specialAddPct - 0.04) < 1e-9, lv2.specialAddPct);

  const lv3 = computeTotal(catalog, [mk(3)], enemyNeutral, []).results[0];
  check("ブレイズ モジュールX Lv3+ONは+6%", Math.abs(lv3.specialAddPct - 0.06) < 1e-9, lv3.specialAddPct);
  approxEqual(lv3.final, 6656, 1, "ブレイズ S3 モジュールX Lv3+ON (self=0.712+0.06=0.772)");
}

console.log("\n=== P3: 鼓舞(インスパイア)ソース(濁心スカジ) ===\n");
// 実データ(character_table.json talents + battle_equip_table.json)の値をそのまま使う。
// 出典・導出の詳細はdata/fk_kill_calc/buffers.yaml末尾のコメント参照。
function skadi2Source(overrides) {
  return {
    id: "skadi2",
    operatorId: "char_1012_skadi2",
    name: "濁心スカジ",
    tags: ["補助"], // profession=SUPPORT, position=RANGED (近距離タグは無い)
    atkBase: 418,
    atkPotential: 27,
    modules: [
      { id: "uniequip_002_skadi2", typeName: "X", name: "蜕化的残迹", atkByLevel: [26, 32, 35] },
      { id: "uniequip_003_skadi2", typeName: "Y", name: "新生代", atkByLevel: [22, 27, 30] },
    ],
    skills: [
      { skillNum: "2", ratio: 0.6 },
      { skillNum: "3", ratio: 1.1 },
    ],
    talentPotentialLabel: "素質凸",
    selfParts: [
      {
        id: "talent",
        label: "素質「捕食本能」(範囲内に他オペがいる)",
        shortLabel: "素質",
        description: null,
        pct: 0.06,
        pctPotentialBonus: 0.03,
        moduleOverride: {
          module: "uniequip_003_skadi2",
          pctByLevel: [0.06, 0.08, 0.09],
          potentialBonusByLevel: [0.03, 0.03, 0.03],
        },
        requiresModule: null,
        pctByModuleLevel: null,
        replaces: null,
        alwaysOn: true,
        defaultOn: false,
      },
      {
        id: "talent_abyssal",
        label: "攻撃範囲内に【アビサルハンター】がいる",
        shortLabel: "素質",
        description: null,
        pct: 0.15,
        pctPotentialBonus: 0.03,
        moduleOverride: {
          module: "uniequip_003_skadi2",
          pctByLevel: [0.15, 0.15, 0.2],
          potentialBonusByLevel: [0.03, 0.03, 0.03],
        },
        requiresModule: null,
        pctByModuleLevel: null,
        replaces: "talent",
        alwaysOn: false,
        defaultOn: false,
      },
      {
        id: "module_x_two_ops",
        label: "攻撃範囲内に他オペ2名以上(モジュールX)",
        shortLabel: "X",
        description: null,
        pct: 0,
        pctPotentialBonus: 0,
        moduleOverride: null,
        requiresModule: "uniequip_002_skadi2",
        pctByModuleLevel: [0.08, 0.08, 0.08],
        replaces: null,
        alwaysOn: false,
        defaultOn: true,
      },
    ],
    ...overrides,
  };
}
function skadi2Cfg(overrides) {
  return {
    on: true,
    skillNum: "2",
    potential: true,
    talentPotential: false,
    moduleId: null,
    moduleLv: 3,
    buffPct: 0,
    buffIds: [],
    parts: {},
    ...overrides,
  };
}
const podencoBuffer = { id: "podenco", name: "ポデンコ", kind: "pct", value: 0.11, scope: { type: "conditional", targetTags: ["補助"] }, singleTarget: false, bonus: null };
const plasmaBuffer = { id: "plasma", name: "血漿", kind: "pct", value: 0.9, scope: { type: "individual" }, singleTarget: false, bonus: null };
const exusiaiBuffer = { id: "exusiai", name: "エクシア", kind: "pct", value: 0.1, scope: { type: "individual" }, singleTarget: true, bonus: null };

{
  // 参照シート実測値: S2(Xの2名条件OFF, 素質凸ON) → 480×1.09×0.6=313.9≈314
  const source = skadi2Source();
  const cfg = skadi2Cfg({ skillNum: "2", talentPotential: true, moduleId: "uniequip_002_skadi2", moduleLv: 3, parts: { module_x_two_ops: false } });
  const result = computeInspireSource(source, cfg, { buffers: [] }, []);
  approxEqual(result.amount, 314, 1, "濁心スカジ S2 鼓舞(Xの2名条件OFF)");
}
{
  // 参照シート実測値: S3(Xの2名条件ON, 素質凸ON) → 480×1.17×1.1=617.8≈618
  const source = skadi2Source();
  const cfg = skadi2Cfg({ skillNum: "3", talentPotential: true, moduleId: "uniequip_002_skadi2", moduleLv: 3 }); // module_x_two_opsはdefaultOn=trueのまま
  const result = computeInspireSource(source, cfg, { buffers: [] }, []);
  approxEqual(result.amount, 618, 1, "濁心スカジ S3 鼓舞(Xの2名条件ON)");
}
{
  // ポデンコ(条件付き。対象=補助)ONで鼓舞量が増える(スカジ自身のtagsが補助のため適用される)。
  const source = skadi2Source();
  const cfg = skadi2Cfg({ skillNum: "2", moduleId: null });
  const catalog = { buffers: [podencoBuffer] };
  const without = computeInspireSource(source, cfg, catalog, []);
  const withPodenco = computeInspireSource(source, cfg, catalog, ["podenco"]);
  check("ポデンコONで鼓舞量が増える", withPodenco.amount > without.amount, `${without.amount} -> ${withPodenco.amount}`);
  approxEqual(withPodenco.selfPct - without.selfPct, 0.11, 1e-9, "ポデンコ分の差分は+11%");
}
{
  // 個別バフ(血漿+90%)をソースのbuffIdsに追加すると鼓舞量が増える(スペック変更で追加)。
  // 480×(1+0.09+0.9)×0.6 = 573.12 (talentPotential ON, Xの2名条件OFF)。
  const source = skadi2Source();
  const cfg = skadi2Cfg({ skillNum: "2", talentPotential: true, moduleId: "uniequip_002_skadi2", moduleLv: 3, parts: { module_x_two_ops: false }, buffIds: ["plasma"] });
  const result = computeInspireSource(source, cfg, { buffers: [plasmaBuffer] }, []);
  approxEqual(result.amount, 573.12, 1, "濁心スカジに血漿(個別バフ)を追加すると鼓舞量が増える");
}
{
  // アビサルハンターONは「素質」の値を置き換える(加算ではない)。moduleId=null(Yなし)。
  const source = skadi2Source();
  const withoutAbyssal = computeInspireSelfParts(source, skadi2Cfg({ moduleId: null }));
  check("通常時はtalentパーツのみ適用される", withoutAbyssal.applied.length === 1 && withoutAbyssal.applied[0].id === "talent", withoutAbyssal.applied);
  check("通常時のpctは0.06", Math.abs(withoutAbyssal.pct - 0.06) < 1e-9, withoutAbyssal.pct);

  const withAbyssal = computeInspireSelfParts(source, skadi2Cfg({ moduleId: null, parts: { talent_abyssal: true } }));
  check("アビサルハンターON時はtalent_abyssalのみ適用される(talentは無効化)", withAbyssal.applied.length === 1 && withAbyssal.applied[0].id === "talent_abyssal", withAbyssal.applied);
  check("アビサルハンターON時のpctは0.15(0.06+0.15の加算ではない)", Math.abs(withAbyssal.pct - 0.15) < 1e-9, withAbyssal.pct);
}
{
  // モジュールY(新生代)装備時のLvごとの素質値(素質凸なし/ありの両方)。
  const source = skadi2Source();
  const cases = [
    [1, false, 0.06], [1, true, 0.09],
    [2, false, 0.08], [2, true, 0.11],
    [3, false, 0.09], [3, true, 0.12],
  ];
  for (const [lv, talentPotential, expected] of cases) {
    const { pct } = computeInspireSelfParts(source, skadi2Cfg({ moduleId: "uniequip_003_skadi2", moduleLv: lv, talentPotential }));
    check(`モジュールY Lv${lv} 素質凸${talentPotential ? "ON" : "OFF"}: talent=${expected}`, Math.abs(pct - expected) < 1e-9, pct);
  }
  // アビサルハンター込みのモジュールYレベル別の値も確認する。
  const abyssalCases = [
    [1, false, 0.15], [1, true, 0.18],
    [2, false, 0.15], [2, true, 0.18],
    [3, false, 0.2], [3, true, 0.23],
  ];
  for (const [lv, talentPotential, expected] of abyssalCases) {
    const { pct } = computeInspireSelfParts(source, skadi2Cfg({ moduleId: "uniequip_003_skadi2", moduleLv: lv, talentPotential, parts: { talent_abyssal: true } }));
    check(`モジュールY Lv${lv}+アビサルハンター 素質凸${talentPotential ? "ON" : "OFF"}: talent_abyssal=${expected}`, Math.abs(pct - expected) < 1e-9, pct);
  }
}
{
  // モジュールXの「2名以上」パーツはXを装備している間だけ有効(実データ確認済み: Lv1〜3で同値+8%)。
  const source = skadi2Source();
  for (const lv of [1, 2, 3]) {
    const { pct } = computeInspireSelfParts(source, skadi2Cfg({ moduleId: "uniequip_002_skadi2", moduleLv: lv }));
    // talent(0.06、素質凸OFF) + module_x_two_ops(0.08) = 0.14
    check(`モジュールX Lv${lv}: talent(0.06)+X(0.08)=0.14`, Math.abs(pct - 0.14) < 1e-9, pct);
  }
  const { pct: withoutX } = computeInspireSelfParts(source, skadi2Cfg({ moduleId: null }));
  check("モジュールX未装備ではXパーツは適用されない(talentのみ0.06)", Math.abs(withoutX - 0.06) < 1e-9, withoutX);
  const { pct: yInstead } = computeInspireSelfParts(source, skadi2Cfg({ moduleId: "uniequip_003_skadi2", moduleLv: 3 }));
  check("モジュールY装備時もXパーツは適用されない(talentのみ0.09)", Math.abs(yInstead - 0.09) < 1e-9, yInstead);
}
{
  // max-not-sum: 2つの鼓舞ソースがONでも、行が受け取るのは最大の1件だけ(合算しない)。
  const bigSource = { ...skadi2Source(), id: "big", operatorId: "char_big", atkBase: 1000, atkPotential: 0, skills: [{ skillNum: "2", ratio: 1 }], selfParts: [] };
  const smallSource = { ...skadi2Source(), id: "small", operatorId: "char_small", atkBase: 100, atkPotential: 0, skills: [{ skillNum: "2", ratio: 1 }], selfParts: [] };
  const catalog = { operators: [], buffers: [], inspireSources: [bigSource, smallSource] };
  const sourceStates = {
    big: skadi2Cfg({ skillNum: "2", moduleId: null }),
    small: skadi2Cfg({ skillNum: "2", moduleId: null }),
  };
  const testRow = { opId: "someone_else", entryIdx: 0, inspireOn: true };
  const applied = computeInspireForRow(catalog, testRow, sourceStates, []);
  check("2ソースON時は最大(big=1000)だけが適用される(合算しない)", !!applied && Math.abs(applied.amount - 1000) < 1e-9, applied);
  check("適用されたソースはbig", applied && applied.sourceId === "big", applied);
}
{
  // row.inspireOn=falseの行には鼓舞が適用されない。
  const source = { ...skadi2Source(), selfParts: [] };
  const catalog = { operators: [], buffers: [], inspireSources: [source] };
  const sourceStates = { skadi2: skadi2Cfg({ skillNum: "2", moduleId: null }) };
  const rowOff = { opId: "someone_else", entryIdx: 0, inspireOn: false };
  check("row.inspireOn=falseなら鼓舞は適用されない", computeInspireForRow(catalog, rowOff, sourceStates, []) === null);
  const rowOn = { opId: "someone_else", entryIdx: 0, inspireOn: true };
  check("row.inspireOn=trueなら鼓舞が適用される", computeInspireForRow(catalog, rowOn, sourceStates, []) !== null);
}
{
  // 鼓舞ソース自身の行にはそのソース自身の鼓舞は乗らない。
  const source = { ...skadi2Source(), selfParts: [] };
  const catalog = { operators: [], buffers: [], inspireSources: [source] };
  const sourceStates = { skadi2: skadi2Cfg({ skillNum: "2", moduleId: null }) };
  const selfRow = { opId: source.operatorId, entryIdx: 0, inspireOn: true };
  check("鼓舞ソース自身の行には自分の鼓舞が乗らない", computeInspireForRow(catalog, selfRow, sourceStates, []) === null);
}
{
  // ソース自身のATKは鼓舞(inspireFlat)の影響を受けない(computeInspireSourceはinspireFlatを
  // 一切足さない=呼び出し元がどんなsourceStatesを渡してもソースの計算結果は変わらない)。
  const source = skadi2Source();
  const cfg = skadi2Cfg({ skillNum: "2", moduleId: null });
  const withoutOtherSources = computeInspireSource(source, cfg, { buffers: [] }, []);
  const withOtherSourcesIgnored = computeInspireSource(source, cfg, { buffers: [] }, []); // 呼び出し方法自体がinspireFlatを持たない
  check("鼓舞ソース自身のATK計算はinspireFlatの影響を受けない(関数自体がその引数を取らない)", withoutOtherSources.amount === withOtherSourcesIgnored.amount);
}
{
  // computeTotalに鼓舞ソース込みで通した時、対象行にinspireFlatとして加算されること。
  const op1000 = { id: "target", name: "target", tags: [], atkBase: 1000, atkPotential: 0, modules: [], fkEntries: [{ tags: [] }] };
  const source = { ...skadi2Source(), selfParts: [] };
  const catalog = { operators: [op1000], buffers: [], inspireSources: [source] };
  const targetRow = { opId: "target", entryIdx: 0, dmgType: "true", potential: true, moduleId: null, moduleLv: 3, multiplier: 1, selfPct: 0, hits: 1, buffPct: 0, dmgMult: 1, ignoreDef: 0, buffIds: [], specialOn: true, inspireOn: true };
  const sourceStates = { skadi2: skadi2Cfg({ skillNum: "2", moduleId: null }) }; // atk=418+27=445, ratio0.6, self=0 -> amount=267
  const { results } = computeTotal(catalog, [targetRow], enemyNeutral, [], sourceStates);
  const inspireAmount = 445 * 0.6;
  approxEqual(results[0].final, 1000 + inspireAmount, 0.01, "computeTotal経由でも鼓舞がinspireFlatとして加算される");
  check("results[0].inspireAppliedにソースidが入る", results[0].inspireApplied && results[0].inspireApplied.sourceId === "skadi2", results[0].inspireApplied);
}
{
  // 旧(P2)形のstate(state.inspire無し)もcomputeTotal/suggestにそのまま通せる(後方互換)。
  const catalog = { operators: [{ id: "old2", name: "old2", tags: [], atkBase: 1000, atkPotential: 0, modules: [], fkEntries: [{ tags: [] }] }], buffers: [], inspireSources: [] };
  const oldRow = row({ opId: "old2", entryIdx: 0, dmgType: "true", moduleId: null, hits: 1 });
  const oldState = { v: 1, enemy: { ...enemyNeutral, hp: 500 }, rows: [oldRow], globalBuffIds: [] }; // state.inspire無し
  const { total, killed } = computeTotal(catalog, oldState.rows, oldState.enemy, oldState.globalBuffIds);
  check("state.inspire無しでもcomputeTotalが計算できる(撃破)", killed === true, total);
  const suggestions = suggest(oldState, catalog);
  check("state.inspire無しでもsuggestが空配列を返す(撃破済みのため)", Array.isArray(suggestions) && suggestions.length === 0, suggestions);
}
{
  // 撃破提案: 鼓舞ソースをONにするだけで撃破できる場合、その提案が出て実際に撃破できる。
  const op1000 = { id: "target2", name: "target2", tags: [], atkBase: 1000, atkPotential: 0, modules: [], fkEntries: [{ tags: [] }] };
  const source = { ...skadi2Source(), selfParts: [] };
  const catalog = { operators: [op1000], buffers: [], inspireSources: [source] };
  const targetRow = row({ opId: "target2", entryIdx: 0, dmgType: "true", moduleId: null, hits: 1 }); // final=1000
  const enemy = { ...enemyNeutral, hp: 1200 }; // 鼓舞(スカジ既定cfg、skillNum省略=S2、445×0.6=267)で1267届く
  const state = { v: 1, enemy, rows: [targetRow], globalBuffIds: [], inspire: { sources: {} } };
  const suggestions = suggest(state, catalog);
  const sug = suggestions.find((s) => s.kind === "toggleInspireSource" && s.sourceId === "skadi2");
  check("鼓舞ソースONの提案が出る", !!sug, sug);
  if (sug) {
    const label = describeSuggestion(sug, catalog, state.rows);
    check("提案に説明文がある(toggleInspireSource)", typeof label === "string" && label.length > 0, label);
    const testStates = { [sug.sourceId]: { ...defaultInspireSourceCfg(source), on: true } };
    const after = computeTotal(catalog, state.rows, enemy, [], testStates);
    check("提案の鼓舞ソースをONにすると撃破できる", after.killed === true, after.total);
  }
}
{
  // 撃破提案: 行の鼓舞トグルをONにするだけで撃破できる場合。
  const op1000 = { id: "target3", name: "target3", tags: [], atkBase: 1000, atkPotential: 0, modules: [], fkEntries: [{ tags: [] }] };
  const source = { ...skadi2Source(), selfParts: [] };
  const catalog = { operators: [op1000], buffers: [], inspireSources: [source] };
  const targetRow = row({ opId: "target3", entryIdx: 0, dmgType: "true", moduleId: null, hits: 1, inspireOn: false });
  const enemy = { ...enemyNeutral, hp: 1200 };
  const state = { v: 1, enemy, rows: [targetRow], globalBuffIds: [], inspire: { sources: { skadi2: { ...defaultInspireSourceCfg(source), on: true } } } };
  const suggestions = suggest(state, catalog);
  const sug = suggestions.find((s) => s.kind === "toggleRowInspire");
  check("行の鼓舞トグルONの提案が出る", !!sug, sug);
  if (sug) {
    const label = describeSuggestion(sug, catalog, state.rows);
    check("提案に説明文がある(toggleRowInspire)", typeof label === "string" && label.length > 0, label);
    const testRows = state.rows.map((r2, j) => (j === sug.rowIndex ? { ...r2, inspireOn: true } : r2));
    const after = computeTotal(catalog, testRows, enemy, state.globalBuffIds, state.inspire.sources);
    check("提案の行の鼓舞トグルをONにすると撃破できる", after.killed === true, after.total);
  }
}
{
  // 撃破提案: ONの鼓舞ソースへ個別バフを1件追加するだけで撃破できる場合。
  const op1000 = { id: "target4", name: "target4", tags: [], atkBase: 1000, atkPotential: 0, modules: [], fkEntries: [{ tags: [] }] };
  const source = { ...skadi2Source(), selfParts: [] };
  const catalog = { operators: [op1000], buffers: [plasmaBuffer], inspireSources: [source] };
  const targetRow = row({ opId: "target4", entryIdx: 0, dmgType: "true", moduleId: null, hits: 1 });
  const enemy = { ...enemyNeutral, hp: 1300 }; // 鼓舞267では届かないが+血漿90%(445×1.9×0.6=507)で届く
  const state = {
    v: 1, enemy, rows: [targetRow], globalBuffIds: [],
    inspire: { sources: { skadi2: { ...defaultInspireSourceCfg(source), on: true } } },
  };
  const suggestions = suggest(state, catalog);
  const sug = suggestions.find((s) => s.kind === "addSourceIndividualBuff" && s.sourceId === "skadi2");
  check("鼓舞ソースへの個別バフ追加提案が出る", !!sug, sug);
  if (sug) {
    const label = describeSuggestion(sug, catalog, state.rows);
    check("提案に説明文がある(addSourceIndividualBuff)", typeof label === "string" && label.length > 0, label);
    const testStates = { skadi2: { ...state.inspire.sources.skadi2, buffIds: [...state.inspire.sources.skadi2.buffIds, sug.buffId] } };
    const after = computeTotal(catalog, state.rows, enemy, [], testStates);
    check("提案の個別バフを鼓舞ソースへ追加すると撃破できる", after.killed === true, after.total);
  }
}
{
  // single_target(エクシア)の⚠は行と鼓舞ソースを跨いで検出される(スペック変更)。
  const source = { ...skadi2Source(), selfParts: [] };
  const catalog = { operators: [{ ...op(1000, 0, null), id: "p2", fkEntries: [{ tags: [] }] }], buffers: [exusiaiBuffer], inspireSources: [source] };
  const rows1 = [row({ opId: "p2", entryIdx: 0, moduleId: null, buffIds: ["exusiai"] })];
  const sourceStates = { skadi2: { ...defaultInspireSourceCfg(source), on: true, buffIds: ["exusiai"] } };
  const conflicts = findSingleTargetConflicts(catalog, rows1, sourceStates);
  check("エクシアを行とソース両方で選ぶと⚠が両方で検出される", conflicts.has("exusiai"), conflicts);

  const sourceStatesOff = { skadi2: { ...defaultInspireSourceCfg(source), on: false, buffIds: ["exusiai"] } };
  const conflictsSourceOff = findSingleTargetConflicts(catalog, rows1, sourceStatesOff);
  check("ソースがOFFの間はソース側の選択を数えない(⚠出ない)", !conflictsSourceOff.has("exusiai"), conflictsSourceOff);
}

console.log("\n=== バフ調整: 「全員」タグ・exclusiveGroup（前衛アーミヤ） ===\n");
{
  const amiyaNormal = { id: "amiya_guard_normal", name: "前衛アーミヤ(通常)", kind: "pct", value: 0.07, scope: { type: "conditional", targetTags: ["全員"] }, singleTarget: false, bonus: null, exclusiveGroup: "amiya_guard" };
  const amiyaSkill = { id: "amiya_guard_skill", name: "前衛アーミヤ(スキル中)", kind: "pct", value: 0.14, scope: { type: "conditional", targetTags: ["全員"] }, singleTarget: false, bonus: null, exclusiveGroup: "amiya_guard" };
  const catalog = { operators: [], buffers: [amiyaNormal, amiyaSkill], inspireSources: [] };
  const sniperEntry = { tags: ["狙撃"] };
  const noTagEntry = { tags: [] };
  const r = row({ buffIds: [] });

  const b1 = computeBuffBreakdown(r, sniperEntry, catalog, ["amiya_guard_normal"]);
  approxEqual(b1.conditionalPct, 0.07, 1e-9, "「全員」の条件付きバフは狙撃の行にも効く(+7%)");
  const b2 = computeBuffBreakdown(r, noTagEntry, catalog, ["amiya_guard_skill"]);
  approxEqual(b2.conditionalPct, 0.14, 1e-9, "「全員」の条件付きバフはタグの無い行にも効く(+14%)");
  const b3 = computeBuffBreakdown(r, sniperEntry, catalog, ["amiya_guard_normal", "amiya_guard_skill"]);
  approxEqual(b3.conditionalPct, 0.14, 1e-9, "同じexclusiveGroupが両方ONでも合算せず最大値(14%)だけ効く");
  check("exclusiveGroupで外れた方は適用一覧に出ない",
    b3.appliedConditional.length === 1 && b3.appliedConditional[0].id === "amiya_guard_skill", b3.appliedConditional);
}

console.log("\n=== P4: 条件付きバフの動的値解決(resolveConditionalValue) ===\n");
// 実データ(character_table.json talents + battle_equip_table.json。2026-09時点)の値をそのまま
// 使う。出典・導出の詳細はdata/fk_kill_calc/buffers.yamlのconditional各行のnote参照。
{
  // エイヤ(talent0.atk): E1(.07/潜在6で.09)/E2(.14/潜在6で.16)、
  // モジュールX Lv2(.18/潜在6で.20)/Lv3(.22/潜在6で.24)。両軸(elite/potential)が変化し、
  // モジュールも値を変えるので3軸とも見せる想定。
  const aya = {
    id: "aya",
    name: "エイヤ",
    kind: "pct",
    value: 0.24,
    scope: { type: "conditional", targetTags: ["術師"] },
    singleTarget: false,
    bonus: null,
    source: {
      operatorId: "char_180_amgoat",
      operatorName: "エイヤ",
      talent: {
        valuesByEliteAndPotential: [
          [0, 0, 0, 0, 0, 0],
          [0.07, 0.07, 0.07, 0.07, 0.07, 0.09],
          [0.14, 0.14, 0.14, 0.14, 0.14, 0.16],
        ],
        eliteVaries: true,
        potentialVaries: true,
        modules: [
          {
            moduleId: "uniequip_002_amgoat",
            typeName: "X",
            name: "エイヤ用X",
            valuesByLevelAndPotential: [
              [0.14, 0.14, 0.14, 0.14, 0.14, 0.16],
              [0.18, 0.18, 0.18, 0.18, 0.18, 0.2],
              [0.22, 0.22, 0.22, 0.22, 0.22, 0.24],
            ],
          },
        ],
      },
      skill: null,
      defaults: { elite: 2, potential: 5, moduleId: "uniequip_002_amgoat", moduleLevel: 3, skillLevel: 1 },
    },
  };
  check("エイヤ: デフォルト設定(E2/潜在6/モジュールXLv3)で+24%", Math.abs(resolveConditionalValue(aya, {}) - 0.24) < 1e-9, resolveConditionalValue(aya, {}));
  check("エイヤ: E1/潜在6/モジュール無しで+9%", Math.abs(resolveConditionalValue(aya, { elite: 1, potential: 5, moduleId: null }) - 0.09) < 1e-9);
  check("エイヤ: E2/潜在1(0-indexed)/モジュール無しで+14%", Math.abs(resolveConditionalValue(aya, { elite: 2, potential: 0, moduleId: null }) - 0.14) < 1e-9);
  check("エイヤ: モジュールXのLv2/潜在1で+18%", Math.abs(resolveConditionalValue(aya, { moduleId: "uniequip_002_amgoat", moduleLevel: 2, potential: 0 }) - 0.18) < 1e-9);
  check("エイヤ: 潜在の選択肢は潜在1-5/潜在6の2つ", JSON.stringify(potentialGroups(aya.source.talent)) === JSON.stringify([{ from: 0, to: 4 }, { from: 5, to: 5 }]), JSON.stringify(potentialGroups(aya.source.talent)));
  check("エイヤ: E1ではモジュールX Lv3を選んでいても無視される(+9%)", Math.abs(resolveConditionalValue(aya, { elite: 1, potential: 5, moduleId: "uniequip_002_amgoat", moduleLevel: 3 }) - 0.09) < 1e-9, resolveConditionalValue(aya, { elite: 1, potential: 5, moduleId: "uniequip_002_amgoat", moduleLevel: 3 }));
  check("エイヤ: モジュールXのLv1は素質未強化(E2/潜在6で+16%)", Math.abs(resolveConditionalValue(aya, { moduleId: "uniequip_002_amgoat", moduleLevel: 1, potential: 5 }) - 0.16) < 1e-9, resolveConditionalValue(aya, { moduleId: "uniequip_002_amgoat", moduleLevel: 1, potential: 5 }));
}
{
  // ポデンコ: モジュールXの上書きが値を変えない(dedupe)ので、Rust側はmodulesを空Vecで
  // 返す想定。JS側はmodulesが空ならmoduleId未選択のまま扱う(ベース値を使う)。
  const podenco = {
    id: "podenco",
    name: "ポデンコ",
    kind: "pct",
    value: 0.11,
    scope: { type: "conditional", targetTags: ["補助"] },
    singleTarget: false,
    bonus: null,
    source: {
      operatorId: "char_258_podego",
      operatorName: "ポデンコ",
      talent: {
        valuesByEliteAndPotential: [
          [0, 0, 0, 0, 0, 0],
          [0.05, 0.05, 0.05, 0.05, 0.07, 0.07],
          [0.09, 0.09, 0.09, 0.09, 0.11, 0.11],
        ],
        eliteVaries: true,
        potentialVaries: true,
        modules: [],
      },
      skill: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 1 },
    },
  };
  check("ポデンコ: 潜在の選択肢は潜在1-4/潜在5-6の2つ", JSON.stringify(potentialGroups(podenco.source.talent)) === JSON.stringify([{ from: 0, to: 3 }, { from: 4, to: 5 }]), JSON.stringify(potentialGroups(podenco.source.talent)));
  check("潜在グループ: 全潜在で値が違えば6つ(Castle型)", potentialGroups({ valuesByEliteAndPotential: [[0.1, 0.12, 0.14, 0.16, 0.18, 0.2]], modules: [] }).length === 6);
  check("ポデンコ: modules軸が空(dedupeで隠れる)", podenco.source.talent.modules.length === 0);
  check("ポデンコ: デフォルトで+11%", Math.abs(resolveConditionalValue(podenco, {}) - 0.11) < 1e-9, resolveConditionalValue(podenco, {}));
}
{
  // ズィマー: スキルLv別blackboard由来(素質ではない)。L1..L10 = .25,.30,.35,.35,.40,.45,.45,.50,.55,.60。
  const zima = {
    id: "zima",
    name: "ズィマー",
    kind: "pct",
    value: 0.6,
    scope: { type: "conditional", targetTags: ["先鋒"] },
    singleTarget: false,
    bonus: null,
    source: {
      operatorId: "char_115_headbr",
      operatorName: "ズィマー",
      talent: null,
      skill: { skillNum: "2", skillLabel: "乌萨斯战吼", valuesByLevel: [0.25, 0.3, 0.35, 0.35, 0.4, 0.45, 0.45, 0.5, 0.55, 0.6] },
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 10 },
    },
  };
  check("ズィマー: デフォルト(特化3=Lv10)で+60%", Math.abs(resolveConditionalValue(zima, {}) - 0.6) < 1e-9, resolveConditionalValue(zima, {}));
  check("ズィマー: Lv1で+25%", Math.abs(resolveConditionalValue(zima, { skillLevel: 1 }) - 0.25) < 1e-9);
  check("ズィマー: Lv8(特化1)で+50%", Math.abs(resolveConditionalValue(zima, { skillLevel: 8 }) - 0.5) < 1e-9);
}
{
  // スズラン: ベースにatkキーが無い(常に0)。モジュールXを装備して初めて効果が出る。
  const suzuran = {
    id: "suzuran",
    name: "スズラン",
    kind: "pct",
    value: 0.09,
    scope: { type: "conditional", targetTags: ["補助"] },
    singleTarget: false,
    bonus: null,
    source: {
      operatorId: "char_358_lisa",
      operatorName: "スズラン",
      talent: {
        valuesByEliteAndPotential: [[0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0]],
        eliteVaries: false,
        potentialVaries: false,
        modules: [
          { moduleId: "uniequip_002_lisa", typeName: "X", name: "スズラン用X", valuesByLevelAndPotential: [[0, 0, 0, 0, 0, 0], [0.06, 0.06, 0.06, 0.06, 0.06, 0.06], [0.09, 0.09, 0.09, 0.09, 0.09, 0.09]] },
        ],
      },
      skill: null,
      defaults: { elite: 2, potential: 5, moduleId: "uniequip_002_lisa", moduleLevel: 3, skillLevel: 1 },
    },
  };
  check("スズラン: デフォルト(モジュールXLv3)で+9%", Math.abs(resolveConditionalValue(suzuran, {}) - 0.09) < 1e-9, resolveConditionalValue(suzuran, {}));
  check("スズラン: モジュール未装備では0%(ヒント表示のトリガー)", resolveConditionalValue(suzuran, { moduleId: null }) === 0);
  check("スズラン: eliteVaries/potentialVariesはどちらもfalse", !suzuran.source.talent.eliteVaries && !suzuran.source.talent.potentialVaries);
}
{
  // 前衛アーミヤ: toggle(スキル中は効果2倍)。E1(.04)/E2(.07)、モジュールX Lv2(.08)/Lv3(.09)。
  const amiya = {
    id: "amiya_guard",
    name: "前衛アーミヤ",
    kind: "pct",
    value: 0.09,
    scope: { type: "conditional", targetTags: ["全員"] },
    singleTarget: false,
    bonus: null,
    toggle: { label: "スキル中(効果2倍)", mult: 2 },
    source: {
      operatorId: "char_1001_amiya2",
      operatorName: "前衛アーミヤ",
      talent: {
        valuesByEliteAndPotential: [[0, 0, 0, 0, 0, 0], [0.04, 0.04, 0.04, 0.04, 0.04, 0.04], [0.07, 0.07, 0.07, 0.07, 0.07, 0.07]],
        eliteVaries: true,
        potentialVaries: false,
        modules: [
          { moduleId: "uniequip_002_amiya2", typeName: "X", name: "アーミヤ用X", valuesByLevelAndPotential: [[0.07, 0.07, 0.07, 0.07, 0.07, 0.07], [0.08, 0.08, 0.08, 0.08, 0.08, 0.08], [0.09, 0.09, 0.09, 0.09, 0.09, 0.09]] },
        ],
      },
      skill: null,
      defaults: { elite: 2, potential: 5, moduleId: "uniequip_002_amiya2", moduleLevel: 3, skillLevel: 1 },
    },
  };
  check("前衛アーミヤ: デフォルト(トグルOFF)で+9%", Math.abs(resolveConditionalValue(amiya, {}) - 0.09) < 1e-9, resolveConditionalValue(amiya, {}));
  check("前衛アーミヤ: トグルONで2倍(+18%)", Math.abs(resolveConditionalValue(amiya, { toggleOn: true }) - 0.18) < 1e-9, resolveConditionalValue(amiya, { toggleOn: true }));
}
{
  // 異格エクシア: bonus.multで基本値の2倍(置き換え)。computeBuffBreakdown経由で確認する
  // (bonusの適用はresolveConditionalValueではなくcomputeBuffBreakdown側の責務のため)。
  const exusiaiAlterSourced = {
    id: "exusiai_alter",
    name: "異格エクシア",
    kind: "pct",
    value: 0.13,
    scope: { type: "conditional", targetTags: ["弾薬スキル"] },
    singleTarget: false,
    bonus: { targetTags: ["ラテラーノ"], value: null, mult: 2, note: null },
    source: {
      operatorId: "char_1041_angel2",
      operatorName: "異格エクシア",
      talent: {
        valuesByEliteAndPotential: [[0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0], [0.09, 0.09, 0.13, 0.13, 0.13, 0.13]],
        eliteVaries: true,
        potentialVaries: true,
        modules: [],
      },
      skill: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 1 },
    },
  };
  const catalog = { buffers: [exusiaiAlterSourced] };
  const b1 = computeBuffBreakdown(row({ buffIds: [] }), { tags: ["弾薬スキル"] }, catalog, ["exusiai_alter"]);
  approxEqual(b1.conditionalPct, 0.13, 1e-9, "異格エクシア(source化後): 弾薬スキルのみは基本値13%");
  const b2 = computeBuffBreakdown(row({ buffIds: [] }), { tags: ["弾薬スキル", "ラテラーノ"] }, catalog, ["exusiai_alter"]);
  approxEqual(b2.conditionalPct, 0.26, 1e-9, "異格エクシア(source化後): ラテラーノ勢はbonus.multで基本値の2倍(26%)");
}

console.log("\n=== P4: state移行(前衛アーミヤの旧2エントリ→新1エントリ+toggle、globalBuffLevelsのデフォルト補完) ===\n");
{
  const amiyaGuard = {
    id: "amiya_guard",
    name: "前衛アーミヤ",
    kind: "pct",
    value: 0.09,
    scope: { type: "conditional", targetTags: ["全員"] },
    singleTarget: false,
    bonus: null,
    toggle: { label: "スキル中(効果2倍)", mult: 2 },
    source: {
      operatorId: "char_1001_amiya2",
      operatorName: "前衛アーミヤ",
      talent: { valuesByEliteAndPotential: [[0,0,0,0,0,0],[0.04,0.04,0.04,0.04,0.04,0.04],[0.07,0.07,0.07,0.07,0.07,0.07]], eliteVaries: true, potentialVaries: false, modules: [] },
      skill: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 1 },
    },
  };
  const catalog = { operators: [], buffers: [amiyaGuard], inspireSources: [] };

  // 旧(P2)形: amiya_guard_skillがONだった → 新amiya_guard + toggleOn=trueに移行する。
  const oldSkillState = { v: 1, enemy: { hp: 0, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 }, rows: [], globalBuffIds: ["amiya_guard_skill"] };
  const { state: migratedSkill } = dropStaleRows(oldSkillState, catalog);
  check("amiya_guard_skillはamiya_guardへ移行される", migratedSkill.globalBuffIds.includes("amiya_guard") && !migratedSkill.globalBuffIds.includes("amiya_guard_skill"), migratedSkill.globalBuffIds);
  check("amiya_guard_skillだった場合、toggleOn=trueへ移行される", migratedSkill.globalBuffLevels.amiya_guard.toggleOn === true, migratedSkill.globalBuffLevels);

  // 旧(P2)形: amiya_guard_normalがONだった → 新amiya_guard(toggleOn=falseのまま)に移行する。
  const oldNormalState = { v: 1, enemy: { hp: 0, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 }, rows: [], globalBuffIds: ["amiya_guard_normal"] };
  const { state: migratedNormal } = dropStaleRows(oldNormalState, catalog);
  check("amiya_guard_normalはamiya_guardへ移行される", migratedNormal.globalBuffIds.includes("amiya_guard"), migratedNormal.globalBuffIds);
  check("amiya_guard_normalだった場合、toggleOnはfalseのまま", migratedNormal.globalBuffLevels.amiya_guard.toggleOn === false);

  // globalBuffLevelsが無い/欠損しているstateでも、source付きバフのデフォルトが補われる。
  const bareState = { v: 1, enemy: { hp: 0, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 }, rows: [], globalBuffIds: [] };
  const { state: cleanedBare } = dropStaleRows(bareState, catalog);
  check(
    "globalBuffLevels無しのstateにもamiya_guardのデフォルトが補われる",
    cleanedBare.globalBuffLevels.amiya_guard && cleanedBare.globalBuffLevels.amiya_guard.elite === 2 && cleanedBare.globalBuffLevels.amiya_guard.toggleOn === false,
    cleanedBare.globalBuffLevels,
  );
}

console.log("\n=== P5: 個別バフの動的値解決(talent×scale/base_pct×scale/stage/max_targets_by_module) ===\n");
// 実データ(character_table.json talents + skill_table.json blackboard_by_level。2026-09時点)の
// 値をそのまま使う。出典・導出の詳細はdata/fk_kill_calc/buffers.yamlのindividual各行のnote参照。
{
  // ワルファリンS2(skill_num=2のblackboard"atk"そのもの。旧固定.90→P5でスキルLv解決)。
  const plasma = {
    id: "plasma",
    name: "ワルファリンS2",
    kind: "pct",
    value: 0.9,
    scope: { type: "individual" },
    singleTarget: true,
    bonus: null,
    source: {
      operatorId: "char_171_bldsk",
      operatorName: "ワルファリン",
      talent: null,
      skill: { skillNum: "2", skillLabel: "不安定血漿", valuesByLevel: [0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.7, 0.8, 0.9], varies: true },
      scale: null,
      basePct: null,
      stage: null,
      maxTargetsByModule: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 10, stageIndex: 1 },
    },
  };
  check("ワルファリンS2: デフォルト(特化3=Lv10)で+90%", Math.abs(resolveConditionalValue(plasma, {}) - 0.9) < 1e-9, resolveConditionalValue(plasma, {}));
  check("ワルファリンS2: Lv1で+30%", Math.abs(resolveConditionalValue(plasma, { skillLevel: 1 }) - 0.3) < 1e-9);
  check("ワルファリンS2はsingleTarget=true", plasma.singleTarget === true);
}
{
  // アS3(skill_num=3のblackboard"atk"そのもの。旧固定.50→P5でスキルLv解決)。
  const durian = {
    id: "durian",
    name: "アS3",
    kind: "pct",
    value: 0.5,
    scope: { type: "individual" },
    singleTarget: true,
    bonus: null,
    source: {
      operatorId: "char_225_haak",
      operatorName: "ア",
      talent: null,
      skill: { skillNum: "3", skillLabel: "劇性増強剤・ドリアン", valuesByLevel: [0.25, 0.25, 0.25, 0.3, 0.3, 0.3, 0.35, 0.4, 0.45, 0.5], varies: true },
      scale: null,
      basePct: null,
      stage: null,
      maxTargetsByModule: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 10, stageIndex: 1 },
    },
  };
  check("アS3: デフォルト(特化3=Lv10)で+50%", Math.abs(resolveConditionalValue(durian, {}) - 0.5) < 1e-9, resolveConditionalValue(durian, {}));
  check("アS3: Lv1で+25%", Math.abs(resolveConditionalValue(durian, { skillLevel: 1 }) - 0.25) < 1e-9);
  check("アS3はsingleTarget=true", durian.singleTarget === true);
}
{
  // スプリアS2(旧固定.20→P5でスキルLv解決。single_targetは既存どおり維持)。
  const sprria = {
    id: "sprria_s2",
    name: "スプリアS2",
    kind: "pct",
    value: 0.3,
    scope: { type: "individual" },
    singleTarget: true,
    bonus: null,
    source: {
      operatorId: "char_4015_spuria",
      operatorName: "スプリア",
      talent: null,
      skill: { skillNum: "2", skillLabel: "インスタントカスタム", valuesByLevel: [0.1, 0.1, 0.1, 0.15, 0.15, 0.15, 0.2, 0.2, 0.25, 0.3], varies: true },
      scale: null,
      basePct: null,
      stage: null,
      maxTargetsByModule: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 10, stageIndex: 1 },
    },
  };
  check("スプリアS2: デフォルト(特化3=Lv10)で+30%(旧固定20%から変更)", Math.abs(resolveConditionalValue(sprria, {}) - 0.3) < 1e-9, resolveConditionalValue(sprria, {}));
}
{
  // スワイヤーS1: talent0.atk(E0.03/E1.06/E2.10、潜在1〜5と潜在6の2段)×スキル1の
  // talent_scale(全レベル2.0固定。varies=falseなのでスキル軸は非表示)。
  const swireTalent = {
    valuesByEliteAndPotential: [
      [0.03, 0.03, 0.03, 0.03, 0.03, 0.05],
      [0.06, 0.06, 0.06, 0.06, 0.06, 0.08],
      [0.1, 0.1, 0.1, 0.1, 0.1, 0.12],
    ],
    eliteVaries: true,
    potentialVaries: true,
    modules: [],
  };
  const swireS1 = {
    id: "swire_s1",
    name: "スワイヤーS1",
    kind: "pct",
    value: 0.24,
    scope: { type: "individual" },
    singleTarget: false,
    bonus: null,
    source: {
      operatorId: "char_308_swire",
      operatorName: "スワイヤー",
      talent: swireTalent,
      skill: null,
      scale: { skillNum: "1", skillLabel: "統括指揮", valuesByLevel: [2, 2, 2, 2, 2, 2, 2, 2, 2, 2], varies: false },
      basePct: null,
      stage: null,
      maxTargetsByModule: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 1, stageIndex: 1 },
    },
  };
  check("スワイヤーS1: デフォルト(E2/潜在6)で0.12×2=+24%", Math.abs(resolveConditionalValue(swireS1, {}) - 0.24) < 1e-9, resolveConditionalValue(swireS1, {}));
  check("スワイヤーS1: E2/潜在1(0-indexed)で0.10×2=+20%", Math.abs(resolveConditionalValue(swireS1, { elite: 2, potential: 0 }) - 0.2) < 1e-9, resolveConditionalValue(swireS1, { elite: 2, potential: 0 }));
  check("スワイヤーS1: scaleのvaries=falseなのでスキル軸は非表示想定", swireS1.source.scale.varies === false);

  // スワイヤーS2: 同じtalentにスキル2のtalent_scale(2.1〜3.0、varies=true)を掛ける。
  const swireS2 = {
    ...swireS1,
    id: "swire_s2",
    name: "スワイヤーS2",
    value: 0.36,
    source: {
      ...swireS1.source,
      scale: { skillNum: "2", skillLabel: "協同作戦", valuesByLevel: [2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9, 3.0], varies: true },
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 10, stageIndex: 1 },
    },
  };
  check("スワイヤーS2: デフォルト(E2/潜在6/Lv10)で0.12×3.0=+36%", Math.abs(resolveConditionalValue(swireS2, {}) - 0.36) < 1e-9, resolveConditionalValue(swireS2, {}));
  check("スワイヤーS2: Lv1で0.12×2.1=+25.2%", Math.abs(resolveConditionalValue(swireS2, { skillLevel: 1 }) - 0.252) < 1e-9, resolveConditionalValue(swireS2, { skillLevel: 1 }));
  check("スワイヤーS2: scaleのvaries=trueなのでスキル軸は表示想定", swireS2.source.scale.varies === true);
}
{
  // ステインレスS1: base_pct(0.12。トークンのため素質値ではない)×スキル1のfake_scale
  // (2〜4)。トグル「装置2台」でON時は解決値に×2。
  const stainless = {
    id: "stainless_s1",
    name: "ステインレスS1",
    kind: "pct",
    value: 0.48,
    scope: { type: "individual" },
    singleTarget: false,
    bonus: null,
    toggle: { label: "装置2台（×2）", mult: 2 },
    source: {
      operatorId: "char_4072_ironmn",
      operatorName: "ステインレス",
      talent: null,
      skill: null,
      scale: { skillNum: "1", skillLabel: "ハイパーブースト", valuesByLevel: [2, 2, 2, 2.5, 2.5, 2.5, 3, 3.5, 3.5, 4], varies: true },
      basePct: 0.12,
      stage: null,
      maxTargetsByModule: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 10, stageIndex: 1 },
    },
  };
  check("ステインレスS1: デフォルト(Lv10)で0.12×4=+48%", Math.abs(resolveConditionalValue(stainless, {}) - 0.48) < 1e-9, resolveConditionalValue(stainless, {}));
  check("ステインレスS1: SLv7(0-indexed6)で0.12×3=+36%", Math.abs(resolveConditionalValue(stainless, { skillLevel: 7 }) - 0.36) < 1e-9, resolveConditionalValue(stainless, { skillLevel: 7 }));
  check("ステインレスS1: トグルON(装置2台)で0.48×2=+96%", Math.abs(resolveConditionalValue(stainless, { toggleOn: true }) - 0.96) < 1e-9, resolveConditionalValue(stainless, { toggleOn: true }));
}
{
  // ナスティS3: スキルLvではなく段階(1〜3段階)で値が変わる(.20/.40/.60)。
  const nasty = {
    id: "nasty_s3",
    name: "ナスティS3",
    kind: "pct",
    value: 0.6,
    scope: { type: "individual" },
    singleTarget: false,
    bonus: null,
    source: {
      operatorId: "char_4212_nasti",
      operatorName: "ナスティ",
      talent: null,
      skill: null,
      scale: null,
      basePct: null,
      stage: { skillId: "sktok_nasti_nstbld", skillLabel: "止まり木", values: [0.2, 0.4, 0.6], labels: ["1段階", "2段階", "3段階"] },
      maxTargetsByModule: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 1, stageIndex: 3 },
    },
  };
  check("ナスティS3: デフォルト(3段階)で+60%", Math.abs(resolveConditionalValue(nasty, {}) - 0.6) < 1e-9, resolveConditionalValue(nasty, {}));
  check("ナスティS3: 1段階目で+20%", Math.abs(resolveConditionalValue(nasty, { stageIndex: 1 }) - 0.2) < 1e-9, resolveConditionalValue(nasty, { stageIndex: 1 }));
  check("ナスティS3: 2段階目で+40%", Math.abs(resolveConditionalValue(nasty, { stageIndex: 2 }) - 0.4) < 1e-9);
}
{
  // エクシア: 素質talent1.atk(E2のみ。潜在1=.06/潜在6=.08)。モジュールX Lv2は素質未強化と
  // 同値(.06/.08)、Lv3で.08/.10。モジュールX Lv2以上装備で対象2名(max_targets_by_module)。
  const exusiai = {
    id: "exusiai",
    name: "エクシア",
    kind: "pct",
    value: 0.1,
    scope: { type: "individual" },
    singleTarget: true,
    bonus: null,
    source: {
      operatorId: "char_103_angel",
      operatorName: "エクシア",
      talent: {
        valuesByEliteAndPotential: [
          [0, 0, 0, 0, 0, 0],
          [0, 0, 0, 0, 0, 0],
          [0.06, 0.06, 0.06, 0.06, 0.06, 0.08],
        ],
        eliteVaries: true,
        potentialVaries: true,
        modules: [
          {
            moduleId: "uniequip_002_angel",
            typeName: "X",
            name: "エクシアの傑作",
            valuesByLevelAndPotential: [
              [0.06, 0.06, 0.06, 0.06, 0.06, 0.08],
              [0.06, 0.06, 0.06, 0.06, 0.06, 0.08],
              [0.08, 0.08, 0.08, 0.08, 0.08, 0.1],
            ],
          },
        ],
      },
      skill: null,
      scale: null,
      basePct: null,
      stage: null,
      maxTargetsByModule: { moduleId: "uniequip_002_angel", minLevel: 2, count: 2 },
      defaults: { elite: 2, potential: 5, moduleId: "uniequip_002_angel", moduleLevel: 3, skillLevel: 1, stageIndex: 1 },
    },
  };
  check("エクシア: デフォルト(E2/潜在6/モジュールXLv3)で+10%", Math.abs(resolveConditionalValue(exusiai, {}) - 0.1) < 1e-9, resolveConditionalValue(exusiai, {}));
  check("エクシア: E0/E1は0(素質未解放)", resolveConditionalValue(exusiai, { elite: 0 }) === 0 && resolveConditionalValue(exusiai, { elite: 1 }) === 0);
  check("エクシア: モジュールXLv3/潜在1(0-indexed)で+8%", Math.abs(resolveConditionalValue(exusiai, { potential: 0 }) - 0.08) < 1e-9, resolveConditionalValue(exusiai, { potential: 0 }));

  // findSingleTargetConflicts: モジュールXLv2以上装備時は2行まで警告なし、3行目からは警告。
  const catalog = { buffers: [exusiai] };
  const levelsDefault = { exusiai: { elite: 2, potential: 5, moduleId: "uniequip_002_angel", moduleLevel: 3, skillLevel: 1, stageIndex: 1, toggleOn: false } };
  const rows2 = [row({ opId: "a", buffIds: ["exusiai"] }), row({ opId: "b", buffIds: ["exusiai"] })];
  check("エクシア: モジュールXLv2以上装備(デフォルト)で2行選んでも警告なし", !findSingleTargetConflicts(catalog, rows2, {}, levelsDefault).has("exusiai"));
  const rows3 = [...rows2, row({ opId: "c", buffIds: ["exusiai"] })];
  check("エクシア: モジュールXLv2以上装備でも3行選ぶと警告", findSingleTargetConflicts(catalog, rows3, {}, levelsDefault).has("exusiai"));
  const levelsNoModule = { exusiai: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 1, stageIndex: 1, toggleOn: false } };
  check("エクシア: モジュール未装備なら2行でも警告(上限1のまま)", findSingleTargetConflicts(catalog, rows2, {}, levelsNoModule).has("exusiai"));
  const levelsLv1 = { exusiai: { elite: 2, potential: 5, moduleId: "uniequip_002_angel", moduleLevel: 1, skillLevel: 1, stageIndex: 1, toggleOn: false } };
  check("エクシア: モジュールXLv1(min_level未満)なら2行でも警告", findSingleTargetConflicts(catalog, rows2, {}, levelsLv1).has("exusiai"));
}
{
  // コーディネーター指示: ワルファリンS2/アS3もsingle_targetなので、2行で選ぶと警告が出ること。
  const plasmaBuf = { id: "plasma", name: "ワルファリンS2", kind: "pct", scope: { type: "individual" }, singleTarget: true, source: null, value: 0.9 };
  const durianBuf = { id: "durian", name: "アS3", kind: "pct", scope: { type: "individual" }, singleTarget: true, source: null, value: 0.5 };
  const catalog = { buffers: [plasmaBuf, durianBuf] };
  const plasmaRows2 = [row({ opId: "a", buffIds: ["plasma"] }), row({ opId: "b", buffIds: ["plasma"] })];
  check("ワルファリンS2: 2行で選ぶと⚠が出る", findSingleTargetConflicts(catalog, plasmaRows2, {}, {}).has("plasma"));
  const plasmaRows1 = [row({ opId: "a", buffIds: ["plasma"] })];
  check("ワルファリンS2: 1行だけなら⚠は出ない", !findSingleTargetConflicts(catalog, plasmaRows1, {}, {}).has("plasma"));
  const durianRows2 = [row({ opId: "a", buffIds: ["durian"] }), row({ opId: "b", buffIds: ["durian"] })];
  check("アS3: 2行で選ぶと⚠が出る", findSingleTargetConflicts(catalog, durianRows2, {}, {}).has("durian"));
}
{
  // computeBuffBreakdown経由でも育成設定(globalBuffLevels)が反映されること。
  const plasma = {
    id: "plasma",
    name: "ワルファリンS2",
    kind: "pct",
    value: 0.9,
    scope: { type: "individual" },
    singleTarget: true,
    bonus: null,
    source: {
      operatorId: "char_171_bldsk",
      operatorName: "ワルファリン",
      talent: null,
      skill: { skillNum: "2", skillLabel: "不安定血漿", valuesByLevel: [0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.7, 0.8, 0.9], varies: true },
      scale: null,
      basePct: null,
      stage: null,
      maxTargetsByModule: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 10, stageIndex: 1 },
    },
  };
  const catalog = { buffers: [plasma] };
  const b1 = computeBuffBreakdown(row({ buffIds: ["plasma"] }), { tags: [] }, catalog, [], { plasma: { skillLevel: 1 } });
  approxEqual(b1.individualPct, 0.3, 1e-9, "個別バフ(source付き): globalBuffLevelsのskillLevel=1で+30%");
  const b2 = computeBuffBreakdown(row({ buffIds: ["plasma"] }), { tags: [] }, catalog, [], {});
  approxEqual(b2.individualPct, 0.9, 1e-9, "個別バフ(source付き): globalBuffLevels省略時はデフォルト(Lv10)で+90%");
}

console.log("\n=== P5: state移行(旧stainless_1/stainless_2の2エントリ→新stainless_s1+toggle) ===\n");
{
  const stainlessS1 = {
    id: "stainless_s1",
    name: "ステインレスS1",
    kind: "pct",
    value: 0.48,
    scope: { type: "individual" },
    singleTarget: false,
    bonus: null,
    toggle: { label: "装置2台（×2）", mult: 2 },
    source: {
      operatorId: "char_4072_ironmn",
      operatorName: "ステインレス",
      talent: null,
      skill: null,
      scale: { skillNum: "1", skillLabel: "ハイパーブースト", valuesByLevel: [2, 2, 2, 2.5, 2.5, 2.5, 3, 3.5, 3.5, 4], varies: true },
      basePct: 0.12,
      stage: null,
      maxTargetsByModule: null,
      defaults: { elite: 2, potential: 5, moduleId: null, moduleLevel: 3, skillLevel: 10, stageIndex: 1 },
    },
  };
  const catalog = { operators: [{ ...op(1000, 0, null), id: "op", fkEntries: [{ tags: [] }] }], buffers: [stainlessS1], inspireSources: [] };
  const enemy = { hp: 0, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 };

  // 旧stainless_1(トグル無し相当)だった行 → stainless_s1(toggleOnはfalseのまま)へ移行。
  const oldRow1State = { v: 1, enemy, rows: [row({ opId: "op", entryIdx: 0, buffIds: ["stainless_1"] })], globalBuffIds: [] };
  const { state: migrated1 } = dropStaleRows(oldRow1State, catalog);
  check("stainless_1はstainless_s1へ移行される", migrated1.rows[0].buffIds.includes("stainless_s1") && !migrated1.rows[0].buffIds.includes("stainless_1"), migrated1.rows[0].buffIds);
  check("stainless_1だった場合、toggleOnはfalseのまま", migrated1.globalBuffLevels.stainless_s1.toggleOn === false, migrated1.globalBuffLevels.stainless_s1);

  // 旧stainless_2(装置2台相当)だった行 → stainless_s1 + toggleOn=trueへ移行。
  const oldRow2State = { v: 1, enemy, rows: [row({ opId: "op", entryIdx: 0, buffIds: ["stainless_2"] })], globalBuffIds: [] };
  const { state: migrated2 } = dropStaleRows(oldRow2State, catalog);
  check("stainless_2はstainless_s1へ移行される", migrated2.rows[0].buffIds.includes("stainless_s1") && !migrated2.rows[0].buffIds.includes("stainless_2"), migrated2.rows[0].buffIds);
  check("stainless_2だった場合、共有toggleOn=trueへ移行される", migrated2.globalBuffLevels.stainless_s1.toggleOn === true, migrated2.globalBuffLevels.stainless_s1);

  // 鼓舞ソース側のbuffIdsでも同じ移行が起きること。
  const sourceForMigration = {
    id: "src",
    operatorId: "op2",
    name: "ソース",
    tags: [],
    atkBase: 100,
    atkPotential: 0,
    modules: [],
    skills: [{ skillNum: "1", ratio: 1 }],
    talentPotentialLabel: "素質凸",
    selfParts: [],
  };
  const catalogWithSource = { ...catalog, inspireSources: [sourceForMigration] };
  const oldSourceState = {
    v: 1,
    enemy,
    rows: [],
    globalBuffIds: [],
    inspire: { sources: { src: { on: true, buffIds: ["stainless_2"] } } },
  };
  const { state: migratedSource } = dropStaleRows(oldSourceState, catalogWithSource);
  check(
    "鼓舞ソースのstainless_2もstainless_s1へ移行され、共有toggleOnがtrueになる",
    migratedSource.inspire.sources.src.buffIds.includes("stainless_s1") && migratedSource.globalBuffLevels.stainless_s1.toggleOn === true,
    migratedSource.inspire.sources.src,
  );
}

console.log("\n=== P6: 昇進/レベル/信頼度からのベースATK計算(computeBaseAtk/resolveAtk) ===\n");
{
  // エーベンホルツ(char_4046_ebnhlz)。オーナー確認済みの実データ。
  const ebenholz = {
    id: "op",
    name: "エーベンホルツ",
    tags: [],
    atkBase: 1550,
    atkPotential: 0,
    modules: [],
    fkEntries: [],
    phases: [
      { maxLevel: 50, atkMin: 611, atkMax: 873 },
      { maxLevel: 80, atkMin: 873, atkMax: 1134 },
      { maxLevel: 90, atkMin: 1134, atkMax: 1400 },
    ],
    atkTrustMax: 150,
    skillUnlockPhase: [
      ["1", 0],
      ["2", 1],
      ["3", 2],
    ],
  };
  check("E2 Lv60(信頼度0で補間だけ抜き出す) = 1310(1134+59×266/89=1310.34…の四捨五入)", computeBaseAtk(ebenholz, 2, 60, 0) === 1310, computeBaseAtk(ebenholz, 2, 60, 0));
  check("E2 Lv90(最大)・信頼度100% = atkBase(1400+150=1550)と一致", computeBaseAtk(ebenholz, 2, 90, 100) === 1550, computeBaseAtk(ebenholz, 2, 90, 100));
  check("E0 Lv1(信頼度0) = 611(補間の端)", computeBaseAtk(ebenholz, 0, 1, 0) === 611, computeBaseAtk(ebenholz, 0, 1, 0));
  check("E1 Lv1(=E0 Lv50の値と同じ871→873。信頼度0)", computeBaseAtk(ebenholz, 1, 1, 0) === 873, computeBaseAtk(ebenholz, 1, 1, 0));
  check("信頼度50%は四捨五入して個別に加算される(150×0.5=75)", computeBaseAtk(ebenholz, 2, 90, 50) === 1400 + 75, computeBaseAtk(ebenholz, 2, 90, 50));
  check("levelは1未満にクランプされる", computeBaseAtk(ebenholz, 2, -5, 0) === computeBaseAtk(ebenholz, 2, 1, 0));
  check("levelはmaxLevelにクランプされる", computeBaseAtk(ebenholz, 2, 9999, 0) === computeBaseAtk(ebenholz, 2, 90, 0));
  check("maxEliteFor=2(3段階)", maxEliteFor(ebenholz) === 2);
  check("maxLevelForElite(E1)=80", maxLevelForElite(ebenholz, 1) === 80);

  // シー(char_2015_dusk)。オーナー実機検証: E2 Lv71・信頼度100%・無モジュール・
  // 潜在+34 → ATK1031(886.618→887 + 110 + 34)。切り捨てだと1030になり実測と食い違う。
  const dusk = {
    id: "op2",
    name: "シー",
    tags: [],
    atkBase: 1028,
    atkPotential: 34,
    modules: [],
    fkEntries: [],
    phases: [
      { maxLevel: 50, atkMin: 426, atkMax: 601 },
      { maxLevel: 80, atkMin: 601, atkMax: 771 },
      { maxLevel: 90, atkMin: 771, atkMax: 918 },
    ],
    atkTrustMax: 110,
  };
  const duskRow = { elite: 2, level: 71, trust: 100, potential: true, moduleId: null, moduleLv: 3 };
  check(
    "シー E2 Lv71・信頼度100%・潜在ON・モジュール無し → resolveAtk=1031(オーナー実機確認値)",
    resolveAtk(dusk, duskRow) === 1031,
    resolveAtk(dusk, duskRow),
  );
  check(
    "上と同じ内訳: 補間部分だけ抜き出すと887(切り捨てなら886。信頼度0で分離して確認)",
    computeBaseAtk(dusk, 2, 71, 0) === 887,
    computeBaseAtk(dusk, 2, 71, 0),
  );

  // phasesが無い(旧来の簡易opオブジェクト)場合はatkBaseへフォールバックする(後方互換)。
  const legacyOp = { atkBase: 500, atkPotential: 0, modules: [] };
  check("phases無しのopはelite/levelを無視してatkBaseを返す", computeBaseAtk(legacyOp, 2, 1, 0) === 500);
  check("phases無しのopのmaxEliteForは0", maxEliteFor(legacyOp) === 0);
}

console.log("\n=== P6: モジュール装備可否(moduleUsable/effectiveModuleId) ===\n");
{
  const moduleE2Lv60 = { id: "m", typeName: "X", name: "m", atkByLevel: [10, 20, 30], unlockPhase: 2, unlockLevel: 60 };
  check("E2 Lv60ちょうどは装備可能", moduleUsable(moduleE2Lv60, 2, 60) === true);
  check("E2 Lv59は装備不可(境界)", moduleUsable(moduleE2Lv60, 2, 59) === false);
  check("E1(昇進不足)は装備不可", moduleUsable(moduleE2Lv60, 1, 90) === false);
  check("nullモジュールは常にfalse", moduleUsable(null, 2, 90) === false);

  const opWithModule = { id: "op3", name: "op3", tags: [], atkBase: 1000, atkPotential: 0, modules: [moduleE2Lv60], fkEntries: [], phases: [], atkTrustMax: 0 };
  check(
    "装備可能な組み合わせではeffectiveModuleIdがそのまま返る",
    effectiveModuleId(opWithModule, { moduleId: "m", elite: 2, level: 60 }) === "m",
  );
  check(
    "装備不可の組み合わせ(Lv59)ではeffectiveModuleIdがnullになる",
    effectiveModuleId(opWithModule, { moduleId: "m", elite: 2, level: 59 }) === null,
  );
  const atkUsable = resolveAtk(opWithModule, { elite: 2, level: 60, trust: 100, potential: false, moduleId: "m", moduleLv: 3 });
  const atkUnusable = resolveAtk(opWithModule, { elite: 2, level: 59, trust: 100, potential: false, moduleId: "m", moduleLv: 3 });
  check("装備不可の間はモジュールATKがresolveAtkに加算されない", atkUnusable < atkUsable, { atkUsable, atkUnusable });

  // モジュールにunlockPhase/unlockLevelが無い(既存の簡易テストオブジェクト)場合は常に装備可能
  // (後方互換。verify.mjsの他のop()ヘルパーがこの形を使い続けられるようにするため)。
  const legacyModule = { id: "m2", typeName: "Y", name: "m2", atkByLevel: [1, 2, 3] };
  check("unlockPhase/unlockLevel無しのモジュールは常に装備可能(後方互換)", moduleUsable(legacyModule, 0, 1) === true);
}

console.log("\n=== P6: makeDefaultRowの昇進/レベル/信頼度の既定値 ===\n");
{
  const opFullPhases = {
    id: "op4",
    name: "op4",
    tags: [],
    atkBase: 1000,
    atkPotential: 0,
    modules: [],
    fkEntries: [{ skillNum: "1", skillLabel: "s1", variantLabel: null, multiplier: { value: 1, source: "auto" }, multiplierCandidates: [], selfAtkPct: { value: 0, source: "auto" }, hits: { value: 1, source: "auto" }, damageType: { value: "physical", source: "auto" }, tags: [] }],
    phases: [
      { maxLevel: 50, atkMin: 100, atkMax: 200 },
      { maxLevel: 80, atkMin: 200, atkMax: 300 },
      { maxLevel: 90, atkMin: 300, atkMax: 400 },
    ],
    atkTrustMax: 50,
    skillUnlockPhase: [["1", 0]],
  };
  const rowFull = makeDefaultRow(opFullPhases, 0);
  check("3段階あるopの既定昇進はE2", rowFull.elite === 2, rowFull.elite);
  check("既定レベルはE2の最大(90)", rowFull.level === 90, rowFull.level);
  check("既定信頼度は100", rowFull.trust === 100, rowFull.trust);

  // フェーズが2つしか無いオペレーター(6凸できない低レアリティ想定)は既定昇進もE1止まり。
  const opTwoPhases = { ...opFullPhases, phases: opFullPhases.phases.slice(0, 2) };
  const rowTwo = makeDefaultRow(opTwoPhases, 0);
  check("フェーズが2つのopの既定昇進はE1", rowTwo.elite === 1, rowTwo.elite);
  check("既定レベルはE1の最大(80)", rowTwo.level === 80, rowTwo.level);
}

console.log("\n=== P6: スキル解放昇進の警告(skillUnlockWarning) ===\n");
{
  const opS3E2 = {
    id: "op5",
    name: "op5",
    skillUnlockPhase: [
      ["1", 0],
      ["2", 1],
      ["3", 2],
    ],
    phases: [{ maxLevel: 1 }, { maxLevel: 1 }, { maxLevel: 1 }],
  };
  const entryS3 = { skillNum: "3" };
  check("E1でS3(昇進2解放)を選ぶと警告が出る", skillUnlockWarning(opS3E2, entryS3, { elite: 1 }) === "S3は昇進2で解放");
  check("E2でS3を選ぶと警告は出ない", skillUnlockWarning(opS3E2, entryS3, { elite: 2 }) === null);
  const entryS1 = { skillNum: "1" };
  check("E0でS1(昇進0解放)を選んでも警告は出ない", skillUnlockWarning(opS3E2, entryS1, { elite: 0 }) === null);
  const entryTalent = { skillNum: "素質1" };
  check("素質行(skillUnlockPhaseに無いskillNum)は警告対象外", skillUnlockWarning(opS3E2, entryTalent, { elite: 0 }) === null);
}

console.log("\n=== P7: スキルLv別の倍率/セルフ%解決(valueAtLevel/resolveEntryValues) ===\n");
{
  // ブレイズS3を模したエントリ(multiplier_key相当のAuto倍率+self_atk_pct_factor相当のManualセルフ%)。
  const entryLv = {
    skillNum: "3",
    skillLabel: "s3",
    variantLabel: null,
    multiplier: { value: 3.0, source: "auto" },
    multiplierByLevel: [2.0, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 3.0],
    multiplierFixed: false,
    multiplierCandidates: [],
    selfAtkPct: { value: 0.712, source: "manual" },
    selfAtkPctByLevel: [0.267, 0.3115, 0.356, 0.4005, 0.445, 0.4895, 0.534, 0.5785, 0.623, 0.712],
    selfAtkPctFixed: false,
    hits: { value: 1, source: "auto" },
    damageType: { value: "physical", source: "auto" },
    tags: [],
  };
  check("valueAtLevel: SLv1(index0)", valueAtLevel(entryLv.multiplierByLevel, 1) === 2.0);
  check("valueAtLevel: 特化3(index9)", valueAtLevel(entryLv.multiplierByLevel, 10) === 3.0);
  check("valueAtLevel: 配列長を超える値は末尾にクランプされる", valueAtLevel(entryLv.multiplierByLevel, 999) === 3.0);
  check("valueAtLevel: 0以下は先頭にクランプされる", valueAtLevel(entryLv.multiplierByLevel, 0) === 2.0);
  check("valueAtLevel: 空配列は0を返す", valueAtLevel([], 5) === 0);

  check("resolveMultiplierAtLevel: SLv7", resolveMultiplierAtLevel(entryLv, 7) === 2.6);
  check("resolveSelfAtkPctAtLevel: SLv7(0.534)", resolveSelfAtkPctAtLevel(entryLv, 7) === 0.534);

  const valuesAtLv1 = resolveEntryValues(entryLv, 1);
  check("resolveEntryValues(SLv1): multiplier=2.0", valuesAtLv1.multiplier === 2.0);
  check("resolveEntryValues(SLv1): selfPct=0.267", valuesAtLv1.selfPct === 0.267);
  const valuesDefault = resolveEntryValues(entryLv); // skillLevel省略時は既定10(特化3)
  check("resolveEntryValues(省略時=特化3): multiplier=3.0", valuesDefault.multiplier === 3.0);
  check("resolveEntryValues(省略時=特化3): selfPct=0.712", valuesDefault.selfPct === 0.712);

  // multiplierByLevel/selfAtkPctByLevelが無い(旧形/未解決)エントリは固定値(.value)にフォールバックする。
  const entryNoByLevel = { multiplier: { value: 1.5, source: "auto" }, selfAtkPct: { value: 0.1, source: "auto" } };
  check("multiplierByLevelが無ければ.valueにフォールバック", resolveMultiplierAtLevel(entryNoByLevel, 3) === 1.5);
  check("selfAtkPctByLevelが無ければ.valueにフォールバック", resolveSelfAtkPctAtLevel(entryNoByLevel, 3) === 0.1);
}

console.log("\n=== P7: 昇進によるスキルLv上限の警告(maxSkillLevelForElite/skillLevelWarning) ===\n");
{
  check("E0の上限はSLv4", maxSkillLevelForElite(0) === 4);
  check("E1の上限はSLv7", maxSkillLevelForElite(1) === 7);
  check("E2の上限は特化3(10)", maxSkillLevelForElite(2) === 10);

  check("E0でSLv4は警告無し", skillLevelWarning(0, 4) === null);
  check("E0でSLv5は警告あり", skillLevelWarning(0, 5) === "SLv5以上は昇進1で解放");
  check("E1でSLv7は警告無し", skillLevelWarning(1, 7) === null);
  check("E1で特化1(8)は警告あり", skillLevelWarning(1, 8) === "特化は昇進2で解放");
  check("E0で特化1(8)も同じ警告文言", skillLevelWarning(0, 8) === "特化は昇進2で解放");
  check("E2で特化3(10)は警告無し", skillLevelWarning(2, 10) === null);
}

console.log("\n=== P7: dropStaleRowsが旧形にskillLevel既定値(10)を補完する ===\n");
{
  const opForMigration = {
    id: "opMig",
    name: "opMig",
    tags: [],
    atkBase: 1000,
    atkPotential: 0,
    modules: [],
    fkEntries: [{ skillNum: "1", skillLabel: "s1", variantLabel: null, multiplier: { value: 1, source: "auto" }, multiplierCandidates: [], selfAtkPct: { value: 0, source: "auto" }, hits: { value: 1, source: "auto" }, damageType: { value: "physical", source: "auto" }, tags: [] }],
    phases: [{ maxLevel: 90, atkMin: 900, atkMax: 1000 }],
    atkTrustMax: 0,
    skillUnlockPhase: [],
  };
  const sourceForMigration = { id: "srcMig", operatorId: "opMig", name: "srcMig", tags: [], atkBase: 1000, atkPotential: 0, modules: [], skills: [{ skillNum: "1", ratio: 1, ratioByLevel: [1], ratioFixed: true }], talentPotentialLabel: "凸", selfParts: [] };
  const catalogForMigration = { operators: [opForMigration], buffers: [], inspireSources: [sourceForMigration] };
  const oldState = {
    v: 1,
    enemy: enemyNeutral,
    rows: [{ opId: "opMig", entryIdx: 0, dmgType: "physical", potential: true, moduleId: null, moduleLv: 3, multiplier: 1, selfPct: 0, hits: 1, buffPct: 0, dmgMult: 1, ignoreDef: 0 }],
    inspire: { sources: { srcMig: { on: true, skillNum: "1" } } },
  };
  const { state: cleaned } = dropStaleRows(oldState, catalogForMigration);
  check("旧形の行にskillLevel=10(特化3)が補完される", cleaned.rows[0].skillLevel === 10, cleaned.rows[0].skillLevel);
  check("旧形の鼓舞ソースcfgにもskillLevel=10が補完される", cleaned.inspire.sources.srcMig.skillLevel === 10, cleaned.inspire.sources.srcMig.skillLevel);
}

console.log("\n=== P7: 鼓舞ソースのratioByLevel解決(resolveInspireRatioAtLevel/computeInspireSource) ===\n");
{
  const skillEntryLv = { skillNum: "3", ratio: 1.1, ratioByLevel: [0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.9, 1.0, 1.1] };
  check("resolveInspireRatioAtLevel: SLv1", resolveInspireRatioAtLevel(skillEntryLv, 1) === 0.5);
  check("resolveInspireRatioAtLevel: 特化3(既定)", resolveInspireRatioAtLevel(skillEntryLv, 10) === 1.1);
  const skillEntryFixed = { skillNum: "2", ratio: 0.6, ratioByLevel: [0.6], ratioFixed: true };
  check("resolveInspireRatioAtLevel: ratioByLevelが1件だけならその値に固定", resolveInspireRatioAtLevel(skillEntryFixed, 1) === 0.6);
  check("resolveInspireRatioAtLevel: ratioByLevel無しは.ratioにフォールバック", resolveInspireRatioAtLevel({ ratio: 0.42 }, 5) === 0.42);

  const sourceLv = {
    id: "srcLv",
    operatorId: "opLv",
    name: "srcLv",
    tags: [],
    atkBase: 1000,
    atkPotential: 0,
    modules: [],
    skills: [skillEntryLv],
    talentPotentialLabel: "凸",
    selfParts: [],
  };
  const cfgLv1 = { on: true, skillNum: "3", skillLevel: 1, potential: false, talentPotential: false, moduleId: null, moduleLv: 3, buffPct: 0, buffIds: [], parts: {} };
  const cfgLv10 = { ...cfgLv1, skillLevel: 10 };
  const catalogEmpty = { buffers: [] };
  const resultLv1 = computeInspireSource(sourceLv, cfgLv1, catalogEmpty);
  const resultLv10 = computeInspireSource(sourceLv, cfgLv10, catalogEmpty);
  approxEqual(resultLv1.amount, 1000 * 0.5, 1e-9, "鼓舞ソースSLv1(ratio=0.5)のamount");
  approxEqual(resultLv10.amount, 1000 * 1.1, 1e-9, "鼓舞ソース特化3(ratio=1.1)のamount");
}

console.log("\n=== engine.js が document を参照していないこと ===\n");
{
  const here = path.dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(path.join(here, "static", "engine.js"), "utf8");
  check("engine.js に 'document' という文字列が出現しない", !src.includes("document"));
  check("engine.js に 'window' という文字列が出現しない", !src.includes("window"));
}

console.log(`\n${allOk ? "=== すべて合格 ===" : "=== 不合格の項目あり ==="}`);
process.exitCode = allOk ? 0 : 1;
