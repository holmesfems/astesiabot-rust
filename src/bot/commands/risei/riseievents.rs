use super::{build_reference_block, build_stage_export_row, fmt_percent, send_reply_with_attachment, server_from_bool};
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::bot::utils::xlsx::build_stage_export_xlsx;
use crate::engine::risei_calculator_engine::server::stage_category_dict;
use crate::engine::risei_calculator_engine::Server;
use poise::serenity_prelude as serenity;

const MAX_ITEMS: usize = 20;
const EVENT_XLSX_FILENAME: &str = "EventDrop.xlsx";

async fn autocomplete_event_stage(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let state = ctx.data().state.clone();
    let snapshot = state
        .risei_calculator
        .snapshot(Server::Mainland)
        .await;
    snapshot
        .auto_complete_event_stage(partial, 25)
        .into_iter()
        .map(|(name, value)| serenity::AutocompleteChoice::new(name, value))
        .collect()
}

/// 期間限定イベントの理性効率を検索します。過去の開催済みイベントや、将来の未開催イベントも対象。
#[poise::command(slash_command)]
pub async fn riseievents(
    ctx: Context<'_>,
    #[description = "ステージ名を入力(例:SV-8 IW-8など)"]
    #[autocomplete = "autocomplete_event_stage"]
    stage: String,
    #[description = "True:グローバル版基準の計算(デフォルト)、False:大陸版の新ステージと新素材を入れた計算"]
    is_global: Option<bool>,
    #[description = "true:ドロップ率データをxlsxで添付"]
    csv_file: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let server = server_from_bool(is_global.unwrap_or(true));
    let state = ctx.data().state.clone();

    let (reply, attachment) = match state.risei_calculator.event_search(server, &stage).await {
        Err(msg) => (EmbedReply::error(&msg), None),
        Ok(stages) => {
            let mut chunks = vec![format!("検索内容 = {stage}")];
            for s in stages.iter().take(MAX_ITEMS) {
                let mut lines = vec![
                    format!("マップ名       : {}", s.name),
                    format!("イベント名     : {}", s.zone_name),
                    format!("総合効率       : {}", fmt_percent(s.total_efficiency)),
                    format!("主ドロップ     : {}", s.main_drop_name),
                    format!("ドロップ率     : {}", fmt_percent(s.main_drop_rate)),
                    format!("試行数         : {}", s.max_times),
                    format!("理性消費       : {}", s.sanity_cost),
                ];
                if let Some(time_cost) = s.time_cost {
                    lines.push(format!("時間消費(倍速) : {time_cost:.2}"));
                }
                if let Some(drop_per_minute) = s.drop_per_minute {
                    lines.push(format!("分間入手数     : {drop_per_minute:.2}"));
                }
                chunks.push(format!("```\n{}\n```", lines.join("\n")));
            }
            let attachment = if csv_file.unwrap_or(false) {
                let snapshot = state.risei_calculator.snapshot(server).await;
                let category_dict = stage_category_dict(state.risei_calculator.stage_category(), server);
                let (columns, value_row, base_stage_row) = build_reference_block(&snapshot, &category_dict);
                let column_refs: Vec<&str> = columns.iter().map(String::as_str).collect();
                let rows: Vec<_> = stages
                    .iter()
                    .map(|s| build_stage_export_row(&s.raw, &snapshot, s.name.clone()))
                    .collect();
                let bytes = build_stage_export_xlsx(&column_refs, &value_row, &base_stage_row, &rows)?;
                Some(serenity::CreateAttachment::bytes(bytes, EVENT_XLSX_FILENAME))
            } else {
                None
            };
            let reply = EmbedReply {
                title: "イベントステージ検索".to_string(),
                chunks,
                msg_type: MsgType::Ok,
                reply_marker: None,
            };
            (reply, attachment)
        }
    };
    send_reply_with_attachment(ctx, reply, attachment).await
}
