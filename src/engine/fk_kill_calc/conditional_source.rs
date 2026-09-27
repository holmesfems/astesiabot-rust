//! 「条件付き/個別バフの「ゲームデータからの動的値解決」(P4で条件付き向けに追加、
//! P5で個別バフにも対応させ3種のソースを追加)。
//!
//! `data/fk_kill_calc/buffers.yaml`の`conditional.*.source`/`individual.*.source`(生データは
//! `buffers.rs`の`RawConditionalSource`/`raw_conditional_sourced()`/`raw_individual_sourced()`)を、
//! `operator_combat`の素質(`RawTalent`/モジュールの`talent_overrides_by_level`)または
//! `skill_data`の`blackboard_by_level`/`blackboard`とマージし、`dto::Buffer`(`source`/`toggle`込み)
//! を組み立てる。`mod.rs::build_catalog`が呼ぶ3層構成の3層目(マージ)に相当する
//! (`build_inspire_sources`と同じ位置付け)。
//!
//! ## ソースの5パターン
//! `build_source`が1つの関数でまとめて扱う。実際に使われる組み合わせ(2026-09時点):
//!   - `talent`のみ: 素質値そのもの(例: castle3、エクシア、前衛アーミヤ)
//!   - `skill_num`のみ: スキルLv別blackboardがそのまま値(例: 血漿、ドリアン、ズィマー、
//!     スプリア)
//!   - `talent` × `scale_skill_num`: 素質値にスキルLv別スケールを掛ける(例: スワイヤーS1
//!     [talent0.atk × skill1.talent_scale(全レベル2.0固定)]、スワイヤーS2
//!     [talent0.atk × skill2.talent_scale(2.1〜3.0)])
//!   - `base_pct` × `scale_skill_num`: ゲームデータに存在しない固定基礎値(トークン等由来)に
//!     スキルLv別スケールを掛ける(例: ステインレスS1。装置本体の+12%はトークン
//!     [token_10027_ironmn_pile1]の素質でoperator_combatに存在しないため、YAML直書きの
//!     定数として`base_pct: 0.12`を持つ×skill1.fake_scale)
//!   - `stage_skill_id`+`stage_keys`+`stage_labels`: スキルLvではなく、同一skillの
//!     blackboard上にある複数の名前付きキーを離散的な「段階」として使う(例: ナスティS3。
//!     token skill`sktok_nasti_nstbld`のblackboard[最大Lv]が
//!     `attack@nasti_nstbld[m4_bonus/m5_bonus/m6_bonus].atk`という3つのキーを同時に
//!     持っており、これは装置のアップグレード段階[1〜3段階]を表す。スキルLv自体は
//!     常に最大[特化3]を使う)
//!
//! ## 解決アルゴリズム(素質由来)
//! 昇進(elite。0=E0/1=E1/2=E2)・潜在(potentialRank。0=潜在1〜5=潜在6)の組み合わせごとに、
//! 該当talentIndexの候補一覧から`phase <= elite && potentialRank <= 選択`を満たす最後の
//! (=候補一覧上、条件を満たす中で一番後ろの。ゲームデータの候補は昇進/潜在の昇順で
//! 並んでいるため「一番後ろ」が「一番強い」と一致する)ものを採用し、該当キーの値を取る
//! (無ければ0)。モジュールを装備している場合は、そのモジュールのLvに対応する
//! `talent_overrides_by_level`から同じ規則(potentialRankのみでフィルタ。モジュールの
//! 上書きは昇進を見ない=モジュールは実質E2必須なので昇進条件を重複判定する必要が無い)
//! で候補を探し、見つかればベースの代わりに採用する(見つからなければベースの
//! E2側フォールバック)。
//!
//! ## dedupe(値が変わらない軸を隠す)
//! 「昇進」「潜在」「モジュール」はいずれも、実際に値が変わる場合だけUIに見せる
//! (`elite_varies`/`potential_varies`/`modules`。空/falseならその軸のセレクトを出さない)。
//! 判定は「潜在最大(5)で昇進0/1/2を比較(elite_varies)」「昇進最大(2)で潜在0〜5を比較
//! (potential_varies)」「モジュール各Lv×各潜在の値をベース(E2側)と比較して1箇所でも
//! 違えば採用(modules)」という単純な全探索(高々3×6=18通り)で行う。
//! P5で追加した「スキルLv別スケール」(`scale`)にも同じ思想のdedupeを適用する
//! (`ConditionalSkillSource::varies`。全レベルで値が同じなら`varies=false`とし、UIはこの
//! 軸のセレクトを出さない。例: スワイヤーS1のtalent_scaleは全レベル2.0固定なので
//! `varies=false`)。
//! 実データ調査済みの具体例(2026-09時点):
//!   - ポデンコ(char_258_podego)talent0: モジュールX上書きの値がベースと完全一致
//!     (Lv2/Lv3とも同じ.09/.11)なのでmodulesは空になる(意図的)。
//!   - ペペ(char_4058_pepe)talent1: モジュールA(uniequip_003_pepe)上書きも同様に
//!     ベースと完全一致(.16/.20)なのでmodulesは空。
//!
//! ## 解決アルゴリズム(スキルLv由来。`skill_num`単独)
//! `skill_num`(fk_dataのskill_num。"1"〜"3"等)から`skill_id_by_num`でskillIdを解決し、
//! `SkillData::get_blackboard_by_level`のレベル1〜(データ数)の該当キー値をそのまま使う。
//! 昇進/潜在/モジュールの概念は無い(スキルLvの1軸のみ)。
//!
//! ## 解決アルゴリズム(P5。スケール乗算・固定基礎値・段階)
//! - `scale_skill_num`/`scale_key`: `skill_num`単独と全く同じ仕組み(`build_skill_source`を
//!   再利用)で値テーブルを作り、`talent`(または`base_pct`)の値に掛け合わせる。
//!   `defaults.skill_level`(単独`skill`と共有するフィールド。両者は排他なので衝突しない)
//!   でどのレベルを使うか選ぶ。
//! - `base_pct`: ゲームデータから機械抽出しない固定の定数。`talent`の代わりに使う
//!   (そのまま`scale`と掛け合わせる)。
//! - `stage_skill_id`: `skill_id_by_num`を経由せず、直接skillIdを指定して
//!   `SkillData::get_blackboard`(最大Lvのフラットな値)から`stage_keys`の各キー値を引く。
//!   `stage_labels`はUI表示用のラベル(例: "1段階"〜"3段階")。`talent`/`skill_num`/
//!   `scale_skill_num`/`base_pct`とは排他。
//!
//! ## 対象人数の拡張(P5。`max_targets_by_module`。個別バフ専用)
//! エクシアの素質「配置後ランダムな味方1名にも同じ効果を付与」はモジュールX
//! (uniequip_002_angel)Lv2以上を装備すると対象が2名になる。値の解決自体には関与せず、
//! `single_target`警告(`engine.js`の`findSingleTargetConflicts`)が許容する選択数の
//! 上限を、現在選択中のモジュール/Lvに応じて1→`count`へ緩和するためだけに使う。
//!
//! ## デフォルト(「最大成長」)
//! 昇進=E2・潜在=潜在6・(値が変わるモジュールがあれば)そのモジュールのLv3・
//! (スキル/スケールソースなら)最大レベル・(段階ソースなら)最終段階を採用する。
//! `dto::Buffer.value`にはこの設定で解決した値を入れる(フロントが初めてONにした時に
//! 見せる値と一致させるため)。
use super::buffers::{self, RawConditionalSource};
use super::dto::{
    Buffer, BufferBonus, BufferKind, BufferScope, BuffToggle, ConditionalSkillSource, ConditionalSource, ConditionalSourceDefaults,
    ConditionalSourceModule, ConditionalStageSource, ConditionalTalentSource, MaxTargetsByModule,
};
use crate::engine::external_source::operator_combat::{OperatorCombat, RawModuleCombat, RawOperatorCombat, RawTalent};
use crate::engine::external_source::operator_data::OperatorData;
use crate::engine::external_source::skill_data::SkillData;
use crate::engine::fk_data_search::search::skill_id_by_num;

