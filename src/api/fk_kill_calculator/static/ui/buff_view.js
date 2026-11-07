// フレームキル計算機 — 潜在の選択肢・バフの軸の選択・条件付き/個別バフのカード。

import {
  atkPotentialGroups,
  inspirePotentialGroups,
  resolveConditionalValue,
  talentPotentialGroups,
} from "../engine.js";
import { escapeHtml, fmtPct, trimNum } from "./dom.js";
import { buffLevels, catalog, linkedOperatorFor, state } from "./store.js";

// P4: 潜在ラベル("潜在1"〜"潜在6")。0始まりのpotentialRankをそのままindexに使う。
function potentialOptionLabel(group) {
  return group.from === group.to ? `潜在${group.from + 1}` : `潜在${group.from + 1}-${group.to + 1}`;
}

// P8: 行のATK潜在セレクト(旧「攻撃凸」チェックボックスの後継)。値が変わる境目だけを
// 選択肢にする(`atkPotentialGroups`。バフカードの潜在セレクトと同じUX)。ATK潜在を
// 持たないオペレーターは境目が無い(選択肢が「潜在1-6」の1つだけ)ため、セレクト自体を
// 隠す(オーナー承認の仕様)。P8 follow-up: `entry`も渡し、`entry.special.mulMultiplier`
// (乗算系特殊強化の素質値テーブル)があればATK潜在境目と合成する(例: ファイヤーウォッチは
// 潜在1-3/潜在4/潜在5-6の3択になる)。
export function renderRowPotentialSelect(op, entry, row, idx) {
  const groups = atkPotentialGroups(op, entry, row);
  if (groups.length <= 1) return "";
  const opts = groups
    .map((g) => {
      const selected = row.potential >= g.from && row.potential <= g.to;
      return `<option value="${g.to}"${selected ? " selected" : ""}>${potentialOptionLabel(g)}</option>`;
    })
    .join("");
  return `<label>潜在
    <select data-role="row" data-field="potential" data-idx="${idx}">${opts}</select>
  </label>`;
}

// P8: 鼓舞ソースの潜在セレクト(旧「攻撃凸」+「素質凸」チェックボックスの後継。1つに統合)。
// 境目はATK潜在の変化点 ∪ 素質凸(自己%パーツの`pctPotentialBonus`)の解放境目
// (`inspirePotentialGroups`。詳細はengine.jsのコメント参照)。
export function renderInspirePotentialSelect(source, cfg) {
  const groups = inspirePotentialGroups(source);
  if (groups.length <= 1) return "";
  const opts = groups
    .map((g) => {
      const selected = cfg.potential >= g.from && cfg.potential <= g.to;
      return `<option value="${g.to}"${selected ? " selected" : ""}>${potentialOptionLabel(g)}</option>`;
    })
    .join("");
  return `<label>潜在
    <select data-role="inspire" data-source-id="${escapeHtml(source.id)}" data-field="potential">${opts}</select>
  </label>`;
}

// P4: スキルLvラベル("1"〜"7" + "特化1"〜"特化3")。Lv1始まりのskillLevelをそのままindexに使う。
export function skillLevelOptionLabel(skillLevel) {
  return skillLevel <= 7 ? `SLv${skillLevel}` : `特化${skillLevel - 7}`;
}

