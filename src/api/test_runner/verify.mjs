// 試験手順ランナーの計算層（static/js/core/ と constants/）の検証スクリプト。
//
// core/ は DOM 非依存なので、ブラウザを立てずに実モジュールを import して動かせる。
// ui/ は document を触るので対象外（ブラウザでの手動確認が必要）。
//
// 実行方法（このマシンには node が無いため、VS Code の Electron を node として使う）:
//   $env:ELECTRON_RUN_AS_NODE="1"
//   & "$env:LOCALAPPDATA\Programs\Microsoft VS Code\Code.exe" src/api/test_runner/verify.mjs
//
// 全部 PASS なら最後に ALL PASS と出る。1件でも落ちれば終了コードが 1 になる。

import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const B = new URL('./static/js/', import.meta.url).href;

let fail = 0;
function ok(name, cond, extra) {
  console.log((cond ? 'PASS  ' : 'FAIL  ') + name + (extra !== undefined ? '  -> ' + extra : ''));
  if (!cond) fail++;
}

const i18n = await import(B + 'constants/i18n.js');
const SJA = (await import(B + 'constants/strings.ja.js')).STRINGS;
const SEN = (await import(B + 'constants/strings.en.js')).STRINGS;
const PJA = (await import(B + 'constants/phrases.ja.js')).PHRASES;
const PEN = (await import(B + 'constants/phrases.en.js')).PHRASES;
const MJA = (await import(B + 'constants/samples.ja.js')).SAMPLES;
const MEN = (await import(B + 'constants/samples.en.js')).SAMPLES;
i18n.installI18n({ strings: SJA, phrases: PJA, samples: MJA });

const parser = await import(B + 'core/parser.js');
const score = await import(B + 'core/score.js');
const state = await import(B + 'core/state.js');
const io = await import(B + 'core/io.js');

// core/ が DOM を掴んでいたらここに到達できない。層の切り分けそのものの検証でもある。
ok('core/ imports with no DOM present', true);

/* ========================= parser ========================= */

const r = parser.parseProcedure(MJA.SAMPLE_A);
ok('sample(ja): ok', r.ok === true);
ok('sample(ja): totalItems', r.totalItems > 0, r.totalItems);
ok('sample(ja): sections', r.sections.length > 0, r.sections.length);
ok('sample(ja): title', !!r.title, JSON.stringify(r.title));
ok('sample(ja): glossary', r.glossary.length > 0, r.glossary.map(g => g.key).join(' / '));
ok('sample(ja): materials', r.materials.length > 0, r.materials.map(m => m.key).join(' / '));
ok('sample(ja): build mode', !!r.build.mode, r.build.mode);
ok('sample(ja): section tag', r.sections.some(s => s.tag), r.sections.map(s => s.tag).join(','));

const rEn = parser.parseProcedure(MEN.SAMPLE_A);
ok('sample(en): same section count', rEn.sections.length === r.sections.length,
   rEn.sections.length + ' vs ' + r.sections.length);
ok('sample(en): same totalItems', rEn.totalItems === r.totalItems,
   rEn.totalItems + ' vs ' + r.totalItems);
ok('sample(en): glossary found', rEn.glossary.length > 0, rEn.glossary.map(g => g.key).join(' / '));
ok('sample(en): materials found', rEn.materials.length > 0, rEn.materials.map(m => m.key).join(' / '));

// --- 見出しキーワードの日英エイリアス ---
// 日本語版は従来どおり動くこと（回帰）、英語版も同じ結果になること。
function minimal(glossaryWord, materialsWord, buildLine) {
  return [
    '## T',
    '### Pre',
    '',
    buildLine,
    '',
    materialsWord + ': ',
    '',
    '* **sample.bin** … https://example.com/a.bin the file',
    '',
    glossaryWord + ': ',
    '',
    '* **Start button** … the green round button',
    '',
    '### 1. S',
    '',
    '| No | Step | Expected |',
    '|---|---|---|',
    '| 1 | do it | it works |',
  ].join('\n');
}
for (const [label, g, m, b] of [
  ['ja   用語定義 / 配布物 / ビルド', '用語定義', '配布物', 'ビルド: 1.2.3'],
  ['en   Glossary / Attachments / Build', 'Glossary', 'Attachments', 'Build: 1.2.3'],
  ['en   Definitions / Downloads / Version', 'Definitions', 'Downloads', 'Version: 1.2.3'],
  ['en   Definition / Asset / build (lower)', 'Definition', 'Asset', 'build: 1.2.3'],
]) {
  const p = parser.parseProcedure(minimal(g, m, b));
  ok('keywords ' + label,
     p.glossary.length === 1 && p.materials.length === 1 &&
     p.build.mode === 'fixed' && p.build.value === '1.2.3',
     'glossary=' + p.glossary.length + ' materials=' + p.materials.length +
     ' build=' + JSON.stringify(p.build));
}

