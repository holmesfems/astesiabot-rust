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
mod overrides;
pub mod tags;

pub use dto::Catalog;
pub use overrides::{OverrideSpecial, OverrideVariant, Overrides, OverridesMap};

use crate::engine::external_source::fk_data::{FkSheetData, FkSheetRow};
use crate::engine::external_source::operator_combat::{OperatorCombat, RawOperatorCombat};
use crate::engine::external_source::operator_data::OperatorData;
use crate::engine::external_source::skill_data::SkillData;
use crate::engine::fk_data_search::search::skill_id_by_num;
use buffers::RawInspireSource;
use dto::{
    CatalogModule, CatalogOperator, DamageType, FkEntry, FkPart, InspireModuleOverride, InspireSelfPart, InspireSkillRatio, InspireSource,
    MultiplierCandidate, PhaseAtk, Special, Valued,
};
use indexmap::IndexMap;
use std::collections::HashMap;

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

/// `buffers.yaml`の`inspire`リスト(生データ)と`operator_combat`をマージして
/// `dto::InspireSource`一覧を組み立てる。対象オペレーターがoperator_combatに
/// 存在しない場合(ゲームデータ更新でid変更等)は`skipped`に`"inspire:<id>"`として
/// 記録し、そのソースをカタログから静かに落とす(fk_dataのskippedと同じ方針。
/// ドリフト自体は`cargo test`の`validate_inspire_sources`で検知する)。
fn build_inspire_sources(combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData, skipped: &mut Vec<String>) -> Vec<InspireSource> {
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

/// `buffers.yaml`の`inspire`リストが実データ(operator_combat+operator_data)と
/// 整合しているかを検証する。不一致の一覧を返す(空ならOK)。ゲームデータ更新で
/// オペレーターID・skill_num・モジュールIDが変わった際のドリフト検知用
/// (`cargo test`で実行する)。
pub fn validate_inspire_sources(sources: &[RawInspireSource], combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData) -> Vec<String> {
    let mut bad = Vec::new();
    for s in sources {
        let Some(combat_op) = combat.operators.get(&s.operator) else {
            bad.push(format!("inspire:{} (operator id'{}'がoperator_combatに無い)", s.id, s.operator));
            continue;
        };
        match ops.operators.get(&s.operator) {
            Some(cost_op) => {
                let skill_ids = skill_id_by_num(cost_op);
                for sk in &s.skills {
                    let Some(skill_id) = skill_ids.get(sk.skill_num.as_str()) else {
                        bad.push(format!("inspire:{} (skill_num'{}'が'{}'の実スキルに無い)", s.id, sk.skill_num, s.operator));
                        continue;
                    };
                    // P7: ratio(固定)/ratio_key(スキルLv別追従)はどちらか一方が必須。
                    match (sk.ratio, &sk.ratio_key) {
                        (None, None) => bad.push(format!("inspire:{} (skill_num'{}'はratioかratio_keyのどちらかが必要)", s.id, sk.skill_num)),
                        (Some(_), Some(_)) => bad.push(format!("inspire:{} (skill_num'{}'のratioとratio_keyを同時指定できない)", s.id, sk.skill_num)),
                        _ => {}
                    }
                    if let Some(key) = &sk.ratio_key {
                        let has_key = skills
                            .get_blackboard_by_level(skill_id)
                            .map(|levels| levels.iter().any(|lv| lv.contains_key(key)))
                            .unwrap_or(false);
                        if !has_key {
                            bad.push(format!("inspire:{} (skill_num'{}'のratio_key'{key}'がblackboardに無い)", s.id, sk.skill_num));
                        }
                    }
                }
            }
            None => bad.push(format!("inspire:{} (operator id'{}'がoperator_dataに無い)", s.id, s.operator)),
        }

        let check_module = |module_id: &str, field: &str, bad: &mut Vec<String>| {
            if !combat_op.modules.iter().any(|m| m.eq_id == module_id) {
                bad.push(format!("inspire:{} ({field}'{module_id}'が'{}'のmodulesに無い)", s.id, s.operator));
            }
        };
        for part in &s.self_parts {
            if let Some(module_override) = &part.module_override {
                check_module(&module_override.module, &format!("self_parts.{}.module_override.module", part.id), &mut bad);
            }
            if let Some(module_id) = &part.requires_module {
                check_module(module_id, &format!("self_parts.{}.requires_module", part.id), &mut bad);
            }
        }
    }
    bad
}

/// `overrides.yaml`の`special`(手動データ)をDTO(`dto::Special`)へ変換する。
/// `mul_multiplier`(P8 follow-upで素質値テーブル参照になった)が実データ(`op.talents`)に
/// 解決できない(talent_index範囲外)場合は`None`を返し、呼び出し側が`skipped`に記録する。
fn to_special_dto(special: &OverrideSpecial, op: &RawOperatorCombat) -> Option<Special> {
    let mul_multiplier = match &special.mul_multiplier {
        Some(mm) => Some(conditional_source::build_talent_source(op, mm.talent, &mm.key)?),
        None => None,
    };
    Some(Special {
        label: special.label.clone(),
        description: special.description.clone(),
        requires_module: special.requires_module.clone(),
        add_self_atk_pct_by_module_level: special.add_self_atk_pct_by_module_level,
        mul_multiplier,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_entry(
    row: &FkSheetRow,
    skill_label: String,
    variant_label: Option<String>,
    multiplier: Valued<f64>,
    multiplier_by_level: Vec<f64>,
    multiplier_fixed: bool,
    multiplier_candidates: Vec<MultiplierCandidate>,
    self_atk_pct: Valued<f64>,
    self_atk_pct_by_level: Vec<f64>,
    self_atk_pct_fixed: bool,
    hits: Valued<u32>,
    damage_type: Valued<DamageType>,
    tags: Vec<String>,
    special: Option<Special>,
    note: Option<String>,
) -> FkEntry {
    FkEntry {
        skill_num: row.skill_num.clone(),
        skill_label,
        variant_label,
        fk_num: row.fk_num.clone(),
        fk_err: row.fk_err.clone(),
        detail: row.detail.clone(),
        last_edited: row.last_edited.clone(),
        multiplier,
        multiplier_by_level,
        multiplier_fixed,
        multiplier_candidates,
        self_atk_pct,
        self_atk_pct_by_level,
        self_atk_pct_fixed,
        hits,
        damage_type,
        tags,
        special,
        default_module: None,
        note,
        parts: Vec::new(),
    }
}

/// P10: 同じskill_num内の`combined: true`バリアント群を1つのFkEntryにまとめる。
/// トップレベルのmultiplier/multiplier_by_level/hits/damage_type等 = 先頭バリアント
/// (=パーツ0)。self_atk_pct/special/tags/default_module/noteも先頭バリアントのものを
/// 使う(2番目以降がこれらを持っていないことは`validate_combined_variants`が検証する)。
/// `variant_label`はパーツラベルを"+"で連結する(例: "物理+術")。
#[allow(clippy::too_many_arguments)]
fn build_combined_entry(
    row: &FkSheetRow,
    skill_label: &str,
    variants: &[OverrideVariant],
    default_multiplier_by_level: &[f64],
    default_multiplier: f64,
    blackboard_by_level: Option<&[IndexMap<String, f64>]>,
    num_levels: usize,
    multiplier_candidates: &[MultiplierCandidate],
    default_self_atk_pct_by_level: &[f64],
    default_self_atk_pct: f64,
    default_hits: u32,
    default_damage_type: DamageType,
    entry_tags: &[String],
    combat_op: &RawOperatorCombat,
    skipped: &mut Vec<String>,
    op_id: &str,
) -> FkEntry {
    let parts: Vec<FkPart> = variants
        .iter()
        .map(|variant| {
            let (multiplier, multiplier_by_level, multiplier_fixed) =
                resolve_variant_multiplier(variant, default_multiplier_by_level, default_multiplier, blackboard_by_level, num_levels);
            FkPart {
                label: variant.label.clone().unwrap_or_default(),
                multiplier,
                multiplier_by_level,
                multiplier_fixed,
                damage_type: Valued::from_override(variant.damage_type, default_damage_type),
            }
        })
        .collect();

    let head = &variants[0];
    let mut tags_for_head = entry_tags.to_vec();
    if let Some(extra) = &head.tags {
        tags_for_head.extend(extra.iter().cloned());
    }
    let (self_atk_pct, self_atk_pct_by_level, self_atk_pct_fixed) =
        resolve_variant_self_atk_pct(head, default_self_atk_pct_by_level, default_self_atk_pct, num_levels);
    let special = head.special.as_ref().and_then(|s| match to_special_dto(s, combat_op) {
        Some(dto) => Some(dto),
        None => {
            skipped.push(format!("special:{op_id}/{}", row.skill_num));
            None
        }
    });
    let variant_label = Some(variants.iter().filter_map(|v| v.label.clone()).collect::<Vec<_>>().join("+"));
    let head_part = parts[0].clone();

    let mut entry = build_entry(
        row,
        skill_label.to_string(),
        variant_label,
        head_part.multiplier,
        head_part.multiplier_by_level,
        head_part.multiplier_fixed,
        multiplier_candidates.to_vec(),
        self_atk_pct,
        self_atk_pct_by_level,
        self_atk_pct_fixed,
        // Hit数は全パーツ共通(1回の攻撃で全パーツが同時に出る)なので先頭バリアントの値だけを使う。
        Valued::from_override(head.hits, default_hits),
        head_part.damage_type,
        tags_for_head,
        special,
        head.note.clone(),
    );
    entry.default_module = head.default_module.clone();
    entry.parts = parts;
    entry
}

/// blackboardから倍率のデフォルト値と候補一覧を作る。
///
/// デフォルト値の選定順序:
///   1. 厳密一致の`atk_scale`キーがあればその値
///   2. 無ければ、キーが`atk_scale`または`damage_scale`で終わる項目のうち、
///      キー名アルファベット順で最初のもの(例: `damage_by_atk_scale`、
///      `attack@s2.atk_scale`。後者は`attack@s2.magic_atk_scale`より
///      アルファベット順で先に来るため、物理側が選ばれる)
///   3. どちらも無ければ1.0
/// (1敗のBlaze/Hornで実データ調査済み。詳細は`overrides.yaml`のコメント参照)。
///
/// 候補一覧はキーに"scale"を含む項目全部。**順序はSeed経由/実fetch経由で
/// blackboardの元の並びが変わっても揺れないよう、常に「選ばれたデフォルトのキーを
/// 先頭、残りはキー名のアルファベット順」に正規化する**
/// (`write_seed_file`がJSONキーを再帰的にソートして書き出す影響で、Seed読み込み時と
/// 実fetch時とでIndexMapの挿入順が食い違うため。この関数の出力を常に決定的にすることで
/// フロント側の表示順がSeed/本番のどちらでも変わらないようにする)。
/// P7: `blackboard_by_level`の各レベルから`key`の値を取り出した配列を作る(無ければ0.0で
/// 埋める)。`blackboard_by_level`自体が無ければ`num_levels`個(0なら1個)の0.0配列を返す
/// (「スキルLv別データが無い＝単一値のフォールバック」を1箇所に集約する)。
fn values_by_level_for_key(blackboard_by_level: Option<&[IndexMap<String, f64>]>, key: &str, num_levels: usize) -> Vec<f64> {
    match blackboard_by_level {
        Some(levels) => levels.iter().map(|lv| lv.get(key).copied().unwrap_or(0.0)).collect(),
        None => vec![0.0; num_levels.max(1)],
    }
}

/// 小数第4位に丸める(P7。`self_atk_pct_factor`計算時の浮動小数ドリフト対策。
/// 例: `0.8 × 0.89`が`0.7119999999999999`のような値になっても`0.712`に正規化する)。
fn round4(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}

/// バリアントの倍率(Valued値・スキルLv別配列・固定フラグ)を解決する(P7)。
/// `multiplier`(固定)/`multiplier_key`(そのスキルのblackboardキー名でスキルLvに追従)は
/// どちらか一方のみが指定されている前提(両方/どちらも無い場合の整合性は
/// `validate_override_level_fields`が検証し、ここではAutoへ安全にフォールバックする)。
fn resolve_variant_multiplier(
    variant: &OverrideVariant,
    default_by_level: &[f64],
    default_value: f64,
    blackboard_by_level: Option<&[IndexMap<String, f64>]>,
    num_levels: usize,
) -> (Valued<f64>, Vec<f64>, bool) {
    if let Some(fixed) = variant.multiplier {
        return (Valued::manual(fixed), vec![fixed; num_levels.max(1)], true);
    }
    if let Some(key) = &variant.multiplier_key {
        let by_level = values_by_level_for_key(blackboard_by_level, key, num_levels);
        let value = by_level.last().copied().unwrap_or(default_value);
        return (Valued::manual(value), by_level, false);
    }
    (Valued::auto(default_value), default_by_level.to_vec(), false)
}

/// バリアントのセルフATK%(Valued値・スキルLv別配列・固定フラグ)を解決する(P7)。
/// `self_atk_pct`(固定)/`self_atk_pct_factor`(Autoのスキルレベル別セルフ%に係数を掛けて
/// 追従させる)はどちらか一方のみが指定されている前提。
fn resolve_variant_self_atk_pct(
    variant: &OverrideVariant,
    default_by_level: &[f64],
    default_value: f64,
    num_levels: usize,
) -> (Valued<f64>, Vec<f64>, bool) {
    if let Some(fixed) = variant.self_atk_pct {
        return (Valued::manual(fixed), vec![fixed; num_levels.max(1)], true);
    }
    if let Some(factor) = variant.self_atk_pct_factor {
        let by_level: Vec<f64> = default_by_level.iter().map(|v| round4(v * factor)).collect();
        let value = by_level.last().copied().unwrap_or(round4(default_value * factor));
        return (Valued::manual(value), by_level, false);
    }
    (Valued::auto(default_value), default_by_level.to_vec(), false)
}

fn resolve_multiplier_defaults(
    blackboard: Option<&IndexMap<String, f64>>,
    blackboard_by_level: Option<&[IndexMap<String, f64>]>,
) -> (f64, Vec<f64>, Vec<MultiplierCandidate>) {
    let num_levels = blackboard_by_level.map(|levels| levels.len()).unwrap_or(1).max(1);
    let Some(blackboard) = blackboard else {
        return (1.0, vec![1.0; num_levels], Vec::new());
    };
    let mut candidates: Vec<(String, f64)> =
        blackboard.iter().filter(|(k, _)| k.contains("scale")).map(|(k, v)| (k.clone(), *v)).collect();
    candidates.sort_by(|(a, _), (b, _)| a.cmp(b));

    // P7: `candidates`をこの後mutate(先頭への並び替え)するため、chosen_keyは所有権付き
    // (`String`)にしてcandidatesへの借用を早めに切る(借用したままmutateしようとすると
    // 借用チェッカに弾かれる)。
    let chosen_key: Option<String> = if blackboard.contains_key("atk_scale") {
        Some("atk_scale".to_string())
    } else {
        candidates
            .iter()
            .map(|(k, _)| k.as_str())
            .find(|k| k.ends_with("atk_scale") || k.ends_with("damage_scale"))
            .map(str::to_string)
    };

    let default_multiplier = match &chosen_key {
        Some(key) => blackboard.get(key.as_str()).copied().unwrap_or(1.0),
        None => 1.0,
    };

    // 選ばれたキーを候補一覧の先頭へ動かす(既にアルファベット順ソート済みなので、
    // それ以外の並びは変えない = 「選ばれたもの優先、残りはアルファベット順」)。
    if let Some(key) = &chosen_key {
        if let Some(pos) = candidates.iter().position(|(k, _)| k == key) {
            let picked = candidates.remove(pos);
            candidates.insert(0, picked);
        }
    }

    // P7: 選ばれたキーのスキルLv別配列がデフォルト倍率(`multiplierByLevel`)、
    // 候補一覧の各キーもそれぞれスキルLv別配列を持たせる(倍率候補ドロップダウンが
    // 現在のスキルLvでの値を表示できるようにする)。
    let default_multiplier_by_level = match &chosen_key {
        Some(key) => values_by_level_for_key(blackboard_by_level, key, num_levels),
        None => vec![1.0; num_levels],
    };
    let multiplier_candidates: Vec<MultiplierCandidate> = candidates
        .into_iter()
        .map(|(key, _)| {
            let values_by_level = values_by_level_for_key(blackboard_by_level, &key, num_levels);
            MultiplierCandidate { key, values_by_level }
        })
        .collect();

    (default_multiplier, default_multiplier_by_level, multiplier_candidates)
}

/// overrides.yamlの各キー(operator_id, skill_num)が実際に`operator_combat`+`fk_data`に
/// 存在することを検証する。存在しないキーの一覧を返す(空ならOK)。ゲームデータ更新で
/// オペレーターIDやskill_numが変わった際のドリフト検知用(`cargo test`で実行する)。
pub fn validate_overrides(overrides: &Overrides, fk: &FkSheetData, combat: &OperatorCombat) -> Vec<String> {
    // fk_data側も全角/半角括弧ゆれがあり得るため、build_catalogと同じ正規化で突き合わせる。
    let normalized_fk_names: HashMap<String, &str> =
        fk.by_operator.keys().map(|n| (normalize_operator_name(n), n.as_str())).collect();

    let mut bad = Vec::new();
    for (op_id, skill_num) in overrides.all_keys() {
        let Some(op) = combat.operators.get(op_id) else {
            bad.push(format!("{op_id}/{skill_num} (operator idがoperator_combatに無い)"));
            continue;
        };
        let Some(fk_name) = normalized_fk_names.get(&normalize_operator_name(&op.name)).copied() else {
            bad.push(format!("{op_id}/{skill_num} (オペレーター名'{}'がfk_dataに無い)", op.name));
            continue;
        };
        let Some(rows) = fk.by_operator.get(fk_name) else {
            bad.push(format!("{op_id}/{skill_num} (オペレーター名'{}'がfk_dataに無い)", op.name));
            continue;
        };
        if !rows.iter().any(|r| r.skill_num == skill_num) {
            bad.push(format!("{op_id}/{skill_num} (skill_num'{skill_num}'が'{}'のfk_data行に無い)", op.name));
        }
    }
    bad
}

/// overrides.yamlの`special.requires_module`(加算系特殊強化のモジュール条件)が、
/// そのオペレーターの実際のモジュール(operator_combatのmodules)を指しているかを検証する。
/// 存在しない参照の一覧を返す(空ならOK)。ゲームデータ更新でuniEquipIdが変わった際の
/// ドリフト検知用(`cargo test`で実行する。P2 follow-upで追加)。
pub fn validate_special_requires_module(overrides: &Overrides, combat: &OperatorCombat) -> Vec<String> {
    let mut bad = Vec::new();
    for (op_id, skill_num) in overrides.all_keys() {
        let Some(variants) = overrides.variants_for(op_id, skill_num) else { continue };
        for variant in variants {
            // special.requires_moduleと、同じくuniEquipIdを指すdefault_moduleをまとめて検証する。
            let refs = [
                ("special.requires_module", variant.special.as_ref().and_then(|s| s.requires_module.as_ref())),
                ("default_module", variant.default_module.as_ref()),
            ];
            for (field, module_id) in refs {
                let Some(module_id) = module_id else { continue };
                let Some(op) = combat.operators.get(op_id) else {
                    bad.push(format!("{op_id}/{skill_num} (operator idがoperator_combatに無い)"));
                    continue;
                };
                if !op.modules.iter().any(|m| m.eq_id == *module_id) {
                    bad.push(format!("{op_id}/{skill_num} ({field}'{module_id}'が'{}'のmodulesに無い)", op.name));
                }
            }
        }
    }
    bad
}

/// overrides.yamlの`special.mul_multiplier`(乗算系。P8 follow-upで固定値から素質値
/// テーブル参照`{ talent, key }`へ置き換えた)が、実データ(`operator_combat`のtalents)に
/// 解決できるかを検証する。`talent_index`が範囲外な参照に加え、「最大成長(E2・潜在6)でも
/// 値が0以下」(=`key`のスペルミス等で存在しないキーを指している。存在しないキーは
/// `resolve_talent_base`が0にフォールバックしてしまい、`build_talent_source`自体は
/// 成功[Some]を返すため、これが無いと静かに見逃す)も不一致として検出する。
/// 存在しない参照の一覧を返す(空ならOK)。ゲームデータ更新でtalent構成が変わった際の
/// ドリフト検知用(`cargo test`で実行する。P8 follow-upで追加)。
pub fn validate_special_mul_multiplier(overrides: &Overrides, combat: &OperatorCombat) -> Vec<String> {
    let mut bad = Vec::new();
    for (op_id, skill_num) in overrides.all_keys() {
        let Some(variants) = overrides.variants_for(op_id, skill_num) else { continue };
        for variant in variants {
            let Some(special) = &variant.special else { continue };
            let Some(mm) = &special.mul_multiplier else { continue };
            let Some(op) = combat.operators.get(op_id) else {
                bad.push(format!("{op_id}/{skill_num} (operator idがoperator_combatに無い)"));
                continue;
            };
            match conditional_source::build_talent_source(op, mm.talent, &mm.key) {
                Some(table) => {
                    let max_growth = table.values_by_elite_and_potential[2][5];
                    if max_growth <= 0.0 {
                        bad.push(format!(
                            "{op_id}/{skill_num} (special.mul_multiplier.talent[{}]/key'{}'が最大成長[E2・潜在6]でも値0以下)",
                            mm.talent, mm.key
                        ));
                    }
                }
                None => bad.push(format!("{op_id}/{skill_num} (special.mul_multiplier.talent[{}]が'{}'のtalentsに無い)", mm.talent, op_id)),
            }
        }
    }
    bad
}

/// overrides.yamlの`multiplier_key`/`self_atk_pct_factor`(P7)が実データと整合しているかを
/// 検証する。固定値との同時指定・存在しないキー参照の一覧を返す(空ならOK)。ゲームデータ
/// 更新でblackboardのキー名が変わった際のドリフト検知用(`cargo test`で実行する)。
pub fn validate_override_level_fields(overrides: &Overrides, ops: &OperatorData, skills: &SkillData) -> Vec<String> {
    let mut bad = Vec::new();
    for (op_id, skill_num) in overrides.all_keys() {
        let Some(variants) = overrides.variants_for(op_id, skill_num) else { continue };
        let skill_id = ops.operators.get(op_id).and_then(|cost_op| skill_id_by_num(cost_op).get(skill_num).copied());
        for variant in variants {
            if variant.multiplier.is_some() && variant.multiplier_key.is_some() {
                bad.push(format!("{op_id}/{skill_num} (multiplierとmultiplier_keyを同時指定できない)"));
            }
            if variant.self_atk_pct.is_some() && variant.self_atk_pct_factor.is_some() {
                bad.push(format!("{op_id}/{skill_num} (self_atk_pctとself_atk_pct_factorを同時指定できない)"));
            }
            if let Some(key) = &variant.multiplier_key {
                let has_key = skill_id
                    .and_then(|id| skills.get_blackboard_by_level(id))
                    .map(|levels| levels.iter().any(|lv| lv.contains_key(key)))
                    .unwrap_or(false);
                if !has_key {
                    bad.push(format!("{op_id}/{skill_num} (multiplier_key'{key}'がblackboardに無い)"));
                }
            }
        }
    }
    bad
}

/// overrides.yamlの`combined`(P10)が正しく使われているかを検証する:
///   (a) 同じskill_num内でcombinedが一部のバリアントだけtrueになっていないこと
///   (b) combinedの2番目以降のバリアントがself_atk_pct/self_atk_pct_factor/special/tagsを
///       持たないこと(default_moduleは先頭と同値なら可)
/// 不一致の一覧を返す(空ならOK)。`cargo test`のドリフト検知用。
pub fn validate_combined_variants(overrides: &Overrides) -> Vec<String> {
    let mut bad = Vec::new();
    for (op_id, skill_num) in overrides.all_keys() {
        let Some(variants) = overrides.variants_for(op_id, skill_num) else { continue };
        if variants.len() < 2 {
            continue;
        }
        let any_combined = variants.iter().any(|v| v.combined);
        let all_combined = variants.iter().all(|v| v.combined);
        if any_combined && !all_combined {
            bad.push(format!("{op_id}/{skill_num} (combinedが一部のバリアントだけtrueになっている。全バリアントで揃えること)"));
            continue;
        }
        if !all_combined {
            continue;
        }
        let head = &variants[0];
        for (i, variant) in variants.iter().enumerate().skip(1) {
            if variant.self_atk_pct.is_some() || variant.self_atk_pct_factor.is_some() {
                bad.push(format!("{op_id}/{skill_num} (combinedバリアント{i}がself_atk_pct/self_atk_pct_factorを持っている。先頭バリアントのみ許可)"));
            }
            if variant.hits.is_some() {
                bad.push(format!("{op_id}/{skill_num} (combinedバリアント{i}がhitsを持っている。Hit数は全パーツ共通なので先頭バリアントのみ許可)"));
            }
            if variant.special.is_some() {
                bad.push(format!("{op_id}/{skill_num} (combinedバリアント{i}がspecialを持っている。先頭バリアントのみ許可)"));
            }
            if variant.tags.is_some() {
                bad.push(format!("{op_id}/{skill_num} (combinedバリアント{i}がtagsを持っている。先頭バリアントのみ許可)"));
            }
            if let Some(dm) = &variant.default_module {
                if Some(dm) != head.default_module.as_ref() {
                    bad.push(format!("{op_id}/{skill_num} (combinedバリアント{i}のdefault_moduleが先頭と異なる)"));
                }
            }
        }
    }
    bad
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::{fk_data, operator_combat, operator_data, skill_data};

    fn load_seed<T: serde::de::DeserializeOwned>(path: &str) -> T {
        let s = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("seed({path})の読み込みに失敗: {e}。先に`cargo run --bin regen_seeds`を実行すること"));
        serde_json::from_str(&s).unwrap_or_else(|e| panic!("seed({path})のparseに失敗: {e}"))
    }

    fn build_from_seeds() -> CatalogBuild {
        let fk: FkSheetData = load_seed(fk_data::SEED_PATH);
        let ops: OperatorData = load_seed(operator_data::SEED_PATH);
        let combat: OperatorCombat = load_seed(operator_combat::SEED_PATH);
        let skills: SkillData = load_seed(skill_data::SEED_PATH);
        build_catalog(&fk, &ops, &combat, &skills, Overrides::global())
    }

    #[test]
    fn catalog_builds_from_seeds() {
        let result = build_from_seeds();
        assert!(!result.catalog.operators.is_empty());
        assert!(!result.catalog.buffers.is_empty(), "P2でbuffers.yaml起点のバフが入るはず");
    }

    #[test]
    fn ash_s3_has_three_manual_variants_with_expected_multipliers() {
        let result = build_from_seeds();
        let ash = result.catalog.operators.iter().find(|op| op.name == "Ash").expect("Ashがカタログに存在すること");
        let s3_entries: Vec<_> = ash.fk_entries.iter().filter(|e| e.skill_num == "3").collect();
        assert_eq!(s3_entries.len(), 3, "AshのS3は300/400/800%の3バリアントのはず");
        let mut multipliers: Vec<f64> = s3_entries.iter().map(|e| e.multiplier.value).collect();
        multipliers.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(multipliers, vec![3.0, 4.0, 8.0]);
        for e in &s3_entries {
            assert_eq!(e.multiplier.source, dto::ValueSource::Manual, "overrideで指定した倍率はManualのはず");
        }
    }

    #[test]
    fn machine_only_entry_has_auto_source() {
        let result = build_from_seeds();
        // ホルンのスキル1(skill_num="1")はoverrides.yamlに載せていないので、Autoになるはず。
        let horn = result.catalog.operators.iter().find(|op| op.name == "ホルン").expect("ホルンがカタログに存在すること");
        let s1 = horn.fk_entries.iter().find(|e| e.skill_num == "1").expect("ホルンのskill_num=1が存在すること");
        assert_eq!(s1.multiplier.source, dto::ValueSource::Auto);
        assert_eq!(s1.self_atk_pct.source, dto::ValueSource::Auto);
        assert_eq!(s1.hits.source, dto::ValueSource::Auto);
        assert_eq!(s1.damage_type.source, dto::ValueSource::Auto);
    }

    /// overrides.yamlの全キーが実データ(operator_combat+fk_data)を指していることの
    /// ドリフト検知。ゲームデータ更新でオペレーターIDやskill_numが変わった場合、
    /// このテストが不一致キーを列挙して落ちる。
    #[test]
    fn every_override_key_points_to_existing_operator_and_skill_num() {
        let fk: FkSheetData = load_seed(fk_data::SEED_PATH);
        let combat: OperatorCombat = load_seed(operator_combat::SEED_PATH);
        let bad = validate_overrides(Overrides::global(), &fk, &combat);
        assert!(bad.is_empty(), "overrides.yamlに実データと不一致なキーがある:\n{}", bad.join("\n"));
    }

    /// overrides.yamlの`special.requires_module`(P2 follow-upで追加)が、実データの
    /// modules一覧に存在するuniEquipIdを指していること。ゲームデータ更新でモジュールIDが
    /// 変わった際のドリフト検知用。
    #[test]
    fn every_special_requires_module_points_to_an_existing_module() {
        let combat: OperatorCombat = load_seed(operator_combat::SEED_PATH);
        let bad = validate_special_requires_module(Overrides::global(), &combat);
        assert!(bad.is_empty(), "overrides.yamlのspecial.requires_moduleが実データと不一致:\n{}", bad.join("\n"));
    }

    /// overrides.yamlの`special.mul_multiplier`(P8 follow-upで素質値テーブル参照
    /// `{ talent, key }`へ置き換え)が、実データ(operator_combatのtalents)に解決でき、
    /// 最大成長で値0以下(=キー名のスペルミス等)でないこと。ゲームデータ更新でtalent構成が
    /// 変わった際のドリフト検知用。
    #[test]
    fn every_special_mul_multiplier_points_to_an_existing_talent_and_key() {
        let combat: OperatorCombat = load_seed(operator_combat::SEED_PATH);
        let bad = validate_special_mul_multiplier(Overrides::global(), &combat);
        assert!(bad.is_empty(), "overrides.yamlのspecial.mul_multiplierが実データと不一致:\n{}", bad.join("\n"));
    }

    /// ブレイズ(char_017_huang) S3: self_atk_pctは特殊強化の有無に関わらず常に0.712
    /// (Manual。バグにより理論値0.8の8/9しか反映されない実測値)で、特殊強化
    /// 「待機ボーナス」はモジュールX(uniequip_002_huang)Lv2で+4%/Lv3で+6%を「加算」する
    /// (置き換えではない)。P2 follow-upの回帰テスト。
    #[test]
    fn blaze_s3_self_atk_pct_is_always_0_712_and_special_is_additive_module_gated() {
        let result = build_from_seeds();
        let blaze = result.catalog.operators.iter().find(|op| op.name == "ブレイズ").expect("ブレイズがカタログに存在すること");
        let s3 = blaze.fk_entries.iter().find(|e| e.skill_num == "3").expect("ブレイズのS3が存在すること");
        assert_eq!(s3.self_atk_pct.value, 0.712, "self_atk_pctはバグ込みの実測値0.712で常に一定のはず");
        assert_eq!(s3.self_atk_pct.source, dto::ValueSource::Manual);
        let special = s3.special.as_ref().expect("ブレイズS3に特殊強化(待機ボーナス)があるはず");
        assert_eq!(special.label, "待機ボーナス");
        assert_eq!(special.requires_module.as_deref(), Some("uniequip_002_huang"));
        assert_eq!(special.add_self_atk_pct_by_module_level, Some([0.0, 0.04, 0.06]));
        assert!(special.description.is_some(), "descriptionがあるはず(ⓘボタン表示用)");
        // 乗算系(mul_multiplier)はブレイズには無い(加算系のみ)。
        assert!(special.mul_multiplier.is_none());
    }

    /// ファイヤーウォッチ(char_158_milu) S2「遠距離特効」: 乗算系(mul_multiplier)。
    /// P8 follow-upで固定値から素質「暗殺者」(talent[0].atk_scale)の値テーブル参照へ
    /// 置き換えた。E1=1.2(潜在1〜4)/1.25(潜在5〜6)、E2=1.4/1.45、モジュールY
    /// (uniequip_002_milu)Lv1=E2基礎値のまま(素質強化が付かない)/Lv2=1.45,1.5/Lv3=1.5,1.55。
    /// E0は素質自体が未解放なので値0(オーナー確認済みの実データ値。2026-09時点)。
    #[test]
    fn fw_s2_special_uses_mul_multiplier_talent_table() {
        let result = build_from_seeds();
        let fw = result.catalog.operators.iter().find(|op| op.name == "ファイヤーウォッチ").expect("ファイヤーウォッチがカタログに存在すること");
        let s2 = fw.fk_entries.iter().find(|e| e.skill_num == "2").expect("ファイヤーウォッチのS2が存在すること");
        let special = s2.special.as_ref().expect("ファイヤーウォッチS2に特殊強化(遠距離特効)があるはず");
        let table = special.mul_multiplier.as_ref().expect("mul_multiplierがあるはず");

        let approx_eq = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // E0: 素質未解放なので全潜在0。
        assert!(table.values_by_elite_and_potential[0].iter().all(|v| approx_eq(*v, 0.0)), "E0={:?}", table.values_by_elite_and_potential[0]);
        // E1: 潜在1〜4=1.2、潜在5〜6=1.25。
        assert!(approx_eq(table.values_by_elite_and_potential[1][0], 1.2), "E1潜在1={}", table.values_by_elite_and_potential[1][0]);
        assert!(approx_eq(table.values_by_elite_and_potential[1][3], 1.2), "E1潜在4={}", table.values_by_elite_and_potential[1][3]);
        assert!(approx_eq(table.values_by_elite_and_potential[1][4], 1.25), "E1潜在5={}", table.values_by_elite_and_potential[1][4]);
        // E2: 潜在1〜4=1.4、潜在5〜6=1.45。
        assert!(approx_eq(table.values_by_elite_and_potential[2][0], 1.4), "E2潜在1={}", table.values_by_elite_and_potential[2][0]);
        assert!(approx_eq(table.values_by_elite_and_potential[2][5], 1.45), "E2潜在6={}", table.values_by_elite_and_potential[2][5]);

        let module_y = table.modules.iter().find(|m| m.module_id == "uniequip_002_milu").expect("モジュールYがあるはず");
        // Lv1(index0)は素質強化が付かないのでE2基礎値のまま。
        assert!(approx_eq(module_y.values_by_level_and_potential[0][0], 1.4), "モジュールYLv1潜在1={}", module_y.values_by_level_and_potential[0][0]);
        assert!(approx_eq(module_y.values_by_level_and_potential[0][5], 1.45), "モジュールYLv1潜在6={}", module_y.values_by_level_and_potential[0][5]);
        // Lv2(index1)=1.45(潜在1〜4)/1.5(潜在5〜6)。
        assert!(approx_eq(module_y.values_by_level_and_potential[1][0], 1.45), "モジュールYLv2潜在1={}", module_y.values_by_level_and_potential[1][0]);
        assert!(approx_eq(module_y.values_by_level_and_potential[1][5], 1.5), "モジュールYLv2潜在6={}", module_y.values_by_level_and_potential[1][5]);
        // Lv3(index2)=1.5(潜在1〜4)/1.55(潜在5〜6)。
        assert!(approx_eq(module_y.values_by_level_and_potential[2][0], 1.5), "モジュールYLv3潜在1={}", module_y.values_by_level_and_potential[2][0]);
        assert!(approx_eq(module_y.values_by_level_and_potential[2][3], 1.5), "モジュールYLv3潜在4={}", module_y.values_by_level_and_potential[2][3]);
        assert!(approx_eq(module_y.values_by_level_and_potential[2][4], 1.55), "モジュールYLv3潜在5={}", module_y.values_by_level_and_potential[2][4]);

        // 加算系(requires_module)はFWには無い(乗算系のみ)。
        assert!(special.requires_module.is_none());
    }

    /// ウィーディ(char_400_weedy) S3「蓄水砲配置バフ」: 加算系(requires_module +
    /// add_self_atk_pct_by_module_level)で、モジュールX(uniequip_002_weedy)
    /// Lv1=0/Lv2=0.15/Lv3=0.20(P2 follow-up 2回目で実データから再導出)。
    #[test]
    fn weedy_s3_special_add_values_match_module_x_levels() {
        let result = build_from_seeds();
        let weedy = result.catalog.operators.iter().find(|op| op.name == "ウィーディ").expect("ウィーディがカタログに存在すること");
        let s3 = weedy.fk_entries.iter().find(|e| e.skill_num == "3").expect("ウィーディのS3が存在すること");
        let special = s3.special.as_ref().expect("ウィーディS3に特殊強化(蓄水砲配置バフ)があるはず");
        assert_eq!(special.requires_module.as_deref(), Some("uniequip_002_weedy"));
        assert_eq!(special.add_self_atk_pct_by_module_level, Some([0.0, 0.15, 0.2]));
    }

    /// 全角括弧(fk_dataシート表記)/半角括弧(ゲームデータ表記)の表記ゆれを吸収して
    /// 名前解決できること(step3で追加した`normalize_operator_name`のドリフト修正)。
    #[test]
    fn fullwidth_paren_operator_name_resolves() {
        let result = build_from_seeds();
        let found = result.catalog.operators.iter().any(|op| op.name.contains("アーミヤ"));
        assert!(found, "fk_dataの「アーミヤ（前衛）」(全角括弧)がoperator_combatの半角括弧表記へ解決できること。skipped={:?}", result.skipped);
        assert!(
            !result.skipped.iter().any(|n| n.contains("アーミヤ")),
            "アーミヤ(前衛)はもうskippedに残らないはず: {:?}",
            result.skipped
        );
    }

    /// ブレイズ(char_017_huang) S3: multiplierのoverrideを外した後もblackboardの
    /// `damage_by_atk_scale`(末尾一致ルール)経由でAutoの4.0が採れること。
    /// self_atk_pctはP2 follow-upで「バグ込みの実測値0.712固定」に変更したため、
    /// 常にManualの0.712になる(このケースの詳細は
    /// `blaze_s3_self_atk_pct_is_always_0_712_and_special_is_additive_module_gated`参照)。
    #[test]
    fn blaze_s3_multiplier_is_now_auto_via_suffix_rule() {
        let result = build_from_seeds();
        let blaze = result.catalog.operators.iter().find(|op| op.name == "ブレイズ").expect("ブレイズがカタログに存在すること");
        let s3 = blaze.fk_entries.iter().find(|e| e.skill_num == "3").expect("ブレイズのS3が存在すること");
        assert_eq!(s3.multiplier.value, 4.0);
        assert_eq!(s3.multiplier.source, dto::ValueSource::Auto, "multiplierのoverrideは撤去済みのはず");
    }

    /// ホルンS2の倍率候補一覧の並びが「選ばれたデフォルト(物理側)が先頭、残りはキー名
    /// アルファベット順」で安定していること(Seed/実fetchでのIndexMap挿入順の違いに
    /// 依存しないことの回帰テスト)。
    #[test]
    fn horn_s2_multiplier_candidates_are_deterministically_ordered() {
        let result = build_from_seeds();
        let horn = result.catalog.operators.iter().find(|op| op.name == "ホルン").expect("ホルンがカタログに存在すること");
        let s2 = horn.fk_entries.iter().find(|e| e.skill_num == "2").expect("ホルンのskill_num=2が存在すること");
        let keys: Vec<&str> = s2.multiplier_candidates.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, vec!["attack@s2.atk_scale", "attack@s2.magic_atk_scale"]);
    }

    /// ホルン(異格。char_4039_horn)S2「テンペストオーダー」は`durationType == "AMMO"`の
    /// 弾薬スキルなので、機械タグ"弾薬スキル"を持つこと(P2)。P10で物理/術は1エントリ
    /// (parts2件)にまとまったので、タグは統合後のエントリ1件が持つ。
    #[test]
    fn horn_s2_entries_are_tagged_ammo_skill() {
        let result = build_from_seeds();
        let horn = result.catalog.operators.iter().find(|op| op.name == "ホルン").expect("ホルンがカタログに存在すること");
        let s2_entries: Vec<_> = horn.fk_entries.iter().filter(|e| e.skill_num == "2").collect();
        assert_eq!(s2_entries.len(), 1, "P10でホルンのS2は物理+術の1エントリ(parts2件)にまとまるはず");
        for e in &s2_entries {
            assert!(e.tags.contains(&tags::AMMO_SKILL_TAG.to_string()), "ホルンS2/{:?}に弾薬スキルタグが無い: {:?}", e.variant_label, e.tags);
        }
    }

    /// フィアメッタ(char_300_phenxi、nationId=laterano)のカタログエントリが
    /// 勢力タグ"ラテラーノ"を持つこと(P2。「異格エクシア」バフのラテラーノ2倍判定に使う)。
    #[test]
    fn fiammetta_has_laterano_faction_tag() {
        let result = build_from_seeds();
        let fiammetta = result.catalog.operators.iter().find(|op| op.name == "フィアメッタ").expect("フィアメッタがカタログに存在すること");
        assert!(fiammetta.tags.contains(&"ラテラーノ".to_string()), "フィアメッタのタグにラテラーノが無い: {:?}", fiammetta.tags);
        for e in &fiammetta.fk_entries {
            assert!(e.tags.contains(&"ラテラーノ".to_string()), "フィアメッタのFkEntryタグにラテラーノが無い: {:?}", e.tags);
        }
    }

    /// P3: 濁心スカジの鼓舞ソースがカタログに載り、スキル比率・素質パーツが期待通りであること。
    #[test]
    fn skadi2_inspire_source_has_expected_skills_and_self_parts() {
        let result = build_from_seeds();
        let skadi2 = result.catalog.inspire_sources.iter().find(|s| s.id == "skadi2").expect("skadi2がinspire_sourcesに存在すること");
        assert_eq!(skadi2.operator_id, "char_1012_skadi2");
        let ratios: Vec<(String, f64)> = skadi2.skills.iter().map(|s| (s.skill_num.clone(), s.ratio)).collect();
        assert_eq!(ratios, vec![("2".to_string(), 0.6), ("3".to_string(), 1.1)]);

        let talent = skadi2.self_parts.iter().find(|p| p.id == "talent").expect("talentパーツがあるはず");
        assert!(talent.always_on);
        assert_eq!(talent.pct, 0.06);
        assert_eq!(talent.pct_potential_bonus, 0.03);
        let module_override = talent.module_override.as_ref().expect("talentにmodule_overrideがあるはず");
        assert_eq!(module_override.pct_by_level, [0.06, 0.08, 0.09]);
        assert_eq!(module_override.potential_bonus_by_level, [0.03, 0.03, 0.03]);

        let abyssal = skadi2.self_parts.iter().find(|p| p.id == "talent_abyssal").expect("talent_abyssalパーツがあるはず");
        assert_eq!(abyssal.replaces.as_deref(), Some("talent"));
        assert_eq!(abyssal.pct, 0.15);

        let module_x = skadi2.self_parts.iter().find(|p| p.id == "module_x_two_ops").expect("module_x_two_opsパーツがあるはず");
        assert_eq!(module_x.requires_module.as_deref(), Some("uniequip_002_skadi2"));
        assert_eq!(module_x.pct_by_module_level, Some([0.08, 0.08, 0.08]));
    }

    /// buffers.yamlのinspireリストが実データ(operator_combat+operator_data)と
    /// 整合していることのドリフト検知。ゲームデータ更新でオペレーターID・skill_num・
    /// モジュールIDが変わった場合、このテストが不一致キーを列挙して落ちる。
    #[test]
    fn every_inspire_source_points_to_existing_operator_skill_and_modules() {
        let combat: OperatorCombat = load_seed(operator_combat::SEED_PATH);
        let ops: OperatorData = load_seed(operator_data::SEED_PATH);
        let skills: SkillData = load_seed(skill_data::SEED_PATH);
        let bad = validate_inspire_sources(buffers::raw_inspire_sources(), &combat, &ops, &skills);
        assert!(bad.is_empty(), "buffers.yamlのinspireリストに実データと不一致な参照がある:\n{}", bad.join("\n"));
    }

    /// P4: buffers.yamlのconditional(source付き)が実データ(operator_combat/operator_data/
    /// skill_data)と整合していることのドリフト検知。ゲームデータ更新でオペレーターID・
    /// talentIndex・skill_num・キー名が変わった場合、このテストが不一致キーを列挙して落ちる。
    #[test]
    fn every_conditional_source_points_to_existing_operator_talent_or_skill() {
        let ops: OperatorData = load_seed(operator_data::SEED_PATH);
        let combat: OperatorCombat = load_seed(operator_combat::SEED_PATH);
        let skills: SkillData = load_seed(skill_data::SEED_PATH);
        let bad = conditional_source::validate_conditional_sources(&combat, &ops, &skills);
        assert!(bad.is_empty(), "buffers.yamlのconditional(source付き)に実データと不一致な参照がある:\n{}", bad.join("\n"));
    }

    /// P5: buffers.yamlのindividual(source付き)が実データ(operator_combat/operator_data/
    /// skill_data)と整合していることのドリフト検知。ゲームデータ更新でオペレーターID・
    /// talentIndex・skill_num・skill_id・キー名が変わった場合、このテストが不一致キーを
    /// 列挙して落ちる。
    #[test]
    fn every_individual_source_points_to_existing_operator_talent_or_skill() {
        let ops: OperatorData = load_seed(operator_data::SEED_PATH);
        let combat: OperatorCombat = load_seed(operator_combat::SEED_PATH);
        let skills: SkillData = load_seed(skill_data::SEED_PATH);
        let bad = conditional_source::validate_individual_sources(&combat, &ops, &skills);
        assert!(bad.is_empty(), "buffers.yamlのindividual(source付き)に実データと不一致な参照がある:\n{}", bad.join("\n"));
    }

    /// P5: オーナー確認済みの実データ値(2026-09時点)と一致すること。ゲームデータ更新で
    /// 数値が変わった場合はここを見直す(デフォルト値は「最大成長」設定。非デフォルトの
    /// 値もいくつか併せて検証する)。
    #[test]
    fn individual_source_default_and_selected_values_match_verified_gamedata() {
        let result = build_from_seeds();
        let find = |id: &str| result.catalog.buffers.iter().find(|b| b.id == id).unwrap_or_else(|| panic!("バフ'{id}'がカタログに無い"));
        let approx_eq = |a: f64, b: f64| (a - b).abs() < 1e-9;

        let plasma = find("plasma");
        assert!(approx_eq(plasma.value, 0.90), "plasmaのデフォルト値={}", plasma.value);
        assert!(plasma.single_target, "plasmaはsingle_targetのはず");

        let durian = find("durian");
        assert!(approx_eq(durian.value, 0.50), "durianのデフォルト値={}", durian.value);
        assert!(durian.single_target, "durianはsingle_targetのはず");

        let swire_s1 = find("swire_s1");
        assert!(approx_eq(swire_s1.value, 0.24), "swire_s1のデフォルト値={}", swire_s1.value);
        let swire_s1_talent = &swire_s1.source.as_ref().unwrap().talent.as_ref().unwrap();
        // E2・潜在1(potentialRank0)= 0.10(talent) × 2.0(scale) = 0.20。
        assert!(approx_eq(swire_s1_talent.values_by_elite_and_potential[2][0], 0.10), "swire_s1talentのE2潜在1={}", swire_s1_talent.values_by_elite_and_potential[2][0]);
        let swire_s1_scale = swire_s1.source.as_ref().unwrap().scale.as_ref().unwrap();
        assert!(!swire_s1_scale.varies, "swire_s1のscale(talent_scale)は全レベル2.0固定でvaries=falseのはず");
        assert!(approx_eq(swire_s1_scale.values_by_level[0], 2.0), "swire_s1のscale値={}", swire_s1_scale.values_by_level[0]);

        let swire_s2 = find("swire_s2");
        assert!(approx_eq(swire_s2.value, 0.36), "swire_s2のデフォルト値={}", swire_s2.value);
        let swire_s2_scale = swire_s2.source.as_ref().unwrap().scale.as_ref().unwrap();
        assert!(swire_s2_scale.varies, "swire_s2のscale(talent_scale)は2.1〜3.0で変化するのでvaries=trueのはず");

        let stainless = find("stainless_s1");
        assert!(approx_eq(stainless.value, 0.48), "stainless_s1のデフォルト値={}", stainless.value);
        assert!(approx_eq(stainless.source.as_ref().unwrap().base_pct.unwrap(), 0.12), "stainless_s1のbase_pct");
        let stainless_scale = stainless.source.as_ref().unwrap().scale.as_ref().unwrap();
        // SLv7(0-indexed6)は0.12×3=0.36。
        assert!(approx_eq(stainless_scale.values_by_level[6], 3.0), "stainless_s1のscale[SLv7]={}", stainless_scale.values_by_level[6]);
        let toggle = stainless.toggle.as_ref().expect("stainless_s1はtoggle(装置2台)を持つはず");
        assert_eq!(toggle.mult, 2.0);

        let nasty = find("nasty_s3");
        assert!(approx_eq(nasty.value, 0.60), "nasty_s3のデフォルト値={}", nasty.value);
        let stage = nasty.source.as_ref().unwrap().stage.as_ref().expect("nasty_s3はstageソースのはず");
        assert!(approx_eq(stage.values[0], 0.20), "nasty_s3の1段階目={}", stage.values[0]);
        assert_eq!(stage.labels, vec!["1段階".to_string(), "2段階".to_string(), "3段階".to_string()]);

        let sprria = find("sprria_s2");
        assert!(approx_eq(sprria.value, 0.30), "sprria_s2のデフォルト値={}", sprria.value);
        assert!(sprria.single_target, "sprria_s2はsingle_targetのはず");

        let exusiai = find("exusiai");
        assert!(approx_eq(exusiai.value, 0.10), "exusiaiのデフォルト値={}", exusiai.value);
        let exusiai_talent = exusiai.source.as_ref().unwrap().talent.as_ref().unwrap();
        // E0/E1は候補が無いので0(素質は昇進2でのみ解放)。
        assert!(approx_eq(exusiai_talent.values_by_elite_and_potential[0][5], 0.0), "exusiaiのE0潜在6={}", exusiai_talent.values_by_elite_and_potential[0][5]);
        let exusiai_module = exusiai_talent.modules.iter().find(|m| m.module_id == "uniequip_002_angel").expect("exusiaiのモジュールXがあるはず");
        // モジュールX Lv3(index2)・潜在1(potentialRank0)= 0.08。
        assert!(approx_eq(exusiai_module.values_by_level_and_potential[2][0], 0.08), "exusiaiのXLv3潜在1={}", exusiai_module.values_by_level_and_potential[2][0]);
        let max_targets = exusiai.source.as_ref().unwrap().max_targets_by_module.as_ref().expect("exusiaiはmax_targets_by_moduleを持つはず");
        assert_eq!(max_targets.module_id, "uniequip_002_angel");
        assert_eq!(max_targets.min_level, 2);
        assert_eq!(max_targets.count, 2);
    }

    /// P4: オーナー確認済みの実データ値(2026-09時点)と一致すること。ゲームデータ更新で
    /// 数値が変わった場合はここを見直す。
    #[test]
    fn conditional_source_default_values_match_verified_gamedata() {
        let result = build_from_seeds();
        let find = |id: &str| result.catalog.buffers.iter().find(|b| b.id == id).unwrap_or_else(|| panic!("バフ'{id}'がカタログに無い"));

        let approx_eq = |a: f64, b: f64| (a - b).abs() < 1e-9;

        let castle3 = find("castle3");
        assert!(approx_eq(castle3.value, 0.20), "castle3のデフォルト値={}", castle3.value);

        let zima = find("zima");
        assert!(approx_eq(zima.value, 0.60), "zimaのデフォルト値={}", zima.value);

        let aya = find("aya");
        assert!(approx_eq(aya.value, 0.24), "ayaのデフォルト値={}", aya.value);

        let podenco = find("podenco");
        assert!(approx_eq(podenco.value, 0.11), "podencoのデフォルト値={}", podenco.value);
        assert!(
            podenco.source.as_ref().unwrap().talent.as_ref().unwrap().modules.is_empty(),
            "podencoはモジュールを装備しても値が変わらないのでmodules軸が空のはず: {:?}",
            podenco.source.as_ref().unwrap().talent.as_ref().unwrap().modules
        );

        let pepe = find("pepe");
        assert!(approx_eq(pepe.value, 0.20), "pepeのデフォルト値={}", pepe.value);
        assert!(
            pepe.source.as_ref().unwrap().talent.as_ref().unwrap().modules.is_empty(),
            "pepeはモジュールを装備しても値が変わらないのでmodules軸が空のはず"
        );

        // P9: ホルン(異格。char_4039_horn)素質「軍事要塞」。E1(.10/潜在3で.13)/
        // E2(.20/潜在3で.23)、モジュールX(uniequip_002_horn)Lv2(.25/潜在3で.28)/
        // Lv3(.28/潜在3で.31)。デフォルト(最大成長)は.31(旧overrides.yamlの固定
        // self_atk_pctと一致する)。
        let horn = find("horn");
        assert!(approx_eq(horn.value, 0.31), "hornのデフォルト値={}", horn.value);
        match &horn.scope {
            dto::BufferScope::Conditional { target_tags } => assert_eq!(target_tags, &vec!["重装".to_string()]),
            _ => panic!("hornはconditionalスコープのはず"),
        }
        let horn_talent = horn.source.as_ref().unwrap().talent.as_ref().unwrap();
        assert!(approx_eq(horn_talent.values_by_elite_and_potential[1][0], 0.10), "hornのE1潜在1={}", horn_talent.values_by_elite_and_potential[1][0]);
        assert!(approx_eq(horn_talent.values_by_elite_and_potential[1][2], 0.13), "hornのE1潜在3={}", horn_talent.values_by_elite_and_potential[1][2]);
        assert!(approx_eq(horn_talent.values_by_elite_and_potential[2][0], 0.20), "hornのE2潜在1={}", horn_talent.values_by_elite_and_potential[2][0]);
        assert!(approx_eq(horn_talent.values_by_elite_and_potential[2][2], 0.23), "hornのE2潜在3={}", horn_talent.values_by_elite_and_potential[2][2]);
        let horn_module = horn_talent.modules.iter().find(|m| m.module_id == "uniequip_002_horn").expect("hornのモジュールXがあるはず");
        assert!(approx_eq(horn_module.values_by_level_and_potential[1][0], 0.25), "hornのモジュールXLv2潜在1={}", horn_module.values_by_level_and_potential[1][0]);
        assert!(approx_eq(horn_module.values_by_level_and_potential[1][2], 0.28), "hornのモジュールXLv2潜在3={}", horn_module.values_by_level_and_potential[1][2]);
        assert!(approx_eq(horn_module.values_by_level_and_potential[2][0], 0.28), "hornのモジュールXLv3潜在1={}", horn_module.values_by_level_and_potential[2][0]);
        assert!(approx_eq(horn_module.values_by_level_and_potential[2][2], 0.31), "hornのモジュールXLv3潜在3={}", horn_module.values_by_level_and_potential[2][2]);

        let amiya = find("amiya_guard");
        assert!(approx_eq(amiya.value, 0.09), "amiya_guardのデフォルト値={}", amiya.value);
        assert!(amiya.toggle.is_some(), "amiya_guardはtoggle(スキル中2倍)を持つはず");
        assert_eq!(amiya.toggle.as_ref().unwrap().mult, 2.0);

        let suzuran = find("suzuran");
        assert!(approx_eq(suzuran.value, 0.09), "suzuranのデフォルト値={}", suzuran.value);

        let exusiai_alter = find("exusiai_alter");
        assert!(approx_eq(exusiai_alter.value, 0.13), "exusiai_alterのデフォルト値={}", exusiai_alter.value);
        let bonus = exusiai_alter.bonus.as_ref().expect("exusiai_alterにbonusがあるはず");
        assert_eq!(bonus.mult, Some(2.0));
        // bonus値自体(基本値×倍率)はフロント側(engine.js)の責務なので、ここではmultの
        // 存在と基本値.13だけを確認する(.13×2=.26になることはJS側のverify.mjsで検証する)。
    }

    /// P7: Ashの300/400/800%は`multiplier_key`でスキルLv別に追従すること
    /// (SLv1[index0]〜特化3[index9]、L1/L10の実データ値を確認する)。
    #[test]
    fn ash_s3_multiplier_follows_skill_level_via_multiplier_key() {
        let result = build_from_seeds();
        let ash = result.catalog.operators.iter().find(|op| op.name == "Ash").expect("Ashがカタログに存在すること");
        let by_label = |label: &str| ash.fk_entries.iter().find(|e| e.skill_num == "3" && e.variant_label.as_deref() == Some(label)).unwrap();

        let v300 = by_label("300%");
        assert_eq!(v300.multiplier.source, dto::ValueSource::Manual);
        assert!(!v300.multiplier_fixed, "multiplier_key指定はスキルLvに追従するのでfixed=falseのはず");
        assert_eq!(v300.multiplier_by_level.first().copied(), Some(2.0), "300%のSLv1");
        assert_eq!(v300.multiplier_by_level.last().copied(), Some(3.0), "300%の特化3");
        assert_eq!(v300.multiplier.value, 3.0, "後方互換のmultiplier.valueは特化3の値のはず");

        let v400 = by_label("400%");
        assert_eq!(v400.multiplier_by_level.first().copied(), Some(3.0), "400%のSLv1");
        assert_eq!(v400.multiplier_by_level.last().copied(), Some(4.0), "400%の特化3");

        let v800 = by_label("800%");
        assert_eq!(v800.multiplier_by_level.first().copied(), Some(6.0), "800%のSLv1");
        assert_eq!(v800.multiplier_by_level.last().copied(), Some(8.0), "800%の特化3");
    }

    /// P7: ホルンS2の物理/術パーツは`multiplier_key`でスキルLv別に追従する。P9: 固定
    /// self_atk_pct(0.31)はconditionalバフ「horn」(素質「軍事要塞」)へ移設したため、
    /// self_atk_pctはAuto=0になる(ホルンS2のskill_table.jsonのblackboardに"atk"キーが
    /// 無いため。実データ確認済み)。旧0.31相当は
    /// `conditional_source_default_values_match_verified_gamedata`の`horn`バフで検証する。
    /// P10: 物理/術は1エントリ・parts2件にまとまり、トップレベル(=パーツ0)は物理の値を持つ。
    #[test]
    fn horn_s2_multiplier_follows_skill_level_and_self_atk_pct_is_auto_zero() {
        let result = build_from_seeds();
        let horn = result.catalog.operators.iter().find(|op| op.name == "ホルン").expect("ホルンがカタログに存在すること");
        let s2 = horn.fk_entries.iter().find(|e| e.skill_num == "2").expect("ホルンのS2が存在すること");
        assert_eq!(s2.variant_label.as_deref(), Some("物理+術"));
        assert_eq!(s2.parts.len(), 2, "ホルンS2は物理+術の2パーツのはず");

        // トップレベル(=パーツ0)は物理の値。
        assert_eq!(s2.multiplier_by_level.first().copied(), Some(1.3));
        assert_eq!(s2.multiplier_by_level.last().copied(), Some(2.4));
        assert_eq!(s2.damage_type.value, dto::DamageType::Physical);
        assert_eq!(s2.default_module.as_deref(), Some("uniequip_002_horn"));
        assert_eq!(s2.self_atk_pct.source, dto::ValueSource::Auto, "self_atk_pctのoverrideは撤去済み(P9でバフ側へ移設)のはずAuto");
        assert_eq!(s2.self_atk_pct.value, 0.0);
        assert!(!s2.self_atk_pct_fixed, "Auto(non-override)なのでfixed=falseのはず");
        assert!(s2.self_atk_pct_by_level.iter().all(|v| *v == 0.0), "atkキーが無いのでスキルLv別配列も全て0のはず");

        let physical = &s2.parts[0];
        assert_eq!(physical.label, "物理");
        assert_eq!(physical.multiplier_by_level.first().copied(), Some(1.3));
        assert_eq!(physical.multiplier_by_level.last().copied(), Some(2.4));
        assert_eq!(physical.damage_type.value, dto::DamageType::Physical);
        assert_eq!(s2.hits.value, 5, "Hit数は全パーツ共通でFkEntry.hits(先頭バリアント)に持つ");

        let arts = &s2.parts[1];
        assert_eq!(arts.label, "術");
        assert_eq!(arts.multiplier_by_level.first().copied(), Some(0.3));
        assert_eq!(arts.multiplier_by_level.last().copied(), Some(0.6));
        assert_eq!(arts.damage_type.value, dto::DamageType::Arts);
    }

    /// P10: overrides.yamlの`combined`(P10)が正しく使われていることのドリフト検知。
    #[test]
    fn every_combined_variant_group_is_consistent() {
        let bad = validate_combined_variants(Overrides::global());
        assert!(bad.is_empty(), "overrides.yamlのcombinedバリアントに問題がある:\n{}", bad.join("\n"));
    }

    /// P7: ブレイズS3のself_atk_pct_factor(0.89)がAutoのスキルレベル別セルフ%(atk)に
    /// 掛かり、SLv7(L7)=0.534・特化3(L10)=0.712になること(浮動小数ドリフトは
    /// round4で吸収する)。
    #[test]
    fn blaze_s3_self_atk_pct_factor_follows_skill_level() {
        let result = build_from_seeds();
        let blaze = result.catalog.operators.iter().find(|op| op.name == "ブレイズ").expect("ブレイズがカタログに存在すること");
        let s3 = blaze.fk_entries.iter().find(|e| e.skill_num == "3").expect("ブレイズのS3が存在すること");
        assert!(!s3.self_atk_pct_fixed, "self_atk_pct_factor指定はスキルLvに追従するのでfixed=falseのはず");
        assert_eq!(s3.self_atk_pct_by_level.len(), 10);
        assert_eq!(s3.self_atk_pct_by_level[6], 0.534, "SLv7(index6)=0.6×0.89=0.534のはず");
        assert_eq!(s3.self_atk_pct_by_level[9], 0.712, "特化3(index9)=0.8×0.89=0.712のはず");
        assert_eq!(s3.self_atk_pct.value, 0.712, "後方互換のself_atk_pct.valueは特化3の値のはず");
    }

    /// P7: ファイヤーウォッチS2はAuto(override無し)のままだが、blackboardのatk_scaleが
    /// スキルLv別(SLv1=1.8〜特化3=3.0)に追従すること。
    #[test]
    fn fw_s2_multiplier_by_level_is_auto_and_follows_skill_level() {
        let result = build_from_seeds();
        let fw = result.catalog.operators.iter().find(|op| op.name == "ファイヤーウォッチ").expect("ファイヤーウォッチがカタログに存在すること");
        let s2 = fw.fk_entries.iter().find(|e| e.skill_num == "2").expect("ファイヤーウォッチのS2が存在すること");
        assert_eq!(s2.multiplier.source, dto::ValueSource::Auto);
        assert_eq!(s2.multiplier_by_level.first().copied(), Some(1.8), "SLv1");
        assert_eq!(s2.multiplier_by_level.last().copied(), Some(3.0), "特化3");
    }

    /// P7: 濁心スカジの鼓舞ソースはratio_keyでスキルLv別に追従し、特化3(L10)の値が
    /// P3時点の実測値(S2=0.6/S3=1.1)と一致すること(後方互換のratioフィールド)。
    #[test]
    fn skadi2_inspire_ratio_follows_skill_level() {
        let result = build_from_seeds();
        let skadi2 = result.catalog.inspire_sources.iter().find(|s| s.id == "skadi2").expect("skadi2がinspire_sourcesに存在すること");
        let s2 = skadi2.skills.iter().find(|s| s.skill_num == "2").unwrap();
        assert!(!s2.ratio_fixed);
        assert_eq!(s2.ratio_by_level.first().copied(), Some(0.15));
        assert_eq!(s2.ratio_by_level.last().copied(), Some(0.6));
        assert_eq!(s2.ratio, 0.6);

        let s3 = skadi2.skills.iter().find(|s| s.skill_num == "3").unwrap();
        assert!(!s3.ratio_fixed);
        assert_eq!(s3.ratio_by_level.first().copied(), Some(0.5));
        assert_eq!(s3.ratio_by_level.last().copied(), Some(1.1));
        assert_eq!(s3.ratio, 1.1);
    }

    /// P7: overrides.yamlの`multiplier_key`/`self_atk_pct_factor`が実データ(blackboard)と
    /// 整合していることのドリフト検知。ゲームデータ更新でキー名が変わった場合、
    /// このテストが不一致キーを列挙して落ちる。
    #[test]
    fn every_override_level_field_points_to_an_existing_blackboard_key() {
        let ops: OperatorData = load_seed(operator_data::SEED_PATH);
        let skills: SkillData = load_seed(skill_data::SEED_PATH);
        let bad = validate_override_level_fields(Overrides::global(), &ops, &skills);
        assert!(bad.is_empty(), "overrides.yamlのmultiplier_key/self_atk_pct_factorが実データと不一致:\n{}", bad.join("\n"));
    }

    /// カバレッジ確認用(fk_dataの何件がカタログに解決できたか)。`--nocapture`で確認する。
    #[test]
    fn coverage_report() {
        let result = build_from_seeds();
        let resolved = result.catalog.operators.len();
        let skipped = result.skipped.len();
        println!("fk_kill_calc coverage: resolved={resolved} skipped={skipped} skipped_names={:?}", result.skipped);
        assert!(resolved > 0);
    }
}
