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
//!   - `multiplier`: スキル倍率(固定値。スキルLvに関わらず同じ値を使う)
//!   - `multiplier_key`: スキルLv別に倍率を追従させたい場合、固定`multiplier`の代わりに
//!     こちらを使う(そのスキルのblackboardキー名。値そのものはスキルLv別に機械抽出する。
//!     P7で追加)。`multiplier`と同時指定は不可(`validate_override_level_fields`が検証)。
//!     例: Ash S3の"400%"バリアントは`multiplier_key: not_hitwall_scale`
//!     (L1=3.0〜L10=4.0)、Horn S2の"物理"バリアントは
//!     `multiplier_key: attack@s2.atk_scale`(L1=1.3〜L10=2.4)。
//!   - `self_atk_pct`: セルフATKバフの割合(固定値。スキルLvに関わらず同じ値を使う。
//!     例: Hornの0.31は素質由来でスキルLvに追従しないため固定のまま[将来バフへ
//!     移設予定])
//!   - `self_atk_pct_factor`: Autoのスキルレベル別セルフ%("atk"キー)にこの係数を掛けた値を
//!     採用する(P7で追加)。`self_atk_pct`と同時指定は不可。各レベルの値は小数第4位に
//!     丸める(浮動小数のドリフト対策)。例: ブレイズS3は`self_atk_pct_factor: 0.89`
//!     (Autoの"atk"[L1=.30〜L10=.80]に掛けてL10=0.712・L7=0.534)
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
//!       - **乗算系**(`mul_multiplier`。P2 follow-up 2回目で追加。P8 follow-upで固定値
//!         (`base`+`module`+`by_module_level`)から素質値テーブル参照へ置き換えた):
//!         `talent`(`RawOperatorCombat.talents`のインデックス。0始まり) + `key`
//!         (そのtalentのblackboardキー名。例: "atk_scale")を指定する。値の解決は
//!         `conditional_source::build_talent_source`(P4の条件付きバフ`source.talent`と
//!         全く同じビルダーを再利用。昇進×潜在 + モジュールLv×潜在の値テーブルを機械抽出する)
//!         に委ね、`Special.mul_multiplier`に`dto::ConditionalTalentSource`として持たせる。
//!         行の`multiplier`(スキル倍率)に**乗算**する係数は、固定値ではなく**行自身の
//!         `elite`/`potential`/実効モジュール(`effectiveModuleId`)**でこのテーブルを引いて
//!         都度決まる(`engine::resolveSpecialMultiplierFactor`)。値が0(=その昇進/潜在では
//!         素質が未解放。例: E0)なら乗算せず×1として扱い、UIはチェックボックスの代わりに
//!         「特殊強化「<label>」は素質が昇進<N>で解放」ヒントを出す(`specialUiState`/
//!         `specialMulCanApply`が判定。`requires_module`のヒントと同じ位置付け)。
//!         例: ファイヤーウォッチS2「遠距離特効」は`talent: 0, key: "atk_scale"`
//!         (素質「暗殺者」)。E1=1.2(潜在1〜4)/1.25(潜在5〜6)、E2=1.4/1.45、
//!         モジュールY(`uniequip_002_milu`)Lv1=E2基礎値のまま(素質強化が付かない)/
//!         Lv2=1.45,1.5/Lv3=1.5,1.55(潜在1〜4/5〜6)。E0は素質自体が未解放(値0→factor 1)。
//!   - `note`: 補足コメント(フロント表示用)
//!   - `combined`: 省略時false(P10)。同じskill_num内で`combined: true`のバリアントは
//!     個別に選ぶ選択肢ではなく「同時に発生する複数のダメージパーツ」として1つのFkEntryに
//!     まとめる(例: ホルンS2の物理+術)。先頭バリアントがパーツ0(FkEntryのトップレベル
//!     multiplier/hits/damage_type等はこのパーツの値)になり、self_atk_pct/special/tags/note
//!     は先頭バリアントのものだけを採用する(`build_catalog`が組み立てる。詳細はそちらの
//!     コメント参照)。同じskill_num内でcombinedが一部のバリアントだけtrueになっている、
//!     または2番目以降がself_atk_pct/special/tagsを持つのはデータの誤りとして
//!     `validate_combined_variants`(`cargo test`)が検知する。
//!
//! ここに載せる値は機械データの自動判定と食い違う実測値であり、オーナーの参照
//! スプレッドシートを出典とする手動補正である。ゲームデータ更新で機械側の値が
//! 変わった場合はこのファイルも見直すこと。
//!
//! (P2 follow-up時点のメモ: 旧来の置き換え系フィールド`multiplier`/`self_atk_pct`/
//! `dmg_mult`/`hits`はFW/Weedyの実データ精査の結果どちらも不要と判明したため
//! `OverrideSpecial`から削除した。将来また置き換え系が要る特殊強化が出てきたら
//! 素直に生やせばよい。)
//!
//! (P8 follow-upのメモ: `mul_multiplier`は当初`base`+`module`+`by_module_level`の
//! 固定値だったが、FWの「遠距離特効」が実は素質「暗殺者」の値そのもの(行の昇進/潜在で
//! 変わる)だったため、固定値では行の潜在設定を無視してしまう不具合があった。
//! `{ talent, key }`(素質値テーブル参照)へ置き換えて解消した。)

