pub mod riseievents;
pub mod riseikakin;
pub mod riseilists;
pub mod riseimaterials;
pub mod riseistages;

use crate::bot::data::{Context, Error};
use crate::bot::reply::{to_embed_batches, EmbedReply};
use crate::bot::utils::xlsx::StageExportRow;
use crate::engine::risei_calculator_engine::server::StageCategoryInfo;
use crate::engine::risei_calculator_engine::{EngineSnapshot, Server, StageItem};
use poise::serenity_prelude as serenity;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// `is_global: bool` スラッシュコマンド引数から [`Server`] へ変換する
/// （Python版の各コマンドの `is_global` 引数と同じ意味。true=グローバル版基準）。
pub fn server_from_bool(is_global: bool) -> Server {
    if is_global {
        Server::Global
    } else {
        Server::Mainland
    }
}

/// EmbedReply をスラッシュコマンドの応答として送信する。
/// 1メッセージに収まらない場合は複数回に分けて送る（`to_embed_batches` 参照）。
pub async fn send_reply(ctx: Context<'_>, reply: EmbedReply) -> Result<(), Error> {
    send_reply_with_attachment(ctx, reply, None).await
}

/// [`send_reply`]にDiscord添付ファイル(csv_fileオプションのxlsx等)を追加できる版
/// （Python版 `RCReply.attatchments` 相当）。複数バッチに分かれる場合、添付は最後の
/// バッチにのみ付与する（Python版の「最後のチャンクにfilesを付ける」動作を踏襲）。
/// 汎用の`EmbedReply`(`bot/reply.rs`)は他機能からも広く使われるため添付フィールドを
/// 持たせず、riseiローカルのこの関数だけを拡張している。
pub async fn send_reply_with_attachment(
    ctx: Context<'_>,
    reply: EmbedReply,
    attachment: Option<serenity::CreateAttachment>,
) -> Result<(), Error> {
    let batches = to_embed_batches(&reply);
    let last = batches.len().saturating_sub(1);
    let mut attachment = attachment;
    for (i, batch) in batches.into_iter().enumerate() {
        let mut created = poise::CreateReply::default();
        created.embeds = batch;
        if i == last {
            if let Some(a) = attachment.take() {
                created = created.attachment(a);
            }
        }
        ctx.send(created).await?;
    }
    Ok(())
}

/// csv_fileオプション用xlsxの参照ブロック(列見出し・理性価値行・基準ステージ行)を組み立てる。
/// riseimaterials/riseistages/riseievents の3コマンドで共通。
/// Python版の換算行列・定番周回マップ行列は検証用途で実用上参照されないため、
/// 「理性価値」「基準ステージ」の2行に簡略化している（詳細は`bot/utils/xlsx.rs`冒頭コメント参照）。
pub fn build_reference_block(
    snapshot: &EngineSnapshot,
    category_dict: &BTreeMap<String, StageCategoryInfo>,
) -> (Vec<String>, Vec<f64>, Vec<Option<String>>) {
    let columns: Vec<String> = snapshot
        .values
        .value_target
        .iter()
        .map(|zh| snapshot.values.item_names.zh_to_ja(zh).to_string())
        .collect();
    let value_row: Vec<f64> = snapshot.values.value_array.iter().copied().collect();

    let mut main_item_to_category: HashMap<&str, &str> = HashMap::new();
    for (key, info) in category_dict {
        main_item_to_category.insert(info.main_item.as_str(), key.as_str());
    }
    let base_stage_row: Vec<Option<String>> = snapshot
        .values
        .value_target
        .iter()
        .map(|zh| {
            main_item_to_category
                .get(zh)
                .and_then(|cat| snapshot.base_stage_display.get(*cat).cloned())
        })
        .collect();

    (columns, value_row, base_stage_row)
}

/// csv_fileオプション用xlsxのステージ1行分を組み立てる（`raw`から生ドロップ率を取り出す）。
pub fn build_stage_export_row(raw: &Arc<StageItem>, snapshot: &EngineSnapshot, name: String) -> StageExportRow {
    let drop_values = raw
        .to_drop_array(&snapshot.values.value_target, &snapshot.values.item_names)
        .iter()
        .copied()
        .collect();
    StageExportRow {
        name,
        drop_values,
        ap_cost: raw.ap_cost,
    }
}

/// パーセント表示（小数点以下1桁）。Python `"{0:.1%}"`.
pub fn fmt_percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

/// 効率などの小数表示（小数点以下3桁）。Python `"{0:.3f}"`.
pub fn fmt_value(value: f64) -> String {
    format!("{value:.3}")
}
