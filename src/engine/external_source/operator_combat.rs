//! フレームキル計算機用のオペレーター戦闘生データ（machine-extractable な数値のみ）。
//! `operator_data.rs`（消費素材ドメイン）とは意図的に分離する別ソース。
//! character_table.json（信頼度100込みの元ATK・潜在ATK）と
//! uniequip_table.json + battle_equip_table.json（モジュールのATK加算値）を1回の
//! fetchでまとめて構築する。CN/JPマージ・キー付け・名前解決のロジックは
//! `operator_data.rs` と同じ方針を踏襲する（詳細は各関数のコメント参照）。

use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

const CHAR_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/character_table.json";
const CHAR_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/character_table.json";
const UNI_EQ_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/uniequip_table.json";
const UNI_EQ_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/uniequip_table.json";
const PATCH_CHAR_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/char_patch_table.json";
const PATCH_CHAR_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/char_patch_table.json";
const BATTLE_EQUIP_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/battle_equip_table.json";
const BATTLE_EQUIP_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/battle_equip_table.json";

/// Seedの保存先。`cargo run --bin regen_seeds` で手動生成し、git commitして
/// リポジトリに含めておく（起動時fetchが失敗した場合のフォールバック用）。
pub const SEED_PATH: &str = "data/seed/operator_combat.json";

/// 職業ID→日本語表記（`operator_data.rs`の`JOB_ID_TO_NAME`と同一定義。
/// 昇格オペレーターの名前 "元名(職名)" 組み立てにのみ使うので、依存を作らず
/// このファイル内に複製する）。
const JOB_ID_TO_NAME: &[(&str, &str)] = &[
    ("WARRIOR", "前衛"),
    ("SNIPER", "狙撃"),
    ("SPECIAL", "特殊"),
    ("SUPPORT", "補助"),
    ("TANK", "重装"),
    ("PIONEER", "先鋒"),
    ("CASTER", "術師"),
    ("MEDIC", "医療"),
];

fn job_ja(profession: &str) -> &'static str {
    JOB_ID_TO_NAME
        .iter()
        .find(|(id, _)| *id == profession)
        .map(|(_, ja)| *ja)
        .unwrap_or("不明")
}

/// モジュール1種分のATK加算値（Stage1〜3、`battle_equip_table.json`の
/// `attributeBlackboard`の`atk`キー）。素材コストは含めない（それは`operator_data.rs`側）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RawModuleCombat {
    pub eq_id: String,
    /// typeName2（"X"/"Y"等）。
    pub eq_type: String,
    pub name: String,
    /// Stage1〜3のATK加算値。データに存在するステージ数だけ入る。
    pub atk_by_level: Vec<f64>,
}

/// オペレーター1名分の戦闘生データ（machine-extractableな数値のみ）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RawOperatorCombat {
    pub id: String,
    pub name: String,
    pub cn_name: String,
    /// 大陸版表記そのまま（例: "WARRIOR"）。日本語表記が要るなら呼び出し側で変換する。
    pub profession: String,
    /// "MELEE"/"RANGED"。
    pub position: String,
    /// 所属勢力(大陸版`nationId`の生値。例: "laterano")。フレームキル計算機の
    /// 勢力タグ判定用(machine-only)。現時点で使うのは「ラテラーノ」バフの対象判定のみ
    /// （`tags.rs::faction_tag_for`）。無ければ空文字列。
    #[serde(default)]
    pub nation_id: String,
    /// 昇進2(E2)最大レベルのATK + 信頼度100時点のATK加算（`favorKeyFrames`最終値）。
    pub atk_base: f64,
    /// 潜在(潜能)によるATK加算の合計（`formulaItem == "ADDITION"`のもののみ。
    /// 倍率型(MULTIPLY等)のATK潜在は将来的に別扱いが必要なため、ここには含めない）。
    pub atk_potential: f64,
    /// デフォルト(無強化)モジュールは含めない。`eq_type`昇順ソート済み。
    pub modules: Vec<RawModuleCombat>,
}

/// オペレーター戦闘生データ一式。
#[derive(Serialize, Deserialize, Default)]
pub struct OperatorCombat {
    /// `IndexMap`はPython系の他ソースと同じくファイル順を保持する（現時点では
    /// 順序に依存する機能は無いが、`operator_data.rs`との一貫性のため合わせる）。
    pub operators: IndexMap<String, RawOperatorCombat>,
    pub name_to_id: HashMap<String, String>,
}

impl OperatorCombat {
    pub fn get_by_name(&self, name: &str) -> Option<&RawOperatorCombat> {
        self.name_to_id.get(name).and_then(|id| self.operators.get(id))
    }

