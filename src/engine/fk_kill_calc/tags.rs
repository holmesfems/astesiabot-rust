//! オペレーターの`profession`/`position`/`nationId`(operator_combatの生値)から、
//! フレームキル計算機のフィルタ用タグ・ダメージ属性の初期値を推測する。あくまで機械的な
//! 推測であり、実データとズレる場合は`overrides.yaml`の`damage_type`/`tags`で個別に
//! 上書きする想定。
//!
//! `TAG_VOCAB`はP2で追加した「タグの語彙」。`FkEntry.tags`に載りうる全タグ名を1箇所に
//! 集約し、`buffers.yaml`の`targets`/`bonus.tags`のドリフト検知(`buffers.rs`のテスト)が
//! これを参照する。新しいタグ種別(勢力タグ等)を足すときはここに追加すること。

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

/// 勢力ID(nationId、大陸版の生値)→タグ名。バフの対象条件に必要な勢力タグだけを持つ
/// (現時点では「ラテラーノ」バフ1件のみ使うため、ここも1件だけ)。将来別勢力のバフが
/// 増えたらここに追加する。
const FACTION_TAGS: &[(&str, &str)] = &[("laterano", "ラテラーノ")];

/// スキルが「弾薬スキル」(`durationType == "AMMO"`)である場合に付与する機械タグ
/// (`skill_data::SkillData::is_ammo_skill`から判定。`build_catalog`がFkEntry単位で足す)。
pub const AMMO_SKILL_TAG: &str = "弾薬スキル";

/// `FkEntry.tags`に載りうる全タグの語彙(近距離判定+職業+勢力+弾薬スキル)。
/// `overrides.yaml`の手動`tags`、`buffers.yaml`の`targets`/`bonus.tags`はここに
/// 含まれる名前だけを使うことを`cargo test`のドリフト検知(buffers.rs)で保証する。
pub fn tag_vocabulary() -> Vec<&'static str> {
    let mut vocab = vec!["近距離", AMMO_SKILL_TAG];
    vocab.extend(PROFESSION_TAGS.iter().map(|(_, ja)| *ja));
    vocab.extend(FACTION_TAGS.iter().map(|(_, ja)| *ja));
    vocab
}

/// nationIdからタグ名を引く(登録の無い勢力は`None`)。
pub fn faction_tag_for(nation_id: &str) -> Option<&'static str> {
    FACTION_TAGS.iter().find(|(id, _)| *id == nation_id).map(|(_, ja)| *ja)
}

/// profession + position + nationId からオペレーター単位のタグ一覧を組み立てる。
/// `position == "MELEE"` なら"近距離"を追加する(参照スプレッドシートの列構成
/// 近距離/先鋒/前衛/術師/補助/狙撃に合わせた。"遠距離"に相当する明示タグは持たない)。
/// 勢力タグ(例:"ラテラーノ")は該当勢力なら追加する。
/// (弾薬スキルタグはスキル単位なので、ここではなく`build_catalog`がFkEntry組み立て時に足す)。
pub fn tags_for(profession: &str, position: &str, nation_id: &str) -> Vec<String> {
    let mut tags = Vec::new();
    if position == "MELEE" {
        tags.push("近距離".to_string());
    }
    if let Some((_, ja)) = PROFESSION_TAGS.iter().find(|(id, _)| *id == profession) {
        tags.push(ja.to_string());
    }
    if let Some(ja) = faction_tag_for(nation_id) {
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
        assert_eq!(tags_for("WARRIOR", "MELEE", ""), vec!["近距離".to_string(), "前衛".to_string()]);
    }

    #[test]
    fn ranged_position_has_no_close_range_tag() {
        assert_eq!(tags_for("SNIPER", "RANGED", ""), vec!["狙撃".to_string()]);
    }

    #[test]
    fn unknown_profession_yields_no_profession_tag() {
        assert_eq!(tags_for("UNKNOWN", "RANGED", ""), Vec::<String>::new());
    }

    #[test]
    fn laterano_nation_id_adds_faction_tag_after_profession_tag() {
        assert_eq!(tags_for("SPECIAL", "RANGED", "laterano"), vec!["特殊".to_string(), "ラテラーノ".to_string()]);
    }

    #[test]
    fn unknown_nation_id_yields_no_faction_tag() {
        assert_eq!(faction_tag_for("lungmen"), None);
        assert_eq!(faction_tag_for("laterano"), Some("ラテラーノ"));
    }

    #[test]
    fn tag_vocabulary_contains_ammo_and_faction_tags() {
        let vocab = tag_vocabulary();
        assert!(vocab.contains(&AMMO_SKILL_TAG));
        assert!(vocab.contains(&"ラテラーノ"));
        assert!(vocab.contains(&"近距離"));
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
