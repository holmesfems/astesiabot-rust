use super::cache::write_seed_file;
use super::http::{client, fetch_json_with_retry};
use super::{BoxFuture, FetchError};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

const CHAR_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/character_table.json";
const CHAR_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/character_table.json";
const UNI_EQ_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/uniequip_table.json";
const UNI_EQ_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/uniequip_table.json";
const PATCH_CHAR_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/char_patch_table.json";
const PATCH_CHAR_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/char_patch_table.json";

/// Seedの保存先。`cargo run --bin regen_seeds` で手動生成し、git commitして
/// リポジトリに含めておく（起動時fetchが失敗した場合のフォールバック用）。
pub const SEED_PATH: &str = "data/seed/operator_data.json";

/// 職業ID→日本語表記（Python `jobIdToName`。昇格オペレーターの名前 "元名(職名)" に使う）。
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

/// 素材1件分（アイテムID→個数）。Python の `{"id":..., "count":...}` そのまま。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CostEntry {
    pub id: String,
    pub count: f64,
}

/// スキル1本分の特化コスト（Python `OperatorCosts.skills` の1要素）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RawSkill {
    pub skill_id: String,
    /// 特化1〜3のコストリスト（長さ3。levelUpCostCond の並び順）。
    pub masteries: Vec<Vec<CostEntry>>,
}

/// モジュール1種分（Python `EQCostItem`）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RawModule {
    pub eq_type: String,
    pub cn_only: bool,
    /// Stage1〜3のコストリスト。
    pub phase_costs: Vec<Vec<CostEntry>>,
}

/// オペレーター1名分の消費素材生データ（Python `OperatorCosts.__init__` の結果。
/// 理性価値・中級換算などの計算は含めない）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RawOperatorCost {
    pub id: String,
    pub name: String,
    pub cn_name: String,
    /// 大陸版表記("TIER_n")・日本版表記(int)のどちらでも解決済みの星の数。
    pub stars: u32,
    /// 大陸版限定オペレーターか。
    pub cn_only: bool,
    /// 昇格オペレーター(前衛/医療アーミヤ等)か。
    pub is_patch: bool,
    /// 直近実装判定（Python `isRecent`。`cn_only || cn_name が customOperatorZhToJa にある`）。
    pub is_recent: bool,
    /// 昇進1,2に必要な素材（`evolveCost`の`[1..]`。Python `OperatorCosts.phases`）。
    pub phases: Vec<Vec<CostEntry>>,
    pub skills: Vec<RawSkill>,
    /// スキルLv1→7に必要な素材（Python `OperatorCosts.allSkills`）。
    pub all_skill_lvlup: Vec<Vec<CostEntry>>,
    /// typeName2が空、またはitemCostがnullのモジュールは除外済み。`eq_type`昇順ソート済み。
    pub modules: Vec<RawModule>,
}

/// オペレーター消費素材データ一式（Python `AllOperatorsInfo`相当）。
/// `cn_to_ja` は旧`operator_names::OperatorNames`と同一ロジックで構築しており、
/// birthday.rs等の中国語名→日本語名変換はここから取得する
/// （char_table.jsonのfetchをこのソース1つに集約するため）。
#[derive(Serialize, Deserialize, Default)]
pub struct OperatorData {
    cn_to_ja: HashMap<String, String>,
    /// `IndexMap`はPython dictの挿入順(char_table.jsonのファイル順→昇格オペレーターの順)を
    /// 保持するために使う。ランキング系関数(`AllOperatorsInfo::sorted_by_elite_cost`等)の
    /// 安定ソートがPython版と同じタイブレーク順になることの前提（`Cargo.toml`で
    /// `serde_json`の`preserve_order`を有効にしてファイル順そのものを保つのと対）。
    pub operators: IndexMap<String, RawOperatorCost>,
    pub name_to_id: HashMap<String, String>,
}

