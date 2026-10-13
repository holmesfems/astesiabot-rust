//! オペレーターの誕生日（bot の誕生日お祝い用）。
//!
//! CN の `handbook_info_table.json`（プロフィール）の「基础档案」にある `【生日】M月D日`
//! （ロボットは `【出厂日】`）を読む。JP未実装のオペレーターもお祝いするためCNを使う
//! （CN/JPの両方にいるオペレーターの日付は全員一致することを確認済み）。
//!
//! - 日付は先頭の`M月D日`だけ読み、後ろの注記は無視する（例: GALLUS²の
//!   `5月4日（由凯尔希根据其骨骼样本推算）`）。「未公开」等の日付が無いオペレーターは対象外。
//! - 入手不可（`isNotObtainable`）でもプロフィールに日付があれば対象にする（予備隊員）。
//! - 異格（`char_meta_table.json`の`spCharGroups`）は、元のオペレーターと同じ日付なら
//!   元のオペレーター1件にまとめる。元に日付が無い場合（凯尔希·思衡托・烛煌）は異格側を残す。
//! - 誕生日を1件も読めなければfetch失敗にする（プロフィールの書式が変わった時に
//!   全員の誕生日が消えるのを防ぐ。失敗なら`Source`が直前の値/Seedを使い続ける）。

use super::cache::write_seed_file;
use super::http::{client, fetch_typed_with_retry};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

const HANDBOOK_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/handbook_info_table.json";
const CHAR_META_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/char_meta_table.json";
const CHAR_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/character_table.json";
const CHAR_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/character_table.json";

/// Seedの保存先。`cargo run --bin regen_seeds` で手動生成し、git commitして
/// リポジトリに含めておく（起動時fetchが失敗した場合のフォールバック用）。
pub const SEED_PATH: &str = "data/seed/operator_birthdays.json";

/// JP未実装オペレーターの仮訳（operator_data / recruit_pool と同じファイル）。
const CUSTOM_OPERATOR_ZH_TO_JA_YAML: &str = include_str!("../../../data/customOperatorZhToJa.yaml");

/// 誕生日が載っているプロフィールの節の題名と、日付の項目名。
const BASIC_PROFILE_TITLE: &str = "基础档案";
const DATE_LABELS: &[&str] = &["【生日】", "【出厂日】"];

/// オペレーターの誕生日一覧。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct OperatorBirthdays {
    /// (月, 日, char_id) 順。
    pub operators: Vec<BirthdayOperator>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BirthdayOperator {
    pub char_id: String,
    /// 表示名。JPのcharacter_table → customOperatorZhToJa.yaml → CN名 の順で決める。
    pub name: String,
    pub cn_name: String,
    pub month: u8,
    pub day: u8,
}

impl OperatorBirthdays {
    /// 指定日が誕生日のオペレーター（char_id順）。
    pub fn on(&self, month: u32, day: u32) -> impl Iterator<Item = &BirthdayOperator> {
        self.operators
            .iter()
            .filter(move |o| u32::from(o.month) == month && u32::from(o.day) == day)
    }
}

pub fn fetch() -> BoxFuture<'static, Result<OperatorBirthdays, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

#[derive(Deserialize)]
struct RawHandbookTable {
    #[serde(rename = "handbookDict")]
    handbook_dict: HashMap<String, RawHandbook>,
}

#[derive(Deserialize)]
struct RawHandbook {
    #[serde(default, rename = "storyTextAudio")]
    story_text_audio: Vec<RawStorySection>,
}

#[derive(Deserialize)]
struct RawStorySection {
    #[serde(default, rename = "storyTitle")]
    story_title: String,
    #[serde(default)]
    stories: Vec<RawStory>,
}

#[derive(Deserialize)]
struct RawStory {
    #[serde(default, rename = "storyText")]
    story_text: String,
}

#[derive(Deserialize)]
struct RawCharMeta {
    /// 元のオペレーターのID → そのオペレーター自身と異格のID一覧。
    #[serde(rename = "spCharGroups")]
    sp_char_groups: HashMap<String, Vec<String>>,
}

/// character_table.jsonの1エントリのうち、ここで使うフィールドだけ。
#[derive(Deserialize)]
struct RawChar {
    #[serde(default)]
    name: String,
}

type CharTable = HashMap<String, RawChar>;

