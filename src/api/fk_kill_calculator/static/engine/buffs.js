// フレームキル計算機 — バフの内訳・潜在の選択肢・FK行とのリンク・単体バフの競合判定。

import { findOperator, resolveTalentTableValue, talentTableRowFor } from "./entry.js";
import { effectiveElite, effectiveModuleId } from "./stats.js";

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
/** 条件付きバフの`targets`で「味方全員」を表す特別なタグ(Rust側`tags.rs::ALL_TAG`と同じ)。 */
export const ALL_TAG = "全員";

/**
 * `b.source`(P4。素質由来 or スキルLv由来)を持つ条件付きバフの値を、選択中の軸
 * (`levels`。省略/未選択の軸は`source.defaults`にフォールバック)で解決する。
 * `b.source`が無い(固定`pct`/`flat`)バフはそのまま`b.value`を返す。
 * `b.toggle`(省略可)がONなら最後に`toggle.mult`を掛ける。
 * @param {object} b カタログの`Buffer`(dto.rs参照)。
 * @param {{elite?:number, potential?:number, moduleId?:(string|null), moduleLevel?:number,
 *           skillLevel?:number, toggleOn?:boolean}} levels
 */
// 潜在(0〜5=潜在1〜6)を「値が変わる境目」でまとめる汎用ヘルパー(P4で素質ソース向けに
// 追加、P8で行/鼓舞ソースのATK潜在・鼓舞ソースの素質凸境目にも使えるよう汎用化した)。
// `tables`は各要素が`table[p]`(潜在p=0〜5の値)を持つ配列(長さ6)の配列で、
// どれか1つでも値の組が変われば境目にする(例: 素質×モジュールの全テーブルで揃って
// 同じ潜在はまとめる。エイヤは潜在1-5/潜在6)。
// 戻り値: [{ from, to }](0-indexed、昇順)。
export function potentialGroups(tables) {
  const keyOf = (p) => tables.map((t) => t[p] ?? 0).join(",");
  const groups = [];
  for (let p = 0; p < 6; p++) {
    const last = groups[groups.length - 1];
    if (last && keyOf(last.to) === keyOf(p)) last.to = p;
    else groups.push({ from: p, to: p });
  }
  return groups;
}

// P8: 素質ソース(talent)向けの全テーブル(昇進×潜在 + 各モジュールのLv×潜在)を集めて
// potentialGroupsに渡す。旧`potentialGroups(talent)`呼び出しの置き換え。
export function talentPotentialGroups(talent) {
  const tables = [
    ...((talent && talent.valuesByEliteAndPotential) || []),
    ...((talent && talent.modules) || []).flatMap((m) => m.valuesByLevelAndPotential || []),
  ];
  return potentialGroups(tables);
}

// P8: 行/鼓舞ソースのATK潜在(単一テーブル)を「値が変わる境目」でグルーピングする。
// `op.atkPotentialByRank`が無い(簡易オブジェクト)場合は境目無し(潜在1-6の1グループ)扱い。
// P8 follow-up: `entry`/`row`を渡すと、`entry.special.mulMultiplier`(乗算系特殊強化の
// 素質値テーブル)がある場合にATK潜在テーブルと合成する(境目の和集合)。テーブルは行自身の
// 現在の昇進/実効モジュール(`effectiveElite`/`effectiveModuleId`)に対応する「潜在ごとの
// 値配列」を使う(`talentTableRowFor`。elite/moduleIdを変えれば選ぶ配列も変わるが、
// この関数自体はrenderの度に呼ばれるので都度追従する)。
// 例: ファイヤーウォッチはATK潜在境目(潜在1-3/4-6)と素質「暗殺者」の境目(潜在1-4/5-6)の
// 和で「潜在1-3/潜在4/潜在5-6」の3択になる。
export function atkPotentialGroups(op, entry, row) {
  const tables = [];
  if (op && op.atkPotentialByRank) tables.push(op.atkPotentialByRank);
  const mulTable = entry && entry.special && entry.special.mulMultiplier;
  if (mulTable && row) {
    const elite = effectiveElite(op, row);
    const moduleId = effectiveModuleId(op, row);
    const rowVec = talentTableRowFor(mulTable, elite, moduleId, row.moduleLv);
    if (rowVec) tables.push(rowVec);
  }
  if (!tables.length) return [{ from: 0, to: 5 }];
  return potentialGroups(tables);
}

