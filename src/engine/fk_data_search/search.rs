use super::dto::{FkSearchResult, FkSkillView, SkillCandidate};
use crate::engine::outer_source::fk_data::FkSheetData;
use crate::engine::outer_source::operator_data::OperatorData;
use crate::engine::outer_source::skill_data::SkillData;
use indexmap::IndexMap;
use std::collections::HashMap;

/// Python `FKInfo.getReply` 相当。オペレーター名・スキル指定からFK情報を解決する。
pub fn resolve(
    fk_data: &FkSheetData,
    operator_data: &OperatorData,
    skill_data: &SkillData,
    operator_name: &str,
    skill_num: &str,
) -> FkSearchResult {
    let Some(rows) = fk_data.by_operator.get(operator_name) else {
        return FkSearchResult::OperatorNotFound;
    };

    // skillNum(1始まりの文字列) -> skillId（Python `SkillFKInfo.__init__`の`idDict`）。
    let skill_id_by_num: HashMap<String, &str> = operator_data
        .get_by_name(operator_name)
        .map(|op| {
            op.skills
                .iter()
                .enumerate()
                .map(|(i, s)| ((i + 1).to_string(), s.skill_id.as_str()))
                .collect()
        })
        .unwrap_or_default();

    let resolve_name = |num: &str| -> String {
        skill_id_by_num
            .get(num)
            .map(|id| skill_data.get_str(id).to_string())
            .unwrap_or_default()
    };

    // `.strip()==""`の判定はトリム後、実際の一致比較は生値で行う非対称性をPython版から踏襲する
    // （ユーザーが空白混じりの値を入れた場合、空扱いにはならないが一致もしないという挙動）。
    let trimmed_empty = skill_num.trim().is_empty();
    if trimmed_empty && rows.len() != 1 {
        let mut choices: IndexMap<String, String> = IndexMap::new();
        for row in rows {
            let name = resolve_name(&row.skill_num);
            let display = if name.is_empty() { row.skill_num.clone() } else { name };
            choices.insert(row.skill_num.clone(), display);
        }
        return FkSearchResult::NeedsSkillSelection { choices };
    }

    let chosen = if rows.len() == 1 && trimmed_empty {
        Some(&rows[0])
    } else {
        rows.iter().find(|r| r.skill_num == skill_num)
    };

    match chosen {
        None => {
            let candidates = rows
                .iter()
                .map(|r| SkillCandidate {
                    skill_num: r.skill_num.clone(),
                    skill_name: resolve_name(&r.skill_num),
                })
                .collect();
            FkSearchResult::SkillNotFound { candidates }
        }
        Some(row) => FkSearchResult::Found(FkSkillView {
            skill_name: resolve_name(&row.skill_num),
            requested_skill_num: skill_num.to_string(),
            fk_num: row.fk_num.clone(),
            fk_err: row.fk_err.clone(),
            detail: row.detail.clone(),
        }),
    }
}

/// Python `FKInfo.autoComplete`相当。部分一致(`in`演算子=部分文字列)でオペレーター名を絞り込む。
pub fn autocomplete(fk_data: &FkSheetData, partial: &str, limit: usize) -> Vec<String> {
    fk_data
        .by_operator
        .keys()
        .filter(|name| name.contains(partial))
        .take(limit)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::outer_source::fk_data::FkSheetRow;

    fn row(skill_num: &str, fk_num: &str) -> FkSheetRow {
        FkSheetRow {
            skill_num: skill_num.to_string(),
            fk_num: fk_num.to_string(),
            fk_err: "誤差".to_string(),
            detail: "詳細".to_string(),
            last_edited: "2024-01-01".to_string(),
            state: "済".to_string(),
        }
    }

    fn fk_data_with(operator: &str, rows: Vec<FkSheetRow>) -> FkSheetData {
        let mut by_operator = IndexMap::new();
        by_operator.insert(operator.to_string(), rows);
        FkSheetData { by_operator }
    }

    #[test]
    fn operator_not_found() {
        let fk_data = fk_data_with("アステシア", vec![row("1", "3")]);
        let operator_data = OperatorData::empty_for_test();
        let skill_data = SkillData::empty_for_test();
        let result = resolve(&fk_data, &operator_data, &skill_data, "存在しない子", "1");
        assert!(matches!(result, FkSearchResult::OperatorNotFound));
    }

    #[test]
    fn single_skill_defaults_when_skill_num_empty() {
        let fk_data = fk_data_with("アステシア", vec![row("1", "3")]);
        let operator_data = OperatorData::empty_for_test();
        let skill_data = SkillData::empty_for_test();
        let result = resolve(&fk_data, &operator_data, &skill_data, "アステシア", "");
        match result {
            FkSearchResult::Found(view) => {
                assert_eq!(view.fk_num, "3");
                assert_eq!(view.requested_skill_num, "");
            }
            _ => panic!("expected Found"),
        }
    }

    #[test]
    fn multiple_skills_need_selection_when_skill_num_empty() {
        let fk_data = fk_data_with("アステシア", vec![row("1", "3"), row("2", "5")]);
        let operator_data = OperatorData::empty_for_test();
        let skill_data = SkillData::empty_for_test();
        let result = resolve(&fk_data, &operator_data, &skill_data, "アステシア", "");
        match result {
            FkSearchResult::NeedsSkillSelection { choices } => {
                assert_eq!(choices.len(), 2);
                assert_eq!(choices.get("1").unwrap(), "1");
            }
            _ => panic!("expected NeedsSkillSelection"),
        }
    }

    #[test]
    fn unknown_skill_num_returns_candidates() {
        let fk_data = fk_data_with("アステシア", vec![row("1", "3"), row("2", "5")]);
        let operator_data = OperatorData::empty_for_test();
        let skill_data = SkillData::empty_for_test();
        let result = resolve(&fk_data, &operator_data, &skill_data, "アステシア", "3");
        match result {
            FkSearchResult::SkillNotFound { candidates } => {
                assert_eq!(candidates.len(), 2);
            }
            _ => panic!("expected SkillNotFound"),
        }
    }

    #[test]
    fn autocomplete_filters_by_substring() {
        let fk_data = fk_data_with("アステシア", vec![row("1", "3")]);
        let hits = autocomplete(&fk_data, "テシ", 25);
        assert_eq!(hits, vec!["アステシア".to_string()]);
    }
}
