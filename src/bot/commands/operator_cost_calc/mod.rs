pub mod operatorcostlist;
pub mod operatorelitecost;
pub mod operatormastercost;
pub mod operatormodulecost;

use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{to_embed_batches, EmbedReply};
use crate::engine::operator_cost_calc::dto::RankingPosition;
use crate::engine::operator_cost_calc::{AllOperatorsInfo, ValueSet};

/// charmaterials.py側の`EPSILON = 1e-6`。個数表示の整数/小数丸め判定に使う
/// （`engine::operator_cost_calc::EPSILON`と同一値だが、こちらは表示整形専用として
/// コマンド層に置く。理性価値の閾値判定は既にengine層のランキング関数側で適用済み）。
const DISPLAY_EPSILON: f64 = 1e-6;

/// 4コマンド共通の計算コンテキストを構築する。実体は`AllOperatorsInfo::snapshot`（REST APIと共有）。
pub async fn build_context(state: &AppState) -> (AllOperatorsInfo, ValueSet) {
    AllOperatorsInfo::snapshot(&state.external_source, &state.risei_calculator).await
}

/// Python `星{star}スキル{nums}個中、第{index}位の消費です`。
pub fn mastery_ranking_text(ranking: &RankingPosition) -> String {
    format!("星{}スキル{}個中、第{}位の消費です", ranking.star, ranking.total, ranking.rank)
}

/// 昇進素材のランキング文言（Python版と同じ）。
pub fn elite_ranking_text(ranking: &RankingPosition) -> String {
    format!("星{}オペレーター{}名中、第{}位の消費です", ranking.star, ranking.total, ranking.rank)
}

/// EmbedReply をスラッシュコマンドの応答として送信する（`bot/commands/risei/mod.rs`の
/// `send_reply`と同じ役割。feature間の結合を避けるためここに複製している）。
pub async fn send_reply(ctx: Context<'_>, reply: EmbedReply) -> Result<(), Error> {
    for batch in to_embed_batches(&reply) {
        let mut created = poise::CreateReply::default();
        created.embeds = batch;
        ctx.send(created).await?;
    }
    Ok(())
}

/// Python `dumpToPrint`(header無し版)。```で囲んだコードブロックにする。
pub fn dump_to_print(lines: &[String]) -> String {
    format!("```\n{}```", lines.join("\n"))
}

/// Python `"{0} × {1:d}".format(...)`/`"{0} × {1:.3f}".format(...)`の丸め判定込み表示。
fn fmt_item_line(name: &str, count: f64) -> String {
    let rounded = count.round();
    if (count - rounded).abs() < DISPLAY_EPSILON {
        format!("{name} × {}", rounded as i64)
    } else {
        format!("{name} × {count:.3}")
    }
}

/// Python `ItemCost.toStrBlock(sortByCount)`。`sort_by_count`時は個数降順に安定並べ替え
/// （タイは元の`value_target`順を保つ）。
pub fn fmt_item_block(items: &[(String, f64)], sort_by_count: bool) -> String {
    let mut items = items.to_vec();
    if sort_by_count {
        items.sort_by(|a, b| b.1.total_cmp(&a.1));
    }
    let lines: Vec<String> = items.iter().map(|(name, count)| fmt_item_line(name, *count)).collect();
    dump_to_print(&lines)
}