const NUM_ELITE: usize = 3;
const NUM_POTENTIAL: usize = 6;

/// talentの候補一覧から、`phase <= elite && potentialRank <= potential`を満たす中で
/// 最後(=一番強い)のものの`key`値を取る。無ければ0.0。
fn resolve_talent_base(talent: &RawTalent, key: &str, elite: u8, potential: u8) -> f64 {
    talent
        .candidates
        .iter()
        .filter(|c| c.phase <= elite && c.potential_rank <= potential)
        .filter_map(|c| c.blackboard.get(key))
        .last()
        .copied()
        .unwrap_or(0.0)
}

/// モジュール1種の、あるLv(0=Lv1/1=Lv2/2=Lv3)におけるtalent_index上書き候補から
/// `potentialRank <= potential`を満たす最後のものの`key`値を取る。無ければ`None`
/// (呼び出し側がベース値へフォールバックする)。
fn resolve_module_override(module: &RawModuleCombat, talent_index: usize, key: &str, level_idx: usize, potential: u8) -> Option<f64> {
    module
        .talent_overrides_by_level
        .get(level_idx)?
        .iter()
        .filter(|c| c.talent_index == talent_index && c.potential_rank <= potential)
        .filter_map(|c| c.blackboard.get(key))
        .last()
        .copied()
}

