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

/// 「特殊強化」トグル(P2)。`overrides.yaml`の`special`をそのままDTO化したもの。
/// UIはこれが`Some`のときだけ`特殊強化: <label>`のチェックボックス(デフォルトON。ただし
/// `requires_module`付きの加算系はモジュール条件を満たさない間、`mul_multiplier`付きの
/// 乗算系は素質が未解放の間、それぞれチェックボックスの代わりにヒントを出す)を出す。
/// ONの間、`requires_module`+`add_self_atk_pct_by_module_level`はセルフ%へ加算、
/// `mul_multiplier`は行の`multiplier`へ乗算する(どちらも都度計算。
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
    /// 乗算系(P2 follow-up 2回目で追加。P8 follow-upで固定値
    /// [`base`/`module`/`by_module_level`]から素質値テーブル参照へ置き換えた)。`None`なら
    /// この特殊強化に乗算系が無い。テーブルは`conditional_source::build_talent_source`
    /// (P4の条件付きバフ`source.talent`と同じビルダー)で組み立てたもので、行の`multiplier`
    /// に乗算する係数は行自身の昇進/潜在/実効モジュールでこのテーブルを引いて都度決まる
    /// (`engine::resolveSpecialMultiplierFactor`)。値が0(素質未解放)なら乗算せず×1として
    /// 扱う。詳細は`overrides.rs`冒頭コメント参照。
    pub mul_multiplier: Option<ConditionalTalentSource>,
}

/// 倍率候補1件分(P7。`multiplier_candidates`の要素)。`values_by_level[i]`はスキルLv(i+1)
/// でのこのキーの値(スキルLv1〜7+特化1〜3で最大10要素。データに存在するレベル数だけ入る)。
/// フロント側は現在選択中のスキルLv(`row.skillLevel`)でこの配列を引く
/// (`engine.js::valueAtLevel`)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultiplierCandidate {
    pub key: String,
    pub values_by_level: Vec<f64>,
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
    /// スキルLv別配列(`multiplier_by_level`)の最終レベル(データが10未満のスキルは
    /// その末尾)の値。後方互換のため維持する(P6以前の呼び出し側はこのフィールドだけを見る)。
    pub multiplier: Valued<f64>,
    /// スキルLv1〜(データ数)ごとの倍率(P7)。Auto: blackboardの選定キーそのまま。
    /// Manual(`multiplier_key`): そのキーのスキルLv別値。Manual(固定`multiplier`):
    /// 全レベル同じ値(`multiplier_fixed=true`)。
    pub multiplier_by_level: Vec<f64>,
    /// `true`ならこの倍率はスキルLvに関わらず固定(Manualの固定`multiplier`指定。P7)。
    /// UIの「補正」バッジの文言出し分けに使う(固定なら「補正(特化3固定)」、
    /// キー追従なら素の「補正」)。
    pub multiplier_fixed: bool,
    /// blackboardのうちキーに"scale"を含む項目一覧("atk_scale"があれば先頭)。
    /// フロント側で「他の倍率候補」を選ばせるための参考情報(P7でスキルLv別に対応)。
    pub multiplier_candidates: Vec<MultiplierCandidate>,
    pub self_atk_pct: Valued<f64>,
    /// スキルLv1〜(データ数)ごとのセルフATK%(P7)。Auto: blackboardの"atk"そのまま。
    /// Manual(`self_atk_pct_factor`): Autoの値に係数を掛けたもの(4桁に丸め済み)。
    /// Manual(固定`self_atk_pct`): 全レベル同じ値(`self_atk_pct_fixed=true`)。
    pub self_atk_pct_by_level: Vec<f64>,
    /// `true`ならこのセルフ%はスキルLvに関わらず固定(Manualの固定`self_atk_pct`指定。P7)。
    pub self_atk_pct_fixed: bool,
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
    /// このモジュールを装備できる最低昇進(0/1/2)。P?で追加。
    pub unlock_phase: u8,
    /// `unlock_phase`到達時点で装備可能になる最低レベル。
    pub unlock_level: u32,
}

