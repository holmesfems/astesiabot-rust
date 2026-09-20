import { T, S } from '../constants/i18n.js';
import { escapeHtml, applyInline, highlightGlossaryHtml, parseProcedure, decodeArrayBufferAuto } from '../core/parser.js';
import {
  state, currentItem, currentBuildLabel, countStatusesInSection, computeOverallStats,
  computeRank, computeMaxCombo, computeTotalDurationMs, sectionLabelFor, clearSession, loadSessionRaw
} from '../core/state.js';
import { formatDuration } from '../core/score.js';
import { buildProgressExportObject, buildExportRows, toMarkdownTable, toTsv, toCsv, buildBusinessReport } from '../core/io.js';
import { el, showScreen, copyToClipboard, downloadProgressJson, downloadCsvBom, exportProgressFromState, copyShareUrl, copyProcedureShareUrl, showToast } from './dom.js';
import {
  openConfirmModal, handleLoadedText, renderParseError, resumeNeedsConfirm, resumeButtonLabel, currentTesterName,
  openGlossaryModal, closeGlossaryModal, handleTermClick, updateNgConfirmBtnState, triggerNgCommentRequiredWarning, closeCommentPanels,
  startNgFlow
} from './modal.js';
import { startNewSession, resumeSession, recordResult, goBack, startNewSessionDirect, resetToStart, handleProgressFile } from './flow.js';
import { resetNgDodgeState, wireNgDodge, ngDodgeLock } from './effects/dodge.js';
import { spawnConfetti } from './effects/confetti.js';

/* =========================================================================
   スタート画面 / 試験情報 / ステップ画面 / 結果画面 の描画・ワイヤリング
   ========================================================================= */
export function renderGlossaryHtml(preamble) {
  if (!preamble || preamble.length === 0) return '<p style="font-size:13px;color:var(--text-dim);">' + T.noPreambleOrGlossary + '</p>';
  var html = '';
  preamble.forEach(function (block) {
    html += '<div class="glossary-block"><h4>' + escapeHtml(block.heading) + '</h4>';
    html += renderBodyLinesHtml(block.bodyLines);
    html += '</div>';
  });
  return html;
}

function renderBodyLinesHtml(bodyLines) {
  if (!bodyLines || bodyLines.length === 0) return '';
  var html = '';
  var i = 0;
  while (i < bodyLines.length) {
    var line = bodyLines[i];
    if (line.trim() === '') { i++; continue; }
    if (/^[*\-]\s+/.test(line.trim())) {
      var items = [];
      while (i < bodyLines.length && /^[*\-]\s+/.test(bodyLines[i].trim())) {
        items.push(applyInline(bodyLines[i].trim().replace(/^[*\-]\s+/, '')));
        i++;
      }
      html += '<ul>' + items.map(function (t) { return '<li>' + t + '</li>'; }).join('') + '</ul>';
      continue;
    }
    var paraLines = [];
    while (i < bodyLines.length && bodyLines[i].trim() !== '' && !/^[*\-]\s+/.test(bodyLines[i].trim())) {
      paraLines.push(applyInline(bodyLines[i].trim()));
      i++;
    }
    html += '<p>' + paraLines.join('<br>') + '</p>';
  }
  return html;
}

// 確認画面のリンク1行。ここだけで「意図した手順を読み込めたか」が分かるようにする
export function renderProcedureLink(result) {
  el('confirm-preview-link').textContent =
    '🔍 ' + (result.title || T.untitled) +
    T.procedureLinkSummary(result.sections.length, result.totalItems);
}

// 手順の内訳。確認画面には出さず、リンクから開くモーダルに描く
export function renderPreview(result) {
  var box = el('procedure-modal-content');
  var testSections = result.sections;
  var html = '';
  html += '<div class="summary-grid">';
  html += '<div class="summary-cell"><div class="label">' + T.docTitleLabel + '</div><div class="value" style="font-size:14px;">' + escapeHtml(result.title || T.untitled) + '</div></div>';
  html += '<div class="summary-cell"><div class="label">' + T.sectionCountLabel + '</div><div class="value">' + testSections.length + '</div></div>';
  html += '<div class="summary-cell"><div class="label">' + T.itemCountLabel + '</div><div class="value">' + result.totalItems + '</div></div>';
  html += '</div>';

  html += '<ul class="section-list">';
  testSections.forEach(function (sec, idx) {
    var tagBadge = sec.tag ? '<span class="badge badge-os">' + escapeHtml(sec.tag) + '</span>' : '';
    html += '<li><span>' + T.sectionNumberTitle(idx + 1, escapeHtml(sec.title || T.untitled)) + ' ' + tagBadge + '</span><span class="badge">' + T.itemCountBadge(sec.items.length) + '</span></li>';
  });
  html += '</ul>';

  if (result.preamble && result.preamble.length > 0) {
    html += '<details class="glossary-details"><summary>' + T.readPreambleSummary + '</summary>' + renderGlossaryHtml(result.preamble) + '</details>';
  }

  box.innerHTML = html;
}