/// 素質由来の`ConditionalTalentSource`を組み立てる。`talent_index`が範囲外なら`None`
/// (ドリフト検知は`validate_conditional_sources`/`validate_individual_sources`が担当する)。
/// `mod.rs`の`to_special_dto`(P8 follow-up。特殊強化の乗算系`mul_multiplier`)からも
/// 同じビルダーを再利用するため`pub(super)`にしている。
pub(super) fn build_talent_source(op: &RawOperatorCombat, talent_index: usize, key: &str) -> Option<ConditionalTalentSource> {
    let talent = op.talents.get(talent_index)?;

    let mut values = [[0.0_f64; NUM_POTENTIAL]; NUM_ELITE];
    for (elite, row) in values.iter_mut().enumerate() {
        for (potential, cell) in row.iter_mut().enumerate() {
            *cell = resolve_talent_base(talent, key, elite as u8, potential as u8);
        }
    }

    let elite_varies = {
        let at_max_potential: Vec<f64> = (0..NUM_ELITE).map(|e| values[e][NUM_POTENTIAL - 1]).collect();
        at_max_potential.iter().any(|v| (v - at_max_potential[0]).abs() > 1e-9)
    };
    let potential_varies = {
        let at_max_elite = values[NUM_ELITE - 1];
        at_max_elite.iter().any(|v| (v - at_max_elite[0]).abs() > 1e-9)
    };

    let mut modules = Vec::new();
    for m in &op.modules {
        let mut mvalues = [[0.0_f64; NUM_POTENTIAL]; 3];
        let mut differs = false;
        for level_idx in 0..3usize {
            for potential in 0..NUM_POTENTIAL {
                let base = values[NUM_ELITE - 1][potential]; // モジュールはE2前提なのでE2側にフォールバック
                let value = resolve_module_override(m, talent_index, key, level_idx, potential as u8).unwrap_or(base);
                mvalues[level_idx][potential] = value;
                if (value - base).abs() > 1e-9 {
                    differs = true;
                }
            }
        }
        if differs {
            modules.push(ConditionalSourceModule {
                module_id: m.eq_id.clone(),
                type_name: m.eq_type.clone(),
                name: m.name.clone(),
                values_by_level_and_potential: mvalues,
            });
        }
    }

    Some(ConditionalTalentSource { values_by_elite_and_potential: values, elite_varies, potential_varies, modules })
}

