// フレームキル計算機 — カタログの引き方・スキルLv別の値・特殊強化・行の初期値。

import { effectiveElite, effectiveModuleId, maxEliteFor, maxLevelForElite } from "./stats.js";

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
export function defaultModuleId(op, entry) {
  if (!op || !op.modules.length) return null;
  // overrideの`default_module`(例: ホルンはXの方がFK向き)があればそれを優先する。モジュールは
  // オペレーター単位なので、選んだエントリに無ければ同じオペレーターの他エントリの指定も使う
  // (ホルンは最初のエントリがoverride対象外のスキルでも、S2の指定でXを初期値にしたい)。
  const preferred =
    (entry && entry.defaultModule) || ((op.fkEntries || []).find((e) => e.defaultModule) || {}).defaultModule;
  if (preferred && op.modules.some((m) => m.id === preferred)) return preferred;
  let best = op.modules[0];
  for (const m of op.modules) {
    if ((m.atkByLevel[2] ?? 0) > (best.atkByLevel[2] ?? 0)) best = m;
  }
  return best.id;
}

/**
 * P7: 配列`arr`(スキルLv1〜(データ数)ごとの値)から、スキルLv`skillLevel`(1始まり)の値を
 * 引く。`skillLevel`は配列の長さにクランプする(データが10未満のスキルでも安全に動く)。
 * `arr`が空/無ければ0を返す。
 */
export function valueAtLevel(arr, skillLevel) {
  if (!arr || !arr.length) return 0;
  const idx = Math.min(Math.max(skillLevel, 1), arr.length) - 1;
  return arr[idx];
}

/** P7: `entry.multiplierByLevel`から現在のスキルLvの倍率を引く。配列が無ければ`entry.multiplier.value`。 */
export function resolveMultiplierAtLevel(entry, skillLevel) {
  const arr = entry.multiplierByLevel;
  if (!arr || !arr.length) return entry.multiplier.value;
  return valueAtLevel(arr, skillLevel);
}

/** P7: `entry.selfAtkPctByLevel`から現在のスキルLvのセルフ%を引く。配列が無ければ`entry.selfAtkPct.value`。 */
export function resolveSelfAtkPctAtLevel(entry, skillLevel) {
  const arr = entry.selfAtkPctByLevel;
  if (!arr || !arr.length) return entry.selfAtkPct.value;
  return valueAtLevel(arr, skillLevel);
}

/** P10: `entry.parts[i]`(FkPart)から現在のスキルLvの倍率を引く(`resolveMultiplierAtLevel`のパーツ版)。 */
function resolvePartMultiplierAtLevel(part, skillLevel) {
  const arr = part.multiplierByLevel;
  if (!arr || !arr.length) return part.multiplier.value;
  return valueAtLevel(arr, skillLevel);
}

/**
 * P10(混合スキル): `entry.parts[1..]`から、現在のスキルLvでの実効値を持つ
 * `row.extraParts`の既定配列を作る(倍率は`resolvePartMultiplierAtLevel`、
 * ダメージ種別はそのパーツの値そのまま、ダメージ倍率は既定1。Hit数は行で共通なので持たない)。`entryIdx`/`skillLevel`
 * 変更時に`makeDefaultRow`と同じくこれで再スナップする。`entry.parts`が無い/1件以下
 * (=混合スキルではない通常エントリ)なら空配列。
 */
export function makeDefaultExtraParts(entry, skillLevel = 10) {
  const parts = (entry && entry.parts) || [];
  return parts.slice(1).map((p) => ({
    multiplier: resolvePartMultiplierAtLevel(p, skillLevel),
    dmgType: p.damageType.value,
    dmgMult: 1,
  }));
}

