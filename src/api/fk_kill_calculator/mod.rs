//! フレームキル計算機（`/FrameKillCalculator`）。
//!
//! ページ本体+静的ファイル(`page_router`)は他のWeb UIツールと同じく`AppState`に
//! 依存しないので`web_ui_router()`に足す。一方 `GET /FrameKillCalculator/catalog.json`
//! （`catalog_router`）だけは別軸: `run_api`(本番。ExternalSourceRegistry経由の最新データ)と
//! `serve_web`(dev。`data/seed/*.json`から起動時に1回だけ組み立てたデータ)とでデータの
//! 取得方法が全く異なる。`web_ui_router()`はどんな`S`にも使える（state引数を取らない）
//! 汎用ルーターなので、ここに`AppState`依存のロジックを混ぜ込むことはできない。そのため、
//! データの取得方法だけを`CatalogProvider`としてこのルーターの外側
//! (呼び出し側=`run_api`/`serve_web`)から注入できるようにしている。
//! `run_api`/`serve_web`の両方が`catalog_router`を個別に`merge`すること
//! （`page_router`は普通に`web_ui_router()`へ足すだけでよい）。

use crate::engine::external_source::BoxFuture;
use crate::engine::fk_kill_calc::Catalog;
use askama::Template;
use axum::http::{header, HeaderMap};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tower_http::services::ServeDir;

const STATIC_DIR: &str = "src/api/fk_kill_calculator/static";

#[derive(Template)]
#[template(path = "fkc_index.html")]
struct IndexTemplate {
    /// canonical / OGP 用の絶対URLの起点（例: https://example.com）。
    base: String,
    /// ツール切り替えヘッダー(templates_shared/toolnav.html)用。
    active_tool: &'static str,
    /// toolnav.htmlが多言語ページ共通で要求するフィールド。このページは日本語専用(/en無し)
    /// なので常に"ja"固定。
    lang: &'static str,
}

async fn index(headers: HeaderMap) -> Html<String> {
    let page = IndexTemplate {
        base: super::base_url(&headers),
        active_tool: "fkc",
        lang: "ja",
    }
    .render()
    .unwrap();
    Html(page)
}

/// ページ本体+静的ファイル配信。`web_ui_router()`に足すこと
/// （catalog.jsonは別軸なので含めない。上記モジュール冒頭コメント参照）。
pub fn page_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/", get(index)).nest_service("/static", ServeDir::new(STATIC_DIR))
}

/// 呼び出し毎に最新のカタログを返すプロバイダ。`run_api`は`ExternalSourceRegistry`の
/// 現在値から都度組み立て、`serve_web`は起動時に1回組み立てたものを毎回同じArcで返す。
pub type CatalogProvider = Arc<dyn Fn() -> BoxFuture<'static, Arc<Catalog>> + Send + Sync>;

/// `S`はアプリ全体の状態型(`run_api`なら`Arc<AppState>`、`serve_web`なら`()`)。
/// このルーター自体は`provider`をクロージャで直接キャプチャするだけで`State<S>`を
/// 使わないため、どんな`S`に対しても`Router<S>`として作れる
/// (`web_ui_router<S>()`内の`/health`ルートと同じパターン)。
pub fn catalog_router<S>(provider: CatalogProvider) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(
        "/FrameKillCalculator/catalog.json",
        get(move || {
            let provider = provider.clone();
            async move {
                let catalog = (provider)().await;
                (
                    [(header::CACHE_CONTROL, "public, max-age=300")],
                    axum::Json(catalog.as_ref()),
                )
                    .into_response()
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn index_page_renders_with_title_and_toolnav() {
        let app = page_router::<()>();
        let response = app
            .oneshot(Request::builder().uri("/").header("host", "example.com").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("<title>フレームキル計算機"));
        assert!(html.contains("toolnav-bar"));
        assert!(html.contains(r#"href="/FrameKillCalculator" aria-current="page""#));
    }

    fn empty_provider() -> CatalogProvider {
        Arc::new(|| Box::pin(async { Arc::new(Catalog::default()) }))
    }

    #[tokio::test]
    async fn catalog_json_returns_200_with_cache_control_and_parseable_json() {
        let app = catalog_router::<()>(empty_provider());
        let response = app
            .oneshot(Request::builder().uri("/FrameKillCalculator/catalog.json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cache_control = response.headers().get(header::CACHE_CONTROL).unwrap().to_str().unwrap();
        assert_eq!(cache_control, "public, max-age=300");
        let content_type = response.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(content_type.starts_with("application/json"));
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("catalog.jsonはJSONとしてparseできること");
        assert!(parsed.get("operators").is_some());
        assert!(parsed.get("buffers").is_some());
    }
}
