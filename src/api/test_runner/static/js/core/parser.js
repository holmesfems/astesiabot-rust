/* =========================================================================
   PARSER (pure functions — decode + parseProcedure)
   DOM非依存（document/window/localStorageを参照しない）。
   依存ゼロ（他モジュールを import しない）。スキルに1ファイルで同梱するため。
   ========================================================================= */

/** 手順書側に記載が無いときに埋める文言。呼び出し側が渡さなければ日本語を使う。 */
export const DEFAULT_LABELS = {
  notSpecified: '（記載なし）',
  simpleFormatSectionTitle: '手順',
};

export function escapeHtml(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

// 表のセル内は1行で書くしかないので、`<br>` / `<br/>` / `<br />`（大文字小文字不問）を改行として扱う。
// コードスパン内の `<br>` はリテラルのまま残すため、コードスパンと同じ走査で拾う。
export function applyInline(raw) {
  var s = escapeHtml(raw);
  s = s.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/`(.+?)`|&lt;br\s*\/?&gt;/gi, function (m, code) {
    return code !== undefined ? '<code>' + code + '</code>' : '<br>';
  });
  return s;
}

// プレーンテキスト化。`<br>` は改行（\n）に戻す（呼び出し側が \n を ' / ' 等に置き換える）。
export function stripMd(raw) {
  return String(raw).replace(/\*\*(.+?)\*\*/g, '$1').replace(/`(.+?)`|<br\s*\/?>/gi, function (m, code) {
    return code !== undefined ? code : '\n';
  });
}