/**
 * P10(混合スキル): 行を「パーツごとの疑似行」配列に展開する。row自身が常にパーツ0
 * (multiplier/dmgType/dmgMultはトップレベルのまま)、2番目以降は
 * `row.extraParts[i]`のmultiplier/dmgType/dmgMultだけで上書きした疑似行(それ以外の
 * フィールド=opId/entryIdx/elite/level/trust/potential/moduleId/moduleLv/selfPct/
 * buffPct/ignoreDef/hits/buffIds/specialOn/inspireOn等は行のまま共有する。extraPartsに
 * 余計なフィールド[旧形のhits等]が残っていても拾わない)。`computeTotal`/`suggest`が
 * パーツ単位でダメージを計算する入口。`row.extraParts`が無ければ`[row]`(通常行と完全互換)。
 */
export function expandRowParts(row) {
  const extra = (row && row.extraParts) || [];
  return [row, ...extra.map((p) => ({ ...row, multiplier: p.multiplier, dmgType: p.dmgType, dmgMult: p.dmgMult }))];
}

/**
 * FkEntryが持つ機械/手動の値をそのまま行フィールドへ写した実効値を組み立てる
 * (multiplier/selfPct/hits/dmgType。dmgMultは特殊強化が持たない限り常に1)。
 * `entryIdx`変更時にこれを使い、行フィールドを新しいエントリの値へ再スナップする
 * （ユーザーの手動編集は「別のエントリを選び直した」時点でリセットされる）。
 * P2 follow-upで「置き換え系」特殊強化(FW/Weedy等)を撤去したため、`entry.special`は
 * もう見ない(加算系/乗算系は`resolveSpecialAddPct`/`resolveSpecialMultiplierFactor`が
 * 都度計算する。行フィールドへのスナップショットが不要になった)。
 * P7: `skillLevel`(既定10=特化3。旧デフォルトと同じ結果になる)で倍率/セルフ%を
 * スキルLv別に解決する。`skillLevel`を変える(entryIdxはそのまま)場合もこの関数を
 * 呼び直して行フィールドを再スナップする(倍率入力欄が常に「現在のスキルLvでの
 * カタログ既定値」を表示するようにするため。ユーザーの手動編集はそこから更に上書きできる)。
 */