// P8: 鼓舞ソースの潜在セレクトの境目。ATK加算の変化点 ∪ 素質凸(pctPotentialBonus)が
// 解放される境目(`source.talentPotentialRank`。0始まり)。素質凸境目は「そのrank以上なら1、
// 未満なら0」という仮想テーブルとしてpotentialGroupsに一緒に渡すことで、ATK/素質凸どちらの
// 境目でも自然にグループが分かれる(例: 濁心スカジは潜在1-3/潜在4/潜在5-6)。
export function inspirePotentialGroups(source) {
  const tables = [(source && source.atkPotentialByRank) || [0, 0, 0, 0, 0, 0]];
  const rank = source && source.talentPotentialRank;
  if (rank != null) tables.push([0, 1, 2, 3, 4, 5].map((p) => (p >= rank ? 1 : 0)));
  return potentialGroups(tables);
}

export function resolveConditionalValue(b, levels = {}) {
  if (!b || !b.source) return b ? b.value : 0;
  const source = b.source;
  const defaults = source.defaults || {};
  let base;
  if (source.stage) {
    // P5: 離散段階ソース(スキルLvではなく段階で値が変わる。例: ナスティS3)。
    const stageIndex = levels.stageIndex ?? defaults.stageIndex ?? 1;
    const idx = Math.min(Math.max(stageIndex, 1), source.stage.values.length) - 1;
    base = source.stage.values[idx] ?? 0;
  } else if (!source.talent && source.basePct == null && source.skill) {
    // 純粋なスキルソース(値そのもの。talent/base_pctが無ければscaleと組み合わせる余地も無い)。
    const skillLevel = levels.skillLevel ?? defaults.skillLevel ?? 1;
    const idx = Math.min(Math.max(skillLevel, 1), source.skill.valuesByLevel.length) - 1;
    base = source.skill.valuesByLevel[idx] ?? 0;
  } else {
    // P5: talent または base_pct を主軸とし、scale(あれば)を掛け合わせる。
    let primary;
    if (source.talent) {
      const elite = levels.elite ?? defaults.elite ?? 2;
      const potential = levels.potential ?? defaults.potential ?? 5;
      const moduleId = levels.moduleId !== undefined ? levels.moduleId : (defaults.moduleId ?? null);
      const moduleLevel = levels.moduleLevel ?? defaults.moduleLevel ?? 3;
      // P8 follow-up: 素質値テーブルの解決自体は特殊強化の乗算系(mulMultiplier)と
      // 共通の`resolveTalentTableValue`に切り出した(モジュールは昇進2でしか装備
      // できないのでE0/E1ではモジュール選択を無視する、という判定も含めて同じロジック)。
      primary = resolveTalentTableValue(source.talent, { elite, potential, moduleId, moduleLevel });
    } else {
      primary = source.basePct ?? 0;
    }
    let scaleFactor = 1;
    if (source.scale) {
      const skillLevel = levels.skillLevel ?? defaults.skillLevel ?? 1;
      const idx = Math.min(Math.max(skillLevel, 1), source.scale.valuesByLevel.length) - 1;
      scaleFactor = source.scale.valuesByLevel[idx] ?? 1;
    }
    base = primary * scaleFactor;
  }
  if (b.toggle) {
    const toggleOn = levels.toggleOn ?? false;
    if (toggleOn) base *= b.toggle.mult;
  }
  return base;
}

