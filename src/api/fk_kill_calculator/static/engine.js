/* ============================================================
   フレームキル計算機 — 計算層（DOM非依存。表示・URL共有は ui.js 側）

   単位の約束（`state.enemy`/`row`のフィールド名の意味）:
     - "Pct"/"pct" で終わるフィールド（selfPct/buffPct/vulnPct/defPct）は
       小数の割合（9% なら 0.09）。ダメージ倍率(dmgMult)も同様に 100% なら 1.0。
     - res/resFlat はアークナイツ内の術耐性表示と同じ 0〜100 のパーセント点数
       （0.3 ではなく 30）。resEff もこのスケールのまま(0〜100)。
     - def/defFlat/ignoreDef は素の防御力の数値（例: 300）。

   全体の流れ（1行=1オペレーターのFK指定）:
     atk    = atkBase + (potential ? atkPotential : 0) + (module ? module.atkByLevel[lv-1] : 0)
     pct    = selfPct + buffPct                          （加算）
     final  = (atk × (1 + pct) + inspireFlat) × multiplier
              （inspireFlatは常に0。P2/P3で「Σ鼓舞」を足す差し込み口として残してある）
     defEff = max(def × (1 − defPct) − defFlat − ignoreDef, 0)
     resEff = clamp(res − resFlat, 0, 100)
     物理: perHit = max(final − defEff, final × 0.05)
     術  : perHit = max(final × (1 − resEff/100), final × 0.05)
     真  : perHit = final
     rowDamage = perHit × dmgMult × (1 + vulnPct) × hits
     total = Σ rowDamage; killed = total >= hp
   ============================================================ */

/**
 * @typedef {{hp:number, def:number, res:number, defFlat:number, defPct:number,
 *            resFlat:number, vulnPct:number}} EnemyState
 * @typedef {{opId:string, entryIdx:number, dmgType:('physical'|'arts'|'true'), potential:boolean,
 *            moduleId:(string|null), moduleLv:number, multiplier:number, selfPct:number,
 *            hits:number, buffPct:number, dmgMult:number, ignoreDef:number}} RowState
 */

/** カタログからoperatorIdでオペレーターを引く。無ければnull。 */
export function findOperator(catalog, opId) {
  return catalog.operators.find((o) => o.id === opId) ?? null;
}

/** rowが指すFkEntryを引く。範囲外ならnull。 */
export function findEntry(op, entryIdx) {
  if (!op) return null;
  return op.fkEntries[entryIdx] ?? null;
}

/** lv3のatkByLevelが最大のモジュールIDを返す（モジュール無しならnull）。 */
export function defaultModuleId(op) {
  if (!op || !op.modules.length) return null;
  let best = op.modules[0];
  for (const m of op.modules) {
    if ((m.atkByLevel[2] ?? 0) > (best.atkByLevel[2] ?? 0)) best = m;
  }
  return best.id;
}

/** オペレーター+FkEntryから、カタログ値をそのまま初期値にした行を作る。 */
export function makeDefaultRow(op, entryIdx) {
  const entry = op.fkEntries[entryIdx];
  return {
    opId: op.id,
    entryIdx,
    dmgType: entry.damageType.value,
    potential: true,
    moduleId: defaultModuleId(op),
    moduleLv: 3,
    multiplier: entry.multiplier.value,
    selfPct: entry.selfAtkPct.value,
    hits: entry.hits.value,
    buffPct: 0,
    dmgMult: 1,
    ignoreDef: 0,
  };
}

/** row.moduleIdが指すモジュールのLv1〜3配列（見つからなければnull）。 */
export function findModule(op, moduleId) {
  if (!op || !moduleId) return null;
  return op.modules.find((m) => m.id === moduleId) ?? null;
}

/** atk = atkBase + (潜在) + (モジュール)。 */
export function resolveAtk(op, row) {
  const module = findModule(op, row.moduleId);
  const moduleAtk = module ? module.atkByLevel[row.moduleLv - 1] ?? 0 : 0;
  return op.atkBase + (row.potential ? op.atkPotential : 0) + moduleAtk;
}

/**
 * 敵の防御有効値(defEff)。`ignoreDef`は行ごと(そのオペレーターの防御無視特性)なので
 * 引数で受け取る。0未満にはならない。
 */
export function resolveDefEff(enemy, ignoreDef) {
  return Math.max(enemy.def * (1 - enemy.defPct) - enemy.defFlat - ignoreDef, 0);
}

/** 敵の術耐性有効値(resEff)。0〜100にクランプする。 */
export function resolveResEff(enemy) {
  const raw = enemy.res - enemy.resFlat;
  return Math.min(Math.max(raw, 0), 100);
}

/**
 * 1行分のダメージを計算する。`inspireFlat`はP2/P3の鼓舞合計を足すための差し込み口
 * （P1では常に0）。
 * @returns {{atk:number, pct:number, final:number, defEff:number, resEff:number,
 *            perHit:number, atFloor:boolean, rowDamage:number}}
 */
