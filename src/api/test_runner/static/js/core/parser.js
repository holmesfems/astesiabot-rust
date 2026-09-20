import { T } from '../constants/i18n.js';

/* =========================================================================
   PARSER (pure functions — decode + parseProcedure)
   DOM非依存（document/window/localStorageを参照しない）。
   ========================================================================= */

export function escapeHtml(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

export function applyInline(raw) {
  var s = escapeHtml(raw);
  s = s.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/`(.+?)`/g, '<code>$1</code>');
  return s;
}

export function stripMd(raw) {
  return String(raw).replace(/\*\*(.+?)\*\*/g, '$1').replace(/`(.+?)`/g, '$1');
}

function escapeRegExp(s) {
  return String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/* ---- glossary (用語定義 / Glossary) parsing ---- */
// 手順書側の見出しキーワードは日英どちらでも拾う。日本語部分は /i の影響を受けない。
var GLOSSARY_HEADING_RE = /用語定義|Definitions?|Glossary/i;
var GLOSSARY_PLACEHOLDER_RE = /◯◯|〇〇|○○|××|XX|ＸＸ/g;
var GLOSSARY_SEP_RE = /\.\.\.|…|：|:|—|－|\s-\s/;

function parseGlossaryItem(itemText) {
  var text = String(itemText).trim();
  if (!text) return null;
  var term, desc;
  var boldMatch = text.match(/^\*\*(.+?)\*\*\s*(.*)$/);
  if (boldMatch) {
    term = boldMatch[1].trim();
    var rest = boldMatch[2];
    var sepMatch = rest.match(GLOSSARY_SEP_RE);
    desc = sepMatch ? rest.slice(sepMatch.index + sepMatch[0].length).trim() : rest.trim();
  } else {
    var m = text.match(GLOSSARY_SEP_RE);
    if (!m) return null;
    term = text.slice(0, m.index).replace(/\*\*/g, '').trim();
    desc = text.slice(m.index + m[0].length).trim();
  }
  if (!term || !desc) return null;
  var key = term.replace(GLOSSARY_PLACEHOLDER_RE, '').trim();
  if (key.length < 2) return null;
  return { term: term, key: key, descHtml: applyInline(desc) };
}

function extractGlossary(preamble) {
  var glossary = [];
  var seenKeys = {};
  (preamble || []).forEach(function (block) {
    var lines = block.bodyLines || [];
    var i = 0;
    while (i < lines.length) {
      if (GLOSSARY_HEADING_RE.test(lines[i])) {
        i++;
        while (i < lines.length && lines[i].trim() === '') i++;
        while (i < lines.length && /^[*\-]\s+/.test(lines[i].trim())) {
          var itemText = lines[i].trim().replace(/^[*\-]\s+/, '');
          var entry = parseGlossaryItem(itemText);
          if (entry && !seenKeys[entry.key]) {
            seenKeys[entry.key] = true;
            glossary.push(entry);
          }
          i++;
        }
        continue;
      }
      i++;
    }
  });
  return glossary;
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
function parseMaterialItem(itemText) {
  var text = String(itemText).trim();
  if (!text) return null;
  var name, desc;
  var boldMatch = text.match(/^\*\*(.+?)\*\*\s*(.*)$/);
  if (boldMatch) {
    name = boldMatch[1].trim();
    var rest = boldMatch[2];
    var sepMatch = rest.match(GLOSSARY_SEP_RE);
    desc = sepMatch ? rest.slice(sepMatch.index + sepMatch[0].length).trim() : rest.trim();
  } else {
    var m = text.match(GLOSSARY_SEP_RE);
    if (!m) return null;
    name = text.slice(0, m.index).replace(/\*\*/g, '').trim();
    desc = text.slice(m.index + m[0].length).trim();
  }
  if (!name) return null;
  var url = null;
  var um = desc.match(MATERIAL_URL_RE);
  if (um) {
    url = um[0].replace(MATERIAL_URL_TAIL_RE, '');
    desc = (desc.slice(0, um.index) + desc.slice(um.index + um[0].length)).replace(/\s+/g, ' ').trim();
  }
  // key が2文字未満でも項目としては残す（本文ハイライトの対象から外れるだけ）
  var key = name.replace(GLOSSARY_PLACEHOLDER_RE, '').trim();
  return { name: name, key: key, url: url, descHtml: applyInline(desc) };
}

function extractMaterials(preamble) {
  var materials = [];
  var seenKeys = {};
  (preamble || []).forEach(function (block) {
    var lines = block.bodyLines || [];
    var i = 0;
    while (i < lines.length) {
      if (MATERIALS_LINE_RE.test(lines[i].trim())) {
        i++;
        while (i < lines.length && lines[i].trim() === '') i++;
        while (i < lines.length && /^[*\-]\s+/.test(lines[i].trim())) {
          var itemText = lines[i].trim().replace(/^[*\-]\s+/, '');
          var entry = parseMaterialItem(itemText);
          if (entry && !seenKeys[entry.key]) {
            seenKeys[entry.key] = true;
            materials.push(entry);
          }
          i++;
        }
        continue;
      }
      i++;
    }
  });
  return materials;
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
  var tag = null;
  var mTag = text.match(/【([^】]*)】\s*$/);
  if (mTag) {
    tag = mTag[1];
    text = text.slice(0, mTag.index).trim();
  }
  return { number: number, tag: tag, title: text.trim() };
}

export function parseProcedure(rawText) {
  var text = String(rawText).replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  var lines = text.split('\n');

  var docTitle = '';
  var blocks = [];
  var current = null;

  for (var i = 0; i < lines.length; i++) {
    var line = lines[i];
    if (/^##\s+/.test(line) && !/^###/.test(line)) {
      docTitle = line.replace(/^##\s+/, '').trim();
      continue;
    }
    if (/^###\s+/.test(line)) {
      current = { heading: line.replace(/^###\s+/, '').trim(), lines: [] };
      blocks.push(current);
      continue;
    }
    if (current) current.lines.push(line);
  }

  var preamble = [];
  var sections = [];
  var totalItems = 0;

  blocks.forEach(function (block) {
    var hasTable = block.lines.some(function (l) { return isTableLine(l); });

    if (!hasTable) {
      var bodyLines = block.lines.slice();
      while (bodyLines.length && bodyLines[0].trim() === '') bodyLines.shift();
      while (bodyLines.length && bodyLines[bodyLines.length - 1].trim() === '') bodyLines.pop();
      preamble.push({ heading: block.heading, bodyLines: bodyLines });
      return;
    }

    var meta = parseHeadingMeta(block.heading);
    var tableRowsRaw = [];
    var nonTableLines = [];
    var skippedHeader = false;
    var skippedSep = false;

    for (var j = 0; j < block.lines.length; j++) {
      var l2 = block.lines[j];
      if (isTableLine(l2)) {
        var cells = splitTableRow(l2);
        if (!skippedHeader) { skippedHeader = true; continue; }
        if (!skippedSep && isSeparatorRow(cells)) { skippedSep = true; continue; }
        tableRowsRaw.push(cells);
      } else if (l2.trim() !== '') {
        nonTableLines.push(l2.trim());
      }
    }

    var items = [];
    tableRowsRaw.forEach(function (cells) {
      if (cells.length === 0) return;
      var num = cells[0] || '';
      var step = cells.length > 1 ? cells[1] : '';
      var expected;
      if (cells.length <= 2) {
        expected = T.notSpecified;
      } else {
        expected = cells.slice(2).join(' ').trim();
        if (expected === '') expected = T.notSpecified;
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
      if (expected === '') expected = T.notSpecified;
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
      sections.push({ number: '1', tag: null, title: T.simpleFormatSectionTitle, note: '', items: fallbackItems });
    }
  }

  return {
    title: docTitle,
    preamble: preamble,
    sections: sections,
    glossary: extractGlossary(preamble),
    build: extractBuild(preamble),
    materials: extractMaterials(preamble),
    totalItems: totalItems,
    ok: totalItems > 0
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
