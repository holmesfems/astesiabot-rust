mod ef_recipe_calculator;
mod lod_chest_solver;
mod recruitment;
mod wl_battery_simulator;

use crate::bot::services::moderation::ModerationState;
use crate::bot::services::uranai::UranaiState;
use crate::engine::fk_data_search::FkDataSearchEngine;
use crate::engine::external_source::ExternalSourceRegistry;
use crate::engine::recruit::RecruitEngine;
use crate::engine::risei_calculator_engine::RiseiCalculatorEngine;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
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
    pub external_source: ExternalSourceRegistry,
    /// 理性価値表の計算エンジン（グローバル版・大陸版）。
    pub risei_calculator: RiseiCalculatorEngine,
    /// FK情報スプレッドシートの鮮度管理（読み取り駆動で1時間毎に再fetch）。
    pub fk_data_search: FkDataSearchEngine,
    /// 占い館（OpenAIチャット）の会話セッション・課金ロール判定・APIクライアント。
    pub uranai: UranaiState,
}

/// canonical / hreflang / sitemap で使う絶対URLの起点（例: `https://example.com`）。
///
/// `PUBLIC_BASE_URL` があればそれを優先する（独自ドメインへ寄せたい場合に設定する）。
/// 無ければリバースプロキシ越しの `X-Forwarded-Proto` + `Host` から組み立てる
/// （Heroku 等は手前でTLSを終端するので scheme をヘッダから見ないとhttpになる）。
pub(crate) fn base_url(headers: &HeaderMap) -> String {
    if let Ok(base) = std::env::var("PUBLIC_BASE_URL") {
        let base = base.trim_end_matches('/').to_string();
        if !base.is_empty() {
            return base;
        }
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
                "http"
            } else {
                "https"
            }
        });
    format!("{scheme}://{host}")
}

/// サーバーレンダリングしているツール類だけをクローラに拾わせる。
/// JSON API（/recruitment/）と Swagger UI は対象外。
async fn robots(headers: HeaderMap) -> impl IntoResponse {
    let base = base_url(&headers);
    let body = format!(
        "User-agent: *\n\
         Allow: /\n\
         Disallow: /docs\n\
         Disallow: /api-docs/\n\
         Disallow: /health\n\
         Sitemap: {base}/sitemap.xml\n"
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body)
}

async fn sitemap(headers: HeaderMap) -> impl IntoResponse {
    let base = base_url(&headers);
    // LodChestSolver は言語ごとに別URLなので、各URLから相互に hreflang を張る。
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"
        xmlns:xhtml="http://www.w3.org/1999/xhtml">
  <url>
    <loc>{base}/LodChestSolver</loc>
    <xhtml:link rel="alternate" hreflang="ja" href="{base}/LodChestSolver"/>
    <xhtml:link rel="alternate" hreflang="en" href="{base}/LodChestSolver/en"/>
    <xhtml:link rel="alternate" hreflang="x-default" href="{base}/LodChestSolver"/>
  </url>
  <url>
    <loc>{base}/LodChestSolver/en</loc>
    <xhtml:link rel="alternate" hreflang="ja" href="{base}/LodChestSolver"/>
    <xhtml:link rel="alternate" hreflang="en" href="{base}/LodChestSolver/en"/>
    <xhtml:link rel="alternate" hreflang="x-default" href="{base}/LodChestSolver"/>
  </url>
  <url>
    <loc>{base}/WLBatterySimulator</loc>
  </url>
  <url>
    <loc>{base}/EFRecipeCalculator</loc>
  </url>
</urlset>
"#
    );
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
}

pub async fn run_api(state: Arc<AppState>) {
    let app = Router::new()
        .route("/", get(|| async { Redirect::temporary("/WLBatterySimulator") }))
        .route("/health", get(|| async { "ok" }))
        .route("/robots.txt", get(robots))
        .route("/sitemap.xml", get(sitemap))
        .route("/recruitment/", post(recruitment::do_recruitment)) // Python と同じパス
        // axum の nest() は内側の "/" を末尾スラッシュなしの prefix にのみ割り当てるため、
        // "/WLBatterySimulator/" 単体は別途 prefix なしへリダイレクトする。
        .route(
            "/WLBatterySimulator/",
            get(|| async { Redirect::permanent("/WLBatterySimulator") }),
        )
        .nest("/WLBatterySimulator", wl_battery_simulator::router())
        .route(
            "/EFRecipeCalculator/",
            get(|| async { Redirect::permanent("/EFRecipeCalculator") }),
        )
        .nest("/EFRecipeCalculator", ef_recipe_calculator::router())
        .route(
            "/LodChestSolver/",
            get(|| async { Redirect::permanent("/LodChestSolver") }),
        )
        .nest("/LodChestSolver", lod_chest_solver::router())
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

