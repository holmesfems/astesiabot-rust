import { T } from '../constants/i18n.js';
import { STATE_HASH_PREFIX } from '../constants/config.js';
import { parseProcedure, stripMd } from './parser.js';
import { state, currentBuildLabel, sectionLabelFor, statusLabel, computeOverallStats } from './state.js';
import { formatDuration } from './score.js';

/* =========================================================================
   進捗の直列化 / 共有URL組み立て / エクスポート整形（DOM非依存）。
   LZString と window はグローバル参照のまま使う（core内でdocumentは参照しない）。
   ========================================================================= */
export function buildProgressExportObject(source) {
  return {
    rawText: source.rawText,
    testerName: source.testerName,
    // 記入モードのビルド番号だけを持ち回る。固定値は rawText から再導出できるので保存しない
    buildEntered: source.buildEntered || '',
    results: source.results,
    pointer: source.pointer,
    startedAt: source.startedAt,
    score: source.score,
    combo: source.combo
  };
}

export function hasLzString() {
  return typeof LZString !== 'undefined' && LZString && typeof LZString.compressToEncodedURIComponent === 'function';
}

export function buildShareUrl(dataObj) {
  var packed = LZString.compressToEncodedURIComponent(JSON.stringify(dataObj));
  return window.location.origin + window.location.pathname + STATE_HASH_PREFIX + packed;
}

// 共有URLは長いので、YouTrack等のMarkdown対応先に貼れるよう [タイトル](URL) 形式でコピーする。
// lz-string の出力は英数字と +-$ のみで括弧を含まないので、URL側のエスケープは不要。
export function escapeMdLinkText(s) {
  return String(s).replace(/([\\\[\]])/g, '\\$1');
}
export function toMarkdownLink(title, url) {
  return '[' + escapeMdLinkText(title) + '](' + url + ')';
}

export function docTitleOf(rawText) {
  var r = parseProcedure(rawText, T);
  return (r && r.title) ? r.title : T.untitledBare;
}

// 結果共有のリンク文言。source.results は旧localStorage形式だと空/短いことがあるため、
// その場合は同じ1回のparseProcedureからtotalItemsを補完する（2回parseしない）。
export function resultLinkTitle(source) {
  var parsed = parseProcedure(source.rawText, T);
  var title = (parsed && parsed.title) ? parsed.title : T.untitledBare;
  var total = (source.results || []).length;
  if (total === 0) total = parsed.totalItems;
  var answered = (source.results || []).filter(function (r) { return r && r.status; }).length;
  return T.resultLinkTitleText(title, answered, total, source.testerName);
}

// 試験手順だけを共有する（記録なし・テスター名なし）。開いた人は名前入力モーダルを経て最初から始める。
export function buildProcedureShareObject(rawText) {
  return { rawText: rawText, testerName: '', buildEntered: '', results: [], pointer: 0, startedAt: null, score: 0, combo: 0 };
}

// 復元し終えたらハッシュをURLから外す（リロードで同じ状態に巻き戻らないように。履歴は増やさない）。
export function clearStateHash() {
  try { history.replaceState(null, '', window.location.pathname + window.location.search); } catch (e) { /* ignore */ }
}

// 業務連絡（チャット/メール貼付用のプレーンテキスト報告文）
export function buildBusinessReport() {
  var stats = computeOverallStats();
  var title = state.docTitle || T.untitled;
  var lines = [];
  var buildLabel = currentBuildLabel();
  var titleWithBuild = T.reportTitleWithBuild(title, buildLabel);
  lines.push(T.reportGreeting(titleWithBuild));
  lines.push(T.reportCounts(stats.total, stats.ok, stats.ng));
  if (stats.ng > 0) {
    lines.push(T.reportNgListHeading);
    state.flatItems.forEach(function (item, idx) {
      var r = state.results[idx];
      if (!r || r.status !== 'ng') return;
      var num = item.number || String(idx + 1);
      var comment = (r.comment && String(r.comment).trim()) ? String(r.comment).trim() : T.reportNoCommentEntered;
      lines.push(T.reportNgLine(num, comment));
    });
    lines.push('');
    lines.push(T.reportClosing);
  } else {
    lines.push(T.reportNoNgItems);
  }
  return lines.join('\n');
}

