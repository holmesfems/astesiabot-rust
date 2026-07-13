use crate::bot::services::moderation::ModerationState;
use crate::recruit::RecruitEngine;
use axum::{extract::State, routing::get, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// アプリ全体で共有する状態。今後 DB やキャッシュもここに載せられる。
pub struct AppState {
    pub recruit: RecruitEngine,
    pub moderation: ModerationState,
}

/// Python の OCRRawData に対応。
#[derive(Deserialize)]
struct OcrRawData {
    text: String,
    #[serde(rename = "pickupOperators", default)]
    pickup_operators: Option<Vec<String>>,
}

/// Python の TagReplyData に対応。
#[derive(Serialize)]
struct TagReplyData {
    title: String,
    reply: String,
}

async fn do_recruitment(
    State(state): State<Arc<AppState>>,
    Json(data): Json<OcrRawData>,
) -> Json<TagReplyData> {
    let pickup = data.pickup_operators.as_deref();
    let result = state.recruit.process_from_ocr(&data.text, pickup);
    Json(TagReplyData {
        title: result.title,
        reply: result.reply,
    })
}

pub async fn run_api(state: Arc<AppState>) {
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/recruitment/", post(do_recruitment)) // Python と同じパス
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
