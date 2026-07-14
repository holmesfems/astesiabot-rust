use std::collections::VecDeque;

pub const MAX_STORAGE: i64 = 100_000;

fn last_true_index(values: &[bool]) -> i32 {
    for i in (0..values.len()).rev() {
        if values[i] {
            return i as i32;
        }
    }
    -1
}

#[derive(Clone)]
struct LoopIndex(Vec<usize>);

impl LoopIndex {
    fn get(&self, index: usize) -> usize {
        self.0[index]
    }
}

pub struct PowerControllerWuling {
    switch_state: [i32; 7],
    switch_value: [i64; 11],
    delay: [i32; 11],
    switch_on_off: [bool; 11],
    max_power: i64,
    loopback_delay: i32,
    loop_index: Option<LoopIndex>,
    remain_loop: VecDeque<LoopIndex>,
    power_time: i64,
}

impl PowerControllerWuling {
    pub fn new() -> Self {
        Self {
            switch_state: [0; 7],
            switch_value: [800, 400, 200, 100, 50, 25, 5, 5, 5, 5, 5],
            delay: [0, 0, 0, 0, 0, 0, 16, 12, 12, 8, 8],
            switch_on_off: [false; 11],
            max_power: 1600,
            loopback_delay: 8,
            loop_index: None,
            remain_loop: VecDeque::new(),
            power_time: 40,
        }
    }

    fn increase2_one_digit(&mut self, digit: i32) {
        if digit == 0 && self.switch_on_off[0] {
            return;
        }
        if self.switch_on_off[digit as usize] {
            self.switch_on_off[digit as usize] = false;
            self.increase2_one_digit(digit - 1);
        } else {
            self.switch_on_off[digit as usize] = true;
        }
    }

    fn increase2(&mut self) {
        self.increase2_one_digit(5);
    }

    fn can_increase2(&self) -> bool {
        !self.switch_on_off[..6].iter().all(|&x| x)
    }

    pub fn fit(&mut self, required_power: f64) {
        let mut result = [false; 11];
        let mut remain = required_power;
        for i in 0..11 {
            if remain >= self.switch_value[i] as f64 {
                remain -= self.switch_value[i] as f64;
                result[i] = true;
            }
            if remain == 0.0 {
                break;
            }
        }
        if remain > 0.0 {
            result[10] = true;
        }
        self.switch_on_off = result;

        let mut on_num = self.switch_on_off[6..11].iter().filter(|&&x| x).count();
        self.remain_loop.clear();
        if on_num == 5 {
            if self.can_increase2() {
                self.increase2();
                on_num = 0;
            }
        }
        if on_num <= 2 {
            self.switch_on_off[6..11]
                .copy_from_slice(&[on_num == 2, on_num >= 1, false, false, false]);
            self.loop_index = Some(LoopIndex(vec![2, 0, 3, 1, 4]));
        } else if on_num == 3 {
            self.switch_on_off[6..11].copy_from_slice(&[false, false, true, true, true]);
            let all_loop: Vec<Vec<usize>> = vec![
                vec![2, 0, 3, 1, 4],
                vec![2, 0, 4, 1, 3],
                vec![3, 0, 2, 1, 4],
                vec![3, 0, 4, 1, 2],
                vec![4, 0, 2, 1, 3],
                vec![4, 0, 3, 1, 2],
            ];
            for loop_ in all_loop {
                self.remain_loop.push_back(LoopIndex(loop_));
            }
            self.loop_index = self.remain_loop.pop_front();
        } else {
            // on_num >= 4
            self.switch_on_off[6..11]
                .copy_from_slice(&[on_num == 5, on_num >= 4, true, true, true]);
            let all_loop: Vec<Vec<usize>> = vec![
                vec![2, 0, 3, 1, 4],
                vec![2, 0, 4, 1, 3],
                vec![3, 0, 2, 1, 4],
                vec![3, 0, 4, 1, 2],
                vec![4, 0, 2, 1, 3],
                vec![4, 0, 3, 1, 2],
                vec![2, 1, 3, 0, 4],
                vec![2, 1, 4, 0, 3],
                vec![3, 1, 2, 0, 4],
                vec![3, 1, 4, 0, 2],
                vec![4, 1, 2, 0, 3],
                vec![4, 1, 3, 0, 2],
            ];
            for loop_ in all_loop {
                self.remain_loop.push_back(LoopIndex(loop_));
            }
            self.loop_index = self.remain_loop.pop_front();
        }
    }