function escapeRegExp(s) {
  return String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/* ---- warnings（「黙って捨てたもの」の機械可読レポート） ----
   parseProcedure の挙動は一切変えない。既存の分岐がこれまで通り null を返す/
   entry を捨てるのを見ている箇所に、追加でここへ積むだけ。
   人間向けの文言は持たない（kind の文字列だけ）。CLI 側（validate.mjs）が
   kind から文言を引く。line は1始まり。特定できなければ null。 */
function truncateWarningText(s) {
  var t = String(s == null ? '' : s).trim();
  return t.length > 120 ? t.slice(0, 120) : t;
}

function pushWarning(list, kind, line, text) {
  list.push({
    kind: kind,
    line: line == null ? null : line,
    text: text == null ? null : truncateWarningText(text)
  });
}

// 箇条書きの読み取りループが止まった直後3行以内に、また箇条書きが現れるかを見る。
// 「地の文に途切れさせられて残りの項目が丸ごと落ちた」疑いの検知用。
// 既存の while ループの読み取り範囲そのものは変えない（ここは読み取り後の後読みだけ）。
// 空行は読み飛ばし、地の文を LIST_INTERRUPT_MAX_PROSE 行までまたいで箇条書きが再開して
// いたら「途切れた」とみなす。行数の固定窓にすると
//   * 最後の項目 / 空行 / 地の文 / 空行 / * 続きの項目
// という一番ありがちな形（空行を挟むので4行後になる）を取りこぼす。
// 警告が指すのは再開した箇条書きではなく、原因になった最初の地の文の行。
// 別のキーワード行（`配布物:` など）に当たったらそこで打ち切る。そこから先の箇条書きは
// そのキーワードのものとして正しく読まれるので、途切れではない。
var LIST_INTERRUPT_MAX_PROSE = 3;
function checkListInterrupted(lines, stopIdx, lineOf, warnings, kind) {
  var proseIdx = -1;
  var proseCount = 0;
  for (var k = stopIdx; k < lines.length; k++) {
    var t = lines[k].trim();
    if (t === '') continue;
    // 前置きブロックの bodyLines なので ### / ## はここに現れない（ブロック分割で消費済み）。
    if (GLOSSARY_HEADING_RE.test(t) || MATERIALS_LINE_RE.test(t)) return;
    if (/^[*\-]\s+/.test(t)) {
      if (proseIdx >= 0) pushWarning(warnings, kind, lineOf(proseIdx), lines[proseIdx].trim());
      return;
    }
    if (proseIdx < 0) proseIdx = k;
    proseCount++;
    if (proseCount > LIST_INTERRUPT_MAX_PROSE) return;
  }
}

/* ---- glossary (用語定義 / Glossary) parsing ---- */
// 手順書側の見出しキーワードは日英どちらでも拾う。日本語部分は /i の影響を受けない。
var GLOSSARY_HEADING_RE = /用語定義|Definitions?|Glossary/i;
var GLOSSARY_PLACEHOLDER_RE = /◯◯|〇〇|○○|××|XX|ＸＸ/g;
var GLOSSARY_SEP_RE = /\.\.\.|…|：|:|—|－|\s-\s/;

// 戻り値を { ok:false, kind } / { ok:true, entry } に変えただけで、
// term/desc/key の判定条件そのものは1文字も変えていない（ok:false になる条件＝旧コードで
// return null になっていた条件と完全一致）。
function parseGlossaryItem(itemText) {
  var text = String(itemText).trim();
  if (!text) return { ok: false, kind: 'glossary-empty-term-or-desc' };
  var term, desc;
  var boldMatch = text.match(/^\*\*(.+?)\*\*\s*(.*)$/);
  if (boldMatch) {
    term = boldMatch[1].trim();
    var rest = boldMatch[2];
    var sepMatch = rest.match(GLOSSARY_SEP_RE);
    desc = sepMatch ? rest.slice(sepMatch.index + sepMatch[0].length).trim() : rest.trim();
  } else {
    var m = text.match(GLOSSARY_SEP_RE);
    if (!m) return { ok: false, kind: 'glossary-no-separator' };
    term = text.slice(0, m.index).replace(/\*\*/g, '').trim();
    desc = text.slice(m.index + m[0].length).trim();
  }
  if (!term || !desc) return { ok: false, kind: 'glossary-empty-term-or-desc' };
  var key = term.replace(GLOSSARY_PLACEHOLDER_RE, '').trim();
  if (key.length < 2) return { ok: false, kind: 'glossary-key-too-short' };
  return { ok: true, entry: { term: term, key: key, descHtml: applyInline(desc) } };
}

function extractGlossary(preamble) {
  var glossary = [];
  var warnings = [];
  var seenKeys = {};
  (preamble || []).forEach(function (block) {
    var lines = block.bodyLines || [];
    var bodyStartLine = block.bodyStartLine;
    function lineOf(idx) { return bodyStartLine == null ? null : bodyStartLine + idx; }
    var i = 0;
    while (i < lines.length) {
      if (GLOSSARY_HEADING_RE.test(lines[i])) {
        i++;
        while (i < lines.length && lines[i].trim() === '') i++;
        while (i < lines.length && /^[*\-]\s+/.test(lines[i].trim())) {
          var rawLine = lines[i].trim();
          var itemText = rawLine.replace(/^[*\-]\s+/, '');
          var result = parseGlossaryItem(itemText);
          if (result.ok) {
            if (!seenKeys[result.entry.key]) {
              seenKeys[result.entry.key] = true;
              glossary.push(result.entry);
            } else {
              pushWarning(warnings, 'glossary-duplicate-key', lineOf(i), rawLine);
            }
          } else {
            pushWarning(warnings, result.kind, lineOf(i), rawLine);
          }
          i++;
        }
        checkListInterrupted(lines, i, lineOf, warnings, 'list-interrupted');
        continue;
      }
      i++;
    }
  });
  return { glossary: glossary, warnings: warnings };
}

/* ---- ビルド指定（ビルド: / Build:）/ 配布物（配布物: / Attachments:）— どちらも任意項目 ----
   前置きの本文行から拾う。行が無ければ build は {mode:'none'}、materials は [] になり、
   従来の手順書は一切挙動が変わらない。
   英語エイリアスは (?:...) の非キャプチャグループで足すこと。BUILD_LINE_RE の m[1] と
   BUILD_INPUT_RE のグループ1を呼び出し側が使っているので、番号をずらすと壊れる。 */
var BUILD_LINE_RE = /^(?:ビルド|Build|Version)\s*[:：]\s*(.*)$/i;
var BUILD_INPUT_RE = /^(記入|入力|Enter|Fill ?in|TBD)/i;
var BUILD_HINT_RE = /[（(]([^）)]*)[）)]/;
var MATERIALS_LINE_RE = /^(?:配布物|Attachments?|Downloads?|Assets?)\s*[:：]/i;
var MATERIAL_URL_RE = /https?:\/\/\S+/;
var MATERIAL_URL_TAIL_RE = /[）)、。，,.]+$/;

