// フレームキル計算機 — #app に委譲したイベント処理。

import {
  autoEnableSourcedBuffs,
  findEntry,
  findOperator,
  makeDefaultExtraParts,
  makeDefaultRow,
  maxLevelForElite,
  resolveEntryValues,
} from "../engine.js";
import { ENEMY_PERCENT_FIELDS, ROW_PERCENT_FIELDS, showToast } from "./dom.js";
import { render, renderLive } from "./page_view.js";
import { catalogDefaults, partCatalogDefaults, valuesEqual } from "./row_view.js";
import {
  blankRow,
  catalog,
  expandedIdx,
  inspireDescOpenIds,
  setBuffLevels,
  setExpandedIdx,
  setGlobalBuffsOpen,
  setSourceCfg,
  shareUrlForState,
  sourceCfg,
  specialDescOpenIdx,
  state,
} from "./store.js";

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
  // P9: このオペレーターをsource.operatorIdに持つ条件付きバフ(例: ホルンの「軍事要塞」)を
  // 一度だけ自動でONにする(以後はユーザー操作でOFFにでき、行の他フィールド変更では
  // 再ONにならない。呼び出しタイミングをここ[opId確定時]だけに絞ることで実現している)。
  state.globalBuffIds = autoEnableSourcedBuffs(catalog, state.globalBuffIds || [], op.id);
  render();
}

// P10(混合スキル): data-part(1以上)ならrow.extraParts[part-1]、それ以外(未指定/0)なら
// row自身を対象にする共通ヘルパー。multiplier/dmgMultだけがdata-partを
// 持ちうる(selfPct/buffPct/ignoreDef等は元々data-part無しの入力なので、このヘルパーを
// 通しても常にrow自身が返る)。
function partTarget(row, el) {
  const partIndex = el.dataset.part ? Number(el.dataset.part) : 0;
  if (partIndex <= 0) return row;
  if (!row.extraParts) row.extraParts = [];
  if (!row.extraParts[partIndex - 1]) row.extraParts[partIndex - 1] = { multiplier: 0, dmgType: "physical", dmgMult: 1 };
  return row.extraParts[partIndex - 1];
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
    // P10(混合スキル): パーツ1以降も新しいentryのカタログ既定値へ作り直す。
    row.extraParts = makeDefaultExtraParts(entry, row.skillLevel);
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
    // P10(混合スキル): パーツ1以降もこの時点のカタログ既定値へ再スナップする。
    row.extraParts = makeDefaultExtraParts(entry, row.skillLevel);
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
    // P8: 潜在セレクト(0〜5の数値。旧「攻撃凸」チェックボックスの後継)。
    row.potential = Number(el.value);
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
    // P10(混合スキル): data-partが付いていればそのパーツ(row.extraParts[part-1])を、
    // 無ければ従来通りrow自身(=パーツ0)を更新する。
    partTarget(row, el).dmgType = el.value;
    render();
    return;
  }
  const isPercent = ROW_PERCENT_FIELDS.has(field);
  const value = readFieldValue(el, isPercent);
  const target = partTarget(row, el);
  // 理由はonEnemyFieldChangeと同じ（blur時のchangeでクリック中のボタンを差し替えない）。
  if (valuesEqual(target[field], value)) return;
  target[field] = value;
  renderLive();
}

function onResetField(idx, field, partIndex = 0) {
  const row = state.rows[idx];
  const op = findOperator(catalog, row.opId);
  const entry = op ? findEntry(op, row.entryIdx) : null;
  // P10(混合スキル): partIndexが1以上ならentry.parts[partIndex]基準の既定値を
  // row.extraParts[partIndex-1]へ書き戻す。
  if (partIndex > 0) {
    const part = entry && entry.parts && entry.parts[partIndex];
    const defaults = partCatalogDefaults(part, row.skillLevel);
    if (defaults[field] === null || defaults[field] === undefined) return;
    if (!row.extraParts) row.extraParts = [];
    if (!row.extraParts[partIndex - 1]) row.extraParts[partIndex - 1] = { multiplier: 0, dmgType: "physical", dmgMult: 1 };
    row.extraParts[partIndex - 1][field] = defaults[field];
    render();
    return;
  }
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
  setGlobalBuffsOpen(true); // チップを操作した=開いて見ている最中なので、再描画後も開いたままにする
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
  if (el.closest("#global-buffs-details")) setGlobalBuffsOpen(true);
  render();
}

