//! スキル説明文の組み立て（Python `SkillIdToName.SkillItem`相当）。
//!
//! Python版は`{key[idx].attr}`のようなブラケット添字記法を潰すために説明文
//! 全体から`[`,`]`,`.`を無条件に除去するが、これは数値フォーマット指定子
//! (`{value:0.0%}`等)内の小数点も巻き込んで壊してしまう副作用があり、実データ上
//! 404件が本来の"25.0%"ではなく"25.000000%"のような表示になる（Python
//! `str.format`が精度なしの`%`指定を6桁精度扱いするため）。またPython版は
//! `chain.max_target`というキーを無条件に`max_target`へ統合するが、実データには
//! 両者が別の値を持つスキル(例: `skchr_halo_1`)が存在し、統合すると片方の値が
//! 消える。
//!
//! そのためここでは「`{...}`区間だけを見つけ、フィールド名部分にだけ
//! ブラケット除去を適用し、フォーマット指定子はそのまま保持する」パーサ方式を
//! 採用し、上記2点を実データに即して正しく表示する（Python版とは意図的に
//! 異なる出力になるが、実データ調査の結果こちらが実際のゲーム内表示に近い）。

use super::raw::RawSkillLevel;
use fancy_regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[@$]ba\.?(dt\.)?[a-z0-9]+>").expect("tag pattern must compile"))
}

fn placeholder_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{[^{}]*\}").expect("placeholder pattern must compile"))
}

/// ハイライトタグ（`<@ba...>`/`<$ba(.dt.)?...>`の開始タグと`</>`終了タグ）を除去する。
/// 中身のテキストは残す。
fn strip_tags(s: &str) -> String {
    let without_open = tag_regex().replace_all(s, "");
    without_open.replace("</>", "")
}

/// タグ除去後に適用するリテラル置換（Python `replace_byDict`相当。ただし
/// `chain.max_target`→`max_target`は前述の理由で採用しない）。
fn apply_literal_fixups(s: &str) -> String {
    s.replace("-{-", "{").replace("{-", "-{").replace("\\n", "\n").replace("AOE", "aoe")
}

/// プレースホルダのフィールド名部分だけに適用するクリーニング（Python `cleanStr`
/// と同じ文字集合`[`,`]`,`.`除去だが、フォーマット指定子には適用しない）。
fn clean_field_name(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '[' | ']' | '.')).collect()
}

