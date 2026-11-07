// フレームキル計算機 — 鼓舞(インスパイア)ソース。

import { computeBuffBreakdown } from "./buffs.js";
import { valueAtLevel } from "./entry.js";
import { effectiveModuleId, maxEliteFor, maxLevelForElite, resolveAtk } from "./stats.js";

/* ============================================================
   P3: 鼓舞(インスパイア)ソース
   ============================================================ */

/**
 * 鼓舞ソースの設定の初期値(state未設定時のフォールバック)。
 * P6: 昇進/レベル/信頼度も行(`makeDefaultRow`)と同じ既定(最大昇進・その最大レベル・
 * 信頼度100)にする。
 */
export function defaultInspireSourceCfg(source) {
  const elite = maxEliteFor(source);
  return {
    on: false,
    skillNum: source.skills && source.skills[0] ? source.skills[0].skillNum : "",
    skillLevel: 10, // P7: スキルLv(既定は特化3。旧デフォルトと同じ結果になる)
    potential: 5, // P8: 潜在ランク(0始まり。既定は潜在6=フル。旧来の攻撃凸+素質凸を統合)
    moduleId: null,
    moduleLv: 3,
    elite,
    level: maxLevelForElite(source, elite),
    trust: 100,
    buffPct: 0,
    buffIds: [],
    parts: {},
  };
}

/**
 * `self_parts`(素質等の自己%条件パーツ)の適用結果を計算する。`replaces`で指定された
 * パーツがON+適用可能な間、置き換え先(`replaces`の値が指すid)の寄与は無効化される
 * (加算ではなく置き換え。例: 「アビサルハンターがいる」がONなら基本の「素質」は
 * 数えない)。`requiresModule`を持つパーツは`cfg.moduleId`がそれと一致する間だけ
 * 「適用可能」になる(一致しなければUIはヒントを出し、常にOFF扱い)。
 * @returns {{pct:number, applied:Array<{id:string,label:string,shortLabel:string,value:number}>}}
 */
export function computeInspireSelfParts(source, cfg) {
  const parts = (source && source.selfParts) || [];
  const partCfg = (cfg && cfg.parts) || {};
  // P6: モジュール条件は`effectiveModuleId`(elite/level込みの装備可否)経由で判定する
  // (未装備/装備不可の間はモジュール由来の効果を含めない)。
  const moduleId = effectiveModuleId(source, cfg);
  // P8: 旧来の「素質凸」チェックボックス(cfg.talentPotential)は潜在セレクト(cfg.potential)へ
  // 統合したため、`source.talentPotentialRank`(0始まり。素質凸ボーナスが解放される潜在)
  // 以上を選んでいる間だけ適用する(rankが無いソースは素質凸ボーナスの概念が無いのでfalse)。
  const talentPotentialOn = source && source.talentPotentialRank != null && (cfg.potential ?? 5) >= source.talentPotentialRank;

  const isApplicable = (p) => !p.requiresModule || moduleId === p.requiresModule;
  const isOn = (p) => {
    if (!isApplicable(p)) return false;
    if (p.alwaysOn) return true;
    const stored = partCfg[p.id];
    return stored !== undefined ? !!stored : !!p.defaultOn;
  };
  const valueFor = (p) => {
    if (p.moduleOverride && moduleId === p.moduleOverride.module) {
      const lv = cfg.moduleLv - 1;
      const base = p.moduleOverride.pctByLevel[lv] ?? 0;
      const bonus = talentPotentialOn ? p.moduleOverride.potentialBonusByLevel[lv] ?? 0 : 0;
      return base + bonus;
    }
    if (p.requiresModule && p.pctByModuleLevel) {
      return p.pctByModuleLevel[cfg.moduleLv - 1] ?? 0;
    }
    return (p.pct || 0) + (talentPotentialOn ? p.pctPotentialBonus || 0 : 0);
  };

  const suppressed = new Set();
  for (const p of parts) {
    if (p.replaces && isOn(p)) suppressed.add(p.replaces);
  }

  const applied = [];
  let pct = 0;
  for (const p of parts) {
    if (suppressed.has(p.id)) continue;
    if (!isOn(p)) continue;
    const value = valueFor(p);
    pct += value;
    applied.push({ id: p.id, label: p.label, shortLabel: p.shortLabel || p.label, value });
  }
  return { pct, applied };
}

