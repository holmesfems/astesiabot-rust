//! skill_table.json の1スキルレベル分に対応する生JSON構造体。
//! `description.rs` の説明文組み立てが必要とするフィールドのみ持つ
//! （levelUpCost等、消費素材まわりは`operator_data.rs`側が別途扱うため対象外）。

use serde::Deserialize;

#[derive(Deserialize)]
pub struct RawSkillLevel {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "skillType")]
    pub skill_type: String,
    #[serde(rename = "spData")]
    pub sp_data: RawSpData,
    /// この値そのものがJSON上常に小数リテラル(例: `8.0`)であるため、
    /// ヘッダ表示ではPython `str(float)`同様に整数値でも小数点付きで表示する
    /// （`description::python_float_str`を参照）。
    pub duration: f64,
    pub blackboard: Vec<RawBlackboardItem>,
}

#[derive(Deserialize)]
pub struct RawSpData {
    #[serde(rename = "spType")]
    pub sp_type: String,
    #[serde(rename = "initSp")]
    pub init_sp: i64,
    #[serde(rename = "spCost")]
    pub sp_cost: i64,
}

#[derive(Deserialize)]
pub struct RawBlackboardItem {
    pub key: String,
    pub value: Option<f64>,
}