export function resolveEntryValues(entry, skillLevel = 10) {
  return {
    multiplier: resolveMultiplierAtLevel(entry, skillLevel),
    selfPct: resolveSelfAtkPctAtLevel(entry, skillLevel),
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
export function specialAddCanApply(op, entry, row) {
  if (!entry || !entry.special || !entry.special.requiresModule) return false;
  const sp = entry.special;
  if (effectiveModuleId(op, row) !== sp.requiresModule) return false;
  const arr = sp.addSelfAtkPctByModuleLevel || [];
  return (arr[row.moduleLv - 1] || 0) > 0;
}

/**
 * 特殊強化の「加算系」がセルフ%に加える値。行フィールドへスナップショットせず
 * **都度**計算する(モジュールを変更しても即座に反映される)。加算系を持たない
 * 特殊強化(乗算系のみ等)には常に0を返す。`row.specialOn`がfalseなら常に0。
 */
export function resolveSpecialAddPct(op, entry, row) {
  if (!row.specialOn || !entry || !entry.special || !entry.special.requiresModule) return 0;
  if (!specialAddCanApply(op, entry, row)) return 0;
  const arr = entry.special.addSelfAtkPctByModuleLevel || [];
  return arr[row.moduleLv - 1] || 0;
}

/**
 * P8 follow-up: 素質由来の値テーブル(`ConditionalTalentSource`)から、指定の昇進/
 * (装備可能なら)モジュールLvに対応する「潜在ごとの値配列」(長さ6)を取り出す。
 * モジュールは昇進2でしか装備できないので`elite < 2`の間は`moduleId`を無視する
 * (呼び出し側は既に`effectiveElite`/`effectiveModuleId`で解決済みの値を渡す)。
 * `talent`が無ければ`null`。`resolveConditionalValue`(条件付きバフ)と
 * `mulMultiplier`(特殊強化の乗算系)の両方が同じ「素質値テーブルを引く」ロジックを
 * 使うため、ここに共通化する。
 */
export function talentTableRowFor(talent, elite, moduleId, moduleLevel) {
  if (!talent) return null;
  const module = moduleId && elite >= 2 ? talent.modules.find((m) => m.moduleId === moduleId) : null;
  if (module) return module.valuesByLevelAndPotential[moduleLevel - 1] ?? null;
  return talent.valuesByEliteAndPotential[elite] ?? null;
}

/** `talentTableRowFor`の結果からさらに`potential`(0〜5)の1値を引く。無ければ0。 */
export function resolveTalentTableValue(talent, { elite, potential, moduleId, moduleLevel }) {
  const row = talentTableRowFor(talent, elite, moduleId, moduleLevel);
  return row ? row[potential] ?? 0 : 0;
}

/**
 * 特殊強化の「乗算系」(`entry.special.mulMultiplier`。P8 follow-upで固定`base`/`module`/
 * `byModuleLevel`から素質値テーブル参照へ置き換えた)を、行自身の昇進/潜在/実効モジュール
 * (`effectiveElite`/`effectiveModuleId`。elite/level条件込み)で解決した生の値。値0は
 * 「その昇進/潜在では素質が未解放」を意味する(例: E0)。`row.specialOn`は見ない
 * (`resolveSpecialMultiplierFactor`/`resolveSpecialCurrentValue`/`specialMulCanApply`の
 * 3箇所で共有する)。
 */
function resolveSpecialMulRawValue(op, entry, row) {
  const table = entry.special.mulMultiplier;
  const elite = effectiveElite(op, row);
  const potential = row.potential ?? 5;
  const moduleId = effectiveModuleId(op, row);
  return resolveTalentTableValue(table, { elite, potential, moduleId, moduleLevel: row.moduleLv });
}

/**
 * 特殊強化の「乗算系」が`row.multiplier`に掛ける係数。素質値テーブルから行自身の
 * 昇進/潜在/実効モジュールで解決した値をそのまま使う。値が0(素質未解放)なら
 * 乗算せず1(影響なし)を返す。乗算系を持たない特殊強化には常に1を返す。
 * `row.specialOn`がfalseなら常に1。
 */
export function resolveSpecialMultiplierFactor(op, entry, row) {
  if (!row.specialOn || !entry || !entry.special || !entry.special.mulMultiplier) return 1;
  const value = resolveSpecialMulRawValue(op, entry, row);
  return value > 0 ? value : 1;
}

/**
 * 特殊強化の「乗算系」(素質値テーブル)が、現在の行の昇進/潜在/モジュールの組み合わせで
 * 有効になり得るか(値>0=その昇進/潜在で素質が解放済み)どうか。**`row.specialOn`は
 * 見ない**(`specialAddCanApply`と同じく、UIのチェックボックス/ヒント出し分け専用の
 * 「素質が解放されているか」判定。ONかどうかは別途`resolveSpecialMultiplierFactor`が見る)。
 */
export function specialMulCanApply(op, entry, row) {
  if (!entry || !entry.special || !entry.special.mulMultiplier) return false;
  return resolveSpecialMulRawValue(op, entry, row) > 0;
}

/**
 * UIが特殊強化のチェックボックス/ヒントのどちらを出すべきかを判定する(P2 follow-up。
 * P8 follow-upで乗算系[素質値テーブル]も「適用不可」になり得るようになったため対応)。
 * 加算系(`requiresModule`付き)はモジュール条件を満たさない間、乗算系(`mulMultiplier`)は
 * 素質が未解放の間、それぞれ「適用不可」になり得るためヒントに切り替える。
 * 特殊強化そのものが無い場合は`"none"`(呼び出し側で描画をスキップする)。
 * @returns {"checkbox"|"hint"|"none"}
 */
export function specialUiState(op, entry, row) {
  if (!entry || !entry.special) return "none";
  const sp = entry.special;
  if (sp.requiresModule && !specialAddCanApply(op, entry, row)) return "hint";
  if (sp.mulMultiplier && !specialMulCanApply(op, entry, row)) return "hint";
  return "checkbox";
}

/**
 * 特殊強化のⓘ説明文に付け足す「現在の効果値」(P2 follow-up)。`row.specialOn`に
 * 関わらず、今の昇進/潜在/モジュール/Lv(P6: elite/levelで実際に装備できている場合のみ)
 * なら発動時にどんな値になるかを返す(プレビュー用途)。
 * 加算系(`requiresModule`)は`{kind:"add", value}`、乗算系(`mulMultiplier`)は
 * `{kind:"mul", value}`を返す。特殊強化が無い/どちらの系統も無ければ`null`。
 * 乗算系は素質値テーブルの解決値(0=素質未解放)を、`resolveSpecialMultiplierFactor`と
 * 同じく1(影響なし)へフォールバックしてプレビューする(P8 follow-up)。
 * @returns {null|{kind:"add"|"mul", value:number}}
 */
export function resolveSpecialCurrentValue(op, entry, row) {
  if (!entry || !entry.special) return null;
  const sp = entry.special;
  const moduleId = effectiveModuleId(op, row);
  if (sp.mulMultiplier) {
    const raw = resolveSpecialMulRawValue(op, entry, row);
    return { kind: "mul", value: raw > 0 ? raw : 1 };
  }
  if (sp.requiresModule && sp.addSelfAtkPctByModuleLevel) {
    const idx = moduleId === sp.requiresModule ? row.moduleLv - 1 : -1;
    const value = idx >= 0 ? sp.addSelfAtkPctByModuleLevel[idx] || 0 : 0;
    return { kind: "add", value };
  }
  return null;
}

/**
 * オペレーター+FkEntryから、カタログ値をそのまま初期値にした行を作る。
 * P6: 昇進(elite)はそのオペレーターが到達できる最大値(通常E2。フェーズが少ない
 * オペレーターはその最大)、レベルはその昇進の最大レベル、信頼度は100を既定にする
 * (＝旧atkBase[E2最大Lv+信頼度100]と同じベースATKになる。デフォルト値は変わらない)。
 */
export function makeDefaultRow(op, entryIdx) {
  const entry = op.fkEntries[entryIdx];
  const skillLevel = 10; // P7: 既定は特化3(旧デフォルトと同じ結果になる)。
  const values = resolveEntryValues(entry, skillLevel);
  const elite = maxEliteFor(op);
  return {
    opId: op.id,
    entryIdx,
    dmgType: values.dmgType,
    potential: 5, // P8: 潜在ランク(0始まり。既定は潜在6=フル。旧来の攻撃凸チェックボックスと同じ結果になる)
    elite,
    level: maxLevelForElite(op, elite),
    trust: 100,
    skillLevel, // P7: スキルLv(1〜10。SLv1〜7+特化1〜3)
    moduleId: defaultModuleId(op, entry),
    moduleLv: 3,
    multiplier: values.multiplier,
    selfPct: values.selfPct,
    hits: values.hits,
    buffPct: 0,
    buffFlat: 0, // 手入力 基礎攻撃力+(%バフの前に足す)
    dmgMult: values.dmgMult,
    ignoreDef: 0,
    buffIds: [], // P2: 個別バフ(行ごとに選ぶ)
    specialOn: true, // P2: 特殊強化トグル(デフォルトON。entry.specialが無ければ意味を持たない)
    inspireOn: true, // P3: 鼓舞トグル(デフォルトON。鼓舞ソースが無ければ意味を持たない)
    extraParts: makeDefaultExtraParts(entry, skillLevel), // P10: 混合スキルのパーツ1以降(通常エントリは空)
  };
}
