import { installI18n, T, P, S } from "./constants/i18n.js";

'use strict';

/* =========================================================================
   PARSER (pure functions — decode + parseProcedure)
   ========================================================================= */
/* ===== PARSER START ===== */

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function applyInline(raw) {
  var s = escapeHtml(raw);
  s = s.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/`(.+?)`/g, '<code>$1</code>');
  return s;
}

function stripMd(raw) {
  return String(raw).replace(/\*\*(.+?)\*\*/g, '$1').replace(/`(.+?)`/g, '$1');
}

function escapeRegExp(s) {
  return String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/* ---- glossary (用語定義) parsing ---- */
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
      if (/用語定義/.test(lines[i])) {
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

/* ---- ビルド指定（ビルド:）/ 配布物（配布物:）— どちらも任意項目 ----
   前置きの本文行から拾う。行が無ければ build は {mode:'none'}、materials は [] になり、
   従来の手順書は一切挙動が変わらない。 */
var BUILD_LINE_RE = /^ビルド\s*[:：]\s*(.*)$/;
var BUILD_INPUT_RE = /^(記入|入力)/;
var BUILD_HINT_RE = /[（(]([^）)]*)[）)]/;
var MATERIALS_LINE_RE = /^配布物\s*[:：]/;
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

function highlightGlossaryHtml(html, glossary, materials) {
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

function parseProcedure(rawText) {
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

function decodeAuto(bytes) {
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

/* ===== PARSER END ===== */

/* =========================================================================
   演出用データ（褒め言葉・称号）は constants/phrases.{ja,en}.js へ移設。P.PRAISE_POOL 等で参照する。
   ========================================================================= */
function classifyPace(avgSec) {
  if (avgSec < 20) return 'fast';
  if (avgSec < 90) return 'steady';
  return 'careful';
}

function timePraisePoolFor(pace) {
  if (pace === 'fast') return P.TIME_PRAISE_FAST;
  if (pace === 'steady') return P.TIME_PRAISE_STEADY;
  return P.TIME_PRAISE_CAREFUL;
}

function timeBonusFor(pace) {
  if (pace === 'fast') return 300;
  if (pace === 'steady') return 200;
  return 250;
}

function formatDuration(ms) {
  var totalSec = Math.max(0, Math.round((ms || 0) / 1000));
  var h = Math.floor(totalSec / 3600);
  var m = Math.floor((totalSec % 3600) / 60);
  var s = totalSec % 60;
  var pad2 = function (n) { return (n < 10 ? '0' : '') + n; };
  if (h > 0) return h + ':' + pad2(m) + ':' + pad2(s);
  return m + ':' + pad2(s);
}

function pickRandom(arr) { return arr[Math.floor(Math.random() * arr.length)]; }

function shuffleArray(arr) {
  var a = arr.slice();
  for (var i = a.length - 1; i > 0; i--) {
    var j = Math.floor(Math.random() * (i + 1));
    var tmp = a[i]; a[i] = a[j]; a[j] = tmp;
  }
  return a;
}

/* 重複なしで最大 n 個を選ぶ（arr.length < n の場合は足りる分だけ返す） */
function pickDistinct(arr, n) {
  return shuffleArray(arr).slice(0, Math.min(n, arr.length));
}

function getPraiseForCombo(combo) {
  var idx = Math.min(combo - 1, P.PRAISE_POOL.length - 1);
  var jitter = Math.floor(Math.random() * 3);
  idx = Math.max(0, idx - jitter);
  return P.PRAISE_POOL[idx];
}

/* =========================================================================
   状態管理
   ========================================================================= */
var STORAGE_KEY = 'testProcedureRunner.session.v1';

var state = {
  docTitle: '',
  preamble: [],
  sections: [],
  glossary: [],
  materials: [],       // { name, key, url, descHtml }（手順書に配布物が無ければ空）
  build: { mode: 'none' }, // 手順書から毎回導出するので保存しない
  buildEntered: '',    // 記入モードでテスターが入力したビルド番号（保存・復元する）
  flatItems: [],       // { sectionIndex, itemIndexInSection, number, stepHtml, expectedHtml, stepRaw, expectedRaw }
  results: [],         // { status: 'ok'|'ng'|null, comment: '', timestamp: null }
  pointer: 0,
  testerName: '',
  startedAt: null,
  score: 0,
  combo: 0,
  transitioning: false, // runtime-only guard against double OK/NG during the advance animation; not persisted (see saveSession)
  stepEnteredAt: null // runtime-only: Date.now() when the current step was rendered; not persisted (see saveSession)
};

function buildFlatItems() {
  var flat = [];
  state.sections.forEach(function (sec, sIdx) {
    sec.items.forEach(function (it, iIdx) {
      flat.push({
        sectionIndex: sIdx,
        itemIndexInSection: iIdx,
        number: it.number,
        stepHtml: it.stepHtml,
        expectedHtml: it.expectedHtml,
        stepRaw: it.stepRaw,
        expectedRaw: it.expectedRaw
      });
    });
  });
  state.flatItems = flat;
}

function sectionRange(sIdx) {
  var start = 0;
  for (var i = 0; i < sIdx; i++) start += state.sections[i].items.length;
  var end = start + state.sections[sIdx].items.length;
  return { start: start, end: end };
}

function saveSession() {
  try {
    var data = {
      rawText: state.rawText,
      testerName: state.testerName,
      buildEntered: state.buildEntered,
      results: state.results,
      pointer: state.pointer,
      startedAt: state.startedAt,
      score: state.score,
      combo: state.combo
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch (e) { /* ignore quota errors etc. */ }
}

function clearSession() {
  try { localStorage.removeItem(STORAGE_KEY); } catch (e) { /* ignore */ }
}

function loadSessionRaw() {
  try {
    var raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw);
  } catch (e) { return null; }
}

/* =========================================================================
   画面遷移
   ========================================================================= */
var SCREENS = ['screen-start', 'screen-step', 'screen-result'];
function showScreen(id) {
  SCREENS.forEach(function (s) {
    document.getElementById(s).hidden = (s !== id);
  });
}

/* =========================================================================
   スタート画面
   ========================================================================= */
var confirmContext = null;

// 確認モーダルを開くたびに引き直す、テスター名の既定値（placeholder に出している値）。
// boot() 内で installI18n 直後・init() より前に1回だけ代入する（ページロードあたり1回という性質は変えない）
var defaultTesterName;

// 入力が空ならプレースホルダに出している名前をそのまま採用する
function currentTesterName() {
  var typed = el('tester-name-input').value.trim();
  return typed !== '' ? typed : defaultTesterName;
}

function el(id) { return document.getElementById(id); }

function renderGlossaryHtml(preamble) {
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
function renderProcedureLink(result) {
  el('confirm-preview-link').textContent =
    '🔍 ' + (result.title || T.untitled) +
    T.procedureLinkSummary(result.sections.length, result.totalItems);
}

// 手順の内訳。確認画面には出さず、リンクから開くモーダルに描く
function renderPreview(result) {
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

/* =========================================================================
   試験情報（ビルド / 試験対象OS / 配布物）
   ------------------------------------------------------------------------
   手順書に `ビルド:` も `配布物:` も無く、節タグも無ければブロックごと隠れるので、
   従来の手順書では開始画面の見た目が変わらない。
   OSは「表示するだけ」で、テスターに選ばせない。
   ========================================================================= */

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
function materialPopupActionsHtml(material) {
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

function materialsSectionHtml(materials) {
  if (!materials || materials.length === 0) return '';
  return '<div class="glossary-block"><h4>📎 ' + T.providedFilesHeading + '</h4>' +
    materials.map(function (m) { return materialRowHtml(m); }).join('') + '</div>';
}

function handleMaterialCopyClick(e) {
  var btn = (e.target && e.target.closest) ? e.target.closest('.material-copy-btn') : null;
  if (!btn) return;
  var url = btn.getAttribute('data-url');
  if (url) copyToClipboard(url, T.linkCopiedToast);
}

function clearBuildInputWarning() {
  var inp = el('build-input');
  if (inp) inp.classList.remove('shake-warn');
}

function triggerBuildInputWarning() {
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
function ensureBuildEntered(result) {
  var build = (result && result.build) ? result.build : { mode: 'none' };
  if (build.mode !== 'input') return true;
  if (el('build-input').value.trim() !== '') return true;
  triggerBuildInputWarning();
  return false;
}

function renderTestInfo(result) {
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

/* =========================================================================
   開始前の確認モーダル
   ------------------------------------------------------------------------
   試験情報（テスター名/ビルド/OS/配布物）をひとまとめにして開始画面の上に出す。
   「準備完了！」を押すまで実際のセッションは始めないので、閉じても何も残らない。
   旧バージョンにあったURL復元時専用のテスター名入力モーダルもここに統合済み。
   ========================================================================= */
function openConfirmModal(opts) {
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

function closeConfirmModal() {
  el('modal-confirm').hidden = true;
  confirmContext = null;
}

// 必須のビルドが空の間は「準備完了！」を押せない見た目にする（NGコメント必須と同じ扱い）
function updateConfirmStartBtnState() {
  var build = (confirmContext && confirmContext.result && confirmContext.result.build)
    ? confirmContext.result.build : { mode: 'none' };
  el('confirm-start-btn').disabled = (build.mode === 'input' && el('build-input').value.trim() === '');
}

// 押せない状態でEnterを叩かれたら、黙って無視せず理由を知らせる
function submitConfirmFromKeyboard() {
  if (!el('confirm-start-btn').disabled) el('confirm-start-btn').click();
  else triggerBuildInputWarning();
}

// 再開時に確認画面を挟むのは、開始に必要な情報が欠けているときだけ。
// テスター名は任意項目なので、ローカル保存・進捗ファイルでは空でも聞き直さない
// （本人が意図的に空にしている）。進捗URLだけは共有形式が名前を持たないため聞く。
function resumeNeedsConfirm(saved, result, requireTesterName) {
  if (requireTesterName && (!saved.testerName || String(saved.testerName).trim() === '')) return true;
  var b = (result && result.build) ? result.build : { mode: 'none' };
  if (b.mode === 'input' && String(saved.buildEntered || '').trim() === '') return true;
  return false;
}

// 「続ける」か「開始する」かは、記録済みの結果が1件でもあるかどうかで決める
// （記録が一切無ければ試験手順だけの共有URLと同じ状態なので、開始する扱いにする）
function resumeButtonLabel(saved) {
  var hasAnyResult = (saved.results || []).some(function (r) { return r && r.status; });
  return hasAnyResult ? '▶ ' + T.continueLabel : '✅ ' + T.readyLabel;
}

// 表示・書き出しに使うビルド番号。固定値はそのまま、記入モードは入力値。無ければ空文字
function currentBuildLabel() {
  var b = state.build || { mode: 'none' };
  if (b.mode === 'fixed') return String(b.value || '').trim();
  if (b.mode === 'input') return String(state.buildEntered || '').trim();
  return '';
}

function renderParseError(reason) {
  var box = el('error-box');
  box.innerHTML =
    '<div class="err-title">⚠️ ' + T.couldNotReadProcedure + '</div>' +
    '<div>' + escapeHtml(reason) + '</div>' +
    T.parseErrorHint;
  box.hidden = false;
}

function handleLoadedText(text) {
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
}

function decodeArrayBufferAuto(buf) {
  return decodeAuto(new Uint8Array(buf));
}

function wireStartScreen() {
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

function openProcedureModal() { el('modal-procedure').hidden = false; }
function closeProcedureModal() { el('modal-procedure').hidden = true; }

function wireConfirmScreen() {
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

/* ---------- 進捗ファイルの書き出し / 読み込み ---------- */
function buildProgressExportObject(source) {
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

function downloadProgressJson(dataObj) {
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

function exportProgressFromState() {
  if (!state.rawText) { showToast(T.noProgressToExport); return; }
  var fname = downloadProgressJson(buildProgressExportObject(state));
  showToast(T.progressExported(fname));
}

function handleProgressFile(file) {
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
    var result = parseProcedure(progress.rawText);
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

// 進捗ファイル / 進捗URL 共通: 進捗オブジェクトの形式検証と旧形式の正規化。
// 不正なら toast を出して null を返す。
function normalizeProgressObject(obj) {
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

/* ---------- 進捗URL（#state=<lz-string圧縮JSON>）での共有 / 復元 ----------
   サーバーには一切送らず、window.location.hash だけで完結するステートレス共有。
   ペイロードは進捗ファイル(.json)と同じ buildProgressExportObject の形。 */
var STATE_HASH_PREFIX = '#state=';

function hasLzString() {
  return typeof LZString !== 'undefined' && LZString && typeof LZString.compressToEncodedURIComponent === 'function';
}

function buildShareUrl(dataObj) {
  var packed = LZString.compressToEncodedURIComponent(JSON.stringify(dataObj));
  return window.location.origin + window.location.pathname + STATE_HASH_PREFIX + packed;
}

// 共有URLは長いので、YouTrack等のMarkdown対応先に貼れるよう [タイトル](URL) 形式でコピーする。
// lz-string の出力は英数字と +-$ のみで括弧を含まないので、URL側のエスケープは不要。
function escapeMdLinkText(s) {
  return String(s).replace(/([\\\[\]])/g, '\\$1');
}
function toMarkdownLink(title, url) {
  return '[' + escapeMdLinkText(title) + '](' + url + ')';
}
function docTitleOf(rawText) {
  var r = parseProcedure(rawText);
  return (r && r.title) ? r.title : T.untitledBare;
}
// 結果共有のリンク文言。source.results は旧localStorage形式だと空/短いことがあるため、
// その場合は同じ1回のparseProcedureからtotalItemsを補完する（2回parseしない）。
function resultLinkTitle(source) {
  var parsed = parseProcedure(source.rawText);
  var title = (parsed && parsed.title) ? parsed.title : T.untitledBare;
  var total = (source.results || []).length;
  if (total === 0) total = parsed.totalItems;
  var answered = (source.results || []).filter(function (r) { return r && r.status; }).length;
  return T.resultLinkTitleText(title, answered, total, source.testerName);
}
// rawOnly=true（Shift+クリック）なら Markdown リンクではなく URL だけをコピーする（ブラウザに直接貼る用）
function copyShareUrl(source, rawOnly) {
  if (!source || !source.rawText) { showToast(T.noProgressToShare); return; }
  if (!hasLzString()) { showToast(T.lzStringUnavailable); return; }
  var url = buildShareUrl(buildProgressExportObject(source));
  if (rawOnly) { copyToClipboard(url, T.resultUrlCopiedRawOnly); return; }
  copyToClipboard(toMarkdownLink(resultLinkTitle(source), url), T.resultUrlCopiedMarkdown);
}

// 試験手順だけを共有する（記録なし・テスター名なし）。開いた人は名前入力モーダルを経て最初から始める。
function buildProcedureShareObject(rawText) {
  return { rawText: rawText, testerName: '', buildEntered: '', results: [], pointer: 0, startedAt: null, score: 0, combo: 0 };
}
// rawOnly=true（Shift+クリック）なら Markdown リンクではなく URL だけをコピーする
function copyProcedureShareUrl(rawText, rawOnly) {
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

// 復元し終えたらハッシュをURLから外す（リロードで同じ状態に巻き戻らないように。履歴は増やさない）。
function clearStateHash() {
  try { history.replaceState(null, '', window.location.pathname + window.location.search); } catch (e) { /* ignore */ }
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
function restoreFromHash() {
  var obj = readProgressFromHash();
  if (!obj) return false;
  clearStateHash();
  var progress = normalizeProgressObject(obj);
  if (!progress) return false;
  var result = parseProcedure(progress.rawText);
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

// 互換: 旧バージョンの「保留」ステータスは未実施(null)として正規化する
function normalizeStatusCompat(status) {
  return status === 'hold' ? null : status;
}

function checkResumeAvailable() {
  var saved = loadSessionRaw();
  if (!saved || !saved.rawText) return;
  var result = parseProcedure(saved.rawText);
  if (!result.ok) return;
  el('resume-box').hidden = false;
  var answered = (saved.results || []).filter(function (r) { return r && r.status; }).length;
  var total = result.totalItems;
  var when = '';
  try { when = new Date(saved.startedAt).toLocaleString(T.dateLocale); } catch (e) { when = ''; }
  el('resume-info').textContent = T.resumeInfoText(result.title || T.untitledBare, answered, total, saved.testerName, when);
}

function startNewSession(result, rawText) {
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

function resumeSession(saved) {
  var result = parseProcedure(saved.rawText);
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

/* =========================================================================
   ステップ画面
   ========================================================================= */
function setupGlossaryFab() {
  var fab = el('glossary-fab-btn');
  var hasPreamble = !!(state.preamble && state.preamble.length > 0);
  var hasMaterials = !!(state.materials && state.materials.length > 0);
  fab.hidden = !(hasPreamble || hasMaterials);
}

function currentItem() { return state.flatItems[state.pointer]; }
function currentSection() { return state.sections[currentItem().sectionIndex]; }

function countStatusesInSection(sIdx) {
  var range = sectionRange(sIdx);
  var counts = { ok: 0, ng: 0, total: range.end - range.start, unanswered: 0 };
  for (var i = range.start; i < range.end; i++) {
    var r = state.results[i];
    if (r && r.status === 'ok') counts.ok++;
    else if (r && r.status === 'ng') counts.ng++;
    else counts.unanswered++;
  }
  return counts;
}

function computeSectionDurationMs(sIdx) {
  var range = sectionRange(sIdx);
  var total = 0;
  for (var i = range.start; i < range.end; i++) {
    var r = state.results[i];
    if (r && r.durationMs) total += r.durationMs;
  }
  return total;
}

function computeTotalDurationMs() {
  var total = 0;
  state.results.forEach(function (r) { if (r && r.durationMs) total += r.durationMs; });
  return total;
}

/* ---------- ステップ経過時間のライブタイマー（ステップ画面にいる間だけ動かす） ---------- */
var stepTimerInterval = null;
function stopStepTimer() {
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

function renderStep(animate) {
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

function hideActionPanels() {
  el('ng-panel').hidden = true;
  el('action-row').hidden = false;
  el('ng-comment').value = '';
  el('ng-inline-warn').hidden = true;
  el('ng-comment').classList.remove('shake-warn');
  updateNgConfirmBtnState();
}

function openCommentPanel() {
  el('action-row').hidden = true;
  el('ng-panel').hidden = false;
  updateNgConfirmBtnState();
  el('ng-comment').focus();
}

/* ---------- NG確認オーバーレイ（引き止め） ---------- */
/* NG_DETERRENT_POOL は constants/phrases.{ja,en}.js へ移設。P.NG_DETERRENT_POOL で参照する */

// 現在の項目が既にNGとして記録済みか（戻ってきた場合など）
function currentItemIsNg() {
  var r = state.results[state.pointer];
  return !!(r && r.status === 'ng');
}

// NGボタン/Nキーの入口。記録済みNGの再編集なら引き止め（オーバーレイ・逃げ演出）を挟まずコメント編集へ直行する
function startNgFlow() {
  if (currentItemIsNg()) { openCommentPanel(); return; }
  openNgConfirmOverlay();
}

function openNgConfirmOverlay() {
  var item = currentItem();
  el('ng-confirm-deterrent').textContent = pickRandom(P.NG_DETERRENT_POOL);
  el('ng-confirm-expected').innerHTML = item.expectedHtml;
  el('overlay-ng-confirm').hidden = false;
}

function closeNgConfirmOverlay() {
  el('overlay-ng-confirm').hidden = true;
}

function wireNgConfirmOverlay() {
  el('ng-confirm-back-btn').addEventListener('click', closeNgConfirmOverlay);
  el('ng-confirm-proceed-btn').addEventListener('click', function () {
    closeNgConfirmOverlay();
    openCommentPanel();
  });
}

/* ---------- NGコメント必須化 ---------- */
function updateNgConfirmBtnState() {
  var val = el('ng-comment').value.trim();
  el('ng-confirm-btn').disabled = (val === '');
}

function triggerNgCommentRequiredWarning() {
  var ta = el('ng-comment');
  ta.classList.remove('shake-warn');
  void ta.offsetWidth;
  ta.classList.add('shake-warn');
  el('ng-inline-warn').hidden = false;
}

function closeCommentPanels() {
  el('ng-panel').hidden = true;
  el('action-row').hidden = false;
  el('ng-inline-warn').hidden = true;
  el('ng-comment').classList.remove('shake-warn');
}

function recordResult(status, comment) {
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
function isLastSection() {
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

function goBack() {
  if (state.transitioning) return; // guard: ignore back navigation while advancing to the next item
  if (state.pointer === 0) return;
  hideActionPanels();
  state.pointer--;
  saveSession();
  renderStep(false);
}

/* ---------- NGボタン回避（マウスが止まったら逃げる。最大2回/項目） ---------- */
/* NG_DODGE_PHRASES は constants/phrases.{ja,en}.js へ移設。P.NG_DODGE_PHRASES で参照する */
var NG_DODGE_LIMIT = 2;
var ngDodgeCount = 0;
var ngDodgeHoverTimer = null;
var ngDodgeLock = false;
var ngDodgeCooldownUntil = 0;

function resetNgDodgeState() {
  ngDodgeCount = 0;
  ngDodgeLock = false;
  ngDodgeCooldownUntil = 0;
  clearTimeout(ngDodgeHoverTimer);
  ngDodgeHoverTimer = null;
  var row = el('action-row');
  var okBtn = el('ok-btn');
  var ngBtnEl = el('ng-btn');
  if (row && okBtn && ngBtnEl) {
    // always restore original order: OK left, NG right
    if (okBtn.nextElementSibling !== ngBtnEl) {
      row.insertBefore(okBtn, ngBtnEl);
    }
    okBtn.style.transition = '';
    okBtn.style.transform = '';
    ngBtnEl.style.transition = '';
    ngBtnEl.style.transform = '';
    ngBtnEl.classList.remove('ng-hop');
    var oldBubble = ngBtnEl.querySelector('.ng-bubble');
    if (oldBubble && oldBubble.parentNode) oldBubble.parentNode.removeChild(oldBubble);
  }
}

function ngHoverCapable() {
  return !(window.matchMedia && window.matchMedia('(hover: none)').matches);
}

function showNgDodgeBubble(ngBtnEl) {
  var old = ngBtnEl.querySelector('.ng-bubble');
  if (old && old.parentNode) old.parentNode.removeChild(old);
  var bubble = document.createElement('span');
  bubble.className = 'ng-bubble';
  bubble.textContent = pickRandom(P.NG_DODGE_PHRASES);
  ngBtnEl.appendChild(bubble);
  void bubble.offsetWidth;
  bubble.classList.add('show');
  setTimeout(function () {
    if (bubble.parentNode) bubble.parentNode.removeChild(bubble);
  }, 900);
}

function dodgeNgButton() {
  if (state.transitioning) return;
  if (currentItemIsNg()) return; // 既にNG記録済みの項目では逃げない
  if (ngDodgeLock) return;
  if (ngDodgeCount >= NG_DODGE_LIMIT) return;
  if (Date.now() < ngDodgeCooldownUntil) return;

  var row = el('action-row');
  var okBtn = el('ok-btn');
  var ngBtnEl = el('ng-btn');
  if (!row || !okBtn || !ngBtnEl) return;

  ngDodgeLock = true;
  ngDodgeCount++;

  if (reducedMotionOS) {
    // functional swap only, no motion
    if (okBtn.nextElementSibling === ngBtnEl) {
      row.insertBefore(ngBtnEl, okBtn);
    } else {
      row.insertBefore(okBtn, ngBtnEl);
    }
    showNgDodgeBubble(ngBtnEl);
    ngDodgeCooldownUntil = Date.now() + 500;
    ngDodgeLock = false;
    return;
  }

  var firstOk = okBtn.getBoundingClientRect();
  var firstNg = ngBtnEl.getBoundingClientRect();

  if (okBtn.nextElementSibling === ngBtnEl) {
    row.insertBefore(ngBtnEl, okBtn);
  } else {
    row.insertBefore(okBtn, ngBtnEl);
  }

  var lastOk = okBtn.getBoundingClientRect();
  var lastNg = ngBtnEl.getBoundingClientRect();
  var dxOk = firstOk.left - lastOk.left;
  var dxNg = firstNg.left - lastNg.left;

  okBtn.style.transition = 'none';
  ngBtnEl.style.transition = 'none';
  okBtn.style.transform = 'translateX(' + dxOk + 'px)';
  ngBtnEl.style.transform = 'translateX(' + dxNg + 'px)';
  void row.offsetWidth;
  okBtn.style.transition = 'transform 320ms cubic-bezier(.2,.9,.3,1.2)';
  ngBtnEl.style.transition = 'transform 320ms cubic-bezier(.2,.9,.3,1.2)';
  okBtn.style.transform = '';
  ngBtnEl.style.transform = '';

  ngBtnEl.classList.remove('ng-hop');
  void ngBtnEl.offsetWidth;
  ngBtnEl.classList.add('ng-hop');
  showNgDodgeBubble(ngBtnEl);

  ngDodgeCooldownUntil = Date.now() + 500;
  setTimeout(function () {
    okBtn.style.transition = '';
    ngBtnEl.style.transition = '';
    ngBtnEl.classList.remove('ng-hop');
    ngDodgeLock = false;
  }, 340);
}

var ngLastPointerType = 'mouse';

function wireNgDodge() {
  var ngBtnEl = el('ng-btn');
  if (!ngBtnEl) return;
  // pointerenter (not the trigger itself) just records whether this hover came from touch,
  // since plain MouseEvents from mouseenter carry no pointerType.
  ngBtnEl.addEventListener('pointerenter', function (e) {
    ngLastPointerType = e.pointerType || 'mouse';
  });
  ngBtnEl.addEventListener('mouseenter', function () {
    if (ngLastPointerType === 'touch') return;
    if (!ngHoverCapable()) return;
    if (state.transitioning) return;
    if (currentItemIsNg()) return; // 既にNG記録済みの項目では逃げない
    if (Date.now() < ngDodgeCooldownUntil) return;
    clearTimeout(ngDodgeHoverTimer);
    ngDodgeHoverTimer = setTimeout(function () {
      dodgeNgButton();
    }, 380);
  });
  ngBtnEl.addEventListener('mouseleave', function () {
    clearTimeout(ngDodgeHoverTimer);
  });
}

function wireStepScreen() {
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

function handleTermClick(e) {
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

function openGlossaryModal() {
  el('glossary-modal-content').innerHTML = renderGlossaryHtml(state.preamble) + materialsSectionHtml(state.materials);
  el('modal-glossary').hidden = false;
}
function closeGlossaryModal() { el('modal-glossary').hidden = true; }

/* ---------- 用語 / 配布物ポップアップ（同じカードを使い回す） ---------- */
function openTermPopup(entry) {
  el('term-popup-header').textContent = '📖 ' + T.termExplanationHeading;
  el('term-popup-title').textContent = entry.term;
  var desc = el('term-popup-desc');
  desc.classList.remove('term-popup-desc-material');
  desc.innerHTML = entry.descHtml;
  el('overlay-term').hidden = false;
  spawnConfetti(24, 0.3);
}

function openMaterialPopup(material) {
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
function closeTermPopup() { el('overlay-term').hidden = true; }

function wireTermPopup() {
  el('term-popup-ok-btn').addEventListener('click', closeTermPopup);
  el('overlay-term').addEventListener('click', function (e) {
    if (e.target === el('overlay-term')) closeTermPopup();
  });
}

/* =========================================================================
   演出: OK セレブレーション（紙吹雪 / フラッシュ / 褒め言葉 / スコア）
   ========================================================================= */
var confettiCanvas;
var confettiCtx;
var particles = [];
var confettiRunning = false;
var reducedMotionOS = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

function resizeConfettiCanvas() {
  confettiCanvas.width = window.innerWidth;
  confettiCanvas.height = window.innerHeight;
}

// confettiCanvas は DOM 要素に依存するため boot() 経由の init() から呼ぶ（トップレベル副作用にしない）
function initConfetti() {
  confettiCanvas = el('confetti-canvas');
  confettiCtx = confettiCanvas.getContext('2d');
  window.addEventListener('resize', resizeConfettiCanvas);
  resizeConfettiCanvas();
}

var CONFETTI_COLORS = ['#f2c14e', '#ffe08a', '#8a5cf6', '#b48cff', '#ffffff', '#5be08a'];

function spawnConfetti(count, originYRatio) {
  if (reducedMotionOS) count = Math.min(count, 12);
  var w = confettiCanvas.width, h = confettiCanvas.height;
  for (var i = 0; i < count; i++) {
    particles.push({
      x: Math.random() * w,
      y: h * (originYRatio || 0.15) * Math.random(),
      vx: (Math.random() - 0.5) * 4,
      vy: 2 + Math.random() * 4,
      size: 5 + Math.random() * 7,
      rot: Math.random() * Math.PI * 2,
      rotSpeed: (Math.random() - 0.5) * 0.3,
      color: pickRandom(CONFETTI_COLORS),
      shape: Math.random() < 0.5 ? 'rect' : 'circle',
      life: 0,
      maxLife: 90 + Math.random() * 60
    });
  }
  if (!confettiRunning) { confettiRunning = true; requestAnimationFrame(confettiLoop); }
}

function confettiLoop() {
  confettiCtx.clearRect(0, 0, confettiCanvas.width, confettiCanvas.height);
  var alive = [];
  for (var i = 0; i < particles.length; i++) {
    var p = particles[i];
    p.x += p.vx;
    p.y += p.vy;
    p.vy += 0.03;
    p.rot += p.rotSpeed;
    p.life++;
    if (p.y < confettiCanvas.height + 20 && p.life < p.maxLife) {
      alive.push(p);
      confettiCtx.save();
      confettiCtx.translate(p.x, p.y);
      confettiCtx.rotate(p.rot);
      confettiCtx.fillStyle = p.color;
      confettiCtx.globalAlpha = Math.max(0, 1 - p.life / p.maxLife);
      if (p.shape === 'rect') {
        confettiCtx.fillRect(-p.size / 2, -p.size / 4, p.size, p.size / 2);
      } else {
        confettiCtx.beginPath();
        confettiCtx.arc(0, 0, p.size / 2.4, 0, Math.PI * 2);
        confettiCtx.fill();
      }
      confettiCtx.restore();
    }
  }
  particles = alive;
  if (particles.length > 0) {
    requestAnimationFrame(confettiLoop);
  } else {
    confettiRunning = false;
    confettiCtx.clearRect(0, 0, confettiCanvas.width, confettiCanvas.height);
  }
}

function triggerFlash() {
  if (reducedMotionOS) return;
  var fx = el('flash-effect');
  fx.classList.remove('flash');
  void fx.offsetWidth;
  fx.classList.add('flash');
}

function showPraisePop(text) {
  var node = el('praise-pop');
  node.textContent = text;
  node.classList.remove('show');
  void node.offsetWidth;
  node.classList.add('show');

  var backdrop = el('praise-backdrop');
  if (backdrop) {
    backdrop.classList.remove('show');
    void backdrop.offsetWidth;
    backdrop.classList.add('show');
  }
}

function showScoreFloat(text) {
  var node = el('score-float');
  node.textContent = text;
  node.classList.remove('show');
  void node.offsetWidth;
  node.classList.add('show');
}

var toastTimer = null;
function showToast(msg) {
  var node = el('toast');
  node.textContent = msg;
  node.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(function () { node.classList.remove('show'); }, 2600);
}

function fireOkCelebration(gained, combo) {
  var count = 80 + Math.floor(Math.random() * 70);
  spawnConfetti(count, 0.1);
  triggerFlash();
  showPraisePop(getPraiseForCombo(combo));
  var scoreText = '+' + gained + ' pt' + (combo >= 2 ? '　COMBO x' + combo + '!' : '');
  showScoreFloat(scoreText);
}

/* =========================================================================
   節完了 / グランドフィナーレ 演出
   ========================================================================= */
function showSectionCompleteOverlay() {
  stopStepTimer();
  var item = currentItem();
  var sIdx = item.sectionIndex;
  var counts = countStatusesInSection(sIdx);

  spawnConfetti(150, 0.05);
  el('sc-heading').textContent = T.sectionCompleteHeading(sIdx + 1);
  el('sc-title').textContent = T.sectionTitleReveal(pickRandom(P.SECTION_TITLE_POOL));
  el('sc-ok').textContent = counts.ok;
  el('sc-ng').textContent = counts.ng;
  var scPraises = pickDistinct(P.SECTION_PRAISE_POOL, 3);
  el('sc-praise1').textContent = scPraises[0] || '';
  el('sc-praise2').textContent = scPraises[1] || '';
  el('sc-praise3').textContent = scPraises[2] || '';

  var sectionDurationMs = computeSectionDurationMs(sIdx);
  var itemCountForPace = counts.total || 1;
  var avgSec = (sectionDurationMs / itemCountForPace) / 1000;
  var pace = classifyPace(avgSec);
  var bonus = timeBonusFor(pace);
  state.score += bonus;
  el('sc-duration').textContent = formatDuration(sectionDurationMs);
  el('sc-time-bonus').textContent = '+' + bonus + ' pt';
  el('sc-time-praise').textContent = pickRandom(timePraisePoolFor(pace));
  saveSession();

  var lastSection = isLastSection();
  el('sc-next-btn').textContent = lastSection ? T.toFinaleLabel : T.nextSectionLabel;
  el('overlay-section-complete').hidden = false;
  currentOverlayPrimaryAction = function () {
    el('overlay-section-complete').hidden = true;
    if (lastSection) {
      showFinaleOverlay();
    } else {
      state.pointer++;
      saveSession();
      renderStep(true); // re-enables ok/ng/back buttons
    }
    state.transitioning = false; // release the guard set in recordResult() now that we've fully moved past this item
  };
}

// 業務連絡（チャット/メール貼付用のプレーンテキスト報告文）
function buildBusinessReport() {
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

function computeOverallStats() {
  var total = state.results.length;
  var ok = 0, ng = 0, unanswered = 0;
  state.results.forEach(function (r) {
    if (r && r.status === 'ok') ok++;
    else if (r && r.status === 'ng') ng++;
    else unanswered++;
  });
  return { total: total, ok: ok, ng: ng, unanswered: unanswered };
}

function computeRank(stats) {
  if (stats.total > 0 && stats.ok === stats.total) return T.rankFlawless;
  var rate = stats.total > 0 ? (stats.ok / stats.total) * 100 : 0;
  if (rate >= 90) return T.rankS;
  if (rate >= 75) return T.rankA;
  if (rate >= 50) return T.rankB;
  return T.rankC;
}

function showFinaleOverlay() {
  stopStepTimer();
  spawnConfetti(150, 0.02);
  setTimeout(function () { spawnConfetti(120, 0.02); }, 300);
  setTimeout(function () { spawnConfetti(120, 0.02); }, 650);
  var stats = computeOverallStats();
  var rate = stats.total > 0 ? Math.round((stats.ok / stats.total) * 100) : 0;
  el('finale-rank').textContent = computeRank(stats);
  el('finale-total').textContent = stats.total;
  el('finale-ok').textContent = stats.ok;
  el('finale-ng').textContent = stats.ng;
  el('finale-rate').textContent = rate + '%';
  var finalePraises = pickDistinct(P.FINALE_PRAISE_POOL, 3);
  el('finale-praise1').textContent = finalePraises[0] || '';
  el('finale-praise2').textContent = finalePraises[1] || '';
  el('finale-praise3').textContent = finalePraises[2] || '';
  var totalDurationMs = computeTotalDurationMs();
  var finaleAvgSec = stats.total > 0 ? (totalDurationMs / stats.total) / 1000 : 0;
  var finalePace = classifyPace(finaleAvgSec);
  el('finale-duration').textContent = formatDuration(totalDurationMs);
  el('finale-time-praise').textContent = pickRandom(timePraisePoolFor(finalePace));
  el('overlay-finale').hidden = false;
  currentOverlayPrimaryAction = function () {
    el('overlay-finale').hidden = true;
    goToResultScreen();
  };
}

var currentOverlayPrimaryAction = null;

function wireOverlays() {
  el('sc-next-btn').addEventListener('click', function () { if (currentOverlayPrimaryAction) currentOverlayPrimaryAction(); });
  el('finale-result-btn').addEventListener('click', function () { if (currentOverlayPrimaryAction) currentOverlayPrimaryAction(); });
  wireNgConfirmOverlay();
}

/* =========================================================================
   結果画面
   ========================================================================= */
function goToResultScreen() {
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

function statusLabel(status) {
  if (status === 'ok') return 'OK';
  if (status === 'ng') return 'NG';
  return T.statusNotDone;
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

function renderResultScreen() {
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

function computeMaxCombo() {
  var max = 0, cur = 0;
  state.results.forEach(function (r) {
    if (r && r.status === 'ok') { cur++; max = Math.max(max, cur); }
    else { cur = 0; }
  });
  return max;
}

function sectionLabelFor(sIdx) {
  var sec = state.sections[sIdx];
  var label = T.sectionNumberTitle(sIdx + 1, sec.title || T.untitled);
  if (sec.tag) label += '【' + sec.tag + '】';
  return label;
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

/* ---------- エクスポート ---------- */
function buildExportRows() {
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
function toMarkdownTable(rows) {
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
function exportHeaderCells(buildLabel) {
  // 共有配列をそのまま返すと呼び出し側の改変が T に伝播するので必ずコピーする
  var cells = T.exportHeaderCellsBase.slice();
  return buildLabel ? [T.buildLabel].concat(cells) : cells;
}

function exportRowCells(r, buildLabel) {
  var cells = [r.number, r.section, r.step, r.expected, r.status, r.comment, r.time, r.duration];
  return buildLabel ? [buildLabel].concat(cells) : cells;
}

function toTsv(rows) {
  var esc = function (s) { return String(s).replace(/\t/g, ' '); };
  var buildLabel = currentBuildLabel();
  var header = exportHeaderCells(buildLabel).join('\t');
  var body = rows.map(function (r) {
    return exportRowCells(r, buildLabel).map(esc).join('\t');
  }).join('\n');
  return header + '\n' + body;
}

function toCsv(rows) {
  var esc = function (s) {
    var v = String(s);
    if (/[",\n]/.test(v)) v = '"' + v.replace(/"/g, '""') + '"';
    return v;
  };
  var buildLabel = currentBuildLabel();
  var header = exportHeaderCells(buildLabel).map(esc).join(',');
  var body = rows.map(function (r) {
    return exportRowCells(r, buildLabel).map(esc).join(',');
  }).join('\n');
  return header + '\n' + body;
}

function copyToClipboard(text, successMsg) {
  var msg = successMsg || T.defaultCopiedMsg;
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(function () {
      flashToast(msg);
    }).catch(function () { fallbackCopy(text, msg); });
  } else {
    fallbackCopy(text, msg);
  }
}

function fallbackCopy(text, successMsg) {
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

function flashToast(msg) {
  showScoreFloat(msg);
}

function downloadCsvBom(text, filename) {
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

function wireResultScreen() {
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

function startNewSessionDirect(result, rawText, testerName, buildEntered) {
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

function resetToStart() {
  stopStepTimer();
  el('error-box').hidden = true;
  el('paste-textarea').value = S.SAMPLE_A; // 初見でも「この手順で開始」だけで体験できるようサンプルを既定値に
  el('tester-name-input').value = '';
  checkResumeAvailable();
  showScreen('screen-start');
}

/* =========================================================================
   キーボード操作
   ========================================================================= */
function isTypingTarget(elm) {
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

/* =========================================================================
   初期化
   ========================================================================= */
function init() {
  initConfetti();
  wireStartScreen();
  wireConfirmScreen();
  wireStepScreen();
  wireOverlays();
  wireTermPopup();
  wireResultScreen();
  // 配布物の「リンクをコピー」は確認画面・用語モーダル・ポップアップの3箇所に出るのでまとめて拾う
  document.addEventListener('click', handleMaterialCopyClick);
  checkResumeAvailable();
  showScreen('screen-start');
  el('year').textContent = new Date().getFullYear();
  el('paste-textarea').value = S.SAMPLE_A; // 初見でも「この手順で開始」だけで体験できるようサンプルを既定値に

  // 進捗URL（#state=...）で開かれた場合はここで復元して確認画面かステップ画面へ直行する
  restoreFromHash();
  window.addEventListener('hashchange', function () { restoreFromHash(); });

  // 背景の軽量きらめき粒子を自作
  var sparkleContainer = el('bg-sparkle');
  if (!reducedMotionOS) {
    var n = 24;
    for (var i = 0; i < n; i++) {
      var s = document.createElement('div');
      s.className = 'sparkle';
      s.style.left = (Math.random() * 100) + '%';
      s.style.top = (Math.random() * 100) + '%';
      s.style.animationDelay = (Math.random() * 3.6) + 's';
      sparkleContainer.appendChild(s);
    }
  }
}

export function boot(bundle) {
  installI18n(bundle);
  // 確認モーダルの placeholder に出す既定テスター名。ページロードあたり1回だけ抽選する
  // （installI18n 後でないと P.TESTER_NAME_POOL が埋まっていないため、ここで代入する）
  defaultTesterName = pickRandom(P.TESTER_NAME_POOL);
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
}