/// スキルLv由来の`ConditionalSkillSource`を組み立てる。skill_numから実スキルが
/// 解決できない/blackboardが無ければ`None`。`ConditionalSource.skill`(値そのもの)と
/// `ConditionalSource.scale`(P5。他の値に掛ける倍率)の両方でこの関数を使い回す
/// (呼び出し側が渡す`skill_num`/`key`が違うだけで、構築ロジックは同一)。
fn build_skill_source(ops: &OperatorData, skills: &SkillData, operator_id: &str, skill_num: &str, key: &str) -> Option<ConditionalSkillSource> {
    let cost_op = ops.operators.get(operator_id)?;
    let skill_ids = skill_id_by_num(cost_op);
    let skill_id = skill_ids.get(skill_num)?;
    let by_level = skills.get_blackboard_by_level(skill_id)?;
    let skill_label = skills.get_str(skill_id);
    let values_by_level: Vec<f64> = by_level.iter().map(|bb| bb.get(key).copied().unwrap_or(0.0)).collect();
    if values_by_level.is_empty() {
        return None;
    }
    let varies = values_by_level.iter().any(|v| (v - values_by_level[0]).abs() > 1e-9);
    Some(ConditionalSkillSource {
        skill_num: skill_num.to_string(),
        skill_label: if skill_label.is_empty() || skill_label == "Missing" { skill_num.to_string() } else { skill_label.to_string() },
        values_by_level,
        varies,
    })
}

/// 離散的な「段階」で値が変わる`ConditionalStageSource`を組み立てる(P5)。`skill_id`を
/// `skill_id_by_num`経由ではなく直接指定する(トークンスキル等、そのオペレーター自身の
/// skill_num一覧には載らないため)。`stage_keys`/`stage_labels`は同じ要素数であること
/// (`validate_one_source`が検証する)。
fn build_stage_source(skills: &SkillData, skill_id: &str, keys: &[String], labels: &[String]) -> Option<ConditionalStageSource> {
    if keys.is_empty() || keys.len() != labels.len() {
        return None;
    }
    let blackboard = skills.get_blackboard(skill_id)?;
    let skill_label = skills.get_str(skill_id);
    let values: Vec<f64> = keys.iter().map(|k| blackboard.get(k).copied().unwrap_or(0.0)).collect();
    Some(ConditionalStageSource {
        skill_id: skill_id.to_string(),
        skill_label: if skill_label.is_empty() || skill_label == "Missing" { skill_id.to_string() } else { skill_label.to_string() },
        values,
        labels: labels.to_vec(),
    })
}

/// 「最大成長」のデフォルト選択状態を決める(昇進2・潜在6・値が変わるモジュールがあれば
/// そのLv3・スキル/スケールソースなら最大Lv・段階ソースなら最終段階)。
fn compute_defaults(
    talent: Option<&ConditionalTalentSource>,
    skill: Option<&ConditionalSkillSource>,
    scale: Option<&ConditionalSkillSource>,
    stage: Option<&ConditionalStageSource>,
) -> ConditionalSourceDefaults {
    let module_id = talent.and_then(|t| t.modules.first()).map(|m| m.module_id.clone());
    // skillとscaleは排他(片方だけが`Some`)なので、どちらの長さを見ても衝突しない。
    let skill_level = skill.map(|s| s.values_by_level.len() as u8).or_else(|| scale.map(|s| s.values_by_level.len() as u8)).unwrap_or(1);
    let stage_index = stage.map(|s| s.values.len() as u8).unwrap_or(1);
    ConditionalSourceDefaults { elite: 2, potential: (NUM_POTENTIAL - 1) as u8, module_id, module_level: 3, skill_level, stage_index }
}

