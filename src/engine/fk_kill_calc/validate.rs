//! 手動補正と鼓舞ソースがゲームデータと合っているかの検証。

use crate::engine::external_source::fk_data::FkSheetData;
use crate::engine::external_source::operator_combat::OperatorCombat;
use crate::engine::external_source::operator_data::OperatorData;
use crate::engine::external_source::skill_data::SkillData;
use crate::engine::fk_data_search::search::skill_id_by_num;
use super::buffers::RawInspireSource;
use std::collections::HashMap;
use super::conditional_source;
use super::{normalize_operator_name, Overrides};


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

/// overrides.yamlの`stone_board`が実データに解決でき、どのキーも最大成長で0より大きいこと、
/// `combined`と併用していないことを検証する(キー名のスペルミスは`build_talent_source`が
/// 0にフォールバックするだけで`None`にならないので、値で見る)。
pub fn validate_stone_board(overrides: &Overrides, combat: &OperatorCombat) -> Vec<String> {
    let mut bad = Vec::new();
    for (op_id, skill_num) in overrides.all_keys() {
        let Some(variants) = overrides.variants_for(op_id, skill_num) else { continue };
        for variant in variants {
            let Some(board) = &variant.stone_board else { continue };
            if variant.combined {
                bad.push(format!("{op_id}/{skill_num} (stone_boardはcombinedと併用できない)"));
            }
            let Some(op) = combat.operators.get(op_id) else {
                bad.push(format!("{op_id}/{skill_num} (operator idがoperator_combatに無い)"));
                continue;
            };
            for key in [&board.damage_per_stack_key, &board.res_pen_per_stack_key, &board.max_stacks_key] {
                match conditional_source::build_talent_source(op, board.talent, key) {
                    Some(table) if table.values_by_elite_and_potential[2][5] > 0.0 => {}
                    Some(_) => bad.push(format!("{op_id}/{skill_num} (stone_boardのtalent[{}]/key'{key}'が最大成長でも値0以下)", board.talent)),
                    None => bad.push(format!("{op_id}/{skill_num} (stone_boardのtalent[{}]が無い)", board.talent)),
                }
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