function parseBuildValue(value) {
  var v = String(value == null ? '' : value).trim();
  if (v === '') return { mode: 'none' };
  if (BUILD_INPUT_RE.test(v)) {
    var hint = v.match(BUILD_HINT_RE);
    return { mode: 'input', hint: hint ? hint[1].trim() : '' };
  }
  return { mode: 'fixed', value: v };
}

function extractBuild(preamble) {
  var found = null;
  (preamble || []).forEach(function (block) {
    (block.bodyLines || []).forEach(function (line) {
      if (found) return;
      var m = String(line).trim().match(BUILD_LINE_RE);
      if (m) found = parseBuildValue(m[1]);
    });
  });
  return found || { mode: 'none' };
}

// 配布物1件。区切りの扱いは parseGlossaryItem と同じ（`* **名前** … 説明`）。
// 説明の中の最初の http(s) URL を url として切り出し、残りを説明文にする。
// glossary と同じく、ok:false になる条件は旧コードの return null と完全一致。
// name が空でも desc は空チェックしていない（旧コードも同じ）ので
// materials-empty-name-or-desc は実質「name が空」のときにしか出ない。
function parseMaterialItem(itemText) {
  var text = String(itemText).trim();
  if (!text) return { ok: false, kind: 'materials-empty-name-or-desc' };
  var name, desc;
  var boldMatch = text.match(/^\*\*(.+?)\*\*\s*(.*)$/);
  if (boldMatch) {
    name = boldMatch[1].trim();
    var rest = boldMatch[2];
    var sepMatch = rest.match(GLOSSARY_SEP_RE);
    desc = sepMatch ? rest.slice(sepMatch.index + sepMatch[0].length).trim() : rest.trim();
  } else {
    var m = text.match(GLOSSARY_SEP_RE);
    if (!m) return { ok: false, kind: 'materials-no-separator' };
    name = text.slice(0, m.index).replace(/\*\*/g, '').trim();
    desc = text.slice(m.index + m[0].length).trim();
  }
  if (!name) return { ok: false, kind: 'materials-empty-name-or-desc' };
  var url = null;
  var um = desc.match(MATERIAL_URL_RE);
  if (um) {
    url = um[0].replace(MATERIAL_URL_TAIL_RE, '');
    desc = (desc.slice(0, um.index) + desc.slice(um.index + um[0].length)).replace(/\s+/g, ' ').trim();
  }
  // key が2文字未満でも項目としては残す（本文ハイライトの対象から外れるだけ）
  var key = name.replace(GLOSSARY_PLACEHOLDER_RE, '').trim();
  return { ok: true, entry: { name: name, key: key, url: url, descHtml: applyInline(desc) } };
}

