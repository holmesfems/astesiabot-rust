// フレームキル計算機 — 計算式の表示。

import { computeTotal, findEntry, resolveStoneBoardValues } from "../engine.js";
import { escapeHtml, fmtInt, fmtPct, trimNum } from "./dom.js";
import { buffLevelsState, catalog, inspireStates, state } from "./store.js";

// フォーミュラ行(P2/P3): `691 ×(1 + セルフ0% + 個別150% + 条件0% + 手入力0%) × 400% = 6,910 /hit`。
// 鼓舞(flat種バフ + P3の鼓舞ソースからの加算の合計)は0でない時だけ足す(仕様どおり)。
// P10(混合スキル): パーツごとに1行ずつ式を組み立て、パーツが2つ以上ある時だけ
// 最後に合計行を足す(パーツを持たない通常行は従来通り1行のまま)。
export function renderFormulaLine(op, row) {
  const entry = findEntry(op, row.entryIdx);
  const { rowDamage, breakdown, specialAddPct, specialMulFactor, inspireApplied, parts } = computeTotal(
    catalog,
    [row],
    state.enemy,
    state.globalBuffIds,
    inspireStates(),
    buffLevelsState(),
  ).results[0];
  const specialLabel = entry && entry.special ? entry.special.label : "";
  const selfPart = specialAddPct > 0 ? `セルフ${fmtPct(row.selfPct)}%+${specialLabel}${fmtPct(specialAddPct)}%` : `セルフ${fmtPct(row.selfPct)}%`;
  const individualPart = `個別${fmtPct(breakdown.individualPct)}%`;
  const conditionalPart = `条件${fmtPct(breakdown.conditionalPct)}%`;
  const manualPart = `手入力${fmtPct(row.buffPct)}%`;
  const flatTotal = breakdown.individualFlat + breakdown.conditionalFlat + (inspireApplied ? inspireApplied.amount : 0);
  const inspirePart = flatTotal !== 0 ? ` + 鼓舞${fmtInt(flatTotal)}` : "";
  const buffFlat = row.buffFlat || 0;
  const stoneValues = entry && entry.stoneBoard ? resolveStoneBoardValues(op, entry, row) : null;
  if (stoneValues && parts.every((d) => d.count === 0)) {
    return "発動する碁石がありません（隣に碁石があり、赤マスから2マス以内の碁石が発動します）";
  }
  const isMulti = parts.length > 1;

  const lines = parts.map((d, i) => {
    const partLabel = stoneValues
      ? `${d.stacks}スタック: `
      : isMulti && entry && entry.parts && entry.parts[i] ? `${entry.parts[i].label}: ` : "";
    const partMultiplier = i === 0 || stoneValues ? row.multiplier : ((row.extraParts && row.extraParts[i - 1] && row.extraParts[i - 1].multiplier) ?? 0);
    const partHits = row.hits; // Hit数は全パーツ共通
    const stackPart = stoneValues ? ` ×(1 + ${d.stacks}×${fmtPct(stoneValues.damagePerStack)}%)` : "";
    const multiplierPart =
      specialMulFactor !== 1
        ? `${fmtPct(partMultiplier)}%${stackPart} × ×${trimNum(specialMulFactor)}(${specialLabel})`
        : `${fmtPct(partMultiplier)}%${stackPart}`;
    // 鼓舞(固定値)は%適用後・倍率の前に足すので、鼓舞がある時は外側を括弧で囲んで
    // 「鼓舞にだけ倍率が掛かる」ように読めないようにする。
    const atkText = buffFlat !== 0 ? `(${fmtInt(d.atk)}${buffFlat > 0 ? "+" : "-"}${fmtInt(Math.abs(buffFlat))})` : fmtInt(d.atk);
    const atkPart = `${atkText} ×(1 + ${selfPart} + ${individualPart} + ${conditionalPart} + ${manualPart})`;
    const beforeMultiplier = inspirePart ? `(${atkPart}${inspirePart})` : atkPart;
    const unit = stoneValues ? "個" : "hit";
    const line1 = `${partLabel}${beforeMultiplier} × ${multiplierPart} = ${fmtInt(d.final)} /${unit}`;
    const floorNote = d.atFloor ? `<span class="floor-note">（5%floor発動中）</span>` : "";
    const line2 = stoneValues
      ? `→ 実効術耐性${trimNum(d.resEff)}で 実ダメ ${fmtInt(d.perHit)}/個 × ${d.count}個 = <b>${fmtInt(d.rowDamage)}</b> ${floorNote}`
      : `→ 実ダメ ${fmtInt(d.perHit)}/hit × ${partHits}Hit = <b>${fmtInt(d.rowDamage)}</b> ${floorNote}`;
    return `${escapeHtml(line1)}<br>${line2}`;
  });
  const totalLine = isMulti ? `<br>合計 = <b>${fmtInt(rowDamage)}</b>` : "";
  return lines.join("<br>") + totalLine;
}
