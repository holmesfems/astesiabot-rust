import { T, S } from '../constants/i18n.js';
import { STATE_HASH_PREFIX } from '../constants/config.js';
import { hasLzString, clearStateHash } from '../core/io.js';
import { parseProcedure } from '../core/parser.js';
import { state, buildFlatItems, sectionRange, saveSession, normalizeStatusCompat, currentItem, loadSessionRaw } from '../core/state.js';
import { el, showScreen, showToast, normalizeProgressObject } from './dom.js';
import {
  resumeNeedsConfirm, openConfirmModal, resumeButtonLabel, currentTesterName,
  closeNgConfirmOverlay, closeTermPopup, closeProcedureModal, closeConfirmModal, closeGlossaryModal, startNgFlow
} from './modal.js';
import { setupGlossaryFab, renderStep, stopStepTimer, renderResultScreen, hideActionPanels } from './renderer.js';
import { fireOkCelebration, showSectionCompleteOverlay, currentOverlayPrimaryAction } from './effects/confetti.js';

/* =========================================================================
   セッション開始・再開・進捗URL復元 / 記録・進捗操作 / キーボード操作
   ========================================================================= */

// URLハッシュから進捗オブジェクトを取り出す。無ければ null。復元に失敗したら toast して null。
function readProgressFromHash() {
  var h = window.location.hash || '';
  if (h.indexOf(STATE_HASH_PREFIX) !== 0) return null;
  var packed = h.slice(STATE_HASH_PREFIX.length);
  if (!packed) return null;
  if (!hasLzString()) { showToast(T.lzStringUnavailable); return null; }
  var json = null;
  try { json = LZString.decompressFromEncodedURIComponent(packed); } catch (e) { json = null; }
  if (!json) { showToast(T.urlProgressRestoreFailed); return null; }
  try { return JSON.parse(json); } catch (e) { showToast(T.urlProgressRestoreFailed); return null; }
}

function resumeProgressFromUrl(progress) {
  resumeSession(progress);
  saveSession(); // 復元した内容をこのブラウザの「前回のつづき」にも載せる
  var allDone = state.results.length > 0 && state.results.every(function (r) { return r && r.status; });
  var untouched = state.results.every(function (r) { return !r || !r.status; });
  if (allDone) {
    // 全項目に結果が入っている進捗（結果画面から共有されたもの）はステップ画面ではなく結果画面へ
    stopStepTimer();
    showScreen('screen-result');
    renderResultScreen();
    showToast(T.restoredCompletedFromUrl);
  } else if (untouched) {
    // 記録が一切無い進捗（試験手順だけの共有）は「復元」ではなく「読み込んだ」と案内する
    showToast(T.loadedSharedProcedure);
  } else {
    showToast(T.restoredProgressFromUrl);
  }
}

// ページ読み込み時 / hash 変更時: #state= があれば復元。復元処理に入ったら true。
export function restoreFromHash() {
  var obj = readProgressFromHash();
  if (!obj) return false;
  clearStateHash();
  var progress = normalizeProgressObject(obj);
  if (!progress) return false;
  var result = parseProcedure(progress.rawText, T);
  if (result.ok && resumeNeedsConfirm(progress, result, true)) {
    openConfirmModal({
      result: result,
      rawText: progress.rawText,
      buttonLabel: resumeButtonLabel(progress),
      testerName: progress.testerName || '',
      buildEntered: progress.buildEntered || '',
      onStart: function () {
        progress.testerName = currentTesterName();
        progress.buildEntered = el('build-input').value.trim();
        resumeProgressFromUrl(progress);
      }
    });
  } else {
    resumeProgressFromUrl(progress);
  }
  return true;
}

export function checkResumeAvailable() {
  var saved = loadSessionRaw();
  if (!saved || !saved.rawText) return;
  var result = parseProcedure(saved.rawText, T);
  if (!result.ok) return;
  el('resume-box').hidden = false;
  var answered = (saved.results || []).filter(function (r) { return r && r.status; }).length;
  var total = result.totalItems;
  var when = '';
  try { when = new Date(saved.startedAt).toLocaleString(T.dateLocale); } catch (e) { when = ''; }
  el('resume-info').textContent = T.resumeInfoText(result.title || T.untitledBare, answered, total, saved.testerName, when);
}

