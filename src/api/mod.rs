mod recruitment;
mod wl_battery_simulator;

use crate::bot::services::moderation::ModerationState;
use crate::bot::services::uranai::UranaiState;
use crate::engine::fk_data_search::FkDataSearchEngine;
use crate::engine::outer_source::OuterSourceRegistry;
use crate::engine::recruit::RecruitEngine;
use crate::engine::risei_calculator_engine::RiseiCalculatorEngine;
use axum::http::StatusCode;
use axum::response::Redirect;
use axum::{routing::get, routing::post, Router};
use std::sync::Arc;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// FastAPI の /docs 相当。JSON API（/recruitment/）のみを対象とする。
/// WLBatterySimulator は askama+htmx のサーバーレンダリングなので対象外。
#[derive(OpenApi)]
#[openapi(
    paths(recruitment::do_recruitment),
    components(schemas(recruitment::OcrRawData, recruitment::TagReplyData))
)]
struct ApiDoc;

/// アプリ全体で共有する状態。今後 DB やキャッシュもここに載せられる。
pub struct AppState {
    pub recruit: RecruitEngine,
    pub moderation: ModerationState,
    /// 外部サイトから取得する情報のレジストリ（operator_data など）。
    pub outer_source: OuterSourceRegistry,
    /// 理性価値表の計算エンジン（グローバル版・大陸版）。
    pub risei_calculator: RiseiCalculatorEngine,
    /// FK情報スプレッドシートの鮮度管理（読み取り駆動で1時間毎に再fetch）。
    pub fk_data_search: FkDataSearchEngine,
    /// 占い館（OpenAIチャット）の会話セッション・課金ロール判定・APIクライアント。
    pub uranai: UranaiState,
}

pub async fn run_api(state: Arc<AppState>) {
    let app = Router::new()
        .route("/", get(|| async { Redirect::temporary("/WLBatterySimulator") }))
        .route("/health", get(|| async { "ok" }))
        .route("/recruitment/", post(recruitment::do_recruitment)) // Python と同じパス
        // axum の nest() は内側の "/" を末尾スラッシュなしの prefix にのみ割り当てるため、
        // "/WLBatterySimulator/" 単体は別途 prefix なしへリダイレクトする。
        .route(
            "/WLBatterySimulator/",
            get(|| async { Redirect::permanent("/WLBatterySimulator") }),
        )
        .nest("/WLBatterySimulator", wl_battery_simulator::router())
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .fallback(not_found)
        .with_state(state);
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .expect("PORT must be a number");
    let address = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn not_found() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "404 Not Found")
}