/// Python `checkint`。数学的に整数な値は小数点無しで表示する。
fn number_str(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn percent_str(value: f64, decimals: usize) -> String {
    format!("{:.*}%", decimals, value * 100.0)
}

fn fixed1_str(value: f64) -> String {
    format!("{value:.1}")
}

/// `{field}`/`{field:spec}`1個分を解決する。フィールドが見つからない、または
/// 未知のフォーマット指定子の場合は`None`（呼び出し側で`{...}`をそのまま残す）。
/// 実データ調査済みの4種類（指定子なし/`"0"`/`"0%"`/`".0%"`/`"0.0%"`/`"0.0"`）のみ対応する。
fn resolve_placeholder(inner: &str, values: &HashMap<String, f64>) -> Option<String> {
    let (field, spec) = match inner.find(':') {
        Some(idx) => (&inner[..idx], Some(&inner[idx + 1..])),
        None => (inner, None),
    };
    let field = clean_field_name(field);
    let value = *values.get(&field).or_else(|| values.get(&field.to_lowercase()))?;
    match spec {
        None | Some("0") => Some(number_str(value)),
        Some("0%") | Some(".0%") => Some(percent_str(value, 0)),
        Some("0.0%") => Some(percent_str(value, 1)),
        Some("0.0") => Some(fixed1_str(value)),
        Some(_) => None,
    }
}

fn render_placeholders(template: &str, values: &HashMap<String, f64>) -> String {
    let re = placeholder_regex();
    let mut out = String::with_capacity(template.len());
    let mut last_end = 0;
    for m in re.find_iter(template).flatten() {
        out.push_str(&template[last_end..m.start()]);
        let inner = &template[m.start() + 1..m.end() - 1];
        match resolve_placeholder(inner, values) {
            Some(resolved) => out.push_str(&resolved),
            None => out.push_str(m.as_str()),
        }
        last_end = m.end();
    }
    out.push_str(&template[last_end..]);
    out
}

/// blackboard(+durationのフォールバック)からプレースホルダ解決用の値マップを作る。
fn build_value_map(level: &RawSkillLevel) -> HashMap<String, f64> {
    let mut map = HashMap::new();
    for item in &level.blackboard {
        if let Some(v) = item.value {
            map.insert(clean_field_name(&item.key), v);
        }
    }
    map.entry("duration".to_string()).or_insert(level.duration);
    map
}

fn skill_type_label(raw: &str) -> String {
    match raw {
        "AUTO" => "自動発動".to_string(),
        "MANUAL" => "手動発動".to_string(),
        "PASSIVE" => "パッシブ".to_string(),
        other => other.to_string(),
    }
}

fn sp_type_label(raw: &str) -> String {
    match raw {
        "INCREASE_WHEN_TAKEN_DAMAGE" => "被撃回復".to_string(),
        "INCREASE_WHEN_ATTACK" => "攻撃回復".to_string(),
        "INCREASE_WITH_TIME" => "自然回復".to_string(),
        other => other.to_string(),
    }
}

/// Python `str(float)`相当（整数値でも`.0`を残す）。ヘッダのduration表示専用
/// （blackboard由来の値は`number_str`のcheckint方式を使う。別ルールなので混同しないこと）。
fn python_float_str(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// 最大レベルのスキルデータから完成済みの説明文を組み立てる（Python
/// `SkillItem.description`プロパティ相当。ヘッダ(発動種別/SP/持続時間)込み）。
/// 説明文が無いスキルは空文字列を返す。
pub fn build_description(level: &RawSkillLevel) -> String {
    let Some(raw_desc) = level.description.as_deref().filter(|d| !d.is_empty()) else {
        return String::new();
    };

    let stripped = strip_tags(raw_desc);
    let fixed = apply_literal_fixups(&stripped);
    let values = build_value_map(level);
    let body = render_placeholders(&fixed, &values).replace("--", "");

    let is_passive = level.skill_type == "PASSIVE";
    let mut header = if is_passive {
        skill_type_label(&level.skill_type)
    } else {
        format!("{}/{}", sp_type_label(&level.sp_data.sp_type), skill_type_label(&level.skill_type))
    };
    if level.duration > 0.0 {
        header.push_str(&format!(" ⌚{}秒", python_float_str(level.duration)));
    }

    let pre_info = if is_passive {
        header
    } else {
        format!("{header}\n▶{} ⚡{}", level.sp_data.init_sp, level.sp_data.sp_cost)
    };

    format!("{pre_info}\n{body}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::skill_data::raw::RawBlackboardItem;

    fn level(
        skill_type: &str,
        sp_type: &str,
        init_sp: i64,
        sp_cost: i64,
        duration: f64,
        description: &str,
        blackboard: &[(&str, f64)],
    ) -> RawSkillLevel {
        RawSkillLevel {
            name: "テストスキル".to_string(),
            description: Some(description.to_string()),
            skill_type: skill_type.to_string(),
            sp_data: super::super::raw::RawSpData {
                sp_type: sp_type.to_string(),
                init_sp,
                sp_cost,
            },
            duration,
            blackboard: blackboard
                .iter()
                .map(|(k, v)| RawBlackboardItem {
                    key: k.to_string(),
                    value: Some(*v),
                })
                .collect(),
        }
    }

    #[test]
    fn no_description_yields_empty_string() {
        let lv = RawSkillLevel {
            name: "無説明".to_string(),
            description: None,
            skill_type: "PASSIVE".to_string(),
            sp_data: super::super::raw::RawSpData {
                sp_type: "INCREASE_WITH_TIME".to_string(),
                init_sp: 0,
                sp_cost: 0,
            },
            duration: 0.0,
            blackboard: vec![],
        };
        assert_eq!(build_description(&lv), "");
    }

    /// skcom_quickattack[1]相当: プレースホルダ+パーセント、手動発動。
    #[test]
    fn manual_skill_with_percent_and_plain_placeholder() {
        let lv = level(
            "MANUAL",
            "INCREASE_WITH_TIME",
            0,
            45,
            25.0,
            "攻击力<@ba.vup>+{atk:0%}</>，攻击速度<@ba.vup>+{attack_speed}</>",
            &[("atk", 0.25), ("attack_speed", 25.0)],
        );
        assert_eq!(
            build_description(&lv),
            "自然回復/手動発動 ⌚25.0秒\n▶0 ⚡45\n攻击力+25%，攻击速度+25"
        );
    }

    /// skchr_indigo_2相当: ブラケット添字記法(`foo[bar].baz`)+複数プレースホルダ。
    #[test]
    fn bracket_subscript_placeholder_is_flattened_and_distinct_from_similar_key() {
        let lv = level(
            "PASSIVE",
            "INCREASE_WITH_TIME",
            0,
            0,
            20.0,
            "触发几率提升至{talent_scale:0.0}倍，每<@ba.vup>{indigo_s_2[damage].interval}</>秒受到{indigo_s_2[damage].atk_scale:0%}的伤害",
            &[
                ("talent_scale", 3.0),
                ("indigo_s_2[damage].atk_scale", 0.2),
                ("indigo_s_2[damage].interval", 0.5),
            ],
        );
        assert_eq!(
            build_description(&lv),
            "パッシブ ⌚20.0秒\n触发几率提升至3.0倍，每0.5秒受到20%的伤害"
        );
    }

    /// skchr_halo_1相当: `max_target`と`chain.max_target`が別々の値を持つケース
    /// （Python版はここを統合してしまい片方の値を失うが、本実装では区別する）。
    #[test]
    fn max_target_and_chain_max_target_stay_distinct() {
        let lv = level(
            "MANUAL",
            "INCREASE_WITH_TIME",
            0,
            0,
            0.0,
            "攻击<@ba.vup>{max_target}</>个目标，最多在<@ba.vup>{chain.max_target}</>个敌人间跳跃",
            &[("max_target", 2.0), ("chain.max_target", 4.0)],
        );
        assert_eq!(
            build_description(&lv),
            "自然回復/手動発動\n▶0 ⚡0\n攻击2个目标，最多在4个敌人间跳跃"
        );
    }

    /// skcom_range_extend相当: 大文字プレースホルダが小文字blackboardキーに解決される。
    #[test]
    fn uppercase_placeholder_resolves_against_lowercase_key() {
        let lv = level(
            "MANUAL",
            "INCREASE_WITH_TIME",
            0,
            0,
            0.0,
            "攻击范围<@ba.vup>+{ABILITY_RANGE_FORWARD_EXTEND}格</>，攻击力<@ba.vup>+{atk:0%}</>",
            &[("atk", 0.4), ("ability_range_forward_extend", 2.0)],
        );
        assert_eq!(
            build_description(&lv),
            "自然回復/手動発動\n▶0 ⚡0\n攻击范围+2格，攻击力+40%"
        );
    }

    /// skchr_nights_2相当: `-{-...}`は値自身の符号をそのまま出す。
    #[test]
    fn negative_value_escape_keeps_sign() {
        let lv = level(
            "MANUAL",
            "INCREASE_WITH_TIME",
            0,
            0,
            0.0,
            "最大生命值<@ba.vdown>-{-max_hp:0%}</>",
            &[("max_hp", -0.75)],
        );
        assert_eq!(build_description(&lv), "自然回復/手動発動\n▶0 ⚡0\n最大生命值-75%");
    }

    /// skchr_yuki_2相当: `{-...}`は符号を反転してから`--`潰しで正の表示にする。
    #[test]
    fn double_negative_escape_collapses_to_positive() {
        let lv = level(
            "MANUAL",
            "INCREASE_WITH_TIME",
            0,
            0,
            0.0,
            "移动速度降低{-attack@move_speed:0%}",
            &[("attack@move_speed", -0.35)],
        );
        assert_eq!(build_description(&lv), "自然回復/手動発動\n▶0 ⚡0\n移动速度降低35%");
    }

    /// skchr_zebra_1相当: blackboardに無い`{duration}`はレベル自身のdurationにフォールバックする。
    #[test]
    fn duration_placeholder_falls_back_to_level_duration() {
        let lv = level(
            "MANUAL",
            "INCREASE_WITH_TIME",
            0,
            0,
            4.0,
            "持续<@ba.vup>{duration}</>秒",
            &[],
        );
        assert_eq!(build_description(&lv), "自然回復/手動発動 ⌚4.0秒\n▶0 ⚡0\n持续4秒");
    }

    /// 0.0%指定子(1桁小数パーセント)が壊れず表示できることの確認（実データで404件該当）。
    #[test]
    fn one_decimal_percent_spec_is_preserved() {
        let lv = level("PASSIVE", "INCREASE_WITH_TIME", 0, 0, 0.0, "会心率+{crit:0.0%}", &[("crit", 0.155)]);
        assert_eq!(build_description(&lv), "パッシブ\n会心率+15.5%");
    }
}