// ビルド値の「記入モード」（テスターに入力させる）の日英
for (const [label, line, hint] of [
  ['ja  記入（例: 1.2.3）', 'ビルド: 記入（例: 1.2.3）', '例: 1.2.3'],
  ['ja  入力(e.g. 1.2.3)', 'ビルド: 入力(e.g. 1.2.3)', 'e.g. 1.2.3'],
  ['en  Enter (e.g. 1.2.3)', 'Build: Enter (e.g. 1.2.3)', 'e.g. 1.2.3'],
  ['en  Fill in (e.g. 1.2.3)', 'Build: Fill in (e.g. 1.2.3)', 'e.g. 1.2.3'],
  ['en  TBD (e.g. 1.2.3)', 'Build: TBD (e.g. 1.2.3)', 'e.g. 1.2.3'],
]) {
  const p = parser.parseProcedure(minimal('Glossary', 'Attachments', line));
  ok('build input mode ' + label,
     p.build.mode === 'input' && p.build.hint === hint, JSON.stringify(p.build));
}

// 英語エイリアスを足しても、無関係な行を誤検出しないこと
const noKw = parser.parseProcedure(minimal('Glossary', 'Attachments', 'Built by: someone'));
ok('no false positive on "Built by:"', noKw.build.mode === 'none', JSON.stringify(noKw.build));

// --- 節タグ: 日本語の【】と英語手順書向けの[] の両方 ---
function heading(h) {
  const p = parser.parseProcedure(['## T', '### ' + h, '', '| No | Step | Exp |', '|---|---|---|', '| 1 | a | b |'].join('\n'));
  return p.sections[0];
}
ok('tag 【Windows】', heading('1. リセット【Windows】').tag === 'Windows', JSON.stringify(heading('1. リセット【Windows】')));
ok('tag [Windows]', heading('1. Reset [Windows]').tag === 'Windows', JSON.stringify(heading('1. Reset [Windows]')));
ok('tag [Windows] strips from title', heading('1. Reset [Windows]').title === 'Reset',
   JSON.stringify(heading('1. Reset [Windows]').title));
ok('tag【】strips from title', heading('1. リセット【Windows】').title === 'リセット',
   JSON.stringify(heading('1. リセット【Windows】').title));
ok('no tag when absent', heading('1. Reset').tag === null);
// 誤爆しないこと: Markdownリンクは ) で終わるので末尾アンカーに掛からない
ok('markdown link in heading is not a tag', heading('1. See [docs](https://example.com)').tag === null,
   JSON.stringify(heading('1. See [docs](https://example.com)').tag));
ok('bracket not at end is not a tag', heading('1. [Draft] Login').tag === null,
   JSON.stringify(heading('1. [Draft] Login').tag));
ok('trailing [WIP] is taken as a tag', heading('1. Login [WIP]').tag === 'WIP');

// --- 同梱サンプルが実際にパースできること（英語版は節タグも英語括弧） ---
ok('sample(en): section tags parsed', rEn.sections.every(s => s.tag),
   rEn.sections.map(s => s.tag).join(','));
ok('sample(en) SAMPLE_B: build input mode',
   parser.parseProcedure(MEN.SAMPLE_B).build.mode === 'input',
   JSON.stringify(parser.parseProcedure(MEN.SAMPLE_B).build));
ok('sample(ja) SAMPLE_B: build input mode',
   parser.parseProcedure(MJA.SAMPLE_B).build.mode === 'input',
   JSON.stringify(parser.parseProcedure(MJA.SAMPLE_B).build));
