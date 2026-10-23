use super::battery_sim::{
    search_fit_plan_for_all_clock, search_fit_plan_for_one_clock, search_plan_for_clock_circuit, FitPlan,
    MAX_STORAGE,
};

pub struct OptimizationResult {
    pub required_power: i64,
    /// 最大発電量で割った余り。制御回路で精密に賄う必要がある分。0ならフル稼働のみで賄えるため計算不要。
    pub remainder: i64,
    /// 「[最大発電]×[商]+余り」形式の表示用文字列
    pub setting_display: String,
    pub time_series: Vec<i64>,
    pub remaining_series: Vec<f64>,
    pub tobit: String,
    pub lowest_storage: i64,
    pub save_battery: f64,
    pub clock: Option<String>,
}

fn max_power_for_blueprint(blueprint_id: &str) -> Result<i64, String> {
    match blueprint_id {
        "CTL_1" | "CTL_2" | "CTL_3" => Ok(1600),
        "CTL_4" => Ok(3200),
        _ => Err("存在しない図面です".to_string()),
    }
}

fn validate_required_power(x: i64) -> Result<(), String> {
    if x < 5 || x % 5 != 0 {
        return Err("必要電力は 5以上の 5刻み整数で入力してください。".to_string());
    }
    Ok(())
}

/// 残電量安全マージンは蓄電量の範囲（0〜MAX_STORAGE。フォームの min/max と同じ）に限る。
/// 範囲外の値は周期回路(CTL_3/CTL_4)の周期を 0 以下にし、clock_to_bit が暴走するため。
fn validate_storage_margin(x: i64) -> Result<(), String> {
    if !(0..=MAX_STORAGE).contains(&x) {
        return Err(format!("残電量安全マージンは 0以上 {}以下の整数で入力してください。", MAX_STORAGE));
    }
    Ok(())
}

/// 表示用に小数点以下2桁へ丸める（例: 1306.1224489795918 -> 1306.12）
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn separate_clock(clock: i64) -> Vec<i64> {
    let base = [32, 16, 8, 4, 2];
    let mut res = Vec::new();
    let mut remain = clock - 40;
    if remain <= 0 {
        return res;
    }
    for &value in &base {
        if remain >= value {
            res.push(value);
            remain -= value;
            if remain == 0 {
                return res;
            }
        }
    }
    res
}

pub fn optimize(
    required_power: i64,
    storage_margin: i64,
    use_margin_under_5: bool,
    blueprint_id: &str,
) -> Result<OptimizationResult, String> {
    let max_power = max_power_for_blueprint(blueprint_id)?;
    validate_required_power(required_power)?;
    validate_storage_margin(storage_margin)?;

    let quotient = required_power / max_power;
    let remainder = required_power % max_power;

    if remainder == 0 {
        // 最大発電量の整数倍で丁度賄えるため、常時フル稼働のみで済み制御回路の計算は不要。
        return Ok(OptimizationResult {
            required_power,
            remainder,
            setting_display: format!("{}×{}", max_power, quotient),
            time_series: Vec::new(),
            remaining_series: Vec::new(),
            tobit: String::new(),
            lowest_storage: MAX_STORAGE,
            save_battery: 0.0,
            clock: None,
        });
    }

    const MERGER_LEFT: &str = r#"<img src="/WLBatterySimulator/static/merger.png"/>"#;
    const MERGER_RIGHT: &str = r#"<img src="/WLBatterySimulator/static/merger.png" style="transform: rotate(180deg);"/>"#;
    const CROSSER: &str = r#"<img src="/WLBatterySimulator/static/crosser.png"/>"#;

    let (fit_plan, fit_clock, merger): (FitPlan, Option<String>, &str) = match blueprint_id {
        "CTL_1" => {
            // PWM回路
            let plan = search_fit_plan_for_one_clock(remainder, storage_margin, use_margin_under_5, 40);
            (plan, None, MERGER_LEFT)
        }
        "CTL_2" => {
            // 周期→PWM回路
            let plan = search_fit_plan_for_all_clock(remainder, storage_margin, use_margin_under_5);
            let sc = separate_clock(plan.clock);
            let mut fit_clock = format!("{}s", plan.clock);
            if !sc.is_empty() {
                let mut reversed = sc.clone();
                reversed.reverse();
                fit_clock.push_str(" (");
                fit_clock.push_str(
                    &reversed
                        .iter()
                        .map(|item| format!("+{}", item))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                fit_clock.push(')');
            }
            (plan, Some(fit_clock), MERGER_LEFT)
        }
        "CTL_3" => {
            // 周期回路(武陵小電池)
            let plan = search_plan_for_clock_circuit(remainder, storage_margin, 1600);
            let fit_clock = Some(format!("{}s", plan.clock));
            (plan, fit_clock, MERGER_RIGHT)
        }
        "CTL_4" => {
            // 周期回路(武陵中電池)
            let plan = search_plan_for_clock_circuit(remainder, storage_margin, 3200);
            let fit_clock = Some(format!("{}s", plan.clock));
            (plan, fit_clock, MERGER_RIGHT)
        }
        _ => return Err("Unknown blueprintId".to_string()),
    };

    let time_series = fit_plan.sim_result.time.clone();
    let remaining_series: Vec<f64> = fit_plan.sim_result.value.iter().map(|&x| x as f64).collect();
    let save_battery = round2(1.5 * 60.0 * 24.0 * (1.0 - fit_plan.need_power / fit_plan.max_power as f64));

    let tobit: String = fit_plan
        .bit_str
        .chars()
        .map(|c| if c == '0' { merger } else { CROSSER })
        .collect();

    let need_power_display = round2(fit_plan.need_power);
    let setting_display = if quotient > 0 {
        format!("{}×{}+{}", max_power, quotient, need_power_display)
    } else {
        format!("{}", need_power_display)
    };

    Ok(OptimizationResult {
        required_power,
        remainder,
        setting_display,
        time_series,
        remaining_series,
        tobit,
        lowest_storage: fit_plan.sim_result.min_value(),
        save_battery,
        clock: fit_clock,
    })
}

#[cfg(test)]
mod tests {
    use super::super::battery_sim::ClockCircuitSimulation;
    use super::*;

    #[test]
    fn storage_margin_out_of_range_is_rejected() {
        // 周期が 0 になり clock_to_bit が暴走していた入力（CTL_3, 余り5, MAX_STORAGE超のマージン）
        assert!(optimize(5, MAX_STORAGE + 200, false, "CTL_3").is_err());
        assert!(optimize(5, MAX_STORAGE + 200, false, "CTL_4").is_err());
        assert!(optimize(5, -1, false, "CTL_3").is_err());
        assert!(optimize(5, i64::MIN, false, "CTL_4").is_err());
    }

    #[test]
    fn storage_margin_bounds_are_accepted() {
        assert!(optimize(5, 0, false, "CTL_3").is_ok());
        let result = optimize(5, MAX_STORAGE, false, "CTL_3").unwrap();
        assert_eq!(result.clock.as_deref(), Some("40s"));
    }

    #[test]
    fn clock_to_bit_returns_empty_for_clock_below_two() {
        let sim = ClockCircuitSimulation::new(1600);
        assert_eq!(sim.clock_to_bit(0), "");
        assert_eq!(sim.clock_to_bit(1), "");
        assert_eq!(sim.clock_to_bit(-40), "");
        assert_eq!(sim.clock_to_bit(40), "1100");
    }
}
