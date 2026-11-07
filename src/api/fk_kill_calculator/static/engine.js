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

   P10で追加した「混合スキル」(`entry.parts`。例: ホルンS2の物理+術)。1回の攻撃で
   複数系統のダメージパーツを同時に与えるスキルは、育成・バフ設定(昇進/潜在/モジュール/
   スキルLv/セルフ%/手入力バフ/防御無視/Hit数/個別バフ/条件付きバフ/特殊強化/鼓舞)を1つだけ
   共有し、パーツごとに違うのは倍率/ダメージ種別/ダメージ倍率だけにする(Hit数は1回の攻撃で
   全パーツが同時に出るため共通。ダメージ種別はカタログ固定でUIからは選ばせない)。
   `row`自身が常にパーツ0(トップレベルのmultiplier/dmgType/dmgMultがそのまま
   パーツ0の値)で、2番目以降は`row.extraParts[i]`(`{multiplier, dmgType, dmgMult}`)
   に持つ(トップレベルへのミラーは作らない。同期問題を避けるため)。`expandRowParts(row)`が
   `[row, {...row,...extraParts[0]}, ...]`という疑似行配列を返し、`computeTotal`が
   各疑似行に`computeRowDamage`を適用してから合算する(バフ内訳/鼓舞/特殊強化は行で1回
   だけ計算して全パーツ共通に使う)。`entry.parts`を持たない通常のスキルは
   `extraParts`が空/無いままなので、この機構は完全に後方互換。
   ============================================================ */

/**
 * @typedef {{hp:number, def:number, res:number, defFlat:number, defPct:number,
 *            resFlat:number, vulnPct:number}} EnemyState
 * @typedef {{opId:string, entryIdx:number, dmgType:('physical'|'arts'|'true'), potential:boolean,
 *            moduleId:(string|null), moduleLv:number, multiplier:number, selfPct:number,
 *            hits:number, buffPct:number, buffFlat:number, dmgMult:number, ignoreDef:number}} RowState
 */

// 実体は ./engine/ の各モジュール。公開する名前はここで一覧にする（ui と verify.mjs の入口）。
export {
  findOperator,
  findEntry,
  defaultModuleId,
  valueAtLevel,
  resolveMultiplierAtLevel,
  resolveSelfAtkPctAtLevel,
  makeDefaultExtraParts,
  expandRowParts,
  resolveEntryValues,
  specialAddCanApply,
  resolveSpecialAddPct,
  resolveSpecialMultiplierFactor,
  specialMulCanApply,
  specialUiState,
  resolveSpecialCurrentValue,
  makeDefaultRow,
} from "./engine/entry.js";
export {
  findModule,
  phaseFor,
  maxEliteFor,
  maxLevelForElite,
  computeBaseAtk,
  moduleUsable,
  effectiveModuleId,
  requiredEliteForEntry,
  skillUnlockWarning,
  maxSkillLevelForElite,
  skillLevelWarning,
  resolveAtkPotential,
  resolveAtk,
  resolveDefEff,
  resolveResEff,
} from "./engine/stats.js";
export {
  ALL_TAG,
  potentialGroups,
  talentPotentialGroups,
  atkPotentialGroups,
  inspirePotentialGroups,
  resolveConditionalValue,
  computeBuffBreakdown,
  findLinkedRow,
  effectiveBuffLevels,
  computeEffectiveGlobalBuffLevels,
  autoEnableSourcedBuffs,
  findSingleTargetConflicts,
} from "./engine/buffs.js";
export {
  defaultInspireSourceCfg,
  computeInspireSelfParts,
  resolveInspireSourceAtk,
  resolveInspireRatioAtLevel,
  computeInspireSource,
  computeInspireForRow,
} from "./engine/inspire.js";
export {
  computeRowDamage,
  computeTotal,
} from "./engine/damage.js";
export {
  STONE_BOARD_SIZE,
  RED_TILE_CELL,
  isStoneInRange,
  normalizeStones,
  resolveStoneBoardValues,
  analyzeStoneBoard,
} from "./engine/stone_board.js";
export {
  dropStaleRows,
} from "./engine/sanitize.js";
export {
  suggest,
  describeSuggestion,
} from "./engine/suggest.js";
