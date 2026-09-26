//! フレームキル計算機の「機械データ + 手動補正(overrides.yaml)」マージ層（★純粋ロジック。
//! bot にも api にも依存しない。`engine::recruit` / `engine::operator_cost_calc` と同じ
//! 位置付け）。
//!
//! 3層構成のうち3層目に相当する:
//! 1. 機械抽出できる生データ → `engine::external_source::operator_combat` +
//!    `engine::external_source::skill_data`(blackboard)
//! 2. 手動補正 → `overrides.rs`(`data/fk_kill_calc/overrides.yaml`)
//! 3. マージ → このモジュールの`build_catalog`

pub mod dto;
mod overrides;
mod tags;

pub use dto::Catalog;
pub use overrides::{OverrideVariant, Overrides, OverridesMap};

use crate::engine::external_source::fk_data::{FkSheetData, FkSheetRow};
use crate::engine::external_source::operator_combat::OperatorCombat;
use crate::engine::external_source::operator_data::OperatorData;
use crate::engine::external_source::skill_data::SkillData;
use crate::engine::fk_data_search::search::skill_id_by_num;
use dto::{CatalogModule, CatalogOperator, DamageType, FkEntry, Valued};
use indexmap::IndexMap;
use std::collections::HashMap;

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
        let tags = tags::tags_for(&combat_op.profession, &combat_op.position);
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

            match overrides.variants_for(op_id, &row.skill_num) {
                Some(variants) if !variants.is_empty() => {
                    for variant in variants {
                        fk_entries.push(build_entry(
                            row,
                            skill_label.clone(),
                            variant.label.clone(),
                            Valued::from_override(variant.multiplier, default_multiplier),
                            multiplier_candidates.clone(),
                            Valued::from_override(variant.self_atk_pct, default_self_atk_pct),
                            Valued::from_override(variant.hits, default_hits),
                            Valued::from_override(variant.damage_type, default_damage_type),
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
                        None,
                    ));
                }
            }
        }

        operators.push(CatalogOperator {
            id: op_id.to_string(),
            name: name.clone(),
            tags,
            atk_base: combat_op.atk_base,
            atk_potential: combat_op.atk_potential,
            modules: combat_op
                .modules
                .iter()
                .map(|m| CatalogModule {
                    id: m.eq_id.clone(),
                    type_name: m.eq_type.clone(),
                    name: m.name.clone(),
                    atk_by_level: m.atk_by_level.clone(),
                })
                .collect(),
            fk_entries,
        });
    }

    CatalogBuild {
        catalog: Catalog { operators, buffers: Vec::new() },
        skipped,
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
        assert!(result.catalog.buffers.is_empty(), "P1のbuffersは常に空のはず");
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
    /// self_atk_pctは引き続きManual(0.772)。
    #[test]
    fn blaze_s3_multiplier_is_now_auto_via_suffix_rule() {
        let result = build_from_seeds();
        let blaze = result.catalog.operators.iter().find(|op| op.name == "ブレイズ").expect("ブレイズがカタログに存在すること");
        let s3 = blaze.fk_entries.iter().find(|e| e.skill_num == "3").expect("ブレイズのS3が存在すること");
        assert_eq!(s3.multiplier.value, 4.0);
        assert_eq!(s3.multiplier.source, dto::ValueSource::Auto, "multiplierのoverrideは撤去済みのはず");
        assert_eq!(s3.self_atk_pct.value, 0.772);
        assert_eq!(s3.self_atk_pct.source, dto::ValueSource::Manual);
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
