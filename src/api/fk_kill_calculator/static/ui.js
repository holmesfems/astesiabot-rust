/* ============================================================
   フレームキル計算機 — 表現層（DOM描画・イベント配線・URL共有）

   計算そのものは ./engine.js（DOM非依存）に任せ、ここでは
   カタログ取得(fetch)・状態管理・DOM描画・URL(#state=...)の
   読み書きだけを担当する。日本語専用ページなので文言はここに直書きする
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
  makeDefaultRow,
  computeTotal,
  dropStaleRows,
  suggest,
  describeSuggestion,
} from "./engine.js";

const ENEMY_PERCENT_FIELDS = new Set(["defPct", "vulnPct"]);
const ROW_PERCENT_FIELDS = new Set(["selfPct", "buffPct", "dmgMult"]);

const BAR_COLORS = ["#4f7cff", "#e8dcb0", "#79e2d0", "#d4794a", "#b48ef0", "#f0748a", "#7bd67b", "#f0a94e"];

let catalog = null;
let state = null;
let expandedIdx = null;
let hashTimer = null;

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

/* ---------------- 状態の初期値・URL共有 ---------------- */

function defaultEnemy() {
  return { hp: 0, def: 0, res: 0, defFlat: 0, defPct: 0, resFlat: 0, vulnPct: 0 };
}

function blankRow() {
  return {
    opId: "",
    entryIdx: 0,
    dmgType: "physical",
    potential: true,
    moduleId: null,
    moduleLv: 3,
    multiplier: 1,
    selfPct: 0,
    hits: 1,
    buffPct: 0,
    dmgMult: 1,
    ignoreDef: 0,
  };
}

function saveToHash() {
  clearTimeout(hashTimer);
  hashTimer = setTimeout(() => {
    const json = JSON.stringify(state);
    const compressed = window.LZString.compressToEncodedURIComponent(json);
    const newHash = "#state=" + compressed;
    if (location.hash !== newHash) history.replaceState(null, "", newHash);
  }, 300);
}

// share操作等、直後にURLを他所へコピーしたい場合はデバウンスを待たずに即書き込む。
function flushHash() {
  clearTimeout(hashTimer);
  const json = JSON.stringify(state);
  const compressed = window.LZString.compressToEncodedURIComponent(json);
  location.hash = "state=" + compressed;
}

