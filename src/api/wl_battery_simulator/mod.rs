//! 武陵発電制御計算機のページ。計算と結果の表示はブラウザ側
//! （`static/optimizer.js`・`static/battery_sim.js`・`static/render.js`）で行い、
//! サーバーはページと静的ファイルを返すだけ。

use askama::Template;
use axum::http::HeaderMap;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

const STATIC_DIR: &str = "src/api/wl_battery_simulator/static";

#[derive(Template)]
#[template(path = "wl_index.html")]
struct IndexTemplate {
    /// OGP(og:url / og:image)用の絶対URLの起点（例: https://example.com）。
    base: String,
    /// ツール切り替えヘッダー(templates_shared/toolnav.html)用。
    active_tool: &'static str,
    lang: &'static str,
}

async fn index(headers: HeaderMap) -> Html<String> {
    let page = IndexTemplate {
        base: super::base_url(&headers),
        active_tool: "wl",
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
    async fn index_page_renders_active_toolnav_chip() {
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
        assert!(html.contains(r#"href="/WLBatterySimulator" aria-current="page""#));
        // 計算はブラウザの ES モジュールで行い、サーバーには送らない。
        assert!(html.contains(r#"<script type="module" src="/WLBatterySimulator/static/app.js?v=1">"#));
        assert!(!html.contains("htmx"));
    }
}
