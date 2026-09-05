//! 幽霊船 宝箱ソルバー（レジェンド オブ ドラグーン）。
//!
//! 言語ごとに別URL・別HTMLを返す（1URL=1言語）。クローラに両言語を見せるため
//! 実行時のJSによる文字列差し替えはせず、静的ラベルは各テンプレートのmarkupに
//! 直書きし、動的に組み立てる文言だけを各ページから `static/ui.js` に渡す。
//!
//!   /LodChestSolver     … 日本語（canonical / x-default）
//!   /LodChestSolver/en  … 英語
//!
//! 計算層（`static/engine.js`）と表現層（`static/ui.js`）は言語間で共有する。

use askama::Template;
use axum::http::HeaderMap;
use axum::response::{Html, Redirect};
use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

const STATIC_DIR: &str = "src/api/lod_chest_solver/static";

#[derive(Template)]
#[template(path = "lod_index.html")]
struct IndexJaTemplate {
    /// canonical / hreflang 用の絶対URLの起点（例: https://example.com）。
    base: String,
    /// ツール切り替えヘッダー(templates_shared/toolnav.html)用。
    active_tool: &'static str,
    lang: &'static str,
}

#[derive(Template)]
#[template(path = "lod_index_en.html")]
struct IndexEnTemplate {
    base: String,
    active_tool: &'static str,
    lang: &'static str,
}

async fn index_ja(headers: HeaderMap) -> Html<String> {
    let page = IndexJaTemplate {
        base: super::base_url(&headers),
        active_tool: "lod",
        lang: "ja",
    }
    .render()
    .unwrap();
    Html(page)
}

async fn index_en(headers: HeaderMap) -> Html<String> {
    let page = IndexEnTemplate {
        base: super::base_url(&headers),
        active_tool: "lod",
        lang: "en",
    }
    .render()
    .unwrap();
    Html(page)
}

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(index_ja))
        .route("/en", get(index_en))
        // 末尾スラッシュ付きは canonical 側へ寄せる（重複URLを作らない）。
        .route(
            "/en/",
            get(|| async { Redirect::permanent("/LodChestSolver/en") }),
        )
        .nest_service("/static", ServeDir::new(STATIC_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn get_body(uri: &str) -> String {
        let app = router::<()>();
        let response = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn ja_page_renders_japanese_toolnav_with_active_chip() {
        let html = get_body("/").await;
        assert!(html.contains("toolnav-bar"));
        assert!(html.contains(r#"href="/LodChestSolver" aria-current="page""#));
        assert!(html.contains("幽霊船宝箱ソルバー"));
    }

    #[tokio::test]
    async fn en_page_renders_english_toolnav_with_active_chip() {
        let html = get_body("/en").await;
        assert!(html.contains("toolnav-bar"));
        assert!(html.contains(r#"href="/LodChestSolver" aria-current="page""#));
        assert!(html.contains("Chest Solver"));
    }
}
