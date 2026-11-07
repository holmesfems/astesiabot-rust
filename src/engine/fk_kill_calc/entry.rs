//! FK エントリの組み立て（機械データと手動補正のマージ）。

use crate::engine::external_source::fk_data::FkSheetRow;
use crate::engine::external_source::operator_combat::RawOperatorCombat;
use super::dto::{
    DamageType, FkEntry, FkPart,
    MultiplierCandidate, Special, StoneBoard, Valued,
};
use indexmap::IndexMap;
use super::conditional_source;
use super::{OverrideSpecial, OverrideStoneBoard, OverrideVariant};

/// `overrides.yaml`の`special`(手動データ)をDTO(`dto::Special`)へ変換する。
/// `mul_multiplier`(P8 follow-upで素質値テーブル参照になった)が実データ(`op.talents`)に
/// 解決できない(talent_index範囲外)場合は`None`を返し、呼び出し側が`skipped`に記録する。
pub(super) fn to_special_dto(special: &OverrideSpecial, op: &RawOperatorCombat) -> Option<Special> {
    let mul_multiplier = match &special.mul_multiplier {
        Some(mm) => Some(conditional_source::build_talent_source(op, mm.talent, &mm.key)?),
        None => None,
    };
    Some(Special {
        label: special.label.clone(),
        description: special.description.clone(),
        requires_module: special.requires_module.clone(),
        add_self_atk_pct_by_module_level: special.add_self_atk_pct_by_module_level,
        mul_multiplier,
    })
}