async fn fetch_impl() -> Result<OperatorBirthdays, FetchError> {
    let client = client();
    let (handbook, meta, cn_chars, jp_chars) = tokio::try_join!(
        fetch_typed_with_retry::<RawHandbookTable>(&client, HANDBOOK_URL_CN),
        fetch_typed_with_retry::<RawCharMeta>(&client, CHAR_META_URL_CN),
        fetch_typed_with_retry::<CharTable>(&client, CHAR_TABLE_URL_CN),
        fetch_typed_with_retry::<CharTable>(&client, CHAR_TABLE_URL_JP),
    )?;
    let custom_names: Option<HashMap<String, String>> = serde_yaml::from_str(CUSTOM_OPERATOR_ZH_TO_JA_YAML)?;
    build(&handbook, &meta, &cn_chars, &jp_chars, &custom_names.unwrap_or_default())
}

/// プロフィール本文から`【生日】`/`【出厂日】`の日付（月, 日）を読む。先頭の`M月D日`だけを見て、
/// 後ろの注記は無視する。日付が無い（「未公开」等）・項目が無い場合はNone。
fn parse_birthday(text: &str) -> Option<(u8, u8)> {
    let value = DATE_LABELS.iter().find_map(|label| {
        let start = text.find(label)? + label.len();
        Some(text[start..].lines().next().unwrap_or("").trim())
    })?;
    let (month, rest) = value.split_once('月')?;
    let (day, _) = rest.split_once('日')?;
    let month: u8 = month.trim().parse().ok()?;
    let day: u8 = day.trim().parse().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some((month, day))
}

fn birthday_of(handbook: &RawHandbookTable, char_id: &str) -> Option<(u8, u8)> {
    handbook
        .handbook_dict
        .get(char_id)?
        .story_text_audio
        .iter()
        .find(|s| s.story_title == BASIC_PROFILE_TITLE)?
        .stories
        .iter()
        .find_map(|s| parse_birthday(&s.story_text))
}