// AI整形プロンプトが提示する書式そのものが、このパーサーで通ること。
// （プロンプトのコードブロック内の例をそのまま食わせる）
for (const [lang, prompt] of [['ja', MJA.AI_FORMAT_PROMPT], ['en', MEN.AI_FORMAT_PROMPT]]) {
  const m = prompt.match(/```markdown\n([\s\S]*?)```/);
  ok('AI prompt(' + lang + '): example block present', !!m);
  if (!m) continue;
  const p = parser.parseProcedure(m[1]);
  ok('AI prompt(' + lang + '): its own example parses',
     p.ok && p.totalItems === 2 && p.glossary.length === 1 &&
     p.build.mode === 'fixed' && p.sections[0].tag !== null,
     'items=' + p.totalItems + ' glossary=' + p.glossary.length +
     ' build=' + JSON.stringify(p.build) + ' tag=' + JSON.stringify(p.sections[0].tag));
}

// --- 表の見出し行・前置きの見出しはパース対象外（言語に依存しない） ---
const headerAgnostic = parser.parseProcedure([
  '## T', '### 1. S', '',
  '| Number | Action | Expected Result |',
  '|---|---|---|',
  '| 1 | a | b |',
  '| 2 | c | d |',
].join('\n'));
ok('table header row is skipped regardless of language', headerAgnostic.totalItems === 2,
   headerAgnostic.totalItems);

// --- フォールバックと欠損 ---
const fb = parser.parseProcedure('起動する\t画面が出る\n終了する\tウィンドウが閉じる');
ok('fallback: tab separated', fb.totalItems === 2, fb.totalItems);
const fbArrow = parser.parseProcedure('起動する→画面が出る');
ok('fallback: arrow separated', fbArrow.totalItems === 1, fbArrow.totalItems);
const twoCol = parser.parseProcedure(['## T', '### 1. S', '| No | Step |', '|---|---|', '| 1 | a |'].join('\n'));
ok('2-column table falls back to notSpecified', twoCol.sections[0].items[0].expectedRaw === SJA.notSpecified,
   JSON.stringify(twoCol.sections[0].items[0].expectedRaw));

// --- decodeAuto ---
const u8 = new TextEncoder().encode('あい');
ok('decodeAuto: utf-8 BOM', parser.decodeAuto(new Uint8Array([0xef, 0xbb, 0xbf, ...u8])) === 'あい');
ok('decodeAuto: utf-16le BOM', parser.decodeAuto(new Uint8Array([0xff, 0xfe, 0x42, 0x30])) === 'あ');
ok('decodeAuto: utf-16be BOM', parser.decodeAuto(new Uint8Array([0xfe, 0xff, 0x30, 0x42])) === 'あ');
ok('decodeAuto: bare utf-8', parser.decodeAuto(u8) === 'あい');

ok('escapeHtml', parser.escapeHtml('<a>&"') === '&lt;a&gt;&amp;&quot;', parser.escapeHtml('<a>&"'));

/* ========================= score ========================= */

ok('formatDuration 0', score.formatDuration(0) === '0:00', score.formatDuration(0));
ok('formatDuration 59s', score.formatDuration(59000) === '0:59', score.formatDuration(59000));
ok('formatDuration 1h1m1s', score.formatDuration(3661000) === '1:01:01', score.formatDuration(3661000));
ok('getPraiseForCombo(1)', typeof score.getPraiseForCombo(1) === 'string');
ok('getPraiseForCombo(999) stays in range', typeof score.getPraiseForCombo(999) === 'string',
   score.getPraiseForCombo(999));
ok('classifyPace returns a known bucket',
   ['fast', 'steady', 'careful'].includes(score.classifyPace(3)) &&
   ['fast', 'steady', 'careful'].includes(score.classifyPace(600)),
   score.classifyPace(3) + ' / ' + score.classifyPace(600));
ok('timeBonusFor is numeric', typeof score.timeBonusFor(score.classifyPace(3)) === 'number');
ok('timePraisePoolFor returns an array', Array.isArray(score.timePraisePoolFor(score.classifyPace(3))));
ok('pickDistinct n > len', score.pickDistinct([1, 2], 5).length === 2);
ok('pickDistinct no duplicates', new Set(score.pickDistinct([1, 2, 3, 4, 5], 4)).size === 4);

/* ========================= state ========================= */

state.state.sections = r.sections;
state.buildFlatItems();   // 戻り値ではなく state.flatItems を直接書く
ok('buildFlatItems length == totalItems', state.state.flatItems.length === r.totalItems,
   state.state.flatItems.length);

