// 必要電力と図面から、推奨設定と蓄電量の時系列を求める（DOM 非依存。表示は render.js）。

import { MAX_STORAGE, minValue, searchFitPlanForOneClock, searchPlanForClockCircuit } from "./battery_sim.js";

// 図面ごとの最大発電量と、制御回路の合流器の向き(表示用。0のビットに置く)。
const BLUEPRINTS = {
  CTL_1: { maxPower: 1600, merger: "left" }, // PWM回路
  CTL_3: { maxPower: 1600, merger: "right" }, // 周期回路(武陵小電池)
  CTL_4: { maxPower: 3200, merger: "right" }, // 周期回路(武陵中電池)
};

// フォームの文字列を整数として読む（Rust 版の i64 の parse と同じく、小数・空欄は不可）。
export function parseIntegerInput(text) {
  const s = String(text ?? "").trim();
  if (!/^[+-]?\d+$/.test(s)) return null;
  const n = Number(s);
  return Number.isSafeInteger(n) ? n : null;
}

function validateRequiredPower(x) {
  if (x < 5 || x % 5 !== 0) return "必要電力は 5以上の 5刻み整数で入力してください。";
  return null;
}

// 残電量安全マージンは蓄電量の範囲（0〜MAX_STORAGE。フォームの min/max と同じ）に限る。
// 範囲外の値は周期回路(CTL_3/CTL_4)の周期を 0 以下にし、clockToBit が暴走するため。
function validateStorageMargin(x) {
  if (x < 0 || x > MAX_STORAGE) {
    return `残電量安全マージンは 0以上 ${MAX_STORAGE}以下の整数で入力してください。`;
  }
  return null;
}

// 表示用に小数点以下2桁へ丸める（例: 1306.1224489795918 -> 1306.12）。
// Rust の round と同じく .5 はゼロから遠い方へ丸める。
function round2(x) {
  const y = x * 100.0;
  return (y < 0 ? -Math.round(-y) : Math.round(y)) / 100.0;
}

// 戻り値は { result } か { error }。result:
//   required_power, remainder(最大発電量で割った余り。0ならフル稼働のみで賄える),
//   setting_display(「[最大発電]×[商]+余り」形式), time_series, remaining_series,
//   bit_str(制御回路のビット。1=交差器 0=合流器), merger("left"|"right"),
//   lowest_storage, save_battery, clock(周期設定の表示。PWM回路は null)
export function optimize(requiredPower, storageMargin, useMarginUnder5, blueprintId) {
  const blueprint = BLUEPRINTS[blueprintId];
  if (!blueprint) return { error: "存在しない図面です" };
  const error = validateRequiredPower(requiredPower) ?? validateStorageMargin(storageMargin);
  if (error) return { error };

  const { maxPower, merger } = blueprint;
  const quotient = Math.trunc(requiredPower / maxPower);
  const remainder = requiredPower % maxPower;

  if (remainder === 0) {
    // 最大発電量の整数倍で丁度賄えるため、常時フル稼働のみで済み制御回路の計算は不要。
    return {
      result: {
        required_power: requiredPower,
        remainder,
        setting_display: `${maxPower}×${quotient}`,
        time_series: [],
        remaining_series: [],
        bit_str: "",
        merger,
        lowest_storage: MAX_STORAGE,
        save_battery: 0.0,
        clock: null,
      },
    };
  }

  const fitPlan =
    blueprintId === "CTL_1"
      ? searchFitPlanForOneClock(remainder, storageMargin, useMarginUnder5, 40)
      : searchPlanForClockCircuit(remainder, storageMargin, maxPower);
  const clock = blueprintId === "CTL_1" ? null : `${fitPlan.clock}s`;

  const needPowerDisplay = round2(fitPlan.need_power);
  return {
    result: {
      required_power: requiredPower,
      remainder,
      setting_display: quotient > 0 ? `${maxPower}×${quotient}+${needPowerDisplay}` : `${needPowerDisplay}`,
      time_series: fitPlan.sim_result.time,
      remaining_series: fitPlan.sim_result.value,
      bit_str: fitPlan.bit_str,
      merger,
      lowest_storage: minValue(fitPlan.sim_result),
      save_battery: round2(1.5 * 60.0 * 24.0 * (1.0 - fitPlan.need_power / fitPlan.max_power)),
      clock,
    },
  };
}
