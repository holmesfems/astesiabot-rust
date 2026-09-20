import { T, P } from '../constants/i18n.js';
import { escapeHtml, parseProcedure } from '../core/parser.js';
import { state, currentItem, currentItemIsNg } from '../core/state.js';
import { pickRandom } from '../core/score.js';
import { el, showScreen } from './dom.js';
import { renderProcedureLink, renderPreview, renderTestInfo, stopStepTimer, ensureBuildEntered, clearBuildInputWarning, triggerBuildInputWarning, materialsSectionHtml, renderGlossaryHtml, materialPopupActionsHtml, openCommentPanel } from './renderer.js';
import { startNewSession } from './flow.js';
import { spawnConfetti } from './effects/confetti.js';

/* =========================================================================
   開始前の確認モーダル / NG確認オーバーレイ / 用語・配布物ポップアップ
   ========================================================================= */
var confirmContext = null;

var defaultTesterName;

// 確認モーダルの placeholder に出す既定テスター名。ページロードあたり1回だけ抽選する
// （main.js の boot() が installI18n の直後・init() より前に1回だけ呼ぶ）
export function pickDefaultTesterName() {
  defaultTesterName = pickRandom(P.TESTER_NAME_POOL);
}

// 入力が空ならプレースホルダに出している名前をそのまま採用する
export function currentTesterName() {
  var typed = el('tester-name-input').value.trim();
  return typed !== '' ? typed : defaultTesterName;
}

export function openConfirmModal(opts) {
  confirmContext = opts;
  renderProcedureLink(opts.result);
  renderPreview(opts.result);   // 手順モーダル側に先に描いておく（開くたびに組み直さない）
  renderTestInfo(opts.result);
  closeProcedureModal();
  // 初期値を直接入れると消してから打ち直すことになるので、薄字（placeholder）で見せて
  // 空のまま進んだときだけ採用する
  defaultTesterName = pickRandom(P.TESTER_NAME_POOL);
  el('tester-name-input').placeholder = defaultTesterName;
  el('tester-name-input').value = opts.testerName || '';
  el('build-input').value = opts.buildEntered || '';
  el('confirm-start-btn').textContent = opts.buttonLabel;
  updateConfirmStartBtnState();
  el('error-box').hidden = true;
  // 進捗URLは hashchange でも飛んでくるので、どの画面から呼ばれても開始画面の上に出す
  stopStepTimer();
  showScreen('screen-start');
  el('modal-confirm').hidden = false;
}

export function closeConfirmModal() {
  el('modal-confirm').hidden = true;
  confirmContext = null;
}

// 必須のビルドが空の間は「準備完了！」を押せない見た目にする（NGコメント必須と同じ扱い）
export function updateConfirmStartBtnState() {
  var build = (confirmContext && confirmContext.result && confirmContext.result.build)
    ? confirmContext.result.build : { mode: 'none' };
  el('confirm-start-btn').disabled = (build.mode === 'input' && el('build-input').value.trim() === '');
}

// 押せない状態でEnterを叩かれたら、黙って無視せず理由を知らせる
export function submitConfirmFromKeyboard() {
  if (!el('confirm-start-btn').disabled) el('confirm-start-btn').click();
  else triggerBuildInputWarning();
}

// 再開時に確認画面を挟むのは、開始に必要な情報が欠けているときだけ。
// テスター名は任意項目なので、ローカル保存・進捗ファイルでは空でも聞き直さない
// （本人が意図的に空にしている）。進捗URLだけは共有形式が名前を持たないため聞く。
export function resumeNeedsConfirm(saved, result, requireTesterName) {
  if (requireTesterName && (!saved.testerName || String(saved.testerName).trim() === '')) return true;
  var b = (result && result.build) ? result.build : { mode: 'none' };
  if (b.mode === 'input' && String(saved.buildEntered || '').trim() === '') return true;
  return false;
}

// 「続ける」か「開始する」かは、記録済みの結果が1件でもあるかどうかで決める
// （記録が一切無ければ試験手順だけの共有URLと同じ状態なので、開始する扱いにする）
export function resumeButtonLabel(saved) {
  var hasAnyResult = (saved.results || []).some(function (r) { return r && r.status; });
  return hasAnyResult ? '▶ ' + T.continueLabel : '✅ ' + T.readyLabel;
}

export function renderParseError(reason) {
  var box = el('error-box');
  box.innerHTML =
    '<div class="err-title">⚠️ ' + T.couldNotReadProcedure + '</div>' +
    '<div>' + escapeHtml(reason) + '</div>' +
    T.parseErrorHint;
  box.hidden = false;
}

export function handleLoadedText(text) {
  var result = parseProcedure(text, T);
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
}

