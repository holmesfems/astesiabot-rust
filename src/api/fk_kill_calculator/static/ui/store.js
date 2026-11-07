// フレームキル計算機 — 画面の状態（カタログ・state・開閉状態）と保存・共有URL。

import {
  computeEffectiveGlobalBuffLevels,
  defaultInspireSourceCfg,
  dropStaleRows,
  effectiveBuffLevels,
  findLinkedRow,
  findOperator,
} from "../engine.js";
import { showToast } from "./dom.js";

export let catalog = null;
export let state = null;
export let expandedIdx = null;
// 「② 全体バフ」の<details>開閉状態(P2)。render()は#app丸ごと作り直すため、
// <details>のネイティブなopen状態はDOM要素ごと消える。ここに覚えておいて
// renderGlobalBuffs()が毎回復元する(expandedIdxと同じ考え方)。
export let globalBuffsOpen = false;
// 判定欄(上部固定)の「撃破するには」を開いているか。render()を跨いで保持する。
export let verdictSuggestOpen = false;
// P2 follow-up: 特殊強化のⓘ説明文が展開されている行のインデックス集合。
// render()は#app丸ごと作り直すため、ここに覚えておいて復元する(globalBuffsOpenと同じ考え方)。
export const specialDescOpenIdx = new Set();
// P3: 鼓舞ソースのⓘ内訳が展開されているソースid集合(specialDescOpenIdxと同じ考え方)。
export const inspireDescOpenIds = new Set();
let saveTimer = null;

// ES モジュールの import は読み取り専用（値の変化は見える）なので、他のモジュールからの代入は setter を通す。
export function setCatalog(v) {
  catalog = v;
}
export function setState(v) {
  state = v;
}
export function setExpandedIdx(v) {
  expandedIdx = v;
}
export function setGlobalBuffsOpen(v) {
  globalBuffsOpen = v;
}
export function setVerdictSuggestOpen(v) {
  verdictSuggestOpen = v;
}

/* ---------------- 状態の初期値・保存・URL共有 ---------------- */

export function defaultEnemy() {
  return { hp: 0, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 };
}

export function blankRow() {
  return {
    opId: "",
    entryIdx: 0,
    dmgType: "physical",
    potential: 5, // P8: 潜在ランク(0始まり。既定は潜在6=フル)
    elite: 2, // P6: オペレーター未選択の間は意味を持たない仮値(選択時にmakeDefaultRowが上書きする)
    level: 90,
    trust: 100,
    skillLevel: 10, // P7: スキルLv(1〜10。SLv1〜7+特化1〜3。既定は特化3)
    moduleId: null,
    moduleLv: 3,
    multiplier: 1,
    selfPct: 0,
    hits: 1,
    buffPct: 0,
    buffFlat: 0, // 手入力 基礎攻撃力+
    dmgMult: 1,
    ignoreDef: 0,
    buffIds: [], // P2: 個別バフ(行ごとに選ぶ)
    specialOn: true, // P2: 特殊強化トグル(デフォルトON)
    inspireOn: true, // P3: 鼓舞トグル(デフォルトON)
  };
}

// P3: 鼓舞ソースの現在の設定(state.inspire.sources[id] + 未設定分はデフォルト値)。
export function sourceCfg(source) {
  const stored = state.inspire && state.inspire.sources && state.inspire.sources[source.id];
  return { ...defaultInspireSourceCfg(source), ...(stored || {}) };
}

// P4/P5: source付きバフ(conditional/individual問わず)の現在の軸選択
// (state.globalBuffLevels[id] + 未設定分はb.source.defaults)。dropStaleRowsが起動時に
// 埋めるが、念のためここでもフォールバックする。
// P9: FK行リンク(そのバフのsource.operatorIdと同じオペレーターのFK行があれば、
// 昇進/潜在/モジュール/スキルLvはその行の設定を使う)込みの解決は`engine.js`の
// `effectiveBuffLevels`に一本化した(computeTotal等の計算側と表示側で判定がズレないため)。
export function buffLevels(b) {
  return effectiveBuffLevels(b, state, catalog);
}