export function computeRowDamage(op, row, enemy, inspireFlat = 0) {
  const atk = resolveAtk(op, row);
  const pct = row.selfPct + row.buffPct;
  const final = (atk * (1 + pct) + inspireFlat) * row.multiplier;
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
 * @returns {{results:(Array<null|{row:RowState, op:object}&ReturnType<typeof computeRowDamage>>),
 *            total:number, killed:boolean}}
 */
export function computeTotal(catalog, rows, enemy) {
  const results = rows.map((row) => {
    const op = findOperator(catalog, row.opId);
    if (!op) return null;
    return { row, op, ...computeRowDamage(op, row, enemy) };
  });
  const total = results.reduce((sum, r) => sum + (r ? r.rowDamage : 0), 0);
  return { results, total, killed: total >= enemy.hp };
}

/**
 * カタログに存在しない opId/entryIdx を指す行を取り除く
 * （ゲームデータ更新でオペレーター/スキルが変わったURL状態を安全に読み込むため）。
 * @returns {{state:object, dropped:number}}
 */
export function dropStaleRows(state, catalog) {
  const opById = new Map(catalog.operators.map((o) => [o.id, o]));
  let dropped = 0;
  const rows = state.rows.filter((row) => {
    const op = opById.get(row.opId);
    const ok = !!op && row.entryIdx >= 0 && row.entryIdx < op.fkEntries.length;
    if (!ok) dropped++;
    return ok;
  });
  return { state: { ...state, rows }, dropped };
}

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
 *       'enemyResFlat'(敵の術耐性-n、術行が1つでも5%floorでなければ。上限は敵の現在の術耐性値)。
 * 上限を超えないと撃破できない場合や、効果が無い（floor済みで伸びしろが無い等）場合は
 * その種類の提案を出さない。全種類が出せなければ`suggest()`は空配列を返す
 * （呼び出し側は「現実的な補正では届きません」のような文言を出す想定）。
 * `effort`（小さいほど「簡単」）昇順でソートする。hits系は+1した値をeffortにする
 * （%やflatの数値と同じ物差しに乗せるための簡単な変換）。
 */
export function suggest(state, catalog) {
  const { rows, enemy } = state;
  const { results, killed } = computeTotal(catalog, rows, enemy);
  if (killed) return [];

  const suggestions = [];
  const killsWith = (testRows, testEnemy) => computeTotal(catalog, testRows, testEnemy ?? enemy).killed;

  results.forEach((r, i) => {
    if (!r) return;

    const buffNeeded = minimalIntegerSatisfying(1, SUGGEST_CAP_BUFF_PCT, (extraPct) =>
      killsWith(rows.map((row, j) => (j === i ? { ...row, buffPct: row.buffPct + extraPct / 100 } : row))),
    );
    if (buffNeeded !== null) {
      suggestions.push({ kind: "rowBuffPct", rowIndex: i, amount: buffNeeded, effort: buffNeeded });
    }

    const hitsNeeded = minimalIntegerSatisfying(1, SUGGEST_CAP_HITS, (extraHits) =>
      killsWith(rows.map((row, j) => (j === i ? { ...row, hits: row.hits + extraHits } : row))),
    );
    if (hitsNeeded !== null) {
      suggestions.push({ kind: "rowHits", rowIndex: i, amount: hitsNeeded, effort: hitsNeeded + 1 });
    }
  });

  // 防御/術耐性の-固定は「敵の現在値を超えて下げる提案はしない」という上限を持つ
  // （0以下にはできない探索区間にする。現在値が0ならそもそも探索しない＝提案を出さない）。
  const hasNonFloorPhysical = results.some((r) => r && r.row.dmgType === "physical" && !r.atFloor);
  const defCap = Math.floor(enemy.def);
  if (hasNonFloorPhysical && defCap > 0) {
    const defNeeded = minimalIntegerSatisfying(1, defCap, (extra) =>
      killsWith(rows, { ...enemy, defFlat: enemy.defFlat + extra }),
    );
    if (defNeeded !== null) {
      suggestions.push({ kind: "enemyDefFlat", amount: defNeeded, effort: defNeeded });
    }
  }

  const hasNonFloorArts = results.some((r) => r && r.row.dmgType === "arts" && !r.atFloor);
  const resCap = Math.floor(enemy.res);
  if (hasNonFloorArts && resCap > 0) {
    const resNeeded = minimalIntegerSatisfying(1, resCap, (extra) =>
      killsWith(rows, { ...enemy, resFlat: enemy.resFlat + extra }),
    );
    if (resNeeded !== null) {
      suggestions.push({ kind: "enemyResFlat", amount: resNeeded, effort: resNeeded });
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
    default:
      return "";
  }
}