/// `defaults`の選択状態で値を解決する(`Buffer.value`に入れる初期値)。P5で追加した
/// `scale`/`base_pct`/`stage`を含めた統一ロジック:
///   - `stage`があれば段階値をそのまま返す(他の軸とは排他)。
///   - `talent`/`base_pct`のどちらも無ければ(=`skill`単独。値そのもの)そのまま返す。
///   - それ以外は`primary(talent or base_pct) × scale係数(scaleが無ければ1.0)`。
fn resolve_default_value(source: &ConditionalSource) -> f64 {
    if let Some(stage) = &source.stage {
        let idx = (source.defaults.stage_index as usize).saturating_sub(1).min(stage.values.len().saturating_sub(1));
        return stage.values.get(idx).copied().unwrap_or(0.0);
    }

    if source.talent.is_none() && source.base_pct.is_none() {
        if let Some(skill) = &source.skill {
            let idx = (source.defaults.skill_level as usize).saturating_sub(1).min(skill.values_by_level.len().saturating_sub(1));
            return skill.values_by_level.get(idx).copied().unwrap_or(0.0);
        }
        return 0.0;
    }

    let primary = if let Some(talent) = &source.talent {
        if let Some(module_id) = &source.defaults.module_id {
            if let Some(m) = talent.modules.iter().find(|m| &m.module_id == module_id) {
                m.values_by_level_and_potential[(source.defaults.module_level - 1) as usize][source.defaults.potential as usize]
            } else {
                talent.values_by_elite_and_potential[source.defaults.elite as usize][source.defaults.potential as usize]
            }
        } else {
            talent.values_by_elite_and_potential[source.defaults.elite as usize][source.defaults.potential as usize]
        }
    } else {
        source.base_pct.unwrap_or(0.0)
    };

    let scale_factor = if let Some(scale) = &source.scale {
        let idx = (source.defaults.skill_level as usize).saturating_sub(1).min(scale.values_by_level.len().saturating_sub(1));
        scale.values_by_level.get(idx).copied().unwrap_or(1.0)
    } else {
        1.0
    };

    primary * scale_factor
}

/// `RawConditionalSource`(individual/conditional共通の生データ)から`ConditionalSource`
/// DTOを組み立てる。実データと不一致(オペレーターID/talentIndex/skill_num/skill_id等が
/// 存在しない)なら`None`(呼び出し側が`skipped`へ記録して静かに落とす)。
fn build_source(spec: &RawConditionalSource, combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData) -> Option<ConditionalSource> {
    let op = combat.operators.get(&spec.operator)?;

    // 段階ソース(P5)は他の軸と排他なので、他を見る前に確定させる。
    if let Some(skill_id) = &spec.stage_skill_id {
        let stage = build_stage_source(skills, skill_id, spec.stage_keys.as_deref()?, spec.stage_labels.as_deref()?)?;
        let defaults = compute_defaults(None, None, None, Some(&stage));
        return Some(ConditionalSource {
            operator_id: op.id.clone(),
            operator_name: op.name.clone(),
            talent: None,
            skill: None,
            scale: None,
            base_pct: None,
            stage: Some(stage),
            max_targets_by_module: None,
            defaults,
        });
    }

    let talent = match spec.talent {
        Some(idx) => Some(build_talent_source(op, idx, spec.key.as_deref()?)?),
        None => None,
    };
    let scale = match &spec.scale_skill_num {
        Some(num) => Some(build_skill_source(ops, skills, &spec.operator, num, spec.scale_key.as_deref()?)?),
        None => None,
    };
    let skill = match &spec.skill_num {
        Some(num) => Some(build_skill_source(ops, skills, &spec.operator, num, spec.key.as_deref()?)?),
        None => None,
    };
    if talent.is_none() && skill.is_none() && spec.base_pct.is_none() {
        return None;
    }

    let max_targets_by_module =
        spec.max_targets_by_module.as_ref().map(|mt| MaxTargetsByModule { module_id: mt.module.clone(), min_level: mt.min_level, count: mt.count });

    let defaults = compute_defaults(talent.as_ref(), skill.as_ref(), scale.as_ref(), None);
    Some(ConditionalSource {
        operator_id: op.id.clone(),
        operator_name: op.name.clone(),
        talent,
        skill,
        scale,
        base_pct: spec.base_pct,
        stage: None,
        max_targets_by_module,
        defaults,
    })
}

/// `RawConditional.bonus`をDTO(`BufferBonus`)へ変換する。`mult`指定はそのまま渡す
/// (固定値`pct`/`flat`は`buffers::build_fixed_bonus`を再利用する)。
fn build_bonus_dto(cond_id: &str, kind: BufferKind, bonus: &buffers::RawBonus) -> BufferBonus {
    if let Some(mult) = bonus.mult {
        return BufferBonus { target_tags: bonus.tags.clone(), value: None, mult: Some(mult), note: bonus.note.clone() };
    }
    buffers::build_fixed_bonus(cond_id, kind, bonus)
}