/// 昇進段階1つ分のLv1〜Lv最大ATK(P?。`operator_combat::RawPhaseAtk`をそのままDTO化したもの)。
/// `engine.js::computeBaseAtk`が線形補間(四捨五入)して昇進/レベル別ATKを計算する元データ。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PhaseAtk {
    pub max_level: u32,
    pub atk_min: f64,
    pub atk_max: f64,
}

/// オペレーター1名分のカタログエントリ。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CatalogOperator {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    /// 昇進2最大レベル+信頼度100時点のATK(後方互換。新しい昇進/レベル/信頼度指定の
    /// 計算は`phases`/`atkTrustMax`をフロントが使う)。
    pub atk_base: f64,
    pub atk_potential: f64,
    /// 潜在ランク別(0始まり。0=潜在1〜5=潜在6)の累積ATK加算値(P8。「潜在セレクト」用。
    /// `operator_combat::RawOperatorCombat::atk_potential_by_rank`をそのままDTO化したもの)。
    pub atk_potential_by_rank: [f64; 6],
    pub modules: Vec<CatalogModule>,
    pub fk_entries: Vec<FkEntry>,
    /// 昇進段階ごとのLv1/Lv最大ATK(P?)。
    pub phases: Vec<PhaseAtk>,
    /// 信頼度100%時点のATK加算値。
    pub atk_trust_max: f64,
    /// skill_num→解放昇進(0/1/2)。`(skillNum, phase)`のペア一覧
    /// (`multiplier_candidates`と同じくVecで持ち、順序はゲームデータのスキル配列順)。
    pub skill_unlock_phase: Vec<(String, u8)>,
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

/// 条件付きバフの「タグ限定ボーナス」(P2。P4で`mult`を追加)。基本の対象タグに加え、
/// `target_tags`のいずれかをエントリが持つ場合、基本値(`Buffer.value`。`source`付きバフなら
/// 選択中の昇進/潜在/モジュール/スキルLvで解決した値)の代わりにこちらを採用する。
/// `value`(固定値。置き換え)と`mult`(基本値への倍率。P4で追加)はどちらか一方のみ持つ:
///   - `value`: 基本値を無視してこの固定値を使う(旧来。例: 異格エクシアの
///     旧仕様「ラテラーノ勢は固定26%」)
///   - `mult`: 選択中の設定で解決した基本値にこの倍率を掛ける(`source`付きバフ専用。
///     基本値自体が昇進/潜在で変わるため、固定値では表現できない場合に使う。
///     例: 異格エクシアの素質「铳弹协约」実データの`mult`キー(2.0)そのもの)
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BufferBonus {
    pub target_tags: Vec<String>,
    pub value: Option<f64>,
    pub mult: Option<f64>,
    pub note: Option<String>,
}

/// バフのON/OFFに紐づく単純な効果倍率トグル(P4)。ONの間、解決した値に`mult`を掛ける
/// (例: 前衛アーミヤの「スキル中は効果2倍」)。`Buffer.source`の軸(昇進/潜在/モジュール等)
/// とは独立(常にBuffer全体に対して掛かる)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BuffToggle {
    pub label: String,
    pub mult: f64,
}

