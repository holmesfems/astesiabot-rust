//! 試験手順ランナー（アークナイツ非依存の単発ツール）。
//!
//! 元は G:\Videos\an39_risei2\riseiCalculator\git\test-procedure\test_runner.html
//! （手順書のMarkdownを読み込み、OK/NGを押すだけで進められるオフライン動作の
//! 単一HTMLアプリ）。LodChestSolver と同じく言語ごとに別URL・別HTMLを返す（1URL=1言語）。
//!
//!   /TestRunner     … 日本語（canonical / x-default）
//!   /TestRunner/en  … 英語
//!
//! LodChestSolver と異なり、計算層（手順書パーサー）と表現層（DOM描画）の分離は
//! 行わず、ja/en 各テンプレートにHTML+JSを全文複製している。英語版は主要UI文言
//! （見出し・ボタン・トースト・演出文言・エクスポート見出し等）のみを翻訳しており、
//! パーサーが認識する見出しキーワード（`用語定義:` 等）は意図的に日本語のまま
//! 変更していない（英語で書かれた手順書のネイティブ解釈はスコープ外）。
//! test_runner.html は外部CSS/JSを一切参照しない完全自己完結ファイルなので、
//! LodChestSolver のような static 配信（ServeDir）は不要。

use askama::Template;
use axum::http::HeaderMap;
use axum::response::{Html, Redirect};
use axum::routing::get;
use axum::Router;

#[derive(Template)]
#[template(path = "tr_index.html")]
struct IndexJaTemplate {
    /// canonical / hreflang 用の絶対URLの起点（例: https://example.com）。
    base: String,
    /// ツール切り替えヘッダー(templates_shared/toolnav.html)用。
    active_tool: &'static str,
    lang: &'static str,
}

#[derive(Template)]
#[template(path = "tr_index_en.html")]
struct IndexEnTemplate {
    base: String,
    active_tool: &'static str,
    lang: &'static str,
}

async fn index_ja(headers: HeaderMap) -> Html<String> {
    let page = IndexJaTemplate {
        base: super::base_url(&headers),
        active_tool: "tr",
        lang: "ja",
    }
    .render()
    .unwrap();
    Html(page)
}

async fn index_en(headers: HeaderMap) -> Html<String> {
    let page = IndexEnTemplate {
        base: super::base_url(&headers),
        active_tool: "tr",
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
            get(|| async { Redirect::permanent("/TestRunner/en") }),
        )
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
        assert!(html.contains(r#"href="/TestRunner" aria-current="page""#));
        assert!(html.contains("試験手順ランナー"));
        assert!(html.contains(r#"id="ok-btn""#));
        assert!(html.contains("手順を読み込む"));
        assert!(html.contains("用語定義"));
    }

    #[tokio::test]
    async fn en_page_renders_english_toolnav_with_active_chip() {
        let html = get_body("/en").await;
        assert!(html.contains("toolnav-bar"));
        assert!(html.contains(r#"href="/TestRunner" aria-current="page""#));
        assert!(html.contains("Test Runner"));
        assert!(html.contains(r#"id="ok-btn""#));
        assert!(html.contains("Load a procedure"));
    }
}
