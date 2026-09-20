import { T } from '../constants/i18n.js';
import { STORAGE_KEY } from '../constants/config.js';

/* =========================================================================
   状態管理（DOM非依存。localStorage アクセスは saveSession/clearSession/
   loadSessionRaw の3関数に閉じ込める）
   ========================================================================= */
export var state = {
  docTitle: '',
  preamble: [],
  sections: [],
  glossary: [],
  materials: [],       // { name, key, url, descHtml }（手順書に配布物が無ければ空）
  build: { mode: 'none' }, // 手順書から毎回導出するので保存しない
  buildEntered: '',    // 記入モードでテスターが入力したビルド番号（保存・復元する）
  flatItems: [],       // { sectionIndex, itemIndexInSection, number, stepHtml, expectedHtml, stepRaw, expectedRaw }
  results: [],         // { status: 'ok'|'ng'|null, comment: '', timestamp: null }
  pointer: 0,
  testerName: '',
  startedAt: null,
  score: 0,
  combo: 0,
  transitioning: false, // runtime-only guard against double OK/NG during the advance animation; not persisted (see saveSession)
  stepEnteredAt: null // runtime-only: Date.now() when the current step was rendered; not persisted (see saveSession)
};

export function buildFlatItems() {
  var flat = [];
  state.sections.forEach(function (sec, sIdx) {
    sec.items.forEach(function (it, iIdx) {
      flat.push({
        sectionIndex: sIdx,
        itemIndexInSection: iIdx,
        number: it.number,
        stepHtml: it.stepHtml,
        expectedHtml: it.expectedHtml,
        stepRaw: it.stepRaw,
        expectedRaw: it.expectedRaw
      });
    });
  });
  state.flatItems = flat;
}

export function sectionRange(sIdx) {
  var start = 0;
  for (var i = 0; i < sIdx; i++) start += state.sections[i].items.length;
  var end = start + state.sections[sIdx].items.length;
  return { start: start, end: end };
}

export function saveSession() {
  try {
    var data = {
      rawText: state.rawText,
      testerName: state.testerName,
      buildEntered: state.buildEntered,
      results: state.results,
      pointer: state.pointer,
      startedAt: state.startedAt,
      score: state.score,
      combo: state.combo
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch (e) { /* ignore quota errors etc. */ }
}

export function clearSession() {
  try { localStorage.removeItem(STORAGE_KEY); } catch (e) { /* ignore */ }
}

export function loadSessionRaw() {
  try {
    var raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw);
  } catch (e) { return null; }
}

// 互換: 旧バージョンの「保留」ステータスは未実施(null)として正規化する
export function normalizeStatusCompat(status) {
  return status === 'hold' ? null : status;
}

export function currentItem() { return state.flatItems[state.pointer]; }
// 現在の項目が既にNGとして記録済みか（戻ってきた場合など）
export function currentItemIsNg() {
  var r = state.results[state.pointer];
  return !!(r && r.status === 'ng');
}
export function currentSection() { return state.sections[currentItem().sectionIndex]; }

export function countStatusesInSection(sIdx) {
  var range = sectionRange(sIdx);
  var counts = { ok: 0, ng: 0, total: range.end - range.start, unanswered: 0 };
  for (var i = range.start; i < range.end; i++) {
    var r = state.results[i];
    if (r && r.status === 'ok') counts.ok++;
    else if (r && r.status === 'ng') counts.ng++;
    else counts.unanswered++;
  }
  return counts;
}

export function computeSectionDurationMs(sIdx) {
  var range = sectionRange(sIdx);
  var total = 0;
  for (var i = range.start; i < range.end; i++) {
    var r = state.results[i];
    if (r && r.durationMs) total += r.durationMs;
  }
  return total;
}

export function computeTotalDurationMs() {
  var total = 0;
  state.results.forEach(function (r) { if (r && r.durationMs) total += r.durationMs; });
  return total;
}

// 表示・書き出しに使うビルド番号。固定値はそのまま、記入モードは入力値。無ければ空文字
export function currentBuildLabel() {
  var b = state.build || { mode: 'none' };
  if (b.mode === 'fixed') return String(b.value || '').trim();
  if (b.mode === 'input') return String(state.buildEntered || '').trim();
  return '';
}

export function computeOverallStats() {
  var total = state.results.length;
  var ok = 0, ng = 0, unanswered = 0;
  state.results.forEach(function (r) {
    if (r && r.status === 'ok') ok++;
    else if (r && r.status === 'ng') ng++;
    else unanswered++;
  });
  return { total: total, ok: ok, ng: ng, unanswered: unanswered };
}

export function computeRank(stats) {
  if (stats.total > 0 && stats.ok === stats.total) return T.rankFlawless;
  var rate = stats.total > 0 ? (stats.ok / stats.total) * 100 : 0;
  if (rate >= 90) return T.rankS;
  if (rate >= 75) return T.rankA;
  if (rate >= 50) return T.rankB;
  return T.rankC;
}

export function computeMaxCombo() {
  var max = 0, cur = 0;
  state.results.forEach(function (r) {
    if (r && r.status === 'ok') { cur++; max = Math.max(max, cur); }
    else { cur = 0; }
  });
  return max;
}

export function sectionLabelFor(sIdx) {
  var sec = state.sections[sIdx];
  var label = T.sectionNumberTitle(sIdx + 1, sec.title || T.untitled);
  if (sec.tag) label += '【' + sec.tag + '】';
  return label;
}

export function statusLabel(status) {
  if (status === 'ok') return 'OK';
  if (status === 'ng') return 'NG';
  return T.statusNotDone;
}