let acc = 0;
let rangesOk = true;
for (let i = 0; i < r.sections.length; i++) {
  const rg = state.sectionRange(i);
  if (rg.start !== acc || rg.end !== acc + r.sections[i].items.length) rangesOk = false;
  acc = rg.end;
}
ok('sectionRange tiles the whole list', rangesOk && acc === r.totalItems, acc);

ok('computeRank all-ok', state.computeRank({ total: 10, ok: 10, ng: 0, unanswered: 0 }) === SJA.rankFlawless);
ok('computeRank 90%', state.computeRank({ total: 10, ok: 9, ng: 1, unanswered: 0 }) === SJA.rankS);
ok('computeRank 75%', state.computeRank({ total: 100, ok: 75, ng: 25, unanswered: 0 }) === SJA.rankA);
ok('computeRank 50%', state.computeRank({ total: 100, ok: 50, ng: 50, unanswered: 0 }) === SJA.rankB);
ok('computeRank below 50%', state.computeRank({ total: 100, ok: 10, ng: 90, unanswered: 0 }) === SJA.rankC);
ok('computeRank empty', state.computeRank({ total: 0, ok: 0, ng: 0, unanswered: 0 }) === SJA.rankC);

state.state.results = state.state.flatItems.map(() => ({ status: null, comment: '', timestamp: null, durationMs: null }));
state.state.pointer = 0;
ok('currentItemIsNg false when unanswered', state.currentItemIsNg() === false);
state.state.results[0] = { status: 'ng', comment: 'カンマ, 改行\n入り "引用符"', timestamp: Date.now(), durationMs: 1000 };
ok('currentItemIsNg true after ng', state.currentItemIsNg() === true);
// 旧バージョンの「保留」は保存値としては 'hold'（表示ラベルではなく内部ID）。
ok('normalizeStatusCompat drops legacy hold', state.normalizeStatusCompat('hold') === null,
   JSON.stringify(state.normalizeStatusCompat('hold')));
ok('normalizeStatusCompat keeps ok/ng/null',
   state.normalizeStatusCompat('ok') === 'ok' &&
   state.normalizeStatusCompat('ng') === 'ng' &&
   state.normalizeStatusCompat(null) === null);

/* ========================= io ========================= */

state.state.rawText = MJA.SAMPLE_A;
const exported = io.buildProgressExportObject(state.state);
ok('progress export survives JSON round-trip',
   JSON.parse(JSON.stringify(exported)).rawText === state.state.rawText);

const rows = io.buildExportRows();
ok('buildExportRows length', rows.length === state.state.flatItems.length, rows.length);
const csv = io.toCsv(rows);
ok('toCsv doubles embedded quotes', csv.includes('""'));
ok('toCsv quotes fields containing a newline', /"[^"]*\n/.test(csv));
ok('toTsv header is tab separated', io.toTsv(rows).split('\n')[0].includes('\t'));
ok('toMarkdownTable starts with a pipe', io.toMarkdownTable(rows).startsWith('|'));
ok('buildBusinessReport lists the NG item', io.buildBusinessReport().includes('NG'));
ok('exportHeaderCells returns a fresh array each call',
   io.exportHeaderCells('') !== io.exportHeaderCells(''));

/* ================= constants の ja/en 対応 ================= */

const kja = Object.keys(SJA).sort(), ken = Object.keys(SEN).sort();
ok('STRINGS key sets match', JSON.stringify(kja) === JSON.stringify(ken),
   'ja=' + kja.length + ' en=' + ken.length);
ok('STRINGS value kinds match per key',
   kja.every(k => typeof SJA[k] === typeof SEN[k]),
   kja.filter(k => typeof SJA[k] !== typeof SEN[k]).join(','));

// PRAISE_POOL は combo を添字に使うので、長さが違うと挙動が変わる。
const lenBad = Object.keys(PJA).filter(k =>
  Array.isArray(PJA[k]) && (!Array.isArray(PEN[k]) || PJA[k].length !== PEN[k].length));
ok('PHRASES array lengths match', lenBad.length === 0, lenBad.join(','));
ok('PHRASES key sets match',
   JSON.stringify(Object.keys(PJA).sort()) === JSON.stringify(Object.keys(PEN).sort()));
ok('SAMPLES key sets match',
   JSON.stringify(Object.keys(MJA).sort()) === JSON.stringify(Object.keys(MEN).sort()));

