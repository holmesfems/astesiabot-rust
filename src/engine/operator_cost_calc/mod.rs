pub mod aggregate;
pub mod calc;
pub mod dto;
pub mod model;

use crate::engine::outer_source::item_names::ItemNames;
use crate::engine::outer_source::operator_data::{OperatorData, RawOperatorCost};
use crate::engine::outer_source::skill_data::SkillData;
use crate::engine::risei_calculator_engine::server::Server;
use crate::engine::risei_calculator_engine::values::RiseiValues;
use model::{FormulaMap, ItemCost};
use std::sync::Arc;

/// charmaterials.py 側の`EPSILON = 1e-6`。ランキング掲載の閾値・数値表示の
/// 整数丸め判定に使う（`model::ItemCost`内部の`normalize()`用ε=0.0001とは別物）。
pub const EPSILON: f64 = 1e-6;

/// グローバル版・大陸版の`RiseiValues`をまとめて保持する。オペレーターごとに
/// `cnOnly`に応じて参照するサーバを切り替える必要がある(Python各所の
/// `CalculatorManager.getValues(glob,...)`の`glob`切り替えに対応)。
pub struct ValueSet {
    pub global: RiseiValues,
    pub mainland: RiseiValues,
}

impl ValueSet {
    /// Python各所の `isGlobal = not costItem.isCNOnly()` に対応する選択。
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

/// 特化ランキング1件（Python `AllOperatorsInfo.SkillCostInfo`）。
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

/// オペレーター消費素材データへの検索・ランキング機能一式（Python `AllOperatorsInfo`+
/// `OperatorCostsCalculator`の一部相当）。bot/apiには依存しない。
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
}

impl AllOperatorsInfo {
    pub fn get_by_name(&self, name: &str) -> Option<&RawOperatorCost> {
        self.data.get_by_name(name)
    }

    /// Python `getSortedCostDict_ByEliteCost`。指定した星の非昇格オペレーターを
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

    /// Python `getSortedSkillCostDict`。指定した星の全スキル特化を理性価値降順に
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
                    is_recent: op.is_recent,
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

    /// Python `autoCompleteForMasterCost`。
    pub fn autocomplete_master_cost(&self, partial: &str, limit: usize) -> Vec<String> {
        self.autocomplete(partial, limit, |op| op.stars >= 4)
    }

    /// Python `autoCompleteForEliteCost`。
    pub fn autocomplete_elite_cost(&self, partial: &str, limit: usize) -> Vec<String> {
        self.autocomplete(partial, limit, |op| op.stars >= 4 && !op.is_patch)
    }

    /// Python `autoCompleteForModuleCost`。
    pub fn autocomplete_module_cost(&self, partial: &str, limit: usize) -> Vec<String> {
        self.autocomplete(partial, limit, aggregate::has_unique_eq)
    }
}
