mod api;
mod bot;
mod recruit;

use api::{run_api, AppState};
use bot::run_bot;
use bot::services::moderation::ModerationState;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN not set");

    // 起動時に一度だけ求人データをロード。bot と api で共有する。
    let engine = recruit::RecruitEngine::load().expect("求人データのロードに失敗");
    let moderation = ModerationState::from_env();
    let state = Arc::new(AppState {
        recruit: engine,
        moderation,
    });
    let bot_state = state.clone();

    tokio::select! {
        _ = run_bot(token, bot_state) => {},
        _ = run_api(state) => {},
    }
}
