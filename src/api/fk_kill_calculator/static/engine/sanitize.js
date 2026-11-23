// フレームキル計算機 — 保存/共有された状態の検証と補完(dropStaleRows)。

import { normalizeStones } from "./stone_board.js";
import { makeDefaultExtraParts } from "./entry.js";
import { maxEliteFor, maxLevelForElite, normalizePotentialRank } from "./stats.js";

/**
 * カタログに存在しない opId/entryIdx を指す行を取り除く
 * （ゲームデータ更新でオペレーター/スキルが変わったURL状態を安全に読み込むため）。
 * ついでにカタログに存在しないバフidを`row.buffIds`/`state.globalBuffIds`から
 * 静かに(トースト無し)取り除き、`specialOn`の省略時デフォルト(true)も補う
 * （古い形のstate/共有URLにはこれらのフィールドが無いため、そのまま読めるようにする）。
 * `row.inspireOn`の省略時デフォルト(true)も補い、`state.inspire.sources`から
 * カタログに存在しないソースid/パーツid・バフidを同じく静かに取り除く。
 * `source`付き条件付きバフ全てに`state.globalBuffLevels[id]`のデフォルト値
 * (`source.defaults`由来。未設定/一部欠損分だけ補う)を補完し、カタログから消えた
 * バフidのエントリは静かに取り除く。また古い形の`amiya_guard_normal`/
 * `amiya_guard_skill`(現在は`amiya_guard`+toggleの1本)が`globalBuffIds`に残っていれば
 * `amiya_guard`(+`amiya_guard_skill`だった場合はtoggleOn)へ移行する。
 * `globalBuffLevels`の補完対象はconditionalに限らず`b.source`を持つ全バフ(個別も含む)。
 * 古い形の`stainless_1`/`stainless_2`(現在は`stainless_s1`+toggleの1本)が
 * `row.buffIds`/鼓舞ソースの`buffIds`に残っていれば`stainless_s1`へ移行する
 * (`stainless_2`だった箇所が1つでもあれば共有toggleをONにする。toggleはバフ単位で
 * 共有する状態[`globalBuffLevels`]なので、どの行/ソース由来でも1回ONにすれば良い)。
 * `row.potential`/鼓舞ソースcfgの`potential`は潜在ランク(0〜5の数値)。このツールは
 * まだプロトタイプ段階のため丁寧な移行(booleanだった古い値から最も近いランクを推測する等)
 * はせず、数値でなければ単純に既定値(5=潜在6)へリセットする(オーナー指示)。
 * 古いcfgの`talentPotential`フィールドは`potential`に統合済みなので削除する。
 * 混合スキル: `row.extraParts`の長さが`entry.parts`の2番目以降の数と合わない、
 * または要素の形(multiplier/hits/dmgType/dmgMultを持つ)が壊れていれば、
 * `makeDefaultExtraParts`でentry既定値から作り直す(プロトタイプなので丁寧な移行は
 * しない。entryIdxが範囲外/別エントリになった行も既存の扱いに任せる)。
 * @returns {{state:object, dropped:number}}
 */
/**
 * `extraParts`が「`entry.parts`の2番目以降の数だけ、正しい形の要素を持つ配列」かを
 * 検証する(`dropStaleRows`が壊れていれば作り直すために使う。プロトタイプなので詳細な
 * 移行はしない: 形が合わなければ丸ごとentry既定値で作り直す)。
 */
function isValidExtraParts(extraParts, expectedLen) {
  // `{"length":0}`のような配列もどきを通すと後段の`.map`で例外になるので、必ずArray.isArrayで見る。
  if (expectedLen === 0) return extraParts == null || (Array.isArray(extraParts) && extraParts.length === 0);
  if (!Array.isArray(extraParts) || extraParts.length !== expectedLen) return false;
  return extraParts.every(
    (p) => p && typeof p.multiplier === "number" && typeof p.dmgType === "string" && typeof p.dmgMult === "number",
  );
}

