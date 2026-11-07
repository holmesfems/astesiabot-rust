use super::*;
use crate::engine::external_source::fk_data::FkSheetData;
use crate::engine::external_source::operator_combat::OperatorCombat;
use crate::engine::external_source::operator_data::OperatorData;
use crate::engine::external_source::skill_data::SkillData;
use super::validate::{
    validate_inspire_sources,
    validate_overrides,
    validate_special_requires_module,
    validate_special_mul_multiplier,
    validate_stone_board,
    validate_override_level_fields,
    validate_combined_variants,
};
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

#[test]
fn every_stone_board_points_to_an_existing_talent_and_keys() {
    let combat: OperatorCombat = load_seed(operator_combat::SEED_PATH);
    let bad = validate_stone_board(Overrides::global(), &combat);
    assert!(bad.is_empty(), "overrides.yamlのstone_boardが実データと不一致:\n{}", bad.join("\n"));
}

/// ウァン S3 の碁石盤: 第二素質(昇進2で解放)の値が昇進・潜在・モジュールX(Lv1は素質強化なし)で
/// 正しく引けること。値は operator_combat の Seed と Wiki の表で確認済み。
#[test]
fn wan_s3_stone_board_uses_second_talent_tables() {
    let result = build_from_seeds();
    let wan = result.catalog.operators.iter().find(|op| op.name == "ウァン").expect("ウァンがカタログに存在すること");
    let s3 = wan.fk_entries.iter().find(|e| e.skill_num == "3").expect("ウァンのS3が存在すること");
    assert_eq!(s3.damage_type.value, dto::DamageType::Arts);
    assert!((s3.multiplier.value - 3.8).abs() < 1e-9, "倍率={}", s3.multiplier.value);
    let board = s3.stone_board.as_ref().expect("S3に碁石盤があるはず");
    assert!(wan.fk_entries.iter().filter(|e| e.skill_num != "3").all(|e| e.stone_board.is_none()), "S3以外に碁石盤は無い");

    let approx_eq = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let module_id = "uniequip_002_wang";
    let damage = &board.damage_per_stack;
    assert!(approx_eq(damage.values_by_elite_and_potential[1][5], 0.0), "昇進1では未解放");
    assert!(approx_eq(damage.values_by_elite_and_potential[2][0], 0.10));
    assert!(approx_eq(damage.values_by_elite_and_potential[2][5], 0.12));
    let x = damage.modules.iter().find(|m| m.module_id == module_id).expect("モジュールXで増加率が変わる");
    assert!(approx_eq(x.values_by_level_and_potential[0][0], 0.10), "Lv1は素質強化なし");
    assert!(approx_eq(x.values_by_level_and_potential[1][0], 0.12));
    assert!(approx_eq(x.values_by_level_and_potential[2][0], 0.13));
    assert!(approx_eq(x.values_by_level_and_potential[2][5], 0.15));

    let res_pen = &board.res_pen_per_stack;
    assert!(approx_eq(res_pen.values_by_elite_and_potential[2][0], 9.0));
    assert!(approx_eq(res_pen.values_by_elite_and_potential[2][5], 10.0));
    let x = res_pen.modules.iter().find(|m| m.module_id == module_id).expect("モジュールXで貫通が変わる");
    assert!(approx_eq(x.values_by_level_and_potential[1][0], 11.0));
    assert!(approx_eq(x.values_by_level_and_potential[2][0], 12.0));
    assert!(approx_eq(x.values_by_level_and_potential[2][5], 13.0));

    assert!(approx_eq(board.max_stacks.values_by_elite_and_potential[1][5], 0.0));
    assert!(approx_eq(board.max_stacks.values_by_elite_and_potential[2][5], 3.0));
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
