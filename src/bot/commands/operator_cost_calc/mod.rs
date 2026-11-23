pub mod operatorcostlist;
pub mod operatorelitecost;
pub mod operatormastercost;
pub mod operatormodulecost;

use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{to_embed_batches, EmbedReply};
use crate::engine::operator_cost_calc::dto::RankingPosition;
use crate::engine::operator_cost_calc::{AllOperatorsInfo, ValueSet};

/// 個数表示の整数/小数丸め判定に使う
/// （`engine::operator_cost_calc::EPSILON`と同一値だが、こちらは表示整形専用として
/// コマンド層に置く。理性価値の閾値判定は既にengine層のランキング関数側で適用済み）。
const DISPLAY_EPSILON: f64 = 1e-6;

/// 4コマンド共通の計算コンテキストを構築する。実体は`AllOperatorsInfo::snapshot`（REST APIと共有）。
pub async fn build_context(state: &AppState) -> (AllOperatorsInfo, ValueSet) {
    AllOperatorsInfo::snapshot(&state.external_source, &state.risei_calculator).await
}

/// 特化コストのランキング文言。
pub fn mastery_ranking_text(ranking: &RankingPosition) -> String {
    format!(
        "星{}スキル{}個中、第{}位の消費です",
        ranking.star, ranking.total, ranking.rank
    )
}

/// 昇進素材のランキング文言。
pub fn elite_ranking_text(ranking: &RankingPosition) -> String {
    format!(
        "星{}オペレーター{}名中、第{}位の消費です",
        ranking.star, ranking.total, ranking.rank
    )
}

/// EmbedReply をスラッシュコマンドの応答として送信する（`bot/commands/risei/mod.rs`の
/// `send_reply`と同じ役割。feature間の結合を避けるためここに複製している）。
pub async fn send_reply(ctx: Context<'_>, reply: EmbedReply) -> Result<(), Error> {
    for batch in to_embed_batches(&reply) {
        ctx.send(poise::CreateReply {
            embeds: batch,
            ..Default::default()
        })
        .await?;
    }
    Ok(())
}

/// ```で囲んだコードブロックにする。
pub fn dump_to_print(lines: &[String]) -> String {
    format!("```\n{}```", lines.join("\n"))
}

/// 整数なら小数点なし、そうでなければ小数点以下3桁で表示する（整数かどうかは`DISPLAY_EPSILON`で判定）。
fn fmt_item_line(name: &str, count: f64) -> String {
    let rounded = count.round();
    if (count - rounded).abs() < DISPLAY_EPSILON {
        format!("{name} × {}", rounded as i64)
    } else {
        format!("{name} × {count:.3}")
    }
}

/// 素材を```のコードブロックに整形する。`sort_by_count`時は個数降順。
pub fn fmt_item_block(items: &[(String, f64)], sort_by_count: bool) -> String {
    let mut items = items.to_vec();
    if sort_by_count {
        items.sort_by(|a, b| b.1.total_cmp(&a.1));
    }
    let lines: Vec<String> = items
        .iter()
        .map(|(name, count)| fmt_item_line(name, *count))
        .collect();
    dump_to_print(&lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<(String, f64)> {
        [("a", 1.0), ("b", 3.0), ("c", 3.0), ("d", 2.0)]
            .iter()
            .map(|(n, c)| (n.to_string(), *c))
            .collect()
    }

    #[test]
    fn item_block_sorts_by_count_desc_keeping_tie_order() {
        assert_eq!(
            fmt_item_block(&items(), true),
            "```\nb × 3\nc × 3\nd × 2\na × 1```"
        );
    }

    #[test]
    fn item_block_keeps_input_order_when_not_sorting() {
        assert_eq!(
            fmt_item_block(&items(), false),
            "```\na × 1\nb × 3\nc × 3\nd × 2```"
        );
    }

    #[test]
    fn item_line_shows_integer_within_epsilon_else_three_decimals() {
        assert_eq!(fmt_item_line("x", 3.0), "x × 3");
        assert_eq!(fmt_item_line("x", 3.0000004), "x × 3");
        assert_eq!(fmt_item_line("x", 2.5), "x × 2.500");
        assert_eq!(fmt_item_line("x", 3.00001), "x × 3.000");
    }

    #[test]
    fn snapshot_ranking_texts() {
        let pos = RankingPosition {
            star: 6,
            rank: 12,
            total: 87,
        };
        insta::assert_snapshot!(
            "operator_cost_ranking_texts",
            format!(
                "{}\n{}",
                mastery_ranking_text(&pos),
                elite_ranking_text(&pos)
            )
        );
    }
}
