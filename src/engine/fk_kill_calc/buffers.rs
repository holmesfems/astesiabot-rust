//! フレームキル計算機のバフカタログ(`data/fk_kill_calc/buffers.yaml`)のロード(P2)。
//!
//! `overrides.rs`と同じく`include_str!`でビルド時埋め込みにする(実行時ファイルI/Oなし)。
//! スキーマは2種類:
//!   - `individual`: 行ごとにチップで選ぶバフ(自己バフ等)。`FkEntry`側の条件は見ない。
//!   - `conditional`: 全体で1回ON/OFFし、`targets`のタグを持つ行にだけ自動で効くバフ。
//!     `bonus`(省略可)は「`bonus.tags`のいずれかをエントリが持つ場合、基本値の代わりに
//!     `bonus.pct`/`bonus.flat`を採用する(置き換え。加算ではない)」という汎用の
//!     タグ限定ボーナス(特定バフIDにハードコードしない。例: 異格エクシアの
//!     「弾薬スキル+13%、ラテラーノ勢は2倍(26%)」)。
//!
//! 各バフは`pct`(ATKへの割合)/`flat`(定額。P1の`inspireFlat`と同じ差し込み口に足す想定)の
//! どちらか一方を必ず指定する(両方/どちらも無しは`build_buffers`がpanicする。
//! ビルド時埋め込みなので実データ側の誤りとして即座に気付ける)。`bonus`を指定する場合も
//! 親と同じ種別(pct/flat)を使うこと(親がpctなのにbonus.flatを指定する、等は不可)。

use super::dto::{Buffer, BufferBonus, BufferKind, BufferScope};
use serde::Deserialize;
use std::sync::OnceLock;

const BUFFERS_YAML: &str = include_str!("../../../data/fk_kill_calc/buffers.yaml");

