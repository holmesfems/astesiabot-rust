//! フレームキル計算機の手動補正データ(`data/fk_kill_calc/overrides.yaml`)のロード。
//!
//! 機械データ(character_table.json/skill_table.jsonのblackboard等)だけでは
//! 実際のスキル倍率・Hit数・ダメージ属性を正しく機械的に判定できないケースが
//! 実データ調査で見つかっている(例: blackboardのキー名が`atk_scale`ではなく
//! `damage_by_atk_scale`だったり、`attack@s2.atk_scale`のように接頭辞付きだったりする、
//! 1スキルが物理/術の2系統ダメージを持つ、300%/400%/800%のように状況で倍率が変わる等)。
//! `overrides.yaml`はそれらをオペレーターID+skill_num単位で手動補正するための資産。
//!
//! スキーマ:
//! ```yaml
//! <operator_id>:
//!   "<skill_num>":  # fk_dataシートの生値そのまま("1"〜"3"の他"素質1"等もありうる)
//!     - { label: "300%", multiplier: 3.0 }
//!     - { label: "400%", multiplier: 4.0 }
//! ```
//! 1つのskill_numに複数バリアントを並べられる(状況によって倍率/属性が変わるスキル用)。
//! バリアントの各フィールドは全て省略可能で、省略したフィールドは機械データ側の値を
//! そのまま採用する(`build_catalog`が担当)。フィールド一覧:
//!   - `label`: バリアントの表示名(例: "300%"、"物理")
//!   - `multiplier`: スキル倍率
//!   - `self_atk_pct`: セルフATKバフの割合
//!   - `hits`: Hit数
//!   - `damage_type`: "physical" | "arts" | "true"
//!     (YAMLで`true`を無引用で書くとbool扱いになるため、bool trueも`true`属性として
//!     受け付ける。文字列で書く場合は`"true"`と引用符を付けること)
//!   - `note`: 補足コメント(フロント表示用)
//!
//! ここに載せる値は機械データの自動判定と食い違う実測値であり、オーナーの参照
//! スプレッドシートを出典とする手動補正である。ゲームデータ更新で機械側の値が
//! 変わった場合はこのファイルも見直すこと。

use super::dto::DamageType;
use indexmap::IndexMap;
use serde::Deserialize;
use std::sync::OnceLock;

/// overrides.yaml 1バリアント分。全フィールド省略可能。
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OverrideVariant {
    pub label: Option<String>,
    pub multiplier: Option<f64>,
    pub self_atk_pct: Option<f64>,
    pub hits: Option<u32>,
    pub damage_type: Option<DamageType>,
    pub note: Option<String>,
}

/// operator_id -> skill_num -> バリアント一覧。
pub type OverridesMap = IndexMap<String, IndexMap<String, Vec<OverrideVariant>>>;

/// ビルド時に`include_str!`で埋め込むため、`run_api`/`serve_web`のどちらから実行しても
/// カレントディレクトリに関係なく読める(実行時のファイルI/Oが不要)。
const OVERRIDES_YAML: &str = include_str!("../../../data/fk_kill_calc/overrides.yaml");

pub struct Overrides {
    map: OverridesMap,
}

static OVERRIDES: OnceLock<Overrides> = OnceLock::new();

impl Overrides {
    fn load() -> Self {
        let map: OverridesMap =
            serde_yaml::from_str(OVERRIDES_YAML).expect("overrides.yamlはビルド時埋め込みなので必ずパースできるはず");
        Self { map }
    }

    /// プロセス内で1回だけパースして使い回す(`include_str!`で埋め込んだ内容は不変なため
    /// リクエスト毎に読み直す必要が無い)。
    pub fn global() -> &'static Overrides {
        OVERRIDES.get_or_init(Self::load)
    }

    pub fn variants_for(&self, operator_id: &str, skill_num: &str) -> Option<&Vec<OverrideVariant>> {
        self.map.get(operator_id)?.get(skill_num)
    }

    /// `(operator_id, skill_num)`の全キーを列挙する。ドリフト検知テスト
    /// (`build_catalog`側で実際のfk_data/operatorと突き合わせる)用。
    pub fn all_keys(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map.iter().flat_map(|(op_id, by_skill)| by_skill.keys().map(move |skill_num| (op_id.as_str(), skill_num.as_str())))
    }

    #[cfg(test)]
    pub fn empty_for_test() -> Self {
        Self { map: OverridesMap::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// overrides.yaml自体が壊れていないこと(パース可能・空でない)の疎通確認。
    #[test]
    fn overrides_yaml_parses_and_is_not_empty() {
        let overrides = Overrides::global();
        assert!(!overrides.map.is_empty());
        let ash_variants = overrides.variants_for("char_456_ash", "3").expect("Ashの上書きがあるはず");
        assert_eq!(ash_variants.len(), 3);
    }

    #[test]
    fn damage_type_accepts_bare_bool_true_as_true_damage() {
        let v: OverrideVariant = serde_yaml::from_str("damage_type: true").unwrap();
        assert_eq!(v.damage_type, Some(DamageType::True));
    }

    #[test]
    fn damage_type_rejects_bool_false() {
        let result: Result<OverrideVariant, _> = serde_yaml::from_str("damage_type: false");
        assert!(result.is_err());
    }
}