impl OperatorData {
    /// 中国語名を日本語名に変換する。対応が無ければ中国語名をそのまま返す
    /// （旧`OperatorNames::to_ja`と同一の意味・同一の変換結果を保証する）。
    pub fn to_ja<'a>(&'a self, cn_name: &'a str) -> &'a str {
        self.cn_to_ja.get(cn_name).map(String::as_str).unwrap_or(cn_name)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&RawOperatorCost> {
        self.name_to_id.get(name).and_then(|id| self.operators.get(id))
    }

    /// テスト用: 変換辞書・コストデータが空の状態を作る。
    #[cfg(test)]
    pub fn empty_for_test() -> Self {
        Self::default()
    }
}

pub fn fetch() -> BoxFuture<'static, Result<OperatorData, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

async fn fetch_impl() -> Result<OperatorData, FetchError> {
    let client = client();
    let (cn_table, jp_table, cn_uniequip, jp_uniequip, cn_patch, jp_patch) = tokio::try_join!(
        fetch_json_with_retry(&client, CHAR_TABLE_URL_CN),
        fetch_json_with_retry(&client, CHAR_TABLE_URL_JP),
        fetch_json_with_retry(&client, UNI_EQ_URL_CN),
        fetch_json_with_retry(&client, UNI_EQ_URL_JP),
        fetch_json_with_retry(&client, PATCH_CHAR_TABLE_URL_CN),
        fetch_json_with_retry(&client, PATCH_CHAR_TABLE_URL_JP),
    )?;

    let custom: BTreeMap<String, String> = match std::fs::read_to_string("data/customOperatorZhToJa.yaml") {
        Ok(s) => serde_yaml::from_str(&s).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    };
    let custom_keys: HashSet<&str> = custom.keys().map(String::as_str).collect();

    let mut operators: IndexMap<String, RawOperatorCost> = IndexMap::new();
    let mut cn_to_ja: HashMap<String, String> = HashMap::new();
    let mut name_to_id: HashMap<String, String> = HashMap::new();

    build_characters(&cn_table, &jp_table, &custom, &custom_keys, &mut operators, &mut cn_to_ja, &mut name_to_id);
    build_patches(&cn_patch, &jp_patch, &custom_keys, &operators.clone(), &mut operators, &mut name_to_id);
    build_modules(&cn_uniequip, &jp_uniequip, &mut operators);

    Ok(OperatorData {
        cn_to_ja,
        operators,
        name_to_id,
    })
}

/// character_table.json のキーが `char_xxx_yyy` 形式（オペレーター）かどうか
/// （Python の `re.match(r"([^_]+)_(\d+)_([^_]+)", key).group(1) == "char"` 相当）。
fn is_char_key(key: &str) -> bool {
    key.split('_').next() == Some("char")
}

/// "TIER_n"（大陸版）/ int（日本版、+1する）のどちらでも星の数を解決する
/// （Python `OperatorCosts.__init__` の rarity 分岐）。
fn parse_stars(value: &Value) -> u32 {
    match value.get("rarity") {
        Some(Value::String(s)) => s.trim_start_matches("TIER_").parse::<u32>().unwrap_or(0),
        Some(Value::Number(n)) => n.as_u64().map(|v| v as u32 + 1).unwrap_or(0),
        _ => 0,
    }
}

fn parse_cost_list(value: Option<&Value>) -> Vec<CostEntry> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(Value::as_str)?.to_string();
            let count = item.get("count").and_then(Value::as_f64)?;
            Some(CostEntry { id, count })
        })
        .collect()
}

/// evolveCostの`[1..]`（phase0はコスト無しのため除外。Python `[ItemCost(...) for ...][1:]`）。
fn parse_phases(value: &Value) -> Vec<Vec<CostEntry>> {
    let Some(Value::Array(phases)) = value.get("phases") else {
        return Vec::new();
    };
    phases.iter().skip(1).map(|p| parse_cost_list(p.get("evolveCost"))).collect()
}

fn parse_skills(value: &Value) -> Vec<RawSkill> {
    let Some(Value::Array(skills)) = value.get("skills") else {
        return Vec::new();
    };
    skills
        .iter()
        .filter_map(|s| {
            let skill_id = s.get("skillId").and_then(Value::as_str)?.to_string();
            let masteries = match s.get("levelUpCostCond") {
                Some(Value::Array(conds)) => conds.iter().map(|c| parse_cost_list(c.get("levelUpCost"))).collect(),
                _ => Vec::new(),
            };
            Some(RawSkill { skill_id, masteries })
        })
        .collect()
}

