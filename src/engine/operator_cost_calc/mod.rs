pub mod aggregate;
pub mod calc;
pub mod dto;
pub mod model;

use crate::engine::external_source::item_names::ItemNames;
use crate::engine::external_source::jp_first_seen::JpFirstSeen;
use crate::engine::external_source::operator_data::{OperatorData, RawOperatorCost};
use crate::engine::external_source::skill_data::SkillData;
use crate::engine::risei_calculator_engine::server::Server;
use crate::engine::risei_calculator_engine::values::RiseiValues;
use crate::engine::external_source::ExternalSourceRegistry;
use crate::engine::risei_calculator_engine::RiseiCalculatorEngine;
use chrono::{FixedOffset, NaiveDate, Utc};
use model::{build_formula_map, FormulaMap, ItemCost};
use std::sync::Arc;

/// ランキング掲載の閾値・数値表示の
/// 整数丸め判定に使う（`model::ItemCost`内部の`normalize()`用ε=0.0001とは別物）。
pub const EPSILON: f64 = 1e-6;

/// 直近実装扱いにする期間（JPのゲームデータに初めて現れてからの日数）。
pub const RECENT_DAYS: i64 = 180;

/// 直近実装か。大陸版限定、またはJPに初めて現れた日が`recent_since`以降。
/// 初登場日が記録に無い（昇格オペレーター等）ものは直近実装ではない。
fn is_recent_by(cn_only: bool, first_seen: Option<NaiveDate>, recent_since: NaiveDate) -> bool {
    cn_only || first_seen.is_some_and(|d| d >= recent_since)
}

/// 直近実装の判定に使う、JPに現れた日。記録に無い非昇格オペレーター（JP実装済み）は
/// 記録を更新した後に実装されたものなので、更新日（`checked_on`）で代用する
/// （`regen_seeds`を回す前でも当日実装分を直近実装に入れるため）。
/// 昇格オペレーターは記録の対象外なのでNoneのまま。
fn jp_seen_on(first_seen: Option<NaiveDate>, is_patch: bool, checked_on: Option<NaiveDate>) -> Option<NaiveDate> {
    first_seen.or(if is_patch { None } else { checked_on })
}

/// グローバル版・大陸版の`RiseiValues`をまとめて保持する。オペレーターごとに
/// `cnOnly`に応じて参照するサーバを切り替える必要がある。
pub struct ValueSet {
    pub global: RiseiValues,
    pub mainland: RiseiValues,
}

impl ValueSet {
    /// 大陸限定なら大陸版、そうでなければグローバル版の理性価値表を選ぶ。
    pub fn for_cn_only(&self, cn_only: bool) -> &RiseiValues {
        if cn_only {
            &self.mainland
        } else {
            &self.global
        }
    }

    pub fn for_server(&self, server: Server) -> &RiseiValues {
        match server {
            Server::Global => &self.global,
            Server::Mainland => &self.mainland,
        }
    }
}

/// 特化ランキング1件。
pub struct SkillCostInfo {
    pub operator_id: String,
    pub operator_name: String,
    pub cn_only: bool,
    pub is_recent: bool,
    pub skill_id: String,
    pub skill_name: String,
    /// 1始まりのスキル番号(S1/S2/...)。
    pub index: usize,
    pub total_cost: ItemCost,
}

impl SkillCostInfo {
    pub fn operator_name_index(&self) -> String {
        format!("{}(S{})", self.operator_name, self.index)
    }

    pub fn total_risei(&self, values: &ValueSet, item_names: &ItemNames) -> f64 {
        self.total_cost.to_risei_value(values.for_cn_only(self.cn_only), item_names)
    }
}

/// オペレーター消費素材データへの検索・ランキング機能一式。bot/apiには依存しない。
///
/// `Arc`保持にしているのは、コマンドハンドラ側が`outer_source`から取得した
/// スナップショットをasyncの境界を越えて借用なしに持ち回れるようにするため
/// （`FormulaMap`はコマンド呼び出しごとに`outer_source.formulas`から再構築する。
/// 数百件程度で軽量なため、risei_calculator_engineのようなキャッシュは設けていない）。
pub struct AllOperatorsInfo {
    pub data: Arc<OperatorData>,
    pub item_names: Arc<ItemNames>,
    pub skill_data: Arc<SkillData>,
    pub formulas: FormulaMap,
    /// この日以降にJPへ初めて現れたオペレーターを直近実装扱いにする（スナップショット時点の
    /// JSTの今日 − [`RECENT_DAYS`]日）。参照の都度今日の日付で決めるので、期限は自動で切れる。
    pub recent_since: NaiveDate,
}

