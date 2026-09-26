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

console.log("\n=== engine.js が document を参照していないこと ===\n");
{
  const here = path.dirname(fileURLToPath(import.meta.url));
  const src = readFileSync(path.join(here, "static", "engine.js"), "utf8");
  check("engine.js に 'document' という文字列が出現しない", !src.includes("document"));
  check("engine.js に 'window' という文字列が出現しない", !src.includes("window"));
}

console.log(`\n${allOk ? "=== すべて合格 ===" : "=== 不合格の項目あり ==="}`);
process.exitCode = allOk ? 0 : 1;