fn build(
    handbook: &RawHandbookTable,
    meta: &RawCharMeta,
    cn_chars: &CharTable,
    jp_chars: &CharTable,
    custom_names: &HashMap<String, String>,
) -> Result<OperatorBirthdays, FetchError> {
    // 異格のID → 元のオペレーターのID。
    let base_of: HashMap<&str, &str> = meta
        .sp_char_groups
        .iter()
        .flat_map(|(base, ids)| ids.iter().filter(move |id| *id != base).map(move |id| (id.as_str(), base.as_str())))
        .collect();

    // char_id順に並べて処理する（同じ日の並び順とSeedの差分を安定させるため）。
    let chars: BTreeMap<&str, &RawChar> = cn_chars
        .iter()
        .filter(|(id, _)| id.starts_with("char_"))
        .map(|(id, c)| (id.as_str(), c))
        .collect();

    let mut seen: HashSet<(String, u8, u8)> = HashSet::new();
    let mut operators = Vec::new();
    for (&char_id, cn) in &chars {
        let Some((month, day)) = birthday_of(handbook, char_id) else {
            continue;
        };
        // 元のオペレーターと同じ日付の異格は、元のオペレーター1件にまとめる。
        if let Some(base) = base_of.get(char_id) {
            if birthday_of(handbook, base) == Some((month, day)) {
                continue;
            }
        }
        let cn_name = cn.name.clone();
        let name = match jp_chars.get(char_id) {
            Some(jp) => jp.name.clone(),
            None => custom_names.get(&cn_name).cloned().unwrap_or_else(|| cn_name.clone()),
        };
        // 同名・同日の重複（予備隊員のようにIDが複数あるもの）は1件にする。
        if !seen.insert((name.clone(), month, day)) {
            continue;
        }
        operators.push(BirthdayOperator {
            char_id: char_id.to_string(),
            name,
            cn_name,
            month,
            day,
        });
    }

    if operators.is_empty() {
        return Err("handbook_info_tableから誕生日を1件も読み取れませんでした（プロフィールの書式が変わった可能性）".into());
    }
    operators.sort_by(|a, b| (a.month, a.day, &a.char_id).cmp(&(b.month, b.day, &b.char_id)));
    Ok(OperatorBirthdays { operators })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_birthday_and_factory_date() {
        let profile = "【代号】阿米娅\n【性别】女\n【出身地】雷姆必拓\n【生日】12月23日\n【种族】卡特斯/奇美拉";
        assert_eq!(parse_birthday(profile), Some((12, 23)));
        // ロボットは「出厂日」。「出厂时间」（稼働期間）とは別の項目。
        let robot = "【出厂时间】两年\n【出厂日】5月12日\n【制造商】莱茵生命";
        assert_eq!(parse_birthday(robot), Some((5, 12)));
    }

    #[test]
    fn ignores_annotation_after_date() {
        assert_eq!(parse_birthday("【生日】5月4日（由凯尔希根据其骨骼样本推算）\n"), Some((5, 4)));
    }

    #[test]
    fn no_date_values_are_skipped() {
        for value in ["未公开", "未知", "本人表示遗忘", "未录入", "本人表示无确切日期"] {
            assert_eq!(parse_birthday(&format!("【生日】{value}\n")), None, "{value}");
        }
        // イフリータのプロフィールは演出で文字化けしていて、項目名自体が崩れている。
        assert_eq!(parse_birthday("【生日V 扙諴尙\n瓁誯Z锑b■苆兹\n【身高】1歸 #掅3"), None);
        assert_eq!(parse_birthday("【代号】W"), None);
    }

    fn handbook(entries: &[(&str, &str)]) -> RawHandbookTable {
        RawHandbookTable {
            handbook_dict: entries
                .iter()
                .map(|(id, text)| {
                    (
                        id.to_string(),
                        RawHandbook {
                            story_text_audio: vec![RawStorySection {
                                story_title: BASIC_PROFILE_TITLE.to_string(),
                                stories: vec![RawStory { story_text: text.to_string() }],
                            }],
                        },
                    )
                })
                .collect(),
        }
    }

    fn chars(entries: &[(&str, &str)]) -> CharTable {
        entries.iter().map(|(id, name)| (id.to_string(), RawChar { name: name.to_string() })).collect()
    }

    #[test]
    fn merges_alters_with_same_date_but_keeps_alter_when_base_has_no_date() {
        let hb = handbook(&[
            ("char_130_doberm", "【生日】8月30日"),
            ("char_1030_noirc2", "【生日】8月30日"), // 元と同じ日付の異格 → まとめる
            ("char_003_kalts", "【生日】未公开"),
            ("char_1043_kalts2", "【生日】1月23日"), // 元に日付が無い異格 → 残す
            ("char_4227_gallus", "【生日】5月4日（由凯尔希根据其骨骼样本推算）"),
        ]);
        let meta = RawCharMeta {
            sp_char_groups: HashMap::from([
                ("char_130_doberm".to_string(), vec!["char_130_doberm".to_string(), "char_1030_noirc2".to_string()]),
                ("char_003_kalts".to_string(), vec!["char_003_kalts".to_string(), "char_1043_kalts2".to_string()]),
            ]),
        };
        let cn = chars(&[
            ("char_130_doberm", "黑角"),
            ("char_1030_noirc2", "火龙S黑角"),
            ("char_003_kalts", "凯尔希"),
            ("char_1043_kalts2", "凯尔希·思衡托"),
            ("char_4227_gallus", "GALLUS²"),
            ("token_10000_x", "召唤物"), // char_以外は対象外
        ]);
        let jp = chars(&[("char_130_doberm", "ノイルホーン"), ("char_003_kalts", "ケルシー")]);
        let custom = HashMap::from([("GALLUS²".to_string(), "GALLUS²".to_string())]);

        let data = build(&hb, &meta, &cn, &jp, &custom).expect("should build");
        let got: Vec<(&str, &str, u8, u8)> = data
            .operators
            .iter()
            .map(|o| (o.name.as_str(), o.cn_name.as_str(), o.month, o.day))
            .collect();
        assert_eq!(
            got,
            vec![
                ("凯尔希·思衡托", "凯尔希·思衡托", 1, 23),
                ("GALLUS²", "GALLUS²", 5, 4),
                ("ノイルホーン", "黑角", 8, 30),
            ]
        );
        assert_eq!(data.on(8, 30).map(|o| o.name.as_str()).collect::<Vec<_>>(), vec!["ノイルホーン"]);
        assert_eq!(data.on(8, 31).count(), 0);
    }

    #[test]
    fn zero_birthdays_fails() {
        let hb = handbook(&[("char_002_amiya", "【生日】未公开")]);
        let meta = RawCharMeta { sp_char_groups: HashMap::new() };
        let cn = chars(&[("char_002_amiya", "阿米娅")]);
        assert!(build(&hb, &meta, &cn, &CharTable::new(), &HashMap::new()).is_err());
    }

    /// 実ネットワークでの疎通確認。`cargo test -- --ignored` で明示実行する。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_birthdays() {
        let data = fetch().await.expect("fetch should succeed against real network");
        assert!(data.operators.len() > 300, "too few birthdays: {}", data.operators.len());
        println!("{}件 / 1月1日: {:?}", data.operators.len(), data.on(1, 1).map(|o| &o.name).collect::<Vec<_>>());
    }
}