function extractMaterials(preamble) {
  var materials = [];
  var warnings = [];
  var seenKeys = {};
  (preamble || []).forEach(function (block) {
    var lines = block.bodyLines || [];
    var bodyStartLine = block.bodyStartLine;
    function lineOf(idx) { return bodyStartLine == null ? null : bodyStartLine + idx; }
    var i = 0;
    while (i < lines.length) {
      if (MATERIALS_LINE_RE.test(lines[i].trim())) {
        i++;
        while (i < lines.length && lines[i].trim() === '') i++;
        while (i < lines.length && /^[*\-]\s+/.test(lines[i].trim())) {
          var rawLine = lines[i].trim();
          var itemText = rawLine.replace(/^[*\-]\s+/, '');
          var result = parseMaterialItem(itemText);
          if (result.ok) {
            if (!seenKeys[result.entry.key]) {
              seenKeys[result.entry.key] = true;
              materials.push(result.entry);
              if (!result.entry.url) {
                pushWarning(warnings, 'material-no-url', lineOf(i), rawLine);
              }
            } else {
              pushWarning(warnings, 'materials-duplicate-key', lineOf(i), rawLine);
            }
          } else {
            pushWarning(warnings, result.kind, lineOf(i), rawLine);
          }
          i++;
        }
        checkListInterrupted(lines, i, lineOf, warnings, 'list-interrupted');
        continue;
      }
      i++;
    }
  });
  return { materials: materials, warnings: warnings };
}

// 用語と配布物を1本の正規表現でまとめて拾う。キーが重なったら長い方を優先し、
// 同じ長さなら配布物を優先する（alternation は先頭から順に試されるので並び順がそのまま優先順位）。
function buildHighlightIndex(glossary, materials) {
  var map = {};
  (glossary || []).forEach(function (g, idx) {
    if (g.key && g.key.length >= 2 && !(g.key in map)) map[g.key] = { kind: 'term', idx: idx };
  });
  (materials || []).forEach(function (m, idx) {
    if (m.key && m.key.length >= 2) map[m.key] = { kind: 'material', idx: idx };
  });
  return map;
}

export function highlightGlossaryHtml(html, glossary, materials) {
  var map = buildHighlightIndex(glossary, materials);
  var keys = Object.keys(map);
  if (keys.length === 0) return { html: html, matched: false, matchedTerm: false, matchedMaterial: false };
  keys.sort(function (a, b) {
    if (b.length !== a.length) return b.length - a.length;
    var am = map[a].kind === 'material' ? 0 : 1;
    var bm = map[b].kind === 'material' ? 0 : 1;
    return am - bm;
  });
  var re = new RegExp(keys.map(escapeRegExp).join('|'), 'g');
  var matchedTerm = false;
  var matchedMaterial = false;
  var parts = html.split(/(<[^>]+>)/);
  for (var p = 0; p < parts.length; p++) {
    if (p % 2 === 1) continue; // HTML tag, skip
    if (!parts[p]) continue;
    parts[p] = parts[p].replace(re, function (m) {
      var hit = map[m];
      if (hit.kind === 'material') {
        matchedMaterial = true;
        return '<span class="term term-material" data-material-idx="' + hit.idx + '">📎' + m + '</span>';
      }
      matchedTerm = true;
      return '<span class="term" data-term-idx="' + hit.idx + '">' + m + '</span>';
    });
  }
  return {
    html: parts.join(''),
    matched: matchedTerm || matchedMaterial,
    matchedTerm: matchedTerm,
    matchedMaterial: matchedMaterial
  };
}

function isTableLine(line) {
  return /^\s*\|/.test(line);
}

function isSeparatorRow(cells) {
  if (cells.length === 0) return false;
  return cells.every(function (c) { return /^:?-+:?$/.test(c.trim()); });
}

function splitTableRow(line) {
  var trimmed = line.trim();
  if (trimmed.charAt(0) === '|') trimmed = trimmed.slice(1);
  if (trimmed.charAt(trimmed.length - 1) === '|') trimmed = trimmed.slice(0, -1);
  return trimmed.split('|').map(function (c) { return c.trim(); });
}

