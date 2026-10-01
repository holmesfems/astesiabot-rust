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
     final  = ((atk + buffFlat) × (1 + pct) + inspireFlat) × multiplier × specialMulFactor
              （buffFlat=手入力の基礎攻撃力加算。%バフの前に足す。行自身のダメージにだけ効き、
              鼓舞ソースのATKには乗らない）
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
       重なれば適用。基本値は`b.value`(固定)または`resolveConditionalValue(b, levels)`
       (P4。`b.source`付きバフはゲームデータから機械抽出した値を選択中の昇進/潜在/
       モジュール/スキルLvで解決する。詳細は同関数のコメント参照)。`bonus`(省略可)が
       ある場合、`bonus.targetTags`とも重なっていれば基本値の代わりに
       `bonus.value`(固定)または`基本値×bonus.mult`(P4)を採用する(置き換え。加算ではない。
       例: 異格エクシアは弾薬スキル+13%だが、ラテラーノ勢は基本値の2倍(26%)に置き換わる)。
     - どちらも`kind`(pct/flat)ごとに合算し、pct分はextraPctへ、flat分は
       extraFlatへ(inspireFlatとして)反映する。

   P4で追加した「条件付きバフの動的値解決」(`resolveConditionalValue`)。
   `b.source`(素質由来 or スキルLv由来)を持つ条件付きバフは、固定`b.value`の代わりに
   `state.globalBuffLevels[b.id]`(昇進/潜在/モジュール/モジュールLv/スキルLv/トグルON)の
   選択に応じて値を都度計算する。素質由来はモジュール選択があれば`source.talent.modules`
   から該当モジュールのLv別テーブルを、無ければ`source.talent.valuesByEliteAndPotential`
   (昇進×潜在の表)を引く。スキル由来は`source.skill.valuesByLevel`(スキルLv1〜10)を
   そのまま引く。`b.toggle`(省略可)がONなら最後に`toggle.mult`を掛ける
   (例: 前衛アーミヤの「スキル中は効果2倍」)。値テーブル自体はRust側
   (`engine::fk_kill_calc::conditional_source`)が事前に解決済みなので、JS側は
   テーブル引きと軸のフォールバック(未選択時は`source.defaults`)だけを行う。

   P2 follow-upで追加した「特殊強化」の2系統(`entry.special`。詳細は各関数のコメント参照)。
   どちらも`row.specialOn`が有効かつモジュール条件を満たす時だけ効く。置き換え系
   (旧仕様。行フィールドをスナップショットで上書き)はFW/Weedy実データ精査の結果
   不要と判明したため撤去済み(`resolveEntryValues`はもうentry.specialを見ない):
     - 加算系(`requires_module`+`addSelfAtkPctByModuleLevel`): セルフ%に加算
       (`resolveSpecialAddPct`)。例: ブレイズ/ウィーディ
     - 乗算系(`mulMultiplier`): `multiplier`に乗算する係数(`resolveSpecialMultiplierFactor`)。
       モジュール未装備でも`mulMultiplier.base`が常に効く(「常に適用可能」＝UIは
       チェックボックスを隠さない)。例: ファイヤーウォッチ

   P3で追加した「鼓舞(インスパイア)」(`catalog.inspireSources`。詳細は各関数のコメント参照)。
   鼓舞ソース(現時点では濁心スカジのみ。FKする側ではなく味方に鼓舞を撒く側)は
   fk_dataシートに載らないため、行(row)とは別に`state.inspire.sources[id]`で
   独立に設定を持つ。行との対応関係:
     - ソースのATK/自己%は行と同じ`resolveAtk`/個別バフ・条件付きバフ判定
       (`computeBuffBreakdown`)をそのまま再利用する(`computeInspireSource`)。
     - `鼓舞amount = ソースATK ×(1 + 素質等の自己%パーツ合計 + 個別バフ% + 条件付きバフ%
       + 手入力%) × スキル比率`。
     - 各行は`row.inspireOn`(既定true)がtrueの間、ONになっている鼓舞ソースのうち
       **最大の1件だけ**を`inspireFlat`に加算する(合算しない。複数ソースが将来増えても
       「一番効果の高い鼓舞1つを受ける」という前提)。
     - 鼓舞ソース自身の行(`row.opId === source.operatorId`)には、そのソース自身の
       鼓舞は乗らない(自分で自分を鼓舞しない。他ソースがあれば対象になり得る)。

   P7で追加した「スキルLv」(`row.skillLevel`/鼓舞ソースの`cfg.skillLevel`。1〜10。
   SLv1〜7+特化1〜3。既定10=特化3で旧デフォルトと完全互換)。カタログ側は
   `FkEntry.multiplierByLevel`/`selfAtkPctByLevel`/`multiplierCandidates[].valuesByLevel`
   （倍率候補）、鼓舞ソースの`skills[].ratioByLevel`としてスキルLv別の配列を持つ
   （`valueAtLevel`/`resolveMultiplierAtLevel`/`resolveSelfAtkPctAtLevel`/
   `resolveInspireRatioAtLevel`がこれを引く）。`skillLevel`を変えると
   `resolveEntryValues(entry, skillLevel)`が行フィールド(倍率/セルフ%)を
   その時点のカタログ既定値へ再スナップする(entryIdx変更時と同じ扱い。ユーザーは
   そこから更に手動で上書きできる)。`maxSkillLevelForElite`/`skillLevelWarning`が
   「E0はSLv4まで、E1はSLv7まで、特化1〜3は昇進2が必要」というゲーム側の一般ルールを
   判定する(既存のskillUnlockWarningと同じく、警告を出しても計算自体は続行する)。
   ============================================================ */