// 節タグ（【Windows】等）を出現順・重複排除で集める
function collectOsTags(sections) {
  var tags = [];
  var seen = {};
  (sections || []).forEach(function (sec) {
    var t = sec.tag ? String(sec.tag).trim() : '';
    if (!t || seen[t]) return;
    seen[t] = true;
    tags.push(t);
  });
  return tags;
}

// 配布物のリンク操作。http(s) 以外のURLはボタンごと出さない
function materialActionsHtml(material) {
  if (!material.url || !/^https?:\/\//i.test(material.url)) return '';
  var safeUrl = escapeHtml(material.url);
  return '<a class="btn btn-ghost btn-small" href="' + safeUrl + '" target="_blank" rel="noopener noreferrer">' + T.open + '</a>' +
    '<button type="button" class="btn btn-ghost btn-small material-copy-btn" data-url="' + safeUrl + '">' + T.copyLink + '</button>';
}

// 配布物ポップアップ用。主アクションの「わかった！」ボタンと競合しないよう、
// ボタン形状ではなく控えめなテキストリンク形式にする
export function materialPopupActionsHtml(material) {
  if (!material.url || !/^https?:\/\//i.test(material.url)) return '';
  var safeUrl = escapeHtml(material.url);
  return '<a class="link-button term-popup-link" href="' + safeUrl + '" target="_blank" rel="noopener noreferrer">↗ ' + T.open + '</a>' +
    '<span class="term-popup-link-sep" aria-hidden="true">|</span>' +
    '<button type="button" class="link-button term-popup-link material-copy-btn" data-url="' + safeUrl + '">📋 ' + T.copyLink + '</button>';
}

function materialRowHtml(material) {
  return '<div class="material-row">' +
    '<div class="material-main">' +
    '<div class="material-name">📎 ' + escapeHtml(material.name) + '</div>' +
    (material.descHtml ? '<div class="material-desc">' + material.descHtml + '</div>' : '') +
    '</div>' +
    '<div class="material-actions">' + materialActionsHtml(material) + '</div>' +
    '</div>';
}

export function materialsSectionHtml(materials) {
  if (!materials || materials.length === 0) return '';
  return '<div class="glossary-block"><h4>📎 ' + T.providedFilesHeading + '</h4>' +
    materials.map(function (m) { return materialRowHtml(m); }).join('') + '</div>';
}

export function handleMaterialCopyClick(e) {
  var btn = (e.target && e.target.closest) ? e.target.closest('.material-copy-btn') : null;
  if (!btn) return;
  var url = btn.getAttribute('data-url');
  if (url) copyToClipboard(url, T.linkCopiedToast);
}

export function clearBuildInputWarning() {
  var inp = el('build-input');
  if (inp) inp.classList.remove('shake-warn');
}

export function triggerBuildInputWarning() {
  var inp = el('build-input');
  if (inp) {
    inp.classList.remove('shake-warn');
    void inp.offsetWidth;
    inp.classList.add('shake-warn');
    try { inp.focus(); } catch (err) { /* ignore */ }
  }
  showToast(T.enterBuildPrompt);
}

// 記入モードでビルドが空なら開始させない（空欄を揺らして知らせるだけ）
export function ensureBuildEntered(result) {
  var build = (result && result.build) ? result.build : { mode: 'none' };
  if (build.mode !== 'input') return true;
  if (el('build-input').value.trim() !== '') return true;
  triggerBuildInputWarning();
  return false;
}

export function renderTestInfo(result) {
  var build = (result && result.build) ? result.build : { mode: 'none' };
  var materials = (result && result.materials) ? result.materials : [];
  var osTags = result ? collectOsTags(result.sections) : [];

  clearBuildInputWarning();

  el('build-fixed-row').hidden = (build.mode !== 'fixed');
  if (build.mode === 'fixed') el('build-fixed-chip').textContent = T.buildLabel + ' ' + build.value;

  el('build-input-row').hidden = (build.mode !== 'input');
  if (build.mode === 'input') {
    // 読み取り場所の案内はラベルに畳み込む（別行に出すと見出しと解説の区別が付かないため）
    el('build-input-label').textContent = T.buildRequiredLabel(build.hint);
  }

  el('os-row').hidden = (osTags.length === 0);
  if (osTags.length > 0) {
    el('os-badges').innerHTML = '<span class="badge badge-os">🖥 ' + escapeHtml(osTags.join(' / ')) + '</span>';
  }

  el('materials-row').hidden = (materials.length === 0);
  if (materials.length > 0) {
    // 見出しは「用意してください」固定。配布物はURL付きとは限らない（手渡し・共有フォルダ等）ので、
    // 「ダウンロード」と言い切れる条件を判定するより、どちらでも通る言い方にしておく
    el('materials-list').innerHTML = materials.map(function (m) { return materialRowHtml(m); }).join('');
  }
}

export function wireStartScreen() {
  el('ai-prompt-copy-btn').addEventListener('click', function () {
    copyToClipboard(S.AI_FORMAT_PROMPT, T.aiPromptCopiedToast);
  });

  el('file-select-btn').addEventListener('click', function () { el('file-input').click(); });
  el('file-input').addEventListener('change', function (e) {
    var f = e.target.files && e.target.files[0];
    if (!f) return;
    var reader = new FileReader();
    reader.onload = function () {
      try {
        var text = decodeArrayBufferAuto(reader.result);
        handleLoadedText(text);
      } catch (err) {
        renderParseError(T.fileReadError(err.message));
      }
    };
    reader.readAsArrayBuffer(f);
  });

  var dz = el('dropzone');
  dz.addEventListener('dragover', function (e) { e.preventDefault(); dz.classList.add('dragover'); });
  dz.addEventListener('dragleave', function () { dz.classList.remove('dragover'); });
  dz.addEventListener('drop', function (e) {
    e.preventDefault();
    dz.classList.remove('dragover');
    var f = e.dataTransfer.files && e.dataTransfer.files[0];
    if (!f) return;
    var reader = new FileReader();
    reader.onload = function () {
      try {
        var text = decodeArrayBufferAuto(reader.result);
        handleLoadedText(text);
      } catch (err) {
        renderParseError(T.fileReadError(err.message));
      }
    };
    reader.readAsArrayBuffer(f);
  });
  dz.addEventListener('click', function () { el('file-input').click(); });
  dz.addEventListener('keydown', function (e) { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); el('file-input').click(); } });

  el('paste-start-btn').addEventListener('click', function () {
    var text = el('paste-textarea').value;
    if (!text || text.trim() === '') {
      renderParseError(T.noTextEntered);
      return;
    }
    var result = parseProcedure(text);
    if (!result.ok || result.totalItems === 0) {
      renderParseError(T.noItemsDetected);
      return;
    }
    openConfirmModal({
      result: result,
      rawText: text,
      buttonLabel: '✅ ' + T.readyLabel,
      testerName: '',
      buildEntered: '',
      onStart: function () { startNewSession(result, text); }
    });
  });

  el('share-procedure-start-btn').addEventListener('click', function (e) {
    copyProcedureShareUrl(el('paste-textarea').value, !!(e && e.shiftKey));
  });

  el('sample-a-btn').addEventListener('click', function () {
    el('paste-textarea').value = S.SAMPLE_A;
    el('error-box').hidden = true;
    showToast(T.sampleALoadedToast);
  });
  el('sample-b-btn').addEventListener('click', function () {
    el('paste-textarea').value = S.SAMPLE_B;
    el('error-box').hidden = true;
    showToast(T.sampleBLoadedToast);
  });

  el('resume-btn').addEventListener('click', function () {
    var saved = loadSessionRaw();
    if (!saved) return;
    var result = parseProcedure(saved.rawText);
    if (result.ok && resumeNeedsConfirm(saved, result, false)) {
      openConfirmModal({
        result: result,
        rawText: saved.rawText,
        buttonLabel: resumeButtonLabel(saved),
        testerName: saved.testerName || '',
        buildEntered: saved.buildEntered || '',
        onStart: function () {
          saved.testerName = currentTesterName();
          saved.buildEntered = el('build-input').value.trim();
          resumeSession(saved);
        }
      });
    } else {
      resumeSession(saved);
    }
  });

  el('export-progress-btn').addEventListener('click', function () {
    var saved = loadSessionRaw();
    if (!saved || !saved.rawText) { showToast(T.noProgressToExport); return; }
    var fname = downloadProgressJson(buildProgressExportObject(saved));
    showToast(T.progressExported(fname));
  });

  el('progress-file-select-btn').addEventListener('click', function () { el('progress-file-input').click(); });
  el('progress-file-input').addEventListener('change', function (e) {
    var f = e.target.files && e.target.files[0];
    if (f) handleProgressFile(f);
  });

  var pdz = el('progress-dropzone');
  pdz.addEventListener('dragover', function (e) { e.preventDefault(); pdz.classList.add('dragover'); });
  pdz.addEventListener('dragleave', function () { pdz.classList.remove('dragover'); });
  pdz.addEventListener('drop', function (e) {
    e.preventDefault();
    pdz.classList.remove('dragover');
    var f = e.dataTransfer.files && e.dataTransfer.files[0];
    if (f) handleProgressFile(f);
  });
  pdz.addEventListener('click', function () { el('progress-file-input').click(); });
  pdz.addEventListener('keydown', function (e) { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); el('progress-file-input').click(); } });
}

