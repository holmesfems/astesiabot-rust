use super::*;
use crate::engine::risei_calculator_engine::{RiseiCalculatorEngine, Server};
use axum::http::{header, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use super::spec::{ENDPOINTS, ListKind};
use super::suggest::suggest;
use super::skill::{SKILL_MD_PATH, SKILL_ZIP_PATH};
use crate::engine::external_source::ExternalSourceRegistry;
use axum::body::Body;
use axum::http::Request;
use tokio::sync::OnceCell;
use tower::ServiceExt;

/// Seedのみ(ネットワーク無し)で組み立てたエンジン+レジストリをテスト間で共有する(初期計算が重いため)。
async fn state() -> RiseiApiState {
    static STATE: OnceCell<RiseiApiState> = OnceCell::const_new();
    STATE
        .get_or_init(|| async {
            let outer_source = Arc::new(ExternalSourceRegistry::load(true).await);
            let risei = Arc::new(RiseiCalculatorEngine::load(&outer_source).await.expect("seedから理性価値表を計算できる"));
            RiseiApiState { risei, external_source: outer_source }
        })
        .await
        .clone()
}

async fn engine() -> Arc<RiseiCalculatorEngine> {
    state().await.risei
}

async fn get(uri: &str) -> (StatusCode, Value) {
    let response = router::<()>(state().await)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("{uri}: JSONではない応答"));
    (status, body)
}

fn uri(path: &str, query: &str) -> String {
    format!("{path}?{}", query.replace(' ', "%20"))
}

#[tokio::test]
async fn index_lists_every_endpoint_and_category() {
    for path in ["/api/risei", "/api/risei/"] {
        let (status, body) = get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(body["endpoints"].as_array().unwrap().len(), ENDPOINTS.len());
        assert!(!body["material_categories"].as_array().unwrap().is_empty());
        assert_eq!(body["list_kinds"].as_array().unwrap().len(), ListKind::ALL.len());
        assert_eq!(body["operator_list_kinds"].as_array().unwrap().len(), 4);
        let paths: Vec<&str> = body["endpoints"].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap()).collect();
        for path in [
            "/api/risei/operators/mastery",
            "/api/risei/operators/elite",
            "/api/risei/operators/module",
            "/api/risei/operators/lists/{kind}",
        ] {
            assert!(paths.contains(&path), "{path}");
        }
    }
}

