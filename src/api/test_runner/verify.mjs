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

console.log(fail === 0 ? '\nALL PASS' : '\n' + fail + ' FAILURES');
if (fail > 0) process.exit(1);
