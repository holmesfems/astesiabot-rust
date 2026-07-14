use serde::Deserialize;

/// オペレーター1体分。recruitmentOperators.json の要素に対応。
#[derive(Debug, Clone, Deserialize)]
pub struct Operator {
    pub name: String,
    pub job: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub stars: u8,
}

/// recruitmentOperators.json 全体。
/// JSON 側には歴史的経緯で "new" フィールドが残っているが、実データは常に空で
/// 将来実装予定は "future" が一元管理するため、Rust 側では読み込まない。
#[derive(Debug, Deserialize)]
pub struct OperatorDb {
    #[serde(default)]
    pub main: Vec<Operator>,
    #[serde(default)]
    pub future: Vec<FutureEntry>,
}

/// `future` 要素。data/recruitmentOperators.json の形式に合わせる。
#[derive(Debug, Deserialize)]
pub struct FutureEntry {
    // human-readable label for future entries; not used by code
    #[serde(default)]
    #[allow(dead_code)]
    pub comment: Option<String>,
    #[serde(rename = "yyyymmdd")]
    pub yyyymmdd: String,
    #[serde(rename = "opList")]
    pub op_list: Vec<Operator>,
}

/// tagList.json に対応。タグ名を種別ごとに分類するために使う。
#[derive(Debug, Deserialize)]
pub struct TagList {
    #[serde(rename = "eliteTags")]
    pub elite_tags: Vec<String>,
    #[serde(rename = "jobTags")]
    pub job_tags: Vec<String>,
    #[serde(rename = "positionTags")]
    pub position_tags: Vec<String>,
    #[serde(rename = "otherTags")]
    pub other_tags: Vec<String>,
}

/// タグの種別。Python の tagType 文字列を enum にしたもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagType {
    Elite,
    Job,
    Other, // position + other をまとめて扱う（Python と同じ）
}

/// 分類済みのタグ（名前＋種別）
#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub tag_type: TagType,
}