/**
 * 保存/共有された状態(localStorage・`#state=`共有URL)は誰でも作れる信頼できない入力。
 * ui.jsは数値フィールドを`<input value="${...}">`等へそのまま埋め込むため、有限の数値に
 * ならない値(HTMLを含む文字列等)は既定値に置き換える。`null`/`undefined`も既定値扱い
 * (従来の`x != null ? x : 既定値`と同じ)。
 */
function finiteNumberOr(v, fallback) {
  const n = v == null ? NaN : Number(v);
  // 1e308のような巨大値は二分探索(suggest)や積の計算を壊すので、現実の入力より十分大きい
  // 上限(±NUM_CAP)で頭打ちにする。実在の敵HP/防御やダメージはこれより遥かに小さい。
  return Number.isFinite(n) ? Math.max(-NUM_CAP, Math.min(NUM_CAP, n)) : fallback;
}

const NUM_CAP = 1e9;

// 列挙値は既知の値以外を既定値へ戻す(UIの<select>やダメージ式の分岐に未知の文字列を入れない)。
const DMG_TYPES = ["physical", "arts", "true"];
function enumOr(v, allowed, fallback) {
  return allowed.includes(v) ? v : fallback;
}
/** 整数に丸めて[lo,hi]へ収める。数値でなければfallback。 */
function intInRange(v, lo, hi, fallback) {
  return Math.max(lo, Math.min(hi, Math.round(finiteNumberOr(v, fallback))));
}
/** 文字列かnull(モジュールid等。UIへ埋め込む前にescapeされるが、型だけは揃える)。 */
function stringOrNull(v) {
  return typeof v === "string" ? v : null;
}

// 敵の入力欄(ui.jsの`defaultEnemy`と同じフィールド。既定値は全て0)。
const ENEMY_FIELDS = ["hp", "def", "res", "defFlat", "defPct", "resFlat", "vulnPct"];