export function startNewSession(result, rawText) {
  state.docTitle = result.title;
  state.preamble = result.preamble;
  state.sections = result.sections;
  state.glossary = result.glossary || [];
  state.materials = result.materials || [];
  state.build = result.build || { mode: 'none' };
  state.buildEntered = (state.build.mode === 'input') ? el('build-input').value.trim() : '';
  state.rawText = rawText;
  buildFlatItems();
  state.results = state.flatItems.map(function () { return { status: null, comment: '', timestamp: null, durationMs: null }; });
  state.pointer = 0;
  state.transitioning = false; // reset runtime guard: a new/resumed session must never inherit a stale flag
  state.testerName = currentTesterName();
  state.startedAt = new Date().toISOString();
  state.score = 0;
  state.combo = 0;
  saveSession();
  showScreen('screen-step');
  setupGlossaryFab();
  renderStep(false);
}

export function resumeSession(saved) {
  var result = parseProcedure(saved.rawText, T);
  state.docTitle = result.title;
  state.preamble = result.preamble;
  state.sections = result.sections;
  state.glossary = result.glossary || [];
  state.materials = result.materials || [];
  state.build = result.build || { mode: 'none' };
  state.buildEntered = saved.buildEntered || '';
  state.rawText = saved.rawText;
  buildFlatItems();
  var results = saved.results || [];
  state.results = state.flatItems.map(function (_, idx) {
    var r = results[idx] || { status: null, comment: '', timestamp: null, durationMs: null };
    // 互換: 旧バージョンの「保留」ステータスは未実施(null)として扱う
    // 互換: durationMs が無い旧データは null（計測なし）として扱う
    return { status: normalizeStatusCompat(r.status), comment: r.comment || '', timestamp: r.timestamp || null, durationMs: (r.durationMs != null ? r.durationMs : null) };
  });
  state.pointer = Math.min(saved.pointer || 0, state.flatItems.length - 1);
  state.transitioning = false; // reset runtime guard: a new/resumed session must never inherit a stale flag
  state.testerName = saved.testerName || '';
  state.startedAt = saved.startedAt || new Date().toISOString();
  state.score = saved.score || 0;
  state.combo = saved.combo || 0;
  showScreen('screen-step');
  setupGlossaryFab();
  renderStep(false);
}

// 進捗JSONの読み込み。ファイルを読むだけの処理に見えるが、実際は
// 「確認モーダルを出すか、そのまま再開するか」を決めるセッション復帰フローなので、
// DOMユーティリティ(ui/dom.js)ではなくここに置く。
export function handleProgressFile(file) {
  var reader = new FileReader();
  reader.onload = function () {
    var obj;
    try {
      obj = JSON.parse(reader.result);
    } catch (e) {
      showToast(T.progressReadFailed);
      return;
    }
    var progress = normalizeProgressObject(obj);
    if (!progress) return;
    var result = parseProcedure(progress.rawText, T);
    if (result.ok && resumeNeedsConfirm(progress, result, false)) {
      openConfirmModal({
        result: result,
        rawText: progress.rawText,
        buttonLabel: resumeButtonLabel(progress),
        testerName: progress.testerName || '',
        buildEntered: progress.buildEntered || '',
        onStart: function () {
          progress.testerName = currentTesterName();
          progress.buildEntered = el('build-input').value.trim();
          resumeSession(progress);
        }
      });
    } else {
      resumeSession(progress);
    }
  };
  reader.readAsText(file);
}

export function recordResult(status, comment) {
  if (state.transitioning) return; // guard: ignore double OK/NG while the previous result is still animating to the next item
  var idx = state.pointer;
  var durationMs = state.stepEnteredAt ? (Date.now() - state.stepEnteredAt) : null;
  state.results[idx] = { status: status, comment: comment || '', timestamp: new Date().toISOString(), durationMs: durationMs };
  state.transitioning = true;
  el('ok-btn').disabled = true;
  el('ng-btn').disabled = true;
  el('back-btn').disabled = true;
  if (status === 'ok') {
    state.combo++;
    var gained = 100 * state.combo;
    state.score += gained;
    fireOkCelebration(gained, state.combo);
  } else {
    state.combo = 0;
  }
  saveSession();
  advanceAfterResult(status);
}

