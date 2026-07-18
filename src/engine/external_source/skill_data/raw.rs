//! skill_table.json の1スキルレベル分に対応する生JSON構造体。
//! `description.rs` の説明文組み立てが必要とするフィールドのみ持つ
//! （levelUpCost等、消費素材まわりは`operator_data.rs`側が別途扱うため対象外）。

use serde::{Deserialize, Deserializer};
use std::fmt;

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
    /// 本来は`"INCREASE_WITH_TIME"`等の文字列だが、一部スキル(PASSIVE中心に600件超、
    /// 2024年時点のCNデータで確認済み)は数値(例: `8`)が入っている。ここを`String`型で
    /// 厳格にパースするとそのスキル1件全体(name/descriptionを含む)がdeserializeに失敗し、
    /// スキル名解決が丸ごと欠落する（Python版はduck typingで同じ値を無条件に保持するため
    /// この欠落は起きない）。`string_or_number`で数値も文字列化して受け付け、Python版と
    /// 同じ「name/descriptionは必ず解決できる」を保証する。
    #[serde(rename = "spType", deserialize_with = "string_or_number")]
    pub sp_type: String,
    #[serde(rename = "initSp")]
    pub init_sp: i64,
    #[serde(rename = "spCost")]
    pub sp_cost: i64,
}

/// JSON上の文字列/数値のどちらでも受け付けて`String`に正規化する。
fn string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrNumber;

    impl serde::de::Visitor<'_> for StringOrNumber {
        type Value = String;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string or a number")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }

        fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }

    deserializer.deserialize_any(StringOrNumber)
}

#[derive(Deserialize)]
pub struct RawBlackboardItem {
    pub key: String,
    pub value: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実データで確認された退行ケース: `spData.spType`が数値(例: `8`)でもスキル1件全体の
    /// deserializeが失敗しないこと（回帰: イネスのスキル3等でスキル名が"Missing"になっていた）。
    #[test]
    fn sp_type_accepts_numeric_value() {
        let json = serde_json::json!({
            "name": "孤影の帰路",
            "description": "説明文",
            "skillType": "PASSIVE",
            "spData": {
                "spType": 8,
                "initSp": 0,
                "spCost": 0
            },
            "duration": 16.0,
            "blackboard": []
        });
        let level: RawSkillLevel = serde_json::from_value(json).expect("numeric spType should not fail parsing");
        assert_eq!(level.sp_data.sp_type, "8");
        assert_eq!(level.name, "孤影の帰路");
    }

    #[test]
    fn sp_type_accepts_string_value() {
        let json = serde_json::json!({
            "name": "スキル",
            "description": null,
            "skillType": "AUTO",
            "spData": {
                "spType": "INCREASE_WITH_TIME",
                "initSp": 0,
                "spCost": 10
            },
            "duration": 0.0,
            "blackboard": []
        });
        let level: RawSkillLevel = serde_json::from_value(json).expect("string spType should still parse");
        assert_eq!(level.sp_data.sp_type, "INCREASE_WITH_TIME");
    }
}
