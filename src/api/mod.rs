mod recruitment;

use crate::bot::services::moderation::ModerationState;
use crate::engine::recruit::RecruitEngine;
use axum::{routing::get, routing::post, Router};
use std::sync::Arc;

/// アプリ全体で共有する状態。今後 DB やキャッシュもここに載せられる。
pub struct AppState {
    pub recruit: RecruitEngine,
    pub moderation: ModerationState,
}

pub async fn run_api(state: Arc<AppState>) {
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/recruitment/", post(recruitment::do_recruitment)) // Python と同じパス
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
