//! サイトのトップページ。F鯖（アークナイツDiscordサーバー）とアステシアちゃんbotの
//! 紹介、各ツールへの入口をまとめる。
//!
//! 日本語のみ・1URL（`/`）。LodChestSolver/TestRunnerのような言語別URL分割はしない。
//! F鯖向けの案内が主目的で、汎用ツールのようにクローラへ多言語を見せる必要が薄いため。
//! CSSは分量が小さいので `templates_shared/toolnav.html` と同様にページ内に直書きしている。
//! `static/` は OGP画像(ogp.png)と星空背景のスクリプト(starfield.js)、オープニングの
//! スクリプト(opening.js)の配信用。どちらのJSも分量が大きいのでテンプレートから切り出している。
//! オープニング（星の細剣のエンブレム）のSVGは `templates/home_opening.html` を include して
//! サーバー側で埋め込む（JS無効でも本文はそのまま見える。再生の要否は home_index.html の <head>）。

use askama::Template;
use axum::http::HeaderMap;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

const STATIC_DIR: &str = "src/api/home/static";

#[derive(Template)]
#[template(path = "home_index.html")]
struct IndexTemplate {
    base: String,
    active_tool: &'static str,
    lang: &'static str,
}

async fn index(headers: HeaderMap) -> Html<String> {
    let page = IndexTemplate {
        base: super::base_url(&headers),
        active_tool: "home",
        lang: "ja",
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
        .route("/", get(index))
        .nest_service("/static", ServeDir::new(STATIC_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn index_renders_toolnav_and_key_content() {
        let app = router::<()>();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("toolnav-bar"));
        assert!(html.contains(r#"href="/" aria-current="page""#));
        assert!(html.contains("discord.gg/arknightsflame"));
        assert!(html.contains("href=\"/WLBatterySimulator\""));
        assert!(html.contains("href=\"/EFRecipeCalculator\""));
        assert!(html.contains("href=\"/LodChestSolver\""));
        assert!(html.contains("href=\"/TestRunner\""));
        assert!(html.contains("<h2>エンドフィールド</h2>"));
        assert!(html.contains("<h2>他の趣味ツール</h2>"));
        assert!(html.contains(r#"<script src="/static/starfield.js" defer></script>"#));
        assert!(html.contains(r#"<script src="/static/opening.js" defer></script>"#));
        assert!(html.contains(r#"<div id="opening" aria-hidden="true">"#));
        assert!(html.contains(r#"<svg id="emblem""#));
    }

    #[tokio::test]
    async fn static_scripts_are_served() {
        for uri in ["/static/starfield.js", "/static/opening.js"] {
            let app = router::<()>();
            let response = app
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
        }
    }
}
