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

console.log("\n=== engine.js が document を参照していないこと ===\n");
{
  const here = path.dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(path.join(here, "static", "engine.js"), "utf8");
  check("engine.js に 'document' という文字列が出現しない", !src.includes("document"));
  check("engine.js に 'window' という文字列が出現しない", !src.includes("window"));
}

console.log(`\n${allOk ? "=== すべて合格 ===" : "=== 不合格の項目あり ==="}`);
process.exitCode = allOk ? 0 : 1;