export function buildExportRows() {
  return state.flatItems.map(function (item, idx) {
    var r = state.results[idx] || { status: null, comment: '', timestamp: null, durationMs: null };
    var timeStr = r.timestamp ? new Date(r.timestamp).toLocaleString(T.dateLocale) : '';
    var durationStr = (r.durationMs != null) ? formatDuration(r.durationMs) : '';
    return {
      number: item.number || String(idx + 1),
      section: sectionLabelFor(item.sectionIndex),
      step: stripMd(item.stepRaw).replace(/\n/g, ' / '),
      expected: stripMd(item.expectedRaw).replace(/\n/g, ' / '),
      status: statusLabel(r.status),
      comment: (r.comment || '').replace(/\n/g, ' / '),
      time: timeStr,
      duration: durationStr
    };
  });
}

/* 表は列を増やさず、表の上にビルドを1行添える（ビルド指定が無ければ従来どおり） */
export function toMarkdownTable(rows) {
  var esc = function (s) { return String(s).replace(/\|/g, '\\|'); };
  var buildLabel = currentBuildLabel();
  var prefix = T.buildPrefixLine(buildLabel);
  var header = T.markdownTableHeader;
  var sep = '|---|---|---|---|---|---|---|---|';
  var body = rows.map(function (r) {
    return '| ' + [r.number, r.section, r.step, r.expected, r.status, r.comment, r.time, r.duration].map(esc).join(' | ') + ' |';
  }).join('\n');
  return prefix + header + '\n' + sep + '\n' + body;
}

/* CSV/TSV は「ビルド」列を先頭に足す（全行同値）。ビルド指定が無ければ列ごと省く */
export function exportHeaderCells(buildLabel) {
  // 共有配列をそのまま返すと呼び出し側の改変が T に伝播するので必ずコピーする
  var cells = T.exportHeaderCellsBase.slice();
  return buildLabel ? [T.buildLabel].concat(cells) : cells;
}

export function exportRowCells(r, buildLabel) {
  var cells = [r.number, r.section, r.step, r.expected, r.status, r.comment, r.time, r.duration];
  return buildLabel ? [buildLabel].concat(cells) : cells;
}

/* 表計算ソフトの数式インジェクション対策（CSV/TSV共通）。
 * 手順書・コメント・ビルドは共有URLや進捗ファイル経由で他人が書けるので、セルの
 * 先頭が数式記号だと開いた人の表計算ソフトで実行され得る（=HYPERLINK で他セルを送信等）。
 * 「セルの先頭」はファイル上の先頭だけでなく、読み込み側が区切りに使い得る文字の直後も
 * 含む（地域設定の`;`、読み込み画面で選べるタブ、引用符を見ない取り込みでの改行など）。
 * そうした位置で、空白や`"`の後に数式記号が来る場合は直前に`'`を入れて文字列扱いにする。
 * 全角の＝＋－＠は日本語版Excelが半角へ寄せて数式にする可能性があるので含める。 */
var FORMULA_LEAD = '=+\\-@＝＋－＠';
function neutralizeFormula(v, separators) {
  // 読み飛ばすのは区切り以外の空白と`"`だけ（改行・タブまで飛ばすと、その先の
  // 新しいセル/行の先頭を見落とす）
  var re = new RegExp('(^|[' + separators + '])(?=(?:[^\\S\\r\\n\\t]|")*[' + FORMULA_LEAD + '])', 'g');
  return v.replace(re, "$1'");
}

export function toTsv(rows) {
  // 貼り付け先は行を改行、セルをタブで区切るので、セル内のそれは潰す。
  // Excel が前回の「区切り位置」で覚えた`,`/`;`で割られる場合にも備える
  var esc = function (s) {
    var v = String(s).replace(/\r\n?|\n/g, ' / ').replace(/\t/g, ' ');
    return neutralizeFormula(v, ',;');
  };
  var buildLabel = currentBuildLabel();
  var header = exportHeaderCells(buildLabel).join('\t');
  var body = rows.map(function (r) {
    return exportRowCells(r, buildLabel).map(esc).join('\t');
  }).join('\n');
  return header + '\n' + body;
}

export function toCsv(rows) {
  var esc = function (s) {
    var v = neutralizeFormula(String(s), ',;\\t\\r\\n');
    if (/[",;\t\r\n]/.test(v)) v = '"' + v.replace(/"/g, '""') + '"';
    return v;
  };
  var buildLabel = currentBuildLabel();
  var header = exportHeaderCells(buildLabel).map(esc).join(',');
  var body = rows.map(function (r) {
    return exportRowCells(r, buildLabel).map(esc).join(',');
  }).join('\n');
  return header + '\n' + body;
}
