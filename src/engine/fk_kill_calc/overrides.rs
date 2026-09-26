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
//!   - `tags`: 追加のタグ(機械判定のタグに加算する。省略時は追加無し)。P2で追加。
//!     `engine::fk_kill_calc::tags::tag_vocabulary()`に無い名前は
//!     `validate_overrides`系のドリフト検知(`buffers.rs`)で弾かれる想定。
//!   - `special`: 「特殊強化」トグル(P2で追加)。省略可能。無いスキルはUIにチェックボックスを
//!     出さない。`label`以外は全フィールド省略可能で、以下の効果を持てる:
//!       - `label`: チェックボックス/ヒントの表示名(必須)
//!       - `description`: ⓘボタンで開く説明文(条件+効果を短く。P2 follow-upで追加。
//!         フロントが末尾に「現在: +N%」/「現在: ×N」を動的に付け足す)
//!       - **加算系**(`requires_module` + `add_self_atk_pct_by_module_level`): モジュール依存の
//!         特殊強化。`requires_module`(uniEquipId)を指定したモジュールを装備している時だけ、
//!         `add_self_atk_pct_by_module_level`の対応するLv(1〜3。要素数3必須。効果の無いLvは
//!         0を入れる。Lv1は素質強化自体が無いスキルが多いので0になりがち)を
//!         **セルフ%に加算**する(置き換えではない)。ONかつモジュール条件を満たす時だけ有効
//!         (フロント`engine.js`の`resolveSpecialAddPct`が都度計算する。行フィールドへ
//!         スナップショットしない＝モジュールを変えれば即座に反映される)。モジュール未装備/
//!         条件を満たさない時はUIがチェックボックスの代わりにヒントを出す。
//!         例: ブレイズS3「待機ボーナス」はモジュールX(`uniequip_002_huang`)Lv1=0/Lv2=0.04/Lv3=0.06
//!       - **乗算系**(`mul_multiplier`。P2 follow-up 2回目で追加): `base`(モジュール未装備/
//!         条件を満たさない時の倍率。E2最大潜在の素質値をそのまま使う想定) +
//!         `module`(uniEquipId、省略可) + `by_module_level`(該当モジュールをLv1〜3で装備時の
//!         倍率。要素数3必須。素質強化が無いLvは`base`と同じ値を入れる)。行の`multiplier`
//!         (スキル倍率)に**乗算**する(置き換えでも加算でもない)。モジュール条件を満たさない/
//!         `module`省略時は常に`base`を使う(＝この系統は「常に適用可能」なのでUIは常時
//!         チェックボックスを出し、ヒントには切り替えない)。ONの間だけ有効
//!         (`engine::resolveSpecialMultiplierFactor`が都度計算)。
//!         例: ファイヤーウォッチS2「遠距離特効」はbase=1.45(素質「暗殺者」E2最大潜在)、
//!         モジュールY(`uniequip_002_milu`)Lv1=1.45/Lv2=1.5/Lv3=1.55
//!   - `note`: 補足コメント(フロント表示用)
//!
//! ここに載せる値は機械データの自動判定と食い違う実測値であり、オーナーの参照
//! スプレッドシートを出典とする手動補正である。ゲームデータ更新で機械側の値が
//! 変わった場合はこのファイルも見直すこと。
//!
//! (P2 follow-up時点のメモ: 旧来の置き換え系フィールド`multiplier`/`self_atk_pct`/
//! `dmg_mult`/`hits`はFW/Weedyの実データ精査の結果どちらも不要と判明したため
//! `OverrideSpecial`から削除した。将来また置き換え系が要る特殊強化が出てきたら
//! 素直に生やせばよい。)

use indexmap::IndexMap;
use serde::Deserialize;
use std::sync::OnceLock;

use super::dto::DamageType;

/// 特殊強化の乗算系(P2 follow-up。`mul_multiplier`)。フィールドの意味は
/// ファイル冒頭コメント参照。
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct OverrideMulMultiplier {
    pub base: f64,
    pub module: Option<String>,
    pub by_module_level: Option<[f64; 3]>,
}

/// 「特殊強化」トグル1件分(P2)。`label`以外は全て省略可能。
/// フィールドの意味の詳細はファイル冒頭コメント参照。
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct OverrideSpecial {
    pub label: String,
    pub description: Option<String>,
    /// 加算系の発動条件(装備が必要なモジュールのuniEquipId)。
    pub requires_module: Option<String>,
    /// 加算系の値(モジュールLv1〜3ごとのセルフ%への加算値。要素数3必須)。
    pub add_self_atk_pct_by_module_level: Option<[f64; 3]>,
    /// 乗算系(P2 follow-up 2回目で追加)。
    pub mul_multiplier: Option<OverrideMulMultiplier>,
}

/// overrides.yaml 1バリアント分。`label`〜`note`は全フィールド省略可能。
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OverrideVariant {
    pub label: Option<String>,
    pub multiplier: Option<f64>,
    pub self_atk_pct: Option<f64>,
    pub hits: Option<u32>,
    pub damage_type: Option<DamageType>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    pub special: Option<OverrideSpecial>,
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