/* ========================= parser: warnings（黙って捨てたものの報告） ========================= */
// core/parser.js の警告機能（パーサー警告ワークストリーム設計書 2.2/2.3）の検証。
// パースの挙動そのものは1ミリも変えていない前提なので、ここでは主に
// (1) 既存の戻り値が変わっていないこと (2) warnings が正しく出ること (3) 正常な手順書では
// 余計な警告が出ないこと (4) ラベル注入 (5) スキル同梱コピーの同一性 を見る。

// --- 1. 既存の出力が変わっていないこと（最重要）---
// 改修前（i18n の T をアプリ同様に導入した状態）の値をそのままハードコードして突き合わせる。
// 実装前に改修前の parseProcedure を実際に叩いて採取した値であり、この検証のために
// 都合よく書いた期待値ではない。
const UNCHANGED_CASES = [
  {
    name: 'SAMPLE_A ja', md: MJA.SAMPLE_A, labels: undefined,
    title: '「ぽもどーろ君」タイマー動作確認', totalItems: 15, sectionCount: 5,
    tags: ['Windows', 'Windows', 'Windows', 'Windows', 'Android'],
    glossaryKeys: ['開始ボタン', 'リセット', '分に設定'],
    materialsKeys: ['長時間設定データ.pomo', '通知音テスト.wav'],
    build: { mode: 'fixed', value: '1.2.3-beta4' }
  },
  {
    name: 'SAMPLE_B ja', md: MJA.SAMPLE_B, labels: undefined,
    title: '「でんたくん」基本演算 試験手順', totalItems: 9, sectionCount: 3,
    tags: ['共通', '共通', '共通'],
    glossaryKeys: ['でんたくん', 'クリア', 'M+'],
    materialsKeys: [],
    build: { mode: 'input', hint: '画面右上の「?」→「バージョン情報」に表示される番号' }
  },
  {
    name: 'SAMPLE_A en', md: MEN.SAMPLE_A, labels: SEN,
    title: '"Pomodorin" timer behavior check', totalItems: 15, sectionCount: 5,
    tags: ['Windows', 'Windows', 'Windows', 'Windows', 'Android'],
    glossaryKeys: ['Start button', 'Reset', 'Set work time'],
    materialsKeys: ['long-session.pomo', 'beep.wav'],
    build: { mode: 'fixed', value: '1.2.3-beta4' }
  },
  {
    name: 'SAMPLE_B en', md: MEN.SAMPLE_B, labels: SEN,
    title: '"Calcy" basic arithmetic test procedure', totalItems: 9, sectionCount: 3,
    tags: ['Common', 'Common', 'Common'],
    glossaryKeys: ['Calcy', 'Clear', 'M+'],
    materialsKeys: [],
    build: { mode: 'input', hint: 'the number shown under "?" → "About" at the top right' }
  }
];
// warnings 追加後に許される戻り値のキー集合（warnings 以外は増減しないこと）。
const KNOWN_RESULT_KEYS = JSON.stringify(
  ['title', 'preamble', 'sections', 'glossary', 'build', 'materials', 'totalItems', 'ok', 'warnings'].sort()
);
for (const c of UNCHANGED_CASES) {
  const rr = parser.parseProcedure(c.md, c.labels);
  ok(`unchanged(${c.name}): title`, rr.title === c.title, JSON.stringify(rr.title));
  ok(`unchanged(${c.name}): totalItems`, rr.totalItems === c.totalItems, rr.totalItems);
  ok(`unchanged(${c.name}): sections.length`, rr.sections.length === c.sectionCount, rr.sections.length);
  ok(`unchanged(${c.name}): tags`, JSON.stringify(rr.sections.map((s) => s.tag)) === JSON.stringify(c.tags),
     JSON.stringify(rr.sections.map((s) => s.tag)));
  ok(`unchanged(${c.name}): glossary keys`,
     JSON.stringify(rr.glossary.map((g) => g.key)) === JSON.stringify(c.glossaryKeys),
     JSON.stringify(rr.glossary.map((g) => g.key)));
  ok(`unchanged(${c.name}): materials keys`,
     JSON.stringify(rr.materials.map((m) => m.key)) === JSON.stringify(c.materialsKeys),
     JSON.stringify(rr.materials.map((m) => m.key)));
  ok(`unchanged(${c.name}): build`, JSON.stringify(rr.build) === JSON.stringify(c.build), JSON.stringify(rr.build));
  ok(`unchanged(${c.name}): result keys are the known set (+warnings)`,
     JSON.stringify(Object.keys(rr).sort()) === KNOWN_RESULT_KEYS, JSON.stringify(Object.keys(rr).sort()));
  ok(`unchanged(${c.name}): warnings is an array`, Array.isArray(rr.warnings), typeof rr.warnings);
}

