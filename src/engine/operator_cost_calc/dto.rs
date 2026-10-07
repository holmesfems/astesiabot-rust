//! 計算結果のDTO一式。整形(Discord embed化)は`bot/commands/operator_cost_calc`の責務。
//! アイテム列は`(日本語名, 個数)`の`Vec`で、既にPython版`normalize()`と同じ並び順
//! （龍門幣1000統合＋value_target順＋ε除去）になっている。
//!
//! 全DTOは`Serialize`(REST API `/api/risei/operators/*`がそのままJSONで返す)。アイテム列の
//! `Vec<(String, f64)>`は`[{"name","count"}]`にシリアライズする(Rustの型はタプルのまま)。

use serde::{Serialize, Serializer};

/// `Vec<(名前, 個数)>`を`[{"name": .., "count": ..}]`として出す`serialize_with`用ヘルパー。
fn serialize_items<S: Serializer>(items: &[(String, f64)], serializer: S) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Item<'a> {
        name: &'a str,
        count: f64,
    }
    serializer.collect_seq(items.iter().map(|(name, count)| Item { name, count: *count }))
}

/// ランキング内での位置。表示文言は呼び出し側(bot/API)が決める。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RankingPosition {
    pub star: u32,
    /// 1始まりの順位。
    pub rank: usize,
    /// 母数（ランキング対象の総数。スキル数またはオペレーター数）。
    pub total: usize,
}

/// 計算関数のエラー。`Display`はbotがそのまま表示する文言。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorCostError {
    /// 名前に該当するオペレーターがいない（API側は`did_you_mean`付きの404にする）。
    OperatorNotFound(String),
    /// それ以外（スキル番号範囲外・★3特化なし・モジュールなし等）。
    Other(String),
}

impl std::fmt::Display for OperatorCostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OperatorNotFound(msg) | Self::Other(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for OperatorCostError {}

/// 消費素材1ブロック分（Python各所の「特化N/昇進N/合計」表示単位）。
#[derive(Serialize)]
pub struct ItemCostView {
    pub risei_value: f64,
    #[serde(serialize_with = "serialize_items")]
    pub items: Vec<(String, f64)>,
}

#[derive(Serialize)]
pub struct SkillMasterCostDto {
    pub operator_name: String,
    /// 大陸版限定オペレーターか（trueなら大陸版の理性価値で換算している）。
    pub cn_only: bool,
    pub skill_name: String,
    pub skill_num: u32,
    pub description: String,
    /// 特化1〜3。
    pub masteries: Vec<ItemCostView>,
    pub total: ItemCostView,
    /// 合計の中級素材換算後アイテム列（このブロックには理性価値を表示しない。Python版と同じ）。
    #[serde(serialize_with = "serialize_items")]
    pub total_r2_items: Vec<(String, f64)>,
    /// 同じ星のスキル特化ランキング内の位置。ランキング対象外(理性価値0以下)なら`None`。
    pub ranking: Option<RankingPosition>,
}

#[derive(Serialize)]
pub struct EliteCostDto {
    pub operator_name: String,
    pub cn_only: bool,
    /// 昇進1,2。
    pub phases: Vec<ItemCostView>,
    pub total: ItemCostView,
    #[serde(serialize_with = "serialize_items")]
    pub total_r2_items: Vec<(String, f64)>,
    /// 非昇格 & 星5/6のみ`Some`（Python版の掲載条件と同じ）。
    pub ranking: Option<RankingPosition>,
}

#[derive(Serialize)]
pub struct ModulePhaseView {
    /// 1始まりのStage番号（表示は呼び出し側で"Stage.N"に整形する）。
    pub stage: u32,
    pub risei_value: f64,
    #[serde(serialize_with = "serialize_items")]
    pub items: Vec<(String, f64)>,
}

#[derive(Serialize)]
pub struct ModuleEntryDto {
    /// モジュール種別名（大陸限定なら"(大陸版)"を付与済み）。
    pub header: String,
    /// このモジュールが大陸版限定か（trueなら大陸版の理性価値で換算している）。
    pub cn_only: bool,
    pub phases: Vec<ModulePhaseView>,
    pub total_risei_value: f64,
    #[serde(serialize_with = "serialize_items")]
    pub total_items: Vec<(String, f64)>,
    #[serde(serialize_with = "serialize_items")]
    pub total_r2_items: Vec<(String, f64)>,
}

#[derive(Serialize)]
pub struct ModuleCostDto {
    pub operator_name: String,
    /// オペレーター自身が大陸版限定か（換算サーバはモジュールごと。`ModuleEntryDto.cn_only`）。
    pub cn_only: bool,
    pub modules: Vec<ModuleEntryDto>,
}

/// Python `printCostRanking`の1行分。`rank`は絞り込み前の全体順位を保持する
/// （onlyRecentで一部除外しても番号は詰めない。Python版と同じ振る舞い）。
#[derive(Serialize)]
pub struct RankedEntry {
    pub rank: usize,
    pub name: String,
    pub risei_value: f64,
}

#[derive(Serialize)]
pub struct EliteRankingDto {
    pub star: u32,
    pub total_count: usize,
    /// onlyRecent・理性価値>EPSILON でフィルタ済み。
    pub entries: Vec<RankedEntry>,
}

/// Python `operatorCostList`の costofcnonly/costofglobal 共通形。
#[derive(Serialize)]
pub struct CostSummaryDto {
    /// costofcnonly のみ使用（未実装オペレーター一覧）。costofglobalは空。
    pub operator_names: Vec<String>,
    #[serde(serialize_with = "serialize_items")]
    pub total_items: Vec<(String, f64)>,
    #[serde(serialize_with = "serialize_items")]
    pub eq_items: Vec<(String, f64)>,
    /// (total+eq)の中級素材換算。表示は個数降順(sortByCount=True)。
    #[serde(serialize_with = "serialize_items")]
    pub combined_r2_items: Vec<(String, f64)>,
    pub total_risei_value: f64,
}

#[derive(Serialize)]
pub struct MasterStatsFullDto {
    pub star: u32,
    pub skill_nums: usize,
    pub heaviest_name: String,
    pub heaviest: ItemCostView,
    /// 上位10件（理性価値降順。skillNums<10なら全件で、lightest上位10件と内容が重複し得る。Python版と同じ）。
    pub top10_heaviest: Vec<RankedEntry>,
    pub lightest_name: String,
    pub lightest: ItemCostView,
    pub top10_lightest: Vec<RankedEntry>,
    pub average_risei: f64,
}

#[derive(Serialize)]
pub struct MasterStatsRecentDto {
    pub star: u32,
    pub skill_nums: usize,
    pub entries: Vec<RankedEntry>,
}

#[derive(Serialize)]
/// `mode`タグ付きで出す（`{"mode":"full",...}` / `{"mode":"recent",...}`）。
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum MasterStatsDto {
    Full(MasterStatsFullDto),
    Recent(MasterStatsRecentDto),
}