#[derive(Deserialize, Clone, Debug)]
struct RawBonus {
    tags: Vec<String>,
    pct: Option<f64>,
    flat: Option<f64>,
    note: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
struct RawIndividual {
    id: String,
    name: String,
    pct: Option<f64>,
    flat: Option<f64>,
    #[serde(default)]
    single_target: bool,
    note: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
struct RawConditional {
    id: String,
    name: String,
    pct: Option<f64>,
    flat: Option<f64>,
    targets: Vec<String>,
    bonus: Option<RawBonus>,
    note: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct BuffersYaml {
    #[serde(default)]
    individual: Vec<RawIndividual>,
    #[serde(default)]
    conditional: Vec<RawConditional>,
}

/// `pct`/`flat`のどちらか一方だけが指定されていることを検証し、(種別, 値)を返す。
/// 両方/どちらも無しは実データの誤りなのでpanicする(ビルド時埋め込みなので気付ける)。
fn kind_and_value(pct: Option<f64>, flat: Option<f64>, ctx: &str) -> (BufferKind, f64) {
    match (pct, flat) {
        (Some(p), None) => (BufferKind::Pct, p),
        (None, Some(f)) => (BufferKind::Flat, f),
        (None, None) => panic!("buffers.yaml: {ctx} は pct か flat のどちらかを指定すること"),
        (Some(_), Some(_)) => panic!("buffers.yaml: {ctx} は pct と flat を同時に指定できない"),
    }
}

fn build_bonus(parent_id: &str, parent_kind: BufferKind, raw: RawBonus) -> BufferBonus {
    let (bonus_kind, value) = kind_and_value(raw.pct, raw.flat, &format!("conditional {parent_id}.bonus"));
    assert_eq!(bonus_kind, parent_kind, "buffers.yaml: conditional {parent_id}.bonus は親と同じ種別(pct/flat)を使うこと");
    BufferBonus { target_tags: raw.tags, value, note: raw.note }
}

fn build_buffers() -> Vec<Buffer> {
    let parsed: BuffersYaml = serde_yaml::from_str(BUFFERS_YAML).expect("buffers.yamlはビルド時埋め込みなので必ずパースできるはず");
    let mut buffers = Vec::new();

    for ind in parsed.individual {
        let (kind, value) = kind_and_value(ind.pct, ind.flat, &format!("individual {}", ind.id));
        buffers.push(Buffer {
            id: ind.id,
            name: ind.name,
            kind,
            value,
            scope: BufferScope::Individual,
            single_target: ind.single_target,
            bonus: None,
            note: ind.note,
        });
    }

    for cond in parsed.conditional {
        let (kind, value) = kind_and_value(cond.pct, cond.flat, &format!("conditional {}", cond.id));
        let bonus = cond.bonus.map(|b| build_bonus(&cond.id, kind, b));
        buffers.push(Buffer {
            id: cond.id,
            name: cond.name,
            kind,
            value,
            scope: BufferScope::Conditional { target_tags: cond.targets },
            single_target: false,
            bonus,
            note: cond.note,
        });
    }

    buffers
}

static BUFFERS: OnceLock<Vec<Buffer>> = OnceLock::new();

/// プロセス内で1回だけパースして使い回す(`Overrides::global()`と同じ方針)。
pub fn global() -> &'static [Buffer] {
    BUFFERS.get_or_init(build_buffers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fk_kill_calc::tags::tag_vocabulary;

    /// 「そこそこ現実的な範囲」に収まっているかの雑なチェック。ゲームバランス上の
    /// 上限を意味するものではなく、桁間違い(%を小数で書き忘れる等)の検知が目的。
    const SANE_PCT_MAX: f64 = 3.0; // +300%まで
    const SANE_FLAT_MAX: f64 = 5000.0;

    fn value_in_sane_range(kind: BufferKind, value: f64) -> bool {
        match kind {
            BufferKind::Pct => value > 0.0 && value <= SANE_PCT_MAX,
            BufferKind::Flat => value > 0.0 && value <= SANE_FLAT_MAX,
        }
    }

    #[test]
    fn buffers_yaml_parses_and_is_not_empty() {
        let buffers = global();
        assert!(!buffers.is_empty());
    }

    #[test]
    fn ids_are_unique_across_individual_and_conditional() {
        let buffers = global();
        let mut seen = std::collections::HashSet::new();
        for b in buffers {
            assert!(seen.insert(b.id.as_str()), "buffers.yamlのid'{}'が重複している", b.id);
        }
    }

    #[test]
    fn every_target_tag_is_in_the_known_vocabulary() {
        let vocab = tag_vocabulary();
        for b in global() {
            if let BufferScope::Conditional { target_tags } = &b.scope {
                for t in target_tags {
                    assert!(vocab.contains(&t.as_str()), "buffers.yaml: '{}'のtargetタグ'{t}'がtag_vocabulary()に無い", b.id);
                }
            }
            if let Some(bonus) = &b.bonus {
                for t in &bonus.target_tags {
                    assert!(vocab.contains(&t.as_str()), "buffers.yaml: '{}'のbonus.tags'{t}'がtag_vocabulary()に無い", b.id);
                }
            }
        }
    }

    #[test]
    fn values_are_in_a_sane_range() {
        for b in global() {
            assert!(value_in_sane_range(b.kind, b.value), "buffers.yaml: '{}'の値{}が現実的な範囲外", b.id, b.value);
            if let Some(bonus) = &b.bonus {
                assert!(value_in_sane_range(b.kind, bonus.value), "buffers.yaml: '{}'のbonus値{}が現実的な範囲外", b.id, bonus.value);
            }
        }
    }

    #[test]
    fn individual_buffs_have_individual_scope_and_conditional_have_target_tags() {
        for b in global() {
            match &b.scope {
                BufferScope::Individual => {}
                BufferScope::Conditional { target_tags } => {
                    assert!(!target_tags.is_empty(), "buffers.yaml: conditional '{}' はtargetsを1つ以上持つこと", b.id);
                }
            }
        }
    }

    /// 異格エクシア(P2追加)の値がオーナー確定値+ラテラーノ2倍ボーナスと一致すること。
    #[test]
    fn exusiai_alter_conditional_buff_has_laterano_bonus() {
        let b = global().iter().find(|b| b.id == "exusiai_alter").expect("exusiai_alterがbuffers.yamlに存在すること");
        assert_eq!(b.value, 0.13);
        let bonus = b.bonus.as_ref().expect("exusiai_alterにbonusがあるはず");
        assert_eq!(bonus.value, 0.26);
        assert_eq!(bonus.target_tags, vec!["ラテラーノ".to_string()]);
    }
}