export function setupGlossaryFab() {
  var fab = el('glossary-fab-btn');
  var hasPreamble = !!(state.preamble && state.preamble.length > 0);
  var hasMaterials = !!(state.materials && state.materials.length > 0);
  fab.hidden = !(hasPreamble || hasMaterials);
}

var stepTimerInterval = null;
export function stopStepTimer() {
  if (stepTimerInterval) { clearInterval(stepTimerInterval); stepTimerInterval = null; }
}

function updateStepTimerDisplay() {
  var node = el('step-timer-current');
  if (!node) return;
  var elapsed = state.stepEnteredAt ? (Date.now() - state.stepEnteredAt) : 0;
  node.textContent = '⏱ ' + T.thisStepLabel + ' ' + formatDuration(elapsed);
}

function startStepTimer() {
  stopStepTimer();
  updateStepTimerDisplay();
  stepTimerInterval = setInterval(updateStepTimerDisplay, 1000);
}

export function renderStep(animate) {
  hideActionPanels();
  resetNgDodgeState();
  var item = currentItem();
  var sIdx = item.sectionIndex;
  var sec = state.sections[sIdx];

  el('step-doc-title').textContent = state.docTitle || T.untitledProcedure;
  var buildLabel = currentBuildLabel();
  el('step-build-chip').textContent = buildLabel ? (T.buildLabel + ' ' + buildLabel) : '';
  el('step-build-chip').hidden = !buildLabel;
  el('step-section-title').textContent = (sec.number ? sec.number + '. ' : '') + (sec.title || T.untitledSection);

  var badgesHtml = '<span class="badge">' + T.sectionOfTotal(sIdx + 1, state.sections.length) + '</span>';
  el('step-badges').innerHTML = badgesHtml;

  var answeredCount = state.results.filter(function (r) { return r && r.status; }).length;
  var totalCount = state.flatItems.length;
  var pct = totalCount > 0 ? Math.round((answeredCount / totalCount) * 100) : 0;
  el('progress-bar-inner').style.width = pct + '%';
  el('progress-percent').textContent = T.percentComplete(pct);

  var secCounts = countStatusesInSection(sIdx);
  el('progress-remain-section').textContent = T.remainingInSection(secCounts.unanswered);
  var remainSections = state.sections.length - (sIdx + 1);
  var remainTotalItems = totalCount - answeredCount;
  el('progress-remain-total').textContent = T.remainingOverall(remainTotalItems, remainSections);

  var dotsHtml = '';
  state.sections.forEach(function (s, idx) {
    var c = countStatusesInSection(idx);
    var cls = 'dot';
    if (idx === sIdx) cls += ' current';
    else if (c.unanswered === 0) cls += ' done';
    dotsHtml += '<span class="' + cls + '" title="' + T.sectionNumberTitle(idx + 1, escapeHtml(s.title || '')) + '"></span>';
  });
  el('dots-row').innerHTML = dotsHtml;

  var existing = state.results[state.pointer];
  var statusPill = '';
  if (existing && existing.status) {
    var label = existing.status === 'ok' ? 'OK' : 'NG';
    statusPill = '<span class="status-pill ' + existing.status + '">' + T.recordedLabel(label) + '</span>';
  }
  var osBadgeHtml = sec.tag ? '<span class="badge-os-lg">🖥 ' + escapeHtml(sec.tag) + '</span>' : '';
  el('item-number').innerHTML =
    '<span class="item-number-text">' + T.itemNumberLabel(escapeHtml(item.number || String(item.itemIndexInSection + 1))) + '</span>' +
    osBadgeHtml + statusPill;
  var stepHl = highlightGlossaryHtml(item.stepHtml, state.glossary, state.materials);
  var expHl = highlightGlossaryHtml(item.expectedHtml, state.glossary, state.materials);
  el('item-step-body').innerHTML = stepHl.html;
  el('item-expected-body').innerHTML = expHl.html;
  updateTermHint(stepHl, expHl);

  if (sec.note && sec.note.trim() !== '') {
    var noteHtml = sec.note.split('\n').map(function (l) { return applyInline(l); }).join('<br>');
    el('section-note-line').innerHTML = '📝 ' + T.sectionNoteLabel + ': ' + noteHtml;
    el('section-note-line').hidden = false;
  } else {
    el('section-note-line').hidden = true;
  }

  el('back-btn').disabled = (state.pointer === 0);
  el('ok-btn').disabled = false;
  el('ng-btn').disabled = false;

  // 「戻る」で記録済みNGの項目に来たときは、前回のコメントを捨てずにNGパネルへ事前投入しておく
  if (existing && existing.status === 'ng') {
    el('ng-comment').value = existing.comment || '';
    updateNgConfirmBtnState();
  }

  if (existing && existing.durationMs != null) {
    el('step-timer-prev').textContent = T.previousDuration(formatDuration(existing.durationMs));
    el('step-timer-prev').hidden = false;
  } else {
    el('step-timer-prev').hidden = true;
  }
  state.stepEnteredAt = Date.now();
  startStepTimer();

  var card = el('step-card');
  card.classList.remove('slide-in');
  if (animate) {
    void card.offsetWidth;
    card.classList.add('slide-in');
  }
}

