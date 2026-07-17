use super::{build_reference_block, build_stage_export_row, fmt_percent, fmt_value, send_reply_with_attachment, server_from_bool};
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::bot::utils::xlsx::build_stage_export_xlsx;
use crate::engine::risei_calculator_engine::server::stage_category_dict;
use poise::serenity_prelude as serenity;

const MAX_ITEMS: usize = 15;
const MATERIALS_XLSX_FILENAME: &str = "MaterialsDrop.xlsx";

/// riseimaterials/riseicalculator の target_item 選択肢(Python版は常に大陸版の
/// 全カテゴリ(main+new)を選択肢に使う)。
async fn autocomplete_target_item(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let stage_category = ctx.data().state.risei_calculator.stage_category();
    stage_category
        .main
        .iter()
        .chain(stage_category.new.iter())
        .map(|(key, info)| (info.to_ja.clone(), key.clone()))
        .filter(|(name, _)| name.contains(partial))
        .take(25)
        .map(|(name, key)| serenity::AutocompleteChoice::new(name, key))
        .collect()
}

/// 昇進素材の効率の良い恒常ステージを調べます。
#[poise::command(slash_command)]
pub async fn riseimaterials(
    ctx: Context<'_>,
    #[description = "昇進素材カテゴリを選択"]
    #[autocomplete = "autocomplete_target_item"]
    target_item: String,
    #[description = "True:グローバル版基準の計算(デフォルト)、False:大陸版の新ステージと新素材を入れた計算"]
    is_global: Option<bool>,
    #[description = "true:ドロップ率データをxlsxで添付"]
    csv_file: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let server = server_from_bool(is_global.unwrap_or(true));
    let state = ctx.data().state.clone();

    let (reply, attachment) = match state
        .risei_calculator
        .material_search(&state.outer_source, server, &target_item)
        .await
    {
        Err(msg) => (EmbedReply::error(&msg), None),
        Ok(result) => {
            let mut chunks = vec![format!(
                "{}: 理性価値(中級)={}±{}\n",
                result.category_ja,
                fmt_value(result.main_item_value),
                fmt_value(result.main_item_std_dev)
            )];
            for stage in result.stages.iter().take(MAX_ITEMS) {
                let mut lines = vec![
                    format!("マップ名       : {}", stage.name),
                    format!("理性効率       : {}", fmt_percent(stage.efficiency)),
                    format!("理性消費       : {}", stage.sanity_cost),
                ];
                if let Some(time_cost) = stage.time_cost {
                    lines.push(format!("時間消費(倍速) : {time_cost:.2}"));
                }
                if let Some(drop_per_minute) = stage.drop_per_minute {
                    lines.push(format!("分入手数(中級) : {drop_per_minute:.2}"));
                }
                lines.push(format!(
                    "主素材効率     : {}",
                    fmt_percent(stage.main_item_efficiency)
                ));
                lines.push(format!(
                    "99%信頼区間(3σ): {}",
                    fmt_percent(stage.confidence_3sigma)
                ));
                lines.push(format!(
                    "昇進効率       : {}",
                    fmt_percent(stage.promotion_efficiency)
                ));
                lines.push(format!("試行数         : {}", stage.max_times));
                chunks.push(format!("```\n{}\n```", lines.join("\n")));
            }
            let attachment = if csv_file.unwrap_or(false) {
                let snapshot = state
                    .risei_calculator
                    .snapshot(result.effective_server, &state.outer_source)
                    .await;
                let category_dict =
                    stage_category_dict(state.risei_calculator.stage_category(), result.effective_server);
                let (columns, value_row, base_stage_row) = build_reference_block(&snapshot, &category_dict);
                let column_refs: Vec<&str> = columns.iter().map(String::as_str).collect();
                let rows: Vec<_> = result
                    .stages
                    .iter()
                    .map(|s| build_stage_export_row(&s.raw, &snapshot, s.name.clone()))
                    .collect();
                let bytes = build_stage_export_xlsx(&column_refs, &value_row, &base_stage_row, &rows)?;
                Some(serenity::CreateAttachment::bytes(bytes, MATERIALS_XLSX_FILENAME))
            } else {
                None
            };
            let reply = EmbedReply {
                title: "昇進素材検索".to_string(),
                chunks,
                msg_type: MsgType::Ok,
                reply_marker: None,
            };
            (reply, attachment)
        }
    };
    send_reply_with_attachment(ctx, reply, attachment).await
}
