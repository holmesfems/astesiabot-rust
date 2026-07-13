use super::model::{Operator, OperatorDb, Tag, TagList, TagType};
use super::Error;
use itertools::Itertools;
use std::collections::{BTreeSet, HashSet};
use chrono::{NaiveDate, LocalResult, TimeZone, Utc};
use chrono_tz::Asia::Tokyo;
use std::env;

/// 起動時に一度読み込んで保持するデータ。
pub struct RecruitData {
    operators_main: Vec<Operator>,
    operators_new: Vec<Operator>,
    tag_list: TagList,
    /// tagList の全タグを定義順に並べたもの（入力の並べ替え・フィルタに使う）
    /// Python: tagNameList = jobTags + positionTags + eliteTags + otherTags
    tag_name_order: Vec<String>,
}

impl RecruitData {
    /// data/ 以下の2ファイルを読み込む。
    pub fn load() -> Result<Self, Error> {
        let db_text = std::fs::read_to_string("data/recruitmentOperators.json")?;
        let db: OperatorDb = serde_json::from_str(&db_text)?;

        let tag_text = std::fs::read_to_string("data/tagList.json")?;
        let tag_list: TagList = serde_json::from_str(&tag_text)?;

        let mut tag_name_order = Vec::new();
        tag_name_order.extend(tag_list.job_tags.iter().cloned());
        tag_name_order.extend(tag_list.position_tags.iter().cloned());
        tag_name_order.extend(tag_list.elite_tags.iter().cloned());
        tag_name_order.extend(tag_list.other_tags.iter().cloned());

        // Handle `future` entries: decide whether each future opList belongs to `new` or `main`
        // based on whether now is before the date at configured hour (default 16).
        let mut operators_main = db.main;
        let mut operators_new = db.new;

        // configurable hour via env RECRUIT_FUTURE_HOUR (0-23), default 16
        let future_hour: u32 = env::var("RECRUIT_FUTURE_HOUR")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&h| h < 24)
            .unwrap_or(16);

        // get current time in Asia/Tokyo explicitly
        let now = Utc::now().with_timezone(&Tokyo);
        for fe in db.future.iter() {
            match Self::parse_future_date(&fe.yyyymmdd, future_hour) {
                Some(t_dt) => {
                    if now < t_dt {
                        operators_new.extend(fe.op_list.clone());
                    } else {
                        operators_main.extend(fe.op_list.clone());
                    }
                }
                None => {
                    eprintln!("warning: failed to parse future date '{}', skipping", fe.yyyymmdd);
                }
            }
        }

        Ok(Self {
            operators_main,
            operators_new,
            tag_list,
            tag_name_order,
        })
    }

/// Parse a yyyymmdd or yymmdd string and return a Local datetime at the given hour.
fn parse_future_date(s: &str, hour: u32) -> Option<chrono::DateTime<chrono_tz::Tz>> {
    // try YYYYMMDD then YYMMDD
    let parsed_date = NaiveDate::parse_from_str(s, "%Y%m%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%y%m%d")).ok()?;
    let naive_dt = parsed_date.and_hms_opt(hour, 0, 0)?;
    match Tokyo.from_local_datetime(&naive_dt) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Tokyo)),
        LocalResult::Ambiguous(dt1, _) => Some(dt1.with_timezone(&Tokyo)),
        LocalResult::None => None,
    }
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
        valid.sort_by_key(|t| self.tag_name_order.iter().position(|x| x == *t).unwrap());
        valid
            .into_iter()
            .map(|name| Tag {
                name: name.clone(),
                tag_type: self.tag_type(name),
            })
            .collect()
    }

    /// title 用：既知タグのみ・定義順ソートした「名前」を返す。
    pub fn normalize_names(&self, input: &[String]) -> Vec<String> {
        self.normalize_input(input)
            .into_iter()
            .map(|t| t.name)
            .collect()
    }

    /// 計算本体。入力タグから、条件を満たすタグ組み合わせを返す。
    /// is_global=false なら new（大陸版）も対象に含める。
    /// pickup があれば、その対象を含む組み合わせを星に関わらず採用する。
    pub fn calculate(
        &self,
        input_tags: &[String],
        is_global: bool,
        min_star: u8,
        pickup: Option<&[String]>,
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
            let mut satisfied: Vec<&Operator> = pool
                .iter()
                .filter(|op| satisfy_tags(op, combo))
                .copied()
                .collect();
            if satisfied.is_empty() {
                continue;
            }
            // 星昇順ソート（Python の OperatorList は stars 昇順）
            satisfied.sort_by_key(|o| o.stars);
            let ms = min_star_of(&satisfied);

            // ピックアップ判定
            let (contains_pickup, pickup_target) = match pickup {
                Some(pu) => {
                    let names: Vec<String> =
                        satisfied.iter().map(|o| o.name.clone()).collect();
                    let pt: Vec<String> =
                        pu.iter().filter(|t| names.contains(t)).cloned().collect();
                    (!pt.is_empty(), pt)
                }
                None => (false, Vec::new()),
            };

            // 採用判定（Python: containsPickup優先 → robot → minStar閾値）
            let keep = contains_pickup || (ms == 1 && show_robot) || ms >= min_star;
            if !keep {
                continue;
            }

            result.push(MatchItem {
                combo: combo.iter().map(|t| t.name.clone()).collect(),
                min_star: ms,
                operators: satisfied
                    .iter()
                    .map(|o| OperatorRef {
                        name: o.name.clone(),
                        stars: o.stars,
                    })
                    .collect(),
                star_set: satisfied.iter().map(|o| o.stars).collect(),
                contains_pickup,
                pickup_target,
            });
        }
        result
    }
}

/// オペレーター参照（名前＋星）
#[derive(Debug, Clone)]
pub struct OperatorRef {
    pub name: String,
    pub stars: u8,
}

/// 1組み合わせ分の計算結果。
#[derive(Debug)]
pub struct MatchItem {
    pub combo: Vec<String>,
    pub min_star: u8,
    /// 星昇順ソート済み
    pub operators: Vec<OperatorRef>,
    /// 表示順を安定させるため BTreeSet（Python の出力は昇順）
    pub star_set: BTreeSet<u8>,
    pub contains_pickup: bool,
    pub pickup_target: Vec<String>,
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
