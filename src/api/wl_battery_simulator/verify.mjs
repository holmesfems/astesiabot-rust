// 武陵発電計算機の計算層（static/optimizer.js・battery_sim.js）の検証スクリプト。
// Rust 版のテストを移したもの＋移す前の Rust 版の出力（testdata/golden.json）との突き合わせ。
//
// 実行方法（node は PATH に無いので絶対パスで）:
//   & "C:\Program Files\nodejs\node.exe" src/api/wl_battery_simulator/verify.mjs

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { optimize, parseIntegerInput } from "./static/optimizer.js";
import { ClockCircuitSimulation, MAX_STORAGE } from "./static/battery_sim.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));

let fail = 0;
let pass = 0;
function ok(name, cond, extra) {
  if (cond) pass++;
  else {
    fail++;
    console.log("FAIL  " + name + (extra !== undefined ? "  -> " + JSON.stringify(extra) : ""));
  }
}
const sum = (a) => a.reduce((x, y) => x + y, 0);

// 残電量安全マージンが範囲外ならエラー（周期が 0 になり clockToBit が暴走していた入力）。
for (const [margin, bp] of [
  [MAX_STORAGE + 200, "CTL_3"],
  [MAX_STORAGE + 200, "CTL_4"],
  [-1, "CTL_3"],
  [Number.MIN_SAFE_INTEGER, "CTL_4"],
]) {
  ok(`margin ${margin} on ${bp} is rejected`, optimize(5, margin, false, bp).error !== undefined);
}

// 範囲の端は受け付ける。
ok("margin 0 accepted", optimize(5, 0, false, "CTL_3").result !== undefined);
ok("margin MAX accepted with 40s clock", optimize(5, MAX_STORAGE, false, "CTL_3").result?.clock === "40s");

// clockToBit は周期2未満でビット無し。
{
  const sim = new ClockCircuitSimulation(1600);
  ok("clockToBit(0)", sim.clockToBit(0) === "");
  ok("clockToBit(1)", sim.clockToBit(1) === "");
  ok("clockToBit(-40)", sim.clockToBit(-40) === "");
  ok("clockToBit(40)", sim.clockToBit(40) === "1100");
}

// フォームの文字列は整数だけ受け付ける（Rust 版の i64 の parse と同じ）。
for (const [text, want] of [
  ["250", 250],
  [" 250 ", 250],
  ["+5", 5],
  ["-5", -5],
  ["0", 0],
  ["", null],
  ["2.5", null],
  ["1e3", null],
  ["abc", null],
  ["99999999999999999999", null],
  [null, null],
]) {
  ok(`parseIntegerInput(${JSON.stringify(text)})`, parseIntegerInput(text) === want, parseIntegerInput(text));
}

ok("unknown blueprint", optimize(230, 10000, true, "CTL_2").error === "存在しない図面です");

// 移す前の Rust 版と同じ結果になること。
{
  const golden = JSON.parse(fs.readFileSync(path.join(HERE, "testdata", "golden.json"), "utf8"));
  ok("golden not empty", golden.cases.length > 0);
  for (const c of golden.cases) {
    const [rp, margin, under5, bp] = c.input;
    const name = `golden ${JSON.stringify(c.input)}`;
    const { result, error } = optimize(rp, margin, under5, bp);
    if (c.error !== undefined) {
      ok(name + " error", error === c.error, error);
      continue;
    }
    if (!result) {
      ok(name + " result", false, error);
      continue;
    }
    const got = {
      remainder: result.remainder,
      setting_display: result.setting_display,
      bit_str: result.bit_str,
      clock: result.clock,
      lowest_storage: result.lowest_storage,
      save_battery: result.save_battery,
      points: result.time_series.length,
      time_sum: sum(result.time_series),
      value_sum: sum(result.remaining_series),
    };
    const want = { ...c };
    delete want.input;
    ok(name, JSON.stringify(got) === JSON.stringify(want), { got, want });
  }
}

console.log(`${pass} passed, ${fail} failed`);
if (fail > 0) process.exit(1);
