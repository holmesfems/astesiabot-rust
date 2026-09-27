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
    /// このモジュールを装備できる最低昇進(`uniequip_table.json`の`unlockEvolvePhase`。
    /// 0/1/2 = E0/E1/E2)。実データ調査済み: 現行の全モジュール(509件)は`PHASE_2`固定
    /// (P?で追加。フレームキル計算機の昇進/レベル指定でモジュール装備可否を判定するため)。
    #[serde(default)]
    pub unlock_phase: u8,
    /// `unlock_phase`到達時点で装備可能になる最低レベル(`unlockLevel`。レアリティにより
    /// 40/50/60等)。
    #[serde(default)]
    pub unlock_level: u32,
    /// フレームキル計算機の「条件付きバフ(P4)」用: このモジュールを装備した時の素質上書き
    /// (`addOrOverrideTalentDataBundle`)候補一覧。インデックス0=Lv1/1=Lv2/2=Lv3。
    /// Lv1は素質強化自体が無いことが多く、その場合は空Vec。`talentIndex`が負値(素質を
    /// 上書きしない特性追加等)の候補は含めない(`talent_index`はusizeなので保持できない)。
    #[serde(default)]
    pub talent_overrides_by_level: Vec<Vec<RawModuleTalentOverride>>,
}

/// モジュールによる素質上書き候補1件（P4。`battle_equip_table.json`の
/// `phases[lvIdx].parts[].addOrOverrideTalentDataBundle.candidates[]`）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RawModuleTalentOverride {
    /// このオペレーターの`talents`配列上のインデックス(0始まり)。
    pub talent_index: usize,
    /// 0始まり(`requiredPotentialRank`そのまま。0=潜在1〜5=潜在6)。
    pub potential_rank: u8,
    /// `value`が`None`の項目は除外済み(skill_dataのblackboardと同じ方針)。
    pub blackboard: IndexMap<String, f64>,
}

/// 素質(talent)候補1件（P4。`character_table.json`/`char_patch_table.json`の
/// `talents[i].candidates[]`）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RawTalentCandidate {
    /// 0=PHASE_0(E0)/1=PHASE_1(E1)/2=PHASE_2(E2)。
    pub phase: u8,
    /// 0始まり(`requiredPotentialRank`そのまま。0=潜在1〜5=潜在6)。
    pub potential_rank: u8,
    /// `value`が`None`の項目は除外済み。
    pub blackboard: IndexMap<String, f64>,
}

/// 素質1つ分(候補一覧のみ。`talents`配列上のインデックスがtalentIndexに対応する)。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RawTalent {
    pub candidates: Vec<RawTalentCandidate>,
}

/// 昇進段階1つ分のLv1〜Lv最大ATK(`character_table.json`の`phases[i]`。
/// `attributesKeyFrames`は必ずLv1/Lv最大の2点なので、その間は線形補間する前提のデータ)。
/// P?で追加。フレームキル計算機が昇進/レベル別のATKを計算する元データ
/// (`build_catalog`はこれをそのままカタログへ渡し、四捨五入込みの補間はJS側
/// `engine.js::computeBaseAtk`が行う)。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RawPhaseAtk {
    /// この昇進段階の最大レベル(6凸なら90/80/50、レアリティが低いほど小さい)。
    pub max_level: u32,
    /// Lv1のATK。
    pub atk_min: f64,
    /// `max_level`到達時のATK。
    pub atk_max: f64,
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
    /// `phases.last().atk_max + atk_trust_max`と同じ値(後方互換のため残す。
    /// 昇進/レベル/信頼度を指定する新計算は`phases`/`atk_trust_max`を使う)。
    pub atk_base: f64,
    /// 潜在(潜能)によるATK加算の合計（`formulaItem == "ADDITION"`のもののみ。
    /// 倍率型(MULTIPLY等)のATK潜在は将来的に別扱いが必要なため、ここには含めない）。
    pub atk_potential: f64,
    /// デフォルト(無強化)モジュールは含めない。`eq_type`昇順ソート済み。
    pub modules: Vec<RawModuleCombat>,
    /// 素質一覧(P4。フレームキル計算機の「条件付きバフ」が昇進/潜在から値を機械抽出する
    /// ために使う。`talents[i]`の`i`がtalentIndex)。
    #[serde(default)]
    pub talents: Vec<RawTalent>,
    /// 昇進段階ごとのLv1/Lv最大ATK(P?。インデックス0=E0。データに存在する昇進段階数だけ
    /// 入る。1〜3体はE0のみ、一部は2段階までしか無い[6凸できない下位レアリティ等])。
    #[serde(default)]
    pub phases: Vec<RawPhaseAtk>,
    /// 信頼度100%時点のATK加算値(`favorKeyFrames`最終値。`atk_base`はこれを含んだ値)。
    #[serde(default)]
    pub atk_trust_max: f64,
    /// skill_num(1始まりの文字列。`fk_data_search::search::skill_id_by_num`と同じ採番)→
    /// 解放昇進(0/1/2)。`character_table.json`の`skills[i].unlockCond.phase`から取得
    /// (S1=E0/S2=E1/S3=E2が大半だが、実データに即して機械抽出する)。
    #[serde(default)]
    pub skill_unlock_phase: Vec<(String, u8)>,
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