// P4/P5: `source`(素質/スキルLv/スケール/固定基礎値/段階由来の動的値解決)を持つバフの
// 「値が変わる軸」だけをインラインの<select>で出す共通ヘルパー(dedupe済み。値が変わらない
// 軸は`b.source`側に無いので出ない)。conditionalのカード(renderConditionalSourceCard)と
// individualの育成設定カード(renderIndividualBuffLevelCard)の両方から使う。戻り値の
// select/checkboxは全て`data-role="global-buff-level" data-buff-id="..."`という共通の
// 書式なので、スコープ(conditional/individual)を問わず`onGlobalBuffLevelChange`が処理する。
// `b.toggle`(例: 前衛アーミヤの「スキル中は効果2倍」、ステインレスS1の「装置2台」)が
// あればチェックボックスも出す。
// P9: `linkedOp`が渡されている(=このバフがFK行の育成設定にリンクしている)間は、
// 昇進/潜在/モジュール/スキルLvのセレクトを出さず「FK行（<name>）の設定を使用」の
// ヒントだけを出す(トグルは引き続きユーザー操作できるので出す)。
function renderBuffAxisControls(b, levels, linkedOp) {
  const value = resolveConditionalValue(b, levels);
  if (linkedOp) {
    const toggle = b.toggle
      ? `<label class="check-label cond-toggle-label">
          <input type="checkbox" data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="toggleOn" ${levels.toggleOn ? "checked" : ""}>
          ${escapeHtml(b.toggle.label)}
        </label>`
      : "";
    const hint = `<p class="special-hint">FK行（${escapeHtml(linkedOp.name)}）の設定を使用</p>`;
    return { controls: toggle, hint, value };
  }
  const source = b.source;
  const talent = source.talent;
  const skill = source.skill;
  const scale = source.scale;
  const stage = source.stage;

  let controls = "";
  let hint = "";
  if (talent) {
    if (talent.eliteVaries) {
      const opts = [0, 1, 2]
        .map((e) => `<option value="${e}"${levels.elite === e ? " selected" : ""}>E${e}</option>`)
        .join("");
      controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="elite">${opts}</select>`;
    }
    if (talent.potentialVaries) {
      // 値が変わる境目だけを選択肢にする(例: 潜在1-5/潜在6)。option値はグループ内の最大潜在。
      const opts = talentPotentialGroups(talent)
        .map((g) => {
          const selected = levels.potential >= g.from && levels.potential <= g.to;
          return `<option value="${g.to}"${selected ? " selected" : ""}>${potentialOptionLabel(g)}</option>`;
        })
        .join("");
      controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="potential">${opts}</select>`;
    }
    // モジュールは昇進2でしか装備できないので、E0/E1の間はモジュール欄を出さない。
    const minElite = talent.valuesByEliteAndPotential.findIndex((byPot) => (byPot[levels.potential] ?? 0) > 0);
    if (value <= 0 && minElite > levels.elite) {
      hint = `<p class="special-hint">「${escapeHtml(b.name)}」の素質は昇進${minElite}で解放</p>`;
    } else if (talent.modules.length && levels.elite >= 2) {
      const moduleOpts =
        `<option value=""${!levels.moduleId ? " selected" : ""}>なし</option>` +
        talent.modules
          .map((m) => `<option value="${escapeHtml(m.moduleId)}"${levels.moduleId === m.moduleId ? " selected" : ""}>${escapeHtml(m.typeName)}</option>`)
          .join("");
      controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="moduleId">${moduleOpts}</select>`;
      if (levels.moduleId) {
        const lvOpts = [1, 2, 3]
          .map((lv) => `<option value="${lv}"${levels.moduleLevel === lv ? " selected" : ""}>Lv${lv}</option>`)
          .join("");
        controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="moduleLevel">${lvOpts}</select>`;
      } else if (value <= 0) {
        // スズランのように、モジュール未装備だと値が0(=効果なし)のケースのヒント。
        const m = talent.modules[0];
        hint = `<p class="special-hint">「${escapeHtml(b.name)}」はモジュール${escapeHtml(m.typeName)}装備時のみ有効</p>`;
      }
    }
  }
  if (skill) {
    const opts = skill.valuesByLevel
      .map((_, i) => {
        const lv = i + 1;
        return `<option value="${lv}"${levels.skillLevel === lv ? " selected" : ""}>${skillLevelOptionLabel(lv)}</option>`;
      })
      .join("");
    controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="skillLevel">${opts}</select>`;
  }
  // P5: talent/base_pctに掛け合わせるスキルLv別スケール。全レベルで値が同じ(varies=false。
  // 例: スワイヤーS1のtalent_scale)ならセレクトを出さない。
  if (scale && scale.varies) {
    const opts = scale.valuesByLevel
      .map((_, i) => {
        const lv = i + 1;
        return `<option value="${lv}"${levels.skillLevel === lv ? " selected" : ""}>${skillLevelOptionLabel(lv)}</option>`;
      })
      .join("");
    controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="skillLevel">${opts}</select>`;
  }
  // P5: スキルLvではなく離散段階(例: ナスティS3の装置アップグレード段階)。
  if (stage) {
    const opts = stage.values
      .map((_, i) => {
        const idx = i + 1;
        const label = stage.labels[i] ?? `${idx}`;
        return `<option value="${idx}"${levels.stageIndex === idx ? " selected" : ""}>${escapeHtml(label)}</option>`;
      })
      .join("");
    controls += `<select data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="stageIndex">${opts}</select>`;
  }
  if (b.toggle) {
    controls += `<label class="check-label cond-toggle-label">
      <input type="checkbox" data-role="global-buff-level" data-buff-id="${escapeHtml(b.id)}" data-field="toggleOn" ${levels.toggleOn ? "checked" : ""}>
      ${escapeHtml(b.toggle.label)}
    </label>`;
  }

  return { controls, hint, value };
}