/// 「条件付き/個別バフ」の値をゲームデータから機械抽出する動的ソース(P4で条件付き向けに
/// 追加、P5で個別バフにも対応させ以下3種を追加)。`talent`(素質)/`skill`(スキルLv別
/// blackboardがそのまま値になる)/`scale`(P5。素質または`base_pct`に掛け合わせる2軸目の
/// スキルLv別倍率)/`base_pct`(P5。ゲームデータに存在しない固定基礎値)/`stage`(P5。
/// スキルLvではなく離散的な「段階」で値が変わるソース)の組み合わせで最終値を表現する。
/// 実際に使われる組み合わせは:
///   - `talent`のみ: 素質そのものが値(例: castle3、エクシア)
///   - `skill`のみ: スキルLv別blackboardがそのまま値(例: 血漿、ドリアン、ズィマー)
///   - `talent` × `scale`: 素質値にスキルLv別スケールを掛ける(例: スワイヤーS1/S2)
///   - `base_pct` × `scale`: 固定基礎値にスキルLv別スケールを掛ける(例: ステインレスS1)
///   - `stage`のみ: 離散的な段階値(例: ナスティS3)
/// `max_targets_by_module`(P5)は値の解決には関与せず、`single_target`警告の上限を
/// フロントが緩和するためだけに使う(例: エクシア。モジュールX Lv2以上で対象2名)。
/// 詳細な設計意図・実データ検証結果は`buffers.rs`冒頭コメント +
/// `conditional_source.rs`冒頭コメント参照。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalSource {
    pub operator_id: String,
    pub operator_name: String,
    pub talent: Option<ConditionalTalentSource>,
    pub skill: Option<ConditionalSkillSource>,
    /// P5: 2軸目の乗算スケール(スキルLv別blackboard由来)。`talent`または`base_pct`と
    /// 掛け合わせて最終値を作る。`skill`と同時に使うことは無い(排他)。
    pub scale: Option<ConditionalSkillSource>,
    /// P5: ゲームデータから機械抽出できない固定の基礎値(トークン等由来。YAML直書き定数)。
    /// `talent`が無い時だけ意味を持ち、`scale`と組み合わせて使う。
    pub base_pct: Option<f64>,
    /// P5: スキルLvではなく離散的な「段階」で値が変わるソース。`talent`/`skill`/`scale`/
    /// `base_pct`とは排他。
    pub stage: Option<ConditionalStageSource>,
    /// P5: 特定モジュールLv以上を装備している間、このバフの対象人数が増える。
    pub max_targets_by_module: Option<MaxTargetsByModule>,
    pub defaults: ConditionalSourceDefaults,
}

/// 素質(talent)由来の値テーブル(P4)。`values_by_elite_and_potential[phase][potentialRank]`
/// (phase=0/1/2=E0/E1/E2、potentialRank=0〜5=潜在1〜6)がベース(モジュール無し)の値。
/// `elite_varies`/`potential_varies`はUIがそれぞれの軸(セレクト)を出すべきかどうかの
/// 自動判定済みフラグ(値が変わらない軸は見せない)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalTalentSource {
    pub values_by_elite_and_potential: [[f64; 6]; 3],
    pub elite_varies: bool,
    pub potential_varies: bool,
    /// 値を実際に変えるモジュールだけを載せる(dedupe済み。例: ポデンコ/ペペはモジュールを
    /// 装備しても値が変わらないため空Vecになり、UIはモジュール選択を出さない)。
    pub modules: Vec<ConditionalSourceModule>,
}

/// 素質を上書きするモジュール1種分の値テーブル(P4)。
/// `values_by_level_and_potential[moduleLv][potentialRank]`(moduleLv=0/1/2=Lv1/2/3)。
/// そのLvにこのtalentIndexへの上書き候補が無ければベース(E2側)の値にフォールバック済み
/// (`build_conditional_sourced_buffers`が計算時に埋める)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalSourceModule {
    pub module_id: String,
    pub type_name: String,
    pub name: String,
    pub values_by_level_and_potential: [[f64; 6]; 3],
}

/// スキルLv別blackboard由来の値テーブル(P4)。`values_by_level[i]`はスキルLv(i+1)の値
/// (Lv1〜7 + 特化1〜3で最大10要素。データに存在するレベル数だけ入る)。
/// `ConditionalSource.skill`(値そのもの)と`ConditionalSource.scale`(P5。他の値に掛ける
/// 倍率)の両方でこの型を使い回す。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalSkillSource {
    pub skill_num: String,
    pub skill_label: String,
    pub values_by_level: Vec<f64>,
    /// P5: レベルによって値が実際に変わるか(dedupe用)。falseならUIはこの軸の
    /// セレクトを出さない(例: スワイヤーS1のtalent_scaleは全レベル2.0で固定)。
    pub varies: bool,
}

/// 離散的な「段階」で値が変わるソース(P5。例: ナスティS3の装置アップグレード段階)。
/// スキルLvの概念は無く、同一スキルのblackboard上にある複数キー(段階ごとに別名で
/// 存在する)から直接値を引く。`values[i]`が`labels[i]`(1段階目、2段階目…)の値。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalStageSource {
    pub skill_id: String,
    pub skill_label: String,
    pub values: Vec<f64>,
    pub labels: Vec<String>,
}

