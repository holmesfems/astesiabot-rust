// 制限モード（回数制限内に開く確率の最大化）の検証スクリプト。
// 実 engine.js を import して決定木を1回だけ辿る全数検証（1コードずつのシミュレーションはしない）。
//
// 実行方法（このマシンには node が無いため、VS Code の Electron を node として使う）:
//   $env:ELECTRON_RUN_AS_NODE="1"
//   & "$env:LOCALAPPDATA\Programs\Microsoft VS Code\Code.exe" src/api/lod_chest_solver/verify.mjs

import { ALL, bestGuesses, narrow, hintCandidates } from "./static/engine.js";

// --- 決定木を1回だけ辿って各コードの開錠タイミングを求める ---
// pivot は「前手の推奨コードがそのまま入力欄に残っている」という実際のUI挙動を再現する。
function walk(cands, universeSize, triesLimit, pivot, triesUsed, onOpen, onFail, guard) {
  if (cands.length === 0) return;
  if (--guard.budget < 0) throw new Error("guard exhausted (possible infinite recursion)");
  if (triesLimit !== null && triesUsed >= triesLimit) {
    for (const c of cands) onFail(c);
    return;
  }
  const remaining = triesLimit === null ? null : triesLimit - triesUsed;
  const { list } = bestGuesses(cands, universeSize, 1, pivot, remaining);
  const g = list[0].guess;
  if (cands.includes(g)) onOpen(g, triesUsed + 1);
  const hit = narrow(cands, g, true);
  const miss = narrow(cands, g, false);
  walk(hit, universeSize, triesLimit, g, triesUsed + 1, onOpen, onFail, guard);
  walk(miss, universeSize, triesLimit, g, triesUsed + 1, onOpen, onFail, guard);
}

function runWalk(cands, universeSize, triesLimit) {
  const opened = new Map();   // code -> 開いた手数
  const failed = new Set();   // code -> 回数切れ
  const guard = { budget: 5_000_000 };
  walk(cands, universeSize, triesLimit, null, 0, (c, d) => opened.set(c, d), c => failed.add(c), guard);
  return { opened, failed };
}

function summarize(cands, opened, failed) {
  const n = cands.length;
  const depths = cands.map(c => opened.get(c) ?? Infinity);
  const worst = Math.max(...depths.filter(d => Number.isFinite(d)));
  const succeeded = n - failed.size;
  const byDepth = new Map();
  for (const d of depths) if (Number.isFinite(d)) byDepth.set(d, (byDepth.get(d) ?? 0) + 1);
  return { n, worst, succeeded, byDepth };
}

function fmtDist(byDepth) {
  const keys = [...byDepth.keys()].sort((a, b) => a - b);
  return keys.map(k => `${k}手目:${byDepth.get(k)}`).join(" ");
}

const PATTERNS = [
  { name: "ABCD (0123, 24通り)", hints: ["0", "1", "2", "3"] },
  { name: "ABCC (0012, 12通り)", hints: ["0", "0", "1", "2"] },
  { name: "AABB (0011, 6通り)", hints: ["0", "0", "1", "1"] },
  { name: "AAAB (0001, 3通り)", hints: ["0", "0", "0", "1"] },
];
const LIMITS = [2, 4, 6, 8, 10];

let allOk = true;
function check(label, cond) {
  console.log(`  [${cond ? "OK" : "NG"}] ${label}`);
  if (!cond) allOk = false;
}

console.log("=== ヒント既知モード: パターン別 × 制限回数 ===\n");

const oldDepthCache = new Map();   // pattern name -> Map(code -> old-algorithm depth)

for (const pat of PATTERNS) {
  const cands = hintCandidates(pat.hints);
  const universeSize = cands.length;
  console.log(`--- ${pat.name} ---`);

  // 改修前（現行アルゴリズム = tries:null の経路。エンジンのnullパスは無変更なので
  // これがそのまま「改修前の手順」を再現する）で1本だけ決定木を辿り、各コードの
  // 開錠手数を記録しておく。回数制限との突き合わせはここから逆算する。
  const { opened: oldOpened } = runWalk(cands, universeSize, null);
  oldDepthCache.set(pat.name, oldOpened);
  const oldSummary = summarize(cands, oldOpened, new Set());
  console.log(`  [旧] 手数分布: ${fmtDist(oldSummary.byDepth)}  (最悪 ${oldSummary.worst}手)`);

  for (const limit of LIMITS) {
    const oldSucceed = cands.filter(c => (oldOpened.get(c) ?? Infinity) <= limit).length;

    const { opened: newOpened, failed: newFailed } = runWalk(cands, universeSize, limit);
    const newSummary = summarize(cands, newOpened, newFailed);

    console.log(`  [新 ${limit}回制限] 手数分布: ${fmtDist(newSummary.byDepth)}  `
      + `${newSummary.succeeded}/${newSummary.n}  (最悪 ${newSummary.worst}手)   `
      + `旧なら ${oldSucceed}/${newSummary.n}`);

    check(`${pat.name} ${limit}回制限: 新(${newSummary.succeeded}) >= 旧(${oldSucceed})`,
      newSummary.succeeded >= oldSucceed);
  }
  console.log("");
}

console.log("=== Wulves氏の公表値との突き合わせ ===\n");
{
  const cands = hintCandidates(["0", "0", "1", "2"]);   // ABCC
  const { opened, failed } = runWalk(cands, cands.length, 4);
  const s = summarize(cands, opened, failed);
  check(`ABCC 4回制限: ${s.succeeded}/12 >= 11/12`, s.succeeded >= 11);
}
{
  const cands = hintCandidates(["0", "0", "1", "1"]);   // AABB
  const { opened, failed } = runWalk(cands, cands.length, 2);
  const s = summarize(cands, opened, failed);
  check(`AABB 2回制限: ${s.succeeded}/6 >= 3/6`, s.succeeded >= 3);
}

console.log("\n=== 無制限モードの非破壊確認（最悪手数が改修前と一致） ===\n");
for (const pat of PATTERNS) {
  const cands = hintCandidates(pat.hints);
  const { opened } = runWalk(cands, cands.length, null);
  const s = summarize(cands, opened, new Set());
  const before = summarize(cands, oldDepthCache.get(pat.name), new Set());
  check(`${pat.name}: 無制限モード最悪手数 ${s.worst} === 改修前 ${before.worst}`, s.worst === before.worst);
}

console.log("\n=== ヒント未見モード（1000通り全数、10回制限） ===\n");
{
  const cands = ALL.map((_, i) => i);
  console.time("blind-walk-10");
  const { opened, failed } = runWalk(cands, 1000, 10);
  console.timeEnd("blind-walk-10");
  const s = summarize(cands, opened, failed);
  console.log(`  10回制限: ${s.succeeded}/${s.n} 開錠 (${(100 * s.succeeded / s.n).toFixed(1)}%)  最悪 ${s.worst}手`);

  console.time("blind-walk-null");
  const { opened: openedNull } = runWalk(cands, 1000, null);
  console.timeEnd("blind-walk-null");
  const sNull = summarize(cands, openedNull, new Set());
  console.log(`  無制限: 最悪 ${sNull.worst}手`);
  check("ヒント未見・無制限モードの最悪手数が14手のまま", sNull.worst === 14);
}

console.log(`\n${allOk ? "=== すべて合格 ===" : "=== 不合格の項目あり ==="}`);
process.exitCode = allOk ? 0 : 1;
