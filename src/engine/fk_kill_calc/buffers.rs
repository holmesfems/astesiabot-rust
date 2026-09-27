//! フレームキル計算機のバフカタログ(`data/fk_kill_calc/buffers.yaml`)のロード(P2)。
//!
//! `overrides.rs`と同じく`include_str!`でビルド時埋め込みにする(実行時ファイルI/Oなし)。
//! スキーマは2種類:
//!   - `individual`: 行ごとにチップで選ぶバフ(自己バフ等)。`FkEntry`側の条件は見ない。
//!   - `conditional`: 全体で1回ON/OFFし、`targets`のタグを持つ行にだけ自動で効くバフ。
//!     値は`pct`/`flat`の固定値、または`source`(P4)によるゲームデータからの動的解決の
//!     どちらかを持つ。`bonus`(省略可)は「`bonus.tags`のいずれかをエントリが持つ場合、
//!     基本値の代わりにこちらを採用する(置き換え。加算ではない)」という汎用のタグ限定
//!     ボーナス(特定バフIDにハードコードしない。例: 異格エクシアの
//!     「弾薬スキル+13%、ラテラーノ勢は2倍(26%)」)。`bonus.value`(固定値)/`bonus.mult`
//!     (基本値への倍率。P4)はどちらか一方。
//!
//! **P4で追加した`source`(動的値解決)**: `pct`/`flat`の代わりに
//! `source: { operator: <charId>, talent: <talentIndex>, key: <blackboardキー> }`
//! (素質由来)または`source: { operator, skill_num: "<fk_dataのskill_num>", key }`
//! (スキルLv別blackboard由来)を指定すると、値をゲームデータ(`operator_combat`の
//! `talents`/モジュール素質上書き、または`skill_data`の`blackboard_by_level`)から
//! 機械抽出する(3層構成のうち1層目)。この解決自体は`build_buffers`(YAMLパースのみで
//! ゲームデータへ依存しない)ではなく、`mod.rs`の`build_conditional_sourced_buffers`
//! (`OperatorCombat`/`SkillData`/`OperatorData`を受け取れる`build_catalog`経由)が担当する
//! (`build_inspire_sources`と同じ2段構え: このファイルはYAMLの生データだけを
//! `raw_conditional_sourced()`で公開し、実際のマージは`mod.rs`側)。
//! 解決アルゴリズムの詳細(昇進/潜在/モジュールの優先順位、値が変わらない軸を隠す
//! dedupe等)は`conditional_source.rs`冒頭コメント参照。
//! `toggle: { label, mult }`(P4)はON/OFFで解決値に`mult`を掛ける単純なトグル
//! (例: 前衛アーミヤの「スキル中は効果2倍」)。
//!
//! 各バフは`pct`(ATKへの割合)/`flat`(定額。P1の`inspireFlat`と同じ差し込み口に足す想定)/
//! `source`のいずれか1つを必ず指定する(`conditional`のみ`source`を選べる。`individual`は
//! 今のところ`pct`/`flat`のみ。0個/2個以上の指定は`build_buffers`がpanicする。
//! ビルド時埋め込みなので実データ側の誤りとして即座に気付ける)。`bonus`を指定する場合、
//! `bonus.value`を使うなら親と同じ種別(pct/flat)を使うこと(親がpctなのにbonus.flatを
//! 指定する、等は不可)。`bonus.mult`は`source`付きバフ専用。

use super::dto::{Buffer, BufferBonus, BufferKind, BufferScope};
use serde::Deserialize;
use std::sync::OnceLock;

const BUFFERS_YAML: &str = include_str!("../../../data/fk_kill_calc/buffers.yaml");

/// `inspire.*.self_parts.*.module_override`の生データ(P3)。
/// フィールドの意味は`buffers.yaml`末尾のコメント + `dto::InspireModuleOverride`参照。
#[derive(Deserialize, Clone, Debug)]
pub struct RawInspireModuleOverride {
    pub module: String,
    pub pct_by_level: [f64; 3],
    pub potential_bonus_by_level: [f64; 3],
}

