/* ============================================================
   フレームキル計算機 — 表現層（DOM描画・イベント配線・URL共有）

   計算そのものは ./engine.js（DOM非依存）に任せ、ここでは
   カタログ取得(fetch)・状態管理(localStorage保存)・DOM描画・共有URL(#state=...)の
   組み立て/読み込みだけを担当する。日本語専用ページなので文言はここに直書きする
   （lod_chest_solver/test_runnerのような多言語ページではないため、
   strings引数を受け取る設計にはしていない）。

   再描画方針: 構造が変わる操作(行の追加/削除/展開、select/checkbox、
   オペレーター確定)では#app配下を丸ごと作り直す(render。diffしない)。
   入力中の要素がinnerHTML置換で消えるとフォーカスを失うため、どの要素に
   フォーカスがあったかを data-field/data-idx で覚えておいて描画後に復元する
   （withPreservedFocus）。
   一方、数値・テキスト欄への打鍵(input/change)では入力欄そのものは作り直さず、
   そこから導出される表示(判定・折りたたみ行・計算式・↺バッジ)だけを差し替える
   （renderLive）。type="number"はselectionStartがnullでカーソル位置を復元できず、
   作り直すと毎打鍵カーソルが先頭へ戻る（"110"が"011"の順でしか打てない、
   "01"が1に丸められて"100"が打てない）ため。イベントはリスト側の要素に都度張り直すのではなく、
   #app 1箇所に委譲(delegation)することで再描画のたびの張り直しを不要にする。
   ============================================================ */

import {
  findOperator,
  findEntry,
  findModule,
  makeDefaultRow,
  resolveEntryValues,
  specialUiState,
  resolveSpecialCurrentValue,
  computeTotal,
  findSingleTargetConflicts,
  dropStaleRows,
  suggest,
  describeSuggestion,
  defaultInspireSourceCfg,
  computeInspireSource,
  computeInspireSelfParts,
  resolveConditionalValue,
  potentialGroups,
  maxEliteFor,
  maxLevelForElite,
  moduleUsable,
  effectiveModuleId,
  skillUnlockWarning,
  valueAtLevel,
  skillLevelWarning,
} from "./engine.js";

const ENEMY_PERCENT_FIELDS = new Set(["defPct", "vulnPct"]);
const ROW_PERCENT_FIELDS = new Set(["selfPct", "buffPct", "dmgMult"]);

const BAR_COLORS = ["#4f7cff", "#e8dcb0", "#79e2d0", "#d4794a", "#b48ef0", "#f0748a", "#7bd67b", "#f0a94e"];

let catalog = null;
let state = null;
let expandedIdx = null;
// 「② 全体バフ」の<details>開閉状態(P2)。render()は#app丸ごと作り直すため、
// <details>のネイティブなopen状態はDOM要素ごと消える。ここに覚えておいて
// renderGlobalBuffs()が毎回復元する(expandedIdxと同じ考え方)。
let globalBuffsOpen = false;
// P2 follow-up: 特殊強化のⓘ説明文が展開されている行のインデックス集合。
// render()は#app丸ごと作り直すため、ここに覚えておいて復元する(globalBuffsOpenと同じ考え方)。
const specialDescOpenIdx = new Set();
// P3: 鼓舞ソースのⓘ内訳が展開されているソースid集合(specialDescOpenIdxと同じ考え方)。
const inspireDescOpenIds = new Set();
let saveTimer = null;

/* ---------------- ユーティリティ ---------------- */

function $(id) {
  return document.getElementById(id);
}

function fmtInt(n) {
  const r = Math.round(n);
  return r.toLocaleString("ja-JP");
}

function fmtPct(fraction, digits = 0) {
  const v = fraction * 100;
  return (Math.round(v * 10 ** digits) / 10 ** digits).toString();
}

// 数値inputに詰める値。浮動小数の誤差(0.30000000000000004等)を丸めて見た目を整える。
function trimNum(n) {
  return Math.round(n * 1e6) / 1e6;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

function showToast(msg) {
  const container = $("toast-container");
  if (!container) return;
  const el = document.createElement("div");
  el.className = "toast";
  el.textContent = msg;
  container.appendChild(el);
  setTimeout(() => el.classList.add("toast-show"), 10);
  setTimeout(() => {
    el.classList.remove("toast-show");
    setTimeout(() => el.remove(), 300);
  }, 4000);
}

/* ---------------- 状態の初期値・保存・URL共有 ---------------- */

function defaultEnemy() {
  return { hp: 0, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 };
}

function blankRow() {
  return {
    opId: "",
    entryIdx: 0,
    dmgType: "physical",
    potential: true,
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
    dmgMult: 1,
    ignoreDef: 0,
    buffIds: [], // P2: 個別バフ(行ごとに選ぶ)
    specialOn: true, // P2: 特殊強化トグル(デフォルトON)
    inspireOn: true, // P3: 鼓舞トグル(デフォルトON)
  };
}

// P3: 鼓舞ソースの現在の設定(state.inspire.sources[id] + 未設定分はデフォルト値)。
function sourceCfg(source) {
  const stored = state.inspire && state.inspire.sources && state.inspire.sources[source.id];
  return { ...defaultInspireSourceCfg(source), ...(stored || {}) };
}

// P4/P5: source付きバフ(conditional/individual問わず)の現在の軸選択
// (state.globalBuffLevels[id] + 未設定分はb.source.defaults)。dropStaleRowsが起動時に
// 埋めるが、念のためここでもフォールバックする。
function buffLevels(b) {
  const d = (b.source && b.source.defaults) || {};
  const stored = (state.globalBuffLevels && state.globalBuffLevels[b.id]) || {};
  return {
    elite: d.elite ?? 2,
    potential: d.potential ?? 5,
    moduleId: d.moduleId ?? null,
    moduleLevel: d.moduleLevel ?? 3,
    skillLevel: d.skillLevel ?? 1,
    stageIndex: d.stageIndex ?? 1,
    toggleOn: false,
    ...stored,
  };
}

function setBuffLevels(buffId, patch) {
  if (!state.globalBuffLevels) state.globalBuffLevels = {};
  state.globalBuffLevels[buffId] = { ...(state.globalBuffLevels[buffId] || {}), ...patch };
}

// P3: computeTotal/findSingleTargetConflicts に渡す鼓舞ソースstate(id→cfg)。
function inspireStates() {
  return (state.inspire && state.inspire.sources) || {};
}

// P4: computeTotal/computeInspireSource に渡すglobalBuffLevels(buffId→軸選択)。
function buffLevelsState() {
  return state.globalBuffLevels || {};
}

function setSourceCfg(sourceId, patch) {
  if (!state.inspire) state.inspire = { sources: {} };
  if (!state.inspire.sources) state.inspire.sources = {};
  const source = (catalog.inspireSources || []).find((s) => s.id === sourceId);
  const current = state.inspire.sources[sourceId] || (source ? defaultInspireSourceCfg(source) : {});
  state.inspire.sources[sourceId] = { ...current, ...patch };
}

// 状態の保存先。普段はlocalStorageに自動保存し、アドレスバーのURLは書き換えない
// （開いた直後から#state=付きURLになると、ページ自体を共有したいときにそのまま
// 貼れないため）。#state=付きURLは「共有URLをコピー」を押したときだけ組み立てて
// クリップボードへ渡す。test_runnerの「ローカル保存+共有URLコピー」と同じ考え方。
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

function saveState() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch (e) {
      /* プライベートモード等で保存できなくても計算自体は続けられる */
    }
  }, 300);
}

function loadSavedState() {
  try {
    return parseStoredState(localStorage.getItem(STORAGE_KEY));
  } catch (e) {
    return null;
  }
}

function shareUrlForState() {
  const compressed = window.LZString.compressToEncodedURIComponent(JSON.stringify(state));
  return location.origin + location.pathname + "#state=" + compressed;
}

