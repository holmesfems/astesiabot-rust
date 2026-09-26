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
     pct    = selfPct + buffPct + extraPct + specialAddPct （加算。extraPctはP2で追加した
              個別バフ(row.buffIds)+条件付きバフ(state.globalBuffIds)のΣ、
              specialAddPctはP2 follow-upで追加した特殊強化(加算系)のΣ。
              `computeBuffBreakdown`/`resolveSpecialAddPct`が計算し、
              `computeTotal`経由で流し込む）
     final  = (atk × (1 + pct) + inspireFlat) × multiplier × specialMulFactor
              （inspireFlatはP1では常に0。P2でflat種のバフ(Σ鼓舞)をここに足す。
              specialMulFactorはP2 follow-upで追加した特殊強化(乗算系)の係数。
              無ければ1＝影響なし。`resolveSpecialMultiplierFactor`が計算する）
     defEff = max(def × (1 − defPct) − defFlat − ignoreDef, 0)
     resEff = clamp(res − resFlat, 0, 100)
     物理: perHit = max(final − defEff, final × 0.05)
     術  : perHit = max(final × (1 − resEff/100), final × 0.05)
     真  : perHit = final
     rowDamage = perHit × dmgMult × (1 + vulnPct) × hits
     total = Σ rowDamage; killed = total >= hp

   P2で追加したバフの適用ルール（`computeBuffBreakdown`）:
     - 個別バフ(catalog.buffers中`scope.type==="individual"`): row.buffIdsに
       含まれるものを無条件に合算する(タグ判定なし。行ごとにユーザーが選ぶため)。
     - 条件付きバフ(`scope.type==="conditional"`): state.globalBuffIdsでON中の
       ものだけ対象。`scope.targetTags`とエントリの`entry.tags`が1つでも
       重なれば適用。`bonus`(省略可)がある場合、`bonus.targetTags`とも
       重なっていれば基本値の代わりに`bonus.value`を採用する(置き換え。加算ではない。
       例: 異格エクシアは弾薬スキル+13%だが、ラテラーノ勢は26%に置き換わる)。
     - どちらも`kind`(pct/flat)ごとに合算し、pct分はextraPctへ、flat分は
       extraFlatへ(inspireFlatとして)反映する。

   P2 follow-upで追加した「特殊強化」の2系統(`entry.special`。詳細は各関数のコメント参照)。
   どちらも`row.specialOn`が有効かつモジュール条件を満たす時だけ効く。置き換え系
   (旧仕様。行フィールドをスナップショットで上書き)はFW/Weedy実データ精査の結果
   不要と判明したため撤去済み(`resolveEntryValues`はもうentry.specialを見ない):
     - 加算系(`requires_module`+`addSelfAtkPctByModuleLevel`): セルフ%に加算
       (`resolveSpecialAddPct`)。例: ブレイズ/ウィーディ
     - 乗算系(`mulMultiplier`): `multiplier`に乗算する係数(`resolveSpecialMultiplierFactor`)。
       モジュール未装備でも`mulMultiplier.base`が常に効く(「常に適用可能」＝UIは
       チェックボックスを隠さない)。例: ファイヤーウォッチ
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

/**
 * FkEntryが持つ機械/手動の値をそのまま行フィールドへ写した実効値を組み立てる
 * (multiplier/selfPct/hits/dmgType。dmgMultは特殊強化が持たない限り常に1)。
 * `entryIdx`変更時にこれを使い、行フィールドを新しいエントリの値へ再スナップする
 * （ユーザーの手動編集は「別のエントリを選び直した」時点でリセットされる）。
 * P2 follow-upで「置き換え系」特殊強化(FW/Weedy等)を撤去したため、`entry.special`は
 * もう見ない(加算系/乗算系は`resolveSpecialAddPct`/`resolveSpecialMultiplierFactor`が
 * 都度計算する。行フィールドへのスナップショットが不要になった)。
 */
export function resolveEntryValues(entry) {
  return {
    multiplier: entry.multiplier.value,
    selfPct: entry.selfAtkPct.value,
    hits: entry.hits.value,
    dmgType: entry.damageType.value,
    dmgMult: 1,
  };
}

