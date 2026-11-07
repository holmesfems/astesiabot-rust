// フレームキル計算機 — オペレーター行の展開ビュー。

import {
  RED_TILE_CELL,
  STONE_BOARD_SIZE,
  analyzeStoneBoard,
  computeTotal,
  findEntry,
  findModule,
  findOperator,
  maxEliteFor,
  maxLevelForElite,
  isStoneInRange,
  moduleUsable,
  resolveConditionalValue,
  resolveEntryValues,
  resolveSpecialCurrentValue,
  resolveStoneBoardValues,
  skillLevelWarning,
  skillUnlockWarning,
  specialAddCanApply,
  specialUiState,
  valueAtLevel,
} from "../engine.js";
import { renderRowPotentialSelect, skillLevelOptionLabel } from "./buff_view.js";
import { escapeHtml, fmtInt, fmtPct, trimNum } from "./dom.js";
import { renderFormulaLine } from "./formula_view.js";
import {
  buffLevels,
  buffLevelsState,
  catalog,
  inspireStates,
  specialDescOpenIdx,
  state,
} from "./store.js";

function entryOptionLabel(entry) {
  const prefix = /^\d+$/.test(entry.skillNum) ? `S${entry.skillNum}` : entry.skillNum;
  const label = `${prefix} ${entry.skillLabel}`;
  return entry.variantLabel ? `${label} / ${entry.variantLabel}` : label;
}

export function operatorDatalist() {
  return `<datalist id="operator-datalist">${catalog.operators
    .map((o) => `<option value="${escapeHtml(o.name)}">`)
    .join("")}</datalist>`;
}

