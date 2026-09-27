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
use crate::engine::external_source::operator_combat::OperatorCombat;
use crate::engine::external_source::operator_data::OperatorData;
use crate::engine::external_source::skill_data::SkillData;
use crate::engine::fk_data_search::search::skill_id_by_num;
use buffers::RawInspireSource;
use dto::{
    CatalogModule, CatalogOperator, DamageType, FkEntry, InspireModuleOverride, InspireSelfPart, InspireSkillRatio, InspireSource,
    MulMultiplier, PhaseAtk, Special, Valued,
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
            let skill_label = skill_id
                .map(|id| skills.get_str(id))
                .filter(|s| !s.is_empty() && *s != "Missing")
                .map(str::to_string)
                .unwrap_or_else(|| row.skill_num.clone());

            let (default_multiplier, multiplier_candidates) = resolve_multiplier_defaults(blackboard);
            let default_self_atk_pct = blackboard.and_then(|bb| bb.get("atk")).copied().unwrap_or(0.0);
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
                Some(variants) if !variants.is_empty() => {
                    for variant in variants {
                        let mut tags_for_variant = entry_tags.clone();
                        if let Some(extra) = &variant.tags {
                            tags_for_variant.extend(extra.iter().cloned());
                        }
                        fk_entries.push(build_entry(
                            row,
                            skill_label.clone(),
                            variant.label.clone(),
                            Valued::from_override(variant.multiplier, default_multiplier),
                            multiplier_candidates.clone(),
                            Valued::from_override(variant.self_atk_pct, default_self_atk_pct),
                            Valued::from_override(variant.hits, default_hits),
                            Valued::from_override(variant.damage_type, default_damage_type),
                            tags_for_variant,
                            variant.special.as_ref().map(to_special_dto),
                            variant.note.clone(),
                        ));
                    }
                }
                _ => {
                    fk_entries.push(build_entry(
                        row,
                        skill_label.clone(),
                        None,
                        Valued::auto(default_multiplier),
                        multiplier_candidates,
                        Valued::auto(default_self_atk_pct),
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
            modules: to_catalog_modules(&combat_op.modules),
            fk_entries,
            phases: to_phase_atk_dtos(&combat_op.phases),
            atk_trust_max: combat_op.atk_trust_max,
            skill_unlock_phase: combat_op.skill_unlock_phase.clone(),
        });
    }

    let inspire_sources = build_inspire_sources(combat, &mut skipped);

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
fn build_inspire_sources(combat: &OperatorCombat, skipped: &mut Vec<String>) -> Vec<InspireSource> {
    let mut sources = Vec::new();
    for raw in buffers::raw_inspire_sources() {
        let Some(op) = combat.operators.get(&raw.operator) else {
            skipped.push(format!("inspire:{}", raw.id));
            continue;
        };
        let tags = tags::tags_for(&op.profession, &op.position, &op.nation_id);
        sources.push(InspireSource {
            id: raw.id.clone(),
            operator_id: raw.operator.clone(),
            name: raw.name.clone(),
            tags,
            atk_base: op.atk_base,
            atk_potential: op.atk_potential,
            modules: to_catalog_modules(&op.modules),
            skills: raw.skills.iter().map(|s| InspireSkillRatio { skill_num: s.skill_num.clone(), ratio: s.ratio }).collect(),
            talent_potential_label: raw.talent_potential_label.clone(),
            self_parts: raw.self_parts.iter().map(to_inspire_self_part_dto).collect(),
            phases: to_phase_atk_dtos(&op.phases),
            atk_trust_max: op.atk_trust_max,
            skill_unlock_phase: op.skill_unlock_phase.clone(),
        });
    }
    sources
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
pub fn validate_inspire_sources(sources: &[RawInspireSource], combat: &OperatorCombat, ops: &OperatorData) -> Vec<String> {
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
                    if !skill_ids.contains_key(sk.skill_num.as_str()) {
                        bad.push(format!("inspire:{} (skill_num'{}'が'{}'の実スキルに無い)", s.id, sk.skill_num, s.operator));
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

/// `overrides.yaml`の`special`(手動データ)をそのままDTO(`dto::Special`)へ変換する。
fn to_special_dto(special: &OverrideSpecial) -> Special {
    Special {
        label: special.label.clone(),
        description: special.description.clone(),
        requires_module: special.requires_module.clone(),
        add_self_atk_pct_by_module_level: special.add_self_atk_pct_by_module_level,
        mul_multiplier: special.mul_multiplier.as_ref().map(|m| MulMultiplier {
            base: m.base,
            module: m.module.clone(),
            by_module_level: m.by_module_level,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_entry(
    row: &FkSheetRow,
    skill_label: String,
    variant_label: Option<String>,
    multiplier: Valued<f64>,
    multiplier_candidates: Vec<(String, f64)>,
    self_atk_pct: Valued<f64>,
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
        multiplier_candidates,
        self_atk_pct,
        hits,
        damage_type,
        tags,
        special,
        note,
    }
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
fn resolve_multiplier_defaults(blackboard: Option<&IndexMap<String, f64>>) -> (f64, Vec<(String, f64)>) {
    let Some(blackboard) = blackboard else {
        return (1.0, Vec::new());
    };
    let mut candidates: Vec<(String, f64)> =
        blackboard.iter().filter(|(k, _)| k.contains("scale")).map(|(k, v)| (k.clone(), *v)).collect();
    candidates.sort_by(|(a, _), (b, _)| a.cmp(b));

    let chosen_key: Option<&str> = if blackboard.contains_key("atk_scale") {
        Some("atk_scale")
    } else {
        candidates
            .iter()
            .map(|(k, _)| k.as_str())
            .find(|k| k.ends_with("atk_scale") || k.ends_with("damage_scale"))
    };

    let default_multiplier = match chosen_key {
        Some(key) => blackboard.get(key).copied().unwrap_or(1.0),
        None => 1.0,
    };

    // 選ばれたキーを候補一覧の先頭へ動かす(既にアルファベット順ソート済みなので、
    // それ以外の並びは変えない = 「選ばれたもの優先、残りはアルファベット順」)。
    if let Some(key) = chosen_key {
        if let Some(pos) = candidates.iter().position(|(k, _)| k == key) {
            let picked = candidates.remove(pos);
            candidates.insert(0, picked);
        }
    }

    (default_multiplier, candidates)
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
    let check_module_id = |op_id: &str, skill_num: &str, module_id: &str, field: &str, bad: &mut Vec<String>| {
        let Some(op) = combat.operators.get(op_id) else {
            bad.push(format!("{op_id}/{skill_num} (operator idがoperator_combatに無い)"));
            return;
        };
        if !op.modules.iter().any(|m| m.eq_id == module_id) {
            bad.push(format!("{op_id}/{skill_num} ({field}'{module_id}'が'{}'のmodulesに無い)", op.name));
        }
    };

    for (op_id, skill_num) in overrides.all_keys() {
        let Some(variants) = overrides.variants_for(op_id, skill_num) else { continue };
        for variant in variants {
            let Some(special) = &variant.special else { continue };
            if let Some(module_id) = &special.requires_module {
                check_module_id(op_id, skill_num, module_id, "special.requires_module", &mut bad);
            }
            if let Some(mul) = &special.mul_multiplier {
                if let Some(module_id) = &mul.module {
                    check_module_id(op_id, skill_num, module_id, "special.mul_multiplier.module", &mut bad);
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

    /// ファイヤーウォッチ(char_158_milu) S2「遠距離特効」: 乗算系(mul_multiplier)で、
    /// モジュールY(uniequip_002_milu)未装備時はbase=1.45、Lv1=1.45/Lv2=1.5/Lv3=1.55
    /// (P2 follow-up 2回目で追加)。
    #[test]
    fn fw_s2_special_uses_mul_multiplier_with_module_y_levels() {
        let result = build_from_seeds();
        let fw = result.catalog.operators.iter().find(|op| op.name == "ファイヤーウォッチ").expect("ファイヤーウォッチがカタログに存在すること");
        let s2 = fw.fk_entries.iter().find(|e| e.skill_num == "2").expect("ファイヤーウォッチのS2が存在すること");
        let special = s2.special.as_ref().expect("ファイヤーウォッチS2に特殊強化(遠距離特効)があるはず");
        let mul = special.mul_multiplier.as_ref().expect("mul_multiplierがあるはず");
        assert_eq!(mul.base, 1.45);
        assert_eq!(mul.module.as_deref(), Some("uniequip_002_milu"));
        assert_eq!(mul.by_module_level, Some([1.45, 1.5, 1.55]));
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
        let keys: Vec<&str> = s2.multiplier_candidates.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["attack@s2.atk_scale", "attack@s2.magic_atk_scale"]);
    }

    /// ホルン(異格。char_4039_horn)S2「テンペストオーダー」は`durationType == "AMMO"`の
    /// 弾薬スキルなので、両バリアントとも機械タグ"弾薬スキル"を持つこと(P2)。
    #[test]
    fn horn_s2_entries_are_tagged_ammo_skill() {
        let result = build_from_seeds();
        let horn = result.catalog.operators.iter().find(|op| op.name == "ホルン").expect("ホルンがカタログに存在すること");
        let s2_entries: Vec<_> = horn.fk_entries.iter().filter(|e| e.skill_num == "2").collect();
        assert_eq!(s2_entries.len(), 2, "ホルンのS2は物理/術の2バリアントのはず");
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
        let bad = validate_inspire_sources(buffers::raw_inspire_sources(), &combat, &ops);
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
