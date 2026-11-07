// フレームキル計算機 — 撃破するための提案。

import { computeEffectiveGlobalBuffLevels } from "./buffs.js";
import { computeTotal } from "./damage.js";
import { findOperator } from "./entry.js";
import { defaultInspireSourceCfg } from "./inspire.js";

/* ============================================================
   撃破するための提案（`suggest`）。撃破できていない時だけ意味を持つ。
   「効果が単調に増える1変数」を整数二分探索で求める素朴な実装。
   ============================================================ */

// 「現実的にありえそうな範囲」に上限を設ける（UIラウンド2でオーナー指摘: 無制限探索だと
// 「Hit数を+29増やす」「バフ+4806%増やす」のような非現実的な提案が出てしまっていた）。
// 上限を超えないと撃破できない変更は「不可能」として提案から除外する
// （`minimalIntegerSatisfying`はhiまで探しても見つからなければnullを返す）。
const SUGGEST_CAP_BUFF_PCT = 300; // 追加バフ%の上限（+300%まで。仕様どおりの固定値）
const SUGGEST_CAP_HITS = 3; // 追加Hit数の上限（仕様どおりシンプルに3固定）

/**
 * `predicate`が[lo,hi]区間で単調非減少(false...false,true...true)である前提で、
 * predicateが真になる最小の整数を返す。hiでも真にならなければnull(=不可能)。
 */
function minimalIntegerSatisfying(lo, hi, predicate) {
  if (!predicate(hi)) return null;
  if (predicate(lo)) return lo;
  let l = lo,
    h = hi;
  while (l < h) {
    const mid = l + Math.floor((h - l) / 2);
    if (predicate(mid)) h = mid;
    else l = mid + 1;
  }
  return l;
}

/**
 * 撃破できていない状態に対し、撃破に足りる最小の変更案を最大4件まで返す。
 * 種類: 'rowBuffPct'(その行のバフ+n%、上限+300%) / 'rowHits'(その行のHit数+n、上限+3) /
 *       'enemyDefFlat'(敵の防御-n、物理行が1つでも5%floorでなければ。上限は敵の現在の防御値) /
 *       'enemyResFlat'(敵の術耐性-n、術行が1つでも5%floorでなければ。上限は敵の現在の術耐性値) /
 *       'addIndividualBuff'(P2。その行にまだ選んでいない個別バフを1つ追加する) /
 *       'toggleGlobalBuff'(P2。まだOFFの条件付きバフを1つONにする)。
 * 上限を超えないと撃破できない場合や、効果が無い（floor済みで伸びしろが無い等）場合は
 * その種類の提案を出さない。全種類が出せなければ`suggest()`は空配列を返す
 * （呼び出し側は「現実的な補正では届きません」のような文言を出す想定）。
 * `effort`（小さいほど「簡単」）昇順でソートする。hits系は+1した値をeffortにする
 * （%やflatの数値と同じ物差しに乗せるための簡単な変換）。バフ系の`effort`はそのバフの
 * pct(%換算)/flat値そのもの（"cheapest first by added pct"の仕様どおり）。
 */
