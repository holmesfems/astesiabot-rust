use serde::Deserialize;
use std::collections::BTreeMap;

/// 大陸版(CN)基準かグローバル版基準かの計算軸
/// （Python の `isGlobal:bool` 相当。bool blindness を避けるためenumにした）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    Global,
    Mainland,
}

/// 理性価値表の計算対象アイテム（中国語名）。順序はPython版 `listInfo.ValueTarget`
/// と同一（インデックスの意味は無いが、表示順の一致のため崩さない）。
pub const VALUE_TARGET: &[&str] = &[
    "基础作战记录",
    "初级作战记录",
    "中级作战记录",
    "高级作战记录",
    "赤金",
    "龙门币1000",
    "源岩",
    "固源岩",
    "固源岩组",
    "提纯源岩",
    "破损装置",
    "装置",
    "全新装置",
    "改量装置",
    "酯原料",
    "聚酸酯",
    "聚酸酯组",
    "聚酸酯块",
    "代糖",
    "糖",
    "糖组",
    "糖聚块",
    "异铁碎片",
    "异铁",
    "异铁组",
    "异铁块",
    "双酮",
    "酮凝集",
    "酮凝集组",
    "酮阵列",
    "扭转醇",
    "白马醇",
    "轻锰矿",
    "三水锰矿",
    "研磨石",
    "五水研磨石",
    "RMA70-12",
    "RMA70-24",
    "凝胶",
    "聚合凝胶",
    "炽合金",
    "炽合金块",
    "晶体元件",
    "晶体电路",
    "半自然溶剂",
    "精炼溶剂",
    "化合切削液",
    "切削原液",
    "转质盐组",
    "转质盐聚块",
    "褐素纤维",
    "固化纤维板",
    "环烃聚质",
    "环烃预制体",
    "类凝结核",
    "手性屈光体",
    "聚合剂",
    "双极纳米片",
    "D32钢",
    "晶体电子单元",
    "烧结核凝晶",
    "重相位对映体",
    "技巧概要·卷1",
    "技巧概要·卷2",
    "技巧概要·卷3",
];

/// 大陸先行の新素材（Mainland のみ計算対象に追加。Python `ValueTarget_new`）。
pub const VALUE_TARGET_NEW: &[&str] = &["电极单元", "聚能动力单元", "液化高能气体", "液化醚吸聚体"];

/// 初級資格証(te2list)の対象アイテム（Python `Item_rarity2`）。
pub const ITEM_RARITY2: &[&str] = &[
    "固源岩组",
    "全新装置",
    "聚酸酯组",
    "糖组",
    "异铁组",
    "酮凝集组",
    "扭转醇",
    "轻锰矿",
    "研磨石",
    "RMA70-12",
    "凝胶",
    "炽合金",
    "晶体元件",
    "半自然溶剂",
    "化合切削液",
    "转质盐组",
    "褐素纤维",
    "环烃聚质",
    "类凝结核",
];
pub const ITEM_RARITY2_NEW: &[&str] = &["电极单元", "液化高能气体"];

/// 上級資格証(te3list)の対象アイテム（Python `Item_rarity3`）。
pub const ITEM_RARITY3: &[&str] = &[
    "提纯源岩",
    "改量装置",
    "聚酸酯块",
    "糖聚块",
    "异铁块",
    "酮阵列",
    "白马醇",
    "三水锰矿",
    "五水研磨石",
    "RMA70-24",
    "聚合凝胶",
    "炽合金块",
    "晶体电路",
    "精炼溶剂",
    "切削原液",
    "转质盐聚块",
    "固化纤维板",
    "环烃预制体",
    "手性屈光体",
];
pub const ITEM_RARITY3_NEW: &[&str] = &["聚能动力单元", "液化醚吸聚体"];

/// 上級素材(Item_rarity4)。中級換算(`rare3and4ToRare2`)専用で、資格証効率表には出てこない
/// （Python `Item_rarity4`。`_new`側は空のため定数を用意していない）。
pub const ITEM_RARITY4: &[&str] = &["聚合剂", "双极纳米片", "D32钢", "晶体电子单元", "烧结核凝晶", "重相位对映体"];

/// 大陸版実装済み、グローバル版未実装のゾーン一覧。新章実装時にここへ追記し、
/// グローバル版で実装され次第削除する運用（Python版 `new_zone` と同じ役割）。
pub const NEW_ZONE_MAINLAND_ONLY: &[&str] = &["main_17"];

fn for_server<'a>(base: &'a [&'a str], new: &'a [&'a str], server: Server) -> Vec<&'a str> {
    let mut v: Vec<&str> = base.to_vec();
    if server == Server::Mainland {
        v.extend_from_slice(new);
    }
    v
}

pub fn value_target(server: Server) -> Vec<&'static str> {
    for_server(VALUE_TARGET, VALUE_TARGET_NEW, server)
}

pub fn item_rarity2(server: Server) -> Vec<&'static str> {
    for_server(ITEM_RARITY2, ITEM_RARITY2_NEW, server)
}

pub fn item_rarity3(server: Server) -> Vec<&'static str> {
    for_server(ITEM_RARITY3, ITEM_RARITY3_NEW, server)
}

/// 上級素材一覧（`_new`側が無いためサーバに関わらず同一。Python `getItemRarity4`）。
pub fn item_rarity4(_server: Server) -> Vec<&'static str> {
    ITEM_RARITY4.to_vec()
}

/// `data/risei/stage_category.json` の1カテゴリ分（Python `StageCategoryInfo`）。
#[derive(Debug, Clone, Deserialize)]
pub struct StageCategoryInfo {
    #[serde(rename = "Stages")]
    pub stages: Vec<String>,
    #[serde(rename = "Items")]
    pub items: Vec<String>,
    #[serde(rename = "MainItem")]
    pub main_item: String,
    pub to_ja: String,
    #[serde(rename = "SubItem", default)]
    pub sub_item: Vec<String>,
    #[serde(rename = "SubOrder", default)]
    pub sub_order: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct StageCategoryFile {
    pub main: BTreeMap<String, StageCategoryInfo>,
    pub new: BTreeMap<String, StageCategoryInfo>,
}

pub const STAGE_CATEGORY_PATH: &str = "data/risei/stage_category.json";

pub fn load_stage_category_file() -> Result<StageCategoryFile, crate::engine::risei_calculator_engine::Error> {
    let s = std::fs::read_to_string(STAGE_CATEGORY_PATH)?;
    Ok(serde_json::from_str(&s)?)
}

/// サーバに応じたカテゴリ辞書（Python `getStageCategoryDict`。Mainlandは
/// `new`カテゴリも含む）。
pub fn stage_category_dict(
    file: &StageCategoryFile,
    server: Server,
) -> BTreeMap<String, StageCategoryInfo> {
    let mut dict = file.main.clone();
    if server == Server::Mainland {
        dict.extend(file.new.clone());
    }
    dict
}
