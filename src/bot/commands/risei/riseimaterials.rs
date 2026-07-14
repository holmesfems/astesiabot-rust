use super::{fmt_percent, fmt_value, send_reply, server_from_bool};
use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::risei_calculator_engine::stage_info::DEFAULT_SHOW_MIN_TIMES;
use crate::engine::risei_calculator_engine::{drop_per_minute, filter_stages_by_show_min_times, Server};
use poise::serenity_prelude as serenity;

const MAX_ITEMS: usize = 15;

/// 昇進素材カテゴリの1ステージ分の効率情報（Python `riseimaterials`のjsonForAI各項目相当）。
pub struct MaterialStageInfo {
    pub name: String,
    pub efficiency: f64,
    pub sanity_cost: f64,
    /// 倍速時のクリア時間(秒)。ステージにクリア時間データが無ければ`None`。
    pub time_cost: Option<f64>,
    pub drop_per_minute: Option<f64>,
    pub main_item_efficiency: f64,
    pub confidence_3sigma: f64,
    pub promotion_efficiency: f64,
    pub max_times: i64,
}

/// riseimaterials の計算結果一式（Discord/将来のGPT function calling共通）。
pub struct MaterialSearchResult {
    /// `new`カテゴリ指定時は自動的にMainlandへ切り替わる。
    pub effective_server: Server,
    pub category_ja: String,
    pub main_item_value: f64,
    pub main_item_std_dev: f64,
    /// 総合効率の降順でソート済み。件数の絞り込みは呼び出し側の責務。
    pub stages: Vec<MaterialStageInfo>,
}

/// riseimaterials相当の計算のみを行う共通部。`new`カテゴリを指定した場合は
/// 自動的に大陸版基準になる。整形はこの関数の呼び出し側（Discordコマンド/将来の
/// GPT function calling）がそれぞれ行う。
pub async fn material_search(state: &AppState, server: Server, category_key: &str) -> Result<MaterialSearchResult, String> {
    let effective_server = if state.risei_calculator.stage_category().new.contains_key(category_key) {
        Server::Mainland
    } else {
        server
    };
    let snapshot = state.risei_calculator.snapshot(effective_server, &state.outer_source).await;
    let cat = snapshot.category(category_key).ok_or_else(|| format!("無効なカテゴリ:{category_key}"))?;
    let info = cat.info.clone();
    let stages = filter_stages_by_show_min_times(snapshot.category_stages(category_key), DEFAULT_SHOW_MIN_TIMES);
    let promotion_items: Vec<&str> = snapshot.values.value_target[4..].to_vec();

    let mut list: Vec<MaterialStageInfo> = stages
        .iter()
        .map(|stage| {
            let items: Vec<&str> = info.items.iter().map(String::as_str).collect();
            let (time_cost, drop_per_minute_value) = if stage.min_clear_time > 0.0 {
                (Some(stage.min_clear_time / 2.0), Some(drop_per_minute(stage, &info, &snapshot.values)))
            } else {
                (None, None)
            };
            MaterialStageInfo {
                name: stage.name_with_replicate(),
                efficiency: stage.get_efficiency(&snapshot.values),
                sanity_cost: stage.ap_cost,
                time_cost,
                drop_per_minute: drop_per_minute_value,
                main_item_efficiency: stage.get_partial_efficiency(&snapshot.values, &items),
                confidence_3sigma: snapshot.stage_dev(stage) * 3.0,
                promotion_efficiency: stage.get_partial_efficiency(&snapshot.values, &promotion_items),
                max_times: stage.max_times(),
            }
        })
        .collect();
    list.sort_by(|a, b| b.efficiency.total_cmp(&a.efficiency));

    Ok(MaterialSearchResult {
        effective_server,
        category_ja: info.to_ja.clone(),
        main_item_value: snapshot.values.get_value_from_zh(&info.main_item),
        main_item_std_dev: snapshot.values.get_std_dev_from_zh(&info.main_item),
        stages: list,
    })
}

/// riseimaterials/riseicalculator の target_item 選択肢（Python版は常に大陸版の
/// 全カテゴリ(main+new)を選択肢に使う）。
async fn autocomplete_target_item(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
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
    #[description = "True:グローバル版基準の計算(デフォルト)、False:大陸版の新ステージと新素材を入れた計算"] is_global: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let server = server_from_bool(is_global.unwrap_or(true));
    let state = ctx.data().state.clone();

    let reply = match material_search(&state, server, &target_item).await {
        Err(msg) => EmbedReply::error(&msg),
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
                lines.push(format!("主素材効率     : {}", fmt_percent(stage.main_item_efficiency)));
                lines.push(format!("99%信頼区間(3σ): {}", fmt_percent(stage.confidence_3sigma)));
                lines.push(format!("昇進効率       : {}", fmt_percent(stage.promotion_efficiency)));
                lines.push(format!("試行数         : {}", stage.max_times));
                chunks.push(format!("```\n{}\n```", lines.join("\n")));
            }
            EmbedReply {
                title: "昇進素材検索".to_string(),
                chunks,
                msg_type: MsgType::Ok,
            }
        }
    };
    send_reply(ctx, reply).await
}
