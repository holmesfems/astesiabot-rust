//! フレームキル計算機のカタログJSON用DTO。JSONは camelCase に統一する
//! （`serde(rename_all = "camelCase")`で表現層向けに変換し、Rust側のフィールド名は
//! 他ソースと同じsnake_caseのまま保つ）。
//!
//! 計算層(このモジュール)はDTOを組み立てるだけで、表現層(web API)には依存しない
//! （CLAUDE.mdの「計算層と表現層を分ける」方針）。

use serde::Serialize;

/// 値の出所。機械データそのまま(`Auto`)か、`overrides.yaml`による手動補正(`Manual`)か。
/// フロント側で「この値は手動補正済み」の表示を出し分けるために持たせる。
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ValueSource {
    Auto,
    Manual,
}

/// 出所付きの値。
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Valued<T> {
    pub value: T,
    pub source: ValueSource,
}

impl<T> Valued<T> {
    pub fn auto(value: T) -> Self {
        Self { value, source: ValueSource::Auto }
    }

    pub fn manual(value: T) -> Self {
        Self { value, source: ValueSource::Manual }
    }

    /// `override_value`が`Some`ならManual、`None`なら`default`をAutoとして採用する
    /// （`build_catalog`の「未指定フィールドは機械値を継承、指定されたものだけManualになる」
    /// というoverride適用ルールをこの1箇所に集約する）。
    pub fn from_override(override_value: Option<T>, default: T) -> Self {
        match override_value {
            Some(v) => Self::manual(v),
            None => Self::auto(default),
        }
    }
}

/// ダメージ属性。`Deserialize`は下の`impl`で手動実装する(YAMLのbool"true"対策)。
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    Physical,
    Arts,
    True,
}

/// `overrides.yaml`の`damage_type: physical|arts|true`用に手動Deserializeを実装する。
/// YAMLは`true`/`false`を無引用で書くとブール型としてパースされてしまうため
/// （`damage_type: true`は文字列"true"ではなくbool trueになる）、文字列に加えて
/// bool trueも`DamageType::True`として受け付けることで、書き手が引用符を忘れても
/// 壊れないようにする（bool falseは無効なので明示的にエラーにする）。
impl<'de> serde::Deserialize<'de> for DamageType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = DamageType;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(r#""physical", "arts", "true"、またはbool true"#)
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<DamageType, E> {
                match v {
                    "physical" => Ok(DamageType::Physical),
                    "arts" => Ok(DamageType::Arts),
                    "true" => Ok(DamageType::True),
                    other => Err(E::custom(format!("unknown damage_type: {other}"))),
                }
            }

            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<DamageType, E> {
                if v {
                    Ok(DamageType::True)
                } else {
                    Err(E::custom("damage_type: false は無効です(trueのみ許容。文字列にする場合は\"true\"と引用符を付けること)"))
                }
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// 特殊強化の乗算系(P2 follow-up 2回目。`mul_multiplier`)。行の`multiplier`
/// (スキル倍率)に乗算する係数を、モジュール未装備/条件不一致時は`base`、
/// `module`をLv1〜3で装備時は`by_module_level`の対応要素から都度計算する
/// (`engine::resolveSpecialMultiplierFactor`)。詳細は`overrides.rs`冒頭コメント参照。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MulMultiplier {
    pub base: f64,
    pub module: Option<String>,
    pub by_module_level: Option<[f64; 3]>,
}

/// 「特殊強化」トグル(P2)。`overrides.yaml`の`special`をそのままDTO化したもの。
/// UIはこれが`Some`のときだけ`特殊強化: <label>`のチェックボックス(デフォルトON。ただし
/// `requires_module`付きの加算系はモジュール条件を満たさない間はチェックボックスの代わりに
/// ヒントを出す)を出す。ONの間、`requires_module`+`add_self_atk_pct_by_module_level`は
/// セルフ%へ加算、`mul_multiplier`は行の`multiplier`へ乗算する(どちらも都度計算。
/// 詳細は`overrides.rs`冒頭コメント参照)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Special {
    pub label: String,
    /// ⓘボタンで開く説明文(P2 follow-up 2回目で追加)。フロントが末尾に
    /// 「現在: +N%」/「現在: ×N」を動的に付け足して表示する。
    pub description: Option<String>,
    /// 加算系の発動条件(装備が必要なモジュールのuniEquipId)。`None`ならこの特殊強化に
    /// 加算系が無い(乗算系のみ、または将来別の仕組みのみ)。
    pub requires_module: Option<String>,
    /// 加算系の値(モジュールLv1〜3ごとのセルフ%への加算値)。
    pub add_self_atk_pct_by_module_level: Option<[f64; 3]>,
    /// 乗算系(P2 follow-up 2回目で追加)。`None`ならこの特殊強化に乗算系が無い。
    pub mul_multiplier: Option<MulMultiplier>,
}

