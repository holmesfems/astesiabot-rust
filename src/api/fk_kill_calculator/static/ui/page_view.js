// フレームキル計算機 — ページの組み立て: 敵欄・折りたたみ行・全体バフ・判定欄・render/renderLive。

import {
  computeInspireSource,
  computeTotal,
  describeSuggestion,
  findEntry,
  findOperator,
  findSingleTargetConflicts,
  suggest,
} from "../engine.js";
import {
  renderConditionalSourceCard,
  renderIndividualBuffLevelsSection,
  skillLevelOptionLabel,
} from "./buff_view.js";
import { $, BAR_COLORS, escapeHtml, fmtInt, fmtPct, trimNum, withPreservedFocus } from "./dom.js";
import { renderFormulaLine } from "./formula_view.js";
import { renderInspireResultBlock, renderInspireSourceCard } from "./inspire_view.js";
import {
  fieldBadges,
  operatorDatalist,
  renderModuleUnusableHint,
  renderRowExpanded,
} from "./row_view.js";
import {
  buffLevelsState,
  catalog,
  expandedIdx,
  globalBuffsOpen,
  inspireStates,
  saveState,
  sourceCfg,
  state,
  verdictSuggestOpen,
} from "./store.js";

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
  // P10(混合スキル): パーツが2つ以上あるエントリはパーツ内訳(例: "物理 1200 + 術 300")を出す。
  if (entry && entry.parts && entry.parts.length > 1) {
    const partsText = entry.parts.map((p, i) => `${p.label} ${fmtInt(r.parts[i].rowDamage)}`).join(" + ");
    return `${op.name} ${eliteLevelLabel(row)} ${shortSkillRef(entry)}: ${partsText} → 実ダメ ${fmtInt(r.rowDamage)}`;
  }
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
      // 判定欄は上部に固定しているので、提案は既定で折りたたむ(開くと本文の上に広がる)。
      suggestionsHtml = `<details id="verdict-suggestions"${verdictSuggestOpen ? " open" : ""}><summary class="suggest-title">撃破するには（${suggestions.length}件）</summary><ul>${suggestions
        .map((s) => `<li>${escapeHtml(describeSuggestion(s, catalog, state.rows))}</li>`)
        .join("")}</ul></details>`;
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
  </section>`;
}

// 判定に応じて画面の縁を光らせる(#31)。OK=緑/NG=赤/HP未入力=光らせない。
// renderVerdictはHTMLを返すだけなので、bodyのクラス切り替えはここで別に行う。
function updateVerdictGlow() {
  const { killed } = computeTotal(catalog, state.rows, state.enemy, state.globalBuffIds, inspireStates(), buffLevelsState());
  const pending = state.enemy.hp <= 0;
  document.body.classList.toggle("verdict-glow-ok", !pending && killed);
  document.body.classList.toggle("verdict-glow-ng", !pending && !killed);
}

// ページ末尾の共有ボタンと対象範囲の注記(判定欄を上部固定にしたので、常に見える必要の
// ないものはここへ分けた)。
function renderPageFooter() {
  return `
  <section id="page-footer-actions">
    <button type="button" id="share-url-btn" data-action="share">共有URLをコピー</button>
    <p class="scope-note">会心・確率発動・継続ダメージ・召喚物・オペ間デバフの順序依存は非対応。</p>
  </section>`;
}

/* ---------------- 全体再描画 ---------------- */

export function render() {
  withPreservedFocus(() => {
    $("app").innerHTML =
      renderVerdict() + renderEnemy() + renderGlobalBuffs() + renderIndividualBuffLevelsSection() + renderRows() + renderPageFooter();
  });
  updateVerdictGlow();
  saveState();
}

// 数値・テキスト欄の打鍵中の再描画。入力欄は触らず、導出表示だけを差し替える
// （理由はファイル冒頭の「再描画方針」参照）。
export function renderLive() {
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
        // P10(混合スキル): data-part(1以上)が付いていればrow.extraParts[part-1]から
        // 現在値を読む(0/未指定ならrow自身=パーツ0のまま)。
        const partIndex = badges.dataset.part ? Number(badges.dataset.part) : 0;
        const currentValue = partIndex > 0 ? (row.extraParts && row.extraParts[partIndex - 1] && row.extraParts[partIndex - 1][field]) : row[field];
        badges.outerHTML = fieldBadges(entry, field, currentValue, idx, row.skillLevel, partIndex);
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
  updateVerdictGlow();
  saveState();
}
