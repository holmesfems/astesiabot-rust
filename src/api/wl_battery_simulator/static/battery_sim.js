// 武陵発電制御のシミュレーション（DOM 非依存）。制御器のスイッチ列と、蓄電量の時系列を求める。
// 整数の割り算は Rust 版（i64）に合わせて Math.trunc で切り捨てる。

export const MAX_STORAGE = 100_000;

function lastTrueIndex(values) {
  for (let i = values.length - 1; i >= 0; i--) {
    if (values[i]) return i;
  }
  return -1;
}

const LOOPS_3 = [
  [2, 0, 3, 1, 4],
  [2, 0, 4, 1, 3],
  [3, 0, 2, 1, 4],
  [3, 0, 4, 1, 2],
  [4, 0, 2, 1, 3],
  [4, 0, 3, 1, 2],
];
const LOOPS_4 = [
  ...LOOPS_3,
  [2, 1, 3, 0, 4],
  [2, 1, 4, 0, 3],
  [3, 1, 2, 0, 4],
  [3, 1, 4, 0, 2],
  [4, 1, 2, 0, 3],
  [4, 1, 3, 0, 2],
];

export class PowerControllerWuling {
  constructor() {
    this.switchState = [0, 0, 0, 0, 0, 0, 0];
    this.switchValue = [800, 400, 200, 100, 50, 25, 5, 5, 5, 5, 5];
    this.delay = [0, 0, 0, 0, 0, 0, 16, 12, 12, 8, 8];
    this.switchOnOff = new Array(11).fill(false);
    this.maxPower = 1600;
    this.loopbackDelay = 8;
    this.loopIndex = null;
    this.remainLoop = [];
    this.powerTime = 40;
  }

  increase2OneDigit(digit) {
    if (digit === 0 && this.switchOnOff[0]) return;
    if (this.switchOnOff[digit]) {
      this.switchOnOff[digit] = false;
      this.increase2OneDigit(digit - 1);
    } else {
      this.switchOnOff[digit] = true;
    }
  }

  increase2() {
    this.increase2OneDigit(5);
  }

  canIncrease2() {
    return !this.switchOnOff.slice(0, 6).every((x) => x);
  }

  fit(requiredPower) {
    const result = new Array(11).fill(false);
    let remain = requiredPower;
    for (let i = 0; i < 11; i++) {
      if (remain >= this.switchValue[i]) {
        remain -= this.switchValue[i];
        result[i] = true;
      }
      if (remain === 0) break;
    }
    if (remain > 0) result[10] = true;
    this.switchOnOff = result;

    let onNum = this.switchOnOff.slice(6, 11).filter((x) => x).length;
    this.remainLoop = [];
    if (onNum === 5 && this.canIncrease2()) {
      this.increase2();
      onNum = 0;
    }
    if (onNum <= 2) {
      this.switchOnOff.splice(6, 5, onNum === 2, onNum >= 1, false, false, false);
      this.loopIndex = [2, 0, 3, 1, 4];
    } else if (onNum === 3) {
      this.switchOnOff.splice(6, 5, false, false, true, true, true);
      this.remainLoop = LOOPS_3.slice();
      this.loopIndex = this.remainLoop.shift();
    } else {
      // onNum >= 4
      this.switchOnOff.splice(6, 5, onNum === 5, onNum >= 4, true, true, true);
      this.remainLoop = LOOPS_4.slice();
      this.loopIndex = this.remainLoop.shift();
    }
  }

  fitByClock(requiredPower, clock) {
    this.fit((requiredPower * clock) / this.powerTime);
  }

  needsRetry() {
    return this.remainLoop.length > 0;
  }

  retry() {
    this.loopIndex = this.remainLoop.shift();
    this.resetState();
  }

  resetState() {
    this.switchState = [0, 0, 0, 0, 0, 0, 0];
  }

  increasePower() {
    this.fit(this.nowPower() + 5);
    this.resetState();
  }

  // [受け入れるか, 遅延]
  next() {
    for (let i = 0; i < 6; i++) {
      if (this.switchState[i] === 0) {
        this.switchState[i] = 1;
        return [this.switchOnOff[i], this.delay[i]];
      }
      this.switchState[i] = 0;
    }
    const index = 6 + this.loopIndex[this.switchState[6]];
    const result = this.switchOnOff[index];
    let delay = this.delay[index];
    if (this.switchState[6] === 0) delay += this.loopbackDelay;
    this.switchState[6] = (this.switchState[6] + 1) % 5;
    return [result, delay];
  }

  period() {
    const switchDepth = lastTrueIndex(this.switchOnOff);
    if (switchDepth < 0) return -1;
    return Math.trunc(this.maxPower / this.switchValue[switchDepth]);
  }

  isMax() {
    return this.switchOnOff.every((x) => x);
  }

  nowPower() {
    let sum = 0;
    for (let i = 0; i < 11; i++) if (this.switchOnOff[i]) sum += this.switchValue[i];
    return sum;
  }

  toBit() {
    return this.switchOnOff.map((on) => (on ? "1" : "0")).join("");
  }

  isUnder5() {
    return this.switchOnOff.slice(6, 11).some((x) => x);
  }
}

// { time: number[], value: number[], is_valid: boolean }
export function minValue(simResult) {
  return simResult.value.reduce((a, b) => Math.min(a, b), Infinity);
}

