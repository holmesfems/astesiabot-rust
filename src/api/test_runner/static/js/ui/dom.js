import { T } from '../constants/i18n.js';
import { SCREENS } from '../constants/config.js';
import { parseProcedure } from '../core/parser.js';
import { state, normalizeStatusCompat } from '../core/state.js';
import { buildProgressExportObject, hasLzString, buildShareUrl, resultLinkTitle, toMarkdownLink, buildProcedureShareObject } from '../core/io.js';

/* =========================================================================
   汎用DOMヘルパー / 画面遷移 / 進捗ファイル(Blob・FileReader) / クリップボード・ダウンロード
   ========================================================================= */
export function el(id) { return document.getElementById(id); }

// OSの「視差効果を減らす」設定。紙吹雪も逃げるNGボタンもこれを見るので、
// どちらか片方の演出モジュールではなく共通のここに置く（constants/ は core/ からも
// import されるため、window を触るこの行を置くと core/ が node で読めなくなる）。
export var reducedMotionOS = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

export function showScreen(id) {
  SCREENS.forEach(function (s) {
    document.getElementById(s).hidden = (s !== id);
  });
  // 画面を入れ替えたらスクロール位置も先頭へ戻す。開始画面で下までスクロールした
  // まま実行画面へ移ると、ステップカードの上部が見切れたところから始まってしまう。
  // 同じことが 実行→結果 / フィナーレ→結果 / 進捗URLで開いたときの確認モーダル でも
  // 起きるので、遷移を一手に引き受けているここで直す。
  // 項目送り（OK/NG・戻る）は renderStep だけで showScreen を通らないため、
  // 同じ画面の中でのスクロール位置は維持される。
  window.scrollTo(0, 0);
}

// 進捗ファイル / 進捗URL 共通: 進捗オブジェクトの形式検証と旧形式の正規化。
// 不正なら toast を出して null を返す。
export function normalizeProgressObject(obj) {
  if (!obj || typeof obj.rawText !== 'string' || obj.rawText.trim() === '' || !Array.isArray(obj.results)) {
    showToast(T.invalidProgressFormat);
    return null;
  }
  var check = parseProcedure(obj.rawText);
  if (!check.ok) {
    showToast(T.progressProcedureParseFailed);
    return null;
  }
  // 互換: 旧バージョンで書き出された「保留」ステータスは未実施(null)として扱う
  obj.results = obj.results.map(function (r) {
    return r ? { status: normalizeStatusCompat(r.status), comment: r.comment, timestamp: r.timestamp, durationMs: (r.durationMs != null ? r.durationMs : null) } : r;
  });
  return obj;
}

export function downloadProgressJson(dataObj) {
  var json = JSON.stringify(dataObj, null, 2);
  var blob = new Blob([json], { type: 'application/json' });
  var url = URL.createObjectURL(blob);
  var a = document.createElement('a');
  var ts = new Date();
  var pad = function (n) { return (n < 10 ? '0' : '') + n; };
  var fname = 'progress_' + ts.getFullYear() + pad(ts.getMonth() + 1) + pad(ts.getDate()) +
    '_' + pad(ts.getHours()) + pad(ts.getMinutes()) + pad(ts.getSeconds()) + '.json';
  a.href = url;
  a.download = fname;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  setTimeout(function () { URL.revokeObjectURL(url); }, 1000);
  return fname;
}

export function exportProgressFromState() {
  if (!state.rawText) { showToast(T.noProgressToExport); return; }
  var fname = downloadProgressJson(buildProgressExportObject(state));
  showToast(T.progressExported(fname));
}

// rawOnly=true（Shift+クリック）なら Markdown リンクではなく URL だけをコピーする（ブラウザに直接貼る用）
export function copyShareUrl(source, rawOnly) {
  if (!source || !source.rawText) { showToast(T.noProgressToShare); return; }
  if (!hasLzString()) { showToast(T.lzStringUnavailable); return; }
  var url = buildShareUrl(buildProgressExportObject(source));
  if (rawOnly) { copyToClipboard(url, T.resultUrlCopiedRawOnly); return; }
  copyToClipboard(toMarkdownLink(resultLinkTitle(source), url), T.resultUrlCopiedMarkdown);
}

// rawOnly=true（Shift+クリック）なら Markdown リンクではなく URL だけをコピーする
export function copyProcedureShareUrl(rawText, rawOnly) {
  if (!rawText || String(rawText).trim() === '') { showToast(T.noProcedureToShare); return; }
  var check = parseProcedure(rawText);
  if (!check.ok || check.totalItems === 0) { showToast(T.procedureParseFailed); return; }
  if (!hasLzString()) { showToast(T.lzStringUnavailable); return; }
  // check は上ですでにparseProcedure済みなので、そこからタイトルを組む（二重parseしない）
  var title = (check.title ? check.title : T.untitledBare) + T.procedureTitleSuffix;
  var url = buildShareUrl(buildProcedureShareObject(rawText));
  if (rawOnly) { copyToClipboard(url, T.procedureUrlCopiedRawOnly); return; }
  copyToClipboard(toMarkdownLink(title, url), T.procedureUrlCopiedMarkdown);
}

export function copyToClipboard(text, successMsg) {
  var msg = successMsg || T.defaultCopiedMsg;
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(function () {
      flashToast(msg);
    }).catch(function () { fallbackCopy(text, msg); });
  } else {
    fallbackCopy(text, msg);
  }
}

export function fallbackCopy(text, successMsg) {
  var ta = el('clipboard-fallback');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.left = '0';
  ta.style.top = '0';
  ta.focus();
  ta.select();
  try {
    document.execCommand('copy');
    flashToast(successMsg || T.copiedFallbackMsg);
  } catch (e) {
    flashToast(T.copyFailedMsg);
  }
  ta.style.position = 'fixed';
  ta.style.left = '-9999px';
  ta.style.top = '-9999px';
}

// 一瞬だけ浮かんで消える表示。スコア加算にもトースト代わりにも使う同じ部品なので、
// 紙吹雪（effects/confetti.js）ではなくこちらに置く。
export function showScoreFloat(text) {
  var node = el('score-float');
  node.textContent = text;
  node.classList.remove('show');
  void node.offsetWidth;
  node.classList.add('show');
}

export function flashToast(msg) {
  showScoreFloat(msg);
}

export function downloadCsvBom(text, filename) {
  var bom = '﻿';
  var blob = new Blob([bom + text], { type: 'text/csv;charset=utf-8;' });
  var url = URL.createObjectURL(blob);
  var a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  setTimeout(function () { URL.revokeObjectURL(url); }, 1000);
}

var toastTimer = null;
export function showToast(msg) {
  var node = el('toast');
  node.textContent = msg;
  node.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(function () { node.classList.remove('show'); }, 2600);
}