export function moduleOptions(op, row) {
  let html = `<option value="">なし</option>`;
  for (const m of op.modules) {
    const sel = row.moduleId === m.id ? " selected" : "";
    html += `<option value="${escapeHtml(m.id)}"${sel}>${escapeHtml(m.name)}（${escapeHtml(m.typeName)}）</option>`;
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
export function catalogDefaults(entry, skillLevel) {
  if (!entry) return { multiplier: null, selfPct: null, hits: null, dmgType: null };
  const values = resolveEntryValues(entry, skillLevel);
  return { multiplier: values.multiplier, selfPct: values.selfPct, hits: values.hits, dmgType: values.dmgType };
}

// P10(混合スキル): パーツ(entry.parts[partIndex])単位のカタログ既定値
// (multiplier/dmgTypeのみ。Hit数は行で共通、selfPct/dmgMultはパーツを持たない)。
export function partCatalogDefaults(part, skillLevel) {
  if (!part) return { multiplier: null, dmgType: null };
  return {
    multiplier: valueAtLevel(part.multiplierByLevel, skillLevel),
    dmgType: part.damageType.value,
  };
}

// P7: 「補正」バッジの文言。固定(Manualかつスキルレベルに追従しない)なら「補正(特化3固定)」、
// スキルLvに追従するManual(multiplier_key/self_atk_pct_factor経由)ならただの「補正」。
function manualBadgeLabel(field, entry) {
  const fixedMap = { multiplier: entry.multiplierFixed, selfPct: entry.selfAtkPctFixed };
  return fixedMap[field] ? "補正(特化3固定)" : "補正";
}

// P10(混合スキル): `partIndex`が1以上ならentry.parts[partIndex](FkPart)を基準に
// manual補正バッジ/↺リセットボタンを判定する(field=multiplier/hits/dmgTypeのみ)。
// `partIndex`省略/0なら従来通りentry基準(selfPctも含む全フィールド対応)。
export function fieldBadges(entry, field, currentValue, idx, skillLevel, partIndex = 0) {
  if (!entry) return "";
  const part = partIndex > 0 ? entry.parts && entry.parts[partIndex] : null;
  const sourceMap = part
    ? { multiplier: part.multiplier, dmgType: part.damageType }
    : { multiplier: entry.multiplier, selfPct: entry.selfAtkPct, hits: entry.hits, dmgType: entry.damageType };
  const src = sourceMap[field];
  const partAttr = partIndex ? ` data-part="${partIndex}"` : "";
  // renderLiveが入力欄を作り直さずにバッジだけ差し替えられるよう、常にラッパーで包む。
  let html = `<span class="field-badges" data-badges-for="${field}" data-idx="${idx}"${partAttr}>`;
  if (src && src.source === "manual") {
    const label = part ? (field === "multiplier" && part.multiplierFixed ? "補正(特化3固定)" : "補正") : manualBadgeLabel(field, entry);
    html += `<span class="badge badge-manual" title="オーナーによる手動補正値">${escapeHtml(label)}</span>`;
  }
  const defaults = part ? partCatalogDefaults(part, skillLevel) : catalogDefaults(entry, skillLevel);
  const def = defaults[field];
  if (def !== null && def !== undefined && !valuesEqual(def, currentValue)) {
    html += `<button type="button" class="badge badge-reset" data-action="reset-field" data-field="${field}" data-idx="${idx}"${partAttr} title="カタログ既定値に戻す" aria-label="カタログ既定値に戻す">↺</button>`;
  }
  return html + `</span>`;
}

export function valuesEqual(a, b) {
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

// P8 follow-up: mulMultiplier(素質値テーブル)が現在の行の昇進で未解放(全潜在0)な間の
// 最小昇進を求める("素質は昇進<N>で解放"ヒント用)。見つからない(理論上起きない)場合は
// テーブルの行数(=最大昇進+1)を返す。
function minEliteForMulMultiplier(table) {
  const rows = (table && table.valuesByEliteAndPotential) || [];
  const idx = rows.findIndex((r) => r.some((v) => v > 0));
  return idx >= 0 ? idx : rows.length;
}

// 特殊強化のヒント文。加算系(requiresModule)がモジュール条件を満たさない間は
// 「特殊強化「<label>」はモジュール<X> Lv<N>以上で有効」、乗算系(mulMultiplier。P8
// follow-upで素質値テーブル参照になった)が素質未解放の間は「特殊強化「<label>」は
// 素質が昇進<N>で解放」を出す(`specialUiState`と同じ優先順位: requiresModuleを先に
// 判定する)。
// P6: モジュール自体が現在の昇進/レベルで装備できない間は、実データ(unlockPhase/
// unlockLevel)から「昇進<X> Lv<Y>以上で装備可能」を示す。装備はできているが
// モジュールLv側の条件(addSelfAtkPctByModuleLevelの最初の非ゼロ要素)を
// 満たさない間は従来どおり「モジュール<X> Lv<N>以上で有効」を示す。
function specialHintText(op, entry, row) {
  const sp = entry.special;
  if (sp.requiresModule && !specialAddCanApply(op, entry, row)) {
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
  if (sp.mulMultiplier) {
    return `特殊強化「${sp.label}」は素質が昇進${minEliteForMulMultiplier(sp.mulMultiplier)}で解放`;
  }
  return "";
}

// ⓘ説明文の末尾に付ける「現在: +N%」/「現在: ×N」(P2 follow-up)。
function specialCurrentValueText(op, entry, row) {
  const cur = resolveSpecialCurrentValue(op, entry, row);
  if (!cur) return "";
  return cur.kind === "mul" ? `現在: ×${trimNum(cur.value)}` : `現在: +${fmtPct(cur.value)}%`;
}

// 「特殊強化: <label>」チェックボックス + ⓘ説明文トグル(P2/P2 follow-up)。
// entry.specialが無いスキルは何も出さない。加算系(requiresModule)でモジュール条件を
// 満たさない間、乗算系(mulMultiplier。P8 follow-upで素質値テーブル参照になった)で
// 素質が未解放の間は、それぞれチェックボックスの代わりにヒントを出す
// (`specialUiState`が判定する)。
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
export function eliteOptions(op, row) {
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
export function skillLevelOptionsN(n, current) {
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
export function renderSkillLevelWarning(cfgLike) {
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
export function renderModuleUnusableHint(op, row) {
  if (!op || !row.moduleId) return "";
  const module = findModule(op, row.moduleId);
  if (!module || moduleUsable(module, row.elite, row.level)) return "";
  return `<p class="special-hint">モジュール${escapeHtml(module.typeName)}は昇進${module.unlockPhase} Lv${module.unlockLevel}以上で装備可能（現在は加算されません）</p>`;
}

// 碁石盤(ウァン S3): Hit数の代わりに出す5x5の盤面。中央は赤マスで置けない。
// 碁石の数字はスタック数。発動する/線の数にだけ入る/孤立で未起動、を色で分ける。
function renderStoneBoard(op, entry, row, idx) {
  const values = resolveStoneBoardValues(op, entry, row);
  const stones = new Map(analyzeStoneBoard(row.stones, values.maxStacks).map((s) => [s.cell, s]));
  const cells = Array.from({ length: STONE_BOARD_SIZE ** 2 }, (_, cell) => {
    if (cell === RED_TILE_CELL) return `<div class="stone-cell stone-red-tile" role="img" aria-label="赤マス">赤マス</div>`;
    const stone = stones.get(cell);
    const rangeClass = isStoneInRange(cell) ? "" : " stone-out";
    const stateClass = !stone ? "" : !stone.active ? "stone-idle" : stone.fires ? "stone-fire" : "stone-line";
    const mark = stone ? `<span class="stone ${stateClass}">${stone.active ? stone.stacks : ""}</span>` : "";
    return `<button type="button" class="stone-cell${rangeClass}" data-action="toggle-stone" data-role="stone" data-field="stone-${cell}" data-idx="${idx}" data-cell="${cell}" aria-pressed="${!!stone}" aria-label="${Math.floor(cell / STONE_BOARD_SIZE) + 1}行${(cell % STONE_BOARD_SIZE) + 1}列">${mark}</button>`;
  }).join("");
  const talentNote =
    values.maxStacks > 0
      ? `第二素質: 1スタックごとにダメージ+${fmtPct(values.damagePerStack)}%・術耐性-${trimNum(values.resPenPerStack)}（最大${trimNum(values.maxStacks)}）`
      : "第二素質は昇進2で解放（今は線のボーナスなし）";
  return `
    <div class="stone-board-block">
      <div class="stone-board-label">碁石の配置</div>
      <div class="stone-board">${cells}</div>
      <p class="stone-board-legend">緑=発動 / 灰=範囲外（線の数にだけ入る） / 点線=孤立で未起動。数字はスタック数</p>
      <p class="stone-board-legend">${talentNote}</p>
      <button type="button" class="stone-clear" data-action="clear-stones" data-idx="${idx}">盤面を空にする</button>
    </div>`;
}

// P10(混合スキル): ダメージ種別/倍率/[倍率候補。パーツ無し限定]/Hit数/ダメージ倍率%を
// まとめて描く。`entry.parts`が無い/1件以下(=混合スキルではない通常エントリ)なら
// 従来通りの単一ブロック(data-partは付けない=パーツ0扱いのまま)、2件以上あれば
// 共通のHit数欄1つ + パーツごとの小ブロック(見出し=パーツラベル。例: "物理"/"術"。
// 中身は倍率/ダメージ倍率%だけ)を並べる。ダメージ種別は見出しで自明なので選ばせず、
// 倍率候補セレクトも出さない(overrideでキーが指定済みのため)。
function renderDamagePartsSection(op, entry, row, idx) {
  const parts = (entry && entry.parts) || [];
  if (parts.length < 2) {
    const hasBoard = !!(entry && entry.stoneBoard);
    const hitsField = hasBoard
      ? ""
      : `<label>Hit数
        <input type="number" step="1" min="0" data-role="row" data-field="hits" data-idx="${idx}" value="${row.hits}">
        ${fieldBadges(entry, "hits", row.hits, idx, row.skillLevel)}
      </label>`;
    return `
    <div class="row-grid2">
      <label>ダメージ種別
        <select data-role="row" data-field="dmgType" data-idx="${idx}">
          <option value="physical"${row.dmgType === "physical" ? " selected" : ""}>物理</option>
          <option value="arts"${row.dmgType === "arts" ? " selected" : ""}>術</option>
          <option value="true"${row.dmgType === "true" ? " selected" : ""}>真</option>
        </select>
        ${fieldBadges(entry, "dmgType", row.dmgType, idx, row.skillLevel)}
      </label>
      <label>倍率候補
        <select data-role="row" data-field="multiplierCandidate" data-idx="${idx}" ${entry && entry.multiplierCandidates.length ? "" : "disabled"}>
          ${entry ? multiplierCandidateOptions(entry, row.skillLevel) : '<option value="">候補から選ぶ…</option>'}
        </select>
      </label>
    </div>
    <div class="row-grid2">
      <label>倍率
        <input type="number" step="any" data-role="row" data-field="multiplier" data-idx="${idx}" value="${trimNum(row.multiplier)}">
        ${fieldBadges(entry, "multiplier", row.multiplier, idx, row.skillLevel)}
      </label>
      ${hitsField}
    </div>
    ${hasBoard ? renderStoneBoard(op, entry, row, idx) : ""}
    <div class="row-grid2">
      <label>ダメージ倍率%
        <input type="number" step="any" data-role="row" data-field="dmgMult" data-idx="${idx}" value="${fmtPct(row.dmgMult, 3)}">
      </label>
    </div>`;
  }

  // Hit数は全パーツ共通(1回の攻撃で全パーツが同時に出るため)。row.hitsだけを持つ。
  const sharedHits = `
    <div class="row-grid2">
      <label>Hit数（${escapeHtml(parts.map((p) => p.label).join("・"))}共通）
        <input type="number" step="1" min="0" data-role="row" data-field="hits" data-idx="${idx}" value="${row.hits}">
        ${fieldBadges(entry, "hits", row.hits, idx, row.skillLevel)}
      </label>
    </div>`;
  return sharedHits + parts
    .map((part, partIndex) => {
      const target = partIndex === 0 ? row : (row.extraParts && row.extraParts[partIndex - 1]) || {};
      // ダメージ種別はパーツ見出し(ラベル。例: "物理"/"術")で自明なので選ばせない
      // (カタログのpart.damageTypeのまま。row.dmgType/extraParts[i].dmgTypeに入っている)。
      const multiplier = target.multiplier ?? valueAtLevel(part.multiplierByLevel, row.skillLevel);
      const dmgMult = target.dmgMult ?? 1;
      const partAttr = partIndex ? ` data-part="${partIndex}"` : "";
      return `
      <div class="row-part-block">
        <h4 class="row-part-heading">${escapeHtml(part.label)}</h4>
        <div class="row-grid2">
          <label>倍率
            <input type="number" step="any" data-role="row" data-field="multiplier" data-idx="${idx}"${partAttr} value="${trimNum(multiplier)}">
            ${fieldBadges(entry, "multiplier", multiplier, idx, row.skillLevel, partIndex)}
          </label>
          <label>ダメージ倍率%
            <input type="number" step="any" data-role="row" data-field="dmgMult" data-idx="${idx}"${partAttr} value="${fmtPct(dmgMult, 3)}">
          </label>
        </div>
      </div>`;
    })
    .join("");
}

export function renderRowExpanded(row, idx, singleConflicts) {
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
  const potentialHtml = op ? renderRowPotentialSelect(op, entry, row, idx) : "";
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
    <div class="row-actions-top">
      <button type="button" data-action="collapse-row" data-idx="${idx}">▲ 折りたたむ</button>
    </div>
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
    ${potentialHtml ? `<div class="row-grid2">${potentialHtml}</div>` : ""}
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
    ${renderDamagePartsSection(op, entry, row, idx)}
    <div class="row-grid2">
      <label>セルフ%
        <input type="number" step="any" data-role="row" data-field="selfPct" data-idx="${idx}" value="${fmtPct(row.selfPct, 3)}">
        ${fieldBadges(entry, "selfPct", row.selfPct, idx, row.skillLevel)}
      </label>
      <label>手入力バフ+%
        <input type="number" step="any" data-role="row" data-field="buffPct" data-idx="${idx}" value="${fmtPct(row.buffPct, 3)}">
      </label>
    </div>
    ${renderIndividualBuffChips(row, idx, singleConflicts)}
    ${renderConditionalStatusLine(r)}
    ${renderSpecialCheckbox(op, entry, row, idx)}
    ${renderRowInspireToggle(r, row, idx)}
    <div class="row-grid2">
      <label>手入力 基礎攻撃力+
        <input type="number" step="any" data-role="row" data-field="buffFlat" data-idx="${idx}" value="${trimNum(row.buffFlat || 0)}">
      </label>
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