/**
 * @typedef {{hp:number, def:number, res:number, defFlat:number, defPct:number,
 *            resFlat:number, vulnPct:number}} EnemyState
 * @typedef {{opId:string, entryIdx:number, dmgType:('physical'|'arts'|'true'), potential:boolean,
 *            moduleId:(string|null), moduleLv:number, multiplier:number, selfPct:number,
 *            hits:number, buffPct:number, buffFlat:number, dmgMult:number, ignoreDef:number}} RowState
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
function talentTableRowFor(talent, elite, moduleId, moduleLevel) {
  if (!talent) return null;
  const module = moduleId && elite >= 2 ? talent.modules.find((m) => m.moduleId === moduleId) : null;
  if (module) return module.valuesByLevelAndPotential[moduleLevel - 1] ?? null;
  return talent.valuesByEliteAndPotential[elite] ?? null;
}

/** `talentTableRowFor`の結果からさらに`potential`(0〜5)の1値を引く。無ければ0。 */
function resolveTalentTableValue(talent, { elite, potential, moduleId, moduleLevel }) {
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
  };
}

/** row.moduleIdが指すモジュールのLv1〜3配列（見つからなければnull）。 */
export function findModule(op, moduleId) {
  if (!op || !moduleId) return null;
  return op.modules.find((m) => m.id === moduleId) ?? null;
}

/* ============================================================
   P6: 昇進(elite)/レベル(level)/信頼度(trust)を指定してベースATKを計算する。
   `op`(CatalogOperator/InspireSourceどちらも同じ形状)の`phases`
   (`[{maxLevel, atkMin, atkMax}, ...]`。インデックス0=E0)を線形補間して求める。
   詳細はオーナー確認済みの実測値で検証済み:
     - エーベンホルツ E2 Lv60 = 1134 + 59×(266/89) = 1310.34… → 四捨五入1310
     - シー E2 Lv71・信頼度100%・無凸モジュール・潜在+34 =
       (771→918の補間886.618→887) + 110(信頼度) + 34(潜在) = 1031
       (切り捨てなら1030になり実測と食い違うため、Math.round[四捨五入]が正しい)
   `op.phases`が無い(verify.mjs等が直接atkBaseを渡す簡易opオブジェクト)場合は
   `op.atkBase`をそのまま使う(elite/level/trustは無視。既存テストとの後方互換)。
   ============================================================ */

/** 昇進段階(0=E0/1=E1/2=E2)に対応する`op.phases`要素。範囲外/`phases`無しはnull。 */
export function phaseFor(op, elite) {
  const phases = (op && op.phases) || [];
  return phases[elite] ?? null;
}

