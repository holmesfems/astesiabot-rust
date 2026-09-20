#!/usr/bin/env node
// 試験手順書（Markdown）が Test Runner のパーサーで正しく読めるかを検証する CLI。
//
//   node validate.mjs <手順書.md> [--json] [--lang ja|en]
//
// このディレクトリを丸ごと別端末にコピーしても動く（parser.js を相対 import する
// だけで、リポジトリ本体には一切依存しない）。
//
// 終了コード: ok === false、または error 級の警告が1件でもあれば 1。それ以外は 0。

import fs from 'node:fs';
import path from 'node:path';
import { parseProcedure } from './parser.js';

const LABELS_JA = { notSpecified: '（記載なし）', simpleFormatSectionTitle: '手順' };
const LABELS_EN = { notSpecified: '(not specified)', simpleFormatSectionTitle: 'Steps' };

function parseArgs(argv) {
  const out = { file: null, json: false, lang: 'ja' };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--json') out.json = true;
    else if (a === '--lang') { out.lang = argv[++i]; }
    else if (a.startsWith('--lang=')) out.lang = a.slice('--lang='.length);
    else if (!out.file) out.file = a;
    else {
      console.error('unknown argument: ' + a);
      process.exit(2);
    }
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));

if (!args.file) {
  console.error('使い方: node validate.mjs <手順書.md> [--json] [--lang ja|en]');
  process.exit(2);
}
if (args.lang !== 'ja' && args.lang !== 'en') {
  console.error('--lang は ja か en のみ対応しています: ' + args.lang);
  process.exit(2);
}

let raw;
try {
  raw = fs.readFileSync(path.resolve(process.cwd(), args.file), 'utf-8');
} catch (e) {
  console.error('ファイルを読めませんでした: ' + args.file);
  console.error(String(e && e.message ? e.message : e));
  process.exit(2);
}

const labels = args.lang === 'en' ? LABELS_EN : LABELS_JA;
const result = parseProcedure(raw, labels);

// kind -> { severity, message(w) } の対応表。parser.js 自身は文言を持たない
// （i18n依存ゼロを保つため）ので、ここ（CLI側）だけが唯一の文言の置き場所。
const MESSAGES = {
  'no-items': {
    severity: 'error',
    message: () => '試験項目が1件も読み取れませんでした。表の書式（| 列 | 列 | 列 |）を見直してください。'
  },
  'section-without-table': {
    severity: 'warn',
    message: () => 'この節には表が無いため、試験項目ではなく前置きとして扱われます'
  },
  'glossary-no-separator': {
    severity: 'warn',
    message: () => '用語定義の区切り（… や :）が見つからないため、この行は無視されます'
  },
  'glossary-empty-term-or-desc': {
    severity: 'warn',
    message: () => '用語名または説明が空のため、この用語定義は無視されます'
  },
  'glossary-key-too-short': {
    severity: 'warn',
    message: () => '用語名が1文字のため無視されます（2文字以上必要）'
  },
  'glossary-duplicate-key': {
    severity: 'info',
    message: () => '同じ用語名が既に定義されています。先に出てきた方が使われ、この行は無視されます'
  },
  'materials-no-separator': {
    severity: 'warn',
    message: () => '配布物の区切り（… や :）が見つからないため、この行は無視されます'
  },
  'materials-empty-name-or-desc': {
    severity: 'warn',
    message: () => 'ファイル名が空のため、この配布物は無視されます'
  },
  'materials-duplicate-key': {
    severity: 'info',
    message: () => '同じファイル名が既に定義されています。先に出てきた方が使われ、この行は無視されます'
  },
  'material-no-url': {
    severity: 'info',
    message: () => 'この配布物には http(s) のURLが見つかりませんでした（URLなしのまま登録されます）'
  },
  'list-interrupted': {
    severity: 'warn',
    message: () => 'この行で箇条書きが途切れています。直後にまた箇条書きがあるように見えるので、' +
      '以降の項目が読み取られていない可能性があります（空行や地の文が挟まっていないか確認してください）'
  },
  'keyword-in-table-section': {
    severity: 'warn',
    message: () => 'この「用語定義:」/「配布物:」は表のある節の中にあるため読み取り対象外です' +
      '（用語定義・配布物は表の無い前置きの節に書いてください）'
  },
  'table-column-count-mismatch': {
    severity: 'warn',
    message: () => 'この行のセル数が見出し行と一致していません（セルの中に | が紛れ込んでいないか確認してください）'
  },
  'duplicate-item-number': {
    severity: 'info',
    message: () => '同じ項目番号が複数回使われています（連番の振り直しミスの可能性があります）'
  }
};

