use serde::Deserialize;

/// data/recruitment/tagList.json に対応。タグ名を種別ごとに分類するために使う。
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

/// タグの種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagType {
    Elite,
    Job,
    Other, // position + other をまとめて扱う
}

/// 分類済みのタグ（名前＋種別）
#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub tag_type: TagType,
}