// --- 2. 警告が出ること（設計書1.1の8ケース＋残りのkind。行番号まで見る）---
// 設計書1.1の例示のうち row2/row3 はそのままの文言だと実際には落ちない
// （row2: 太字＋区切りなしは rest 全体を desc として拾ってしまう／row3: "OK" は2文字なので
// key-too-short に掛からない）ため、該当 kind を実際に踏み抜く最小入力に置き換えている。
function hasWarning(warnings, kind, line) {
  return warnings.some((w) => w.kind === kind && w.line === line);
}
const WARNING_CASES = [
  {
    name: '1 section-without-table', kind: 'section-without-table', line: 6,
    md: ['## T', '### 試験の概要', '', '準備するもの: x', '', '### 3. 設定の保存', '', 'ここには表がありません'].join('\n')
  },
  {
    name: '2 glossary-no-separator', kind: 'glossary-no-separator', line: 6,
    md: ['## T', '### Pre', '', '用語定義:', '', '* 開始ボタン 緑色のボタン'].join('\n')
  },
  {
    name: '2b glossary-empty-term-or-desc', kind: 'glossary-empty-term-or-desc', line: 6,
    md: ['## T', '### Pre', '', '用語定義:', '', '* **開始ボタン**'].join('\n')
  },
  {
    name: '3 glossary-key-too-short', kind: 'glossary-key-too-short', line: 6,
    md: ['## T', '### Pre', '', '用語定義:', '', '* **開** … 確定ボタン'].join('\n')
  },
  {
    name: '4 glossary-duplicate-key', kind: 'glossary-duplicate-key', line: 7,
    md: ['## T', '### Pre', '', '用語定義:', '', '* **開始ボタン** … 説明1', '* **開始ボタン** … 説明2'].join('\n')
  },
  {
    name: '5 keyword-in-table-section', kind: 'keyword-in-table-section', line: 4,
    md: ['## T', '### 1. S', '', '用語定義:', '', '| No | Step | Expected |', '|---|---|---|', '| 1 | a | b |'].join('\n')
  },
  {
    name: '6 list-interrupted', kind: 'list-interrupted', line: 7,
    md: ['## T', '### Pre', '', '用語定義:', '', '* **開始ボタン** … 緑色のボタン', 'これは説明文です',
      '* **リセット** … リセットボタン'].join('\n')
  },
  {
    // 一番ありがちな形。地の文の前後に空行が入るので、最後の項目から4行後に再開する。
    // 固定幅の後読み窓だとここを取りこぼす（実際に取りこぼしていた回帰ケース）。
    name: '6b list-interrupted（空行を挟んで4行後に再開）', kind: 'list-interrupted', line: 8,
    md: ['## T', '### Pre', '', '用語定義:', '', '* **開始ボタン** … 緑色のボタン', '',
      'これは説明文です', '', '* **リセット** … リセットボタン'].join('\n')
  },
  {
    // 地の文が3行を超えたら別の話題とみなして警告しない（誤警報の上限）。
    name: '6c 地の文が4行続いたら list-interrupted にしない', notKind: 'list-interrupted',
    md: ['## T', '### Pre', '', '用語定義:', '', '* **開始ボタン** … 緑色のボタン', '',
      '説明1', '説明2', '説明3', '説明4', '', '* **別の箇条書き** … これは別物'].join('\n')
  },
  {
    // 別のキーワードが始まったら、その先の箇条書きはそちらのものなので途切れではない。
    name: '6d 配布物: が続く場合は list-interrupted にしない', notKind: 'list-interrupted',
    md: ['## T', '### Pre', '', '用語定義:', '', '* **開始ボタン** … 緑色のボタン', '',
      '配布物:', '', '* **sample.bin** … https://example.com/a.bin the file'].join('\n')
  },
  {
    name: '7 materials-no-separator', kind: 'materials-no-separator', line: 6,
    md: ['## T', '### Pre', '', '配布物:', '', '* sample.bin the file'].join('\n')
  },
  {
    name: '7b materials-empty-name-or-desc', kind: 'materials-empty-name-or-desc', line: 6,
    md: ['## T', '### Pre', '', '配布物:', '', '* … the file with no name'].join('\n')
  },
  {
    name: '7c materials-duplicate-key', kind: 'materials-duplicate-key', line: 7,
    md: ['## T', '### Pre', '', '配布物:', '', '* **sample.bin** … https://example.com/a first',
      '* **sample.bin** … https://example.com/b second'].join('\n')
  },
  {
    name: '7d material-no-url', kind: 'material-no-url', line: 6,
    md: ['## T', '### Pre', '', '配布物:', '', '* **sample.bin** … a file with no url'].join('\n')
  },
  {
    name: '8 table-column-count-mismatch', kind: 'table-column-count-mismatch', line: 6,
    md: ['## T', '### 1. S', '', '| No | Step | Expected |', '|---|---|---|', '| 1 | a|b | c |'].join('\n')
  },
  {
    name: '9 duplicate-item-number', kind: 'duplicate-item-number', line: 7,
    md: ['## T', '### 1. S', '', '| No | Step | Expected |', '|---|---|---|', '| 1 | a | b |', '| 1 | c | d |'].join('\n')
  },
  { name: '10 no-items', kind: 'no-items', line: null, md: '' }
];
for (const c of WARNING_CASES) {
  const ws = parser.parseProcedure(c.md).warnings;
  if (c.notKind) {
    // 誤警報を出さないことの検証（警告が出ない側の境界）
    ok(`warning case ${c.name}: ${c.notKind} が出ない`,
       !ws.some((w) => w.kind === c.notKind), JSON.stringify(ws));
  } else {
    ok(`warning case ${c.name}: kind+line found`, hasWarning(ws, c.kind, c.line), JSON.stringify(ws));
  }
}