/* ---------------- イベント処理: 鼓舞ソース(P3) ---------------- */

function toggleInspireSource(sourceId) {
  const source = (catalog.inspireSources || []).find((s) => s.id === sourceId);
  if (!source) return;
  const cfg = sourceCfg(source);
  setSourceCfg(sourceId, { on: !cfg.on });
  setGlobalBuffsOpen(true); // チップを操作した=開いて見ている最中なので、再描画後も開いたままにする
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

// select/checkbox(スキル・潜在・モジュール・モジュールLv・自己%パーツ)は
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
  if (field === "potential") {
    // P8: 潜在セレクト(0〜5の数値。旧「攻撃凸」+「素質凸」チェックボックスの後継)。
    setSourceCfg(sourceId, { potential: Number(el.value) });
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
  setExpandedIdx(state.rows.length - 1);
  render();
}

function dupRow(idx) {
  const copy = JSON.parse(JSON.stringify(state.rows[idx]));
  state.rows.splice(idx + 1, 0, copy);
  render();
}

function delRow(idx) {
  state.rows.splice(idx, 1);
  if (expandedIdx === idx) setExpandedIdx(null);
  else if (expandedIdx != null && expandedIdx > idx) setExpandedIdx(expandedIdx - 1);
  render();
}

function editRow(idx) {
  setExpandedIdx(idx);
  render();
}

function collapseRow() {
  setExpandedIdx(null);
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

// datalistのサジェスト選択による"input"か。Chromium/Safariは
// inputType="insertReplacementText"のInputEvent、Firefoxはinputの無い素のEventを投げる。
// 手打ちで名前が完全一致しただけでは確定しない(「アーミヤ」→「アーミヤ（医療）」のように
// 別オペレーター名の途中で確定してしまうため)。
function isDatalistPick(ev) {
  return typeof InputEvent === "undefined" || !(ev instanceof InputEvent) || ev.inputType === "insertReplacementText";
}

export function onAppInput(ev) {
  const el = ev.target;
  if (!el.dataset) return;
  // オペレーター名入力は「値がop.nameから導出される」フィールドなので、毎キー入力で
  // 再描画してしまうと確定前の文字列がop.nameで即座に上書きされ、入力できなくなる。
  // 確定は"change"（blur/Enter）に任せ、打鍵の"input"では何もしない。
  // ただしdatalistのサジェストを選んだ時だけはその場で確定する(blurを待たない)。
  if (el.dataset.field === "opName") {
    if (isDatalistPick(ev) && catalog.operators.some((o) => o.name === el.value)) {
      onRowFieldChange(el);
    }
    return;
  }
  if (el.dataset.role === "enemy") onEnemyFieldChange(el);
  else if (el.dataset.role === "row" && el.tagName === "INPUT" && el.type !== "checkbox") onRowFieldChange(el);
  else if (el.dataset.role === "inspire" && el.tagName === "INPUT" && el.type !== "checkbox") onInspireFieldChange(el);
}

export function onAppChange(ev) {
  const el = ev.target;
  if (!el.dataset) return;
  if (el.dataset.role === "enemy") onEnemyFieldChange(el);
  else if (el.dataset.role === "row") onRowFieldChange(el);
  else if (el.dataset.role === "inspire") onInspireFieldChange(el);
  else if (el.dataset.role === "global-buff-level") onGlobalBuffLevelChange(el);
}

export function onAppClick(ev) {
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
      onResetField(idx, btn.dataset.field, btn.dataset.part ? Number(btn.dataset.part) : 0);
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