export function suggest(state, catalog) {
  const { rows, enemy, globalBuffIds = [] } = state;
  // P9: FK行リンク込みの解決値を使う(`state.globalBuffLevels`を直接見ない)。
  const globalBuffLevels = computeEffectiveGlobalBuffLevels(catalog, state);
  const inspireSourceStates = (state.inspire && state.inspire.sources) || {};
  const { results, killed } = computeTotal(catalog, rows, enemy, globalBuffIds, inspireSourceStates, globalBuffLevels);
  if (killed) return [];

  const suggestions = [];
  const killsWith = (testRows, testEnemy, testGlobalBuffIds, testInspireSourceStates) =>
    computeTotal(
      catalog,
      testRows,
      testEnemy ?? enemy,
      testGlobalBuffIds ?? globalBuffIds,
      testInspireSourceStates ?? inspireSourceStates,
      globalBuffLevels,
    ).killed;

  results.forEach((r, i) => {
    if (!r) return;

    const buffNeeded = minimalIntegerSatisfying(1, SUGGEST_CAP_BUFF_PCT, (extraPct) =>
      killsWith(rows.map((row, j) => (j === i ? { ...row, buffPct: row.buffPct + extraPct / 100 } : row))),
    );
    if (buffNeeded !== null) {
      suggestions.push({ kind: "rowBuffPct", rowIndex: i, amount: buffNeeded, effort: buffNeeded });
    }

    // P10(混合スキル): Hit数は全パーツ共通(row.hits)なので、提案も行単位で1回だけ。
    const hitsNeeded = minimalIntegerSatisfying(1, SUGGEST_CAP_HITS, (extraHits) =>
      killsWith(rows.map((row, j) => (j === i ? { ...row, hits: row.hits + extraHits } : row))),
    );
    if (hitsNeeded !== null) {
      suggestions.push({ kind: "rowHits", rowIndex: i, amount: hitsNeeded, effort: hitsNeeded + 1 });
    }
  });

  // 防御/術耐性の-固定は「敵の現在値を超えて下げる提案はしない」という上限を持つ
  // （0以下にはできない探索区間にする。現在値が0ならそもそも探索しない＝提案を出さない）。
  // P10(混合スキル): 「非floorの物理/術があるか」はパーツまで見る(entry.dmgTypeは
  // パーツ0の属性でしかないため、r.partsの各dmgType/atFloorを確認する)。
  const hasNonFloorPhysical = results.some((r) => r && r.parts.some((d) => d.dmgType === "physical" && !d.atFloor));
  const defCap = Math.floor(enemy.def);
  if (hasNonFloorPhysical && defCap > 0) {
    const defNeeded = minimalIntegerSatisfying(1, defCap, (extra) =>
      killsWith(rows, { ...enemy, defFlat: enemy.defFlat + extra }),
    );
    if (defNeeded !== null) {
      suggestions.push({ kind: "enemyDefFlat", amount: defNeeded, effort: defNeeded });
    }
  }

  const hasNonFloorArts = results.some((r) => r && r.parts.some((d) => d.dmgType === "arts" && !d.atFloor));
  const resCap = Math.floor(enemy.res);
  if (hasNonFloorArts && resCap > 0) {
    const resNeeded = minimalIntegerSatisfying(1, resCap, (extra) =>
      killsWith(rows, { ...enemy, resFlat: enemy.resFlat + extra }),
    );
    if (resNeeded !== null) {
      suggestions.push({ kind: "enemyResFlat", amount: resNeeded, effort: resNeeded });
    }
  }

  // P2: 個別バフを1件追加するだけで撃破できる行を提案する（+300%キャップを流用: そのバフ
  // 自体のpctが300%を超えることは無いが、念のため同じ上限で足切りする）。
  const buffers = (catalog && catalog.buffers) || [];
  const individualBuffers = buffers.filter((b) => b.scope.type === "individual");
  rows.forEach((row, i) => {
    if (!row.opId) return;
    const already = new Set(row.buffIds || []);
    for (const b of individualBuffers) {
      if (already.has(b.id)) continue;
      const effort = b.kind === "pct" ? b.value * 100 : b.value;
      if (b.kind === "pct" && effort > SUGGEST_CAP_BUFF_PCT) continue;
      const testRows = rows.map((row2, j) => (j === i ? { ...row2, buffIds: [...(row2.buffIds || []), b.id] } : row2));
      if (killsWith(testRows)) {
        suggestions.push({ kind: "addIndividualBuff", rowIndex: i, buffId: b.id, buffName: b.name, effort });
      }
    }
  });

  // P2: 条件付きバフを1件ONにするだけで撃破できる場合を提案する。
  const conditionalBuffers = buffers.filter((b) => b.scope.type === "conditional");
  for (const b of conditionalBuffers) {
    if (globalBuffIds.includes(b.id)) continue;
    const effort = b.kind === "pct" ? b.value * 100 : b.value;
    if (b.kind === "pct" && effort > SUGGEST_CAP_BUFF_PCT) continue;
    if (killsWith(rows, enemy, [...globalBuffIds, b.id])) {
      suggestions.push({ kind: "toggleGlobalBuff", buffId: b.id, buffName: b.name, effort });
    }
  }

  // P3: 鼓舞ソースをONにするだけで撃破できる場合を提案する(effort=0固定。「ONにする/
  // しない」の2択でしかなく、%やHit数のような度合いが無いため)。既に設定済みのcfgが
  // あればそれを流用し(スキル/モジュール等はそのまま)、無ければ既定値でONにして試す。
  const inspireSources = (catalog && catalog.inspireSources) || [];
  for (const source of inspireSources) {
    const cur = inspireSourceStates[source.id] || defaultInspireSourceCfg(source);
    if (cur.on) continue;
    const testStates = { ...inspireSourceStates, [source.id]: { ...cur, on: true } };
    if (killsWith(rows, enemy, globalBuffIds, testStates)) {
      suggestions.push({ kind: "toggleInspireSource", sourceId: source.id, sourceName: source.name, effort: 0 });
    }
  }

  // P3: 行の鼓舞トグルをONにするだけで撃破できる場合を提案する(effort=0固定。理由は上と同じ)。
  rows.forEach((r, i) => {
    if (!r.opId || r.inspireOn !== false) return;
    const testRows = rows.map((r2, j) => (j === i ? { ...r2, inspireOn: true } : r2));
    if (killsWith(testRows)) {
      suggestions.push({ kind: "toggleRowInspire", rowIndex: i, effort: 0 });
    }
  });

  // P3: ONになっている鼓舞ソースに個別バフを1件追加するだけで撃破できる場合を提案する
  // (OFFのソースへ追加しても効果が測れないため対象外)。
  for (const source of inspireSources) {
    const cur = inspireSourceStates[source.id];
    if (!cur || !cur.on) continue;
    const already = new Set(cur.buffIds || []);
    for (const b of individualBuffers) {
      if (already.has(b.id)) continue;
      const effort = b.kind === "pct" ? b.value * 100 : b.value;
      if (b.kind === "pct" && effort > SUGGEST_CAP_BUFF_PCT) continue;
      const testStates = { ...inspireSourceStates, [source.id]: { ...cur, buffIds: [...(cur.buffIds || []), b.id] } };
      if (killsWith(rows, enemy, globalBuffIds, testStates)) {
        suggestions.push({ kind: "addSourceIndividualBuff", sourceId: source.id, sourceName: source.name, buffId: b.id, buffName: b.name, effort });
      }
    }
  }

  suggestions.sort((a, b) => a.effort - b.effort);
  return suggestions.slice(0, 4);
}

