//! 計算結果のDTO一式。整形(Discord embed化)は`bot/commands/operator_cost_calc`の責務。
//! アイテム列は`(日本語名, 個数)`の`Vec`で、既にPython版`normalize()`と同じ並び順
//! （龍門幣1000統合＋value_target順＋ε除去）になっている。

/// 消費素材1ブロック分（Python各所の「特化N/昇進N/合計」表示単位）。
pub struct ItemCostView {
    pub risei_value: f64,
    pub items: Vec<(String, f64)>,
}

pub struct SkillMasterCostDto {
    pub skill_name: String,
    pub skill_num: u32,
    pub description: String,
    /// 特化1〜3。
    pub masteries: Vec<ItemCostView>,
    pub total: ItemCostView,
    /// 合計の中級素材換算後アイテム列（このブロックには理性価値を表示しない。Python版と同じ）。
    pub total_r2_items: Vec<(String, f64)>,
    /// Python `星{star}スキル{nums}個中、第{index}位の消費です`。ランキング対象外(理性価値0以下)なら`None`。
    pub ranking_text: Option<String>,
}

pub struct EliteCostDto {
    pub operator_name: String,
    /// 昇進1,2。
    pub phases: Vec<ItemCostView>,
    pub total: ItemCostView,
    pub total_r2_items: Vec<(String, f64)>,
    /// 非昇格 & 星5/6のみ`Some`（Python版の掲載条件と同じ）。
    pub ranking_text: Option<String>,
}

pub struct ModulePhaseView {
    /// 1始まりのStage番号（表示は呼び出し側で"Stage.N"に整形する）。
    pub stage: u32,
    pub risei_value: f64,
    pub items: Vec<(String, f64)>,
}

pub struct ModuleEntryDto {
    /// モジュール種別名（大陸限定なら"(大陸版)"を付与済み）。
    pub header: String,
    pub phases: Vec<ModulePhaseView>,
    pub total_risei_value: f64,
    pub total_items: Vec<(String, f64)>,
    pub total_r2_items: Vec<(String, f64)>,
}

pub struct ModuleCostDto {
    pub operator_name: String,
    pub modules: Vec<ModuleEntryDto>,
}

/// Python `printCostRanking`の1行分。`rank`は絞り込み前の全体順位を保持する
/// （onlyRecentで一部除外しても番号は詰めない。Python版と同じ振る舞い）。
pub struct RankedEntry {
    pub rank: usize,
    pub name: String,
    pub risei_value: f64,
}

pub struct EliteRankingDto {
    pub star: u32,
    pub total_count: usize,
    /// onlyRecent・理性価値>EPSILON でフィルタ済み。
    pub entries: Vec<RankedEntry>,
}

/// Python `operatorCostList`の costofcnonly/costofglobal 共通形。
pub struct CostSummaryDto {
    /// costofcnonly のみ使用（未実装オペレーター一覧）。costofglobalは空。
    pub operator_names: Vec<String>,
    pub total_items: Vec<(String, f64)>,
    pub eq_items: Vec<(String, f64)>,
    /// (total+eq)の中級素材換算。表示は個数降順(sortByCount=True)。
    pub combined_r2_items: Vec<(String, f64)>,
    pub total_risei_value: f64,
}

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

pub struct MasterStatsRecentDto {
    pub star: u32,
    pub skill_nums: usize,
    pub entries: Vec<RankedEntry>,
}

pub enum MasterStatsDto {
    Full(MasterStatsFullDto),
    Recent(MasterStatsRecentDto),
}
