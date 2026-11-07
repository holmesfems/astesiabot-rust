//! フレームキル計算機の「機械データ + 手動補正(overrides.yaml)」マージ層（★純粋ロジック。
//! bot にも api にも依存しない。`engine::recruit` / `engine::operator_cost_calc` と同じ
//! 位置付け）。
//!
//! 3層構成のうち3層目に相当する:
//! 1. 機械抽出できる生データ → `engine::external_source::operator_combat` +
//!    `engine::external_source::skill_data`(blackboard)
//! 2. 手動補正 → `overrides.rs`(`data/fk_kill_calc/overrides.yaml`)
//! 3. マージ → このモジュールの`build_catalog`

pub mod buffers;
mod conditional_source;
pub mod dto;
mod entry;
mod inspire;
mod overrides;
pub mod tags;
mod validate;

#[cfg(test)]
mod tests;

pub use dto::Catalog;
pub use overrides::{OverrideSpecial, OverrideVariant, Overrides, OverridesMap};

use crate::engine::external_source::fk_data::FkSheetData;
use crate::engine::external_source::operator_combat::OperatorCombat;
use crate::engine::external_source::operator_data::OperatorData;
use crate::engine::external_source::skill_data::SkillData;
use crate::engine::fk_data_search::search::skill_id_by_num;
use dto::{
    CatalogModule, CatalogOperator, PhaseAtk, Valued,
};
use std::collections::HashMap;

pub use validate::{
    validate_inspire_sources,
    validate_overrides,
    validate_special_requires_module,
    validate_special_mul_multiplier,
    validate_override_level_fields,
    validate_combined_variants,
};
use inspire::build_inspire_sources;
use entry::{
    to_special_dto,
    build_entry,
    build_combined_entry,
    values_by_level_for_key,
    resolve_variant_multiplier,
    resolve_variant_self_atk_pct,
    resolve_multiplier_defaults,
};

/// `operator_combat::RawModuleCombat`一覧をカタログ表示用の`CatalogModule`一覧へ変換する
/// (`CatalogOperator.modules`/`InspireSource.modules`の両方から使う共通ロジック)。
fn to_catalog_modules(modules: &[crate::engine::external_source::operator_combat::RawModuleCombat]) -> Vec<CatalogModule> {
    modules
        .iter()
        .map(|m| CatalogModule {
            id: m.eq_id.clone(),
            type_name: m.eq_type.clone(),
            name: m.name.clone(),
            atk_by_level: m.atk_by_level.clone(),
            unlock_phase: m.unlock_phase,
            unlock_level: m.unlock_level,
        })
        .collect()
}

/// `operator_combat::RawPhaseAtk`一覧をDTO(`dto::PhaseAtk`)へ変換する
/// (`CatalogOperator.phases`/`InspireSource.phases`の両方から使う共通ロジック)。
fn to_phase_atk_dtos(phases: &[crate::engine::external_source::operator_combat::RawPhaseAtk]) -> Vec<PhaseAtk> {
    phases.iter().map(|p| PhaseAtk { max_level: p.max_level, atk_min: p.atk_min, atk_max: p.atk_max }).collect()
}

/// fk_dataシートとゲームデータ側の表記ゆれ（全角/半角括弧、前後の空白）を吸収する。
/// fk_dataシートは「アーミヤ（前衛）」のように全角括弧を使うことがあるが、
/// operator_combat/operator_data側の名前（`operator_data.rs`の`build_patches`が
/// 組み立てる`"{元名}({職名})"`）は半角括弧なので、比較前に必ずこれを通す。
fn normalize_operator_name(name: &str) -> String {
    name.trim().replace('（', "(").replace('）', ")")
}

/// `build_catalog`の戻り値。fk_dataに載っているがoperator_combat/operator_dataで
/// 名前解決できなかったオペレーター名を`skipped`に集約し、カバレッジ確認に使えるようにする
/// (召喚物トークン等、そもそもオペレーターカタログに載らない名前がfk_dataに混ざることがある)。
pub struct CatalogBuild {
    pub catalog: Catalog,
    pub skipped: Vec<String>,
}