/// 特定モジュールLv以上を装備している間、このバフの対象人数が増える(P5。例: エクシアの
/// 素質。モジュールX Lv2以上で2名)。値の解決には関与しない(`single_target`警告の
/// 上限をフロントが緩和するためだけに使う)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MaxTargetsByModule {
    pub module_id: String,
    pub min_level: u8,
    pub count: u8,
}

/// 「最大成長」を表すデフォルトの選択状態(P4)。UIが条件付きバフを初めてONにした時に使う
/// (`昇進2・潜在6・値が変わるモジュールが有ればLv3・スキルソースなら最大Lv`)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalSourceDefaults {
    /// 0/1/2 = E0/E1/E2。
    pub elite: u8,
    /// 0〜5 = 潜在1〜6。
    pub potential: u8,
    pub module_id: Option<String>,
    /// 1〜3。`module_id`が`None`なら意味を持たない。
    pub module_level: u8,
    /// 1〜(データ数)。`skill`/`scale`ソースでなければ意味を持たない。
    pub skill_level: u8,
    /// P5: 1〜(段階数)。`stage`ソースでなければ意味を持たない(既定は最終段階)。
    pub stage_index: u8,
}

/// バフ定義1件。P1では`Catalog::buffers`は常に空のVecだったが、P2で
/// `data/fk_kill_calc/buffers.yaml`(individual/conditional)から組み立てる。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Buffer {
    pub id: String,
    pub name: String,
    pub kind: BufferKind,
    /// `source`が`None`の固定バフはこの値をそのまま使う。`source`付きバフでは
    /// `defaults`の選択状態で解決した値(=UIが初めてONにした時に見せる値)を入れる
    /// (フロントが選択を変えた後の値は`resolveConditionalValue`が都度計算し直す)。
    pub value: f64,
    pub scope: BufferScope,
    /// 単体狙い(true)か範囲(false)か。同じ`single_target`バフを複数行で選ぶと
    /// フロントが⚠警告を出す(P2)。
    pub single_target: bool,
    /// `scope`が`Conditional`のバフだけが持ちうる、タグ限定の上書き値(P2)。
    pub bonus: Option<BufferBonus>,
    /// 同じグループ名を持つ条件付きバフは同時に効かない(ONでも最大値の1件だけ採用し、
    /// UIでは片方をONにするともう片方をOFFにする)。例: 前衛アーミヤ(通常)/(スキル中)。
    pub exclusive_group: Option<String>,
    /// ゲームデータから機械抽出する動的ソース(P4。省略時は`value`固定のまま)。
    pub source: Option<ConditionalSource>,
    /// ON/OFFで効果倍率が変わるトグル(P4。例: 前衛アーミヤの「スキル中は効果2倍」)。
    pub toggle: Option<BuffToggle>,
    pub note: Option<String>,
}

/// 鼓舞ソースの「モジュールによる素質強化」(P3)。指定モジュールを指定Lvで装備している間、
/// `pct`の代わりに`pct_by_level`の対応要素を採用する(置き換え。加算ではない)。
/// `potential_bonus_by_level`は`talentPotential`(素質凸)がONの時だけ追加加算する
/// (置き換え後の値に対する加算。詳細は`buffers.rs`冒頭コメント参照)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InspireModuleOverride {
    pub module: String,
    pub pct_by_level: [f64; 3],
    pub potential_bonus_by_level: [f64; 3],
}