    #[cfg(test)]
    pub fn empty_for_test() -> Self {
        Self::default()
    }
}

pub fn fetch() -> BoxFuture<'static, Result<OperatorCombat, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<OperatorCombat, FetchError> {
    let client = client();
    let (cn_table, jp_table, cn_uniequip, jp_uniequip, cn_patch, jp_patch, cn_battle_equip, jp_battle_equip) = tokio::try_join!(
        fetch_json_with_retry(&client, CHAR_TABLE_URL_CN),
        fetch_json_with_retry(&client, CHAR_TABLE_URL_JP),
        fetch_json_with_retry(&client, UNI_EQ_URL_CN),
        fetch_json_with_retry(&client, UNI_EQ_URL_JP),
        fetch_json_with_retry(&client, PATCH_CHAR_TABLE_URL_CN),
        fetch_json_with_retry(&client, PATCH_CHAR_TABLE_URL_JP),
        fetch_json_with_retry(&client, BATTLE_EQUIP_TABLE_URL_CN),
        fetch_json_with_retry(&client, BATTLE_EQUIP_TABLE_URL_JP),
    )?;

    // JP未実装オペレーターの名前フォールバック（`operator_data.rs`と同じファイルを使う）。
    let custom: BTreeMap<String, String> = match std::fs::read_to_string("data/customOperatorZhToJa.yaml") {
        Ok(s) => serde_yaml::from_str(&s).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    };

    let mut operators: IndexMap<String, RawOperatorCombat> = IndexMap::new();
    let mut name_to_id: HashMap<String, String> = HashMap::new();

    build_characters(&cn_table, &jp_table, &custom, &mut operators, &mut name_to_id);
    build_patches(&cn_patch, &jp_patch, &operators.clone(), &mut operators, &mut name_to_id);
    build_modules(&cn_uniequip, &jp_uniequip, &cn_battle_equip, &jp_battle_equip, &mut operators);

    Ok(OperatorCombat { operators, name_to_id })
}

/// character_table.json のキーが `char_xxx_yyy` 形式（オペレーター）かどうか
/// （`operator_data.rs::is_char_key`と同一ロジック）。
fn is_char_key(key: &str) -> bool {
    key.split('_').next() == Some("char")
}