/// `raw_conditional_sourced()`(YAML生データ)と`operator_combat`/`operator_data`/
/// `skill_data`をマージして`dto::Buffer`一覧を組み立てる。対象オペレーター/talentIndex/
/// skill_numが実データに存在しない場合は`skipped`に`"buff:<id>"`として記録し、その
/// バフをカタログから静かに落とす(fk_dataのskipped/inspireのskippedと同じ方針。
/// ドリフト自体は`cargo test`の`validate_conditional_sources`で検知する)。
pub fn build_conditional_sourced_buffers(combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData, skipped: &mut Vec<String>) -> Vec<Buffer> {
    let mut out = Vec::new();
    for cond in buffers::raw_conditional_sourced() {
        let spec = cond.source.as_ref().expect("raw_conditional_sourced()はsourceを持つはず");
        let Some(source) = build_source(spec, combat, ops, skills) else {
            skipped.push(format!("buff:{}", cond.id));
            continue;
        };
        let value = resolve_default_value(&source);
        let bonus = cond.bonus.as_ref().map(|b| build_bonus_dto(&cond.id, BufferKind::Pct, b));
        let toggle = cond.toggle.as_ref().map(|t| BuffToggle { label: t.label.clone(), mult: t.mult });

        out.push(Buffer {
            id: cond.id.clone(),
            name: cond.name.clone(),
            kind: BufferKind::Pct,
            value,
            scope: BufferScope::Conditional { target_tags: cond.targets.clone() },
            single_target: false,
            bonus,
            exclusive_group: cond.exclusive_group.clone(),
            source: Some(source),
            toggle,
            note: cond.note.clone(),
        });
    }
    out
}

/// `raw_individual_sourced()`(YAML生データ。P5)と`operator_combat`/`operator_data`/
/// `skill_data`をマージして`dto::Buffer`一覧を組み立てる。`build_conditional_sourced_buffers`
/// の個別バフ版(ロジックは`build_source`を共有する)。個別バフの`source`は今のところ
/// 全て`pct`種のみ(`flat`種が必要になったら`RawIndividual`に`kind`を足して拡張する)。
pub fn build_individual_sourced_buffers(combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData, skipped: &mut Vec<String>) -> Vec<Buffer> {
    let mut out = Vec::new();
    for ind in buffers::raw_individual_sourced() {
        let spec = ind.source.as_ref().expect("raw_individual_sourced()はsourceを持つはず");
        let Some(source) = build_source(spec, combat, ops, skills) else {
            skipped.push(format!("buff:{}", ind.id));
            continue;
        };
        let value = resolve_default_value(&source);
        let toggle = ind.toggle.as_ref().map(|t| BuffToggle { label: t.label.clone(), mult: t.mult });

        out.push(Buffer {
            id: ind.id.clone(),
            name: ind.name.clone(),
            kind: BufferKind::Pct,
            value,
            scope: BufferScope::Individual,
            single_target: ind.single_target,
            bonus: None,
            exclusive_group: None,
            source: Some(source),
            toggle,
            note: ind.note.clone(),
        });
    }
    out
}