// P9: バフの`source.operatorId`と一致するFK行が無い(=リンクしない)場合`null`。
// リンクしている場合、そのオペレーター(カタログのCatalogOperator)を返す
// (バフカードの「FK行（<name>）の設定を使用」表示に使う)。`effectiveBuffLevels`と同じく
// talent/skill/scaleのいずれも無い(段階のみ)ソースは対象外にする(表示と計算の判定を揃える)。
export function linkedOperatorFor(b) {
  const src = b.source;
  if (!src || !(src.talent || src.skill || src.scale)) return null;
  const row = findLinkedRow(b, state);
  return row ? findOperator(catalog, row.opId) : null;
}

export function setBuffLevels(buffId, patch) {
  if (!state.globalBuffLevels) state.globalBuffLevels = {};
  state.globalBuffLevels[buffId] = { ...(state.globalBuffLevels[buffId] || {}), ...patch };
}

// P3: computeTotal/findSingleTargetConflicts に渡す鼓舞ソースstate(id→cfg)。
export function inspireStates() {
  return (state.inspire && state.inspire.sources) || {};
}

// P4: computeTotal/computeInspireSource に渡すglobalBuffLevels(buffId→軸選択)。
// P9: FK行リンクを反映した解決済みmap(`computeEffectiveGlobalBuffLevels`)を返す。
export function buffLevelsState() {
  return computeEffectiveGlobalBuffLevels(catalog, state);
}

export function setSourceCfg(sourceId, patch) {
  if (!state.inspire) state.inspire = { sources: {} };
  if (!state.inspire.sources) state.inspire.sources = {};
  const source = (catalog.inspireSources || []).find((s) => s.id === sourceId);
  const current = state.inspire.sources[sourceId] || (source ? defaultInspireSourceCfg(source) : {});
  state.inspire.sources[sourceId] = { ...current, ...patch };
}

// 状態の保存先。普段はlocalStorageに自動保存し、アドレスバーのURLは書き換えない
// （開いた直後から#state=付きURLになると、ページ自体を共有したいときにそのまま
// 貼れないため）。#state=付きURLは「共有URLをコピー」を押したときだけ組み立てて
// クリップボードへ渡す。「ローカル保存+共有URLコピー」の考え方。
const STORAGE_KEY = "fkc-state-v1";

function parseStoredState(json) {
  if (!json) return null;
  const parsed = JSON.parse(json);
  if (!parsed || parsed.v !== 1 || !Array.isArray(parsed.rows) || !parsed.enemy) return null;
  const { state: cleaned, dropped } = dropStaleRows(parsed, catalog);
  if (dropped > 0) {
    showToast(`カタログに存在しないオペレーター/スキルが${dropped}件あったため取り除きました`);
  }
  return cleaned;
}

export function saveState() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch (e) {
      /* プライベートモード等で保存できなくても計算自体は続けられる */
    }
  }, 300);
}

export function loadSavedState() {
  try {
    return parseStoredState(localStorage.getItem(STORAGE_KEY));
  } catch (e) {
    return null;
  }
}

export function shareUrlForState() {
  const compressed = window.LZString.compressToEncodedURIComponent(JSON.stringify(state));
  return location.origin + location.pathname + "#state=" + compressed;
}

// 共有URL(#state=...)から開いた場合の読み込み。読み込んだらアドレスバーから
// #state=を消す（以後はlocalStorageが正本。手元の前回状態より共有リンクを優先する）。
export function takeStateFromSharedUrl() {
  const m = /^#state=(.+)$/.exec(location.hash);
  if (!m) return null;
  history.replaceState(null, "", location.pathname + location.search);
  try {
    return parseStoredState(window.LZString.decompressFromEncodedURIComponent(m[1]));
  } catch (e) {
    return null;
  }
}

/* ---------------- カタログ取得 ---------------- */

export async function fetchCatalog() {
  const res = await fetch("/FrameKillCalculator/catalog.json");
  return res.json();
}