/// 昇進2(E2、`phases`の最終要素)最大レベルのATK + 信頼度100時点のATK加算を合算する。
/// `phases`/`favorKeyFrames`が無い(取得できない)場合はその項を0として扱う。
fn parse_atk_base(source_value: &Value) -> f64 {
    let elite_max_atk = source_value
        .get("phases")
        .and_then(Value::as_array)
        .and_then(|phases| phases.last())
        .and_then(|phase| phase.get("attributesKeyFrames"))
        .and_then(Value::as_array)
        .and_then(|kfs| kfs.last())
        .and_then(|kf| kf.get("data"))
        .and_then(|data| data.get("atk"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let trust_max_atk = source_value
        .get("favorKeyFrames")
        .and_then(Value::as_array)
        .and_then(|kfs| kfs.last())
        .and_then(|kf| kf.get("data"))
        .and_then(|data| data.get("atk"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    elite_max_atk + trust_max_atk
}

/// `potentialRanks`のうち`attributeType == "ATK"` かつ `formulaItem == "ADDITION"`の
/// modifierの値を合算する（実データ調査済み: 対象6体は全てADDITION型で1件のみ）。
/// 倍率型(MULTIPLY等)のATK潜在は意図的に含めない（フラットな`atk_base`加算として
/// 扱うのが不適切なため。将来的に必要になったら別フィールドで持つこと）。
fn parse_atk_potential(source_value: &Value) -> f64 {
    let Some(Value::Array(ranks)) = source_value.get("potentialRanks") else {
        return 0.0;
    };
    ranks
        .iter()
        .filter_map(|rank| rank.get("buff")?.get("attributes")?.get("attributeModifiers")?.as_array())
        .flatten()
        .filter_map(|modifier| {
            let attr_type = modifier.get("attributeType").and_then(Value::as_str)?;
            let formula_item = modifier.get("formulaItem").and_then(Value::as_str)?;
            if attr_type != "ATK" || formula_item != "ADDITION" {
                return None;
            }
            modifier.get("value").and_then(Value::as_f64)
        })
        // `sum()`は空のとき-0.0を返すため、0.0起点のfoldにする（Seedに"-0.0"が出るのを防ぐ）。
        .fold(0.0, |acc, v| acc + v)
}

/// char_table.json のCN/JPをマージしながらオペレーター一覧を構築する
/// （`operator_data.rs::build_characters`と同じCN/JPマージ・名前解決方針）。
fn build_characters(
    cn_table: &Value,
    jp_table: &Value,
    custom: &BTreeMap<String, String>,
    operators: &mut IndexMap<String, RawOperatorCombat>,
    name_to_id: &mut HashMap<String, String>,
) {
    let Value::Object(cn_map) = cn_table else { return };
    for (key, cn_value) in cn_map {
        if !is_char_key(key) {
            continue;
        }
        if cn_value.get("isNotObtainable").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(cn_name) = cn_value.get("name").and_then(Value::as_str) else {
            continue;
        };

        let jp_value = jp_table.get(key.as_str());
        let ja_name = jp_value
            .and_then(|jp| jp.get("name"))
            .and_then(Value::as_str)
            .or_else(|| custom.get(cn_name).map(String::as_str));
        let source_value = jp_value.unwrap_or(cn_value);
        let name = ja_name.map(str::to_string).unwrap_or_else(|| cn_name.to_string());

        let Some(profession) = source_value.get("profession").and_then(Value::as_str) else {
            continue;
        };
        let Some(position) = source_value.get("position").and_then(Value::as_str) else {
            continue;
        };

        let nation_id = source_value.get("nationId").and_then(Value::as_str).unwrap_or_default().to_string();

        let raw = RawOperatorCombat {
            id: key.clone(),
            name: name.clone(),
            cn_name: cn_name.to_string(),
            profession: profession.to_string(),
            position: position.to_string(),
            nation_id,
            atk_base: parse_atk_base(source_value),
            atk_potential: parse_atk_potential(source_value),
            modules: Vec::new(),
        };
        name_to_id.insert(name, key.clone());
        operators.insert(key.clone(), raw);
    }
}

/// 昇格オペレーター(前衛/医療アーミヤ等)を追加する
/// （`operator_data.rs::build_patches`と同じ元オペレーター逆引き方針）。
/// `base_operators`は追加前のスナップショット(元オペレーター名逆引き用)。
fn build_patches(
    cn_patch: &Value,
    jp_patch: &Value,
    base_operators: &IndexMap<String, RawOperatorCombat>,
    operators: &mut IndexMap<String, RawOperatorCombat>,
    name_to_id: &mut HashMap<String, String>,
) {
    let Some(Value::Object(patch_info_cn)) = cn_patch.get("patchChars") else { return };
    let patch_info_jp = jp_patch.get("patchChars");
    let Some(Value::Object(patch_key_cn)) = cn_patch.get("infos") else { return };

    // `tmplIds`にpatch_keyを含むoriginal operatorを逆引きする（Python `originalOperatorName`相当）。
    let find_original = |patch_key: &str| -> Option<(&str, &str)> {
        for (original_id, info) in patch_key_cn.iter() {
            let matches = info
                .get("tmplIds")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(patch_key)));
            if matches {
                if let Some(op) = base_operators.get(original_id) {
                    return Some((op.name.as_str(), op.cn_name.as_str()));
                }
                return Some(("", ""));
            }
        }
        None
    };

    for (key, cn_value) in patch_info_cn {
        let jp_value = patch_info_jp.and_then(|jp| jp.get(key.as_str()));
        let source_value = jp_value.unwrap_or(cn_value);
        let Some(profession) = source_value.get("profession").and_then(Value::as_str) else {
            continue;
        };
        let Some(position) = source_value.get("position").and_then(Value::as_str) else {
            continue;
        };
        let job = job_ja(profession);
        let (original_name, original_cn_name) = find_original(key).unwrap_or(("", ""));
        let name = format!("{original_name}({job})");
        let cn_name = format!("{original_cn_name}({job})");
        let nation_id = source_value.get("nationId").and_then(Value::as_str).unwrap_or_default().to_string();

        let raw = RawOperatorCombat {
            id: key.clone(),
            name: name.clone(),
            cn_name,
            profession: profession.to_string(),
            position: position.to_string(),
            nation_id,
            atk_base: parse_atk_base(source_value),
            atk_potential: parse_atk_potential(source_value),
            modules: Vec::new(),
        };
        name_to_id.insert(name, key.clone());
        operators.insert(key.clone(), raw);
    }
}

/// uniequip_table.json（モジュールのメタ情報: 名前/typeName2/charId）と
/// battle_equip_table.json（Stage1〜3のATK加算値）を突き合わせて各オペレーターへ追加する。
fn build_modules(
    cn_uniequip: &Value,
    jp_uniequip: &Value,
    cn_battle_equip: &Value,
    jp_battle_equip: &Value,
    operators: &mut IndexMap<String, RawOperatorCombat>,
) {
    let Some(Value::Object(equip_dict)) = cn_uniequip.get("equipDict") else { return };
    let jp_equip_dict = jp_uniequip.get("equipDict");

    let mut by_operator: HashMap<String, Vec<RawModuleCombat>> = HashMap::new();
    for (equip_id, cn_value) in equip_dict {
        // typeName2が空/無しはデフォルト(無強化)モジュールなので対象外
        // （`operator_data.rs::build_modules`と同じ判定）。
        let Some(eq_type) = cn_value.get("typeName2").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(char_id) = cn_value.get("charId").and_then(Value::as_str) else {
            continue;
        };
        if !operators.contains_key(char_id) {
            continue;
        }

        let jp_value = jp_equip_dict.and_then(|jp| jp.get(equip_id.as_str()));
        let name = jp_value
            .and_then(|jp| jp.get("uniEquipName"))
            .and_then(Value::as_str)
            .or_else(|| cn_value.get("uniEquipName").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();

        // battle_equip_table.jsonはCN/JPどちらもトップレベルがequipIdそのままのdict。
        // JPにエントリがあればそちらを優先し、無ければCNを使う（数値自体はCN/JPで
        // 通常一致するが、`operator_data.rs`と同じ「実装済みならJP値優先」の方針を揃える）。
        let battle_value = jp_battle_equip
            .get(equip_id.as_str())
            .or_else(|| cn_battle_equip.get(equip_id.as_str()));
        let atk_by_level: Vec<f64> = battle_value
            .and_then(|v| v.get("phases"))
            .and_then(Value::as_array)
            .map(|phases| {
                phases
                    .iter()
                    .map(|phase| {
                        phase
                            .get("attributeBlackboard")
                            .and_then(Value::as_array)
                            .and_then(|items| items.iter().find(|item| item.get("key").and_then(Value::as_str) == Some("atk")))
                            .and_then(|item| item.get("value"))
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0)
                    })
                    .collect()
            })
            .unwrap_or_default();

        by_operator.entry(char_id.to_string()).or_default().push(RawModuleCombat {
            eq_id: equip_id.clone(),
            eq_type: eq_type.to_string(),
            name,
            atk_by_level,
        });
    }

    for (char_id, mut modules) in by_operator {
        modules.sort_by(|a, b| a.eq_type.cmp(&b.eq_type));
        if let Some(op) = operators.get_mut(&char_id) {
            op.modules = modules;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ネットワークで戦闘生データを取得できるかの疎通確認。
    /// `cargo test -- --ignored` で明示実行する（通常のCIでは走らせない）。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_gamedata() {
        let data = fetch().await.expect("fetch should succeed against real network");
        assert!(!data.operators.is_empty());
        let ash = data.get_by_name("Ash").expect("Ash should exist");
        assert!(ash.atk_base > 0.0);
    }

    /// オーナーの参照スプレッドシート値との突き合わせ（オフライン。Seedを直接読む）。
    /// `data/seed/operator_combat.json` が無い場合は先に
    /// `cargo run --bin regen_seeds` を実行すること。
    #[test]
    fn seed_matches_reference_values() {
        let json = std::fs::read_to_string(SEED_PATH)
            .unwrap_or_else(|e| panic!("seed({SEED_PATH})の読み込みに失敗: {e}。先に`cargo run --bin regen_seeds`を実行すること"));
        let data: OperatorCombat = serde_json::from_str(&json).expect("seedがOperatorCombatとしてparseできること");

        // (JA名, atk_base, atk_potential, いずれかのモジュールのLv1〜3攻撃力)。owner提供の参照スプレッドシート値。
        let cases: &[(&str, f64, f64, [f64; 3])] = &[
            ("濁心スカジ", 418.0, 27.0, [26.0, 32.0, 35.0]),
            ("ブレイズ", 825.0, 28.0, [50.0, 70.0, 86.0]),
            ("Ash", 624.0, 27.0, [25.0, 33.0, 40.0]),
            ("ファイヤーウォッチ", 1175.0, 35.0, [60.0, 75.0, 87.0]),
        ];
        for (name, expected_atk_base, expected_atk_potential, expected_module_atk) in cases {
            let op = data.get_by_name(name).unwrap_or_else(|| panic!("{name}がseedに存在すること"));
            assert_eq!(op.atk_base, *expected_atk_base, "{name}のatk_baseが不一致");
            assert_eq!(op.atk_potential, *expected_atk_potential, "{name}のatk_potentialが不一致");
            assert!(
                op.modules.iter().any(|m| m.atk_by_level == expected_module_atk),
                "{name}に攻撃力{expected_module_atk:?}のモジュールが無い"
            );
        }
    }
}
