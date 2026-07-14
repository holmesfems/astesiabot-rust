use super::model::{Operator, OperatorDb, Tag, TagList, TagType};
use super::Error;
use itertools::Itertools;
use std::collections::{BTreeSet, HashMap, HashSet};
use chrono::{NaiveDate, LocalResult, TimeZone, Utc};
use chrono_tz::Asia::Tokyo;
use std::env;

/// オペレーター参照（名前＋星）。キャッシュにも計算結果にも使う軽量表現。
#[derive(Debug, Clone)]
pub struct OperatorRef {
    pub name: String,
    pub stars: u8,
}

/// future オペレーターのキャッシュ内表現。実装時刻を持つ。
#[derive(Debug, Clone)]
struct FutureOperatorRef {
    op: OperatorRef,
    /// 実装時刻（Asia/Tokyo、指定 hour:00:00）
    release: chrono::DateTime<chrono_tz::Tz>,
}

/// タグ名から種別を判定（Python の tagType 相当）。
/// load() 時（self がまだ無い段階）でも使うためフリー関数にしてある。
fn tag_type_of(tag_list: &TagList, name: &str) -> TagType {
    if tag_list.elite_tags.iter().any(|t| t == name) {
        TagType::Elite
    } else if tag_list.job_tags.iter().any(|t| t == name) {
        TagType::Job
    } else {
        TagType::Other
    }
}

/// 起動時に一度読み込んで保持するデータ。
///
/// タグ組み合わせ→該当オペレーターのマップ（main_map / future_map）は起動時に1回だけ構築する
/// （Python 版の GlobalTagMap / FutureTagMap 相当）。future の実装時刻フィルタは、この
/// 事前計算済みマップを引いた後に、都度「今実装済みか」だけを軽くチェックする形にしているため、
/// 実装タイミングを監視して再計算するような仕組みは不要。
pub struct RecruitData {
    tag_list: TagList,
    /// tagList の全タグを定義順に並べたもの（入力の並べ替え・フィルタに使う）
    /// Python: tagNameList = jobTags + positionTags + eliteTags + otherTags
    tag_name_order: Vec<String>,
    /// タグ組み合わせ（定義順タプル）→ 満たす main オペレーター一覧。
    main_map: HashMap<Vec<String>, Vec<OperatorRef>>,
    /// 同上。future 分（実装時刻は問わず全件含む。時刻での絞り込みは calculate() 側で行う）。
    future_map: HashMap<Vec<String>, Vec<FutureOperatorRef>>,
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