/// フレームキル情報1件分(スキル1つのバリアント1つ)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FkEntry {
    /// fk_dataシート上の生値("1"〜"3"の他、"素質1"のような数値以外もありうる)。
    pub skill_num: String,
    /// スキル名(解決できなければ`skill_num`そのもの)。
    pub skill_label: String,
    /// overrideによる複数バリアント区別用のラベル(例: "300%")。単一バリアントなら`None`。
    pub variant_label: Option<String>,
    pub fk_num: String,
    pub fk_err: String,
    pub detail: String,
    pub last_edited: String,
    pub multiplier: Valued<f64>,
    /// blackboardのうちキーに"scale"を含む項目一覧("atk_scale"があれば先頭)。
    /// フロント側で「他の倍率候補」を選ばせるための参考情報。
    pub multiplier_candidates: Vec<(String, f64)>,
    pub self_atk_pct: Valued<f64>,
    pub hits: Valued<u32>,
    pub damage_type: Valued<DamageType>,
    /// このエントリのタグ(P2)。オペレーター機械タグ(近距離/職業/勢力) +
    /// スキル機械タグ(弾薬スキル) + overrideの手動`tags`(加算)。条件付きバフの
    /// 対象判定(`targets`/`bonus.tags`との積集合)に使う。
    pub tags: Vec<String>,
    /// 「特殊強化」トグル(P2)。無いスキルはUIにチェックボックスを出さない。
    pub special: Option<Special>,
    pub note: Option<String>,
}

/// モジュール1種分(カタログ表示用。素材コストは持たない)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModule {
    pub id: String,
    pub type_name: String,
    pub name: String,
    pub atk_by_level: Vec<f64>,
}

/// オペレーター1名分のカタログエントリ。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CatalogOperator {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub atk_base: f64,
    pub atk_potential: f64,
    pub modules: Vec<CatalogModule>,
    pub fk_entries: Vec<FkEntry>,
}

/// バフの種別。定額(Flat)か、ATKに対する割合(Pct)か。
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BufferKind {
    Pct,
    Flat,
}

/// バフの適用範囲。P2以降で使う(P1では`buffers`は常に空)。
/// タグ名は`Buffer.kind`(定額/割合)と紛れないよう`type`にする。
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BufferScope {
    /// 個体のみに掛かる(自己バフ等)。
    Individual,
    /// 対象タグを持つオペレーターにのみ掛かる(編成バフ等)。
    #[serde(rename_all = "camelCase")]
    Conditional { target_tags: Vec<String> },
}

/// 条件付きバフの「タグ限定ボーナス」(P2)。基本の対象タグに加え、`target_tags`のいずれかを
/// エントリが持つ場合、基本値(`Buffer.value`)の代わりにこちらの値を採用する(置き換え。
/// 加算ではない)。例: 異格エクシアの「弾薬スキル+13%、ラテラーノ勢は2倍(26%)」。
/// 汎用の仕組みとして持つ(特定バフIDにハードコードしない)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BufferBonus {
    pub target_tags: Vec<String>,
    pub value: f64,
    pub note: Option<String>,
}

/// バフ定義1件。P1では`Catalog::buffers`は常に空のVecだったが、P2で
/// `data/fk_kill_calc/buffers.yaml`(individual/conditional)から組み立てる。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Buffer {
    pub id: String,
    pub name: String,
    pub kind: BufferKind,
    pub value: f64,
    pub scope: BufferScope,
    /// 単体狙い(true)か範囲(false)か。同じ`single_target`バフを複数行で選ぶと
    /// フロントが⚠警告を出す(P2)。
    pub single_target: bool,
    /// `scope`が`Conditional`のバフだけが持ちうる、タグ限定の上書き値(P2)。
    pub bonus: Option<BufferBonus>,
    pub note: Option<String>,
}

/// カタログ全体。`/FrameKillCalculator/catalog.json`のレスポンス本体。
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub operators: Vec<CatalogOperator>,
    /// P1では常に空(P2で中身を持たせる)。
    pub buffers: Vec<Buffer>,
}