// 金色=用語 / 水色=配布物。実際にマッチしたものだけを案内する
function updateTermHint(stepHl, expHl) {
  var node = el('term-hint');
  var hasTerm = !!(stepHl.matchedTerm || expHl.matchedTerm);
  var hasMaterial = !!(stepHl.matchedMaterial || expHl.matchedMaterial);
  if (!hasTerm && !hasMaterial) { node.hidden = true; return; }
  node.textContent = T.termHint(hasTerm, hasMaterial);
  node.hidden = false;
}

export function hideActionPanels() {
  el('ng-panel').hidden = true;
  el('action-row').hidden = false;
  el('ng-comment').value = '';
  el('ng-inline-warn').hidden = true;
  el('ng-comment').classList.remove('shake-warn');
  updateNgConfirmBtnState();
}

export function openCommentPanel() {
  el('action-row').hidden = true;
  el('ng-panel').hidden = false;
  updateNgConfirmBtnState();
  el('ng-comment').focus();
}

export function wireStepScreen() {
  el('ok-btn').addEventListener('click', function () { recordResult('ok', ''); });
  el('ng-btn').addEventListener('click', function () { if (ngDodgeLock) return; startNgFlow(); });
  el('back-btn').addEventListener('click', goBack);
  wireNgDodge();

  el('ng-confirm-btn').addEventListener('click', function () {
    var comment = el('ng-comment').value.trim();
    if (!comment) { triggerNgCommentRequiredWarning(); return; }
    recordResult('ng', comment);
  });
  el('ng-cancel-btn').addEventListener('click', closeCommentPanels);

  el('ng-comment').addEventListener('input', function () {
    updateNgConfirmBtnState();
    if (el('ng-comment').value.trim() !== '') el('ng-inline-warn').hidden = true;
  });
  el('ng-comment').addEventListener('animationend', function () {
    el('ng-comment').classList.remove('shake-warn');
  });
  el('ng-comment').addEventListener('keydown', function (e) {
    if (e.ctrlKey && e.key === 'Enter') {
      e.preventDefault();
      if (!el('ng-confirm-btn').disabled) el('ng-confirm-btn').click();
      else triggerNgCommentRequiredWarning();
    }
  });
  el('glossary-fab-btn').addEventListener('click', openGlossaryModal);
  el('glossary-close-btn').addEventListener('click', closeGlossaryModal);

  el('share-procedure-step-btn').addEventListener('click', function (e) { copyProcedureShareUrl(state.rawText, !!(e && e.shiftKey)); });

  el('item-step-body').addEventListener('click', handleTermClick);
  el('item-expected-body').addEventListener('click', handleTermClick);
}