        // configurable hour via env RECRUIT_FUTURE_HOUR (0-23), default 16
        let future_hour: u32 = env::var("RECRUIT_FUTURE_HOUR")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&h| h < 24)
            .unwrap_or(16);

        let mut future_releases: Vec<(chrono::DateTime<chrono_tz::Tz>, Vec<Operator>)> = Vec::new();
        for fe in db.future.into_iter() {
            match Self::parse_future_date(&fe.yyyymmdd, future_hour) {
                Some(release) => future_releases.push((release, fe.op_list)),
                None => {
                    eprintln!("warning: failed to parse future date '{}', skipping", fe.yyyymmdd);
                }
            }
        }

        // 全タグ（定義順）を Tag 化し、1〜3個の全組み合わせを起動時に1回だけ生成する。
        let all_tags: Vec<Tag> = tag_name_order
            .iter()
            .map(|name| Tag {
                name: name.clone(),
                tag_type: tag_type_of(&tag_list, name),
            })
            .collect();
        let mut all_combos: Vec<Vec<&Tag>> = Vec::new();
        for k in 1..=3 {
            for combo in all_tags.iter().combinations(k) {
                all_combos.push(combo);
            }
        }

        let mut main_map: HashMap<Vec<String>, Vec<OperatorRef>> = HashMap::new();
        let mut future_map: HashMap<Vec<String>, Vec<FutureOperatorRef>> = HashMap::new();
        for combo in &all_combos {
            let main_satisfied: Vec<OperatorRef> = db
                .main
                .iter()
                .filter(|op| satisfy_tags(op, combo))
                .map(|op| OperatorRef {
                    name: op.name.clone(),
                    stars: op.stars,
                })
                .collect();

            let mut future_satisfied: Vec<FutureOperatorRef> = Vec::new();
            for (release, operators) in &future_releases {
                for op in operators.iter().filter(|op| satisfy_tags(op, combo)) {
                    future_satisfied.push(FutureOperatorRef {
                        op: OperatorRef {
                            name: op.name.clone(),
                            stars: op.stars,
                        },
                        release: release.clone(),
                    });
                }
            }

            if main_satisfied.is_empty() && future_satisfied.is_empty() {
                continue;
            }
            let key: Vec<String> = combo.iter().map(|t| t.name.clone()).collect();
            if !main_satisfied.is_empty() {
                main_map.insert(key.clone(), main_satisfied);
            }
            if !future_satisfied.is_empty() {
                future_map.insert(key, future_satisfied);
            }
        }

        Ok(Self {
            tag_list,
            tag_name_order,
            main_map,
            future_map,
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
        tag_type_of(&self.tag_list, name)
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
    /// is_global=false なら未実装 future も無条件で対象に含める（Python の Mainland 相当）。
    /// pickup があれば、その対象を含む組み合わせを星に関わらず採用する。
    pub fn calculate(
        &self,
        input_tags: &[String],
        is_global: bool,
        min_star: u8,
        pickup: Option<&[String]>,
    ) -> Vec<MatchItem> {
        let tags = self.normalize_input(input_tags);

        // 入力タグ（最大8個程度）の1〜3個の組み合わせだけを都度生成する。
        // 事前計算済み main_map / future_map へのキーとして使う。
        let mut combos: Vec<Vec<&Tag>> = Vec::new();
        for k in 1..=3 {
            for combo in tags.iter().combinations(k) {
                combos.push(combo);
            }
        }

        // future は実装済みかどうかを現在時刻で都度判定する（キャッシュ自体は時刻に依存しない）。
        let now = Utc::now().with_timezone(&Tokyo);

        // minStar==4 のとき showRobot=true（Python の recruitDoProcess）
        let show_robot = min_star == 4;

        let mut result = Vec::new();
        for combo in &combos {
            let key: Vec<String> = combo.iter().map(|t| t.name.clone()).collect();

            let mut satisfied: Vec<OperatorRef> =
                self.main_map.get(&key).cloned().unwrap_or_default();
            if let Some(future_ops) = self.future_map.get(&key) {
                for fop in future_ops {
                    // 実装済み → main 相当（is_global に関わらず対象）
                    // 未実装 → is_global=false のときだけ対象（Python の Mainland 相当）
                    if now >= fop.release || !is_global {
                        satisfied.push(fop.op.clone());
                    }
                }
            }
            if satisfied.is_empty() {
                continue;
            }
            // 星昇順ソート（Python の OperatorList は stars 昇順）
            satisfied.sort_by_key(|o| o.stars);
            let ms = min_star_of(&satisfied);

            // ピックアップ判定
            let (contains_pickup, pickup_target) = match pickup {
                Some(pu) => {
                    let names: Vec<&str> = satisfied.iter().map(|o| o.name.as_str()).collect();
                    let pt: Vec<String> = pu
                        .iter()
                        .filter(|t| names.contains(&t.as_str()))
                        .cloned()
                        .collect();
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
                combo: key,
                min_star: ms,
                star_set: satisfied.iter().map(|o| o.stars).collect(),
                operators: satisfied,
                contains_pickup,
                pickup_target,
            });
        }
        result
    }
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
fn min_star_of(ops: &[OperatorRef]) -> u8 {
    let stars: HashSet<u8> = ops.iter().map(|o| o.stars).collect();
    let high: Vec<u8> = stars.iter().copied().filter(|&s| s >= 3).collect();
    if let Some(&m) = high.iter().min() {
        return m;
    }
    stars.iter().copied().max().unwrap_or(0)
}