impl AllOperatorsInfo {
    /// bot/REST APIが共有する計算コンテキストの構築。外部ソースと理性価値表の現在のキャッシュを
    /// 読むだけでfetch・再計算はしない。`FormulaMap`は呼び出しごとに`formulas`から再構築する。
    pub async fn snapshot(
        external_source: &ExternalSourceRegistry,
        risei: &RiseiCalculatorEngine,
    ) -> (AllOperatorsInfo, ValueSet) {
        let data = external_source.operator_data.get().await;
        let item_names = external_source.item_names.get().await;
        let skill_data = external_source.skill_data.get().await;
        let formulas_raw = external_source.formulas.get().await;
        let formulas = build_formula_map(&formulas_raw.formulas, &item_names);
        let today_jst = Utc::now().with_timezone(&FixedOffset::east_opt(9 * 3600).expect("valid JST offset")).date_naive();
        let recent_since = today_jst - chrono::Duration::days(RECENT_DAYS);
        let info = AllOperatorsInfo { data, item_names, skill_data, formulas, recent_since };

        let global_snapshot = risei.snapshot(Server::Global).await;
        let mainland_snapshot = risei.snapshot(Server::Mainland).await;
        let values = ValueSet {
            global: global_snapshot.values.clone(),
            mainland: mainland_snapshot.values.clone(),
        };
        (info, values)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&RawOperatorCost> {
        self.data.get_by_name(name)
    }

    /// 直近実装か（`/operatorcostlist`の「直近実装/将来実装オペレータのみ表示」）。
    /// 大陸版限定、またはJPに初めて現れてから[`RECENT_DAYS`]日以内（`data/seed/jp_first_seen.json`）。
    pub fn is_recent(&self, op: &RawOperatorCost) -> bool {
        let record = JpFirstSeen::embedded();
        let seen_on = jp_seen_on(record.first_seen(&op.id), op.is_patch, record.checked_on());
        is_recent_by(op.cn_only, seen_on, self.recent_since)
    }

    /// 指定した星の非昇格オペレーターを
    /// 昇進素材の理性価値(OnlyValueTarget)降順に並べる。
    pub fn sorted_by_elite_cost(&self, star: u32, values: &ValueSet) -> Vec<(&RawOperatorCost, f64)> {
        let mut list: Vec<(&RawOperatorCost, f64)> = self
            .data
            .operators
            .values()
            .filter(|op| op.stars == star && !op.is_patch)
            .map(|op| {
                let cost = aggregate::total_phase_cost(op);
                let v = values.for_cn_only(op.cn_only);
                (op, cost.to_risei_value_only_value_target(v, &self.item_names))
            })
            .collect();
        list.sort_by(|a, b| b.1.total_cmp(&a.1));
        list
    }

    /// 指定した星の全スキル特化を理性価値降順に
    /// 並べる（`totalRisei() > EPSILON`のみ）。
    pub fn sorted_skill_cost(&self, star: u32, values: &ValueSet) -> Vec<SkillCostInfo> {
        let mut list = Vec::new();
        for op in self.data.operators.values().filter(|op| op.stars == star) {
            for (i, skill) in op.skills.iter().enumerate() {
                let costs: Vec<ItemCost> = skill.masteries.iter().map(|m| ItemCost::from_cost_entries(m)).collect();
                list.push(SkillCostInfo {
                    operator_id: op.id.clone(),
                    operator_name: op.name.clone(),
                    cn_only: op.cn_only,
                    is_recent: self.is_recent(op),
                    skill_id: skill.skill_id.clone(),
                    skill_name: self.skill_data.get_str(&skill.skill_id).to_string(),
                    index: i + 1,
                    total_cost: ItemCost::sum(&costs),
                });
            }
        }
        list.retain(|item| item.total_risei(values, &self.item_names) > EPSILON);
        list.sort_by(|a, b| {
            b.total_risei(values, &self.item_names)
                .total_cmp(&a.total_risei(values, &self.item_names))
        });
        list
    }

    fn autocomplete<'b>(&'b self, partial: &'b str, limit: usize, pred: impl Fn(&RawOperatorCost) -> bool) -> Vec<String> {
        self.data
            .operators
            .values()
            .filter(|op| op.name.contains(partial) && pred(op))
            .map(|op| op.name.clone())
            .take(limit)
            .collect()
    }

    /// 特化コスト用のオペレーター名補完。
    pub fn autocomplete_master_cost(&self, partial: &str, limit: usize) -> Vec<String> {
        self.autocomplete(partial, limit, |op| op.stars >= 4)
    }

    /// 昇進コスト用のオペレーター名補完。
    pub fn autocomplete_elite_cost(&self, partial: &str, limit: usize) -> Vec<String> {
        self.autocomplete(partial, limit, |op| op.stars >= 4 && !op.is_patch)
    }

    /// モジュールコスト用のオペレーター名補完。
    pub fn autocomplete_module_cost(&self, partial: &str, limit: usize) -> Vec<String> {
        self.autocomplete(partial, limit, aggregate::has_unique_eq)
    }
}

#[cfg(test)]
mod recent_tests {
    use super::*;

    #[test]
    fn recent_is_cn_only_or_first_seen_on_or_after_boundary() {
        let since = NaiveDate::from_ymd_opt(2026, 4, 16).unwrap();
        let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d);
        assert!(is_recent_by(true, None, since), "大陸版限定は常に直近実装");
        assert!(is_recent_by(false, day(4, 16), since), "境界の日は含む");
        assert!(is_recent_by(false, day(9, 16), since));
        assert!(!is_recent_by(false, day(4, 15), since), "境界の前日は含まない");
        assert!(!is_recent_by(false, None, since), "初登場日の記録が無いものは直近実装ではない");
    }

    #[test]
    fn unrecorded_operator_is_seen_on_checked_date_unless_patch() {
        let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d);
        assert_eq!(jp_seen_on(day(6, 22), false, day(9, 16)), day(6, 22), "記録があればその日");
        assert_eq!(jp_seen_on(None, false, day(9, 16)), day(9, 16), "記録の更新後に実装された");
        assert_eq!(jp_seen_on(None, true, day(9, 16)), None, "昇格オペレーターは記録の対象外");
    }
}