function doOnce(st, controller, clock, requiredPower, powerTime, maxPower) {
  const [isAccept, delay0] = controller.next();
  let delay = delay0;
  if (st.isFirst) {
    st.t.push(st.nowt);
    st.v.push(st.powerRemain);
  }
  st.nowt += clock;
  st.isFirst = false;
  if (isAccept) {
    if (st.nowd >= delay) {
      delay = st.nowd;
    } else {
      st.t.push(st.nowt + delay - clock);
      st.powerRemain -= requiredPower * (delay - st.nowd);
      if (st.powerRemain < 0) st.powerRemain = 0;
      st.v.push(st.powerRemain);
      if (st.powerRemain === 0) return false;
    }
    const powerStartTime = st.nowt - clock + delay;
    const powerEndTime = powerStartTime + powerTime;
    st.t.push(powerEndTime);
    st.powerRemain += (maxPower - requiredPower) * powerTime;
    if (st.powerRemain > MAX_STORAGE) st.powerRemain = MAX_STORAGE;
    st.v.push(st.powerRemain);
    if (st.nowt > powerEndTime) {
      st.t.push(st.nowt);
      st.powerRemain -= requiredPower * (st.nowt - powerEndTime);
      if (st.powerRemain < 0) st.powerRemain = 0;
      st.v.push(st.powerRemain);
      if (st.powerRemain === 0) return false;
      st.nowd = 0;
    } else {
      st.nowd = powerEndTime - st.nowt;
    }
  } else {
    st.powerRemain -= requiredPower * (clock - st.nowd);
    if (st.powerRemain < 0) st.powerRemain = 0;
    st.nowd = 0;
    st.t.push(st.nowt);
    st.v.push(st.powerRemain);
    if (st.powerRemain === 0) return false;
  }
  return true;
}

export function simulate(requiredPower, controller, clock) {
  const period = controller.period();
  const st = { t: [], v: [], nowt: 0, nowd: 0, powerRemain: MAX_STORAGE, isFirst: true };

  const iterations = period < 0 ? 0 : 2 * period;
  for (let i = 0; i < iterations; i++) {
    if (!doOnce(st, controller, clock, requiredPower, controller.powerTime, controller.maxPower)) {
      return { time: st.t, value: st.v, is_valid: false };
    }
  }
  const thisSimulate = { time: st.t, value: st.v, is_valid: true };
  if (controller.needsRetry()) {
    controller.retry();
    const nextSimulate = simulate(requiredPower, controller, clock);
    if (minValue(nextSimulate) <= minValue(thisSimulate)) return nextSimulate;
  }
  return thisSimulate;
}

// FitPlan: { need_power, max_power, bit_str, sim_result, clock }
export function searchFitPlanForOneClock(requiredPower, storageMargin, useMarginUnder5, clock) {
  const controller = new PowerControllerWuling();
  controller.fitByClock(requiredPower, clock);
  let bestPlan = null;
  let bestRemainPower = 0;
  for (;;) {
    const simResult = simulate(requiredPower, controller, clock);
    const plan = {
      need_power: (controller.nowPower() * controller.powerTime) / clock,
      max_power: controller.maxPower,
      bit_str: controller.toBit(),
      sim_result: simResult,
      clock,
    };
    const minv = minValue(simResult);
    if (bestRemainPower <= minv) {
      bestRemainPower = minv;
      bestPlan = plan;
    }
    if (simResult.is_valid && ((useMarginUnder5 && !controller.isUnder5()) || minv >= storageMargin)) {
      return plan;
    }
    if (controller.isMax()) return bestPlan;
    controller.increasePower();
  }
}

export class ClockCircuitSimulation {
  constructor(batteryPower) {
    this.batteryPower = batteryPower;
  }

  fitClock(power, margin) {
    const clock1 = (this.batteryPower / power) * 40.0;
    const clock2 = (MAX_STORAGE - margin) / power + 40.0;
    const clock = Math.min(clock1, clock2);
    return 2 * Math.floor(clock / 2.0);
  }

  clockToBit(clock) {
    const halfClock = Math.trunc(clock / 2);
    // halfClock < 1 だと log2 が -Infinity/NaN になり、下のループが回らないか暴走する。
    // halfClock == 1 と同じくビット無しとして返す。
    if (halfClock < 1) return "";
    const intExp = Math.ceil(Math.log2(halfClock));
    let diff = 2 ** intExp - halfClock;
    let bit = "";
    for (let i = intExp - 2; i >= 0; i--) {
      const subRoot = 2 ** i;
      if (diff >= subRoot) {
        diff -= subRoot;
        bit += "1";
      } else {
        bit += "0";
      }
    }
    return bit;
  }

  getTvCurve(power, clock) {
    const time = [];
    const value = [];
    const actualClock = Math.max(clock, 40);
    let remain = MAX_STORAGE;
    let nowt = 0;

    time.push(0);
    value.push(remain);

    nowt += 40;
    time.push(nowt);
    value.push(remain);

    nowt += actualClock - 40;
    time.push(nowt);
    remain -= power * (actualClock - 40);
    value.push(remain);
    if (remain < 0) return { time, value, is_valid: false };

    nowt += 40;
    time.push(nowt);
    remain += this.batteryPower * 40;
    if (remain > MAX_STORAGE) remain = MAX_STORAGE;
    value.push(remain);
    if (remain < MAX_STORAGE) return { time, value, is_valid: false };

    nowt += actualClock - 40;
    time.push(nowt);
    remain -= power * (actualClock - 40);
    value.push(remain);
    return { time, value, is_valid: true };
  }

  clockToPower(clock) {
    return (this.batteryPower * 40.0) / clock;
  }
}

export function searchPlanForClockCircuit(requiredPower, storageMargin, batteryPower) {
  const clockBattery = new ClockCircuitSimulation(batteryPower);
  const clock = clockBattery.fitClock(requiredPower, storageMargin);
  return {
    need_power: clockBattery.clockToPower(clock),
    max_power: batteryPower,
    bit_str: clockBattery.clockToBit(clock),
    sim_result: clockBattery.getTvCurve(requiredPower, clock),
    clock,
  };
}