function restoreFromHash() {
  const m = /^#state=(.+)$/.exec(location.hash);
  if (!m) return null;
  try {
    const json = window.LZString.decompressFromEncodedURIComponent(m[1]);
    if (!json) return null;
    const parsed = JSON.parse(json);
    if (!parsed || parsed.v !== 1 || !Array.isArray(parsed.rows) || !parsed.enemy) return null;
    const { state: cleaned, dropped } = dropStaleRows(parsed, catalog);
    if (dropped > 0) {
      showToast(`カタログに存在しないオペレーター/スキルが${dropped}件あったため取り除きました`);
    }
    return cleaned;
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
  const field = active && active.dataset ? active.dataset.field : null;
  const idx = active && active.dataset ? active.dataset.idx : null;
  const selStart = active && "selectionStart" in active ? active.selectionStart : null;
  const selEnd = active && "selectionEnd" in active ? active.selectionEnd : null;

  fn();

  if (field != null) {
    const selector = idx != null ? `[data-field="${field}"][data-idx="${idx}"]` : `[data-field="${field}"]`;
    const el = document.querySelector(selector);
    if (el) {
      el.focus();
      if (typeof el.setSelectionRange === "function" && selStart != null) {
        try {
          el.setSelectionRange(selStart, selEnd);
        } catch (e) {
          /* select等setSelectionRangeを持たない要素は無視 */
        }
      }
    }
  }
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
  const { results } = computeTotal(catalog, [row], state.enemy);
  const r = results[0];
  return `${op.name} ${shortSkillRef(entry)}: ${fmtInt(r.perHit)}×${row.hits}Hit → 実ダメ ${fmtInt(r.rowDamage)}`;
}

// 折りたたみ行の中身: [色ドット(凡例兼用)] [名前+短いスキル参照。省略可] … [ダメージ。省略不可・太字]。
// スキルの表示名(entry.skillLabel)は展開ビューにあるのでここには出さない（UIラウンド2）。
function renderRowSummary(row, idx) {
  const op = findOperator(catalog, row.opId);
  const dot = `<span class="row-color-dot" style="background:${rowColor(idx)}" aria-hidden="true"></span>`;
  if (!op) {
    return `${dot}<span class="row-summary-name row-summary-empty">オペレーター未選択</span>`;
  }
  const entry = findEntry(op, row.entryIdx);
  const { results } = computeTotal(catalog, [row], state.enemy);
  const r = results[0];
  const tooltip = escapeHtml(rowPlainSummary(row));
  const nameRef = escapeHtml(`${op.name} ${shortSkillRef(entry)}`);
  return `${dot}<span class="row-summary-name" title="${tooltip}">${nameRef}</span>`
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

function multiplierCandidateOptions(entry) {
  let html = `<option value="">候補から選ぶ…</option>`;
  for (const [key, value] of entry.multiplierCandidates) {
    html += `<option value="${value}">${escapeHtml(key)} (${value})</option>`;
  }
  return html;
}

// カタログ側の既定値(entry由来)。行のフィールドがユーザー操作でこの値から
// 変わっていれば「↺」リセットボタンを出す。entryが無ければ全てnull。
function catalogDefaults(entry) {
  if (!entry) return { multiplier: null, selfPct: null, hits: null, dmgType: null };
  return {
    multiplier: entry.multiplier.value,
    selfPct: entry.selfAtkPct.value,
    hits: entry.hits.value,
    dmgType: entry.damageType.value,
  };
}

function fieldBadges(entry, field, currentValue, idx) {
  if (!entry) return "";
  const sourceMap = { multiplier: entry.multiplier, selfPct: entry.selfAtkPct, hits: entry.hits, dmgType: entry.damageType };
  const src = sourceMap[field];
  // renderLiveが入力欄を作り直さずにバッジだけ差し替えられるよう、常にラッパーで包む。
  let html = `<span class="field-badges" data-badges-for="${field}" data-idx="${idx}">`;
  if (src && src.source === "manual") html += `<span class="badge badge-manual" title="オーナーによる手動補正値">補正</span>`;
  const defaults = catalogDefaults(entry);
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

function renderRowExpanded(row, idx) {
  const op = findOperator(catalog, row.opId);
  const entry = op ? findEntry(op, row.entryIdx) : null;

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
      <label>FK対象 ${entrySelectHtml}</label>
    </div>
    <div class="row-grid2">
      <label>ダメージ種別
        <select data-role="row" data-field="dmgType" data-idx="${idx}">
          <option value="physical"${row.dmgType === "physical" ? " selected" : ""}>物理</option>
          <option value="arts"${row.dmgType === "arts" ? " selected" : ""}>術</option>
          <option value="true"${row.dmgType === "true" ? " selected" : ""}>真</option>
        </select>
        ${fieldBadges(entry, "dmgType", row.dmgType, idx)}
      </label>
      <label class="check-label">
        <input type="checkbox" data-role="row" data-field="potential" data-idx="${idx}" ${row.potential ? "checked" : ""}>
        攻撃凸
      </label>
    </div>
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
    <div class="row-grid2">
      <label>倍率
        <input type="number" step="any" data-role="row" data-field="multiplier" data-idx="${idx}" value="${trimNum(row.multiplier)}">
        ${fieldBadges(entry, "multiplier", row.multiplier, idx)}
      </label>
      <label>倍率候補
        <select data-role="row" data-field="multiplierCandidate" data-idx="${idx}" ${entry && entry.multiplierCandidates.length ? "" : "disabled"}>
          ${entry ? multiplierCandidateOptions(entry) : '<option value="">候補から選ぶ…</option>'}
        </select>
      </label>
    </div>
    <div class="row-grid2">
      <label>セルフ%
        <input type="number" step="any" data-role="row" data-field="selfPct" data-idx="${idx}" value="${fmtPct(row.selfPct, 3)}">
        ${fieldBadges(entry, "selfPct", row.selfPct, idx)}
      </label>
      <label>Hit数
        <input type="number" step="1" min="0" data-role="row" data-field="hits" data-idx="${idx}" value="${row.hits}">
        ${fieldBadges(entry, "hits", row.hits, idx)}
      </label>
    </div>
    <div class="row-grid2">
      <label>このオペへのバフ+%
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

function renderFormulaLine(op, row) {
  const { atk, pct, final, perHit, rowDamage, atFloor } = computeTotal(catalog, [row], state.enemy).results[0];
  const selfPart = `セルフ${fmtPct(row.selfPct)}%`;
  const buffPart = `バフ${fmtPct(row.buffPct)}%`;
  const line1 = `${fmtInt(atk)} ×(1 + ${selfPart} + ${buffPart}) × ${fmtPct(row.multiplier)}% = ${fmtInt(final)} /hit`;
  const floorNote = atFloor ? `<span class="floor-note">（5%floor発動中）</span>` : "";
  const line2 = `→ 実ダメ ${fmtInt(perHit)}/hit × ${row.hits}Hit = <b>${fmtInt(rowDamage)}</b> ${floorNote}`;
  return `${escapeHtml(line1)}<br>${line2}`;
}

function renderRows() {
  let html = `${operatorDatalist()}<section class="card" id="rows-section"><h2>② FKするオペレーター</h2><div id="rows-list">`;
  state.rows.forEach((row, idx) => {
    const expanded = idx === expandedIdx;
    html += `<div class="row-card${expanded ? " row-expanded" : ""}" data-idx="${idx}">`;
    if (expanded) {
      html += renderRowExpanded(row, idx);
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
  const { results, total, killed } = computeTotal(catalog, state.rows, state.enemy);
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
  if (killed) {
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
    $("app").innerHTML = renderEnemy() + renderRows() + renderVerdict();
  });
  saveToHash();
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
        badges.outerHTML = fieldBadges(entry, field, row[field], idx);
      });
    } else {
      const main = card.querySelector(".row-summary-main");
      if (main) main.innerHTML = renderRowSummary(row, idx);
    }
  });
  $("verdict-section").outerHTML = renderVerdict();
  saveToHash();
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
    if (entry) {
      row.multiplier = entry.multiplier.value;
      row.selfPct = entry.selfAtkPct.value;
      row.hits = entry.hits.value;
      row.dmgType = entry.damageType.value;
    }
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
  const defaults = catalogDefaults(entry);
  if (defaults[field] === null || defaults[field] === undefined) return;
  row[field] = defaults[field];
  render();
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
  flushHash();
  try {
    await navigator.clipboard.writeText(location.href);
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
}

function onAppChange(ev) {
  const el = ev.target;
  if (!el.dataset) return;
  if (el.dataset.role === "enemy") onEnemyFieldChange(el);
  else if (el.dataset.role === "row") onRowFieldChange(el);
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
    default:
      break;
  }
}

/* ---------------- 起動 ---------------- */

export async function initUi() {
  catalog = await fetchCatalog();
  state = restoreFromHash() ?? { v: 1, enemy: defaultEnemy(), rows: [] };
  expandedIdx = null;

  const app = $("app");
  app.addEventListener("input", onAppInput);
  app.addEventListener("change", onAppChange);
  app.addEventListener("click", onAppClick);

  render();
}