export function goToResultScreen() {
  stopStepTimer();
  state.transitioning = false; // the finale path never passes through advanceAfterResult, so release the guard here
  clearSession();
  showScreen('screen-result');
  renderResultScreen();
  maybeFireAllOkConfetti();
}

/* 全項目 OK（NG 0 かつ未実施 0）のときだけ、結果画面遷移時に一度だけ紙吹雪を降らせる */
function maybeFireAllOkConfetti() {
  var stats = computeOverallStats();
  var allOk = stats.total > 0 && stats.ng === 0 && stats.unanswered === 0 && stats.ok === stats.total;
  if (!allOk) return;
  [0, 400, 900, 1500].forEach(function (delay) {
    setTimeout(function () { spawnConfetti(100, 0.05); }, delay);
  });
  var sparkleDuration = 6000;
  var sparkleInterval = setInterval(function () { spawnConfetti(40, 0.1); }, 1200);
  setTimeout(function () { clearInterval(sparkleInterval); }, sparkleDuration + 200);
}

function buildSummaryHtml() {
  var stats = computeOverallStats();
  var rate = stats.total > 0 ? Math.round((stats.ok / stats.total) * 100) : 0;
  var rank = computeRank(stats);
  var maxCombo = Math.max(state.combo, computeMaxCombo());

  var testerHasName = !!state.testerName;
  var testerVal = state.testerName || T.notEntered;

  var html = '';
  html += summaryCell(T.testerLabel, testerVal, testerHasName ? 'value-rainbow' : '');
  html += summaryCell(T.dateTimeLabel, new Date().toLocaleString(T.dateLocale));
  html += summaryCell(T.docTitleLabel, state.docTitle || T.untitled);
  var buildLabel = currentBuildLabel();
  if (buildLabel) html += summaryCell(T.buildLabel, buildLabel);
  html += summaryCell(T.totalLabel, stats.total);
  html += summaryCell('OK', stats.ok);
  html += summaryCell('NG', stats.ng);
  html += summaryCell(T.statusNotDone, stats.unanswered);
  html += summaryCell(T.okRateLabel, rate + '%');
  html += summaryCell(T.overallRankLabel, rank);
  html += summaryCell(T.scoreLabel, state.score + ' pt');
  html += summaryCell(T.maxComboLabel, 'x' + maxCombo);
  html += summaryCell(T.totalTimeLabel, formatDuration(computeTotalDurationMs()));
  return html;
}

