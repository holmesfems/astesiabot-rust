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
}

#[derive(Template)]
#[template(path = "lod_index_en.html")]
struct IndexEnTemplate {
    base: String,
}

async fn index_ja(headers: HeaderMap) -> Html<String> {
    let page = IndexJaTemplate {
        base: super::base_url(&headers),
    }
    .render()
    .unwrap();
    Html(page)
}

async fn index_en(headers: HeaderMap) -> Html<String> {
    let page = IndexEnTemplate {
        base: super::base_url(&headers),
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
