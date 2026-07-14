use astesiabot_rust::api::{run_api, AppState};
use astesiabot_rust::bot::run_bot;
use astesiabot_rust::bot::services::moderation::ModerationState;
use astesiabot_rust::engine;
use std::sync::Arc;
use std::time::Duration;

/// 外部サイト情報（operator_names など）の定期再fetch間隔。
const OUTER_SOURCE_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN not set");

    // 起動時に一度だけ求人データをロード。bot と api で共有する。
    let recruit_engine = engine::recruit::RecruitEngine::load().expect("求人データのロードに失敗");
    let moderation = ModerationState::from_env();
    // 外部サイト情報も起動時に一括fetch（失敗時の扱いは Source::load 参照）。
    let outer_source = engine::outer_source::OuterSourceRegistry::load().await;
    let state = Arc::new(AppState {
        recruit: recruit_engine,
        moderation,
        outer_source,
    });
    let bot_state = state.clone();

    let refresh_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(OUTER_SOURCE_REFRESH_INTERVAL).await;
            refresh_state.outer_source.refresh_all().await;
        }
    });

    tokio::select! {
        _ = run_bot(token, bot_state) => {},
        _ = run_api(state) => {},
    }
}