/** そのopが到達できる最大昇進(0始まり)。`phases`が空なら0。 */
export function maxEliteFor(op) {
  const phases = (op && op.phases) || [];
  return Math.max(phases.length - 1, 0);
}

/** 指定昇進の最大レベル。`phases`に無ければ1(=クランプ計算の安全なフォールバック)。 */
export function maxLevelForElite(op, elite) {
  const phase = phaseFor(op, elite);
  return phase ? phase.maxLevel : 1;
}

/**
 * 昇進/レベル/信頼度からベースATKを計算する。Lv1〜Lv最大の間は線形補間し、
 * 四捨五入(`Math.round`)する。信頼度加算(`atkTrustMax × trust/100`)も同様に四捨五入し、
 * 補間後の値へ加算する(それぞれ丸めてから足す。詳細はファイル冒頭コメント参照)。
 * `level`は`phase.maxLevel`にクランプし、1未満にはしない。
 */
export function computeBaseAtk(op, elite, level, trust) {
  if (!op || !op.phases || !op.phases.length) return (op && op.atkBase) || 0;
  const phase = phaseFor(op, elite) ?? op.phases[op.phases.length - 1];
  const clampedLevel = Math.min(Math.max(level, 1), phase.maxLevel);
  const raw =
    phase.maxLevel <= 1
      ? phase.atkMax
      : phase.atkMin + ((clampedLevel - 1) / (phase.maxLevel - 1)) * (phase.atkMax - phase.atkMin);
  const trustAtk = Math.round((op.atkTrustMax || 0) * ((trust ?? 100) / 100));
  return Math.round(raw) + trustAtk;
}

/** row/cfgの`elite`(未設定なら`op`の最大昇進)。古い形(P1〜P5)のstate/共有URLとの
 * 互換のためのフォールバック(`dropStaleRows`が本来は補完するが、念のためここでも安全策)。 */
function effectiveElite(op, rowOrCfg) {
  return rowOrCfg && rowOrCfg.elite != null ? rowOrCfg.elite : maxEliteFor(op);
}

/** row/cfgの`level`(未設定ならその昇進の最大レベル)。 */
function effectiveLevel(op, rowOrCfg, elite) {
  return rowOrCfg && rowOrCfg.level != null ? rowOrCfg.level : maxLevelForElite(op, elite);
}

/** モジュールがそのelite/levelで実際に装備可能か。`module.unlockPhase`/`unlockLevel`が
 * 無い(verify.mjs等の簡易モジュールオブジェクト)場合は常にtrue(既存テストとの後方互換)。 */
export function moduleUsable(module, elite, level) {
  if (!module) return false;
  if (elite < module.unlockPhase) return false;
  if (elite === module.unlockPhase && level < module.unlockLevel) return false;
  return true;
}

/**
 * row/cfgが選択したモジュールのうち、現在のelite/levelで実際に装備できているものだけを
 * 返す(装備不可なら`null`)。ATK計算・特殊強化のモジュール条件・鼓舞の自己%パーツの
 * モジュール条件は全てこれ経由でmoduleIdを参照する(選択はしていても未装備の間は
 * 効果に含めない。UIはヒントで理由を示す)。
 */
export function effectiveModuleId(op, rowOrCfg) {
  const module = findModule(op, rowOrCfg && rowOrCfg.moduleId);
  if (!module) return null;
  const elite = effectiveElite(op, rowOrCfg);
  const level = effectiveLevel(op, rowOrCfg, elite);
  return moduleUsable(module, elite, level) ? rowOrCfg.moduleId : null;
}

/** entryが要求する解放昇進(`op.skillUnlockPhase`にskill_numが載っていなければnull。
 * 素質行等スキルではないentryはそもそも載らない)。 */
export function requiredEliteForEntry(op, entry) {
  if (!op || !entry) return null;
  const found = (op.skillUnlockPhase || []).find(([num]) => num === entry.skillNum);
  return found ? found[1] : null;
}

/**
 * 現在の`row.elite`ではentryのスキルがまだ解放されていない場合の警告文(例:
 * "S3は昇進2で解放")。解放済み/判定できない(entryが無い・素質行等)場合はnull。
 * 警告が出ていても計算自体は続行する(オーナー方針。撃破可否の判定はしない)。
 */