/// `inspire.*.self_parts`の生データ1件分(P3)。
#[derive(Deserialize, Clone, Debug)]
pub struct RawInspireSelfPart {
    pub id: String,
    pub label: String,
    /// 省略時は`label`を使う(`build_self_part`で埋める)。
    pub short_label: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub pct: f64,
    #[serde(default)]
    pub pct_potential_bonus: f64,
    pub module_override: Option<RawInspireModuleOverride>,
    pub requires_module: Option<String>,
    pub pct_by_module_level: Option<[f64; 3]>,
    pub replaces: Option<String>,
    #[serde(default)]
    pub always_on: bool,
    #[serde(default)]
    pub default_on: bool,
}

/// `inspire.*.skills`の生データ1件分(P3)。
#[derive(Deserialize, Clone, Debug)]
pub struct RawInspireSkill {
    pub skill_num: String,
    pub ratio: f64,
}

/// `inspire`リスト1件分の生データ(P3)。`operator`(operator_combatのid)と
/// `name`はYAML側の表示用で、実際のatk/tags等は`build_catalog`が
/// `operator_combat`から引いて合成する(このモジュールは`OperatorCombat`に依存しないため)。
#[derive(Deserialize, Clone, Debug)]
pub struct RawInspireSource {
    pub id: String,
    pub operator: String,
    pub name: String,
    pub skills: Vec<RawInspireSkill>,
    pub talent_potential_label: String,
    pub self_parts: Vec<RawInspireSelfPart>,
}

/// `conditional.*.bonus`の生データ。`pct`/`flat`(固定値。旧来)と`mult`(基本値への倍率。
/// P4で追加。`source`付きバフ専用)はどちらか一方。
#[derive(Deserialize, Clone, Debug)]
pub(crate) struct RawBonus {
    pub(crate) tags: Vec<String>,
    pub(crate) pct: Option<f64>,
    pub(crate) flat: Option<f64>,
    #[serde(default)]
    pub(crate) mult: Option<f64>,
    pub(crate) note: Option<String>,
}

/// `conditional.*.source`の生データ(P4)。`talent`/`skill_num`のどちらか一方を持つ
/// (`build_conditional_sourced_buffers`が検証する)。
#[derive(Deserialize, Clone, Debug)]
pub(crate) struct RawConditionalSource {
    pub(crate) operator: String,
    pub(crate) talent: Option<usize>,
    pub(crate) skill_num: Option<String>,
    pub(crate) key: String,
}

/// `conditional.*.toggle`の生データ(P4)。
#[derive(Deserialize, Clone, Debug)]
pub(crate) struct RawToggle {
    pub(crate) label: String,
    pub(crate) mult: f64,
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
pub(crate) struct RawConditional {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) pct: Option<f64>,
    pub(crate) flat: Option<f64>,
    pub(crate) targets: Vec<String>,
    pub(crate) bonus: Option<RawBonus>,
    pub(crate) exclusive_group: Option<String>,
    pub(crate) note: Option<String>,
    /// P4: ゲームデータからの動的値解決。`Some`なら`pct`/`flat`は指定しない
    /// (`build_buffers`はこの手のconditionalを対象外にし、`raw_conditional_sourced()`
    /// 経由で`mod.rs`側に解決を委ねる)。
    #[serde(default)]
    pub(crate) source: Option<RawConditionalSource>,
    #[serde(default)]
    pub(crate) toggle: Option<RawToggle>,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct BuffersYaml {
    #[serde(default)]
    individual: Vec<RawIndividual>,
    #[serde(default)]
    conditional: Vec<RawConditional>,
    /// 鼓舞ソース一覧(P3)。P1/P2時点では常に空。
    #[serde(default)]
    inspire: Vec<RawInspireSource>,
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

/// 固定値(`pct`/`flat`)のbonusを組み立てる(旧来の置き換え方式)。`mult`指定時は
/// `build_conditional_sourced_buffers`(mod.rs)側が別途組み立てるので、ここは呼ばない。
fn build_bonus(parent_id: &str, parent_kind: BufferKind, raw: &RawBonus) -> BufferBonus {
    assert!(raw.mult.is_none(), "buffers.yaml: conditional {parent_id}.bonus.mult は固定値バフでは使えない(source付きバフ専用)");
    let (bonus_kind, value) = kind_and_value(raw.pct, raw.flat, &format!("conditional {parent_id}.bonus"));
    assert_eq!(bonus_kind, parent_kind, "buffers.yaml: conditional {parent_id}.bonus は親と同じ種別(pct/flat)を使うこと");
    BufferBonus { target_tags: raw.tags.clone(), value: Some(value), mult: None, note: raw.note.clone() }
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
            exclusive_group: None,
            source: None,
            toggle: None,
            note: ind.note,
        });
    }

    // P4: `source`付きのconditionalは値をゲームデータから解決する必要があるため、ここでは
    // 組み立てず`raw_conditional_sourced()`経由で`mod.rs::build_conditional_sourced_buffers`
    // に任せる(固定`pct`/`flat`のconditionalだけをここで組み立てる)。
    for cond in parsed.conditional {
        if cond.source.is_some() {
            continue;
        }
        let (kind, value) = kind_and_value(cond.pct, cond.flat, &format!("conditional {}", cond.id));
        let bonus = cond.bonus.as_ref().map(|b| build_bonus(&cond.id, kind, b));
        buffers.push(Buffer {
            id: cond.id,
            name: cond.name,
            kind,
            value,
            scope: BufferScope::Conditional { target_tags: cond.targets },
            single_target: false,
            bonus,
            exclusive_group: cond.exclusive_group,
            source: None,
            toggle: None,
            note: cond.note,
        });
    }

    buffers
}