// P4: `source`を持つ条件付きバフ1件分のカード。OFFの間はトグルチップだけ、ONになると
// `renderBuffAxisControls`が返す軸コントロールを表示する。
export function renderConditionalSourceCard(b) {
  const onIds = state.globalBuffIds || [];
  const on = onIds.includes(b.id);
  const targetLabel = b.scope.targetTags.join("/");
  const toggleChip = `<button type="button" class="chip" data-action="toggle-global-buff" data-buff-id="${escapeHtml(b.id)}" aria-pressed="${on}">${escapeHtml(b.name)} ${escapeHtml(targetLabel)}</button>`;
  if (!on) {
    return `<div class="cond-source-card">${toggleChip}</div>`;
  }

  const levels = buffLevels(b);
  const linkedOp = linkedOperatorFor(b);
  const { controls, hint, value } = renderBuffAxisControls(b, levels, linkedOp);
  const valueLabel = b.kind === "pct" ? `+${fmtPct(value)}%` : `+${trimNum(value)}`;

  return `<div class="cond-source-card cond-source-on" data-buff-id="${escapeHtml(b.id)}">
    <div class="cond-source-header">${toggleChip}<span class="cond-source-value">${valueLabel}</span></div>
    ${hint}
    <div class="cond-source-controls">${controls}</div>
  </div>`;
}

// P5: `source`を持つ個別バフ1件分の「育成設定」カード。individualバフはconditionalと
// 違いスコープ自体のON/OFFチップを持たない(行/鼓舞ソースのチップで選ぶため)ので、
// トグルチップの代わりに名前をそのまま見出しにする。
function renderIndividualBuffLevelCard(b) {
  const levels = buffLevels(b);
  const linkedOp = linkedOperatorFor(b);
  const { controls, hint, value } = renderBuffAxisControls(b, levels, linkedOp);
  const valueLabel = b.kind === "pct" ? `+${fmtPct(value)}%` : `+${trimNum(value)}`;
  return `<div class="cond-source-card cond-source-on" data-buff-id="${escapeHtml(b.id)}">
    <div class="cond-source-header"><span class="cond-source-name">${escapeHtml(b.name)}</span><span class="cond-source-value">${valueLabel}</span></div>
    ${hint}
    <div class="cond-source-controls">${controls}</div>
  </div>`;
}

// P5: 行(row.buffIds)/鼓舞ソース(ONのcfg.buffIds)のどちらかで現在選ばれている
// バフidの集合(育成設定カードの表示対象を絞るために使う)。
function usedIndividualBuffIds() {
  const used = new Set();
  for (const row of state.rows) for (const id of row.buffIds || []) used.add(id);
  const sources = (state.inspire && state.inspire.sources) || {};
  for (const cfg of Object.values(sources)) {
    if (!cfg || !cfg.on) continue;
    for (const id of cfg.buffIds || []) used.add(id);
  }
  return used;
}

// 「個別バフの育成設定」セクション(P5)。sourceを持つ個別バフのうち、行または鼓舞ソースで
// 現在チェックされているものだけをカード表示する(オーナー方針:
// 「チェックしたやつの育成状況を設定出来るようにすればOK」)。1件も無ければヒントのみ。
export function renderIndividualBuffLevelsSection() {
  const sourced = (catalog.buffers || []).filter((b) => b.scope.type === "individual" && b.source);
  if (!sourced.length) return "";
  const usedIds = usedIndividualBuffIds();
  const relevant = sourced.filter((b) => usedIds.has(b.id));
  const body = relevant.length
    ? relevant.map((b) => renderIndividualBuffLevelCard(b)).join("")
    : `<p class="section-empty-hint">行やソースで個別バフを選ぶと、ここで育成状況(昇進・潜在・モジュール・スキルLv等)を設定できます。</p>`;
  return `<section class="card" id="individual-buff-levels-section">
    <h2>個別バフの育成設定</h2>
    ${body}
  </section>`;
}