/// 機械データ(`operator_combat`/`skill_data`)と手動補正(`overrides.yaml`)をマージして
/// カタログを組み立てる。fk_dataに載っているオペレーター名を起点に処理する
/// (逆に、fk_dataに情報が無いオペレーターはカタログに載らない。
/// フレームキル計算機はFK情報が無いスキルを扱えないため)。
pub fn build_catalog(fk: &FkSheetData, ops: &OperatorData, combat: &OperatorCombat, skills: &SkillData, overrides: &Overrides) -> CatalogBuild {
    let mut operators = Vec::new();
    let mut skipped = Vec::new();

    // 全角/半角括弧ゆれを吸収した名前 -> id の索引を先に1回だけ作る
    // (fk_dataの操作対象は100件超あるので、行毎に正規化しなくて済むようにする)。
    let normalized_name_to_id: HashMap<String, &str> =
        combat.name_to_id.iter().map(|(n, id)| (normalize_operator_name(n), id.as_str())).collect();

    for (name, rows) in &fk.by_operator {
        let Some(op_id) = normalized_name_to_id.get(&normalize_operator_name(name)).copied() else {
            skipped.push(name.clone());
            continue;
        };
        let Some(combat_op) = combat.operators.get(op_id) else {
            skipped.push(name.clone());
            continue;
        };
        // skill_num -> skillId の解決には`operator_data`(消費素材ドメイン)の`skills`一覧を使う
        // (fk_data_search::search と同じロジック。`skill_id_by_num`を共有する)。
        // 名前ではなく`op_id`(operator_combatの名前解決を経て確定済み)でoperator_data側を
        // 引くことで、fk_dataとoperator_data間の名前表記ゆれも同時に吸収する
        // (operator_combatとoperator_dataは同じchar_table.jsonキーをidにしているため)。
        let Some(cost_op) = ops.operators.get(op_id) else {
            skipped.push(name.clone());
            continue;
        };

        let skill_ids = skill_id_by_num(cost_op);
        let operator_tags = tags::tags_for(&combat_op.profession, &combat_op.position, &combat_op.nation_id);
        let default_damage_type = tags::guess_damage_type(&combat_op.profession);

        let mut fk_entries = Vec::new();
        for row in rows {
            let skill_id = skill_ids.get(row.skill_num.as_str()).copied();
            let blackboard = skill_id.and_then(|id| skills.get_blackboard(id));
            // P7: スキルLv別blackboard(空Vecなら「無い」扱いに正規化する。levels.rsが
            // deserialize失敗時に空Mapを積むことがあるため、要素はあっても値が全部0の
            // ケースまでは弾かない=`values_by_level_for_key`が0.0フォールバックで処理する)。
            let blackboard_by_level = skill_id.and_then(|id| skills.get_blackboard_by_level(id)).filter(|v| !v.is_empty()).map(Vec::as_slice);
            let skill_label = skill_id
                .map(|id| skills.get_str(id))
                .filter(|s| !s.is_empty() && *s != "Missing")
                .map(str::to_string)
                .unwrap_or_else(|| row.skill_num.clone());

            let (default_multiplier, default_multiplier_by_level, multiplier_candidates) =
                resolve_multiplier_defaults(blackboard, blackboard_by_level);
            let num_levels = default_multiplier_by_level.len();
            let default_self_atk_pct_by_level = values_by_level_for_key(blackboard_by_level, "atk", num_levels);
            let default_self_atk_pct = default_self_atk_pct_by_level.last().copied().unwrap_or(0.0);
            let default_hits: u32 = 1;

            // このエントリ(スキル単位)のタグ = オペレーター機械タグ + 弾薬スキル機械タグ
            // (`skill_data::is_ammo_skill`。skill_idが解決できないエントリ(素質行等)は
            // 対象外) + overrideの手動tags(加算。variantごとに追加できる)。
            let is_ammo = skill_id.map(|id| skills.is_ammo_skill(id)).unwrap_or(false);
            let mut entry_tags = operator_tags.clone();
            if is_ammo {
                entry_tags.push(tags::AMMO_SKILL_TAG.to_string());
            }

            match overrides.variants_for(op_id, &row.skill_num) {
                // P10: 同じskill_num内でcombined:trueのバリアント群は「同時発生する
                // 複数ダメージパーツ」として1つのFkEntryにまとめる(選択肢を増やさない)。
                Some(variants) if !variants.is_empty() && variants[0].combined => {
                    fk_entries.push(build_combined_entry(
                        row,
                        &skill_label,
                        variants,
                        &default_multiplier_by_level,
                        default_multiplier,
                        blackboard_by_level,
                        num_levels,
                        &multiplier_candidates,
                        &default_self_atk_pct_by_level,
                        default_self_atk_pct,
                        default_hits,
                        default_damage_type,
                        &entry_tags,
                        combat_op,
                        &mut skipped,
                        op_id,
                    ));
                }
                Some(variants) if !variants.is_empty() => {
                    for variant in variants {
                        let mut tags_for_variant = entry_tags.clone();
                        if let Some(extra) = &variant.tags {
                            tags_for_variant.extend(extra.iter().cloned());
                        }
                        let (multiplier, multiplier_by_level, multiplier_fixed) = resolve_variant_multiplier(
                            variant,
                            &default_multiplier_by_level,
                            default_multiplier,
                            blackboard_by_level,
                            num_levels,
                        );
                        let (self_atk_pct, self_atk_pct_by_level, self_atk_pct_fixed) =
                            resolve_variant_self_atk_pct(variant, &default_self_atk_pct_by_level, default_self_atk_pct, num_levels);
                        // P8 follow-up: `mul_multiplier`(素質値テーブル参照)が実データに解決
                        // できない(talent_index範囲外)場合は、fk_dataのskipped/inspireのskippedと
                        // 同じ方針で`skipped`に記録した上でこの特殊強化を静かに落とす(ドリフト自体は
                        // `cargo test`の`validate_special_mul_multiplier`が検知する)。
                        let special = variant.special.as_ref().and_then(|s| match to_special_dto(s, combat_op) {
                            Some(dto) => Some(dto),
                            None => {
                                skipped.push(format!("special:{op_id}/{}", row.skill_num));
                                None
                            }
                        });
                        let mut entry = build_entry(
                            row,
                            skill_label.clone(),
                            variant.label.clone(),
                            multiplier,
                            multiplier_by_level,
                            multiplier_fixed,
                            multiplier_candidates.clone(),
                            self_atk_pct,
                            self_atk_pct_by_level,
                            self_atk_pct_fixed,
                            Valued::from_override(variant.hits, default_hits),
                            Valued::from_override(variant.damage_type, default_damage_type),
                            tags_for_variant,
                            special,
                            variant.note.clone(),
                        );
                        entry.default_module = variant.default_module.clone();
                        fk_entries.push(entry);
                    }
                }
                _ => {
                    fk_entries.push(build_entry(
                        row,
                        skill_label.clone(),
                        None,
                        Valued::auto(default_multiplier),
                        default_multiplier_by_level,
                        false,
                        multiplier_candidates,
                        Valued::auto(default_self_atk_pct),
                        default_self_atk_pct_by_level,
                        false,
                        Valued::auto(default_hits),
                        Valued::auto(default_damage_type),
                        entry_tags,
                        None,
                        None,
                    ));
                }
            }
        }

        operators.push(CatalogOperator {
            id: op_id.to_string(),
            name: name.clone(),
            tags: operator_tags,
            atk_base: combat_op.atk_base,
            atk_potential: combat_op.atk_potential,
            atk_potential_by_rank: combat_op.atk_potential_by_rank,
            modules: to_catalog_modules(&combat_op.modules),
            fk_entries,
            phases: to_phase_atk_dtos(&combat_op.phases),
            atk_trust_max: combat_op.atk_trust_max,
            skill_unlock_phase: combat_op.skill_unlock_phase.clone(),
        });
    }

    let inspire_sources = build_inspire_sources(combat, ops, skills, &mut skipped);

    // P4/P5: 固定pct/flatのバフ(individual + 一部conditional)に、ゲームデータから動的解決した
    // conditional/individual(`source`付き)を続けて足す。fk_dataのskipped/inspireのskippedと
    // 同じ方針で、実データと不一致なバフは`skipped`に記録した上で静かに落とす
    // (`conditional_source::build_conditional_sourced_buffers`/
    // `build_individual_sourced_buffers`参照)。
    let mut buffers = buffers::global().to_vec();
    buffers.extend(conditional_source::build_conditional_sourced_buffers(combat, ops, skills, &mut skipped));
    buffers.extend(conditional_source::build_individual_sourced_buffers(combat, ops, skills, &mut skipped));

    CatalogBuild { catalog: Catalog { operators, buffers, inspire_sources }, skipped }
}
