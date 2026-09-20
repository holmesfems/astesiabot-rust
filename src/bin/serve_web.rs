//! Web UI（WLBatterySimulator / EFRecipeCalculator / LodChestSolver / TestRunner）だけを
//! 配信する dev 用サーバー。
//!
//! bot（Discord接続）は起動せず、ExternalSourceRegistry も読まない。`.env` も要求しない
//! （`PUBLIC_BASE_URL` だけ任意で見る。canonical/hreflang/sitemap の絶対URLに使われる）。
//! Web UI のブラウザ実機テスト（`src/api/test_runner/e2e.mjs`）が spawn して使う想定。
//!
//! ルート定義は持たない。`astesiabot_rust::api::web_ui_router()` を使うことで、
//! 本番の `run_api`（src/api/mod.rs）とルート集合を必ず一致させる。
//!
//! `data/` や `src/api/*/static` を相対パスで読むため、プロジェクトルートで実行する前提
//! （`cargo run --bin serve_web` を本体の `cargo run` と同じ場所から叩けばよい）。
//!
//! ポート: 環境変数 `WEB_UI_PORT`（既定 3001。本番既定の3000と衝突させず、
//! `cargo run` と同時に立てられるようにしている）。bind は 127.0.0.1 のみ（外部公開しない）。

use astesiabot_rust::api::web_ui_router;
use axum::http::StatusCode;

async fn not_found() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "404 Not Found")
}

#[tokio::main]
async fn main() {
    let port = std::env::var("WEB_UI_PORT")
        .unwrap_or_else(|_| "3001".to_string())
        .parse::<u16>()
        .expect("WEB_UI_PORT must be a number");

    let app = web_ui_router::<()>().fallback(not_found);

    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .unwrap_or_else(|e| panic!("failed to bind 127.0.0.1:{port}: {e}"));

    println!("serve_web: Web UI only (no bot, no ExternalSourceRegistry)");
    println!("  http://127.0.0.1:{port}/health");
    println!("  http://127.0.0.1:{port}/WLBatterySimulator");
    println!("  http://127.0.0.1:{port}/EFRecipeCalculator");
    println!("  http://127.0.0.1:{port}/LodChestSolver  /  /LodChestSolver/en");
    println!("  http://127.0.0.1:{port}/TestRunner  /  /TestRunner/en");

    axum::serve(listener, app).await.unwrap();
}