    pub fn fit_by_clock(&mut self, required_power: i64, clock: i64) {
        self.fit(required_power as f64 * clock as f64 / self.power_time as f64);
    }

    pub fn needs_retry(&self) -> bool {
        !self.remain_loop.is_empty()
    }

    pub fn retry(&mut self) {
        self.loop_index = self.remain_loop.pop_front();
        self.reset_state();
    }

    fn reset_state(&mut self) {
        self.switch_state = [0; 7];
    }

    pub fn increase_power(&mut self) {
        let power = self.now_power() + 5;
        self.fit(power as f64);
        self.reset_state();
    }

    pub fn next(&mut self) -> (bool, i32) {
        for i in 0..6 {
            if self.switch_state[i] == 0 {
                self.switch_state[i] = 1;
                return (self.switch_on_off[i], self.delay[i]);
            } else {
                self.switch_state[i] = 0;
            }
        }
        let loop_index = self.loop_index.as_ref().expect("fit must be called first");
        let index = 6 + loop_index.get(self.switch_state[6] as usize);
        let result = self.switch_on_off[index];
        let mut delay = self.delay[index];
        if self.switch_state[6] == 0 {
            delay += self.loopback_delay;
        }
        self.switch_state[6] = (self.switch_state[6] + 1) % 5;
        (result, delay)
    }

    pub fn period(&self) -> i64 {
        let switch_depth = last_true_index(&self.switch_on_off);
        if switch_depth < 0 {
            return -1;
        }
        self.max_power / self.switch_value[switch_depth as usize]
    }

    pub fn is_max(&self) -> bool {
        self.switch_on_off.iter().all(|&x| x)
    }

    pub fn now_power(&self) -> i64 {
        self.switch_value
            .iter()
            .zip(self.switch_on_off.iter())
            .filter(|(_, &on)| on)
            .map(|(&v, _)| v)
            .sum()
    }

    pub fn to_bit(&self) -> String {
        self.switch_on_off
            .iter()
            .map(|&on| if on { '1' } else { '0' })
            .collect()
    }

    pub fn is_under5(&self) -> bool {
        self.switch_on_off[6..11].iter().any(|&x| x)
    }

    pub fn max_power(&self) -> i64 {
        self.max_power
    }

    pub fn power_time(&self) -> i64 {
        self.power_time
    }
}

#[derive(Clone)]
pub struct BatterySimResult {
    pub time: Vec<i64>,
    pub value: Vec<i64>,
    pub is_valid: bool,
}

impl BatterySimResult {
    pub fn min_value(&self) -> i64 {
        self.value.iter().copied().min().unwrap()
    }
}

