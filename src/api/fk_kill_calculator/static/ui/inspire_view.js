// フレームキル計算機 — 鼓舞ソースのカード。

import {
  computeInspireSource,
  effectiveModuleId,
  maxLevelForElite,
  resolveConditionalValue,
} from "../engine.js";
import { renderInspirePotentialSelect } from "./buff_view.js";
import { escapeHtml, fmtInt, fmtPct, trimNum } from "./dom.js";
import {
  eliteOptions,
  moduleOptions,
  renderModuleUnusableHint,
  renderSkillLevelWarning,
  skillLevelOptionsN,
} from "./row_view.js";
import {
  buffLevels,
  buffLevelsState,
  catalog,
  inspireDescOpenIds,
  sourceCfg,
  state,
} from "./store.js";

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
export function renderInspireResultBlock(source, cfg, result) {
  const descOpen = inspireDescOpenIds.has(source.id);
  const breakdownText = describeInspireBreakdown(result);
  const infoBtn = `<button type="button" class="special-info-btn" data-action="toggle-inspire-desc" data-source-id="${escapeHtml(source.id)}" aria-expanded="${descOpen}" title="${escapeHtml(breakdownText)}">ⓘ</button>`;
  const descBlock = descOpen ? `<p class="special-desc">${escapeHtml(breakdownText)}</p>` : "";
  return `<p class="inspire-result">→ 鼓舞 +${fmtInt(result.amount)}${infoBtn}</p>${descBlock}`;
}

// 鼓舞ソース1件分のカード。OFFの間はトグルチップだけ。ONになると
// スキル/潜在(P8。旧攻撃凸+素質凸を統合)/モジュール/自己%パーツ/個別バフ/手入力バフ
// +結果行を表示する。
export function renderInspireSourceCard(source, singleConflicts) {
  const cfg = sourceCfg(source);
  const toggleChip = `<button type="button" class="chip" data-action="toggle-inspire-source" data-source-id="${escapeHtml(source.id)}" aria-pressed="${escapeHtml(cfg.on)}">${escapeHtml(source.name)}</button>`;
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
      ${renderInspirePotentialSelect(source, cfg)}
    </div>
    ${renderSkillLevelWarning(cfg)}
    <div class="row-grid3">
      <label>昇進
        <select data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="elite">${eliteOptions(source, cfg)}</select>
      </label>
      <label>Lv
        <div class="level-with-max">
          <input type="number" step="1" min="1" max="${maxLevelForElite(source, cfg.elite)}" data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="level" value="${escapeHtml(cfg.level)}">
          <span class="level-max-hint">/${maxLevelForElite(source, cfg.elite)}</span>
        </div>
      </label>
      <label>信頼度%
        <input type="number" step="1" min="0" max="100" data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="trust" value="${escapeHtml(cfg.trust)}">
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