// --- 3. 正常な手順書では余計な警告が出ないこと（infoは許容） ---
const WARN_OR_ERROR_KINDS = new Set([
  'no-items', 'section-without-table', 'glossary-no-separator', 'glossary-empty-term-or-desc',
  'glossary-key-too-short', 'materials-no-separator', 'materials-empty-name-or-desc',
  'list-interrupted', 'keyword-in-table-section', 'table-column-count-mismatch'
]);
for (const c of UNCHANGED_CASES) {
  const ws = parser.parseProcedure(c.md, c.labels).warnings;
  const bad = ws.filter((w) => WARN_OR_ERROR_KINDS.has(w.kind));
  ok(`no warn/error-level warnings on ${c.name}`, bad.length === 0, JSON.stringify(bad));
}
// AI整形プロンプト自身の実例も同様（プロンプトの自己矛盾チェックを兼ねる）
for (const [lang, prompt, labels] of [['ja', MJA.AI_FORMAT_PROMPT, undefined], ['en', MEN.AI_FORMAT_PROMPT, SEN]]) {
  const m = prompt.match(/```markdown\n([\s\S]*?)```/);
  if (!m) continue;
  const ws = parser.parseProcedure(m[1], labels).warnings;
  const bad = ws.filter((w) => WARN_OR_ERROR_KINDS.has(w.kind));
  ok(`no warn/error-level warnings on AI prompt(${lang}) example`, bad.length === 0, JSON.stringify(bad));
}

// --- 4. ラベル注入 ---
const twoColMd = ['## T', '### 1. S', '', '| No | Step |', '|---|---|', '| 1 | a |'].join('\n');
ok('label injection: default (no 2nd arg) uses ja notSpecified',
   parser.parseProcedure(twoColMd).sections[0].items[0].expectedRaw === '（記載なし）',
   parser.parseProcedure(twoColMd).sections[0].items[0].expectedRaw);
ok('label injection: en labels switch notSpecified',
   parser.parseProcedure(twoColMd, SEN).sections[0].items[0].expectedRaw === SEN.notSpecified,
   parser.parseProcedure(twoColMd, SEN).sections[0].items[0].expectedRaw);