/**
 * 鼓舞ソースのATK。行と同じ`resolveAtk`をそのまま再利用する(`cfg`を行と同じ形状
 * {potential, moduleId, moduleLv, elite, level, trust} に正規化して渡すだけで済む)。
 */
export function resolveInspireSourceAtk(source, cfg) {
  return resolveAtk(source, {
    potential: cfg.potential,
    moduleId: cfg.moduleId,
    moduleLv: cfg.moduleLv,
    elite: cfg.elite,
    level: cfg.level,
    trust: cfg.trust,
  });
}

/**
 * 鼓舞ソース1件分の計算。個別バフ(`cfg.buffIds`)/条件付きバフ(`globalBuffIds`、
 * ソース自身の`tags`との一致で判定)は行と同じ`computeBuffBreakdown`をそのまま
 * 再利用する(行/entryの形状に正規化して渡す)。
 * `amount = atk ×(1 + 素質等の自己%パーツ合計 + 個別% + 条件付き% + 手入力%) × ratio`。
 * ソース自身のATKには鼓舞(inspireFlat)を足さない(鼓舞ソースは他の鼓舞から
 * ブーストされない。呼び出し側=`computeInspireForRow`が自己適用も除外する)。
 * @returns {{amount:number, atk:number, ratio:number, skillNum:string, selfPct:number,
 *            selfParts:{pct:number, applied:Array}, breakdown:object, manualPct:number}}
 */
/**
 * P7: `skillEntry.ratioByLevel`から`skillLevel`(既定10=特化3)の鼓舞倍率を引く。
 * 配列が無ければ`skillEntry.ratio`(固定値)にフォールバックする。
 */
export function resolveInspireRatioAtLevel(skillEntry, skillLevel) {
  if (!skillEntry) return 0;
  const arr = skillEntry.ratioByLevel;
  if (!arr || !arr.length) return skillEntry.ratio || 0;
  return valueAtLevel(arr, skillLevel);
}

export function computeInspireSource(source, cfg, catalog, globalBuffIds = [], globalBuffLevels = {}) {
  const skills = (source && source.skills) || [];
  const skillEntry = skills.find((s) => s.skillNum === cfg.skillNum) || skills[0] || { skillNum: "", ratio: 0 };
  const ratio = resolveInspireRatioAtLevel(skillEntry, cfg.skillLevel ?? 10);
  const atk = resolveInspireSourceAtk(source, cfg);
  const selfParts = computeInspireSelfParts(source, cfg);
  const pseudoRow = { buffIds: cfg.buffIds || [] };
  const pseudoEntry = { tags: (source && source.tags) || [] };
  const breakdown = computeBuffBreakdown(pseudoRow, pseudoEntry, catalog, globalBuffIds, globalBuffLevels);
  const manualPct = cfg.buffPct || 0;
  const selfPct = selfParts.pct + breakdown.extraPct + manualPct;
  const amount = atk * (1 + selfPct) * ratio;
  return { amount, atk, ratio, skillNum: skillEntry.skillNum, selfPct, selfParts, breakdown, manualPct };
}

/**
 * 行が受け取る鼓舞を決める。ONになっている鼓舞ソースのうち`amount`が最大の1件だけを
 * 採用する(合算しない)。`row.inspireOn`がfalseの間は常に`null`。鼓舞ソース自身の行
 * (`row.opId === source.operatorId`)にはそのソース自身の鼓舞は乗らない。
 * @returns {null|{sourceId:string, sourceName:string, amount:number}}
 */
export function computeInspireForRow(catalog, row, inspireSourceStates, globalBuffIds = [], globalBuffLevels = {}) {
  if (!row || row.inspireOn === false) return null;
  const sources = (catalog && catalog.inspireSources) || [];
  const states = inspireSourceStates || {};
  let best = null;
  for (const source of sources) {
    const cfg = states[source.id];
    if (!cfg || !cfg.on) continue;
    if (row.opId && row.opId === source.operatorId) continue;
    const result = computeInspireSource(source, cfg, catalog, globalBuffIds, globalBuffLevels);
    if (result.amount <= 0) continue;
    if (!best || result.amount > best.amount) best = { sourceId: source.id, sourceName: source.name, amount: result.amount };
  }
  return best;
}