// 共有URL(#state=...)から開いた場合の読み込み。読み込んだらアドレスバーから
// #state=を消す（以後はlocalStorageが正本。手元の前回状態より共有リンクを優先する）。
function takeStateFromSharedUrl() {
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

async function fetchCatalog() {
  const res = await fetch("/FrameKillCalculator/catalog.json");
  return res.json();
}

/* ---------------- フォーカス保持付き再描画 ---------------- */

function withPreservedFocus(fn) {
  const active = document.activeElement;
  // 同じdata-fieldの欄は複数ある(例: 各バフカードのskillLevel/toggleOn)ので、
  // role/idx/buff-id/source-idまで一致する要素にフォーカスを戻す。
  const keys = ["role", "field", "idx", "buffId", "sourceId"];
  const attrs = {};
  if (active && active.dataset && active.dataset.field != null) {
    for (const k of keys) if (active.dataset[k] != null) attrs[k] = active.dataset[k];
  }
  const selStart = active && "selectionStart" in active ? active.selectionStart : null;
  const selEnd = active && "selectionEnd" in active ? active.selectionEnd : null;
  // #appを丸ごと差し替えると一瞬高さが変わってスクロール位置がずれることがあるので戻す。
  const scrollX = window.scrollX;
  const scrollY = window.scrollY;

  fn();

  if (attrs.field != null) {
    const toAttr = (k) => "data-" + k.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
    const selector = Object.entries(attrs)
      .map(([k, v]) => `[${toAttr(k)}="${CSS.escape(v)}"]`)
      .join("");
    const el = document.querySelector(selector);
    if (el) {
      // focus()の既定動作はその要素までスクロールするので止める。
      el.focus({ preventScroll: true });
      if (typeof el.setSelectionRange === "function" && selStart != null) {
        try {
          el.setSelectionRange(selStart, selEnd);
        } catch (e) {
          /* select等setSelectionRangeを持たない要素は無視 */
        }
      }
    }
  }
  if (window.scrollX !== scrollX || window.scrollY !== scrollY) window.scrollTo(scrollX, scrollY);
}

/* ---------------- 描画: 敵セクション ---------------- */

function renderEnemy() {
  const e = state.enemy;
  return `
  <section class="card" id="enemy-section">
    <h2>① 敵</h2>
    <div class="grid3">
      <label>HP<input type="number" id="enemy-hp" data-role="enemy" data-field="hp" data-idx="" value="${e.hp}" min="0"></label>
      <label>防御<input type="number" id="enemy-def" data-role="enemy" data-field="def" data-idx="" value="${e.def}" min="0"></label>
      <label>術耐性<input type="number" id="enemy-res" data-role="enemy" data-field="res" data-idx="" value="${e.res}" min="0" max="100"></label>
    </div>
    <details id="enemy-debuffs">
      <summary>敵へのデバフ（全員に適用）</summary>
      <div class="grid2">
        <label>防御 -固定<input type="number" data-role="enemy" data-field="defFlat" data-idx="" value="${e.defFlat}"></label>
        <label>防御 -%<input type="number" data-role="enemy" data-field="defPct" data-idx="" value="${fmtPct(e.defPct, 3)}"></label>
        <label>術耐性 -固定<input type="number" data-role="enemy" data-field="resFlat" data-idx="" value="${e.resFlat}"></label>
        <label>脆弱 +%<input type="number" data-role="enemy" data-field="vulnPct" data-idx="" value="${fmtPct(e.vulnPct, 3)}"></label>
      </div>
    </details>
  </section>`;
}

/* ---------------- 描画: オペレーター行 ---------------- */

function entryOptionLabel(entry) {
  const prefix = /^\d+$/.test(entry.skillNum) ? `S${entry.skillNum}` : entry.skillNum;
  const label = `${prefix} ${entry.skillLabel}`;
  return entry.variantLabel ? `${label} / ${entry.variantLabel}` : label;
}

// 棒グラフのセグメントと同じ色を使う（そのドット自体が凡例を兼ねる。UIラウンド2）。
function rowColor(idx) {
  return BAR_COLORS[idx % BAR_COLORS.length];
}

// "S3/400%"のような短いスキル参照。素質行(skill_numが数値でない)はそのまま出す。
function shortSkillRef(entry) {
  if (!entry) return "";
  const prefix = /^\d+$/.test(entry.skillNum) ? `S${entry.skillNum}` : entry.skillNum;
  return entry.variantLabel ? `${prefix}/${entry.variantLabel}` : prefix;
}

// 折りたたみ行のtitleツールチップ・棒グラフのtitle属性で共通に使うプレーンテキスト
// （HTMLタグを含まない。title属性にHTMLを入れてもエスケープされず表示されるだけなので、
// ここは常にプレーンテキストにする）。
function rowPlainSummary(row) {
  const op = findOperator(catalog, row.opId);
  if (!op) return "オペレーター未選択";
  const entry = findEntry(op, row.entryIdx);
  const { results } = computeTotal(catalog, [row], state.enemy, state.globalBuffIds, inspireStates(), buffLevelsState());
  const r = results[0];
  return `${op.name} ${eliteLevelLabel(row)} ${shortSkillRef(entry)}: ${fmtInt(r.perHit)}×${row.hits}Hit → 実ダメ ${fmtInt(r.rowDamage)}`;
}

// P6: "E2 Lv90"(信頼度100%は省略。それ以外は"信頼60%"のように付け足す)。
// P7: スキルLvが既定(特化3=10)でない場合だけ末尾に付け足す(例: "E2 Lv90 SLv7")。
function eliteLevelLabel(row) {
  let s = `E${row.elite} Lv${row.level}`;
  if (row.trust !== 100) s += ` 信頼${row.trust}%`;
  if ((row.skillLevel ?? 10) !== 10) s += ` ${skillLevelOptionLabel(row.skillLevel)}`;
  return s;
}

// バフN件(個別+適用中の条件付き)。0件ならバッジを出さない。
function buffCountBadge(r) {
  if (!r || !r.breakdown) return "";
  const n = (r.row.buffIds || []).length + r.breakdown.appliedConditional.length;
  if (n <= 0) return "";
  return `<span class="badge badge-buffcount" title="適用中のバフ数">バフ${n}</span>`;
}

// P3: 鼓舞が実際に適用されている(amount>0)行にだけ小さな「鼓舞」バッジを出す。
function inspireBadge(r) {
  if (!r || !r.inspireApplied) return "";
  return `<span class="badge badge-inspire" title="${escapeHtml(r.inspireApplied.sourceName)}の鼓舞+${fmtInt(r.inspireApplied.amount)}が適用中">鼓舞</span>`;
}

// 折りたたみ行の中身: [色ドット(凡例兼用)] [名前+短いスキル参照。省略可] [バフN。0件なら省略]
// … [ダメージ。省略不可・太字]。
// スキルの表示名(entry.skillLabel)は展開ビューにあるのでここには出さない（UIラウンド2）。
function renderRowSummary(row, idx) {
  const op = findOperator(catalog, row.opId);
  const dot = `<span class="row-color-dot" style="background:${rowColor(idx)}" aria-hidden="true"></span>`;
  if (!op) {
    return `${dot}<span class="row-summary-name row-summary-empty">オペレーター未選択</span>`;
  }
  const entry = findEntry(op, row.entryIdx);
  const { results } = computeTotal(catalog, [row], state.enemy, state.globalBuffIds, inspireStates(), buffLevelsState());
  const r = results[0];
  const tooltip = escapeHtml(rowPlainSummary(row));
  const nameRef = escapeHtml(`${op.name} ${shortSkillRef(entry)}`);
  const eliteBadge = `<span class="badge badge-elite" title="昇進・レベル・信頼度">${escapeHtml(eliteLevelLabel(row))}</span>`;
  return `${dot}<span class="row-summary-name" title="${tooltip}">${nameRef}</span>${eliteBadge}${buffCountBadge(r)}${inspireBadge(r)}`
    + `<span class="row-summary-damage" title="${tooltip}">${fmtInt(r.rowDamage)}</span>`;
}

function operatorDatalist() {
  return `<datalist id="operator-datalist">${catalog.operators
    .map((o) => `<option value="${escapeHtml(o.name)}">`)
    .join("")}</datalist>`;
}

function moduleOptions(op, row) {
  let html = `<option value="">なし</option>`;
  for (const m of op.modules) {
    const sel = row.moduleId === m.id ? " selected" : "";
    html += `<option value="${escapeHtml(m.id)}"${sel}>${escapeHtml(m.name)}（${m.typeName}）</option>`;
  }
  return html;
}

// P7: entry.multiplierCandidatesは{key, valuesByLevel}の配列(valuesByLevelはスキルLv別)。
// 現在のスキルLvでの値をvalueAtLevelで引いて表示する。
function multiplierCandidateOptions(entry, skillLevel) {
  let html = `<option value="">候補から選ぶ…</option>`;
  for (const { key, valuesByLevel } of entry.multiplierCandidates) {
    const value = valueAtLevel(valuesByLevel, skillLevel);
    html += `<option value="${value}">${escapeHtml(key)} (${trimNum(value)})</option>`;
  }
  return html;
}

// カタログ側の既定値(entry由来。P7: 現在のスキルLvでの値)。行のフィールドがユーザー操作で
// この値から変わっていれば「↺」リセットボタンを出す。entryが無ければ全てnull。
// P2 follow-up: 特殊強化はもう置き換え系を持たない(加算系/乗算系は都度計算する
// 別枠なので、ここでの既定値には影響しない)ため`specialOn`は見なくなった。
function catalogDefaults(entry, skillLevel) {
  if (!entry) return { multiplier: null, selfPct: null, hits: null, dmgType: null };
  const values = resolveEntryValues(entry, skillLevel);
  return { multiplier: values.multiplier, selfPct: values.selfPct, hits: values.hits, dmgType: values.dmgType };
}

// P7: 「補正」バッジの文言。固定(Manualかつスキルレベルに追従しない)なら「補正(特化3固定)」、
// スキルLvに追従するManual(multiplier_key/self_atk_pct_factor経由)ならただの「補正」。
function manualBadgeLabel(field, entry) {
  const fixedMap = { multiplier: entry.multiplierFixed, selfPct: entry.selfAtkPctFixed };
  return fixedMap[field] ? "補正(特化3固定)" : "補正";
}

function fieldBadges(entry, field, currentValue, idx, skillLevel) {
  if (!entry) return "";
  const sourceMap = { multiplier: entry.multiplier, selfPct: entry.selfAtkPct, hits: entry.hits, dmgType: entry.damageType };
  const src = sourceMap[field];
  // renderLiveが入力欄を作り直さずにバッジだけ差し替えられるよう、常にラッパーで包む。
  let html = `<span class="field-badges" data-badges-for="${field}" data-idx="${idx}">`;
  if (src && src.source === "manual") {
    const label = manualBadgeLabel(field, entry);
    html += `<span class="badge badge-manual" title="オーナーによる手動補正値">${escapeHtml(label)}</span>`;
  }
  const defaults = catalogDefaults(entry, skillLevel);
  const def = defaults[field];
  if (def !== null && def !== undefined && !valuesEqual(def, currentValue)) {
    html += `<button type="button" class="badge badge-reset" data-action="reset-field" data-field="${field}" data-idx="${idx}" title="カタログ既定値に戻す" aria-label="カタログ既定値に戻す">↺</button>`;
  }
  return html + `</span>`;
}

function valuesEqual(a, b) {
  if (typeof a === "number" && typeof b === "number") return Math.abs(a - b) < 1e-9;
  return a === b;
}

// entry.tagsを小さな地味なチップで並べる(P2)。
function renderTagsChips(entry) {
  if (!entry || !entry.tags || !entry.tags.length) return "";
  return `<div class="row-tags">${entry.tags.map((t) => `<span class="tag-chip">${escapeHtml(t)}</span>`).join("")}</div>`;
}

// 個別バフ(行ごとに選ぶ)のトグルチップ一覧(P2)。同じsingle_targetバフが他の行でも
// 選ばれていれば⚠を出す(singleConflicts: findSingleTargetConflictsの戻り値)。
function renderIndividualBuffChips(row, idx, singleConflicts) {
  const individual = (catalog.buffers || []).filter((b) => b.scope.type === "individual");
  if (!individual.length) return "";
  const chips = individual
    .map((b) => {
      const on = (row.buffIds || []).includes(b.id);
      // P5: sourceを持つ個別バフは「個別バフの育成設定」で選んだ現在値を表示する。
      const value = b.source ? resolveConditionalValue(b, buffLevels(b)) : b.value;
      const pctLabel = b.kind === "pct" ? `+${fmtPct(value)}%` : `+${trimNum(value)}`;
      const conflict = on && singleConflicts.has(b.id);
      const warn = conflict ? `<span class="chip-warn" title="単体対象のバフです。他の行でも選ばれています">⚠</span>` : "";
      return `<button type="button" class="chip" data-action="toggle-row-buff" data-idx="${idx}" data-buff-id="${escapeHtml(b.id)}" aria-pressed="${on}">${warn}${escapeHtml(b.name)} ${pctLabel}</button>`;
    })
    .join("");
  return `<div class="row-field"><label>個別バフ</label><div class="chip-row">${chips}</div></div>`;
}

// 全体でONの条件付きバフが、このエントリに適用されるか(✓)/されないか(–)の一覧(P2)。
// ON中の条件付きバフが1件も無ければ何も出さない。
function renderConditionalStatusLine(r) {
  if (!r || !r.breakdown) return "";
  const { appliedConditional, notAppliedConditional } = r.breakdown;
  if (!appliedConditional.length && !notAppliedConditional.length) return "";
  const parts = [
    ...appliedConditional.map((c) => `<span class="cond-applies">✓ ${escapeHtml(c.name)}</span>`),
    ...notAppliedConditional.map((c) => `<span class="cond-not-applies">– ${escapeHtml(c.name)}</span>`),
  ];
  return `<p class="row-conditional-status">条件付き: ${parts.join(" ")}</p>`;
}

// entry.special.requiresModule(加算系)が指すモジュールの表示名("X"等)をop.modulesから
// 引く。見つからなければ"X"にフォールバックする(該当モジュールは大抵種別Xのため)。
function moduleTypeNameFor(op, moduleId) {
  const m = op && op.modules.find((mm) => mm.id === moduleId);
  return m ? m.typeName : "X";
}

// 「特殊強化「<label>」はモジュール<X> Lv<N>以上で有効」ヒント文(加算系専用)。
// P6: モジュール自体が現在の昇進/レベルで装備できない間は、実データ(unlockPhase/
// unlockLevel)から「昇進<X> Lv<Y>以上で装備可能」を示す。装備はできているが
// モジュールLv側の条件(addSelfAtkPctByModuleLevelの最初の非ゼロ要素)を
// 満たさない間は従来どおり「モジュール<X> Lv<N>以上で有効」を示す。
function specialHintText(op, entry, row) {
  const sp = entry.special;
  const module = findModule(op, sp.requiresModule);
  const typeName = module ? module.typeName : moduleTypeNameFor(op, sp.requiresModule);
  if (module && !moduleUsable(module, row.elite, row.level)) {
    return `特殊強化「${sp.label}」はモジュール${typeName}（昇進${module.unlockPhase} Lv${module.unlockLevel}以上で装備可能）が必要`;
  }
  const arr = sp.addSelfAtkPctByModuleLevel || [];
  const nonZeroIdx = arr.findIndex((v) => v > 0);
  const minLv = nonZeroIdx >= 0 ? nonZeroIdx + 1 : 1;
  return `特殊強化「${sp.label}」はモジュール${typeName} Lv${minLv}以上で有効`;
}

// ⓘ説明文の末尾に付ける「現在: +N%」/「現在: ×N」(P2 follow-up)。
function specialCurrentValueText(op, entry, row) {
  const cur = resolveSpecialCurrentValue(op, entry, row);
  if (!cur) return "";
  return cur.kind === "mul" ? `現在: ×${trimNum(cur.value)}` : `現在: +${fmtPct(cur.value)}%`;
}

// 「特殊強化: <label>」チェックボックス + ⓘ説明文トグル(P2/P2 follow-up)。
// entry.specialが無いスキルは何も出さない。加算系(requiresModule)でモジュール条件を
// 満たさない間はチェックボックスの代わりにヒントを出す(乗算系はmulMultiplier.baseが
// 常に効くので出し分けしない。`specialUiState`が判定する)。
function renderSpecialCheckbox(op, entry, row, idx) {
  if (!entry || !entry.special) return "";
  const sp = entry.special;
  const uiState = specialUiState(op, entry, row);
  const descOpen = specialDescOpenIdx.has(idx);
  const descText = [sp.description, specialCurrentValueText(op, entry, row)].filter(Boolean).join(" / ");
  const infoBtn = sp.description
    ? `<button type="button" class="special-info-btn" data-action="toggle-special-desc" data-idx="${idx}" aria-expanded="${descOpen}" title="${escapeHtml(descText)}">ⓘ</button>`
    : "";
  const descBlock = descOpen && sp.description ? `<p class="special-desc">${escapeHtml(descText)}</p>` : "";

  if (uiState === "hint") {
    return `<div class="row-field"><p class="special-hint">${escapeHtml(specialHintText(op, entry, row))}${infoBtn}</p>${descBlock}</div>`;
  }
  return `<div class="row-field">
    <label class="check-label">
      <input type="checkbox" data-role="row" data-field="specialOn" data-idx="${idx}" ${row.specialOn !== false ? "checked" : ""}>
      特殊強化: ${escapeHtml(sp.label)}
    </label>${infoBtn}
    ${descBlock}
  </div>`;
}

// P3: 行の鼓舞トグル。鼓舞ソースが1件も無ければ何も出さない(specialのentry.special有無と
// 同じ考え方)。実際に適用中のソースがあればその名前をtitleで示す。
function renderRowInspireToggle(r, row, idx) {
  const sources = catalog.inspireSources || [];
  if (!sources.length) return "";
  const applied = r && r.inspireApplied;
  const amountText = applied ? `+${fmtInt(applied.amount)}` : "±0";
  const title = applied ? `${escapeHtml(applied.sourceName)}の鼓舞` : "現在ONになっている鼓舞ソースがありません";
  return `<div class="row-field">
    <label class="check-label" title="${title}">
      <input type="checkbox" data-role="row" data-field="inspireOn" data-idx="${idx}" ${row.inspireOn !== false ? "checked" : ""}>
      鼓舞: ${amountText} をこの行に適用
    </label>
  </div>`;
}

// P6: 昇進(0/1/2=E0/E1/E2)のセレクト選択肢。そのオペレーターが到達できる段階まで
// (`maxEliteFor`。フェーズが少ないオペレーターは選択肢自体が少ない)。
function eliteOptions(op, row) {
  const maxElite = maxEliteFor(op);
  let html = "";
  for (let e = 0; e <= maxElite; e++) {
    html += `<option value="${e}"${row.elite === e ? " selected" : ""}>E${e}</option>`;
  }
  return html;
}

// P6: 「[E2▾] Lv[ 90 ]/90  信頼度[100]%」の3列コントロール。昇進セレクトを変えると
// (onRowFieldChangeが)レベルをその昇進の最大値へリセットする。
function renderEliteLevelTrustControls(op, row, idx) {
  const maxLevel = maxLevelForElite(op, row.elite);
  return `<div class="row-grid3">
    <label>昇進
      <select data-role="row" data-field="elite" data-idx="${idx}">${eliteOptions(op, row)}</select>
    </label>
    <label>Lv
      <div class="level-with-max">
        <input type="number" step="1" min="1" max="${maxLevel}" data-role="row" data-field="level" data-idx="${idx}" value="${row.level}">
        <span class="level-max-hint">/${maxLevel}</span>
      </div>
    </label>
    <label>信頼度%
      <input type="number" step="1" min="0" max="100" data-role="row" data-field="trust" data-idx="${idx}" value="${row.trust}">
    </label>
  </div>`;
}

// P6: entryのスキルが現在の昇進でまだ解放されていない場合の警告(計算は続行する)。
function renderSkillUnlockWarning(op, entry, row) {
  if (!op || !entry) return "";
  const warning = skillUnlockWarning(op, entry, row);
  if (!warning) return "";
  return `<p class="special-hint">${escapeHtml(warning)}（計算は続行されます）</p>`;
}

// P7: スキルLv(1〜n)のセレクト選択肢。nは`entry.multiplierByLevel`の長さ(データが
// 10未満のスキルにも対応)。
function skillLevelOptionsN(n, current) {
  // Lv数の少ないスキル(例: 7段階)では、計算側のクランプと同じく最大Lvを選択表示にする。
  current = Math.min(Math.max(current, 1), n);
  let html = "";
  for (let lv = 1; lv <= n; lv++) {
    html += `<option value="${lv}"${current === lv ? " selected" : ""}>${skillLevelOptionLabel(lv)}</option>`;
  }
  return html;
}

// P7: `elite`/`skillLevel`の組み合わせから、まだ解放されていないスキルLvを選んでいる
// 場合の警告(「特化は昇進2で解放」/「SLv5以上は昇進1で解放」)。計算は続行する
// (skillUnlockWarningと同じ方針)。`cfgLike`は`{elite, skillLevel}`を持つrow/inspireのcfg。
function renderSkillLevelWarning(cfgLike) {
  const warning = skillLevelWarning(cfgLike.elite, cfgLike.skillLevel ?? 10);
  if (!warning) return "";
  return `<p class="special-hint">${escapeHtml(warning)}（計算は続行されます）</p>`;
}

// P7: 行のスキルLvセレクト(FK対象と同じ行の右側に狭めに置く)。選択肢の数は
// entry.multiplierByLevelの長さに合わせる(無ければ10=SLv1〜特化3のフルセットにフォールバック)。
function renderSkillLevelSelect(entry, row, idx) {
  const n = (entry.multiplierByLevel && entry.multiplierByLevel.length) || 10;
  return `<label class="skill-level-field">スキルLv
      <select data-role="row" data-field="skillLevel" data-idx="${idx}">${skillLevelOptionsN(n, row.skillLevel ?? 10)}</select>
    </label>`;
}

// P6: 選択中のモジュールが現在の昇進/レベルでは装備できない場合のヒント
// (実データのunlockPhase/unlockLevelから組み立てる。ATKには加算されない)。
function renderModuleUnusableHint(op, row) {
  if (!op || !row.moduleId) return "";
  const module = findModule(op, row.moduleId);
  if (!module || moduleUsable(module, row.elite, row.level)) return "";
  return `<p class="special-hint">モジュール${escapeHtml(module.typeName)}は昇進${module.unlockPhase} Lv${module.unlockLevel}以上で装備可能（現在は加算されません）</p>`;
}

function renderRowExpanded(row, idx, singleConflicts) {
  const op = findOperator(catalog, row.opId);
  const entry = op ? findEntry(op, row.entryIdx) : null;
  const { results } = computeTotal(catalog, [row], state.enemy, state.globalBuffIds, inspireStates(), buffLevelsState());
  const r = results[0];

  let entrySelectHtml = `<select data-role="row" data-field="entryIdx" data-idx="${idx}" ${op ? "" : "disabled"}>`;
  if (op) {
    entrySelectHtml += op.fkEntries
      .map((e, i) => `<option value="${i}"${i === row.entryIdx ? " selected" : ""}>${escapeHtml(entryOptionLabel(e))}</option>`)
      .join("");
  }
  entrySelectHtml += `</select>`;

  const moduleDisabled = !op || !op.modules.length ? "disabled" : "";
  const formula = op && entry ? renderFormulaLine(op, row) : "";
  const fkInfo = entry
    ? `<details class="row-fkinfo">
        <summary>FK情報</summary>
        <div class="fk-meta">
          <span>FK数: ${escapeHtml(entry.fkNum || "-")}</span>
          <span>ブレ: ${escapeHtml(entry.fkErr || "-")}</span>
          <span>最終更新: ${escapeHtml(entry.lastEdited || "-")}</span>
        </div>
        <pre class="fk-detail">${escapeHtml(entry.detail || "")}</pre>
      </details>`
    : "";

  return `
    <div class="row-field">
      <label>オペレーター
        <input type="text" list="operator-datalist" data-role="row" data-field="opName" data-idx="${idx}"
               value="${escapeHtml(op ? op.name : "")}" placeholder="名前を入力…">
      </label>
    </div>
    <div class="row-field">
      <div class="entry-with-skill-level">
        <label class="entry-field">FK対象 ${entrySelectHtml}</label>
        ${entry ? renderSkillLevelSelect(entry, row, idx) : ""}
      </div>
      ${renderSkillUnlockWarning(op, entry, row)}
      ${entry ? renderSkillLevelWarning(row) : ""}
    </div>
    ${op ? renderEliteLevelTrustControls(op, row, idx) : ""}
    <div class="row-grid2">
      <label>ダメージ種別
        <select data-role="row" data-field="dmgType" data-idx="${idx}">
          <option value="physical"${row.dmgType === "physical" ? " selected" : ""}>物理</option>
          <option value="arts"${row.dmgType === "arts" ? " selected" : ""}>術</option>
          <option value="true"${row.dmgType === "true" ? " selected" : ""}>真</option>
        </select>
        ${fieldBadges(entry, "dmgType", row.dmgType, idx, row.skillLevel)}
      </label>
      <label class="check-label">
        <input type="checkbox" data-role="row" data-field="potential" data-idx="${idx}" ${row.potential ? "checked" : ""}>
        攻撃凸
      </label>
    </div>
    ${renderTagsChips(entry)}
    <div class="row-grid2">
      <label>モジュール
        <select data-role="row" data-field="moduleId" data-idx="${idx}" ${moduleDisabled}>${op ? moduleOptions(op, row) : '<option value="">なし</option>'}</select>
      </label>
      <label>Lv
        <select data-role="row" data-field="moduleLv" data-idx="${idx}" ${row.moduleId ? "" : "disabled"}>
          <option value="1"${row.moduleLv === 1 ? " selected" : ""}>1</option>
          <option value="2"${row.moduleLv === 2 ? " selected" : ""}>2</option>
          <option value="3"${row.moduleLv === 3 ? " selected" : ""}>3</option>
        </select>
      </label>
    </div>
    <div class="module-hint-wrap" data-role="module-hint" data-idx="${idx}">${renderModuleUnusableHint(op, row)}</div>
    <div class="row-grid2">
      <label>倍率
        <input type="number" step="any" data-role="row" data-field="multiplier" data-idx="${idx}" value="${trimNum(row.multiplier)}">
        ${fieldBadges(entry, "multiplier", row.multiplier, idx, row.skillLevel)}
      </label>
      <label>倍率候補
        <select data-role="row" data-field="multiplierCandidate" data-idx="${idx}" ${entry && entry.multiplierCandidates.length ? "" : "disabled"}>
          ${entry ? multiplierCandidateOptions(entry, row.skillLevel) : '<option value="">候補から選ぶ…</option>'}
        </select>
      </label>
    </div>
    <div class="row-grid2">
      <label>セルフ%
        <input type="number" step="any" data-role="row" data-field="selfPct" data-idx="${idx}" value="${fmtPct(row.selfPct, 3)}">
        ${fieldBadges(entry, "selfPct", row.selfPct, idx, row.skillLevel)}
      </label>
      <label>Hit数
        <input type="number" step="1" min="0" data-role="row" data-field="hits" data-idx="${idx}" value="${row.hits}">
        ${fieldBadges(entry, "hits", row.hits, idx, row.skillLevel)}
      </label>
    </div>
    ${renderIndividualBuffChips(row, idx, singleConflicts)}
    ${renderConditionalStatusLine(r)}
    ${renderSpecialCheckbox(op, entry, row, idx)}
    ${renderRowInspireToggle(r, row, idx)}
    <div class="row-grid2">
      <label>手入力バフ+%
        <input type="number" step="any" data-role="row" data-field="buffPct" data-idx="${idx}" value="${fmtPct(row.buffPct, 3)}">
      </label>
      <label>ダメージ倍率%
        <input type="number" step="any" data-role="row" data-field="dmgMult" data-idx="${idx}" value="${fmtPct(row.dmgMult, 3)}">
      </label>
    </div>
    <div class="row-field">
      <label>防御無視（固定値）
        <input type="number" step="any" data-role="row" data-field="ignoreDef" data-idx="${idx}" value="${row.ignoreDef}">
      </label>
    </div>
    <p class="row-formula" data-role="row-formula" data-idx="${idx}">${formula}</p>
    ${fkInfo}
    <div class="row-actions-expanded">
      <button type="button" data-action="collapse-row" data-idx="${idx}">折りたたむ</button>
    </div>
  `;
}

// フォーミュラ行(P2/P3): `691 ×(1 + セルフ0% + 個別150% + 条件0% + 手入力0%) × 400% = 6,910 /hit`。
// 鼓舞(flat種バフ + P3の鼓舞ソースからの加算の合計)は0でない時だけ足す(仕様どおり)。
function renderFormulaLine(op, row) {
  const entry = findEntry(op, row.entryIdx);
  const { atk, final, perHit, rowDamage, atFloor, breakdown, specialAddPct, specialMulFactor, inspireApplied } = computeTotal(
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
  const multiplierPart =
    specialMulFactor !== 1 ? `${fmtPct(row.multiplier)}% × ×${trimNum(specialMulFactor)}(${specialLabel})` : `${fmtPct(row.multiplier)}%`;
  // 鼓舞(固定値)は%適用後・倍率の前に足すので、鼓舞がある時は外側を括弧で囲んで
  // 「鼓舞にだけ倍率が掛かる」ように読めないようにする。
  const atkPart = `${fmtInt(atk)} ×(1 + ${selfPart} + ${individualPart} + ${conditionalPart} + ${manualPart})`;
  const beforeMultiplier = inspirePart ? `(${atkPart}${inspirePart})` : atkPart;
  const line1 = `${beforeMultiplier} × ${multiplierPart} = ${fmtInt(final)} /hit`;
  const floorNote = atFloor ? `<span class="floor-note">（5%floor発動中）</span>` : "";
  const line2 = `→ 実ダメ ${fmtInt(perHit)}/hit × ${row.hits}Hit = <b>${fmtInt(rowDamage)}</b> ${floorNote}`;
  return `${escapeHtml(line1)}<br>${line2}`;
}

/* ---------------- 描画: 鼓舞(インスパイア)ソース(P3) ---------------- */

// entry.tagsと同様、source.modulesから種別名("X"等)を引く(見つからなければ"?")。
function moduleTypeNameForSource(source, moduleId) {
  const m = source && source.modules.find((mm) => mm.id === moduleId);
  return m ? m.typeName : "?";
}

// self_partsのうち常時ONでないもの(トグル可能/モジュール条件付き)を並べる。
// requiresModuleを持つが現在のモジュール選択と一致しない間はP2の特殊強化ヒントと
// 同じ考え方でヒント文に切り替える(チェックボックスを隠す)。
function renderInspireSelfPartsControls(source, cfg) {
  const parts = source.selfParts || [];
  // P6: モジュール条件は`effectiveModuleId`(elite/level込みの装備可否)経由で判定する
  // (computeInspireSelfPartsと同じ判定を使う。未装備/装備不可の間はヒント側に回る)。
  const moduleId = effectiveModuleId(source, cfg);
  let html = "";
  for (const p of parts) {
    if (p.alwaysOn) continue;
    const applicable = !p.requiresModule || moduleId === p.requiresModule;
    if (!applicable) {
      const typeName = moduleTypeNameForSource(source, p.requiresModule);
      html += `<p class="special-hint" title="${escapeHtml(p.description || "")}">「${escapeHtml(p.label)}」はモジュール${escapeHtml(typeName)}装備時のみ有効</p>`;
      continue;
    }
    const stored = cfg.parts && cfg.parts[p.id];
    const checked = stored !== undefined ? !!stored : !!p.defaultOn;
    html += `<label class="check-label" title="${escapeHtml(p.description || "")}">
      <input type="checkbox" data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="part:${escapeHtml(p.id)}" ${checked ? "checked" : ""}>
      ${escapeHtml(p.label)}
    </label>`;
  }
  return html;
}

// 個別バフチップ(行と同じ一覧を鼓舞ソース用に描く。スペック変更で追加: ソース自身にも
// 個別バフを乗せられる。行と同じsingle_target⚠判定[singleConflicts]を共有する)。
function renderSourceIndividualBuffChips(source, cfg, singleConflicts) {
  const individual = (catalog.buffers || []).filter((b) => b.scope.type === "individual");
  if (!individual.length) return "";
  const chips = individual
    .map((b) => {
      const on = (cfg.buffIds || []).includes(b.id);
      const value = b.source ? resolveConditionalValue(b, buffLevels(b)) : b.value;
      const pctLabel = b.kind === "pct" ? `+${fmtPct(value)}%` : `+${trimNum(value)}`;
      const conflict = on && singleConflicts.has(b.id);
      const warn = conflict ? `<span class="chip-warn" title="単体対象のバフです。他でも選ばれています">⚠</span>` : "";
      return `<button type="button" class="chip" data-action="toggle-source-buff" data-source-id="${escapeHtml(source.id)}" data-buff-id="${escapeHtml(b.id)}" aria-pressed="${on}">${warn}${escapeHtml(b.name)} ${pctLabel}</button>`;
    })
    .join("");
  return `<div class="row-field"><label>個別バフ</label><div class="chip-row">${chips}</div></div>`;
}

// ⓘ内訳の1行: `480 ×(1 + 素質9% + X8% + 個別0% + 条件付き0% + 手入力0%) × 110% = 618`。
function describeInspireBreakdown(result) {
  const partsText = result.selfParts.applied.map((p) => `${p.shortLabel}${fmtPct(p.value)}%`).join(" + ") || "素質0%";
  const individualText = `個別${fmtPct(result.breakdown.individualPct)}%`;
  const conditionalText = `条件付き${fmtPct(result.breakdown.conditionalPct)}%`;
  const manualText = `手入力${fmtPct(result.manualPct)}%`;
  const ratioText = `${fmtPct(result.ratio)}%`;
  return `${fmtInt(result.atk)} ×(1 + ${partsText} + ${individualText} + ${conditionalText} + ${manualText}) × ${ratioText} = ${fmtInt(result.amount)}`;
}

// 結果行(→ 鼓舞 +314)+ⓘ展開ブロック。renderLiveが手入力バフ+%の打鍵中に
// innerHTMLごと差し替えられるよう、常にラッパー(`.inspire-result-wrap`)で包む
// (row-formulaと同じ考え方)。
function renderInspireResultBlock(source, cfg, result) {
  const descOpen = inspireDescOpenIds.has(source.id);
  const breakdownText = describeInspireBreakdown(result);
  const infoBtn = `<button type="button" class="special-info-btn" data-action="toggle-inspire-desc" data-source-id="${escapeHtml(source.id)}" aria-expanded="${descOpen}" title="${escapeHtml(breakdownText)}">ⓘ</button>`;
  const descBlock = descOpen ? `<p class="special-desc">${escapeHtml(breakdownText)}</p>` : "";
  return `<p class="inspire-result">→ 鼓舞 +${fmtInt(result.amount)}${infoBtn}</p>${descBlock}`;
}

// 鼓舞ソース1件分のカード。OFFの間はトグルチップだけ。ONになると
// スキル/攻撃凸/素質凸/モジュール/自己%パーツ/個別バフ/手入力バフ+結果行を表示する。
function renderInspireSourceCard(source, singleConflicts) {
  const cfg = sourceCfg(source);
  const toggleChip = `<button type="button" class="chip" data-action="toggle-inspire-source" data-source-id="${escapeHtml(source.id)}" aria-pressed="${cfg.on}">${escapeHtml(source.name)}</button>`;
  if (!cfg.on) {
    return `<div class="inspire-source-card">${toggleChip}</div>`;
  }

  const result = computeInspireSource(source, cfg, catalog, state.globalBuffIds, buffLevelsState());
  const skillOptions = source.skills
    .map((s) => {
      const label = /^\d+$/.test(s.skillNum) ? `S${s.skillNum}` : s.skillNum;
      return `<option value="${escapeHtml(s.skillNum)}"${s.skillNum === cfg.skillNum ? " selected" : ""}>${escapeHtml(label)}</option>`;
    })
    .join("");
  // P7: 現在選択中のスキルのratioByLevelの長さでスキルLvの選択肢数を決める。
  const currentSkill = source.skills.find((s) => s.skillNum === cfg.skillNum) || source.skills[0];
  const skillLevelN = (currentSkill && currentSkill.ratioByLevel && currentSkill.ratioByLevel.length) || 10;

  return `
  <div class="inspire-source-card inspire-source-on" data-source-id="${escapeHtml(source.id)}">
    <div class="inspire-source-header">${toggleChip}</div>
    <div class="row-grid3">
      <label>スキル
        <select data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="skillNum">${skillOptions}</select>
      </label>
      <label>スキルLv
        <select data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="skillLevel">${skillLevelOptionsN(skillLevelN, cfg.skillLevel ?? 10)}</select>
      </label>
      <label class="check-label">
        <input type="checkbox" data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="potential" ${cfg.potential ? "checked" : ""}>
        攻撃凸
      </label>
    </div>
    ${renderSkillLevelWarning(cfg)}
    <label class="check-label">
      <input type="checkbox" data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="talentPotential" ${cfg.talentPotential ? "checked" : ""}>
      ${escapeHtml(source.talentPotentialLabel)}
    </label>
    <div class="row-grid3">
      <label>昇進
        <select data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="elite">${eliteOptions(source, cfg)}</select>
      </label>
      <label>Lv
        <div class="level-with-max">
          <input type="number" step="1" min="1" max="${maxLevelForElite(source, cfg.elite)}" data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="level" value="${cfg.level}">
          <span class="level-max-hint">/${maxLevelForElite(source, cfg.elite)}</span>
        </div>
      </label>
      <label>信頼度%
        <input type="number" step="1" min="0" max="100" data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="trust" value="${cfg.trust}">
      </label>
    </div>
    <div class="row-grid2">
      <label>モジュール
        <select data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="moduleId">${moduleOptions(source, cfg)}</select>
      </label>
      <label>Lv
        <select data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="moduleLv" ${cfg.moduleId ? "" : "disabled"}>
          <option value="1"${cfg.moduleLv === 1 ? " selected" : ""}>1</option>
          <option value="2"${cfg.moduleLv === 2 ? " selected" : ""}>2</option>
          <option value="3"${cfg.moduleLv === 3 ? " selected" : ""}>3</option>
        </select>
      </label>
    </div>
    <div class="module-hint-wrap" data-role="inspire-module-hint" data-source-id="${escapeHtml(source.id)}">${renderModuleUnusableHint(source, cfg)}</div>
    ${renderInspireSelfPartsControls(source, cfg)}
    ${renderSourceIndividualBuffChips(source, cfg, singleConflicts)}
    <div class="row-field">
      <label>手入力バフ+%
        <input type="number" step="any" data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="buffPct" value="${fmtPct(cfg.buffPct, 3)}">
      </label>
    </div>
    <div class="inspire-result-wrap" data-role="inspire-result" data-source-id="${escapeHtml(source.id)}">${renderInspireResultBlock(source, cfg, result)}</div>
  </div>`;
}

// P4: 潜在ラベル("潜在1"〜"潜在6")。0始まりのpotentialRankをそのままindexに使う。
function potentialOptionLabel(group) {
  return group.from === group.to ? `潜在${group.from + 1}` : `潜在${group.from + 1}-${group.to + 1}`;
}

// P4: スキルLvラベル("1"〜"7" + "特化1"〜"特化3")。Lv1始まりのskillLevelをそのままindexに使う。
function skillLevelOptionLabel(skillLevel) {
  return skillLevel <= 7 ? `SLv${skillLevel}` : `特化${skillLevel - 7}`;
}

// P4/P5: `source`(素質/スキルLv/スケール/固定基礎値/段階由来の動的値解決)を持つバフの
// 「値が変わる軸」だけをインラインの<select>で出す共通ヘルパー(dedupe済み。値が変わらない
// 軸は`b.source`側に無いので出ない)。conditionalのカード(renderConditionalSourceCard)と
// individualの育成設定カード(renderIndividualBuffLevelCard)の両方から使う。戻り値の
// select/checkboxは全て`data-role="global-buff-level" data-buff-id="..."`という共通の
// 書式なので、スコープ(conditional/individual)を問わず`onGlobalBuffLevelChange`が処理する。
// `b.toggle`(例: 前衛アーミヤの「スキル中は効果2倍」、ステインレスS1の「装置2台」)が
// あればチェックボックスも出す。
function renderBuffAxisControls(b, levels) {
  const value = resolveConditionalValue(b, levels);
  const source = b.source;
  const talent = source.talent;
  const skill = source.skill;
  const scale = source.scale;
  const stage = source.stage;

  let controls = "";
  let hint = "";
  if (talent) {
    if (talent.eliteVaries) {
      const opts = [0, 1, 2]
        .map((e) => `<option value="${e}"${levels.elite === e ? " selected" : ""}>E${e}</option>`)
        .join("");
      controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="elite">${opts}</select>`;
    }
    if (talent.potentialVaries) {
      // 値が変わる境目だけを選択肢にする(例: 潜在1-5/潜在6)。option値はグループ内の最大潜在。
      const opts = potentialGroups(talent)
        .map((g) => {
          const selected = levels.potential >= g.from && levels.potential <= g.to;
          return `<option value="${g.to}"${selected ? " selected" : ""}>${potentialOptionLabel(g)}</option>`;
        })
        .join("");
      controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="potential">${opts}</select>`;
    }
    // モジュールは昇進2でしか装備できないので、E0/E1の間はモジュール欄を出さない。
    const minElite = talent.valuesByEliteAndPotential.findIndex((byPot) => (byPot[levels.potential] ?? 0) > 0);
    if (value <= 0 && minElite > levels.elite) {
      hint = `<p class="special-hint">「${escapeHtml(b.name)}」の素質は昇進${minElite}で解放</p>`;
    } else if (talent.modules.length && levels.elite >= 2) {
      const moduleOpts =
        `<option value=""${!levels.moduleId ? " selected" : ""}>なし</option>` +
        talent.modules
          .map((m) => `<option value="${escapeHtml(m.moduleId)}"${levels.moduleId === m.moduleId ? " selected" : ""}>${escapeHtml(m.typeName)}</option>`)
          .join("");
      controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="moduleId">${moduleOpts}</select>`;
      if (levels.moduleId) {
        const lvOpts = [1, 2, 3]
          .map((lv) => `<option value="${lv}"${levels.moduleLevel === lv ? " selected" : ""}>Lv${lv}</option>`)
          .join("");
        controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="moduleLevel">${lvOpts}</select>`;
      } else if (value <= 0) {
        // スズランのように、モジュール未装備だと値が0(=効果なし)のケースのヒント。
        const m = talent.modules[0];
        hint = `<p class="special-hint">「${escapeHtml(b.name)}」はモジュール${escapeHtml(m.typeName)}装備時のみ有効</p>`;
      }
    }
  }
  if (skill) {
    const opts = skill.valuesByLevel
      .map((_, i) => {
        const lv = i + 1;
        return `<option value="${lv}"${levels.skillLevel === lv ? " selected" : ""}>${skillLevelOptionLabel(lv)}</option>`;
      })
      .join("");
    controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="skillLevel">${opts}</select>`;
  }
  // P5: talent/base_pctに掛け合わせるスキルLv別スケール。全レベルで値が同じ(varies=false。
  // 例: スワイヤーS1のtalent_scale)ならセレクトを出さない。
  if (scale && scale.varies) {
    const opts = scale.valuesByLevel
      .map((_, i) => {
        const lv = i + 1;
        return `<option value="${lv}"${levels.skillLevel === lv ? " selected" : ""}>${skillLevelOptionLabel(lv)}</option>`;
      })
      .join("");
    controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="skillLevel">${opts}</select>`;
  }
  // P5: スキルLvではなく離散段階(例: ナスティS3の装置アップグレード段階)。
  if (stage) {
    const opts = stage.values
      .map((_, i) => {
        const idx = i + 1;
        const label = stage.labels[i] ?? `${idx}`;
        return `<option value="${idx}"${levels.stageIndex === idx ? " selected" : ""}>${escapeHtml(label)}</option>`;
      })
      .join("");
    controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="stageIndex">${opts}</select>`;
  }
  if (b.toggle) {
    controls += `<label class="check-label cond-toggle-label">
      <input type="checkbox" data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="toggleOn" ${levels.toggleOn ? "checked" : ""}>
      ${escapeHtml(b.toggle.label)}
    </label>`;
  }

  return { controls, hint, value };
}

// P4: `source`を持つ条件付きバフ1件分のカード。OFFの間はトグルチップだけ、ONになると
// `renderBuffAxisControls`が返す軸コントロールを表示する。
function renderConditionalSourceCard(b) {
  const onIds = state.globalBuffIds || [];
  const on = onIds.includes(b.id);
  const targetLabel = b.scope.targetTags.join("/");
  const toggleChip = `<button type="button" class="chip" data-action="toggle-global-buff" data-buff-id="${escapeHtml(b.id)}" aria-pressed="${on}">${escapeHtml(b.name)} ${escapeHtml(targetLabel)}</button>`;
  if (!on) {
    return `<div class="cond-source-card">${toggleChip}</div>`;
  }

  const levels = buffLevels(b);
  const { controls, hint, value } = renderBuffAxisControls(b, levels);
  const valueLabel = b.kind === "pct" ? `+${fmtPct(value)}%` : `+${trimNum(value)}`;

  return `<div class="cond-source-card cond-source-on" data-buff-id="${escapeHtml(b.id)}">
    <div class="cond-source-header">${toggleChip}<span class="cond-source-value">${valueLabel}</span></div>
    ${hint}
    <div class="cond-source-controls">${controls}</div>
  </div>`;
}

// P5: `source`を持つ個別バフ1件分の「育成設定」カード。individualバフはconditionalと
// 違いスコープ自体のON/OFFチップを持たない(行/鼓舞ソースのチップで選ぶため)ので、
// トグルチップの代わりに名前をそのまま見出しにする。
function renderIndividualBuffLevelCard(b) {
  const levels = buffLevels(b);
  const { controls, hint, value } = renderBuffAxisControls(b, levels);
  const valueLabel = b.kind === "pct" ? `+${fmtPct(value)}%` : `+${trimNum(value)}`;
  return `<div class="cond-source-card cond-source-on" data-buff-id="${escapeHtml(b.id)}">
    <div class="cond-source-header"><span class="cond-source-name">${escapeHtml(b.name)}</span><span class="cond-source-value">${valueLabel}</span></div>
    ${hint}
    <div class="cond-source-controls">${controls}</div>
  </div>`;
}

// P5: 行(row.buffIds)/鼓舞ソース(ONのcfg.buffIds)のどちらかで現在選ばれている
// バフidの集合(育成設定カードの表示対象を絞るために使う)。
function usedIndividualBuffIds() {
  const used = new Set();
  for (const row of state.rows) for (const id of row.buffIds || []) used.add(id);
  const sources = (state.inspire && state.inspire.sources) || {};
  for (const cfg of Object.values(sources)) {
    if (!cfg || !cfg.on) continue;
    for (const id of cfg.buffIds || []) used.add(id);
  }
  return used;
}

// 「個別バフの育成設定」セクション(P5)。sourceを持つ個別バフのうち、行または鼓舞ソースで
// 現在チェックされているものだけをカード表示する(オーナー方針:
// 「チェックしたやつの育成状況を設定出来るようにすればOK」)。1件も無ければヒントのみ。
function renderIndividualBuffLevelsSection() {
  const sourced = (catalog.buffers || []).filter((b) => b.scope.type === "individual" && b.source);
  if (!sourced.length) return "";
  const usedIds = usedIndividualBuffIds();
  const relevant = sourced.filter((b) => usedIds.has(b.id));
  const body = relevant.length
    ? relevant.map((b) => renderIndividualBuffLevelCard(b)).join("")
    : `<p class="section-empty-hint">行やソースで個別バフを選ぶと、ここで育成状況(昇進・潜在・モジュール・スキルLv等)を設定できます。</p>`;
  return `<section class="card" id="individual-buff-levels-section">
    <h2>個別バフの育成設定</h2>
    ${body}
  </section>`;
}

// 「② 全体バフ（条件付き）」セクション。summaryに"N件ON"を出す(仕様どおり。P3で
// 鼓舞ソースのON件数と合計量も合算して出す)。
function renderGlobalBuffs() {
  const conditional = (catalog.buffers || []).filter((b) => b.scope.type === "conditional");
  // P4: 固定値(pct/flat)のconditionalは従来どおりチップ1行に並べ、`source`付き
  // (ゲームデータから動的解決するもの)は選択軸を出せるカード形式にする。
  const fixedConditional = conditional.filter((b) => !b.source);
  const sourcedConditional = conditional.filter((b) => b.source);
  const onIds = state.globalBuffIds || [];
  const onCount = conditional.filter((b) => onIds.includes(b.id)).length;
  const chips = fixedConditional
    .map((b) => {
      const on = onIds.includes(b.id);
      const pctLabel = b.kind === "pct" ? `+${fmtPct(b.value)}%` : `+${trimNum(b.value)}`;
      const targetLabel = b.scope.targetTags.join("/");
      return `<button type="button" class="chip" data-action="toggle-global-buff" data-buff-id="${escapeHtml(b.id)}" aria-pressed="${on}">${escapeHtml(b.name)} ${escapeHtml(targetLabel)} ${pctLabel}</button>`;
    })
    .join("");
  const sourcedHtml = sourcedConditional.map((b) => renderConditionalSourceCard(b)).join("");

  const inspireSources = catalog.inspireSources || [];
  const singleConflicts = findSingleTargetConflicts(catalog, state.rows, inspireStates(), buffLevelsState());
  const onSourceCfgs = inspireSources.map((s) => sourceCfg(s)).filter((cfg) => cfg.on);
  const inspireTotal = inspireSources.reduce((sum, s) => {
    const cfg = sourceCfg(s);
    return cfg.on ? sum + computeInspireSource(s, cfg, catalog, state.globalBuffIds, buffLevelsState()).amount : sum;
  }, 0);
  const summaryCount = onCount + onSourceCfgs.length;
  const inspireSummarySuffix = onSourceCfgs.length ? `・鼓舞+${fmtInt(inspireTotal)}` : "";
  const inspireHtml = inspireSources.length
    ? `<div class="inspire-block"><p class="inspire-heading">── 鼓舞 ──</p>${inspireSources.map((s) => renderInspireSourceCard(s, singleConflicts)).join("")}</div>`
    : "";

  return `
  <section class="card" id="global-buffs-section">
    <h2>② 全体バフ（条件付き）</h2>
    <details id="global-buffs-details"${globalBuffsOpen ? " open" : ""}>
      <summary>${summaryCount}件ON${inspireSummarySuffix}</summary>
      <div class="chip-row">${chips}</div>
      ${sourcedHtml}
      ${inspireHtml}
    </details>
  </section>`;
}

function renderRows() {
  const singleConflicts = findSingleTargetConflicts(catalog, state.rows, inspireStates(), buffLevelsState());
  let html = `${operatorDatalist()}<section class="card" id="rows-section"><h2>③ FKするオペレーター</h2><div id="rows-list">`;
  state.rows.forEach((row, idx) => {
    const expanded = idx === expandedIdx;
    html += `<div class="row-card${expanded ? " row-expanded" : ""}" data-idx="${idx}">`;
    if (expanded) {
      html += renderRowExpanded(row, idx, singleConflicts);
    } else {
      html += `<div class="row-summary">
        <span class="row-summary-main">${renderRowSummary(row, idx)}</span>
        <span class="row-actions">
          <button type="button" class="row-edit-btn" data-action="edit-row" data-idx="${idx}" aria-label="編集">編集</button>
          <button type="button" data-action="dup-row" data-idx="${idx}" aria-label="複製">複製</button>
          <button type="button" data-action="del-row" data-idx="${idx}" aria-label="削除">×</button>
        </span>
      </div>`;
    }
    html += `</div>`;
  });
  html += `</div><button type="button" id="add-row-btn" data-action="add-row">＋ オペレーターを追加</button></section>`;
  return html;
}

/* ---------------- 描画: 判定セクション ---------------- */

function renderVerdict() {
  const { results, total, killed } = computeTotal(catalog, state.rows, state.enemy, state.globalBuffIds, inspireStates(), buffLevelsState());
  const hp = state.enemy.hp;
  const scale = Math.max(total, hp, 1);

  const segments = results
    .map((r, i) => {
      if (!r || r.rowDamage <= 0) return "";
      const widthPct = (r.rowDamage / scale) * 100;
      const color = rowColor(i);
      return `<div class="bar-seg" style="width:${widthPct}%;background:${color}" title="${escapeHtml(rowPlainSummary(r.row))}"></div>`;
    })
    .join("");
  const hpLinePct = Math.min((hp / scale) * 100, 100);

  let verdictHtml;
  if (hp <= 0) {
    // HP未入力(0)で「撃破できる」と出すと誤解を招くので、入力を促すだけにする。
    verdictHtml = `<span class="verdict-pending">敵のHPを入力してください</span> (与ダメ合計 ${fmtInt(total)})`;
  } else if (killed) {
    const pct = hp > 0 ? Math.round((total / hp) * 100) : 100;
    const margin = fmtInt(total - hp);
    verdictHtml = `<span class="verdict-ok">✅ 撃破できる</span> (${fmtInt(total)} / ${fmtInt(hp)}, ${pct}%, 余裕 +${margin})`;
  } else {
    const deficit = fmtInt(hp - total);
    const pct = hp > 0 ? Math.round((total / hp) * 100) : 0;
    verdictHtml = `<span class="verdict-ng">❌ あと ${deficit} 足りない</span> (${pct}%)`;
  }

  let suggestionsHtml = "";
  if (!killed && state.rows.length > 0) {
    const suggestions = suggest(state, catalog);
    if (suggestions.length) {
      suggestionsHtml = `<div id="verdict-suggestions"><p class="suggest-title">撃破するには:</p><ul>${suggestions
        .map((s) => `<li>${escapeHtml(describeSuggestion(s, catalog, state.rows))}</li>`)
        .join("")}</ul></div>`;
    } else {
      // 上限(バフ+300%/Hit+3/防御・術耐性は敵の現在値まで)を超えないと撃破できない場合。
      // 非現実的な提案(「Hit数を+29増やす」等)を出すよりは、正直に諦めを伝える。
      const deficit = fmtInt(hp - total);
      suggestionsHtml = `<div id="verdict-suggestions"><p class="suggest-title">現実的な補正では届きません（あと ${deficit}）</p></div>`;
    }
  }

  return `
  <section id="verdict-section">
    <div id="verdict-text">${verdictHtml}</div>
    <div id="verdict-bar"><div class="bar-wrap">
      <div class="bar-track">${segments}</div>
      <div class="hp-line" style="left:${hpLinePct}%"><span class="hp-line-label">HP</span></div>
    </div></div>
    ${suggestionsHtml}
    <div class="verdict-actions">
      <button type="button" id="share-url-btn" data-action="share">共有URLをコピー</button>
    </div>
    <p class="scope-note">会心・確率発動・継続ダメージ・召喚物・オペ間デバフの順序依存は非対応。</p>
  </section>`;
}

/* ---------------- 全体再描画 ---------------- */

function render() {
  withPreservedFocus(() => {
    $("app").innerHTML = renderEnemy() + renderGlobalBuffs() + renderIndividualBuffLevelsSection() + renderRows() + renderVerdict();
  });
  saveState();
}

// 数値・テキスト欄の打鍵中の再描画。入力欄は触らず、導出表示だけを差し替える
// （理由はファイル冒頭の「再描画方針」参照）。
function renderLive() {
  state.rows.forEach((row, idx) => {
    const card = document.querySelector(`.row-card[data-idx="${idx}"]`);
    if (!card) return;
    if (idx === expandedIdx) {
      const op = findOperator(catalog, row.opId);
      const entry = op ? findEntry(op, row.entryIdx) : null;
      const formula = card.querySelector(".row-formula");
      if (formula) formula.innerHTML = op && entry ? renderFormulaLine(op, row) : "";
      card.querySelectorAll(".field-badges").forEach((badges) => {
        const field = badges.dataset.badgesFor;
        badges.outerHTML = fieldBadges(entry, field, row[field], idx, row.skillLevel);
      });
      // P6: レベル入力の打鍵中でもモジュール装備可否ヒントが即座に追従するよう
      // 差し替える(elite/moduleIdの変更は既にrender()で全体を作り直す)。
      const moduleHint = card.querySelector('[data-role="module-hint"]');
      if (moduleHint) moduleHint.innerHTML = op ? renderModuleUnusableHint(op, row) : "";
    } else {
      const main = card.querySelector(".row-summary-main");
      if (main) main.innerHTML = renderRowSummary(row, idx);
    }
  });
  // P3: ONになっている鼓舞ソースの結果行(→ 鼓舞 +N)も打鍵のたびに差し替える
  // (手入力バフ+%はrenderLive経由で更新するため。行の入力欄は他のケースと同じく触らない)。
  (catalog.inspireSources || []).forEach((source) => {
    const wrap = document.querySelector(`.inspire-result-wrap[data-source-id="${source.id}"]`);
    if (!wrap) return;
    const cfg = sourceCfg(source);
    const result = computeInspireSource(source, cfg, catalog, state.globalBuffIds, buffLevelsState());
    wrap.innerHTML = renderInspireResultBlock(source, cfg, result);
    // P6: レベル入力の打鍵中でもモジュール装備可否ヒントが即座に追従するようにする。
    const moduleHint = document.querySelector(`[data-role="inspire-module-hint"][data-source-id="${source.id}"]`);
    if (moduleHint) moduleHint.innerHTML = renderModuleUnusableHint(source, cfg);
  });
  $("verdict-section").outerHTML = renderVerdict();
  saveState();
}

/* ---------------- イベント処理 ---------------- */

function readFieldValue(el, isPercentField) {
  const raw = parseFloat(el.value);
  const num = Number.isFinite(raw) ? raw : 0;
  return isPercentField ? num / 100 : num;
}

function onEnemyFieldChange(el) {
  const field = el.dataset.field;
  const isPercent = ENEMY_PERCENT_FIELDS.has(field);
  const value = readFieldValue(el, isPercent);
  // inputで反映済みの値と同じなら何もしない。フォーカスが外れた瞬間のchangeで
  // 再描画すると、いまクリックされようとしているボタン(↺・共有URL)が差し替わって
  // クリックが失われるため。
  if (valuesEqual(state.enemy[field], value)) return;
  state.enemy[field] = value;
  renderLive();
}

function onOperatorNameChange(idx, name) {
  const op = catalog.operators.find((o) => o.name === name);
  if (!op) {
    // 一致しない入力(未確定/誤入力)は無視するが、opId自体はクリアしておく
    // (存在しないオペレーターのまま計算に混ざらないようにする)。
    state.rows[idx] = { ...blankRow() };
    render();
    return;
  }
  state.rows[idx] = makeDefaultRow(op, 0);
  render();
}

function onRowFieldChange(el) {
  const idx = Number(el.dataset.idx);
  const field = el.dataset.field;
  const row = state.rows[idx];
  if (!row) return;

  if (field === "opName") {
    onOperatorNameChange(idx, el.value);
    return;
  }
  if (field === "entryIdx") {
    const op = findOperator(catalog, row.opId);
    const newIdx = Number(el.value);
    const entry = op ? findEntry(op, newIdx) : null;
    row.entryIdx = newIdx;
    // P7: FK対象を選び直しても、選んでいたスキルLvはそのまま保つ(育成状況はスキル間で
    // 揃っていることが多いため)。Lv数の少ないスキルでは値の解決時にクランプされる。
    if (entry) {
      const values = resolveEntryValues(entry, row.skillLevel);
      row.multiplier = values.multiplier;
      row.selfPct = values.selfPct;
      row.hits = values.hits;
      row.dmgType = values.dmgType;
      row.dmgMult = values.dmgMult;
    }
    render();
    return;
  }
  if (field === "skillLevel") {
    // P7: スキルLvを変えたら、その時点のカタログ既定値(倍率/セルフ%)へ行フィールドを
    // 再スナップする(entryIdx変更と同じ扱い。ユーザーはそこから更に手動で上書きできる)。
    const op = findOperator(catalog, row.opId);
    const entry = op ? findEntry(op, row.entryIdx) : null;
    row.skillLevel = Number(el.value);
    if (entry) {
      const values = resolveEntryValues(entry, row.skillLevel);
      row.multiplier = values.multiplier;
      row.selfPct = values.selfPct;
      row.hits = values.hits;
      row.dmgType = values.dmgType;
      row.dmgMult = values.dmgMult;
    }
    render();
    return;
  }
  if (field === "specialOn") {
    // P2 follow-upで置き換え系の特殊強化を撤去したため、ONにしても行フィールドの
    // 再スナップは不要(加算系/乗算系はcomputeTotal側で都度計算される)。
    row.specialOn = el.checked;
    render();
    return;
  }
  if (field === "inspireOn") {
    row.inspireOn = el.checked;
    render();
    return;
  }
  if (field === "multiplierCandidate") {
    if (el.value !== "") row.multiplier = Number(el.value);
    render();
    return;
  }
  if (field === "potential") {
    row.potential = el.checked;
    render();
    return;
  }
  if (field === "moduleId") {
    row.moduleId = el.value || null;
    render();
    return;
  }
  if (field === "moduleLv") {
    row.moduleLv = Number(el.value);
    render();
    return;
  }
  if (field === "elite") {
    const op = findOperator(catalog, row.opId);
    row.elite = Number(el.value);
    // P6: 昇進を変えたらレベルはその昇進の最大値へ合わせる(仕様どおり)。
    row.level = op ? maxLevelForElite(op, row.elite) : row.level;
    render();
    return;
  }
  if (field === "dmgType") {
    row.dmgType = el.value;
    render();
    return;
  }
  const isPercent = ROW_PERCENT_FIELDS.has(field);
  const value = readFieldValue(el, isPercent);
  // 理由はonEnemyFieldChangeと同じ（blur時のchangeでクリック中のボタンを差し替えない）。
  if (valuesEqual(row[field], value)) return;
  row[field] = value;
  renderLive();
}

function onResetField(idx, field) {
  const row = state.rows[idx];
  const op = findOperator(catalog, row.opId);
  const entry = op ? findEntry(op, row.entryIdx) : null;
  const defaults = catalogDefaults(entry, row.skillLevel);
  if (defaults[field] === null || defaults[field] === undefined) return;
  row[field] = defaults[field];
  render();
}

function toggleRowBuff(idx, buffId) {
  const row = state.rows[idx];
  if (!row) return;
  const set = new Set(row.buffIds || []);
  if (set.has(buffId)) set.delete(buffId);
  else set.add(buffId);
  row.buffIds = Array.from(set);
  render();
}

function toggleGlobalBuff(buffId) {
  const set = new Set(state.globalBuffIds || []);
  if (set.has(buffId)) {
    set.delete(buffId);
  } else {
    set.add(buffId);
    // 同じexclusiveGroupの他のバフ(例: 前衛アーミヤの通常/スキル中)は同時に効かないのでOFFにする。
    const group = (catalog.buffers || []).find((b) => b.id === buffId)?.exclusiveGroup;
    if (group) {
      for (const b of catalog.buffers) {
        if (b.id !== buffId && b.exclusiveGroup === group) set.delete(b.id);
      }
    }
  }
  state.globalBuffIds = Array.from(set);
  globalBuffsOpen = true; // チップを操作した=開いて見ている最中なので、再描画後も開いたままにする
  render();
}

// P4: `source`付き条件付きバフの軸選択(昇進/潜在/モジュール/モジュールLv/スキルLv/トグル)。
// 全てselect/checkboxなので常にrenderで確定させる(onInspireFieldChangeと同じ方針)。
function onGlobalBuffLevelChange(el) {
  const buffId = el.dataset.buffId;
  const field = el.dataset.field;
  if (field === "toggleOn") {
    setBuffLevels(buffId, { toggleOn: el.checked });
  } else if (field === "moduleId") {
    setBuffLevels(buffId, { moduleId: el.value || null });
  } else {
    setBuffLevels(buffId, { [field]: Number(el.value) });
  }
  // 全体バフの欄の中で操作した=開いて見ている最中なので、再描画後も開いたままにする。
  // 「個別バフの育成設定」の欄から操作した場合は全体バフの開閉に触らない。
  if (el.closest("#global-buffs-details")) globalBuffsOpen = true;
  render();
}

/* ---------------- イベント処理: 鼓舞ソース(P3) ---------------- */

function toggleInspireSource(sourceId) {
  const source = (catalog.inspireSources || []).find((s) => s.id === sourceId);
  if (!source) return;
  const cfg = sourceCfg(source);
  setSourceCfg(sourceId, { on: !cfg.on });
  globalBuffsOpen = true; // チップを操作した=開いて見ている最中なので、再描画後も開いたままにする
  render();
}

function toggleSourceBuff(sourceId, buffId) {
  const source = (catalog.inspireSources || []).find((s) => s.id === sourceId);
  if (!source) return;
  const cfg = sourceCfg(source);
  const set = new Set(cfg.buffIds || []);
  if (set.has(buffId)) set.delete(buffId);
  else set.add(buffId);
  setSourceCfg(sourceId, { buffIds: Array.from(set) });
  render();
}

// select/checkbox(スキル・攻撃凸・素質凸・モジュール・モジュールLv・自己%パーツ)は
// 常にrenderで確定させる(row側のonRowFieldChangeと同じ方針。構造が変わるため)。
// 手入力バフ+%(数値input)だけはrenderLive経由でカーソル位置を保つ。
function onInspireFieldChange(el) {
  const sourceId = el.dataset.sourceId;
  const field = el.dataset.field;
  const source = (catalog.inspireSources || []).find((s) => s.id === sourceId);
  if (!source) return;
  const cfg = sourceCfg(source);

  if (field.startsWith("part:")) {
    const partId = field.slice("part:".length);
    setSourceCfg(sourceId, { parts: { ...(cfg.parts || {}), [partId]: el.checked } });
    render();
    return;
  }
  if (field === "skillNum") {
    // P7: スキルを選び直しても、選んでいたスキルLvはそのまま保つ(FK行のentryIdx変更と同じ扱い)。
    setSourceCfg(sourceId, { skillNum: el.value });
    render();
    return;
  }
  if (field === "skillLevel") {
    setSourceCfg(sourceId, { skillLevel: Number(el.value) });
    render();
    return;
  }
  if (field === "potential" || field === "talentPotential") {
    setSourceCfg(sourceId, { [field]: el.checked });
    render();
    return;
  }
  if (field === "moduleId") {
    setSourceCfg(sourceId, { moduleId: el.value || null });
    render();
    return;
  }
  if (field === "moduleLv") {
    setSourceCfg(sourceId, { moduleLv: Number(el.value) });
    render();
    return;
  }
  if (field === "elite") {
    const elite = Number(el.value);
    // P6: 昇進を変えたらレベルはその昇進の最大値へ合わせる(行と同じ仕様)。
    setSourceCfg(sourceId, { elite, level: maxLevelForElite(source, elite) });
    render();
    return;
  }
  if (field === "buffPct") {
    const value = readFieldValue(el, true);
    if (valuesEqual(cfg.buffPct, value)) return;
    setSourceCfg(sourceId, { buffPct: value });
    renderLive();
    return;
  }
  if (field === "level" || field === "trust") {
    const value = readFieldValue(el, false);
    if (valuesEqual(cfg[field], value)) return;
    setSourceCfg(sourceId, { [field]: value });
    renderLive();
  }
}

function addRow() {
  state.rows.push(blankRow());
  expandedIdx = state.rows.length - 1;
  render();
}

function dupRow(idx) {
  const copy = JSON.parse(JSON.stringify(state.rows[idx]));
  state.rows.splice(idx + 1, 0, copy);
  render();
}

function delRow(idx) {
  state.rows.splice(idx, 1);
  if (expandedIdx === idx) expandedIdx = null;
  else if (expandedIdx != null && expandedIdx > idx) expandedIdx -= 1;
  render();
}

function editRow(idx) {
  expandedIdx = idx;
  render();
}

function collapseRow() {
  expandedIdx = null;
  render();
}

async function shareUrl() {
  try {
    await navigator.clipboard.writeText(shareUrlForState());
    showToast("URLをコピーしました");
  } catch (e) {
    showToast("コピーに失敗しました（手動でコピーしてください）");
  }
}

function onAppInput(ev) {
  const el = ev.target;
  if (!el.dataset) return;
  // オペレーター名入力は「値がop.nameから導出される」フィールドなので、毎キー入力で
  // 再描画してしまうと確定前の文字列がop.nameで即座に上書きされ、入力できなくなる。
  // 確定は"change"（blur/datalist選択/Enter）に任せ、"input"では何もしない。
  if (el.dataset.field === "opName") return;
  if (el.dataset.role === "enemy") onEnemyFieldChange(el);
  else if (el.dataset.role === "row" && el.tagName === "INPUT" && el.type !== "checkbox") onRowFieldChange(el);
  else if (el.dataset.role === "inspire" && el.tagName === "INPUT" && el.type !== "checkbox") onInspireFieldChange(el);
}

function onAppChange(ev) {
  const el = ev.target;
  if (!el.dataset) return;
  if (el.dataset.role === "enemy") onEnemyFieldChange(el);
  else if (el.dataset.role === "row") onRowFieldChange(el);
  else if (el.dataset.role === "inspire") onInspireFieldChange(el);
  else if (el.dataset.role === "global-buff-level") onGlobalBuffLevelChange(el);
}

function onAppClick(ev) {
  const btn = ev.target.closest("[data-action]");
  if (!btn) return;
  const action = btn.dataset.action;
  const idx = btn.dataset.idx !== undefined ? Number(btn.dataset.idx) : null;
  switch (action) {
    case "add-row":
      addRow();
      break;
    case "edit-row":
      editRow(idx);
      break;
    case "dup-row":
      dupRow(idx);
      break;
    case "del-row":
      delRow(idx);
      break;
    case "collapse-row":
      collapseRow();
      break;
    case "reset-field":
      onResetField(idx, btn.dataset.field);
      break;
    case "share":
      shareUrl();
      break;
    case "toggle-row-buff":
      toggleRowBuff(idx, btn.dataset.buffId);
      break;
    case "toggle-global-buff":
      toggleGlobalBuff(btn.dataset.buffId);
      break;
    case "toggle-special-desc":
      if (specialDescOpenIdx.has(idx)) specialDescOpenIdx.delete(idx);
      else specialDescOpenIdx.add(idx);
      render();
      break;
    case "toggle-inspire-source":
      toggleInspireSource(btn.dataset.sourceId);
      break;
    case "toggle-source-buff":
      toggleSourceBuff(btn.dataset.sourceId, btn.dataset.buffId);
      break;
    case "toggle-inspire-desc": {
      const sourceId = btn.dataset.sourceId;
      if (inspireDescOpenIds.has(sourceId)) inspireDescOpenIds.delete(sourceId);
      else inspireDescOpenIds.add(sourceId);
      render();
      break;
    }
    default:
      break;
  }
}

/* ---------------- 起動 ---------------- */

export async function initUi() {
  catalog = await fetchCatalog();
  const shared = takeStateFromSharedUrl();
  state = shared ?? loadSavedState() ?? { v: 1, enemy: defaultEnemy(), rows: [], globalBuffIds: [], globalBuffLevels: {}, inspire: { sources: {} } };
  if (shared) {
    showToast("共有URLの内容を読み込みました");
    saveState();
  }
  expandedIdx = null;

  const app = $("app");
  app.addEventListener("input", onAppInput);
  app.addEventListener("change", onAppChange);
  app.addEventListener("click", onAppClick);
  // <details id="global-buffs-details">をユーザーがsummary直クリックで開閉した場合、
  // その状態をrender()後も覚えておく(toggleイベントはbubbleしないためcapture:trueで拾う)。
  app.addEventListener(
    "toggle",
    (ev) => {
      if (ev.target && ev.target.id === "global-buffs-details") globalBuffsOpen = ev.target.open;
    },
    true,
  );

  render();
}