/// `unlockCondition.phase`("PHASE_0"/"PHASE_1"/"PHASE_2")を0/1/2へ変換する。
/// 未知の値はPHASE_0(0)扱い(実データで確認済みの値以外は来ない想定)。
fn phase_to_u8(phase: &str) -> u8 {
    match phase {
        "PHASE_1" => 1,
        "PHASE_2" => 2,
        _ => 0,
    }
}

/// blackboard配列(`[{key, value}]`)をIndexMapへ変換する。`value`が無い/数値でない項目は
/// 除外する(skill_dataのblackboard構築と同じ方針)。
fn parse_blackboard(value: Option<&Value>) -> IndexMap<String, f64> {
    let Some(Value::Array(items)) = value else { return IndexMap::new() };
    items
        .iter()
        .filter_map(|item| {
            let key = item.get("key")?.as_str()?.to_string();
            let value = item.get("value")?.as_f64()?;
            Some((key, value))
        })
        .collect()
}

/// `talents[i].candidates[]`(character_table.json/char_patch_table.json共通)をパースする。
/// P4: フレームキル計算機の「条件付きバフ」が昇進/潜在から値を機械抽出するために使う。
fn parse_talents(source_value: &Value) -> Vec<RawTalent> {
    let Some(Value::Array(talents)) = source_value.get("talents") else { return Vec::new() };
    talents
        .iter()
        .map(|t| {
            let candidates = t
                .get("candidates")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            let phase = c.get("unlockCondition")?.get("phase")?.as_str().map(phase_to_u8)?;
                            let potential_rank = c.get("requiredPotentialRank")?.as_u64()? as u8;
                            let blackboard = parse_blackboard(c.get("blackboard"));
                            Some(RawTalentCandidate { phase, potential_rank, blackboard })
                        })
                        .collect()
                })
                .unwrap_or_default();
            RawTalent { candidates }
        })
        .collect()
}