#[tokio::test]
async fn materials_accepts_key_and_japanese_name() {
    let (status, by_name) = get(&uri("/api/risei/materials", "target=源岩&limit=3")).await;
    assert_eq!(status, StatusCode::OK, "{by_name}");
    let stages = by_name["stages"].as_array().unwrap();
    assert!(!stages.is_empty() && stages.len() <= 3);
    assert!(stages[0].get("raw").is_none(), "xlsx用のrawは出さない");
    assert_eq!(by_name["server"], "global");

    let key = by_name["category_key"].as_str().unwrap();
    let (status, by_key) = get(&uri("/api/risei/materials", &format!("target={key}&limit=3"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(by_key["stages"], by_name["stages"]);
}

#[tokio::test]
async fn unknown_material_lists_available_categories() {
    let (status, body) = get(&uri("/api/risei/materials", "target=存在しない素材xyz")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body["available"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn stages_and_events_return_results() {
    let (status, body) = get(&uri("/api/risei/stages", "stage=1-7")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stages"][0]["name"], "1-7");

    let (status, body) = get(&uri("/api/risei/stages", "stage=1-7&server=mainland")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["effective_server"], "mainland");

    let (status, body) = get(&uri("/api/risei/events", "stage=SV-8")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["stages"][0]["zone_name"].is_string());
}

#[tokio::test]
async fn misspelled_stage_suggests_candidates() {
    // 大文字小文字違い → 候補に正しい表記が出る
    let (status, body) = get(&uri("/api/risei/events", "stage=sv-8")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let suggestions: Vec<&str> = body["did_you_mean"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(suggestions.iter().any(|s| s.starts_with("SV-8")), "{body}");

    // 恒常/イベントの取り違え → 正しいエンドポイントを案内する
    // (SV等の恒常化したサイドストーリーは両方にあるので、イベント側にしか無いものを選ぶ)
    let snapshot = engine().await.snapshot(Server::Global).await;
    let event_only = snapshot
        .stage_info
        .event_code_to_stage
        .keys()
        .filter(|code| code.is_ascii() && !code.contains(' '))
        .find(|code| {
            !snapshot.search_event_stage(code).is_empty() && snapshot.search_main_stage(code).is_empty()
        })
        .expect("イベント専用ステージがSeedにある")
        .clone();
    let (status, body) = get(&uri("/api/risei/stages", &format!("stage={event_only}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body["hint"].as_str().unwrap().contains("/api/risei/events"), "{body}");
}

#[tokio::test]
async fn lists_return_every_kind() {
    for kind in ListKind::ALL {
        if let ListKind::Kakin = kind {
            continue; // items ではなく baselines/packs。kakin_returns_global_comparison で見る
        }
        let (status, body) = get(&format!("/api/risei/lists/{}", kind.key())).await;
        assert_eq!(status, StatusCode::OK, "{}", kind.key());
        assert!(!body["items"].as_array().unwrap().is_empty(), "{}", kind.key());
    }
    let (status, body) = get("/api/risei/lists/te").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["did_you_mean"].as_array().unwrap().contains(&json!("te2")));
}

#[tokio::test]
async fn kakin_returns_global_comparison() {
    let (status, body) = get("/api/risei/lists/kakin").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["effective_server"], "global");
    assert!(body["updated_at"].is_string());
    assert_eq!(body["baselines"][0]["name"], "10000円恒常パック");
    assert_eq!(body["baselines"][0]["total_efficiency"], 1.0);
    // 期間限定パックは販売状況次第で0件もあり得る(販売期間外・price_kakin.yamlから削除)ので、
    // 件数には依存しない
    let packs = body["packs"].as_array().unwrap();
    let efficiencies: Vec<f64> = packs.iter().map(|p| p["total_efficiency"].as_f64().unwrap()).collect();
    assert!(efficiencies.windows(2).all(|w| w[0] >= w[1]), "総合効率の降順");
    assert!(packs.iter().all(|p| p["contents"].as_array().is_some_and(|c| !c.is_empty())));
    assert!(packs.iter().all(|p| p.get("note").is_none()));
    assert!(packs.iter().all(|p| p["upcoming"] == false), "既定では販売前のパックを出さない");

    let (status, body) = get(&uri("/api/risei/lists/kakin", "upcoming=true")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["packs"].as_array().unwrap().len() >= packs.len());
    let (status, _) = get(&uri("/api/risei/lists/kakin", "upcoming=yes")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get(&uri("/api/risei/lists/values", "upcoming=true")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "kakin 以外では受け付けない");

    let (status, body) = get(&uri("/api/risei/lists/kakin", "server=mainland")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["usage"]["params"].is_array());
}

#[tokio::test]
async fn bad_params_are_rejected_with_usage() {
    // 旧コマンドの引数名(is_global)は黙って無視しない
    let (status, body) = get(&uri("/api/risei/stages", "stage=1-7&is_global=false")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["usage"]["params"].is_array());

    let (status, body) = get(&uri("/api/risei/stages", "stage=1-7&sever=global")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["did_you_mean"], json!(["server"]));

    let (status, _) = get(&uri("/api/risei/stages", "stage=1-7&server=jp")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get("/api/risei/stages").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get(&uri("/api/risei/stages", "stage=1-7&limit=0")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// -- オペレーター消費素材 --------------------------------------------------

fn has_items(v: &Value) -> bool {
    v.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|i| i["name"].is_string() && i["count"].is_number()))
}

#[tokio::test]
async fn operator_mastery_returns_three_masteries_and_ranking() {
    let (status, body) = get(&uri("/api/risei/operators/mastery", "operator=ブレイズ&skill=2")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["operator_name"], "ブレイズ");
    assert_eq!(body["skill_num"], 2);
    assert_eq!(body["cn_only"], false);
    assert_eq!(body["values_server"], "global");
    assert!(body["updated_at"].is_string());
    let masteries = body["masteries"].as_array().unwrap();
    assert_eq!(masteries.len(), 3);
    assert!(masteries.iter().all(|m| has_items(&m["items"]) && m["risei_value"].as_f64().unwrap() > 0.0));
    assert!(has_items(&body["total"]["items"]) && has_items(&body["total_r2_items"]));
    assert_eq!(body["ranking"]["star"], 6);
    assert!(body["ranking"]["rank"].as_u64().unwrap() >= 1);
    assert!(body["ranking"]["total"].as_u64().unwrap() >= body["ranking"]["rank"].as_u64().unwrap());

    // 大陸版限定オペレーターは大陸版の理性価値で換算する
    let (status, body) = get(&uri("/api/risei/operators/mastery", "operator=焔狐竜オーキッド&skill=1")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cn_only"], true);
    assert_eq!(body["values_server"], "mainland");
}

#[tokio::test]
async fn operator_elite_and_module_return_costs() {
    let (status, body) = get(&uri("/api/risei/operators/elite", "operator=ブレイズ")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["phases"].as_array().unwrap().len(), 2);
    assert!(has_items(&body["total"]["items"]) && has_items(&body["total_r2_items"]));
    assert_eq!(body["ranking"]["star"], 6);

    let (status, body) = get(&uri("/api/risei/operators/module", "operator=ブレイズ")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let modules = body["modules"].as_array().unwrap();
    assert!(!modules.is_empty());
    for module in modules {
        assert!(module["header"].is_string());
        assert_eq!(module["phases"].as_array().unwrap().len(), 3);
        assert_eq!(module["phases"][0]["stage"], 1);
        assert!(has_items(&module["total_items"]) && has_items(&module["total_r2_items"]));
    }
}

#[tokio::test]
async fn unknown_operator_suggests_names() {
    for path in ["mastery", "elite", "module"] {
        let q = if path == "mastery" { "operator=ブレズ&skill=1" } else { "operator=ブレズ" };
        let (status, body) = get(&uri(&format!("/api/risei/operators/{path}"), q)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        let suggestions = body["did_you_mean"].as_array().unwrap();
        assert!(suggestions.contains(&json!("ブレイズ")), "{path}: {body}");
        assert!(body["usage"]["params"].is_array());
    }
}

#[tokio::test]
async fn operator_bad_args_return_json_errors() {
    let (status, body) = get(&uri("/api/risei/operators/mastery", "operator=ブレイズ&skill=4")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _) = get(&uri("/api/risei/operators/mastery", "operator=ブレイズ&skill=abc")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get(&uri("/api/risei/operators/mastery", "operator=ブレイズ")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get(&uri("/api/risei/operators/elite", "operator=ブレイズ&server=global")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // ★3は特化が無い(エンジン側のエラー。botと同じ文言)
    let (status, body) = get(&uri("/api/risei/operators/mastery", "operator=ハイビスカス&skill=1")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body["error"].as_str().unwrap().contains("特化は存在しません"));
    assert!(body.get("did_you_mean").is_none());
}

#[tokio::test]
async fn operator_lists_return_every_kind() {
    let (status, body) = get(&uri("/api/risei/operators/lists/elite", "star=6")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["values_server"], "mixed");
    assert_eq!(body["star"], 6);
    assert!(!body["entries"].as_array().unwrap().is_empty());
    assert_eq!(body["only_recent"], false);

    let (status, body) = get(&uri("/api/risei/operators/lists/mastery", "star=6")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mode"], "full");
    assert_eq!(body["top10_heaviest"].as_array().unwrap().len(), 10);

    let (status, body) = get(&uri("/api/risei/operators/lists/mastery", "star=5&only_recent=true")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mode"], "recent");
    assert_eq!(body["only_recent"], true);

    let (status, body) = get("/api/risei/operators/lists/unimplemented_total").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["values_server"], "mainland");
    assert!(has_items(&body["total_items"]) && body["total_risei_value"].as_f64().unwrap() > 0.0);
    assert!(!body["operator_names"].as_array().unwrap().is_empty());

    let (status, body) = get("/api/risei/operators/lists/implemented_total").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["values_server"], "global");
    assert!(has_items(&body["combined_r2_items"]));

    for path in ["/api/risei/operators/lists", "/api/risei/operators/lists/"] {
        let (status, body) = get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(body["operator_list_kinds"].as_array().unwrap().len(), 4);
    }
}

#[tokio::test]
async fn operator_lists_reject_bad_args() {
    let (status, body) = get("/api/risei/operators/lists/elite").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["usage"]["params"].is_array());
    let (status, _) = get(&uri("/api/risei/operators/lists/elite", "star=3")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get(&uri("/api/risei/operators/lists/elite", "star=6&only_recent=maybe")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // 引数を取らない種類にstar/only_recentを付けても黙って無視しない
    let (status, _) = get(&uri("/api/risei/operators/lists/implemented_total", "star=6")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get(&uri("/api/risei/operators/lists/unimplemented_total", "only_recent=true")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = get("/api/risei/operators/lists/eli").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["did_you_mean"].as_array().unwrap().contains(&json!("elite")));

    let (status, body) = get("/api/risei/operators/mastry").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["did_you_mean"][0], "/api/risei/operators/mastery");
}

#[tokio::test]
async fn unknown_path_suggests_endpoint() {
    let (status, body) = get("/api/risei/material").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["did_you_mean"][0], "/api/risei/materials");
}

/// run_apiと同じく web_ui_router()(skill_routerを含む) と router() を merge しても
/// パスが衝突せず、skill.zip と API の両方に届くこと。
#[tokio::test]
async fn skill_zip_coexists_with_api_router() {
    let app = crate::api::web_ui_router::<()>().merge(router::<()>(state().await));
    let response = app
        .clone()
        .oneshot(Request::builder().uri(SKILL_ZIP_PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/zip");
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"arknights-risei-api.zip\""
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("valid zip");
    assert_eq!(archive.len(), 1);
    let mut entry = archive.by_name("arknights-risei-api/SKILL.md").unwrap();
    let mut in_zip = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut in_zip).unwrap();
    assert_eq!(in_zip, std::fs::read(SKILL_MD_PATH).unwrap(), "zip内のSKILL.mdは正本とバイト一致する");

    let response = app
        .oneshot(Request::builder().uri("/api/risei/stages?stage=1-7").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[test]
fn suggest_prefers_prefix_then_distance() {
    assert_eq!(suggest("mat", ["materials", "stages"]), vec!["materials"]);
    assert_eq!(suggest("stage", ["stages", "events"]), vec!["stages"]);
    assert!(suggest("zzzzzz", ["stages"]).is_empty());
}

#[test]
fn suggest_skips_candidates_whose_length_differs_beyond_threshold() {
    // 文字数の差がちょうど閾値なら従来どおり編集距離で判定する
    assert_eq!(suggest("b", ["xbc"]), vec!["xbc"]);
    // 差が閾値を超える候補は編集距離も必ず閾値を超えるので、計算せずに落としても結果は同じ
    assert!(suggest("b", ["xbcd"]).is_empty());
    // 長大な入力でも前方一致・候補の長さに応じた計算だけで返る
    let long = "a".repeat(65_000);
    assert!(suggest(&long, ["1-7", "GT-HX-3", "/api/risei/stages"]).is_empty());
}
