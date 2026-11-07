//! 鼓舞ソースの DTO 組み立て。

use crate::engine::external_source::operator_combat::OperatorCombat;
use crate::engine::external_source::operator_data::OperatorData;
use crate::engine::external_source::skill_data::SkillData;
use crate::engine::fk_data_search::search::skill_id_by_num;
use super::dto::{
    InspireModuleOverride, InspireSelfPart, InspireSkillRatio, InspireSource,
};
use super::{buffers, tags};
use super::{to_catalog_modules, to_phase_atk_dtos};

/// `buffers.yaml`の`inspire`リスト(生データ)と`operator_combat`をマージして
/// `dto::InspireSource`一覧を組み立てる。対象オペレーターがoperator_combatに
/// 存在しない場合(ゲームデータ更新でid変更等)は`skipped`に`"inspire:<id>"`として
/// 記録し、そのソースをカタログから静かに落とす(fk_dataのskippedと同じ方針。
/// ドリフト自体は`cargo test`の`validate_inspire_sources`で検知する)。
pub(super) fn build_inspire_sources(combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData, skipped: &mut Vec<String>) -> Vec<InspireSource> {
    let mut sources = Vec::new();
    for raw in buffers::raw_inspire_sources() {
        let Some(op) = combat.operators.get(&raw.operator) else {
            skipped.push(format!("inspire:{}", raw.id));
            continue;
        };
        let tags = tags::tags_for(&op.profession, &op.position, &op.nation_id);
        let skill_ratios: Vec<InspireSkillRatio> =
            raw.skills.iter().map(|s| build_inspire_skill_ratio(&raw.operator, s, ops, skills)).collect();
        sources.push(InspireSource {
            id: raw.id.clone(),
            operator_id: raw.operator.clone(),
            name: raw.name.clone(),
            tags,
            atk_base: op.atk_base,
            atk_potential: op.atk_potential,
            atk_potential_by_rank: op.atk_potential_by_rank,
            modules: to_catalog_modules(&op.modules),
            skills: skill_ratios,
            talent_potential_rank: raw.talent_potential_rank,
            self_parts: raw.self_parts.iter().map(to_inspire_self_part_dto).collect(),
            phases: to_phase_atk_dtos(&op.phases),
            atk_trust_max: op.atk_trust_max,
            skill_unlock_phase: op.skill_unlock_phase.clone(),
        });
    }
    sources
}

/// `RawInspireSkill`(固定`ratio` or スキルLv別追従`ratio_key`。P7)から
/// `dto::InspireSkillRatio`を組み立てる。`ratio_key`が実スキルのblackboardに解決できない
/// 場合は固定`ratio`(未指定なら0.0)にフォールバックする(ドリフト自体は
/// `validate_inspire_sources`が検知する)。
fn build_inspire_skill_ratio(operator_id: &str, raw: &buffers::RawInspireSkill, ops: &OperatorData, skills: &SkillData) -> InspireSkillRatio {
    if let Some(key) = &raw.ratio_key {
        if let Some(by_level) = ops
            .operators
            .get(operator_id)
            .and_then(|cost_op| skill_id_by_num(cost_op).get(raw.skill_num.as_str()).copied())
            .and_then(|skill_id| skills.get_blackboard_by_level(skill_id))
            .filter(|v| !v.is_empty())
        {
            let values_by_level: Vec<f64> = by_level.iter().map(|bb| bb.get(key).copied().unwrap_or(0.0)).collect();
            let ratio = values_by_level.last().copied().unwrap_or(0.0);
            return InspireSkillRatio { skill_num: raw.skill_num.clone(), ratio, ratio_by_level: values_by_level, ratio_fixed: false };
        }
    }
    let ratio = raw.ratio.unwrap_or(0.0);
    InspireSkillRatio { skill_num: raw.skill_num.clone(), ratio, ratio_by_level: vec![ratio], ratio_fixed: true }
}

/// `buffers::RawInspireSelfPart`をそのままDTO(`dto::InspireSelfPart`)へ変換する。
/// `short_label`省略時は`label`をそのまま使う。
fn to_inspire_self_part_dto(part: &buffers::RawInspireSelfPart) -> InspireSelfPart {
    InspireSelfPart {
        id: part.id.clone(),
        label: part.label.clone(),
        short_label: part.short_label.clone().unwrap_or_else(|| part.label.clone()),
        description: part.description.clone(),
        pct: part.pct,
        pct_potential_bonus: part.pct_potential_bonus,
        module_override: part.module_override.as_ref().map(|m| InspireModuleOverride {
            module: m.module.clone(),
            pct_by_level: m.pct_by_level,
            potential_bonus_by_level: m.potential_bonus_by_level,
        }),
        requires_module: part.requires_module.clone(),
        pct_by_module_level: part.pct_by_module_level,
        replaces: part.replaces.clone(),
        always_on: part.always_on,
        default_on: part.default_on,
    }
}
