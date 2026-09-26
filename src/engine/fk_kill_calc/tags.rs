//! オペレーターの`profession`/`position`(operator_combatの生値)から、フレームキル計算機の
//! フィルタ用タグ・ダメージ属性の初期値を推測する。あくまで機械的な推測であり、実データと
//! ズレる場合は`overrides.yaml`の`damage_type`で個別に上書きする想定。

use super::dto::DamageType;

/// 職業ID(profession)ごとの日本語タグ(`operator_data.rs`の`JOB_ID_TO_NAME`と同じ日本語表記)。
/// あちらは昇格オペレーターの名前組み立てに使うが、こちらはタグ一覧の材料として複製する
/// (依存を作らないため。`operator_combat.rs`が既にこの複製方針を採っている)。
const PROFESSION_TAGS: &[(&str, &str)] = &[
    ("PIONEER", "先鋒"),
    ("WARRIOR", "前衛"),
    ("TANK", "重装"),
    ("SNIPER", "狙撃"),
    ("CASTER", "術師"),
    ("MEDIC", "医療"),
    ("SUPPORT", "補助"),
    ("SPECIAL", "特殊"),
];

/// profession + position からタグ一覧を組み立てる。
/// `position == "MELEE"` なら"近距離"を追加する(参照スプレッドシートの列構成
/// 近距離/先鋒/前衛/術師/補助/狙撃に合わせた。"遠距離"に相当する明示タグは持たない)。
pub fn tags_for(profession: &str, position: &str) -> Vec<String> {
    let mut tags = Vec::new();
    if position == "MELEE" {
        tags.push("近距離".to_string());
    }
    if let Some((_, ja)) = PROFESSION_TAGS.iter().find(|(id, _)| *id == profession) {
        tags.push(ja.to_string());
    }
    tags
}

/// スキルのダメージ属性を職業から推測する(術師/医療/補助は術ダメージ、それ以外は物理ダメージ
/// と仮定する簡易ルール)。実データと異なる場合は`overrides.yaml`の`damage_type`で個別に上書きする。
pub fn guess_damage_type(profession: &str) -> DamageType {
    match profession {
        "CASTER" | "MEDIC" | "SUPPORT" => DamageType::Arts,
        _ => DamageType::Physical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melee_position_adds_close_range_tag_before_profession_tag() {
        assert_eq!(tags_for("WARRIOR", "MELEE"), vec!["近距離".to_string(), "前衛".to_string()]);
    }

    #[test]
    fn ranged_position_has_no_close_range_tag() {
        assert_eq!(tags_for("SNIPER", "RANGED"), vec!["狙撃".to_string()]);
    }

    #[test]
    fn unknown_profession_yields_no_profession_tag() {
        assert_eq!(tags_for("UNKNOWN", "RANGED"), Vec::<String>::new());
    }

    #[test]
    fn caster_medic_support_guess_arts() {
        assert_eq!(guess_damage_type("CASTER"), DamageType::Arts);
        assert_eq!(guess_damage_type("MEDIC"), DamageType::Arts);
        assert_eq!(guess_damage_type("SUPPORT"), DamageType::Arts);
    }

    #[test]
    fn others_guess_physical() {
        for p in ["PIONEER", "WARRIOR", "TANK", "SNIPER", "SPECIAL"] {
            assert_eq!(guess_damage_type(p), DamageType::Physical);
        }
    }
}