/* サマリー部分（summary-grid・NG警告バナー）のみ再描画する。表は再描画しない */
function refreshResultSummary() {
  el('summary-grid').innerHTML = buildSummaryHtml();
  updateNgCommentWarningBanner();
}

export function renderResultScreen() {
  refreshResultSummary();
  renderResultTable();
}

function hasEmptyNgComment() {
  return state.results.some(function (r) {
    return !!(r && r.status === 'ng' && (!r.comment || !String(r.comment).trim()));
  });
}

function updateNgCommentWarningBanner() {
  var banner = el('ng-comment-warning');
  if (!banner) return;
  banner.hidden = !hasEmptyNgComment();
}

function summaryCell(label, value, extraClass) {
  var valueClass = 'value' + (extraClass ? ' ' + extraClass : '');
  return '<div class="summary-cell"><div class="label">' + escapeHtml(label) + '</div><div class="' + valueClass + '">' + escapeHtml(String(value)) + '</div></div>';
}

function renderResultTable() {
  var tbody = el('result-table-body');
  var rows = '';
  state.flatItems.forEach(function (item, idx) {
    var r = state.results[idx] || { status: null, comment: '', durationMs: null };
    var rowClass = r.status === 'ng' ? 'row-ng' : '';
    var timeStr = r.timestamp ? new Date(r.timestamp).toLocaleString(T.dateLocale) : '';
    var durationStr = (r.durationMs != null) ? formatDuration(r.durationMs) : '';
    var commentEmpty = !r.comment || !String(r.comment).trim();
    var commentErrClass = (r.status === 'ng' && commentEmpty) ? ' input-error' : '';
    rows +=
      '<tr class="' + rowClass + '" data-idx="' + idx + '">' +
      '<td>' + escapeHtml(item.number || String(idx + 1)) + '</td>' +
      '<td>' + escapeHtml(sectionLabelFor(item.sectionIndex)) + '</td>' +
      '<td>' + item.stepHtml + '</td>' +
      '<td>' + item.expectedHtml + '</td>' +
      '<td>' + resultSelectHtml(idx, r.status) + '</td>' +
      '<td><input type="text" class="comment-input' + commentErrClass + '" data-idx="' + idx + '" value="' + escapeHtml(r.comment || '') + '"></td>' +
      '<td>' + escapeHtml(timeStr) + '</td>' +
      '<td>' + escapeHtml(durationStr) + '</td>' +
      '</tr>';
  });
  tbody.innerHTML = rows;

  tbody.querySelectorAll('select.result-select').forEach(function (sel) {
    sel.addEventListener('change', function () {
      var idx = parseInt(sel.getAttribute('data-idx'), 10);
      var tr = sel.closest('tr');
      var commentInput = tr.querySelector('input.comment-input');
      var newStatus = sel.value || null;

      var r = state.results[idx] || { status: null, comment: '', timestamp: null };
      r.status = newStatus;
      r.timestamp = new Date().toISOString();
      state.results[idx] = r;

      tr.classList.remove('row-ng');
      if (newStatus === 'ng') tr.classList.add('row-ng');

      var commentEmpty = !commentInput || !commentInput.value.trim();
      if (newStatus === 'ng' && commentEmpty) {
        if (commentInput) {
          commentInput.classList.add('input-error');
          commentInput.focus();
        }
        showToast(T.ngCommentRequiredToast);
      } else if (commentInput) {
        commentInput.classList.remove('input-error');
      }

      // 表は再描画しない（select 変更でフォーカスが失われるのを防ぐ）。サマリー数値のみ更新する
      refreshResultSummary();
    });
  });
  tbody.querySelectorAll('input.comment-input').forEach(function (inp) {
    inp.addEventListener('input', function () {
      var idx = parseInt(inp.getAttribute('data-idx'), 10);
      var r = state.results[idx] || { status: null, comment: '', timestamp: null };
      r.comment = inp.value;
      state.results[idx] = r;
      if (inp.value.trim() !== '') {
        inp.classList.remove('input-error');
      } else if (r.status === 'ng') {
        inp.classList.add('input-error');
      }
      updateNgCommentWarningBanner();
    });
  });
}