use indexmap::IndexMap;
use serde::Deserialize;
use std::sync::OnceLock;

use super::dto::DamageType;

/// 特殊強化の乗算系(P2 follow-up。`mul_multiplier`)。P8 follow-upで固定値
/// (`base`/`module`/`by_module_level`)から素質値テーブル参照へ置き換えた。
/// フィールドの意味はファイル冒頭コメント参照。
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct OverrideMulMultiplier {
    /// `RawOperatorCombat.talents`のインデックス(0始まり)。
    pub talent: usize,
    /// そのtalentのblackboardキー名(例: "atk_scale")。
    pub key: String,
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
    /// 乗算系(P2 follow-up 2回目で追加。P8 follow-upで素質値テーブル参照へ置き換え)。
    pub mul_multiplier: Option<OverrideMulMultiplier>,
}

/// overrides.yaml 1バリアント分。`label`〜`note`は全フィールド省略可能。
///
/// P7(スキルLv対応)で追加した`multiplier_key`/`self_atk_pct_factor`は、固定値の
/// `multiplier`/`self_atk_pct`とそれぞれ排他(`build_catalog`側でも両立しない設計だが、
/// 実データの誤りとして`cargo test`のドリフト検知[`validate_override_level_fields`]が
/// 検出する)。詳細はファイル冒頭コメント参照。
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OverrideVariant {
    pub label: Option<String>,
    pub multiplier: Option<f64>,
    /// スキルLv別に倍率を追従させたい場合、固定`multiplier`の代わりにこのキー名
    /// (そのスキルのblackboardキー)を指定する(P7)。`multiplier`と同時指定は不可。
    pub multiplier_key: Option<String>,
    pub self_atk_pct: Option<f64>,
    /// Autoのスキルレベル別セルフ%("atk"キー)にこの係数を掛けた値を採用する(P7)。
    /// 各レベルの値は小数第4位に丸める(浮動小数のドリフト対策)。`self_atk_pct`と
    /// 同時指定は不可。
    pub self_atk_pct_factor: Option<f64>,
    pub hits: Option<u32>,
    pub damage_type: Option<DamageType>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    pub special: Option<OverrideSpecial>,
    /// このエントリでオペレーターを追加した時の初期モジュール(uniEquipId)。省略時は
    /// フロントの既定(Lv3のATK加算が最大のモジュール)。ATKだけでなく素質強化の有無で
    /// FK向きのモジュールが変わるオペレーター用(例: ホルンはYの方がATKは高いが、
    /// 重装バフ「軍事要塞」を強化するXの方が実効ATKが高い)。
    pub default_module: Option<String>,
    pub note: Option<String>,
    /// 同じskill_num内でtrueのバリアントを「同時発生する複数パーツ」として1つのFkEntryに
    /// まとめる(P10)。省略時false。詳細はファイル冒頭コメント参照。
    #[serde(default)]
    pub combined: bool,
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
