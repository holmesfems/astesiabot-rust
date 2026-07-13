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

/// recruitmentOperators.json 全体。今は main のみ使用。
#[derive(Debug, Deserialize)]
pub struct OperatorDb {
    #[serde(default)]
    pub main: Vec<Operator>,
    #[serde(default)]
    pub new: Vec<Operator>,
    // future は後回し（beginFrom の時刻計算が絡むため）
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