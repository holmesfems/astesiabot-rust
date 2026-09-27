//! 条件付きバフの「ゲームデータからの動的値解決」(P4)。
//!
//! `data/fk_kill_calc/buffers.yaml`の`conditional.*.source`(生データは`buffers.rs`の
//! `RawConditionalSource`/`raw_conditional_sourced()`)を、`operator_combat`の素質
//! (`RawTalent`/モジュールの`talent_overrides_by_level`)または`skill_data`の
//! `blackboard_by_level`とマージし、`dto::Buffer`(`source`/`toggle`込み)を組み立てる。
//! `mod.rs::build_catalog`が呼ぶ3層構成の3層目(マージ)に相当する
//! (`build_inspire_sources`と同じ位置付け)。
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
//! 実データ調査済みの具体例(2026-09時点):
//!   - ポデンコ(char_258_podego)talent0: モジュールX上書きの値がベースと完全一致
//!     (Lv2/Lv3とも同じ.09/.11)なのでmodulesは空になる(意図的)。
//!   - ペペ(char_4058_pepe)talent1: モジュールA(uniequip_003_pepe)上書きも同様に
//!     ベースと完全一致(.16/.20)なのでmodulesは空。
//!
//! ## 解決アルゴリズム(スキルLv由来)
//! `skill_num`(fk_dataのskill_num。"1"〜"3"等)から`skill_id_by_num`でskillIdを解決し、
//! `SkillData::get_blackboard_by_level`のレベル1〜(データ数)の該当キー値をそのまま使う。
//! 昇進/潜在/モジュールの概念は無い(スキルLvの1軸のみ)。
//!
//! ## デフォルト(「最大成長」)
//! 昇進=E2・潜在=潜在6・(値が変わるモジュールがあれば)そのモジュールのLv3・
//! (スキルソースなら)最大レベルを採用する。`dto::Buffer.value`にはこの設定で解決した値を
//! 入れる(フロントが条件付きバフを初めてONにした時に見せる値と一致させるため)。

use super::buffers::{self, RawConditional};
use super::dto::{
    Buffer, BufferBonus, BufferKind, BufferScope, BuffToggle, ConditionalSkillSource, ConditionalSource, ConditionalSourceDefaults,
    ConditionalSourceModule, ConditionalTalentSource,
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
/// (ドリフト検知は`validate_conditional_sources`が担当する)。
fn build_talent_source(op: &RawOperatorCombat, talent_index: usize, key: &str) -> Option<ConditionalTalentSource> {
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
/// 解決できない/blackboardが無ければ`None`。
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
    Some(ConditionalSkillSource {
        skill_num: skill_num.to_string(),
        skill_label: if skill_label.is_empty() || skill_label == "Missing" { skill_num.to_string() } else { skill_label.to_string() },
        values_by_level,
    })
}

/// 「最大成長」のデフォルト選択状態を決める(昇進2・潜在6・値が変わるモジュールがあれば
/// そのLv3・スキルソースなら最大Lv)。
fn compute_defaults(talent: Option<&ConditionalTalentSource>, skill: Option<&ConditionalSkillSource>) -> ConditionalSourceDefaults {
    let module_id = talent.and_then(|t| t.modules.first()).map(|m| m.module_id.clone());
    let skill_level = skill.map(|s| s.values_by_level.len() as u8).unwrap_or(1);
    ConditionalSourceDefaults { elite: 2, potential: (NUM_POTENTIAL - 1) as u8, module_id, module_level: 3, skill_level }
}

/// `defaults`の選択状態で値を解決する(`Buffer.value`に入れる初期値)。
fn resolve_default_value(source: &ConditionalSource) -> f64 {
    if let Some(talent) = &source.talent {
        if let Some(module_id) = &source.defaults.module_id {
            if let Some(m) = talent.modules.iter().find(|m| &m.module_id == module_id) {
                return m.values_by_level_and_potential[(source.defaults.module_level - 1) as usize][source.defaults.potential as usize];
            }
        }
        return talent.values_by_elite_and_potential[source.defaults.elite as usize][source.defaults.potential as usize];
    }
    if let Some(skill) = &source.skill {
        let idx = (source.defaults.skill_level as usize).saturating_sub(1).min(skill.values_by_level.len().saturating_sub(1));
        return skill.values_by_level.get(idx).copied().unwrap_or(0.0);
    }
    0.0
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
        let Some(buffer) = build_one(cond, combat, ops, skills) else {
            skipped.push(format!("buff:{}", cond.id));
            continue;
        };
        out.push(buffer);
    }
    out
}

fn build_one(cond: &RawConditional, combat: &OperatorCombat, ops: &OperatorData, skills: &SkillData) -> Option<Buffer> {
    let source_spec = cond.source.as_ref()?;
    let op = combat.operators.get(&source_spec.operator)?;

    let talent = match source_spec.talent {
        Some(idx) => Some(build_talent_source(op, idx, &source_spec.key)?),
        None => None,
    };
    let skill = match &source_spec.skill_num {
        Some(num) => Some(build_skill_source(ops, skills, &source_spec.operator, num, &source_spec.key)?),
        None => None,
    };
    if talent.is_none() && skill.is_none() {
        return None;
    }

    let defaults = compute_defaults(talent.as_ref(), skill.as_ref());
    let source = ConditionalSource { operator_id: op.id.clone(), operator_name: op.name.clone(), talent, skill, defaults };
    let value = resolve_default_value(&source);

    let bonus = cond.bonus.as_ref().map(|b| build_bonus_dto(&cond.id, BufferKind::Pct, b));
    let toggle = cond.toggle.as_ref().map(|t| BuffToggle { label: t.label.clone(), mult: t.mult });

    Some(Buffer {
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
    })
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
        let Some(op) = combat.operators.get(&spec.operator) else {
            bad.push(format!("{} (operator id'{}'がoperator_combatに無い)", cond.id, spec.operator));
            continue;
        };
        match (spec.talent, &spec.skill_num) {
            (Some(idx), None) => match build_talent_source(op, idx, &spec.key) {
                Some(t) => {
                    let defaults = compute_defaults(Some(&t), None);
                    let default_value = resolve_default_value(&ConditionalSource {
                        operator_id: op.id.clone(),
                        operator_name: op.name.clone(),
                        talent: Some(t),
                        skill: None,
                        defaults,
                    });
                    if default_value <= 0.0 {
                        bad.push(format!(
                            "{} (operator'{}'のtalent[{idx}].{}が最大成長でも0以下)",
                            cond.id, spec.operator, spec.key
                        ));
                    }
                }
                None => bad.push(format!("{} (operator'{}'にtalent[{idx}]が無い)", cond.id, spec.operator)),
            },
            (None, Some(skill_num)) => match build_skill_source(ops, skills, &spec.operator, skill_num, &spec.key) {
                Some(s) => {
                    if s.values_by_level.iter().all(|v| *v <= 0.0) {
                        bad.push(format!("{} (operator'{}'のskill_num'{skill_num}'.{}が全レベルで0以下)", cond.id, spec.operator, spec.key));
                    }
                }
                None => bad.push(format!("{} (operator'{}'のskill_num'{skill_num}'が実スキルに解決できない)", cond.id, spec.operator)),
            },
            _ => bad.push(format!("{} (sourceはtalent/skill_numのどちらか一方だけを指定すること)", cond.id)),
        }
    }
    bad
}
