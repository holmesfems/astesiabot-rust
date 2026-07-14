use super::{fmt_percent, send_reply, server_from_bool};
use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::risei_calculator_engine::Server;
use poise::serenity_prelude as serenity;

const MAX_ITEMS: usize = 20;

/// riseievents の1ステージ分の効率情報（Python `riseievents`のjsonForAI各項目相当）。
pub struct EventStageInfo {
    pub name: String,
    pub zone_name: String,
    pub total_efficiency: f64,
    pub main_drop_name: String,
    pub main_drop_rate: f64,
    pub max_times: i64,
    pub sanity_cost: f64,
    pub time_cost: Option<f64>,
    pub drop_per_minute: Option<f64>,
}

/// riseievents相当の計算のみを行う共通部。整形は呼び出し側の責務。
pub async fn event_search(state: &AppState, server: Server, target_code: &str) -> Result<Vec<EventStageInfo>, String> {
    let snapshot = state.risei_calculator.snapshot(server, &state.outer_source).await;
    let mut stages = snapshot.search_event_stage(target_code);
    if stages.is_empty() {
        return Err(format!("無効なステージ指定{target_code}"));
    }

    stages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(stages
        .iter()
        .filter_map(|stage| {
            let (main_drop_name, main_drop_rate) = stage.get_max_efficiency_item(&snapshot.values.item_names)?;
            let time_cost = (stage.min_clear_time > 0.0).then_some(stage.min_clear_time / 2.0);
            let drop_per_minute_value =
                (stage.min_clear_time > 0.0).then(|| main_drop_rate / stage.min_clear_time * 120.0);
            Some(EventStageInfo {
                name: stage.name_with_replicate(),
                zone_name: stage.zone_name.clone(),
                total_efficiency: stage.get_efficiency(&snapshot.values),
                main_drop_name,
                main_drop_rate,
                max_times: stage.max_times(),
                sanity_cost: stage.ap_cost,
                time_cost,
                drop_per_minute: drop_per_minute_value,
            })
        })
        .collect())
}

async fn autocomplete_event_stage(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let state = ctx.data().state.clone();
    let snapshot = state.risei_calculator.snapshot(Server::Mainland, &state.outer_source).await;
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
    #[description = "True:グローバル版基準の計算(デフォルト)、False:大陸版の新ステージと新素材を入れた計算"] is_global: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let server = server_from_bool(is_global.unwrap_or(true));
    let state = ctx.data().state.clone();

    let reply = match event_search(&state, server, &stage).await {
        Err(msg) => EmbedReply::error(&msg),
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
            EmbedReply {
                title: "イベントステージ検索".to_string(),
                chunks,
                msg_type: MsgType::Ok,
            }
        }
    };
    send_reply(ctx, reply).await
}
