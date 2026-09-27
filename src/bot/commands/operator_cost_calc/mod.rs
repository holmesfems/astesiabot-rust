pub mod operatorcostlist;
pub mod operatorelitecost;
pub mod operatormastercost;
pub mod operatormodulecost;

use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{to_embed_batches, EmbedReply};
use crate::engine::operator_cost_calc::model::build_formula_map;
use crate::engine::operator_cost_calc::{AllOperatorsInfo, ValueSet};
use crate::engine::risei_calculator_engine::Server;

/// charmaterials.py側の`EPSILON = 1e-6`。個数表示の整数/小数丸め判定に使う
/// （`engine::operator_cost_calc::EPSILON`と同一値だが、こちらは表示整形専用として
/// コマンド層に置く。理性価値の閾値判定は既にengine層のランキング関数側で適用済み）。
const DISPLAY_EPSILON: f64 = 1e-6;

/// 4コマンド共通の計算コンテキストを構築する（Python版`OperatorCostsCalculator.operatorInfo`
/// + `CalculatorManager.getValues`相当）。`FormulaMap`はコマンド呼び出しごとに
/// `outer_source.formulas`のスナップショットから再構築する。
pub async fn build_context(state: &AppState) -> (AllOperatorsInfo, ValueSet) {
    let data = state.external_source.operator_data.get().await;
    let item_names = state.external_source.item_names.get().await;
    let skill_data = state.external_source.skill_data.get().await;
    let formulas_raw = state.external_source.formulas.get().await;
    let formulas = build_formula_map(&formulas_raw.formulas, &item_names);
    let info = AllOperatorsInfo {
        data,
        item_names,
        skill_data,
        formulas,
    };

    let global_snapshot = state.risei_calculator.snapshot(Server::Global, &state.external_source).await;
    let mainland_snapshot = state.risei_calculator.snapshot(Server::Mainland, &state.external_source).await;
    let values = ValueSet {
        global: global_snapshot.values.clone(),
        mainland: mainland_snapshot.values.clone(),
    };
    (info, values)
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