/// 鼓舞ソースの「自己%条件パーツ」1件分(P3)。素質・モジュール由来の自己ATK%条件を
/// 汎用的に表現する(特定オペレーターにハードコードしない。詳細はbuffers.rs冒頭コメント参照)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InspireSelfPart {
    pub id: String,
    /// チェックボックス/ヒントに出す説明的なラベル。
    pub label: String,
    /// 計算式(内訳)表示用の短いラベル(例: "素質"、"X")。省略時は`label`を使う。
    pub short_label: String,
    /// 内訳ⓘに表示する補足説明(省略可)。
    pub description: Option<String>,
    /// 基礎値(モジュール条件が無い/満たさない時の値)。
    pub pct: f64,
    /// `talentPotential`(素質凸)ONの時に`pct`へ加算する値。
    pub pct_potential_bonus: f64,
    /// モジュールによる置き換え強化(省略可。`talent`/`talent_abyssal`のような
    /// 「常にある効果をモジュールで底上げする」パーツ用)。
    pub module_override: Option<InspireModuleOverride>,
    /// このパーツ自体の発動にモジュール装備を必須とする場合のuniEquipId(省略可。
    /// `module_x_two_ops`のような「モジュール無しでは存在しない効果」パーツ用。
    /// `module_override`と両方指定することは無い)。
    pub requires_module: Option<String>,
    /// `requires_module`使用時のLv1〜3ごとの値(要素数3必須)。
    pub pct_by_module_level: Option<[f64; 3]>,
    /// 他のパーツidを「置き換える」(加算ではない)。このパーツがON+適用可能な間、
    /// `replaces`が指すパーツの寄与は無効化される。
    pub replaces: Option<String>,
    /// 常時有効(チェックボックスを出さない)。`replaces`で無効化され得る。
    pub always_on: bool,
    /// トグル可能なパーツの初期状態(ユーザーが未設定の時に使う既定値)。
    pub default_on: bool,
}

/// 鼓舞ソースの「このスキルなら鼓舞倍率はいくつか」(P3。P7でスキルLv別対応)。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InspireSkillRatio {
    pub skill_num: String,
    /// スキルLv別配列(`ratio_by_level`)の最終レベルの値。後方互換のため維持する。
    pub ratio: f64,
    /// スキルLv1〜(データ数)ごとの鼓舞倍率(P7)。`ratio_key`指定時はblackboardの
    /// そのキーのスキルLv別値、固定`ratio`指定時は全レベル同じ値
    /// (`ratio_fixed=true`)。
    pub ratio_by_level: Vec<f64>,
    /// `true`ならこの倍率はスキルLvに関わらず固定(固定`ratio`指定。P7)。
    pub ratio_fixed: bool,
}

/// 鼓舞(インスパイア)ソース1件分(P3)。fk_dataシート起点ではなく`buffers.yaml`の`inspire`
/// リスト起点で組み立てる(鼓舞役はFKする側ではないためfk_dataに載らない)。
/// `atkBase`/`atkPotential`/`modules`/`tags`は`operator_combat`由来（`CatalogOperator`と
/// 同じ構築方針。エンジン側は`resolveAtk`をそのまま再利用できるようフィールド名を揃えている）。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InspireSource {
    pub id: String,
    pub operator_id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub atk_base: f64,
    pub atk_potential: f64,
    /// 潜在ランク別(0始まり。0=潜在1〜5=潜在6)の累積ATK加算値(P8。`CatalogOperator`と同じ)。
    pub atk_potential_by_rank: [f64; 6],
    pub modules: Vec<CatalogModule>,
    pub skills: Vec<InspireSkillRatio>,
    /// 素質凸(`self_parts`の`pct_potential_bonus`)が解放される0始まりpotential_rank(P8。
    /// 潜在セレクトが自己%パーツの素質凸ボーナスを適用し始める境目)。`None`ならこの鼓舞
    /// ソースは素質凸ボーナスの概念を持たない(現時点では全ソースがSome。将来的に
    /// pct_potential_bonusを持たないソースが増えたらNoneにする想定)。
    pub talent_potential_rank: Option<u8>,
    pub self_parts: Vec<InspireSelfPart>,
    /// 昇進段階ごとのLv1/Lv最大ATK(P?。`CatalogOperator.phases`と同じ)。
    pub phases: Vec<PhaseAtk>,
    /// 信頼度100%時点のATK加算値。
    pub atk_trust_max: f64,
    /// skill_num→解放昇進(0/1/2)。
    pub skill_unlock_phase: Vec<(String, u8)>,
}

/// カタログ全体。`/FrameKillCalculator/catalog.json`のレスポンス本体。
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub operators: Vec<CatalogOperator>,
    /// P1では常に空(P2で中身を持たせる)。
    pub buffers: Vec<Buffer>,
    /// 鼓舞ソース一覧(P3)。P1/P2では常に空。
    pub inspire_sources: Vec<InspireSource>,
}