#[allow(clippy::too_many_arguments)]
fn do_once(
    t: &mut Vec<i64>,
    v: &mut Vec<i64>,
    nowt: &mut i64,
    power_remain: &mut i64,
    nowd: &mut i64,
    is_first: &mut bool,
    controller: &mut PowerControllerWuling,
    clock: i64,
    required_power: i64,
    power_time: i64,
    max_power: i64,
) -> bool {
    let (is_accept, delay) = controller.next();
    let mut delay = delay as i64;
    if *is_first {
        t.push(*nowt);
        v.push(*power_remain);
    }
    *nowt += clock;
    *is_first = false;
    if is_accept {
        if *nowd >= delay {
            delay = *nowd;
        } else {
            t.push(*nowt + delay - clock);
            *power_remain -= required_power * (delay - *nowd);
            if *power_remain < 0 {
                *power_remain = 0;
            }
            v.push(*power_remain);
            if *power_remain == 0 {
                return false;
            }
        }
        let power_start_time = *nowt - clock + delay;
        let power_end_time = power_start_time + power_time;
        t.push(power_end_time);
        *power_remain += (max_power - required_power) * power_time;
        if *power_remain > MAX_STORAGE {
            *power_remain = MAX_STORAGE;
        }
        v.push(*power_remain);
        if *nowt > power_end_time {
            t.push(*nowt);
            *power_remain -= required_power * (*nowt - power_end_time);
            if *power_remain < 0 {
                *power_remain = 0;
            }
            v.push(*power_remain);
            if *power_remain == 0 {
                return false;
            }
            *nowd = 0;
        } else {
            *nowd = power_end_time - *nowt;
        }
    } else {
        *power_remain -= required_power * (clock - *nowd);
        if *power_remain < 0 {
            *power_remain = 0;
        }
        *nowd = 0;
        t.push(*nowt);
        v.push(*power_remain);
        if *power_remain == 0 {
            return false;
        }
    }
    true
}

pub fn simulate(required_power: i64, controller: &mut PowerControllerWuling, clock: i64) -> BatterySimResult {
    let mut power_remain = MAX_STORAGE;
    let period = controller.period();
    let power_time = controller.power_time();
    let max_power = controller.max_power();
    let mut t = Vec::new();
    let mut v = Vec::new();
    let mut nowt: i64 = 0;
    let mut nowd: i64 = 0;
    let mut is_first = true;

    let iterations = if period < 0 { 0 } else { 2 * period };
    for _ in 0..iterations {
        if !do_once(
            &mut t,
            &mut v,
            &mut nowt,
            &mut power_remain,
            &mut nowd,
            &mut is_first,
            controller,
            clock,
            required_power,
            power_time,
            max_power,
        ) {
            return BatterySimResult {
                time: t,
                value: v,
                is_valid: false,
            };
        }
    }
    let this_simulate = BatterySimResult {
        time: t,
        value: v,
        is_valid: true,
    };
    if controller.needs_retry() {
        controller.retry();
        let next_simulate = simulate(required_power, controller, clock);
        if next_simulate.min_value() <= this_simulate.min_value() {
            return next_simulate;
        }
    }
    this_simulate
}

#[derive(Clone)]
pub struct FitPlan {
    pub need_power: f64,
    pub max_power: i64,
    pub bit_str: String,
    pub sim_result: BatterySimResult,
    pub clock: i64,
}

pub fn search_fit_plan_for_one_clock(
    required_power: i64,
    storage_margin: i64,
    use_margin_under_5: bool,
    clock: i64,
) -> FitPlan {
    let mut controller = PowerControllerWuling::new();
    controller.fit_by_clock(required_power, clock);
    let mut best_plan: Option<FitPlan> = None;
    let mut best_remain_power: i64 = 0;
    loop {
        let sim_result = simulate(required_power, &mut controller, clock);
        let need_power = controller.now_power() as f64 * controller.power_time() as f64 / clock as f64;
        let plan = FitPlan {
            need_power,
            max_power: controller.max_power(),
            bit_str: controller.to_bit(),
            sim_result,
            clock,
        };
        let minv = plan.sim_result.min_value();
        if best_remain_power <= minv {
            best_remain_power = minv;
            best_plan = Some(plan.clone());
        }
        if plan.sim_result.is_valid
            && ((use_margin_under_5 && !controller.is_under5()) || plan.sim_result.min_value() >= storage_margin)
        {
            return plan;
        }
        if controller.is_max() {
            return best_plan.expect("at least one plan should have been evaluated");
        }
        controller.increase_power();
    }
}