export function computeBuffBreakdown(row, entry, catalog, globalBuffIds = [], globalBuffLevels = {}) {
  const buffers = (catalog && catalog.buffers) || [];
  const byId = new Map(buffers.map((b) => [b.id, b]));
  const entryTags = entry && entry.tags ? entry.tags : [];

  let individualPct = 0,
    individualFlat = 0;
  for (const id of row.buffIds || []) {
    const b = byId.get(id);
    if (!b || b.scope.type !== "individual") continue;
    // P5: sourceを持つ個別バフはglobalBuffLevels[id](育成設定)で解決した値を使う
    // (conditionalと同じ`resolveConditionalValue`をそのまま再利用する)。
    const value = b.source ? resolveConditionalValue(b, (globalBuffLevels && globalBuffLevels[id]) || {}) : b.value;
    if (b.kind === "pct") individualPct += value;
    else individualFlat += value;
  }

  let conditionalPct = 0,
    conditionalFlat = 0;
  const appliedConditional = [];
  const notAppliedConditional = [];
  // exclusiveGroupが同じ条件付きバフは同時に効かないので、グループごとに最大値の1件だけ残す
  // (UIは片方をONにするともう片方をOFFにするが、共有URL等で両方ONの状態が来ても二重に乗せない)。
  const bestInGroup = new Map();
  for (const id of globalBuffIds) {
    const b = byId.get(id);
    if (!b || b.scope.type !== "conditional" || !b.exclusiveGroup) continue;
    const cur = bestInGroup.get(b.exclusiveGroup);
    if (!cur || b.value > cur.value) bestInGroup.set(b.exclusiveGroup, b);
  }
  for (const id of globalBuffIds) {
    const b = byId.get(id);
    if (!b || b.scope.type !== "conditional") continue;
    if (b.exclusiveGroup && bestInGroup.get(b.exclusiveGroup) !== b) continue;
    // "全員"は特別なタグで、どのエントリにも一致する(エントリ側のtagsには載らない)。
    const matches = b.scope.targetTags.some((t) => t === ALL_TAG || entryTags.includes(t));
    if (!matches) {
      notAppliedConditional.push({ id, name: b.name });
      continue;
    }
    const levels = (globalBuffLevels && globalBuffLevels[id]) || {};
    const baseValue = resolveConditionalValue(b, levels);
    const bonusMatches = !!(b.bonus && b.bonus.targetTags.some((t) => entryTags.includes(t)));
    let value;
    if (bonusMatches) {
      value = b.bonus.mult != null ? baseValue * b.bonus.mult : b.bonus.value;
    } else {
      value = baseValue;
    }
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
 * P9: 「FK行との育成設定リンク」。バフの`source.operatorId`と一致するFK行
 * (`state.rows`内で最初に見つかったもの)を返す。無ければ`null`。
 * `effectiveBuffLevels`と、UIの「FK行（<name>）の設定を使用」表示の両方から使う
 * 共通ロジック(計算と表示を同じ判定基準に揃えるため)。
 */
export function findLinkedRow(buffer, state) {
  if (!buffer || !buffer.source) return null;
  const opId = buffer.source.operatorId;
  const rows = (state && state.rows) || [];
  return rows.find((r) => r.opId === opId) ?? null;
}

/**
 * P9: バフ1件分の軸選択(`resolveConditionalValue`の`levels`引数)を組み立てる。
 * 通常は`state.globalBuffLevels[buffer.id]`(未設定分は`source.defaults`)をそのまま使うが、
 * このバフの`source.operatorId`と同じオペレーターのFK行があれば、
 * 昇進(elite)/潜在(potential)/モジュール(moduleId・moduleLevel)/スキルLv(skillLevel)は
 * その行の設定を優先する(「行の育成状況＝バフの育成状況」という前提。例: ホルンの
 * 「軍事要塞」はホルン自身の昇進/潜在/モジュールで値が決まる)。トグル(`toggleOn`。
 * 例: 前衛アーミヤの「スキル中は効果2倍」)は行に対応する概念が無いためユーザー操作のまま
 * (`state.globalBuffLevels`から読む)。
 * `talent`/`skill`/`scale`のいずれも持たないソース(段階のみ。例: ナスティS3)は
 * リンクしても差し替わる軸が無いため対象外にする(UIの「FK行の設定を使用」表示も
 * 同じ判定を使うため、`renderBuffAxisControls`から呼ばれるui.js側のヘルパーも
 * この関数と同じ条件[`src.talent || src.skill || src.scale`]を使うこと)。
 * `computeBuffBreakdown`(行/鼓舞ソース双方)・`findSingleTargetConflicts`・`suggest`・
 * UIのバフカードなど、バフの値を解決する箇所は全てこの関数(または
 * `computeEffectiveGlobalBuffLevels`が返すmap)経由で軸選択を得ること。
 */
export function effectiveBuffLevels(buffer, state, catalog) {
  const d = (buffer.source && buffer.source.defaults) || {};
  const stored = (state && state.globalBuffLevels && state.globalBuffLevels[buffer.id]) || {};
  const base = {
    elite: d.elite ?? 2,
    potential: d.potential ?? 5,
    moduleId: d.moduleId ?? null,
    moduleLevel: d.moduleLevel ?? 3,
    skillLevel: d.skillLevel ?? 1,
    stageIndex: d.stageIndex ?? 1,
    toggleOn: false,
    ...stored,
  };
  const src = buffer.source;
  if (!src || !(src.talent || src.skill || src.scale)) return base;
  const row = findLinkedRow(buffer, state);
  if (!row) return base;
  const op = findOperator(catalog, row.opId);
  return {
    ...base,
    elite: row.elite,
    potential: row.potential,
    moduleId: effectiveModuleId(op, row),
    moduleLevel: row.moduleLv,
    skillLevel: row.skillLevel,
  };
}

/**
 * P9: カタログの全バフに`effectiveBuffLevels`を適用したbuffId→軸選択のmap。
 * `computeTotal`/`findSingleTargetConflicts`/`suggest`/`computeInspireSource`に渡す
 * `globalBuffLevels`はこの解決済みmapを使う(呼び出し側がFK行リンクを個別に意識せず
 * 済むようにするための唯一の差し込み口。ui.jsの`buffLevelsState()`もこれをそのまま使う)。
 */
export function computeEffectiveGlobalBuffLevels(catalog, state) {
  const buffers = (catalog && catalog.buffers) || [];
  const out = {};
  for (const b of buffers) out[b.id] = effectiveBuffLevels(b, state, catalog);
  return out;
}

/**
 * P9: 行のオペレーターが確定した瞬間(`opId`が今のidになった瞬間)、その
 * オペレーターを`source.operatorId`に持つ条件付きバフを一度だけ自動でONにする
 * (例: ホルンを行に追加すると「軍事要塞」がON。異格エクシア等の他のバフ付き
 * オペレーターでも同様に働く。意図的な汎用挙動)。既にONのidは変えない(冪等)。
 * 「一度だけ」という制約自体はこの関数の責務ではなく呼び出し側(ui.jsの
 * `onOperatorNameChange`。行のオペレーターを選び直した時だけ呼ぶ)が担う
 * ‐ ユーザーが手動でOFFにした後、行の他フィールドを変更してもこの関数は
 * 呼ばれないため、OFFのままになる。
 * @param {object} catalog
 * @param {string[]} globalBuffIds 現在ONの条件付きバフid一覧
 * @param {string} opId 今確定した行のオペレーターid
 * @returns {string[]} 新しいglobalBuffIds(既存の順序を保ち、該当分を末尾に追加する)
 */
export function autoEnableSourcedBuffs(catalog, globalBuffIds, opId) {
  const buffers = (catalog && catalog.buffers) || [];
  const set = new Set(globalBuffIds || []);
  for (const b of buffers) {
    if (b.scope.type !== "conditional" || !b.source || b.source.operatorId !== opId) continue;
    set.add(b.id);
  }
  return Array.from(set);
}

/**
 * `single_target`(単体対象)バフが許容数を超えて選ばれているかを検出する。
 * UIが⚠警告を出すためのデータ(「本当に両方に乗るのか？」の注意喚起。計算自体は
 * 単純合算のままで、警告を出すだけに留める)。P3: 鼓舞ソース(`inspireSourceStates`。
 * ON中のソースの`buffIds`のみを数える。OFF中は効果自体が無いため対象外)側の選択も
 * 行と同じ名前空間で数える(例: エクシアを行とソース両方で選ぶと両方に⚠が出る)。
 * 許容数は通常1だが、P5の`b.source.maxTargetsByModule`(例: エクシア。モジュールX Lv2以上
 * 装備時は2名まで許容)が現在の`globalBuffLevels`選択で条件を満たす間は`count`に緩和する。
 * @returns {Set<string>} 許容数を超えて選ばれているバフidの集合
 */
export function findSingleTargetConflicts(catalog, rows, inspireSourceStates = {}, globalBuffLevels = {}) {
  const buffers = (catalog && catalog.buffers) || [];
  const singleBuffs = buffers.filter((b) => b.singleTarget);
  const limitFor = (b) => {
    const mt = b.source && b.source.maxTargetsByModule;
    if (!mt) return 1;
    const defaults = (b.source && b.source.defaults) || {};
    const levels = (globalBuffLevels && globalBuffLevels[b.id]) || {};
    const moduleId = levels.moduleId !== undefined ? levels.moduleId : (defaults.moduleId ?? null);
    const moduleLevel = levels.moduleLevel ?? defaults.moduleLevel ?? 3;
    return moduleId === mt.moduleId && moduleLevel >= mt.minLevel ? mt.count : 1;
  };
  const limitById = new Map(singleBuffs.map((b) => [b.id, limitFor(b)]));
  const countById = new Map();
  const count = (ids) => {
    for (const id of ids || []) {
      if (!limitById.has(id)) continue;
      countById.set(id, (countById.get(id) || 0) + 1);
    }
  };
  for (const row of rows) count(row.buffIds);
  for (const cfg of Object.values(inspireSourceStates || {})) {
    if (!cfg || !cfg.on) continue;
    count(cfg.buffIds);
  }
  const conflicts = new Set();
  for (const [id, n] of countById) if (n > (limitById.get(id) ?? 1)) conflicts.add(id);
  return conflicts;
}
