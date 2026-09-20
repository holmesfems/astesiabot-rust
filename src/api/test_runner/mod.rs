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
//! HTML+JS 本体は完全自己完結だが、進捗URL共有（`#state=<lz-string圧縮JSON>`）の
//! ために lz-string 1.5.0（MIT）を `static/lz-string.min.js` として同梱し、
//! `/TestRunner/static` から同一オリジンで配信する（CDN参照はせず、オフライン動作を維持）。
//! 進捗はサーバーへ送らず、フロントエンドが window.location.hash だけで復元する。

use askama::Template;
use axum::http::HeaderMap;
use axum::response::{Html, Redirect};
use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

const STATIC_DIR: &str = "src/api/test_runner/static";

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
        assert!(html.contains(r#"href="/TestRunner" aria-current="page""#));
        assert!(html.contains("試験手順ランナー"));
        assert!(html.contains(r#"id="ok-btn""#));
        assert!(html.contains("手順を読み込む"));
        assert!(html.contains("🚀 開始する"));
        assert!(!html.contains("paste-hint"));

        // "用語定義" はパーサーが手順書内で認識するキーワード（body の静的マークアップには出てこない）。
        // Step 1 で main.js へ、Step 2 で core/parser.js へ移ったので、その先を直接確認する。
        let js = get_body("/static/js/core/parser.js").await;
        assert!(js.contains("用語定義"));
    }

    /// 試験手順だけを共有するボタン（記録なし・テスター名なし）が両言語・3画面に揃っていること。
    #[tokio::test]
    async fn both_pages_have_share_procedure_ui() {
        for uri in ["/", "/en"] {
            let html = get_body(uri).await;
            assert!(html.contains(r#"id="share-procedure-start-btn""#), "{uri}");
            assert!(html.contains(r#"id="share-procedure-step-btn""#), "{uri}");
            assert!(html.contains(r#"id="share-procedure-result-btn""#), "{uri}");
        }
    }

    /// 結果URL共有UI（結果画面の共有ボタン・テスター名入力・lz-string読み込み）が両言語に揃っていること。
    /// テスター名入力は開始前の確認モーダル（id="modal-confirm"）に統合済みで、
    /// 独立した id="modal-tester-name" は存在しない（Step 1 搬出時点で実態に合わせて修正）。
    /// 進捗URLコピーはこの結果画面ボタン1箇所のみ（開始画面/ステップ画面の重複ボタン・保存して中断は撤去済み）。
    #[tokio::test]
    async fn both_pages_have_progress_url_sharing_ui() {
        for uri in ["/", "/en"] {
            let html = get_body(uri).await;
            assert!(html.contains(r#"src="/TestRunner/static/lz-string.min.js""#), "{uri}");
            assert!(html.contains(r#"id="share-url-result-btn""#), "{uri}");
            assert!(html.contains(r#"id="modal-confirm""#), "{uri}");
            assert!(html.contains(r#"id="tester-name-input""#), "{uri}");
            assert!(!html.contains(r#"id="save-pause-btn""#), "{uri}");
        }
    }

    #[tokio::test]
    async fn static_serves_vendored_lz_string() {
        let js = get_body("/static/lz-string.min.js").await;
        assert!(js.contains("lz-string 1.5.0"));
        assert!(js.contains("compressToEncodedURIComponent"));
    }

    /// Step 1 搬出: CSS/JS がテンプレ埋め込みではなく static/ から配信されること。
    #[tokio::test]
    async fn static_serves_extracted_style_and_main_js() {
        let css = get_body("/static/style.css").await;
        assert!(css.contains("toolnav-bar") || css.contains(":root"));

        let js = get_body("/static/js/main.js").await;
        assert!(js.contains("export function boot("));
        assert!(js.contains("installI18n"));
    }

    /// Step 2 分割: 全モジュールが static/ から配信され、それぞれの責務が入っていること。
    /// ES module は import を1本書き忘れても読み込み時には落ちず、その関数が呼ばれる瞬間まで
    /// 表面化しないので、ここでは「配信されること」と「中身が期待した層のものであること」だけを見る。
    /// 未定義参照の全数チェックは別途 static 解析で行う（cargo test では検出できない）。
    #[tokio::test]
    async fn static_serves_all_split_modules() {
        // core/ は DOM 非依存であること（document を掴んでいたら層の切り分けが壊れている）
        for (path, needle) in [
            ("core/parser.js", "export function parseProcedure("),
            ("core/score.js", "export function getPraiseForCombo("),
            ("core/state.js", "export var state ="),
            ("core/io.js", "export function buildShareUrl("),
        ] {
            let js = get_body(&format!("/static/js/{path}")).await;
            assert!(js.contains(needle), "{path}");
            assert!(!js.contains("document."), "{path} must stay DOM-free");
        }

        for (path, needle) in [
            ("ui/dom.js", "export function el("),
            ("ui/renderer.js", "export function renderStep("),
            ("ui/modal.js", "export function openConfirmModal("),
            ("ui/flow.js", "export function recordResult("),
            ("ui/effects/confetti.js", "export function spawnConfetti("),
            ("ui/effects/dodge.js", "export function wireNgDodge("),
            ("constants/config.js", "export const STORAGE_KEY ="),
        ] {
            let js = get_body(&format!("/static/js/{path}")).await;
            assert!(js.contains(needle), "{path}");
        }
    }

    /// Step 1 搬出: 文言/演出/サンプルの constants が言語ごとに配信されること。
    #[tokio::test]
    async fn static_serves_i18n_constants_per_language() {
        let strings_ja = get_body("/static/js/constants/strings.ja.js").await;
        assert!(strings_ja.contains("用語解説"));

        let strings_en = get_body("/static/js/constants/strings.en.js").await;
        assert!(strings_en.contains("Term explanation"));

        let phrases_ja = get_body("/static/js/constants/phrases.ja.js").await;
        assert!(phrases_ja.contains("export const PRAISE_POOL"));

        let phrases_en = get_body("/static/js/constants/phrases.en.js").await;
        assert!(phrases_en.contains("export const PRAISE_POOL"));

        let samples_ja = get_body("/static/js/constants/samples.ja.js").await;
        assert!(samples_ja.contains("ぽもどーろ君"));

        let samples_en = get_body("/static/js/constants/samples.en.js").await;
        assert!(samples_en.contains("Pomodorin"));
    }

    /// Step 1 搬出: 両ページとも <style> は外部CSSへのリンクに、<script> はブートストラップに
    /// 置き換わっており、ページごとに正しい言語の constants を import していること。
    #[tokio::test]
    async fn both_pages_link_stylesheet_and_import_own_language_constants() {
        let ja = get_body("/").await;
        assert!(ja.contains(r#"<link rel="stylesheet" href="/TestRunner/static/style.css">"#));
        assert!(ja.contains(r#"import { boot } from "/TestRunner/static/js/main.js";"#));
        assert!(ja.contains("constants/strings.ja.js"));
        assert!(ja.contains("constants/phrases.ja.js"));
        assert!(ja.contains("constants/samples.ja.js"));
        assert!(!ja.contains("constants/strings.en.js"));

        let en = get_body("/en").await;
        assert!(en.contains(r#"<link rel="stylesheet" href="/TestRunner/static/style.css">"#));
        assert!(en.contains(r#"import { boot } from "/TestRunner/static/js/main.js";"#));
        assert!(en.contains("constants/strings.en.js"));
        assert!(en.contains("constants/phrases.en.js"));
        assert!(en.contains("constants/samples.en.js"));
        assert!(!en.contains("constants/strings.ja.js"));
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