/// `overrides.yaml`の`stone_board`をDTOへ変換する。`talent`が範囲外なら`None`
/// (呼び出し側が`skipped`に記録する。`to_special_dto`と同じ方針)。
pub(super) fn to_stone_board_dto(board: &OverrideStoneBoard, op: &RawOperatorCombat) -> Option<StoneBoard> {
    let table = |key: &str| conditional_source::build_talent_source(op, board.talent, key);
    Some(StoneBoard {
        damage_per_stack: table(&board.damage_per_stack_key)?,
        res_pen_per_stack: table(&board.res_pen_per_stack_key)?,
        max_stacks: table(&board.max_stacks_key)?,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_entry(
    row: &FkSheetRow,
    skill_label: String,
    variant_label: Option<String>,
    multiplier: Valued<f64>,
    multiplier_by_level: Vec<f64>,
    multiplier_fixed: bool,
    multiplier_candidates: Vec<MultiplierCandidate>,
    self_atk_pct: Valued<f64>,
    self_atk_pct_by_level: Vec<f64>,
    self_atk_pct_fixed: bool,
    hits: Valued<u32>,
    damage_type: Valued<DamageType>,
    tags: Vec<String>,
    special: Option<Special>,
    note: Option<String>,
) -> FkEntry {
    FkEntry {
        skill_num: row.skill_num.clone(),
        skill_label,
        variant_label,
        fk_num: row.fk_num.clone(),
        fk_err: row.fk_err.clone(),
        detail: row.detail.clone(),
        last_edited: row.last_edited.clone(),
        multiplier,
        multiplier_by_level,
        multiplier_fixed,
        multiplier_candidates,
        self_atk_pct,
        self_atk_pct_by_level,
        self_atk_pct_fixed,
        hits,
        damage_type,
        tags,
        special,
        default_module: None,
        stone_board: None,
        note,
        parts: Vec::new(),
    }
}

/// P10: 同じskill_num内の`combined: true`バリアント群を1つのFkEntryにまとめる。
/// トップレベルのmultiplier/multiplier_by_level/hits/damage_type等 = 先頭バリアント
/// (=パーツ0)。self_atk_pct/special/tags/default_module/noteも先頭バリアントのものを
/// 使う(2番目以降がこれらを持っていないことは`validate_combined_variants`が検証する)。
/// `variant_label`はパーツラベルを"+"で連結する(例: "物理+術")。
#[allow(clippy::too_many_arguments)]
pub(super) fn build_combined_entry(
    row: &FkSheetRow,
    skill_label: &str,
    variants: &[OverrideVariant],
    default_multiplier_by_level: &[f64],
    default_multiplier: f64,
    blackboard_by_level: Option<&[IndexMap<String, f64>]>,
    num_levels: usize,
    multiplier_candidates: &[MultiplierCandidate],
    default_self_atk_pct_by_level: &[f64],
    default_self_atk_pct: f64,
    default_hits: u32,
    default_damage_type: DamageType,
    entry_tags: &[String],
    combat_op: &RawOperatorCombat,
    skipped: &mut Vec<String>,
    op_id: &str,
) -> FkEntry {
    let parts: Vec<FkPart> = variants
        .iter()
        .map(|variant| {
            let (multiplier, multiplier_by_level, multiplier_fixed) =
                resolve_variant_multiplier(variant, default_multiplier_by_level, default_multiplier, blackboard_by_level, num_levels);
            FkPart {
                label: variant.label.clone().unwrap_or_default(),
                multiplier,
                multiplier_by_level,
                multiplier_fixed,
                damage_type: Valued::from_override(variant.damage_type, default_damage_type),
            }
        })
        .collect();

    let head = &variants[0];
    let mut tags_for_head = entry_tags.to_vec();
    if let Some(extra) = &head.tags {
        tags_for_head.extend(extra.iter().cloned());
    }
    let (self_atk_pct, self_atk_pct_by_level, self_atk_pct_fixed) =
        resolve_variant_self_atk_pct(head, default_self_atk_pct_by_level, default_self_atk_pct, num_levels);
    let special = head.special.as_ref().and_then(|s| match to_special_dto(s, combat_op) {
        Some(dto) => Some(dto),
        None => {
            skipped.push(format!("special:{op_id}/{}", row.skill_num));
            None
        }
    });
    let variant_label = Some(variants.iter().filter_map(|v| v.label.clone()).collect::<Vec<_>>().join("+"));
    let head_part = parts[0].clone();

    let mut entry = build_entry(
        row,
        skill_label.to_string(),
        variant_label,
        head_part.multiplier,
        head_part.multiplier_by_level,
        head_part.multiplier_fixed,
        multiplier_candidates.to_vec(),
        self_atk_pct,
        self_atk_pct_by_level,
        self_atk_pct_fixed,
        // Hit数は全パーツ共通(1回の攻撃で全パーツが同時に出る)なので先頭バリアントの値だけを使う。
        Valued::from_override(head.hits, default_hits),
        head_part.damage_type,
        tags_for_head,
        special,
        head.note.clone(),
    );
    entry.default_module = head.default_module.clone();
    entry.parts = parts;
    entry
}

/// blackboardから倍率のデフォルト値と候補一覧を作る。
///
/// デフォルト値の選定順序:
///   1. 厳密一致の`atk_scale`キーがあればその値
///   2. 無ければ、キーが`atk_scale`または`damage_scale`で終わる項目のうち、
///      キー名アルファベット順で最初のもの(例: `damage_by_atk_scale`、
///      `attack@s2.atk_scale`。後者は`attack@s2.magic_atk_scale`より
///      アルファベット順で先に来るため、物理側が選ばれる)
///   3. どちらも無ければ1.0
///
/// (1敗のBlaze/Hornで実データ調査済み。詳細は`overrides.yaml`のコメント参照)。
///
/// 候補一覧はキーに"scale"を含む項目全部。**順序はSeed経由/実fetch経由で
/// blackboardの元の並びが変わっても揺れないよう、常に「選ばれたデフォルトのキーを
/// 先頭、残りはキー名のアルファベット順」に正規化する**
/// (`write_seed_file`がJSONキーを再帰的にソートして書き出す影響で、Seed読み込み時と
/// 実fetch時とでIndexMapの挿入順が食い違うため。この関数の出力を常に決定的にすることで
/// フロント側の表示順がSeed/本番のどちらでも変わらないようにする)。
/// P7: `blackboard_by_level`の各レベルから`key`の値を取り出した配列を作る(無ければ0.0で
/// 埋める)。`blackboard_by_level`自体が無ければ`num_levels`個(0なら1個)の0.0配列を返す
/// (「スキルLv別データが無い＝単一値のフォールバック」を1箇所に集約する)。
pub(super) fn values_by_level_for_key(blackboard_by_level: Option<&[IndexMap<String, f64>]>, key: &str, num_levels: usize) -> Vec<f64> {
    match blackboard_by_level {
        Some(levels) => levels.iter().map(|lv| lv.get(key).copied().unwrap_or(0.0)).collect(),
        None => vec![0.0; num_levels.max(1)],
    }
}

/// 小数第4位に丸める(P7。`self_atk_pct_factor`計算時の浮動小数ドリフト対策。
/// 例: `0.8 × 0.89`が`0.7119999999999999`のような値になっても`0.712`に正規化する)。
pub(super) fn round4(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}

/// バリアントの倍率(Valued値・スキルLv別配列・固定フラグ)を解決する(P7)。
/// `multiplier`(固定)/`multiplier_key`(そのスキルのblackboardキー名でスキルLvに追従)は
/// どちらか一方のみが指定されている前提(両方/どちらも無い場合の整合性は
/// `validate_override_level_fields`が検証し、ここではAutoへ安全にフォールバックする)。
pub(super) fn resolve_variant_multiplier(
    variant: &OverrideVariant,
    default_by_level: &[f64],
    default_value: f64,
    blackboard_by_level: Option<&[IndexMap<String, f64>]>,
    num_levels: usize,
) -> (Valued<f64>, Vec<f64>, bool) {
    if let Some(fixed) = variant.multiplier {
        return (Valued::manual(fixed), vec![fixed; num_levels.max(1)], true);
    }
    if let Some(key) = &variant.multiplier_key {
        let by_level = values_by_level_for_key(blackboard_by_level, key, num_levels);
        let value = by_level.last().copied().unwrap_or(default_value);
        return (Valued::manual(value), by_level, false);
    }
    (Valued::auto(default_value), default_by_level.to_vec(), false)
}

/// バリアントのセルフATK%(Valued値・スキルLv別配列・固定フラグ)を解決する(P7)。
/// `self_atk_pct`(固定)/`self_atk_pct_factor`(Autoのスキルレベル別セルフ%に係数を掛けて
/// 追従させる)はどちらか一方のみが指定されている前提。
pub(super) fn resolve_variant_self_atk_pct(
    variant: &OverrideVariant,
    default_by_level: &[f64],
    default_value: f64,
    num_levels: usize,
) -> (Valued<f64>, Vec<f64>, bool) {
    if let Some(fixed) = variant.self_atk_pct {
        return (Valued::manual(fixed), vec![fixed; num_levels.max(1)], true);
    }
    if let Some(factor) = variant.self_atk_pct_factor {
        let by_level: Vec<f64> = default_by_level.iter().map(|v| round4(v * factor)).collect();
        let value = by_level.last().copied().unwrap_or(round4(default_value * factor));
        return (Valued::manual(value), by_level, false);
    }
    (Valued::auto(default_value), default_by_level.to_vec(), false)
}

pub(super) fn resolve_multiplier_defaults(
    blackboard: Option<&IndexMap<String, f64>>,
    blackboard_by_level: Option<&[IndexMap<String, f64>]>,
) -> (f64, Vec<f64>, Vec<MultiplierCandidate>) {
    let num_levels = blackboard_by_level.map(|levels| levels.len()).unwrap_or(1).max(1);
    let Some(blackboard) = blackboard else {
        return (1.0, vec![1.0; num_levels], Vec::new());
    };
    let mut candidates: Vec<(String, f64)> =
        blackboard.iter().filter(|(k, _)| k.contains("scale")).map(|(k, v)| (k.clone(), *v)).collect();
    candidates.sort_by(|(a, _), (b, _)| a.cmp(b));

    // P7: `candidates`をこの後mutate(先頭への並び替え)するため、chosen_keyは所有権付き
    // (`String`)にしてcandidatesへの借用を早めに切る(借用したままmutateしようとすると
    // 借用チェッカに弾かれる)。
    let chosen_key: Option<String> = if blackboard.contains_key("atk_scale") {
        Some("atk_scale".to_string())
    } else {
        candidates
            .iter()
            .map(|(k, _)| k.as_str())
            .find(|k| k.ends_with("atk_scale") || k.ends_with("damage_scale"))
            .map(str::to_string)
    };

    let default_multiplier = match &chosen_key {
        Some(key) => blackboard.get(key.as_str()).copied().unwrap_or(1.0),
        None => 1.0,
    };

    // 選ばれたキーを候補一覧の先頭へ動かす(既にアルファベット順ソート済みなので、
    // それ以外の並びは変えない = 「選ばれたもの優先、残りはアルファベット順」)。
    if let Some(key) = &chosen_key {
        if let Some(pos) = candidates.iter().position(|(k, _)| k == key) {
            let picked = candidates.remove(pos);
            candidates.insert(0, picked);
        }
    }

    // P7: 選ばれたキーのスキルLv別配列がデフォルト倍率(`multiplierByLevel`)、
    // 候補一覧の各キーもそれぞれスキルLv別配列を持たせる(倍率候補ドロップダウンが
    // 現在のスキルLvでの値を表示できるようにする)。
    let default_multiplier_by_level = match &chosen_key {
        Some(key) => values_by_level_for_key(blackboard_by_level, key, num_levels),
        None => vec![1.0; num_levels],
    };
    let multiplier_candidates: Vec<MultiplierCandidate> = candidates
        .into_iter()
        .map(|(key, _)| {
            let values_by_level = values_by_level_for_key(blackboard_by_level, &key, num_levels);
            MultiplierCandidate { key, values_by_level }
        })
        .collect();

    (default_multiplier, default_multiplier_by_level, multiplier_candidates)
}