fn parse_all_skill_lvlup(value: &Value) -> Vec<Vec<CostEntry>> {
    let Some(Value::Array(items)) = value.get("allSkillLvlup") else {
        return Vec::new();
    };
    items.iter().map(|item| parse_cost_list(item.get("lvlUpCost"))).collect()
}

/// char_table.json のCN/JPをマージしながらオペレーター一覧を構築する
/// （Python `AllOperatorsInfo.init` 前半）。
fn build_characters(
    cn_table: &Value,
    jp_table: &Value,
    custom: &BTreeMap<String, String>,
    custom_keys: &HashSet<&str>,
    operators: &mut IndexMap<String, RawOperatorCost>,
    cn_to_ja: &mut HashMap<String, String>,
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
        // 名前解決ロジックは旧 operator_names::fetch_cn_to_ja と完全同一に保つこと
        // （golden test で data/seed/operator_names.json と突き合わせて保証する）。
        let ja_name = jp_value
            .and_then(|jp| jp.get("name"))
            .and_then(Value::as_str)
            .or_else(|| custom.get(cn_name).map(String::as_str));
        if let Some(ja_name) = ja_name {
            cn_to_ja.insert(cn_name.to_string(), ja_name.to_string());
        }

        let (source_value, cn_only) = match jp_value {
            Some(jp) => (jp, false),
            None => (cn_value, true),
        };
        let name = ja_name.map(str::to_string).unwrap_or_else(|| cn_name.to_string());
        let is_recent = cn_only || custom_keys.contains(cn_name);

        let raw = RawOperatorCost {
            id: key.clone(),
            name: name.clone(),
            cn_name: cn_name.to_string(),
            stars: parse_stars(source_value),
            cn_only,
            is_patch: false,
            is_recent,
            phases: parse_phases(source_value),
            skills: parse_skills(source_value),
            all_skill_lvlup: parse_all_skill_lvlup(source_value),
            modules: Vec::new(),
        };
        name_to_id.insert(name, key.clone());
        operators.insert(key.clone(), raw);
    }
}

/// 昇格オペレーター(前衛/医療アーミヤ等)を追加する（Python `AllOperatorsInfo.init` 中盤）。
/// `base_operators`は追加前のスナップショット(元オペレーター名逆引き用)。
fn build_patches(
    cn_patch: &Value,
    jp_patch: &Value,
    custom_keys: &HashSet<&str>,
    base_operators: &IndexMap<String, RawOperatorCost>,
    operators: &mut IndexMap<String, RawOperatorCost>,
    name_to_id: &mut HashMap<String, String>,
) {
    let Some(Value::Object(patch_info_cn)) = cn_patch.get("patchChars") else { return };
    let patch_info_jp = jp_patch.get("patchChars");
    let Some(Value::Object(patch_key_cn)) = cn_patch.get("infos") else { return };

    // Python `originalOperatorName`: tmplIds に patch_key を含む original operator を逆引きする。
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
        let (source_value, cn_only) = match jp_value {
            Some(jp) => (jp, false),
            None => (cn_value, true),
        };
        let Some(profession) = source_value.get("profession").and_then(Value::as_str) else {
            continue;
        };
        let job = job_ja(profession);
        let (original_name, original_cn_name) = find_original(key).unwrap_or(("", ""));
        let name = format!("{original_name}({job})");
        let cn_name = format!("{original_cn_name}({job})");
        let is_recent = cn_only || custom_keys.contains(cn_name.as_str());

        let raw = RawOperatorCost {
            id: key.clone(),
            name: name.clone(),
            cn_name,
            stars: parse_stars(source_value),
            cn_only,
            is_patch: true,
            is_recent,
            phases: parse_phases(source_value),
            skills: parse_skills(source_value),
            all_skill_lvlup: parse_all_skill_lvlup(source_value),
            modules: Vec::new(),
        };
        name_to_id.insert(name, key.clone());
        operators.insert(key.clone(), raw);
    }
}