export function dropStaleRows(state, catalog) {
  const opById = new Map(catalog.operators.map((o) => [o.id, o]));
  const buffers = catalog.buffers || [];
  const validBuffIds = new Set(buffers.map((b) => b.id));
  const inspireSources = (catalog.inspireSources || []);
  let dropped = 0;

  // stainless_1/stainless_2 → stainless_s1 の移行(migrateStainlessIdsの前に、
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
      const entry = op.fkEntries[row.entryIdx];
      // 昇進/レベル/信頼度が無い古い形のstate/共有URLを補完する
      // (`makeDefaultRow`と同じ既定: 最大昇進・その最大レベル・信頼度100)。
      const elite = finiteNumberOr(row.elite, maxEliteFor(op));
      const level = finiteNumberOr(row.level, maxLevelForElite(op, elite));
      const skillLevel = intInRange(row.skillLevel, 1, 10, 10); // スキルLvが無い古い形の補完
      // extraPartsの形/長さがentry.partsの2番目以降と合わなければ作り直す。
      const expectedExtraLen = Math.max(((entry && entry.parts && entry.parts.length) || 0) - 1, 0);
      // 形が合っていても、既知のフィールドだけを型を揃えて作り直す(余計なキーや非数を通さない)。
      const extraParts = isValidExtraParts(row.extraParts, expectedExtraLen)
        ? (row.extraParts || []).map((p) => ({
            multiplier: finiteNumberOr(p.multiplier, 1),
            dmgType: enumOr(p.dmgType, DMG_TYPES, "physical"),
            dmgMult: finiteNumberOr(p.dmgMult, 1),
          }))
        : makeDefaultExtraParts(entry, skillLevel);
      return {
        ...row,
        elite,
        level,
        trust: finiteNumberOr(row.trust, 100),
        skillLevel,
        hits: finiteNumberOr(row.hits, 1),
        ignoreDef: finiteNumberOr(row.ignoreDef, 0),
        buffFlat: finiteNumberOr(row.buffFlat, 0), // 手入力 基礎攻撃力+
        dmgType: enumOr(row.dmgType, DMG_TYPES, "physical"),
        moduleId: stringOrNull(row.moduleId),
        moduleLv: intInRange(row.moduleLv, 1, 3, 3),
        multiplier: finiteNumberOr(row.multiplier, 1),
        selfPct: finiteNumberOr(row.selfPct, 0),
        buffPct: finiteNumberOr(row.buffPct, 0),
        dmgMult: finiteNumberOr(row.dmgMult, 1),
        // `potential`は潜在ランク(0〜5の数値)。古いboolean値などはプロトタイプ段階のため
        // 丁寧な移行はせず、数値でなければ単純に既定値(5=潜在6)へリセットする(オーナー指示)。
        potential: normalizePotentialRank(row.potential),
        buffIds: migrateStainlessIds(row.buffIds || []).filter((id) => validBuffIds.has(id)),
        specialOn: row.specialOn !== false,
        inspireOn: row.inspireOn !== false,
        extraParts,
        stones: normalizeStones(row.stones),
      };
    });

  // 古い形の前衛アーミヤ2エントリ(exclusive_group)→1エントリ+toggleへの移行。
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

  // source付きバフ全て(conditional/individual問わず)にglobalBuffLevelsの
  // デフォルトを補う(初めてONにした時に見せる値と、共有URL/localStorageの欠損補完を兼ねる)。
  const rawLevels = state.globalBuffLevels || {};
  const globalBuffLevels = {};
  for (const b of buffers) {
    if (!b.source) continue;
    const d = b.source.defaults || {};
    const stored = rawLevels[b.id];
    // 保存値(外部入力)は既知のキーだけを型を揃えて取り込む(`...stored`で生のまま通さない)。
    const st = stored && typeof stored === "object" ? stored : {};
    globalBuffLevels[b.id] = {
      elite: intInRange(st.elite, 0, 2, d.elite ?? 2),
      potential: normalizePotentialRank(st.potential ?? d.potential ?? 5),
      moduleId: st.moduleId === undefined ? (d.moduleId ?? null) : stringOrNull(st.moduleId),
      moduleLevel: intInRange(st.moduleLevel, 1, 3, d.moduleLevel ?? 3),
      skillLevel: intInRange(st.skillLevel, 1, 10, d.skillLevel ?? 1),
      stageIndex: intInRange(st.stageIndex, 1, 99, d.stageIndex ?? 1),
      toggleOn: st.toggleOn === true,
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
    // 昇進/レベル/信頼度が無い古い形を補完する(行と同じ既定値)。
    // 共有URL/localStorageは外部入力なので、数値フィールドは有限の数値でなければ既定値へ
    // 戻し、onはbooleanにする(文字列のまま残すとUIのinnerHTMLにマークアップとして入り得る)。
    const elite = finiteNumberOr(cfg.elite, maxEliteFor(source));
    const level = finiteNumberOr(cfg.level, maxLevelForElite(source, elite));
    // 古い`talentPotential`は`potential`(潜在ランク)に統合済みなので、
    // spread元から取り除いて捨てる(restCfgには残さない)。
    const { talentPotential, ...restCfg } = cfg;
    cleanedSources[sourceId] = {
      ...restCfg,
      on: cfg.on === true, // ui.jsが`aria-pressed="${cfg.on}"`へ埋め込むため真偽値に限る(既定はOFF)
      elite,
      level,
      trust: finiteNumberOr(cfg.trust, 100),
      skillLevel: intInRange(cfg.skillLevel, 1, 10, 10), // スキルLvが無い古い形の補完
      moduleId: stringOrNull(cfg.moduleId),
      skillNum: typeof cfg.skillNum === "string" ? cfg.skillNum : "",
      moduleLv: intInRange(cfg.moduleLv, 1, 3, 3),
      buffPct: finiteNumberOr(cfg.buffPct, 0),
      potential: normalizePotentialRank(cfg.potential), // 数値でなければ既定値(5)へリセット
      buffIds: migrateStainlessIds(cfg.buffIds || []).filter((id) => validBuffIds.has(id)),
      parts,
    };
  }

  // 敵は既知のフィールドだけを有限の数値で作り直す(生のオブジェクトをそのまま通さない)。
  const rawEnemy = state.enemy || {};
  const enemy = Object.fromEntries(ENEMY_FIELDS.map((k) => [k, finiteNumberOr(rawEnemy[k], 0)]));

  return { state: { ...state, enemy, rows, globalBuffIds, globalBuffLevels, inspire: { sources: cleanedSources } }, dropped };
}