export function skillUnlockWarning(op, entry, row) {
  const required = requiredEliteForEntry(op, entry);
  if (required == null) return null;
  const elite = effectiveElite(op, row);
  if (elite >= required) return null;
  const prefix = /^\d+$/.test(entry.skillNum) ? `S${entry.skillNum}` : entry.skillNum;
  return `${prefix}は昇進${required}で解放`;
}

/**
 * P7: 昇進(elite)ごとに実際に到達できるスキルLvの上限(ゲームの一般ルール。オペレーター/
 * スキルには依らない): E0はSLv4まで、E1はSLv7まで、特化1〜3(SLv8〜10)は昇進2が必要。
 */
export function maxSkillLevelForElite(elite) {
  if (elite >= 2) return 10;
  if (elite === 1) return 7;
  return 4;
}

/**
 * 現在の`elite`では`skillLevel`がまだ解放されていない場合の警告文(例:
 * "特化は昇進2で解放"/"SLv5以上は昇進1で解放")。解放済みならnull。
 * `skillUnlockWarning`(そのスキル自体の解放昇進)とは独立の判定で、両方出ることもある
 * (計算自体はどちらの警告が出ていても続行する。オーナー方針)。
 */
export function skillLevelWarning(elite, skillLevel) {
  if (skillLevel >= 8) return elite >= 2 ? null : "特化は昇進2で解放";
  if (skillLevel >= 5) return elite >= 1 ? null : "SLv5以上は昇進1で解放";
  return null;
}

/**
 * P8: 潜在ランク(0始まり。0=潜在1〜5=潜在6)からATK加算値を引く。範囲外はクランプする。
 * `op.atkPotentialByRank`(カタログ由来。長さ6の累積配列)があればそれをそのまま引く。
 * 無い(verify.mjs等の簡易opオブジェクト)場合は`op.atkPotential`(合計値)を
 * 「潜在0(潜在1)だけ+0、それ以外は全額」という単純な表として扱う後方互換フォールバック。
 */
export function resolveAtkPotential(op, potentialRank) {
  const rank = Math.min(Math.max(potentialRank ?? 5, 0), 5);
  if (!op) return 0;
  if (op.atkPotentialByRank) return op.atkPotentialByRank[rank] ?? 0;
  return rank > 0 ? op.atkPotential || 0 : 0;
}

/**
 * P8: 潜在ランク(`row.potential`/鼓舞ソースcfgの`potential`)の値を検証する。0〜5の数値なら
 * そのまま、そうでなければ(旧boolean形式・欠損値)既定値(5=潜在6)にリセットする
 * (`dropStaleRows`が使う。丁寧な移行はしない方針の詳細は同関数のコメント参照)。
 */
function normalizePotentialRank(v) {
  return typeof v === "number" && v >= 0 && v <= 5 ? v : 5;
}

