mod recruitment;
mod wl_battery_simulator;

use crate::bot::services::moderation::ModerationState;
use crate::engine::recruit::RecruitEngine;
use axum::response::Redirect;
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
        // axum の nest() は内側の "/" を末尾スラッシュなしの prefix にのみ割り当てるため、
        // "/WLBatterySimulator/" 単体は別途 prefix なしへリダイレクトする。
        .route(
            "/WLBatterySimulator/",
            get(|| async { Redirect::permanent("/WLBatterySimulator") }),
        )
        .nest("/WLBatterySimulator", wl_battery_simulator::router())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
