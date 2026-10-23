// Seed を作り直したあとの PR 本文（Markdown）を標準出力に出す。regen-seeds.yml から呼ぶ。
// 使い方: node .github/scripts/seed_pr_summary.mjs <ok|failed> <落ちたテスト名の一覧ファイル>
// 比べる相手は HEAD（作り直す前）。commit する前に呼ぶこと。
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";

const [testResult = "ok", failedListPath] = process.argv.slice(2);
const GOLDEN_DIR = "src/engine/risei_calculator_engine/testdata";
const TOP_N = 10;

const git = (...args) => execFileSync("git", args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
const headBlob = (path) => {
  try {
    return git("show", `HEAD:${path}`);
  } catch {
    return null; // HEAD に無い（新しく増えたファイル）
  }
};
const headSize = (path) => {
  try {
    return Number(git("cat-file", "-s", `HEAD:${path}`).trim());
  } catch {
    return null;
  }
};
const kb = (bytes) => (bytes === null ? "-" : `${(bytes / 1024).toFixed(1)} KB`);
const signedKb = (bytes) =>
  `${bytes >= 0 ? "+" : ""}${Math.abs(bytes) < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`}`;
const pct = (x) => `${x >= 0 ? "+" : ""}${x.toFixed(2)}%`;

// git status --porcelain の1行は "XY path"
const changedFiles = (dir) =>
  git("status", "--porcelain", "--untracked-files=all", "--", dir)
    .split("\n")
    .filter(Boolean)
    .map((line) => line.slice(3).trim());

const lines = [];
lines.push("週1の自動更新で `cargo run --bin regen_seeds` を回し、理性価値表の golden を出し直しました。");
lines.push("");

// テストの結果
lines.push("## テスト");
if (testResult === "ok") {
  lines.push("`cargo test` はすべて通りました。");
} else {
  const failed = failedListPath && existsSync(failedListPath) ? readFileSync(failedListPath, "utf8").split("\n").filter(Boolean) : [];
  lines.push("⚠ **`cargo test` で落ちたテストがあります。** ゲームデータの変化をドリフト検知のテストが拾った可能性があります。マージする前に直してください。");
  lines.push("");
  for (const name of failed.length ? failed : ["（テスト名を取れませんでした。ワークフローのログを見てください）"]) {
    lines.push(`- \`${name}\``);
  }
}
lines.push("");

// 変わった Seed
lines.push("## 変わった Seed");
const seeds = changedFiles("data/seed");
if (seeds.length === 0) {
  lines.push("なし");
} else {
  lines.push("| ファイル | 前 | 後 | 差 |");
  lines.push("|---|---:|---:|---:|");
  for (const path of seeds) {
    const before = headSize(path);
    const after = existsSync(path) ? statSync(path).size : null;
    const diff = before !== null && after !== null ? signedKb(after - before) : before === null ? "新規" : "削除";
    lines.push(`| \`${path}\` | ${kb(before)} | ${kb(after)} | ${diff} |`);
  }
}
lines.push("");

// 理性価値表（golden）の変化
lines.push("## 理性価値表の変化（golden）");
for (const server of ["global", "mainland"]) {
  const path = `${GOLDEN_DIR}/golden_${server}.json`;
  const beforeText = headBlob(path);
  if (!existsSync(path) || beforeText === null) continue;
  const before = JSON.parse(beforeText);
  const after = JSON.parse(readFileSync(path, "utf8"));
  lines.push(`### ${server === "global" ? "グローバル版" : "大陸版"}`);

  const categories = [...new Set([...Object.keys(before.base_stages), ...Object.keys(after.base_stages)])].sort();
  const baseChanges = categories.filter((c) => before.base_stages[c] !== after.base_stages[c]);
  if (baseChanges.length === 0) {
    lines.push("基準マップ: 変わらず");
  } else {
    lines.push("基準マップ:");
    lines.push("");
    lines.push("| カテゴリ | 前 | 後 |");
    lines.push("|---|---|---|");
    for (const c of baseChanges) lines.push(`| ${c} | ${before.base_stages[c] ?? "-"} | ${after.base_stages[c] ?? "-"} |`);
  }
  lines.push("");

  const changes = Object.entries(after.values)
    .filter(([name]) => before.values[name])
    .map(([name, value]) => ({ name, before: before.values[name], after: value, rel: ((value - before.values[name]) / before.values[name]) * 100 }))
    .filter((c) => Math.abs(c.rel) >= 1e-6)
    .sort((a, b) => Math.abs(b.rel) - Math.abs(a.rel));
  const added = Object.keys(after.values).filter((name) => !(name in before.values));
  const removed = Object.keys(before.values).filter((name) => !(name in after.values));
  if (changes.length === 0) {
    lines.push("理性価値: 変わらず");
  } else {
    lines.push(`理性価値（変化の大きい順に${Math.min(TOP_N, changes.length)}件／変わったのは全${changes.length}件）:`);
    lines.push("");
    lines.push("| 素材 | 前 | 後 | 変化 |");
    lines.push("|---|---:|---:|---:|");
    for (const c of changes.slice(0, TOP_N)) lines.push(`| ${c.name} | ${c.before.toFixed(4)} | ${c.after.toFixed(4)} | ${pct(c.rel)} |`);
  }
  if (added.length) lines.push(`\n増えた素材: ${added.join("、")}`);
  if (removed.length) lines.push(`\n無くなった素材: ${removed.join("、")}`);
  lines.push("");
}

lines.push("---");
lines.push("このPRは `.github/workflows/regen-seeds.yml` が作りました。`GITHUB_TOKEN` で作ったPRでは CI が動かないので、テストはワークフローの中で回して上に載せています。");
console.log(lines.join("\n"));