/// `<uniEquipId>.phases[lvIdx].parts[].addOrOverrideTalentDataBundle.candidates[]`
/// (battle_equip_table.json)をパースする。戻り値のインデックス0=Lv1/1=Lv2/2=Lv3。
/// `talentIndex`が負値(素質を上書きしない特性追加等)の候補は除外する。
fn parse_module_talent_overrides(battle_value: Option<&Value>) -> Vec<Vec<RawModuleTalentOverride>> {
    let Some(phases) = battle_value.and_then(|v| v.get("phases")).and_then(Value::as_array) else {
        return Vec::new();
    };
    phases
        .iter()
        .map(|phase| {
            let Some(parts) = phase.get("parts").and_then(Value::as_array) else { return Vec::new() };
            parts
                .iter()
                .flat_map(|part| {
                    let candidates = part
                        .get("addOrOverrideTalentDataBundle")
                        .and_then(|b| b.get("candidates"))
                        .and_then(Value::as_array);
                    let Some(candidates) = candidates else { return Vec::new() };
                    candidates
                        .iter()
                        .filter_map(|c| {
                            let talent_index = c.get("talentIndex").and_then(Value::as_i64)?;
                            if talent_index < 0 {
                                return None;
                            }
                            let potential_rank = c.get("requiredPotentialRank").and_then(Value::as_u64)? as u8;
                            let blackboard = parse_blackboard(c.get("blackboard"));
                            Some(RawModuleTalentOverride { talent_index: talent_index as usize, potential_rank, blackboard })
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

/// `phases[i].attributesKeyFrames`(Lv1/Lv最大の2点)から昇進ごとのATKデータを構築する。
/// `maxLevel`/`attributesKeyFrames`が欠けているエントリは(実データでは起きない想定だが)
/// 安全側に倒してスキップする。
fn parse_phases(source_value: &Value) -> Vec<RawPhaseAtk> {
    let Some(Value::Array(phases)) = source_value.get("phases") else { return Vec::new() };
    phases
        .iter()
        .filter_map(|phase| {
            let max_level = phase.get("maxLevel").and_then(Value::as_u64)? as u32;
            let kfs = phase.get("attributesKeyFrames").and_then(Value::as_array)?;
            let atk_min = kfs.first()?.get("data")?.get("atk")?.as_f64()?;
            let atk_max = kfs.last()?.get("data")?.get("atk")?.as_f64()?;
            Some(RawPhaseAtk { max_level, atk_min, atk_max })
        })
        .collect()
}

/// 信頼度100%時点のATK加算値(`favorKeyFrames`最終値)。`favorKeyFrames`が無ければ0。
fn parse_atk_trust_max(source_value: &Value) -> f64 {
    source_value
        .get("favorKeyFrames")
        .and_then(Value::as_array)
        .and_then(|kfs| kfs.last())
        .and_then(|kf| kf.get("data"))
        .and_then(|data| data.get("atk"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

/// `skills[i].unlockCond.phase`(character_table.json/char_patch_table.json共通)から
/// skill_num("1"始まり。`skills`配列の並び順がスキル1,2,3...という前提。
/// `fk_data_search::search::skill_id_by_num`と同じ採番)→解放昇進(0/1/2)を構築する。
fn parse_skill_unlock_phase(source_value: &Value) -> Vec<(String, u8)> {
    let Some(Value::Array(skills)) = source_value.get("skills") else { return Vec::new() };
    skills
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let phase = s.get("unlockCond")?.get("phase")?.as_str().map(phase_to_u8)?;
            Some(((i + 1).to_string(), phase))
        })
        .collect()
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

        let phases = parse_phases(source_value);
        let atk_trust_max = parse_atk_trust_max(source_value);
        let atk_base = phases.last().map(|p| p.atk_max).unwrap_or(0.0) + atk_trust_max;

        let raw = RawOperatorCombat {
            id: key.clone(),
            name: name.clone(),
            cn_name: cn_name.to_string(),
            profession: profession.to_string(),
            position: position.to_string(),
            nation_id,
            atk_base,
            atk_potential: parse_atk_potential(source_value),
            modules: Vec::new(),
            talents: parse_talents(source_value),
            phases,
            atk_trust_max,
            skill_unlock_phase: parse_skill_unlock_phase(source_value),
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

        let phases = parse_phases(source_value);
        let atk_trust_max = parse_atk_trust_max(source_value);
        let atk_base = phases.last().map(|p| p.atk_max).unwrap_or(0.0) + atk_trust_max;

        let raw = RawOperatorCombat {
            id: key.clone(),
            name: name.clone(),
            cn_name,
            profession: profession.to_string(),
            position: position.to_string(),
            nation_id,
            atk_base,
            atk_potential: parse_atk_potential(source_value),
            modules: Vec::new(),
            talents: parse_talents(source_value),
            phases,
            atk_trust_max,
            skill_unlock_phase: parse_skill_unlock_phase(source_value),
        };
        name_to_id.insert(name, key.clone());
        operators.insert(key.clone(), raw);
    }
}

/// uniequip_table.json（モジュールのメタ情報: 名前/typeName2/charId）と
/// battle_equip_table.json（Stage1〜3のATK加算値 + P4の素質上書き）を突き合わせて
/// 各オペレーターへ追加する。
///
/// **`tmplId`優先の注意(P4で発覚した実データの罠)**: アーミヤの前衛/医療形態のような
/// 「複数の`char_patch_table`派生形が同じ基礎`charId`を共有する」オペレーターの場合、
/// uniequip_table.json側の各モジュールは`charId`が基礎オペレーター(例:
/// "char_002_amiya")のまま共通で、`tmplId`にどの派生形専用か(例: "char_1001_amiya2"
/// =前衛アーミヤ)が入る。ここを`charId`だけで振り分けると、派生形専用のはずの
/// モジュールが全て基礎オペレーターの方に付いてしまう(前衛アーミヤの実装時に発覚した
/// 回帰。P4で修正)。`tmplId`があればそちらを優先して振り分け先にする。
/// (濁心スカジのように`char_1012_skadi2`という専用charIdを直接持つ派生キャラは
/// `tmplId`を持たない=`charId`のままで正しく振り分けられる。両ケースを両立させるため
/// `tmplId.unwrap_or(charId)`にする)。
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
        // tmplId優先の理由は関数冒頭コメント参照。
        let target_id = cn_value.get("tmplId").and_then(Value::as_str).unwrap_or(char_id);
        if !operators.contains_key(target_id) {
            continue;
        }

        let jp_value = jp_equip_dict.and_then(|jp| jp.get(equip_id.as_str()));
        let name = jp_value
            .and_then(|jp| jp.get("uniEquipName"))
            .and_then(Value::as_str)
            .or_else(|| cn_value.get("uniEquipName").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        // 装備可否条件(P?)。実データ調査済み: 現行の全モジュール(509件)は`unlockEvolvePhase`が
        // "PHASE_2"固定だが、将来的な変化に備えて機械抽出する。CN/JPで値が異なることは
        // 無い想定なのでcn_valueから読む(uniEquipName等と違いJP優先にする必要が無い)。
        let unlock_phase = cn_value.get("unlockEvolvePhase").and_then(Value::as_str).map(phase_to_u8).unwrap_or(0);
        let unlock_level = cn_value.get("unlockLevel").and_then(Value::as_u64).unwrap_or(0) as u32;

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

        by_operator.entry(target_id.to_string()).or_default().push(RawModuleCombat {
            eq_id: equip_id.clone(),
            eq_type: eq_type.to_string(),
            name,
            atk_by_level,
            unlock_phase,
            unlock_level,
            talent_overrides_by_level: parse_module_talent_overrides(battle_value),
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

    /// 昇進(P?)関連フィールドの実データ突き合わせ(オフライン。Seedを直接読む)。
    /// エーベンホルツ(char_4046_ebnhlz): E0=Lv1-50(611〜873)/E1=Lv1-80(873〜1134)/
    /// E2=Lv1-90(1134〜1400)、信頼度最大加算(atk_trust_max)=150、
    /// スキル解放昇進はS1=E0/S2=E1/S3=E2。ブレイズのモジュールX(uniequip_002_huang)は
    /// 昇進2 Lv60以上で装備可能。いずれもオーナー確認済みの実データ(2026-09時点)。
    #[test]
    fn seed_has_expected_phase_and_module_unlock_data() {
        let json = std::fs::read_to_string(SEED_PATH)
            .unwrap_or_else(|e| panic!("seed({SEED_PATH})の読み込みに失敗: {e}。先に`cargo run --bin regen_seeds`を実行すること"));
        let data: OperatorCombat = serde_json::from_str(&json).expect("seedがOperatorCombatとしてparseできること");

        let ebenholz = data.get_by_name("エーベンホルツ").expect("エーベンホルツがseedに存在すること");
        assert_eq!(ebenholz.phases.len(), 3, "エーベンホルツはE0/E1/E2の3段階のはず");
        assert_eq!(ebenholz.phases[0].max_level, 50);
        assert_eq!(ebenholz.phases[0].atk_min, 611.0);
        assert_eq!(ebenholz.phases[0].atk_max, 873.0);
        assert_eq!(ebenholz.phases[1].max_level, 80);
        assert_eq!(ebenholz.phases[1].atk_min, 873.0);
        assert_eq!(ebenholz.phases[1].atk_max, 1134.0);
        assert_eq!(ebenholz.phases[2].max_level, 90);
        assert_eq!(ebenholz.phases[2].atk_min, 1134.0);
        assert_eq!(ebenholz.phases[2].atk_max, 1400.0);
        assert_eq!(ebenholz.atk_trust_max, 150.0);
        assert_eq!(ebenholz.atk_base, ebenholz.phases[2].atk_max + ebenholz.atk_trust_max, "atk_baseはphases/atk_trust_maxと整合しているはず");
        assert_eq!(
            ebenholz.skill_unlock_phase,
            vec![("1".to_string(), 0), ("2".to_string(), 1), ("3".to_string(), 2)],
            "エーベンホルツはS1=E0/S2=E1/S3=E2のはず"
        );

        let blaze = data.get_by_name("ブレイズ").expect("ブレイズがseedに存在すること");
        let module_x = blaze.modules.iter().find(|m| m.eq_id == "uniequip_002_huang").expect("ブレイズにモジュールX(uniequip_002_huang)があるはず");
        assert_eq!(module_x.unlock_phase, 2, "モジュールは昇進2で装備可能なはず");
        assert_eq!(module_x.unlock_level, 60, "ブレイズ(6凸)のモジュール装備可能レベルは60のはず");

        // シー(char_2015_dusk)E2: Lv1=771/Lv90=918、信頼度最大加算=110。オーナー確認済みの
        // 実測値(E2 Lv71・信頼度100%・無モジュール・潜在+34でATK1031)の元データ
        // (補間の四捨五入自体はJS側`engine.js::computeBaseAtk`のverify.mjsで検証する)。
        let dusk = data.get_by_name("シー").expect("シーがseedに存在すること");
        assert_eq!(dusk.phases[2].max_level, 90);
        assert_eq!(dusk.phases[2].atk_min, 771.0);
        assert_eq!(dusk.phases[2].atk_max, 918.0);
        assert_eq!(dusk.atk_trust_max, 110.0);
    }
}