function parseHeadingMeta(headingText) {
  var text = headingText.trim();
  var number = null;
  var mNum = text.match(/^(\d+)\.\s*/);
  if (mNum) {
    number = mNum[1];
    text = text.slice(mNum[0].length);
  }
  // 節タグ（試験対象OS等）は見出しの末尾。日本語の【】と英語手順書向けの[]の両方を受ける。
  // 末尾アンカーなので `### See [docs](url)` は `)` 終わりでマッチせず、`### [Draft] Login` も
  // 行末でないのでマッチしない。
  var tag = null;
  var mTag = text.match(/【([^】]*)】\s*$|\[([^\]]*)\]\s*$/);
  if (mTag) {
    tag = mTag[1] !== undefined ? mTag[1] : mTag[2];
    text = text.slice(0, mTag.index).trim();
  }
  return { number: number, tag: tag, title: text.trim() };
}

export function parseProcedure(rawText, labels) {
  var L = labels || DEFAULT_LABELS;
  var text = String(rawText).replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  var lines = text.split('\n');

  var warnings = [];

  var docTitle = '';
  var blocks = [];
  var current = null;

  // block.lines は元テキストの連続したスライスなので、ブロックごとに開始行番号を
  // 1つ持てば block.lines[j] の絶対行が startLine + j で求まる（行番号の配列は並走させない）。
  for (var i = 0; i < lines.length; i++) {
    var line = lines[i];
    if (/^##\s+/.test(line) && !/^###/.test(line)) {
      docTitle = line.replace(/^##\s+/, '').trim();
      continue;
    }
    if (/^###\s+/.test(line)) {
      current = {
        heading: line.replace(/^###\s+/, '').trim(),
        headingRaw: line,
        headingLine: i + 1,
        startLine: i + 2,
        lines: []
      };
      blocks.push(current);
      continue;
    }
    if (current) current.lines.push(line);
  }

  var preamble = [];
  var sections = [];
  var totalItems = 0;
  var seenItemNumbers = {};

  blocks.forEach(function (block) {
    var hasTable = block.lines.some(function (l) { return isTableLine(l); });
    var meta = parseHeadingMeta(block.heading);

    if (!hasTable) {
      // 見出しが「1.」のような番号付きなら、他の試験項目節と同じ命名なのに表が無い
      // ＝表を書き忘れて前置き扱いに落ちた疑いが強いので警告する。「試験の概要」のような
      // 番号なし見出し（本来から前置きとして書かれる節）は対象外にする。
      if (meta.number !== null) {
        pushWarning(warnings, 'section-without-table', block.headingLine, block.headingRaw);
      }
      var bodyLines = block.lines.slice();
      var leadingBlank = 0;
      while (bodyLines.length && bodyLines[0].trim() === '') { bodyLines.shift(); leadingBlank++; }
      while (bodyLines.length && bodyLines[bodyLines.length - 1].trim() === '') bodyLines.pop();
      preamble.push({ heading: block.heading, bodyLines: bodyLines, bodyStartLine: block.startLine + leadingBlank });
      return;
    }

    var tableRowsRaw = [];
    var nonTableLines = [];
    var skippedHeader = false;
    var skippedSep = false;
    var headerCellCount = null;

    for (var j = 0; j < block.lines.length; j++) {
      var l2 = block.lines[j];
      var absLine = block.startLine + j;
      if (isTableLine(l2)) {
        var cells = splitTableRow(l2);
        if (!skippedHeader) { skippedHeader = true; headerCellCount = cells.length; continue; }
        if (!skippedSep && isSeparatorRow(cells)) { skippedSep = true; continue; }
        if (headerCellCount !== null && cells.length !== headerCellCount) {
          pushWarning(warnings, 'table-column-count-mismatch', absLine, l2.trim());
        }
        tableRowsRaw.push({ cells: cells, line: absLine, raw: l2.trim() });
      } else if (l2.trim() !== '') {
        var trimmed2 = l2.trim();
        if (GLOSSARY_HEADING_RE.test(trimmed2) || MATERIALS_LINE_RE.test(trimmed2)) {
          pushWarning(warnings, 'keyword-in-table-section', absLine, trimmed2);
        }
        nonTableLines.push(trimmed2);
      }
    }

    var items = [];
    tableRowsRaw.forEach(function (row) {
      var cells = row.cells;
      if (cells.length === 0) return;
      var num = cells[0] || '';
      var step = cells.length > 1 ? cells[1] : '';
      var expected;
      if (cells.length <= 2) {
        expected = L.notSpecified;
      } else {
        expected = cells.slice(2).join(' ').trim();
        if (expected === '') expected = L.notSpecified;
      }
      if (num !== '') {
        if (seenItemNumbers[num]) {
          pushWarning(warnings, 'duplicate-item-number', row.line, row.raw);
        } else {
          seenItemNumbers[num] = true;
        }
      }
      items.push({
        number: num,
        stepRaw: step,
        expectedRaw: expected,
        stepHtml: applyInline(step),
        expectedHtml: applyInline(expected)
      });
      totalItems++;
    });

    sections.push({
      number: meta.number,
      tag: meta.tag,
      title: meta.title,
      note: nonTableLines.join('\n'),
      items: items
    });
  });

  if (sections.length === 0) {
    var fallbackItems = [];
    lines.forEach(function (l) {
      if (l.trim() === '') return;
      if (/^#/.test(l.trim())) return;
      var parts = l.split(/\t|→/);
      var step = parts[0] !== undefined ? parts[0].trim() : l.trim();
      var expected = parts.length > 1 ? parts.slice(1).join('→').trim() : '';
      if (expected === '') expected = L.notSpecified;
      fallbackItems.push({
        number: String(fallbackItems.length + 1),
        stepRaw: step,
        expectedRaw: expected,
        stepHtml: applyInline(step),
        expectedHtml: applyInline(expected)
      });
      totalItems++;
    });
    if (fallbackItems.length > 0) {
      sections.push({ number: '1', tag: null, title: L.simpleFormatSectionTitle, note: '', items: fallbackItems });
    }
  }

  var glossaryResult = extractGlossary(preamble);
  var materialsResult = extractMaterials(preamble);
  warnings = warnings.concat(glossaryResult.warnings, materialsResult.warnings);

  if (totalItems === 0) {
    pushWarning(warnings, 'no-items', null, null);
  }

  // bodyStartLine は行番号計算のための内部情報。戻り値の preamble には元の形
  // （heading/bodyLines の2キーのみ）だけを載せる（挙動を1つも変えない合意のため）。
  var preamblePublic = preamble.map(function (p) {
    return { heading: p.heading, bodyLines: p.bodyLines };
  });

  return {
    title: docTitle,
    preamble: preamblePublic,
    sections: sections,
    glossary: glossaryResult.glossary,
    build: extractBuild(preamble),
    materials: materialsResult.materials,
    totalItems: totalItems,
    ok: totalItems > 0,
    warnings: warnings
  };
}

export function decodeAuto(bytes) {
  var buf = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  if (buf.length >= 2 && buf[0] === 0xff && buf[1] === 0xfe) {
    return new TextDecoder('utf-16le').decode(buf.slice(2));
  }
  if (buf.length >= 2 && buf[0] === 0xfe && buf[1] === 0xff) {
    return new TextDecoder('utf-16be').decode(buf.slice(2));
  }
  if (buf.length >= 3 && buf[0] === 0xef && buf[1] === 0xbb && buf[2] === 0xbf) {
    return new TextDecoder('utf-8').decode(buf.slice(3));
  }
  var sampleLen = Math.min(buf.length, 4000);
  var zerosOdd = 0, zerosEven = 0;
  for (var i = 0; i < sampleLen; i++) {
    if (buf[i] === 0) { if (i % 2 === 1) zerosOdd++; else zerosEven++; }
  }
  if (zerosOdd > sampleLen * 0.15 && zerosOdd > zerosEven) {
    return new TextDecoder('utf-16le').decode(buf);
  }
  return new TextDecoder('utf-8').decode(buf);
}

export function decodeArrayBufferAuto(buf) {
  return decodeAuto(new Uint8Array(buf));
}