pub fn search_fit_plan_for_all_clock(required_power: i64, storage_margin: i64, use_margin_under_5: bool) -> FitPlan {
    let dummy_controller = PowerControllerWuling::new();
    let mut fit_plans: Vec<FitPlan> = Vec::new();
    let mut clock = 40i64;
    loop {
        let is_clock_available = required_power as f64
            <= (dummy_controller.max_power() * dummy_controller.power_time()) as f64 / clock as f64
            && clock <= 102;
        if !is_clock_available {
            break;
        }
        fit_plans.push(search_fit_plan_for_one_clock(
            required_power,
            storage_margin,
            use_margin_under_5,
            clock,
        ));
        clock += 2;
    }
    fit_plans
        .into_iter()
        .min_by(|a, b| a.need_power.partial_cmp(&b.need_power).unwrap())
        .expect("at least one clock should be available")
}

pub struct ClockCircuitSimulation {
    battery_power: i64,
}

impl ClockCircuitSimulation {
    pub fn new(battery_power: i64) -> Self {
        Self { battery_power }
    }

    pub fn fit_clock(&self, power: i64, margin: i64) -> i64 {
        let clock1 = self.battery_power as f64 / power as f64 * 40.0;
        let clock2 = (MAX_STORAGE - margin) as f64 / power as f64 + 40.0;
        let clock = clock1.min(clock2);
        2 * (clock / 2.0).floor() as i64
    }

    pub fn clock_to_bit(&self, clock: i64) -> String {
        let half_clock = clock / 2;
        let exponent = (half_clock as f64).log2();
        let int_exp = exponent.ceil() as i32;
        let power2 = 2f64.powi(int_exp);
        let mut diff = power2 - half_clock as f64;
        let mut bit = String::new();
        let mut i = int_exp - 2;
        while i >= 0 {
            let sub_root = 2f64.powi(i);
            if diff >= sub_root {
                diff -= sub_root;
                bit.push('1');
            } else {
                bit.push('0');
            }
            i -= 1;
        }
        bit
    }

    pub fn get_tv_curve(&self, power: i64, clock: i64) -> BatterySimResult {
        let mut time = Vec::new();
        let mut value = Vec::new();
        let actual_clock = clock.max(40);
        let mut remain = MAX_STORAGE;
        let mut nowt: i64 = 0;

        time.push(0);
        value.push(remain);

        nowt += 40;
        time.push(nowt);
        value.push(remain);

        nowt += actual_clock - 40;
        time.push(nowt);
        remain -= power * (actual_clock - 40);
        value.push(remain);
        if remain < 0 {
            return BatterySimResult {
                time,
                value,
                is_valid: false,
            };
        }

        nowt += 40;
        time.push(nowt);
        remain += self.battery_power * 40;
        if remain > MAX_STORAGE {
            remain = MAX_STORAGE;
        }
        value.push(remain);
        if remain < MAX_STORAGE {
            return BatterySimResult {
                time,
                value,
                is_valid: false,
            };
        }

        nowt += actual_clock - 40;
        time.push(nowt);
        remain -= power * (actual_clock - 40);
        value.push(remain);
        BatterySimResult {
            time,
            value,
            is_valid: true,
        }
    }

    pub fn clock_to_power(&self, clock: i64) -> f64 {
        self.battery_power as f64 * 40.0 / clock as f64
    }
}

pub fn search_plan_for_clock_circuit(required_power: i64, storage_margin: i64, battery_power: i64) -> FitPlan {
    let clock_battery = ClockCircuitSimulation::new(battery_power);
    let clock = clock_battery.fit_clock(required_power, storage_margin);
    let bit = clock_battery.clock_to_bit(clock);
    let sim_res = clock_battery.get_tv_curve(required_power, clock);
    FitPlan {
        need_power: clock_battery.clock_to_power(clock),
        max_power: battery_power,
        bit_str: bit,
        sim_result: sim_res,
        clock,
    }
}