/**
 * 特殊強化の「加算系」(`entry.special.requiresModule` + `addSelfAtkPctByModuleLevel`)が、
 * 現在の行のモジュール/Lvの組み合わせで有効になり得るかどうか。**`row.specialOn`は
 * 見ない**(UIがチェックボックス/ヒントの出し分けに使うための「モジュール条件だけ」の
 * 判定。ONかどうかは別途`resolveSpecialAddPct`が見る)。この関数は加算系
 * (`requiresModule`が付いている特殊強化)専用で、乗算系(`mulMultiplier`。モジュール
 * 無しでも`base`が常に効くため「適用不可」という状態が無い)には使わない
 * (呼び出し側=`specialUiState`が`requiresModule`の有無で分岐する)。
 */
export function specialAddCanApply(entry, row) {
  if (!entry || !entry.special || !entry.special.requiresModule) return false;
  const sp = entry.special;
  if (row.moduleId !== sp.requiresModule) return false;
  const arr = sp.addSelfAtkPctByModuleLevel || [];
  return (arr[row.moduleLv - 1] || 0) > 0;
}

/**
 * 特殊強化の「加算系」がセルフ%に加える値。行フィールドへスナップショットせず
 * **都度**計算する(モジュールを変更しても即座に反映される)。加算系を持たない
 * 特殊強化(乗算系のみ等)には常に0を返す。`row.specialOn`がfalseなら常に0。
 */
export function resolveSpecialAddPct(entry, row) {
  if (!row.specialOn || !entry || !entry.special || !entry.special.requiresModule) return 0;
  if (!specialAddCanApply(entry, row)) return 0;
  const arr = entry.special.addSelfAtkPctByModuleLevel || [];
  return arr[row.moduleLv - 1] || 0;
}

/**
 * 特殊強化の「乗算系」(`entry.special.mulMultiplier`)が`row.multiplier`に掛ける係数。
 * `mulMultiplier.module`を`row.moduleId`がそのLvで装備していれば`byModuleLevel`の
 * 対応要素、そうでなければ常に`base`を使う(モジュール未装備でも`base`は必ず効く＝
 * 「適用不可」という状態が無い)。乗算系を持たない特殊強化には常に1(影響なし)を返す。
 * `row.specialOn`がfalseなら常に1。
 */
export function resolveSpecialMultiplierFactor(entry, row) {
  if (!row.specialOn || !entry || !entry.special || !entry.special.mulMultiplier) return 1;
  const mm = entry.special.mulMultiplier;
  if (mm.module && row.moduleId === mm.module) {
    const arr = mm.byModuleLevel || [];
    const v = arr[row.moduleLv - 1];
    if (v != null) return v;
  }
  return mm.base;
}

/**
 * UIが特殊強化のチェックボックス/ヒントのどちらを出すべきかを判定する(P2 follow-up)。
 * 加算系(`requiresModule`付き)はモジュール条件を満たさない間「適用不可」になり得るため
 * ヒントに切り替える。乗算系(`mulMultiplier`。モジュール無しでも`base`が常に効く)や
 * 特殊強化そのものが無い場合は常にチェックボックス側(「無い」場合は呼び出し側で
 * `entry.special`自体の有無を見て描画をスキップする)。
 * @returns {"checkbox"|"hint"|"none"}
 */
export function specialUiState(entry, row) {
  if (!entry || !entry.special) return "none";
  if (entry.special.requiresModule) {
    return specialAddCanApply(entry, row) ? "checkbox" : "hint";
  }
  return "checkbox";
}

/**
 * 特殊強化のⓘ説明文に付け足す「現在の効果値」(P2 follow-up)。`row.specialOn`に
 * 関わらず、今のモジュール/Lvなら発動時にどんな値になるかを返す(プレビュー用途)。
 * 加算系(`requiresModule`)は`{kind:"add", value}`、乗算系(`mulMultiplier`)は
 * `{kind:"mul", value}`を返す。特殊強化が無い/どちらの系統も無ければ`null`。
 * @returns {null|{kind:"add"|"mul", value:number}}
 */