static BUFFERS: OnceLock<Vec<Buffer>> = OnceLock::new();

/// プロセス内で1回だけパースして使い回す(`Overrides::global()`と同じ方針)。
/// `source`付きのconditionalは含まない(`raw_conditional_sourced()`参照)。
pub fn global() -> &'static [Buffer] {
    BUFFERS.get_or_init(build_buffers)
}

static CONDITIONAL_SOURCED_RAW: OnceLock<Vec<RawConditional>> = OnceLock::new();

fn build_conditional_sourced_raw() -> Vec<RawConditional> {
    let parsed: BuffersYaml = serde_yaml::from_str(BUFFERS_YAML).expect("buffers.yamlはビルド時埋め込みなので必ずパースできるはず");
    parsed.conditional.into_iter().filter(|c| c.source.is_some()).collect()
}

/// `source`付きconditionalの生データ(P4)。`build_catalog`(mod.rs)が`operator_combat`/
/// `skill_data`とマージして値を解決し、`dto::Buffer`を組み立てる材料として使う。
/// プロセス内で1回だけパースして使い回す(`global()`と同じ方針。BUFFERS_YAMLを複数回
/// パースする無駄はあるが、P1/P2からの構造を崩さないための素直な実装
/// =`raw_inspire_sources()`と同じ理由)。
pub(crate) fn raw_conditional_sourced() -> &'static [RawConditional] {
    CONDITIONAL_SOURCED_RAW.get_or_init(build_conditional_sourced_raw)
}

/// [`build_bonus`]のsource付き版を`mod.rs`から呼べるように公開する
/// (固定値bonus(`pct`/`flat`)は`source`付きバフでも使えるため。`mult`のみのbonusは
/// `mod.rs`側で直接組み立てる)。
pub(crate) fn build_fixed_bonus(parent_id: &str, parent_kind: BufferKind, raw: &RawBonus) -> BufferBonus {
    build_bonus(parent_id, parent_kind, raw)
}

static INSPIRE_RAW: OnceLock<Vec<RawInspireSource>> = OnceLock::new();

fn build_inspire_raw() -> Vec<RawInspireSource> {
    let parsed: BuffersYaml = serde_yaml::from_str(BUFFERS_YAML).expect("buffers.yamlはビルド時埋め込みなので必ずパースできるはず");
    parsed.inspire
}

