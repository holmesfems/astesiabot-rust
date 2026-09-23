//! サイトのトップページ。F鯖（アークナイツDiscordサーバー）とアステシアちゃんbotの
//! 紹介、各ツールへの入口をまとめる。
//!
//! 日本語のみ・1URL（`/`）。LodChestSolver/TestRunnerのような言語別URL分割はしない。
//! F鯖向けの案内が主目的で、汎用ツールのようにクローラへ多言語を見せる必要が薄いため。
//! CSSは分量が小さいので `templates_shared/toolnav.html` と同様にページ内に直書きし、
//! 他ツールのような専用 `static/` ディレクトリは持たない。

use askama::Template;
use axum::http::HeaderMap;
use axum::response::Html;
use axum::routing::get;
use axum::Router;

// 銀河背景の回転中心(画面右上の角からさらに外側)の比率。CSSの光の中心とJSの星の
// 回転中心が同じ点になるよう、この2つの定数だけを唯一の情報源にしてテンプレートへ
// 両方(JS用の比率そのもの/CSS用の%表記)を渡す。値を変える時はここだけ直せばよい。
const GALAXY_CENTER_X_RATIO: f64 = 1.3;
const GALAXY_CENTER_Y_RATIO: f64 = -0.3;

#[derive(Template)]
#[template(path = "home_index.html")]
struct IndexTemplate {
    base: String,
    active_tool: &'static str,
    lang: &'static str,
    galaxy_cx_ratio: String,
    galaxy_cy_ratio: String,
    galaxy_cx_pct: String,
    galaxy_cy_pct: String,
}

async fn index(headers: HeaderMap) -> Html<String> {
    let page = IndexTemplate {
        base: super::base_url(&headers),
        active_tool: "home",
        lang: "ja",
        galaxy_cx_ratio: GALAXY_CENTER_X_RATIO.to_string(),
        galaxy_cy_ratio: GALAXY_CENTER_Y_RATIO.to_string(),
        galaxy_cx_pct: format!("{}%", GALAXY_CENTER_X_RATIO * 100.0),
        galaxy_cy_pct: format!("{}%", GALAXY_CENTER_Y_RATIO * 100.0),
    }
    .render()
    .unwrap();
    Html(page)
}

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/", get(index))
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
    }
}