/// 1件分のソース仕様が実データ(operator_combat/operator_data/skill_data)と整合しているかを
/// 検証する。`scope_label`はエラーメッセージの接頭辞("conditional"/"individual")。
fn validate_one_source(
    scope_label: &str,
    id: &str,
    spec: &RawConditionalSource,
    combat: &OperatorCombat,
    ops: &OperatorData,
    skills: &SkillData,
) -> Vec<String> {
    let mut bad = Vec::new();
    let Some(op) = combat.operators.get(&spec.operator) else {
        bad.push(format!("{scope_label}:{id} (operator id'{}'がoperator_combatに無い)", spec.operator));
        return bad;
    };

    if let Some(skill_id) = &spec.stage_skill_id {
        match build_stage_source(skills, skill_id, spec.stage_keys.as_deref().unwrap_or(&[]), spec.stage_labels.as_deref().unwrap_or(&[])) {
            Some(stage) => {
                if stage.values.iter().all(|v| *v <= 0.0) {
                    bad.push(format!("{scope_label}:{id} (stage_skill_id'{skill_id}'の値が全段階で0以下)"));
                }
            }
            None => bad.push(format!("{scope_label}:{id} (stage_skill_id'{skill_id}'が実スキルに解決できない、またはstage_keys/stage_labelsが不正)")),
        }
        return bad;
    }

    let talent = match spec.talent {
        Some(idx) => match build_talent_source(op, idx, spec.key.as_deref().unwrap_or("")) {
            Some(t) => Some(t),
            None => {
                bad.push(format!("{scope_label}:{id} (operator'{}'にtalent[{idx}]が無い)", spec.operator));
                return bad;
            }
        },
        None => None,
    };
    let scale = match &spec.scale_skill_num {
        Some(num) => match build_skill_source(ops, skills, &spec.operator, num, spec.scale_key.as_deref().unwrap_or("")) {
            Some(s) => Some(s),
            None => {
                bad.push(format!("{scope_label}:{id} (operator'{}'のscale_skill_num'{num}'が実スキルに解決できない)", spec.operator));
                return bad;
            }
        },
        None => None,
    };
    let skill = match &spec.skill_num {
        Some(num) => match build_skill_source(ops, skills, &spec.operator, num, spec.key.as_deref().unwrap_or("")) {
            Some(s) => Some(s),
            None => {
                bad.push(format!("{scope_label}:{id} (operator'{}'のskill_num'{num}'が実スキルに解決できない)", spec.operator));
                return bad;
            }
        },
        None => None,
    };

    if talent.is_none() && skill.is_none() && spec.base_pct.is_none() {
        bad.push(format!("{scope_label}:{id} (talent/skill_num/base_pctのいずれも無い)"));
        return bad;
    }

    let defaults = compute_defaults(talent.as_ref(), skill.as_ref(), scale.as_ref(), None);
    let source = ConditionalSource {
        operator_id: op.id.clone(),
        operator_name: op.name.clone(),
        talent,
        skill,
        scale,
        base_pct: spec.base_pct,
        stage: None,
        max_targets_by_module: None,
        defaults,
    };
    let value = resolve_default_value(&source);
    if value <= 0.0 {
        bad.push(format!("{scope_label}:{id} (最大成長でも値が0以下)"));
    }

    if let Some(mt) = &spec.max_targets_by_module {
        if !op.modules.iter().any(|m| m.eq_id == mt.module) {
            bad.push(format!("{scope_label}:{id} (max_targets_by_module.module'{}'が'{}'のmodulesに無い)", mt.module, spec.operator));
        }
    }
    bad
}

/// `raw_conditional_sourced()`が実データ(operator_combat/operator_data/skill_data)と
/// 整合しているかを検証する。不一致の一覧を返す(空ならOK)。ゲームデータ更新で
/// オペレーターID・talentIndex・skill_num・キー名が変わった際のドリフト検知用
/// (`cargo test`で実行する)。
#[allow(dead_code)] // `mod.rs`の`#[cfg(test)]`からのみ呼ばれる(通常ビルドでは未使用)。
pub fn validate_conditional_sources(combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData) -> Vec<String> {
    let mut bad = Vec::new();
    for cond in buffers::raw_conditional_sourced() {
        let Some(spec) = &cond.source else { continue };
        bad.extend(validate_one_source("conditional", &cond.id, spec, combat, ops, skills));
    }
    bad
}

/// `raw_individual_sourced()`が実データと整合しているかを検証する(P5。
/// `validate_conditional_sources`の個別バフ版)。
#[allow(dead_code)] // `mod.rs`の`#[cfg(test)]`からのみ呼ばれる(通常ビルドでは未使用)。
pub fn validate_individual_sources(combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData) -> Vec<String> {
    let mut bad = Vec::new();
    for ind in buffers::raw_individual_sourced() {
        let Some(spec) = &ind.source else { continue };
        bad.extend(validate_one_source("individual", &ind.id, spec, combat, ops, skills));
    }
    bad
}