function resultSelectHtml(idx, status) {
  var options = [
    { v: '', l: T.notDoneOption },
    { v: 'ok', l: 'OK' },
    { v: 'ng', l: 'NG' }
  ];
  var current = status || '';
  var html = '<select class="result-select" data-idx="' + idx + '">';
  options.forEach(function (o) {
    html += '<option value="' + o.v + '"' + (current === o.v ? ' selected' : '') + '>' + o.l + '</option>';
  });
  html += '</select>';
  return html;
}

export function wireResultScreen() {
  el('copy-md-btn').addEventListener('click', function () { copyToClipboard(toMarkdownTable(buildExportRows())); });
  el('copy-tsv-btn').addEventListener('click', function () { copyToClipboard(toTsv(buildExportRows())); });
  el('download-csv-btn').addEventListener('click', function () {
    downloadCsvBom(toCsv(buildExportRows()), 'test_result.csv');
  });
  el('print-btn').addEventListener('click', function () { window.print(); });
  el('copy-report-btn').addEventListener('click', function () {
    copyToClipboard(buildBusinessReport(), T.reportCopiedToast);
  });
  el('export-progress-result-btn').addEventListener('click', exportProgressFromState);
  el('share-url-result-btn').addEventListener('click', function (e) { copyShareUrl(state, !!(e && e.shiftKey)); });
  el('share-procedure-result-btn').addEventListener('click', function (e) { copyProcedureShareUrl(state.rawText, !!(e && e.shiftKey)); });

  el('restart-btn').addEventListener('click', function () {
    if (!confirm(T.confirmRestart)) return;
    var rawText = state.rawText;
    var testerName = state.testerName;
    var buildEntered = state.buildEntered;
    var result = parseProcedure(rawText);
    startNewSessionDirect(result, rawText, testerName, buildEntered);
  });

  el('load-another-btn').addEventListener('click', function () {
    clearSession();
    resetToStart();
  });
}