export function resolveSpecialCurrentValue(entry, row) {
  if (!entry || !entry.special) return null;
  const sp = entry.special;
  if (sp.mulMultiplier) {
    const mm = sp.mulMultiplier;
    let value = mm.base;
    if (mm.module && row.moduleId === mm.module) {
      const arr = mm.byModuleLevel || [];
      const v = arr[row.moduleLv - 1];
      if (v != null) value = v;
    }
    return { kind: "mul", value };
  }
  if (sp.requiresModule && sp.addSelfAtkPctByModuleLevel) {
    const idx = row.moduleId === sp.requiresModule ? row.moduleLv - 1 : -1;
    const value = idx >= 0 ? sp.addSelfAtkPctByModuleLevel[idx] || 0 : 0;
    return { kind: "add", value };
  }
  return null;
}

/** オペレーター+FkEntryから、カタログ値をそのまま初期値にした行を作る。 */
export function makeDefaultRow(op, entryIdx) {
  const entry = op.fkEntries[entryIdx];
  const values = resolveEntryValues(entry);
  return {
    opId: op.id,
    entryIdx,
    dmgType: values.dmgType,
    potential: true,
    moduleId: defaultModuleId(op),
    moduleLv: 3,
    multiplier: values.multiplier,
    selfPct: values.selfPct,
    hits: values.hits,
    buffPct: 0,
    dmgMult: values.dmgMult,
    ignoreDef: 0,
    buffIds: [], // P2: 個別バフ(行ごとに選ぶ)
    specialOn: true, // P2: 特殊強化トグル(デフォルトON。entry.specialが無ければ意味を持たない)
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
 * P2のバフ内訳を計算する。個別バフ(row.buffIds)は無条件に合算、条件付きバフ
 * (globalBuffIds)はentry.tagsとの重なりで判定する（詳細はファイル冒頭コメント参照）。
 * `entry`が無い(未選択行等)場合はタグ判定ができないため条件付きバフは全て不適用扱いになる
 * （個別バフはタグ判定不要なのでentryが無くても合算する）。
 * @returns {{individualPct:number, individualFlat:number, conditionalPct:number,
 *            conditionalFlat:number, extraPct:number, extraFlat:number,
 *            appliedConditional:Array<{id:string,name:string,value:number,kind:string,bonusApplied:boolean}>,
 *            notAppliedConditional:Array<{id:string,name:string}>}}
 */
export function computeBuffBreakdown(row, entry, catalog, globalBuffIds = []) {
  const buffers = (catalog && catalog.buffers) || [];
  const byId = new Map(buffers.map((b) => [b.id, b]));
  const entryTags = entry && entry.tags ? entry.tags : [];

  let individualPct = 0,
    individualFlat = 0;
  for (const id of row.buffIds || []) {
    const b = byId.get(id);
    if (!b || b.scope.type !== "individual") continue;
    if (b.kind === "pct") individualPct += b.value;
    else individualFlat += b.value;
  }

  let conditionalPct = 0,
    conditionalFlat = 0;
  const appliedConditional = [];
  const notAppliedConditional = [];
  for (const id of globalBuffIds) {
    const b = byId.get(id);
    if (!b || b.scope.type !== "conditional") continue;
    const matches = b.scope.targetTags.some((t) => entryTags.includes(t));
    if (!matches) {
      notAppliedConditional.push({ id, name: b.name });
      continue;
    }
    const bonusMatches = !!(b.bonus && b.bonus.targetTags.some((t) => entryTags.includes(t)));
    const value = bonusMatches ? b.bonus.value : b.value;
    if (b.kind === "pct") conditionalPct += value;
    else conditionalFlat += value;
    appliedConditional.push({ id, name: b.name, value, kind: b.kind, bonusApplied: bonusMatches });
  }

  return {
    individualPct,
    individualFlat,
    conditionalPct,
    conditionalFlat,
    extraPct: individualPct + conditionalPct,
    extraFlat: individualFlat + conditionalFlat,
    appliedConditional,
    notAppliedConditional,
  };
}

/**
 * `single_target`(単体対象)バフが2行以上で選ばれているかを検出する。
 * UIが⚠警告を出すためのデータ(「本当に両方に乗るのか？」の注意喚起。計算自体は
 * 単純合算のままで、警告を出すだけに留める)。
 * @returns {Set<string>} 複数行で選ばれているバフidの集合
 */
export function findSingleTargetConflicts(catalog, rows) {
  const buffers = (catalog && catalog.buffers) || [];
  const singleIds = new Set(buffers.filter((b) => b.singleTarget).map((b) => b.id));
  const countById = new Map();
  for (const row of rows) {
    for (const id of row.buffIds || []) {
      if (!singleIds.has(id)) continue;
      countById.set(id, (countById.get(id) || 0) + 1);
    }
  }
  const conflicts = new Set();
  for (const [id, count] of countById) if (count > 1) conflicts.add(id);
  return conflicts;
}

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
  const final = (atk * (1 + pct) + inspireFlat) * row.multiplier * multiplierFactor;
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
 * @returns {{results:(Array<null|{row:RowState, op:object, breakdown:object}&ReturnType<typeof computeRowDamage>>),
 *            total:number, killed:boolean}}
 */
export function computeTotal(catalog, rows, enemy, globalBuffIds = []) {
  const results = rows.map((row) => {
    const op = findOperator(catalog, row.opId);
    if (!op) return null;
    const entry = findEntry(op, row.entryIdx);
    const breakdown = computeBuffBreakdown(row, entry, catalog, globalBuffIds);
    const specialAddPct = resolveSpecialAddPct(entry, row);
    const specialMulFactor = resolveSpecialMultiplierFactor(entry, row);
    const dmg = computeRowDamage(op, row, enemy, breakdown.extraFlat, breakdown.extraPct + specialAddPct, specialMulFactor);
    return { row, op, breakdown, specialAddPct, specialMulFactor, ...dmg };
  });
  const total = results.reduce((sum, r) => sum + (r ? r.rowDamage : 0), 0);
  return { results, total, killed: total >= enemy.hp };
}

/**
 * カタログに存在しない opId/entryIdx を指す行を取り除く
 * （ゲームデータ更新でオペレーター/スキルが変わったURL状態を安全に読み込むため）。
 * ついでに(P2) カタログに存在しないバフidを`row.buffIds`/`state.globalBuffIds`から
 * 静かに(トースト無し)取り除き、`specialOn`の省略時デフォルト(true)も補う
 * （古い形(P1)のstate/共有URLにはこれらのフィールドが無いため、そのまま読めるようにする）。
 * @returns {{state:object, dropped:number}}
 */
export function dropStaleRows(state, catalog) {
  const opById = new Map(catalog.operators.map((o) => [o.id, o]));
  const validBuffIds = new Set((catalog.buffers || []).map((b) => b.id));
  let dropped = 0;
  const rows = state.rows
    .filter((row) => {
      const op = opById.get(row.opId);
      const ok = !!op && row.entryIdx >= 0 && row.entryIdx < op.fkEntries.length;
      if (!ok) dropped++;
      return ok;
    })
    .map((row) => ({
      ...row,
      buffIds: (row.buffIds || []).filter((id) => validBuffIds.has(id)),
      specialOn: row.specialOn !== false,
    }));
  const globalBuffIds = (state.globalBuffIds || []).filter((id) => validBuffIds.has(id));
  return { state: { ...state, rows, globalBuffIds }, dropped };
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
  const { results, killed } = computeTotal(catalog, rows, enemy, globalBuffIds);
  if (killed) return [];

  const suggestions = [];
  const killsWith = (testRows, testEnemy, testGlobalBuffIds) =>
    computeTotal(catalog, testRows, testEnemy ?? enemy, testGlobalBuffIds ?? globalBuffIds).killed;

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
    default:
      return "";
  }
}