const SEVERITY_ICON = { error: '❌', warn: '⚠️ ', info: 'ℹ️ ' };
const SEVERITY_RANK = { error: 0, warn: 1, info: 2 };

function severityOf(kind) {
  const m = MESSAGES[kind];
  return m ? m.severity : 'warn';
}
function messageOf(kind) {
  const m = MESSAGES[kind];
  return m ? m.message() : ('未知の警告種別: ' + kind);
}

// 終了コードは error だけでなく warn でも 1 にする。
// このスキルの運用は「警告がゼロになるまで直してから納品する」なので、
// 書いた項目が黙って落ちている状態（warn級はすべてそれ）で 0 を返すと、
// 呼び出し側が素通りしてしまう。info（URL無しの配布物・番号重複・用語の重複定義）は
// 実害が無いので 0 のまま。
const hasErrorLevel = result.warnings.some((w) => severityOf(w.kind) === 'error');
const hasWarnLevel = result.warnings.some((w) => severityOf(w.kind) === 'warn');
const okOverall = result.ok && !hasErrorLevel && !hasWarnLevel;

if (args.json) {
  const summary = {
    title: result.title,
    totalItems: result.totalItems,
    sectionCount: result.sections.length,
    sectionTags: result.sections.map((s) => s.tag).filter((t) => t),
    glossaryCount: result.glossary.length,
    materialsCount: result.materials.length,
    materialsWithUrlCount: result.materials.filter((m) => m.url).length,
    buildMode: result.build.mode
  };
  const warningsOut = result.warnings.map((w) => ({
    kind: w.kind,
    severity: severityOf(w.kind),
    line: w.line,
    text: w.text,
    message: messageOf(w.kind)
  }));
  console.log(JSON.stringify({ ok: okOverall, summary, warnings: warningsOut }, null, 2));
  process.exit(okOverall ? 0 : 1);
}

// ---- 人間向け出力 ----
const tagCounts = {};
result.sections.forEach((s) => {
  if (!s.tag) return;
  tagCounts[s.tag] = (tagCounts[s.tag] || 0) + 1;
});
const tagSummary = Object.keys(tagCounts).length
  ? ' ' + Object.entries(tagCounts).map(([t, n]) => `[${t}]×${n}`).join(' ')
  : '';

console.log((result.title ? '✅' : '⚠️ ') + ` タイトル: ${result.title ? '「' + result.title + '」' : '（見つかりませんでした）'}`);
console.log((result.totalItems > 0 ? '✅' : '❌') +
  ` 試験項目: ${result.totalItems}件 / ${result.sections.length}節${tagSummary}`);
console.log('✅' + ` ビルド: ${
  result.build.mode === 'fixed' ? '固定 ' + result.build.value :
  result.build.mode === 'input' ? '記入させる（' + result.build.hint + '）' :
  '指定なし'
}`);
console.log('✅' + ` 用語定義: ${result.glossary.length}件` +
  (result.glossary.length ? '（' + result.glossary.map((g) => g.term).join(' / ') + '）' : ''));
const withUrl = result.materials.filter((m) => m.url).length;
console.log('✅' + ` 配布物: ${result.materials.length}件` +
  (result.materials.length ? `（うちURL付き ${withUrl}件）` : ''));

if (result.warnings.length > 0) {
  console.log('');
  const sorted = result.warnings.slice().sort((a, b) => SEVERITY_RANK[severityOf(a.kind)] - SEVERITY_RANK[severityOf(b.kind)]);
  for (const w of sorted) {
    const icon = SEVERITY_ICON[severityOf(w.kind)] || '⚠️ ';
    const loc = w.line != null ? `${w.line}行目 ` : '';
    console.log(`${icon} ${loc}${messageOf(w.kind)}`);
    if (w.text) console.log(`    ${w.text}`);
  }
} else {
  console.log('');
  console.log('✅ 警告はありません');
}

process.exit(okOverall ? 0 : 1);