/** `suggest()`の1件を日本語1行に整形する（DOM非依存。ui.js からもverify.mjsからも使う）。 */
export function describeSuggestion(sug, catalog, rows) {
  const rowOpName = (idx) => {
    const op = findOperator(catalog, rows[idx].opId);
    return op ? op.name : "?";
  };
  switch (sug.kind) {
    case "rowBuffPct":
      return `${rowOpName(sug.rowIndex)}のバフを+${sug.amount}%増やす`;
    case "rowHits":
      return `${rowOpName(sug.rowIndex)}のHit数を+${sug.amount}増やす`;
    case "enemyDefFlat":
      return `敵の防御を-${sug.amount}させる`;
    case "enemyResFlat":
      return `敵の術耐性を-${sug.amount}させる`;
    case "addIndividualBuff":
      return `${rowOpName(sug.rowIndex)}に個別バフ「${sug.buffName}」を追加する`;
    case "toggleGlobalBuff":
      return `条件付きバフ「${sug.buffName}」をONにする`;
    case "toggleInspireSource":
      return `${sug.sourceName}の鼓舞をONにする`;
    case "toggleRowInspire":
      return `${rowOpName(sug.rowIndex)}に鼓舞を適用する`;
    case "addSourceIndividualBuff":
      return `${sug.sourceName}に個別バフ「${sug.buffName}」を追加する`;
    default:
      return "";
  }
}
