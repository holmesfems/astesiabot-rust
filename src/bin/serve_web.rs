//! Web UI（WLBatterySimulator / EFRecipeCalculator / LodChestSolver / TestRunner）だけを
//! 配信する dev 用サーバー。
//!
//! bot（Discord接続）は起動せず、ExternalSourceRegistry も読まない。`.env` も要求しない
//! （`PUBLIC_BASE_URL` だけ任意で見る。canonical/hreflang/sitemap の絶対URLに使われる）。
//! Web UI のブラウザ実機テスト（`src/api/test_runner/e2e.mjs`）が spawn して使う想定。
//!
//! ルート定義は持たない。`astesiabot_rust::api::web_ui_router()` を使うことで、
//! 本番の `run_api`（src/api/mod.rs）とルート集合を必ず一致させる。
//! `/FrameKillCalculator/catalog.json` だけは`AppState`依存のデータ取得が要るため
//! `web_ui_router()`に入れられず、`fk_kill_calculator::catalog_router()`を別途
//! `merge`している（`run_api`側も同様。データの中身だけがSeed直読み/最新fetchで異なる）。
//!
//! `data/` や `src/api/*/static` を相対パスで読むため、プロジェクトルートで実行する前提
//! （`cargo run --bin serve_web` を本体の `cargo run` と同じ場所から叩けばよい）。
//!
//! ポート: 環境変数 `WEB_UI_PORT`（既定 3001。本番既定の3000と衝突させず、
//! `cargo run` と同時に立てられるようにしている）。bind は 127.0.0.1 のみ（外部公開しない）。

use astesiabot_rust::api::fk_kill_calculator::{self, CatalogProvider};
use astesiabot_rust::api::web_ui_router;
use astesiabot_rust::engine::external_source::{fk_data, operator_combat, operator_data, skill_data};
use astesiabot_rust::engine::fk_kill_calc::{self, Overrides};
use axum::http::StatusCode;
use std::sync::Arc;

async fn not_found() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "404 Not Found")
}

/// `data/seed/*.json`を直接読んで`T`にdeserializeする（`serve_web`は起動時fetchも
/// `.env`も要求しないため、`Source::load`は使わずSeedを直読みする）。
fn load_seed<T: serde::de::DeserializeOwned>(path: &str) -> T {
    let s = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("seed({path})の読み込みに失敗: {e}"));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("seed({path})のparseに失敗: {e}"))
}

/// フレームキル計算機のカタログを起動時に1回だけ`data/seed/*.json`から組み立てて、
/// 以後は同じ`Arc`を返し続けるプロバイダ（devサーバーなので鮮度管理は不要。
/// 本番の`run_api`はExternalSourceRegistryの現在値からリクエスト毎に組み立てる）。
fn seed_based_catalog_provider() -> CatalogProvider {
    let fk = load_seed(fk_data::SEED_PATH);
    let ops = load_seed(operator_data::SEED_PATH);
    let combat = load_seed(operator_combat::SEED_PATH);
    let skills = load_seed(skill_data::SEED_PATH);
    let build = fk_kill_calc::build_catalog(&fk, &ops, &combat, &skills, Overrides::global());
    let catalog = Arc::new(build.catalog);
    Arc::new(move || {
        let catalog = catalog.clone();
        Box::pin(async move { catalog })
    })
}

#[tokio::main]
async fn main() {
    let port = std::env::var("WEB_UI_PORT")
        .unwrap_or_else(|_| "3001".to_string())
        .parse::<u16>()
        .expect("WEB_UI_PORT must be a number");

    let app = web_ui_router::<()>()
        .merge(fk_kill_calculator::catalog_router::<()>(seed_based_catalog_provider()))
        .fallback(not_found);

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
    println!("  http://127.0.0.1:{port}/FrameKillCalculator/catalog.json");

    axum::serve(listener, app).await.unwrap();
}
