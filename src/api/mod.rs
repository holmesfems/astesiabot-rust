mod ef_recipe_calculator;
pub mod fk_kill_calculator;
mod home;
mod legacy_host_redirect;
mod lod_chest_solver;
mod recruitment;
mod site_icons;
mod test_runner;
mod wl_battery_simulator;

use crate::bot::services::moderation::ModerationState;
use crate::bot::services::uranai::UranaiState;
use crate::engine::fk_data_search::FkDataSearchEngine;
use crate::engine::external_source::ExternalSourceRegistry;
use crate::engine::fk_kill_calc::{self, Overrides};
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
    // LodChestSolver / TestRunner は言語ごとに別URLなので、各URLから相互に hreflang を張る。
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"
        xmlns:xhtml="http://www.w3.org/1999/xhtml">
  <url>
    <loc>{base}/</loc>
  </url>
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
    <loc>{base}/TestRunner</loc>
    <xhtml:link rel="alternate" hreflang="ja" href="{base}/TestRunner"/>
    <xhtml:link rel="alternate" hreflang="en" href="{base}/TestRunner/en"/>
    <xhtml:link rel="alternate" hreflang="x-default" href="{base}/TestRunner"/>
  </url>
  <url>
    <loc>{base}/TestRunner/en</loc>
    <xhtml:link rel="alternate" hreflang="ja" href="{base}/TestRunner"/>
    <xhtml:link rel="alternate" hreflang="en" href="{base}/TestRunner/en"/>
    <xhtml:link rel="alternate" hreflang="x-default" href="{base}/TestRunner"/>
  </url>
  <url>
    <loc>{base}/WLBatterySimulator</loc>
  </url>
  <url>
    <loc>{base}/EFRecipeCalculator</loc>
  </url>
  <url>
    <loc>{base}/FrameKillCalculator</loc>
  </url>
</urlset>
"#
    );
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
}

/// Web UI（AppState に依存しないページ群）だけのルーター。
/// 本番の `run_api` と、dev 用の `serve_web` バイナリ / e2e テストが共有する。
/// ここにルートを足せば両方に反映される。片方にだけ書かないこと。
pub fn web_ui_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .merge(home::router())
        .merge(site_icons::router())
        .route("/health", get(|| async { "ok" }))
        .route("/robots.txt", get(robots))
        .route("/sitemap.xml", get(sitemap))
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
        .route(
            "/TestRunner/",
            get(|| async { Redirect::permanent("/TestRunner") }),
        )
        .nest("/TestRunner", test_runner::router())
        .route(
            "/FrameKillCalculator/",
            get(|| async { Redirect::permanent("/FrameKillCalculator") }),
        )
        .nest("/FrameKillCalculator", fk_kill_calculator::page_router())
}

/// `state`のExternalSourceRegistry(operator_data/operator_combat/skill_data)+
/// fk_data_search(1時間TTL。/fksearchと同じ鮮度)から、リクエストの都度カタログを
/// 組み立てるプロバイダ。カタログ生成自体はJSONを舐めるだけの軽い処理なので、
/// 現時点ではリクエスト間キャッシュはしない(重くなるようなら`Source`と同じ
/// 「メモリ保持+TTL」方式に寄せて再検討する)。
fn catalog_provider_for(state: Arc<AppState>) -> fk_kill_calculator::CatalogProvider {
    Arc::new(move || {
        let state = state.clone();
        Box::pin(async move {
            let fk = state.fk_data_search.snapshot(&state.external_source).await;
            let ops = state.external_source.operator_data.get().await;
            let combat = state.external_source.operator_combat.get().await;
            let skills = state.external_source.skill_data.get().await;
            let build = fk_kill_calc::build_catalog(&fk, &ops, &combat, &skills, Overrides::global());
            Arc::new(build.catalog)
        })
    })
}

pub async fn run_api(state: Arc<AppState>) {
    let catalog_provider = catalog_provider_for(state.clone());
    let mut app = web_ui_router::<Arc<AppState>>()
        .merge(fk_kill_calculator::catalog_router::<Arc<AppState>>(catalog_provider))
        .route("/recruitment/", post(recruitment::do_recruitment)) // Python と同じパス
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .fallback(not_found)
        .with_state(state);
    // 旧ホスト（*.herokuapp.com / www.）の GET を PUBLIC_BASE_URL へ 301。未設定なら何もしない。
    // serve_web は 127.0.0.1 でしか受けないので、ここ（本番側）だけに掛ける。
    if let Some(canonical) = legacy_host_redirect::CanonicalBase::from_env() {
        app = app.layer(axum::middleware::from_fn_with_state(
            Arc::new(canonical),
            legacy_host_redirect::redirect_legacy_host,
        ));
    }
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


#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn get_html(path: &str) -> String {
        let response = web_ui_router::<()>()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("host", "example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// templates_shared/head_icons.html を include し忘れたページが無いこと。
    #[tokio::test]
    async fn every_page_links_site_icons() {
        for path in [
            "/",
            "/WLBatterySimulator",
            "/EFRecipeCalculator",
            "/LodChestSolver",
            "/LodChestSolver/en",
            "/TestRunner",
            "/TestRunner/en",
            "/FrameKillCalculator",
        ] {
            let html = get_html(path).await;
            let head = &html[..html.find("</head>").expect(path)];
            assert!(head.contains(r#"<link rel="icon" href="/favicon.svg""#), "{path}");
            assert!(head.contains(r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png">"#), "{path}");
            assert!(head.contains(r#"<link rel="manifest" href="/site.webmanifest">"#), "{path}");
        }
    }

    /// 横長OGPを持たないページは正方形アイコンを絶対URLで og:image にする。
    #[tokio::test]
    async fn pages_without_own_ogp_use_square_icon() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "example.com".parse().unwrap());
        let base = base_url(&headers);
        for path in [
            "/WLBatterySimulator",
            "/EFRecipeCalculator",
            "/LodChestSolver",
            "/LodChestSolver/en",
            "/TestRunner",
            "/TestRunner/en",
            "/FrameKillCalculator",
        ] {
            let html = get_html(path).await;
            assert!(
                html.contains(&format!(r#"<meta property="og:image" content="{base}/icon-512.png">"#)),
                "{path}"
            );
            assert!(html.contains(r#"<meta name="twitter:card" content="summary">"#), "{path}");
        }
        let home = get_html("/").await;
        assert!(home.contains(&format!(r#"<meta property="og:image" content="{base}/static/ogp.png">"#)));
        assert!(!home.contains("icon-512.png"));
    }
}