/// uniequip_table.json からモジュール消費素材を各オペレーターへ追加する
/// （Python `AllOperatorsInfo.init` 後半、`OperatorCosts.addEq`）。
fn build_modules(cn_uniequip: &Value, jp_uniequip: &Value, operators: &mut IndexMap<String, RawOperatorCost>) {
    let Some(Value::Object(equip_dict)) = cn_uniequip.get("equipDict") else { return };
    let jp_equip_dict = jp_uniequip.get("equipDict");

    let mut by_operator: HashMap<String, Vec<RawModule>> = HashMap::new();
    for (equip_id, value) in equip_dict {
        // itemCost==null は統合戦略モジュール等、対象外（Python `if value["itemCost"]==None: continue`）。
        let Some(item_cost_obj) = value.get("itemCost").filter(|v| !v.is_null()).and_then(Value::as_object) else {
            continue;
        };
        // typeName2が空/無しはデフォルトモジュールとしてスキップ（Python `if not eqType: return`）。
        let Some(eq_type) = value.get("typeName2").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(char_id) = value.get("charId").and_then(Value::as_str) else {
            continue;
        };
        if !operators.contains_key(char_id) {
            continue;
        }
        // itemCost・typeName2はCN側の値をそのまま使う。JP側は実装済みか(cnOnly)の判定にのみ使う
        // （Python版もuniEqJsonにはCN値を渡し、cnOnlyだけ後付けで上書きしている）。
        let cn_only = jp_equip_dict.and_then(|jp| jp.get(equip_id.as_str())).is_none();
        let phase_costs: Vec<Vec<CostEntry>> = item_cost_obj.values().map(|v| parse_cost_list(Some(v))).collect();
        by_operator.entry(char_id.to_string()).or_default().push(RawModule {
            eq_type: eq_type.to_string(),
            cn_only,
            phase_costs,
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

    /// 実ネットワークでキャラ/モジュール/昇格情報を取得できるかの疎通確認。
    /// `cargo test -- --ignored` で明示実行する（通常のCIでは走らせない）。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_gamedata() {
        let data = fetch().await.expect("fetch should succeed against real network");
        assert!(!data.operators.is_empty());
        assert!(!data.name_to_id.is_empty());
        let ja = data.to_ja("夜烟");
        assert_ne!(ja, "夜烟", "fetch/突き合わせに失敗している可能性: got {ja}");
        println!("夜烟 -> {ja}");
    }

    /// 名前解決の不変テスト: 旧`operator_names`(削除済み)が残したSeed
    /// `data/seed/operator_names.json`に載っている中国語→日本語変換が、この統合ソースでも
    /// 同一の結果を返すことを保証する（`operator_names`を`operator_data`へ統合した際に
    /// 名前解決ロジックを壊していないかのゴールデン照合）。新規追加オペレーターにより
    /// `operator_data`側の件数が増えるのは許容するが、既存キーの変換結果は一致必須。
    #[tokio::test]
    #[ignore]
    async fn cn_to_ja_matches_legacy_operator_names_seed() {
        #[derive(serde::Deserialize)]
        struct LegacySeed {
            cn_to_ja: std::collections::HashMap<String, String>,
        }
        let legacy: LegacySeed = serde_json::from_str(
            &std::fs::read_to_string("data/seed/operator_names.json").expect("legacy seed should exist"),
        )
        .expect("legacy seed should parse");

        let data = fetch().await.expect("fetch should succeed against real network");

        let mismatches: Vec<String> = legacy
            .cn_to_ja
            .iter()
            .filter_map(|(cn, expected_ja)| {
                let got = data.to_ja(cn);
                if got == expected_ja {
                    None
                } else {
                    Some(format!("{cn}: got={got} expected={expected_ja}"))
                }
            })
            .collect();
        assert!(mismatches.is_empty(), "name resolution regressed:\n{}", mismatches.join("\n"));
    }
}
