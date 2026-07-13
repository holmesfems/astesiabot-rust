use super::model::{Operator, OperatorDb, Tag, TagList, TagType};
use crate::bot::data::Error;
use itertools::Itertools;
use std::collections::HashSet;

/// 起動時に一度読み込んで保持するデータ。
pub struct RecruitData {
    operators_main: Vec<Operator>,
    operators_new: Vec<Operator>,
    tag_list: TagList,
    /// tagList の全タグを定義順に並べたもの（入力の並べ替え・フィルタに使う）
    tag_name_order: Vec<String>,
}

impl RecruitData {
    /// data/ 以下の2ファイルを読み込む。
    pub fn load() -> Result<Self, Error> {
        let db_text = std::fs::read_to_string("data/recruitmentOperators.json")?;
        let db: OperatorDb = serde_json::from_str(&db_text)?;

        let tag_text = std::fs::read_to_string("data/tagList.json")?;
        let tag_list: TagList = serde_json::from_str(&tag_text)?;

        // Python の tagNameList = jobTags + positionTags + eliteTags + otherTags
        let mut tag_name_order = Vec::new();
        tag_name_order.extend(tag_list.job_tags.iter().cloned());
        tag_name_order.extend(tag_list.position_tags.iter().cloned());
        tag_name_order.extend(tag_list.elite_tags.iter().cloned());
        tag_name_order.extend(tag_list.other_tags.iter().cloned());

        Ok(Self {
            operators_main: db.main,
            operators_new: db.new,
            tag_list,
            tag_name_order,
        })
    }

    /// タグ名から種別を判定（Python の tagType 相当）
    fn tag_type(&self, name: &str) -> TagType {
        if self.tag_list.elite_tags.iter().any(|t| t == name) {
            TagType::Elite
        } else if self.tag_list.job_tags.iter().any(|t| t == name) {
            TagType::Job
        } else {
            TagType::Other
        }
    }

    /// 入力タグ列を「既知のタグのみ・重複除去・定義順ソート」に正規化。
    /// Python の recruitDoProcess 冒頭の処理に対応。
    fn normalize_input(&self, input: &[String]) -> Vec<Tag> {
        let seen: HashSet<&String> = input.iter().collect();
        let mut valid: Vec<&String> = seen
            .into_iter()
            .filter(|t| self.tag_name_order.contains(t))
            .collect();
        // 定義順にソート
        valid.sort_by_key(|t| {
            self.tag_name_order.iter().position(|x| x == *t).unwrap()
        });
        valid
            .into_iter()
            .map(|name| Tag {
                name: name.clone(),
                tag_type: self.tag_type(name),
            })
            .collect()
    }

    /// 計算本体。入力タグから、★minStar 以上が確定するタグ組み合わせを返す。
    /// is_global=false なら new（大陸版）も対象に含める。
    pub fn calculate(
        &self,
        input_tags: &[String],
        is_global: bool,
        min_star: u8,
    ) -> Vec<MatchItem> {
        let tags = self.normalize_input(input_tags);

        // 1〜3個の全組み合わせ
        let mut combos: Vec<Vec<&Tag>> = Vec::new();
        for k in 1..=3 {
            for combo in tags.iter().combinations(k) {
                combos.push(combo);
            }
        }

        // 対象オペレータープール
        let pool: Vec<&Operator> = if is_global {
            self.operators_main.iter().collect()
        } else {
            self.operators_main
                .iter()
                .chain(self.operators_new.iter())
                .collect()
        };

        // minStar==4 のとき showRobot=true（Python の recruitDoProcess）
        let show_robot = min_star == 4;

        let mut result = Vec::new();
        for combo in &combos {
            let satisfied: Vec<&Operator> = pool
                .iter()
                .filter(|op| satisfy_tags(op, combo))
                .copied()
                .collect();
            if satisfied.is_empty() {
                continue;
            }
            let ms = min_star_of(&satisfied);
            // Python: minStar==1 でrobot表示 or minStar>=閾値 なら採用
            let keep = (ms == 1 && show_robot) || ms >= min_star;
            if keep {
                let names: Vec<String> =
                    satisfied.iter().map(|o| o.name.clone()).collect();
                result.push(MatchItem {
                    combo: combo.iter().map(|t| t.name.clone()).collect(),
                    min_star: ms,
                    operators: names,
                    star_set: satisfied.iter().map(|o| o.stars).collect(),
                });
            }
        }
        result
    }
}

/// 1組み合わせ分の計算結果。
#[derive(Debug)]
pub struct MatchItem {
    pub combo: Vec<String>,
    pub min_star: u8,
    pub operators: Vec<String>,
    pub star_set: HashSet<u8>,
}

/// オペレーターが全タグを満たすか（Python の satisfyTags）。
fn satisfy_tags(op: &Operator, combo: &[&Tag]) -> bool {
    let need_elite = op.stars == 6;
    let mut has_elite = false;
    for tag in combo {
        match tag.tag_type {
            TagType::Elite => {
                has_elite = true;
                let ok = (op.stars == 5 && tag.name == "エリート")
                    || (op.stars == 6 && tag.name == "上級エリート");
                if !ok {
                    return false;
                }
            }
            TagType::Job => {
                if op.job != tag.name {
                    return false;
                }
            }
            TagType::Other => {
                if !op.tags.iter().any(|t| t == &tag.name) {
                    return false;
                }
            }
        }
    }
    if need_elite && !has_elite {
        return false;
    }
    true
}

/// オペレーター集合の「最低星」（Python の minStar）。
/// 3以上があればその最小、なければ3未満の最大。
fn min_star_of(ops: &[&Operator]) -> u8 {
    let stars: HashSet<u8> = ops.iter().map(|o| o.stars).collect();
    let high: Vec<u8> = stars.iter().copied().filter(|&s| s >= 3).collect();
    if let Some(&m) = high.iter().min() {
        return m;
    }
    stars.iter().copied().max().unwrap_or(0)
}