const customLabels = { notSpecified: 'X', simpleFormatSectionTitle: 'Y' };
ok('label injection: custom notSpecified',
   parser.parseProcedure(twoColMd, customLabels).sections[0].items[0].expectedRaw === 'X',
   parser.parseProcedure(twoColMd, customLabels).sections[0].items[0].expectedRaw);
ok('label injection: default fallback section title',
   parser.parseProcedure('起動する\t画面が出る').sections[0].title === '手順',
   parser.parseProcedure('起動する\t画面が出る').sections[0].title);
ok('label injection: en labels switch fallback section title',
   parser.parseProcedure('起動する\t画面が出る', SEN).sections[0].title === SEN.simpleFormatSectionTitle,
   parser.parseProcedure('起動する\t画面が出る', SEN).sections[0].title);
ok('label injection: custom fallback section title',
   parser.parseProcedure('起動する\t画面が出る', customLabels).sections[0].title === 'Y',
   parser.parseProcedure('起動する\t画面が出る', customLabels).sections[0].title);

// --- 5. test-procedure-formatter スキルの配布元（src/api/test_runner/skill/）---
// ローカルの .claude/skills/ は zip を展開して置くだけの使い捨てなので、もう検証対象
// ではない（合意事項3）。正本はここ（skill/）で、parser.js は「置かないこと」自体が
// 検証対象（GET /TestRunner/skill.zip がリクエストのたびに本体の
// static/js/core/parser.js を直接詰めるので、コピーがあると二重管理でズレる）。
const REPO_ROOT_FOR_SKILL = fileURLToPath(new URL('../../../', import.meta.url));
const SKILL_DIR = REPO_ROOT_FOR_SKILL + 'src/api/test_runner/skill/';
function readIfExists(p) {
  try { return fs.readFileSync(p); } catch (e) { return null; }
}

const skillFormatJa = readIfExists(SKILL_DIR + 'format.ja.md');
const skillFormatEn = readIfExists(SKILL_DIR + 'format.en.md');
ok('skill format.ja.md exists', skillFormatJa !== null, SKILL_DIR + 'format.ja.md');
ok('skill format.en.md exists', skillFormatEn !== null, SKILL_DIR + 'format.en.md');
if (skillFormatJa) {
  ok('skill format.ja.md matches AI_FORMAT_PROMPT(ja) exactly', skillFormatJa.toString('utf-8') === MJA.AI_FORMAT_PROMPT);
}
if (skillFormatEn) {
  ok('skill format.en.md matches AI_FORMAT_PROMPT(en) exactly', skillFormatEn.toString('utf-8') === MEN.AI_FORMAT_PROMPT);
}

const skillMd = readIfExists(SKILL_DIR + 'SKILL.md');
ok('skill SKILL.md exists', skillMd !== null, SKILL_DIR + 'SKILL.md');
if (skillMd) {
  const nameMatch = skillMd.toString('utf-8').match(/^---\r?\n[\s\S]*?^name:\s*(\S+)\s*$/m);
  ok('skill SKILL.md frontmatter name is test-procedure-formatter',
     !!nameMatch && nameMatch[1] === 'test-procedure-formatter',
     nameMatch ? nameMatch[1] : '(name: not found)');
}

const skillValidateMjs = readIfExists(SKILL_DIR + 'validate.mjs');
ok('skill validate.mjs exists', skillValidateMjs !== null, SKILL_DIR + 'validate.mjs');
if (skillValidateMjs) {
  const src = skillValidateMjs.toString('utf-8');
  const importSpecifiers = [...src.matchAll(/^import\s[\s\S]*?from\s+['"]([^'"]+)['"]/gm)].map((m) => m[1]);
  ok('skill validate.mjs has at least one import', importSpecifiers.length > 0, importSpecifiers);
  const badImports = importSpecifiers.filter((spec) => spec !== './parser.js' && !spec.startsWith('node:'));
  ok('skill validate.mjs only imports node:* and ./parser.js (no repo-relative or external packages)',
     badImports.length === 0, badImports);
}

// parser.js のコピーが無いこと（今回の設計の要）。ズレの再発をここで機械的に検出する。
ok('skill/parser.js does NOT exist (parser is packed from static/js/core/parser.js at zip time)',
   readIfExists(SKILL_DIR + 'parser.js') === null, SKILL_DIR + 'parser.js');

console.log(fail === 0 ? '\nALL PASS' : '\n' + fail + ' FAILURES');
if (fail > 0) process.exit(1);