/** atk = ベースATK(昇進/レベル/信頼度) + (潜在) + (モジュール。装備可能な場合のみ)。 */
export function resolveAtk(op, row) {
  const elite = effectiveElite(op, row);
  const level = effectiveLevel(op, row, elite);
  const trust = row && row.trust != null ? row.trust : 100;
  const moduleId = effectiveModuleId(op, { moduleId: row.moduleId, elite, level });
  const module = findModule(op, moduleId);
  const moduleAtk = module ? module.atkByLevel[row.moduleLv - 1] ?? 0 : 0;
  return computeBaseAtk(op, elite, level, trust) + resolveAtkPotential(op, row.potential) + moduleAtk;
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

/* ============================================================
   P3: 鼓舞(インスパイア)ソース
   ============================================================ */

/**
 * 鼓舞ソースの設定の初期値(state未設定時のフォールバック)。
 * P6: 昇進/レベル/信頼度も行(`makeDefaultRow`)と同じ既定(最大昇進・その最大レベル・
 * 信頼度100)にする。
 */
export function defaultInspireSourceCfg(source) {
  const elite = maxEliteFor(source);
  return {
    on: false,
    skillNum: source.skills && source.skills[0] ? source.skills[0].skillNum : "",
    skillLevel: 10, // P7: スキルLv(既定は特化3。旧デフォルトと同じ結果になる)
    potential: 5, // P8: 潜在ランク(0始まり。既定は潜在6=フル。旧来の攻撃凸+素質凸を統合)
    moduleId: null,
    moduleLv: 3,
    elite,
    level: maxLevelForElite(source, elite),
    trust: 100,
    buffPct: 0,
    buffIds: [],
    parts: {},
  };
}

/**
 * `self_parts`(素質等の自己%条件パーツ)の適用結果を計算する。`replaces`で指定された
 * パーツがON+適用可能な間、置き換え先(`replaces`の値が指すid)の寄与は無効化される
 * (加算ではなく置き換え。例: 「アビサルハンターがいる」がONなら基本の「素質」は
 * 数えない)。`requiresModule`を持つパーツは`cfg.moduleId`がそれと一致する間だけ
 * 「適用可能」になる(一致しなければUIはヒントを出し、常にOFF扱い)。
 * @returns {{pct:number, applied:Array<{id:string,label:string,shortLabel:string,value:number}>}}
 */
export function computeInspireSelfParts(source, cfg) {
  const parts = (source && source.selfParts) || [];
  const partCfg = (cfg && cfg.parts) || {};
  // P6: モジュール条件は`effectiveModuleId`(elite/level込みの装備可否)経由で判定する
  // (未装備/装備不可の間はモジュール由来の効果を含めない)。
  const moduleId = effectiveModuleId(source, cfg);
  // P8: 旧来の「素質凸」チェックボックス(cfg.talentPotential)は潜在セレクト(cfg.potential)へ
  // 統合したため、`source.talentPotentialRank`(0始まり。素質凸ボーナスが解放される潜在)
  // 以上を選んでいる間だけ適用する(rankが無いソースは素質凸ボーナスの概念が無いのでfalse)。
  const talentPotentialOn = source && source.talentPotentialRank != null && (cfg.potential ?? 5) >= source.talentPotentialRank;

  const isApplicable = (p) => !p.requiresModule || moduleId === p.requiresModule;
  const isOn = (p) => {
    if (!isApplicable(p)) return false;
    if (p.alwaysOn) return true;
    const stored = partCfg[p.id];
    return stored !== undefined ? !!stored : !!p.defaultOn;
  };
  const valueFor = (p) => {
    if (p.moduleOverride && moduleId === p.moduleOverride.module) {
      const lv = cfg.moduleLv - 1;
      const base = p.moduleOverride.pctByLevel[lv] ?? 0;
      const bonus = talentPotentialOn ? p.moduleOverride.potentialBonusByLevel[lv] ?? 0 : 0;
      return base + bonus;
    }
    if (p.requiresModule && p.pctByModuleLevel) {
      return p.pctByModuleLevel[cfg.moduleLv - 1] ?? 0;
    }
    return (p.pct || 0) + (talentPotentialOn ? p.pctPotentialBonus || 0 : 0);
  };

  const suppressed = new Set();
  for (const p of parts) {
    if (p.replaces && isOn(p)) suppressed.add(p.replaces);
  }

  const applied = [];
  let pct = 0;
  for (const p of parts) {
    if (suppressed.has(p.id)) continue;
    if (!isOn(p)) continue;
    const value = valueFor(p);
    pct += value;
    applied.push({ id: p.id, label: p.label, shortLabel: p.shortLabel || p.label, value });
  }
  return { pct, applied };
}

/**
 * 鼓舞ソースのATK。行と同じ`resolveAtk`をそのまま再利用する(`cfg`を行と同じ形状
 * {potential, moduleId, moduleLv, elite, level, trust} に正規化して渡すだけで済む)。
 */
export function resolveInspireSourceAtk(source, cfg) {
  return resolveAtk(source, {
    potential: cfg.potential,
    moduleId: cfg.moduleId,
    moduleLv: cfg.moduleLv,
    elite: cfg.elite,
    level: cfg.level,
    trust: cfg.trust,
  });
}

/**
 * 鼓舞ソース1件分の計算。個別バフ(`cfg.buffIds`)/条件付きバフ(`globalBuffIds`、
 * ソース自身の`tags`との一致で判定)は行と同じ`computeBuffBreakdown`をそのまま
 * 再利用する(行/entryの形状に正規化して渡す)。
 * `amount = atk ×(1 + 素質等の自己%パーツ合計 + 個別% + 条件付き% + 手入力%) × ratio`。
 * ソース自身のATKには鼓舞(inspireFlat)を足さない(鼓舞ソースは他の鼓舞から
 * ブーストされない。呼び出し側=`computeInspireForRow`が自己適用も除外する)。
 * @returns {{amount:number, atk:number, ratio:number, skillNum:string, selfPct:number,
 *            selfParts:{pct:number, applied:Array}, breakdown:object, manualPct:number}}
 */
/**
 * P7: `skillEntry.ratioByLevel`から`skillLevel`(既定10=特化3)の鼓舞倍率を引く。
 * 配列が無ければ`skillEntry.ratio`(固定値)にフォールバックする。
 */
export function resolveInspireRatioAtLevel(skillEntry, skillLevel) {
  if (!skillEntry) return 0;
  const arr = skillEntry.ratioByLevel;
  if (!arr || !arr.length) return skillEntry.ratio || 0;
  return valueAtLevel(arr, skillLevel);
}

export function computeInspireSource(source, cfg, catalog, globalBuffIds = [], globalBuffLevels = {}) {
  const skills = (source && source.skills) || [];
  const skillEntry = skills.find((s) => s.skillNum === cfg.skillNum) || skills[0] || { skillNum: "", ratio: 0 };
  const ratio = resolveInspireRatioAtLevel(skillEntry, cfg.skillLevel ?? 10);
  const atk = resolveInspireSourceAtk(source, cfg);
  const selfParts = computeInspireSelfParts(source, cfg);
  const pseudoRow = { buffIds: cfg.buffIds || [] };
  const pseudoEntry = { tags: (source && source.tags) || [] };
  const breakdown = computeBuffBreakdown(pseudoRow, pseudoEntry, catalog, globalBuffIds, globalBuffLevels);
  const manualPct = cfg.buffPct || 0;
  const selfPct = selfParts.pct + breakdown.extraPct + manualPct;
  const amount = atk * (1 + selfPct) * ratio;
  return { amount, atk, ratio, skillNum: skillEntry.skillNum, selfPct, selfParts, breakdown, manualPct };
}

/**
 * 行が受け取る鼓舞を決める。ONになっている鼓舞ソースのうち`amount`が最大の1件だけを
 * 採用する(合算しない)。`row.inspireOn`がfalseの間は常に`null`。鼓舞ソース自身の行
 * (`row.opId === source.operatorId`)にはそのソース自身の鼓舞は乗らない。
 * @returns {null|{sourceId:string, sourceName:string, amount:number}}
 */
export function computeInspireForRow(catalog, row, inspireSourceStates, globalBuffIds = [], globalBuffLevels = {}) {
  if (!row || row.inspireOn === false) return null;
  const sources = (catalog && catalog.inspireSources) || [];
  const states = inspireSourceStates || {};
  let best = null;
  for (const source of sources) {
    const cfg = states[source.id];
    if (!cfg || !cfg.on) continue;
    if (row.opId && row.opId === source.operatorId) continue;
    const result = computeInspireSource(source, cfg, catalog, globalBuffIds, globalBuffLevels);
    if (result.amount <= 0) continue;
    if (!best || result.amount > best.amount) best = { sourceId: source.id, sourceName: source.name, amount: result.amount };
  }
  return best;
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
 * @returns {{results:(Array<null|{row:RowState, op:object, breakdown:object, inspireApplied:object}&ReturnType<typeof computeRowDamage>>),
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
    const dmg = computeRowDamage(op, row, enemy, inspireFlat, breakdown.extraPct + specialAddPct, specialMulFactor);
    return { row, op, breakdown, specialAddPct, specialMulFactor, inspireApplied, ...dmg };
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
 * P3: `row.inspireOn`の省略時デフォルト(true)も補い、`state.inspire.sources`から
 * カタログに存在しないソースid/パーツid・バフidを同じく静かに取り除く。
 * P4: `source`付き条件付きバフ全てに`state.globalBuffLevels[id]`のデフォルト値
 * (`source.defaults`由来。未設定/一部欠損分だけ補う)を補完し、カタログから消えた
 * バフidのエントリは静かに取り除く。また旧(P2)形の`amiya_guard_normal`/
 * `amiya_guard_skill`(P4で`amiya_guard`+toggleへ1本化)が`globalBuffIds`に残っていれば
 * `amiya_guard`(+`amiya_guard_skill`だった場合はtoggleOn)へ移行する。
 * P5: `globalBuffLevels`の補完対象はconditionalに限らず`b.source`を持つ全バフ(個別も含む)。
 * 旧(P2)形の`stainless_1`/`stainless_2`(2エントリ制。P5で`stainless_s1`+toggleへ1本化)が
 * `row.buffIds`/鼓舞ソースの`buffIds`に残っていれば`stainless_s1`へ移行する
 * (`stainless_2`だった箇所が1つでもあれば共有toggleをONにする。toggleはバフ単位で
 * 共有する状態[`globalBuffLevels`]なので、どの行/ソース由来でも1回ONにすれば良い)。
 * P8: `row.potential`/鼓舞ソースcfgの`potential`は旧来のboolean(攻撃凸チェックボックス)から
 * 潜在ランク(0〜5の数値)へ仕様変更した。このツールはまだプロトタイプ段階のため丁寧な
 * 移行(旧攻撃凸/素質凸の組み合わせから最も近いランクを推測する等)はせず、数値でなければ
 * 単純に既定値(5=潜在6)へリセットする(オーナー指示)。旧`talentPotential`フィールドは
 * 素質凸境目をデータ駆動化(`talentPotentialRank`)したため`potential`に統合し、cfgから
 * 削除する。
 * @returns {{state:object, dropped:number}}
 */
export function dropStaleRows(state, catalog) {
  const opById = new Map(catalog.operators.map((o) => [o.id, o]));
  const buffers = catalog.buffers || [];
  const validBuffIds = new Set(buffers.map((b) => b.id));
  const inspireSources = (catalog.inspireSources || []);
  let dropped = 0;

  // P5: stainless_1/stainless_2 → stainless_s1 の移行(migrateStainlessIdsの前に、
  // 移行前の生データ全体からstainless_2の使用有無を1回だけ調べる。globalBuffLevelsは
  // バフ単位で共有する状態なので、行/鼓舞ソースのどちらでどう使われていたかは問わない)。
  const rawSourceCfgs = Object.values((state.inspire && state.inspire.sources) || {});
  const hadStainless2 = [...state.rows.flatMap((r) => r.buffIds || []), ...rawSourceCfgs.flatMap((c) => (c && c.buffIds) || [])].includes(
    "stainless_2",
  );
  const migrateStainlessIds = (ids) => {
    if (!ids || !ids.length) return ids;
    const set = new Set();
    for (const id of ids) set.add(id === "stainless_1" || id === "stainless_2" ? "stainless_s1" : id);
    return Array.from(set);
  };

  const rows = state.rows
    .filter((row) => {
      const op = opById.get(row.opId);
      const ok = !!op && row.entryIdx >= 0 && row.entryIdx < op.fkEntries.length;
      if (!ok) dropped++;
      return ok;
    })
    .map((row) => {
      const op = opById.get(row.opId);
      // P6: 昇進/レベル/信頼度が無い古い形(P1〜P5)のstate/共有URLを補完する
      // (`makeDefaultRow`と同じ既定: 最大昇進・その最大レベル・信頼度100)。
      const elite = row.elite != null ? row.elite : maxEliteFor(op);
      const level = row.level != null ? row.level : maxLevelForElite(op, elite);
      return {
        ...row,
        elite,
        level,
        trust: row.trust != null ? row.trust : 100,
        skillLevel: row.skillLevel != null ? row.skillLevel : 10, // P7: 旧(P1〜P6)形の補完
        buffFlat: Number.isFinite(row.buffFlat) ? row.buffFlat : 0, // 手入力 基礎攻撃力+ (#29)
        // P8: `potential`は旧来のboolean(攻撃凸チェックボックス)から潜在ランク(0〜5の数値)へ
        // 仕様変更した。プロトタイプ段階のため丁寧な移行はせず、数値でなければ単純に
        // 既定値(5=潜在6)へリセットする(オーナー指示)。
        potential: normalizePotentialRank(row.potential),
        buffIds: migrateStainlessIds(row.buffIds || []).filter((id) => validBuffIds.has(id)),
        specialOn: row.specialOn !== false,
        inspireOn: row.inspireOn !== false,
      };
    });

  // P4: 旧(P2)形の前衛アーミヤ2エントリ(exclusive_group)→新1エントリ+toggleへの移行。
  let rawGlobalBuffIds = (state.globalBuffIds || []).slice();
  const migratedToggleOn = new Set();
  const hadNormal = rawGlobalBuffIds.includes("amiya_guard_normal");
  const hadSkill = rawGlobalBuffIds.includes("amiya_guard_skill");
  if (hadNormal || hadSkill) {
    rawGlobalBuffIds = rawGlobalBuffIds.filter((id) => id !== "amiya_guard_normal" && id !== "amiya_guard_skill");
    if (validBuffIds.has("amiya_guard")) {
      rawGlobalBuffIds.push("amiya_guard");
      if (hadSkill) migratedToggleOn.add("amiya_guard");
    }
  }
  const globalBuffIds = rawGlobalBuffIds.filter((id) => validBuffIds.has(id));

  // P4/P5: source付きバフ全て(conditional/individual問わず)にglobalBuffLevelsの
  // デフォルトを補う(初めてONにした時に見せる値と、共有URL/localStorageの欠損補完を兼ねる)。
  const rawLevels = state.globalBuffLevels || {};
  const globalBuffLevels = {};
  for (const b of buffers) {
    if (!b.source) continue;
    const d = b.source.defaults || {};
    const stored = rawLevels[b.id] || {};
    globalBuffLevels[b.id] = {
      elite: d.elite ?? 2,
      potential: d.potential ?? 5,
      moduleId: d.moduleId ?? null,
      moduleLevel: d.moduleLevel ?? 3,
      skillLevel: d.skillLevel ?? 1,
      stageIndex: d.stageIndex ?? 1,
      toggleOn: false,
      ...stored,
    };
    if (migratedToggleOn.has(b.id)) globalBuffLevels[b.id].toggleOn = true;
    if (b.id === "stainless_s1" && hadStainless2) globalBuffLevels[b.id].toggleOn = true;
  }

  const sourceById = new Map(inspireSources.map((s) => [s.id, s]));
  const rawInspireSources = (state.inspire && state.inspire.sources) || {};
  const cleanedSources = {};
  for (const [sourceId, cfg] of Object.entries(rawInspireSources)) {
    const source = sourceById.get(sourceId);
    if (!source || !cfg) continue; // 未知のソースidは静かに除去
    const knownPartIds = new Set((source.selfParts || []).map((p) => p.id));
    const parts = {};
    for (const [partId, on] of Object.entries(cfg.parts || {})) {
      if (knownPartIds.has(partId)) parts[partId] = on;
    }
    // P6: 昇進/レベル/信頼度が無い古い形を補完する(行と同じ既定値)。
    const elite = cfg.elite != null ? cfg.elite : maxEliteFor(source);
    const level = cfg.level != null ? cfg.level : maxLevelForElite(source, elite);
    // P8: 旧`talentPotential`(素質凸チェックボックス)は`potential`(潜在ランク)へ統合したため、
    // spread元から取り除いて捨てる(restCfgには残さない)。
    const { talentPotential, ...restCfg } = cfg;
    cleanedSources[sourceId] = {
      ...restCfg,
      elite,
      level,
      trust: cfg.trust != null ? cfg.trust : 100,
      skillLevel: cfg.skillLevel != null ? cfg.skillLevel : 10, // P7: 旧(P1〜P6)形の補完
      potential: normalizePotentialRank(cfg.potential), // P8: 旧形式は既定値(5)へリセット
      buffIds: migrateStainlessIds(cfg.buffIds || []).filter((id) => validBuffIds.has(id)),
      parts,
    };
  }

  return { state: { ...state, rows, globalBuffIds, globalBuffLevels, inspire: { sources: cleanedSources } }, dropped };
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