/// `inspire`リストの生データ(P3)。`build_catalog`(mod.rs)が`operator_combat`と
/// マージして`dto::InspireSource`を組み立てる材料として使う。プロセス内で1回だけ
/// パースして使い回す(`global()`と同じ方針。BUFFERS_YAMLを2回パースする無駄はあるが、
/// P1/P2からの構造(BuffersYaml一枚をserde_yamlでパース)を崩さないための素直な実装)。
pub fn raw_inspire_sources() -> &'static [RawInspireSource] {
    INSPIRE_RAW.get_or_init(build_inspire_raw)
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
        // P4: source付きconditionalのidも同じ名前空間で重複しないこと(global()には含まれない)。
        for c in raw_conditional_sourced() {
            assert!(seen.insert(c.id.as_str()), "buffers.yamlのconditional(source付き) id'{}'が既存のバフidと重複している", c.id);
        }
        // P3: 鼓舞ソースのidも同じ名前空間で重複しないこと。
        for s in raw_inspire_sources() {
            assert!(seen.insert(s.id.as_str()), "buffers.yamlのinspire id'{}'が既存のバフidと重複している", s.id);
        }
    }

    /// P3: 鼓舞ソースの`self_parts`が`replaces`で指す先が同じソース内に実在すること。
    #[test]
    fn inspire_self_part_replaces_points_to_an_existing_part_in_the_same_source() {
        for s in raw_inspire_sources() {
            let ids: std::collections::HashSet<&str> = s.self_parts.iter().map(|p| p.id.as_str()).collect();
            for p in &s.self_parts {
                if let Some(target) = &p.replaces {
                    assert!(ids.contains(target.as_str()), "buffers.yaml: inspire '{}'のパーツ'{}'のreplaces'{target}'が同じソース内に無い", s.id, p.id);
                }
            }
        }
    }

    /// P3: 鼓舞ソースのskillsが空でないこと・skill_numが空文字でないことの疎通確認
    /// (実在スキルとの突き合わせは`mod.rs`の`validate_inspire_sources`で行う)。
    #[test]
    fn inspire_sources_have_at_least_one_skill() {
        for s in raw_inspire_sources() {
            assert!(!s.skills.is_empty(), "buffers.yaml: inspire '{}'にskillsが無い", s.id);
            for sk in &s.skills {
                assert!(!sk.skill_num.trim().is_empty(), "buffers.yaml: inspire '{}'のskill_numが空", s.id);
            }
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
        // P4: source付きconditionalのtargets/bonus.tagsも同じ語彙で検証する
        // (global()に含まれないため別途チェックが要る)。
        for c in raw_conditional_sourced() {
            for t in &c.targets {
                assert!(vocab.contains(&t.as_str()), "buffers.yaml: '{}'のtargetタグ'{t}'がtag_vocabulary()に無い", c.id);
            }
            if let Some(bonus) = &c.bonus {
                for t in &bonus.tags {
                    assert!(vocab.contains(&t.as_str()), "buffers.yaml: '{}'のbonus.tags'{t}'がtag_vocabulary()に無い", c.id);
                }
            }
        }
    }

    #[test]
    fn values_are_in_a_sane_range() {
        for b in global() {
            assert!(value_in_sane_range(b.kind, b.value), "buffers.yaml: '{}'の値{}が現実的な範囲外", b.id, b.value);
            if let Some(bonus) = &b.bonus {
                if let Some(v) = bonus.value {
                    assert!(value_in_sane_range(b.kind, v), "buffers.yaml: '{}'のbonus値{v}が現実的な範囲外", b.id);
                }
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
        for c in raw_conditional_sourced() {
            assert!(!c.targets.is_empty(), "buffers.yaml: conditional '{}' はtargetsを1つ以上持つこと", c.id);
        }
    }

    /// P4: `source`の`talent`/`skill_num`はどちらか一方だけを持つこと(0個/2個はNG)。
    #[test]
    fn every_conditional_source_has_exactly_one_of_talent_or_skill_num() {
        for c in raw_conditional_sourced() {
            let source = c.source.as_ref().expect("raw_conditional_sourced()はsourceを持つはず");
            assert_ne!(
                source.talent.is_some(),
                source.skill_num.is_some(),
                "buffers.yaml: '{}'のsourceはtalent/skill_numのどちらか一方だけを指定すること",
                c.id
            );
        }
    }

    /// 異格エクシア(P2追加、P4でsource化)のbonusがラテラーノ2倍(mult)であること。
    /// 実際に解決される数値(.13/.26)は`mod.rs`のend-to-endテストで検証する
    /// (ここではYAMLの生データのみ確認する)。
    #[test]
    fn exusiai_alter_conditional_buff_has_laterano_mult_bonus() {
        let c = raw_conditional_sourced().iter().find(|c| c.id == "exusiai_alter").expect("exusiai_alterがbuffers.yamlに存在すること");
        let bonus = c.bonus.as_ref().expect("exusiai_alterにbonusがあるはず");
        assert_eq!(bonus.mult, Some(2.0));
        assert_eq!(bonus.tags, vec!["ラテラーノ".to_string()]);
    }
}
