use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{to_embed_batches, EmbedReply, MsgType};
use crate::engine::fk_data_search::{FkDataView, FkSearchResult};
use poise::serenity_prelude as serenity;

const BASE_TITLE: &str = "FK情報検索";

/// コマンド呼び出し1回分の検索コンテキストを構築する（Python `FKInfo.getInfoFromName`の
/// TTLチェック込み）。`operator_data`/`skill_data`はスキル名解決にのみ使う。
pub async fn build_view(state: &AppState) -> FkDataView {
    FkDataView {
        fk_data: state.fk_data_search.snapshot(&state.outer_source).await,
        operator_data: state.outer_source.operator_data.get().await,
        skill_data: state.outer_source.skill_data.get().await,
    }
}

fn display_name(skill_num: &str, skill_name: &str) -> String {
    if skill_name.is_empty() {
        skill_num.to_string()
    } else {
        skill_name.to_string()
    }
}

/// Python `FKInfo.getReply`の整形込み版。
pub async fn fk_search_reply(state: &AppState, operator_name: &str, skill_num: &str) -> EmbedReply {
    let view = build_view(state).await;
    match view.search(operator_name, skill_num) {
        FkSearchResult::OperatorNotFound => EmbedReply {
            title: BASE_TITLE.to_string(),
            chunks: vec!["指定のオペレーターのFK情報は見つかりませんでした".to_string()],
            msg_type: MsgType::Err,
        },
        FkSearchResult::NeedsSkillSelection { choices } => {
            let list = choices
                .iter()
                .map(|(num, name)| format!("{num}: {name}"))
                .collect::<Vec<_>>()
                .join("\n");
            EmbedReply {
                title: BASE_TITLE.to_string(),
                chunks: vec![format!("複数のFKスキルがあります:\n{list}\n"), "どのスキルか選んでください".to_string()],
                msg_type: MsgType::Err,
            }
        }
        FkSearchResult::SkillNotFound { candidates } => {
            let list = candidates
                .iter()
                .map(|c| format!("{}: {}", c.skill_num, display_name(&c.skill_num, &c.skill_name)))
                .collect::<Vec<_>>()
                .join("\n");
            EmbedReply {
                title: BASE_TITLE.to_string(),
                chunks: vec![format!(
                    "指定のスキルのFK情報は見つかりませんでした。以下の候補がありますので、一つ選択してください:\n{list}"
                )],
                msg_type: MsgType::Err,
            }
        }
        FkSearchResult::Found(skill) => {
            let header = if !skill.skill_name.is_empty() {
                format!("スキル名: {}\n", skill.skill_name)
            } else {
                format!("スキル指定: {}\n", skill.requested_skill_num)
            };
            EmbedReply {
                title: BASE_TITLE.to_string(),
                chunks: vec![
                    header,
                    format!("最短FK数: {}\n", skill.fk_num),
                    format!("FK誤差: {}\n", skill.fk_err),
                    format!("詳細情報: \n```\n{}\n```", skill.detail),
                ],
                msg_type: MsgType::Ok,
            }
        }
    }
}

/// Python版`operator_name_autocomplete_forfk`相当。TTLチェックは行わない
/// （Python版の`autoComplete`も鮮度更新をトリガーしない非対称性を踏襲）。
async fn autocomplete_operator_name(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let state = &ctx.data().state;
    let fk_data = state.outer_source.fk_data.get().await;
    crate::engine::fk_data_search::search::autocomplete(&fk_data, partial, 25)
        .into_iter()
        .map(|name| serenity::AutocompleteChoice::new(name.clone(), name))
        .collect()
}

/// Python版`send_reply`と同じ役割。feature間の結合を避けるためここに複製している
/// （`bot/commands/operator_cost_calc/mod.rs`の同名関数を参照）。
async fn send_reply(ctx: Context<'_>, reply: EmbedReply) -> Result<(), Error> {
    for batch in to_embed_batches(&reply) {
        let mut created = poise::CreateReply::default();
        created.embeds = batch;
        ctx.send(created).await?;
    }
    Ok(())
}

/// オペレーターのFK情報を調べる。
#[poise::command(slash_command)]
pub async fn fksearch(
    ctx: Context<'_>,
    #[description = "オペレーターの名前、大陸先行オペレーターも日本語を入れてください"]
    #[autocomplete = "autocomplete_operator_name"]
    operator_name: String,
    #[description = "スキルは数字のみ(例:'1','2','3')、素質は'素質'+数字(例:'素質1')で入力してください。FKスキル一つしか持たないオペレーターのみ、空欄でもOK"]
    skill_num: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let state = ctx.data().state.clone();
    let reply = fk_search_reply(&state, &operator_name, skill_num.as_deref().unwrap_or("")).await;
    send_reply(ctx, reply).await
}