function isLastItemOfSection() {
  var item = currentItem();
  var range = sectionRange(item.sectionIndex);
  return state.pointer === range.end - 1;
}

export function isLastSection() {
  return currentItem().sectionIndex === state.sections.length - 1;
}

function advanceAfterResult(status) {
  if (isLastItemOfSection()) {
    showSectionCompleteOverlay();
    return;
  }
  var delay = (status === 'ok') ? 480 : 60;
  setTimeout(function () {
    state.pointer++;
    renderStep(true); // re-enables ok/ng/back buttons
    state.transitioning = false;
  }, delay);
}

export function goBack() {
  if (state.transitioning) return; // guard: ignore back navigation while advancing to the next item
  if (state.pointer === 0) return;
  hideActionPanels();
  state.pointer--;
  saveSession();
  renderStep(false);
}

export function startNewSessionDirect(result, rawText, testerName, buildEntered) {
  state.docTitle = result.title;
  state.preamble = result.preamble;
  state.sections = result.sections;
  state.glossary = result.glossary || [];
  state.materials = result.materials || [];
  state.build = result.build || { mode: 'none' };
  state.buildEntered = buildEntered || '';
  state.rawText = rawText;
  buildFlatItems();
  state.results = state.flatItems.map(function () { return { status: null, comment: '', timestamp: null, durationMs: null }; });
  state.pointer = 0;
  state.transitioning = false; // reset runtime guard: a new/resumed session must never inherit a stale flag
  state.testerName = testerName;
  state.startedAt = new Date().toISOString();
  state.score = 0;
  state.combo = 0;
  saveSession();
  showScreen('screen-step');
  setupGlossaryFab();
  renderStep(false);
}

export function resetToStart() {
  stopStepTimer();
  el('error-box').hidden = true;
  el('paste-textarea').value = S.SAMPLE_A; // 初見でも「この手順で開始」だけで体験できるようサンプルを既定値に
  el('tester-name-input').value = '';
  checkResumeAvailable();
  showScreen('screen-start');
}

export function isTypingTarget(elm) {
  return elm && (elm.tagName === 'TEXTAREA' || elm.tagName === 'INPUT');
}

document.addEventListener('keydown', function (e) {
  if (!el('overlay-ng-confirm').hidden) {
    if (e.key === 'Escape' || e.key === 'Enter') { e.preventDefault(); closeNgConfirmOverlay(); }
    return;
  }
  if (!el('overlay-term').hidden) {
    if (e.key === 'Escape' || e.key === 'Enter') { e.preventDefault(); closeTermPopup(); }
    return;
  }
  if (!el('overlay-section-complete').hidden || !el('overlay-finale').hidden) {
    if (e.key === 'Enter' && currentOverlayPrimaryAction) {
      e.preventDefault();
      currentOverlayPrimaryAction();
    }
    return;
  }
  if (!el('modal-procedure').hidden) {
    if (e.key === 'Escape' || e.key === 'Enter') { e.preventDefault(); closeProcedureModal(); }
    return;
  }
  // Enter は確認モーダル内のテスター名・ビルド欄が拾う（ここでも拾うと二重に開始してしまう）
  if (!el('modal-confirm').hidden) {
    if (e.key === 'Escape') closeConfirmModal();
    return;
  }
  if (!el('modal-glossary').hidden) {
    if (e.key === 'Escape') closeGlossaryModal();
    return;
  }
  if (el('screen-step').hidden) return;
  if (isTypingTarget(document.activeElement)) return;

  var ngOpen = !el('ng-panel').hidden;
  if (ngOpen) return;
  if (state.transitioning) return; // ignore Enter/O/N/Backspace while advancing to the next item

  switch (e.key) {
    case 'Enter':
    case 'o':
    case 'O':
      e.preventDefault();
      recordResult('ok', '');
      break;
    case 'n':
    case 'N':
      e.preventDefault();
      startNgFlow();
      break;
    case 'Backspace':
    case 'ArrowLeft':
      e.preventDefault();
      goBack();
      break;
  }
});
