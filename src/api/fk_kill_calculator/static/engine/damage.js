// フレームキル計算機 — 1行と全体のダメージ計算。

import { computeBuffBreakdown } from "./buffs.js";
import {
  expandRowParts,
  findEntry,
  findOperator,
  resolveSpecialAddPct,
  resolveSpecialMultiplierFactor,
} from "./entry.js";
import { computeInspireForRow } from "./inspire.js";
import { resolveAtk, resolveDefEff, resolveResEff } from "./stats.js";
import { expandStoneBoardParts } from "./stone_board.js";

/**
 * 1行分のダメージを計算する。`inspireFlat`はP1/P2の鼓舞合計(flat種バフのΣ)を足すための
 * 差し込み口、`extraPct`はP2の個別/条件付きバフ(pct種)+特殊強化(加算系)のΣ差し込み口、
 * `multiplierFactor`はP2 follow-upの特殊強化(乗算系)の係数差し込み口
 * (いずれも既定値でP1と完全互換: inspireFlat=0/extraPct=0/multiplierFactor=1)。
 * @returns {{atk:number, pct:number, final:number, defEff:number, resEff:number,
 *            perHit:number, atFloor:boolean, rowDamage:number}}
 */
export function computeRowDamage(op, row, enemy, inspireFlat = 0, extraPct = 0, multiplierFactor = 1) {
  const atk = resolveAtk(op, row);
  const pct = row.selfPct + row.buffPct + extraPct;
  const buffFlat = Number.isFinite(row.buffFlat) ? row.buffFlat : 0;
  const final = ((atk + buffFlat) * (1 + pct) + inspireFlat) * row.multiplier * multiplierFactor;
  const defEff = resolveDefEff(enemy, row.ignoreDef);
  const resEff = resolveResEff(enemy);

  let perHit, atFloor;
  const floor = final * 0.05;
  if (row.dmgType === "physical") {
    const reduced = final - defEff;
    atFloor = reduced <= floor;
    perHit = Math.max(reduced, floor);
  } else if (row.dmgType === "arts") {
    const reduced = final * (1 - resEff / 100);
    atFloor = reduced <= floor;
    perHit = Math.max(reduced, floor);
  } else {
    // true damage: 軽減も5%floorも無い
    perHit = final;
    atFloor = false;
  }

  const rowDamage = perHit * row.dmgMult * (1 + enemy.vulnPct) * row.hits;
  return { atk, pct, final, defEff, resEff, perHit, atFloor, rowDamage };
}

/**
 * 全行の合計ダメージと撃破可否。opId解決に失敗した行(カタログとズレた古いURL等)は
 * `results`に`null`を置き、合計には含めない。呼び出し側は事前に`dropStaleRows`で
 * 弾いておくのが基本だが、ここでも二重に安全策を取る。
 * `globalBuffIds`(P2。省略時は`[]`=P1互換)は条件付きバフの判定に使う。
 * `inspireSourceStates`(P3。省略時は`{}`=P1/P2互換)は`state.inspire.sources`
 * (id→設定)をそのまま渡す想定で、`computeInspireForRow`が各行の鼓舞(最大1件)を決める。
 * `globalBuffLevels`(P4。省略時は`{}`=P1〜P3互換)は`state.globalBuffLevels`
 * (buffId→昇進/潜在/モジュール/スキルLv/トグルの選択)をそのまま渡す想定で、
 * `source`付き条件付きバフの値解決(`resolveConditionalValue`)に使う。
 * P10(混合スキル): `results[i]`はパーツ0(=`entry.parts`が無い通常行はその行自身)の
 * 値を従来通りトップレベルに持ちつつ、`parts`(各パーツの`computeRowDamage`結果+`dmgType`)
 * と、全パーツの合計である`rowDamage`を追加で持つ。
 * @returns {{results:(Array<null|{row:RowState, op:object, breakdown:object, inspireApplied:object,
 *            parts:Array<{dmgType:string}&ReturnType<typeof computeRowDamage>>}&ReturnType<typeof computeRowDamage>>),
 *            total:number, killed:boolean}}
 */
export function computeTotal(catalog, rows, enemy, globalBuffIds = [], inspireSourceStates = {}, globalBuffLevels = {}) {
  const results = rows.map((row) => {
    const op = findOperator(catalog, row.opId);
    if (!op) return null;
    const entry = findEntry(op, row.entryIdx);
    const breakdown = computeBuffBreakdown(row, entry, catalog, globalBuffIds, globalBuffLevels);
    const specialAddPct = resolveSpecialAddPct(op, entry, row);
    const specialMulFactor = resolveSpecialMultiplierFactor(op, entry, row);
    const inspireApplied = computeInspireForRow(catalog, row, inspireSourceStates, globalBuffIds, globalBuffLevels);
    const inspireFlat = breakdown.extraFlat + (inspireApplied ? inspireApplied.amount : 0);
    const extraPct = breakdown.extraPct + specialAddPct;
    // P10(混合スキル): 行をパーツ(`entry.parts`が無ければ`[row]`自身のみ)に展開し、
    // バフ内訳/鼓舞/特殊強化(上で1回だけ計算した値)を全パーツ共通に適用してから合算する。
    // 碁石盤の行は、同じスタック数の碁石を1パーツにまとめる(`stacks`/`count`を持つ)。
    const parts = entry && entry.stoneBoard
      ? expandStoneBoardParts(op, entry, row, enemy).map(({ partRow, partEnemy, stacks, count }) => ({
          dmgType: partRow.dmgType,
          stacks,
          count,
          ...computeRowDamage(op, partRow, partEnemy, inspireFlat, extraPct, specialMulFactor),
        }))
      : expandRowParts(row).map((partRow) => ({
          dmgType: partRow.dmgType,
          ...computeRowDamage(op, partRow, enemy, inspireFlat, extraPct, specialMulFactor),
        }));
    const rowDamage = parts.reduce((sum, d) => sum + d.rowDamage, 0);
    const head = parts[0];
    return { row, op, breakdown, specialAddPct, specialMulFactor, inspireApplied, ...head, rowDamage, parts };
  });
  const total = results.reduce((sum, r) => sum + (r ? r.rowDamage : 0), 0);
  return { results, total, killed: total >= enemy.hp };
}