export function wireConfirmScreen() {
  el('confirm-back-btn').addEventListener('click', closeConfirmModal);
  el('confirm-preview-link').addEventListener('click', openProcedureModal);
  el('procedure-close-btn').addEventListener('click', closeProcedureModal);
  el('confirm-start-btn').addEventListener('click', function () {
    if (!confirmContext) return;
    if (!ensureBuildEntered(confirmContext.result)) return;
    var onStart = confirmContext.onStart;
    closeConfirmModal();
    onStart();
  });
  el('tester-name-input').addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); submitConfirmFromKeyboard(); }
  });
  el('build-input').addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); submitConfirmFromKeyboard(); }
  });
  el('build-input').addEventListener('input', function () {
    clearBuildInputWarning();
    updateConfirmStartBtnState();
  });
  el('build-input').addEventListener('animationend', clearBuildInputWarning);
}

export function openProcedureModal() { el('modal-procedure').hidden = false; }
export function closeProcedureModal() { el('modal-procedure').hidden = true; }

// NGボタン/Nキーの入口。記録済みNGの再編集なら引き止め（オーバーレイ・逃げ演出）を挟まずコメント編集へ直行する
export function startNgFlow() {
  if (currentItemIsNg()) { openCommentPanel(); return; }
  openNgConfirmOverlay();
}

export function openNgConfirmOverlay() {
  var item = currentItem();
  el('ng-confirm-deterrent').textContent = pickRandom(P.NG_DETERRENT_POOL);
  el('ng-confirm-expected').innerHTML = item.expectedHtml;
  el('overlay-ng-confirm').hidden = false;
}

export function closeNgConfirmOverlay() {
  el('overlay-ng-confirm').hidden = true;
}

export function wireNgConfirmOverlay() {
  el('ng-confirm-back-btn').addEventListener('click', closeNgConfirmOverlay);
  el('ng-confirm-proceed-btn').addEventListener('click', function () {
    closeNgConfirmOverlay();
    openCommentPanel();
  });
}

export function updateNgConfirmBtnState() {
  var val = el('ng-comment').value.trim();
  el('ng-confirm-btn').disabled = (val === '');
}

export function triggerNgCommentRequiredWarning() {
  var ta = el('ng-comment');
  ta.classList.remove('shake-warn');
  void ta.offsetWidth;
  ta.classList.add('shake-warn');
  el('ng-inline-warn').hidden = false;
}

export function closeCommentPanels() {
  el('ng-panel').hidden = true;
  el('action-row').hidden = false;
  el('ng-inline-warn').hidden = true;
  el('ng-comment').classList.remove('shake-warn');
}

export function handleTermClick(e) {
  var target = e.target.closest ? e.target.closest('.term') : null;
  if (!target) return;
  if (target.classList.contains('term-material')) {
    var material = state.materials[parseInt(target.getAttribute('data-material-idx'), 10)];
    if (material) openMaterialPopup(material);
    return;
  }
  var entry = state.glossary[parseInt(target.getAttribute('data-term-idx'), 10)];
  if (entry) openTermPopup(entry);
}

export function openGlossaryModal() {
  el('glossary-modal-content').innerHTML = renderGlossaryHtml(state.preamble) + materialsSectionHtml(state.materials);
  el('modal-glossary').hidden = false;
}

export function closeGlossaryModal() { el('modal-glossary').hidden = true; }

export function openTermPopup(entry) {
  el('term-popup-header').textContent = '📖 ' + T.termExplanationHeading;
  el('term-popup-title').textContent = entry.term;
  var desc = el('term-popup-desc');
  desc.classList.remove('term-popup-desc-material');
  desc.innerHTML = entry.descHtml;
  el('overlay-term').hidden = false;
  spawnConfetti(24, 0.3);
}

export function openMaterialPopup(material) {
  el('term-popup-header').textContent = '📎 ' + T.providedFileHeading;
  el('term-popup-title').textContent = material.name;
  var desc = el('term-popup-desc');
  desc.classList.add('term-popup-desc-material');
  var actionsHtml = materialPopupActionsHtml(material);
  desc.innerHTML = (material.descHtml || T.noDescription) +
    (actionsHtml ? '<div class="term-popup-desc-divider"></div><div class="term-popup-actions-inline">' + actionsHtml + '</div>' : '');
  el('overlay-term').hidden = false;
  spawnConfetti(24, 0.3);
}

export function closeTermPopup() { el('overlay-term').hidden = true; }

export function wireTermPopup() {
  el('term-popup-ok-btn').addEventListener('click', closeTermPopup);
  el('overlay-term').addEventListener('click', function (e) {
    if (e.target === el('overlay-term')) closeTermPopup();
  });
}
