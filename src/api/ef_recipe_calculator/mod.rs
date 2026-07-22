mod solver;

use askama::Template;
use axum::extract::Multipart;
use axum::response::{Html, Redirect};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use tower_http::services::ServeDir;

use solver::CalcResult;

const STATIC_DIR: &str = "src/api/ef_recipe_calculator/static";

/// フォーム送信用のまとめ型。RecipeSet + CalcRequest を単一の `payload` フィールド
/// (JSON文字列)で受ける(可変長ネスト構造のため個別フィールド方式は使わない)。
#[derive(Deserialize)]
struct CalcInput {
    recipe_set: solver::RecipeSet,
    request: solver::CalcRequest,
}

// --- 表示用View(整形はここに置く。solver::CalcResultはDTOのまま保つ) ---

struct MaterialView {
    item: String,
    rate: String,
}

fn material_view(n: &solver::MaterialNeed) -> MaterialView {
    MaterialView {
        item: n.item.clone(),
        rate: format!("{:.2}", n.rate_per_min),
    }
}

struct MinedUsageView {
    item: String,
    used: String,
    surplus: String,
    cap: String,
}

struct StepView {
    recipe_name: String,
    equipment_name: String,
    machine_count: u64,
    limiting_output: String,
    utilization_pct: String,
    depth: u32,
    outputs: Vec<MaterialView>,
    inputs: Vec<MaterialView>,
    operating: Vec<MaterialView>,
}

fn step_view(s: &solver::CalcStep) -> StepView {
    StepView {
        recipe_name: s.recipe_name.clone(),
        equipment_name: s.equipment_name.clone(),
        machine_count: s.machine_count,
        limiting_output: s.limiting_output.clone(),
        utilization_pct: format!("{:.1}", s.utilization * 100.0),
        depth: s.depth,
        outputs: s.outputs_effective.iter().map(material_view).collect(),
        inputs: s.inputs_demand.iter().map(material_view).collect(),
        operating: s.operating_demand.iter().map(material_view).collect(),
    }
}

#[derive(Template)]
#[template(path = "ef_index.html")]
struct IndexTemplate {
    error_html: String,
    steps_html: String,
    raw_materials_html: String,
    mined_usage_html: String,
    byproduct_surplus_html: String,
    bottleneck_html: String,
    warnings_html: String,
}

#[derive(Template)]
#[template(path = "ef_error.html")]
struct ErrorTemplate {
    error: Option<String>,
}

#[derive(Template)]
#[template(path = "ef_steps.html")]
struct StepsTemplate {
    steps: Vec<StepView>,
}

#[derive(Template)]
#[template(path = "ef_raw_materials.html")]
struct RawMaterialsTemplate {
    materials: Vec<MaterialView>,
}

#[derive(Template)]
#[template(path = "ef_mined_usage.html")]
struct MinedUsageTemplate {
    usages: Vec<MinedUsageView>,
}

#[derive(Template)]
#[template(path = "ef_byproduct_surplus.html")]
struct ByproductSurplusTemplate {
    surplus: Vec<MaterialView>,
}

#[derive(Template)]
#[template(path = "ef_bottleneck.html")]
struct BottleneckTemplate {
    bottleneck: Option<String>,
}

#[derive(Template)]
#[template(path = "ef_warnings.html")]
struct WarningsTemplate {
    warnings: Vec<String>,
}

async fn index() -> Html<String> {
    let page = IndexTemplate {
        error_html: (ErrorTemplate { error: None }).render().unwrap(),
        steps_html: (StepsTemplate { steps: vec![] }).render().unwrap(),
        raw_materials_html: (RawMaterialsTemplate { materials: vec![] }).render().unwrap(),
        mined_usage_html: (MinedUsageTemplate { usages: vec![] }).render().unwrap(),
        byproduct_surplus_html: (ByproductSurplusTemplate { surplus: vec![] }).render().unwrap(),
        bottleneck_html: (BottleneckTemplate { bottleneck: None }).render().unwrap(),
        warnings_html: (WarningsTemplate { warnings: vec![] }).render().unwrap(),
    }
    .render()
    .unwrap();
    Html(page)
}

async fn calculate_redirect() -> Redirect {
    Redirect::to("/EFRecipeCalculator")
}

fn render_error_fragment(error: String) -> Html<String> {
    let error_html = (ErrorTemplate { error: Some(error) }).render().unwrap();
    Html(format!(
        r#"<div id="error" class="error" hx-swap-oob="outerHTML">
    {error_html}
</div>"#
    ))
}

fn render_result_fragment(result: CalcResult) -> Html<String> {
    let steps_html = (StepsTemplate {
        steps: result.steps.iter().map(step_view).collect(),
    })
    .render()
    .unwrap();
    let raw_materials_html = (RawMaterialsTemplate {
        materials: result.raw_materials.iter().map(material_view).collect(),
    })
    .render()
    .unwrap();
    let mined_usage_html = (MinedUsageTemplate {
        usages: result
            .mined_usage
            .iter()
            .map(|m| MinedUsageView {
                item: m.item.clone(),
                used: format!("{:.2}", m.used_rate),
                surplus: format!("{:.2}", m.surplus_rate),
                cap: format!("{:.2}", m.cap_rate),
            })
            .collect(),
    })
    .render()
    .unwrap();
    let byproduct_surplus_html = (ByproductSurplusTemplate {
        surplus: result.byproduct_surplus.iter().map(material_view).collect(),
    })
    .render()
    .unwrap();
    let bottleneck_html = (BottleneckTemplate {
        bottleneck: result.bottleneck.clone(),
    })
    .render()
    .unwrap();
    let warnings_empty = result.warnings.is_empty();
    let warnings_html = (WarningsTemplate {
        warnings: result.warnings.clone(),
    })
    .render()
    .unwrap();
    let warnings_hidden = if warnings_empty { r#" hidden="true""# } else { "" };

    Html(format!(
        r#"<div id="error" class="error" hx-swap-oob="outerHTML" hidden="true"></div>
<div id="steps" class="card" hx-swap-oob="outerHTML">
    {steps_html}
</div>
<div id="raw-materials" class="card" hx-swap-oob="outerHTML">
    {raw_materials_html}
</div>
<div id="mined-usage" class="card" hx-swap-oob="outerHTML">
    {mined_usage_html}
</div>
<div id="byproduct-surplus" class="card" hx-swap-oob="outerHTML">
    {byproduct_surplus_html}
</div>
<div id="bottleneck" class="card" hx-swap-oob="outerHTML">
    {bottleneck_html}
</div>
<div id="warnings" class="card" hx-swap-oob="outerHTML"{warnings_hidden}>
    {warnings_html}
</div>"#
    ))
}

async fn calculate(mut multipart: Multipart) -> Html<String> {
    let mut payload: Option<String> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        let Some(name) = field.name().map(|s| s.to_string()) else {
            continue;
        };
        let Ok(text) = field.text().await else {
            continue;
        };
        if name == "payload" {
            payload = Some(text);
        }
    }
    let Some(payload) = payload else {
        return render_error_fragment("入力がありません。".to_string());
    };
    let input: CalcInput = match serde_json::from_str(&payload) {
        Ok(v) => v,
        Err(e) => return render_error_fragment(format!("JSON解析に失敗: {e}")),
    };

    match solver::calculate(&input.recipe_set, &input.request) {
        Ok(result) => render_result_fragment(result),
        Err(error) => render_error_fragment(error),
    }
}

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(index))
        .route("/calculate", get(calculate_redirect).post(calculate))
        .nest_service("/static", ServeDir::new(STATIC_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    /// 同梱プリセット(static/presets.json)がRecipeSetの実際の型と食い違っていないかを
    /// 検証する(フロントJSはこのファイルをfetchしてそのまま送信するため、型ドリフトの
    /// 早期検知にはこのテストが唯一の手段)。
    #[derive(Deserialize)]
    struct PresetsFile {
        presets: Vec<PresetEntry>,
    }
    #[derive(Deserialize)]
    struct PresetEntry {
        name: String,
        recipe_set: solver::RecipeSet,
        default_target_item: String,
        default_target_rate_per_min: f64,
    }

    #[test]
    fn bundled_presets_deserialize_and_calculate_without_error() {
        let raw = std::fs::read_to_string("src/api/ef_recipe_calculator/static/presets.json")
            .expect("presets.json を読めること");
        let file: PresetsFile = serde_json::from_str(&raw).expect("RecipeSetの型と一致すること");
        assert!(!file.presets.is_empty());
        for preset in &file.presets {
            let req = solver::CalcRequest {
                target_item: preset.default_target_item.clone(),
                target_rate_per_min: preset.default_target_rate_per_min,
            };
            let result = solver::calculate(&preset.recipe_set, &req)
                .unwrap_or_else(|e| panic!("プリセット『{}』の計算に失敗: {e}", preset.name));
            assert!(
                result.warnings.is_empty(),
                "プリセット『{}』でwarning: {:?}",
                preset.name,
                result.warnings
            );
        }
    }

    fn multipart_body(payload: &str) -> (String, Vec<u8>) {
        let boundary = "----efrecipecalculatortestboundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"payload\"\r\n\r\n{payload}\r\n--{boundary}--\r\n"
        );
        (boundary.to_string(), body.into_bytes())
    }

    #[tokio::test]
    async fn index_page_renders() {
        let app = router::<()>();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn calculate_endpoint_returns_result_fragment_for_valid_payload() {
        let payload = serde_json::json!({
            "recipe_set": {
                "recipes": [{
                    "id": "r1",
                    "name": "テストレシピ",
                    "cycle_seconds": 2.0,
                    "outputs": [{"item": "製品", "qty": 1.0}],
                    "inputs": [{"item": "原料", "qty": 1.0}],
                    "operating_costs": []
                }],
                "selected_recipe_ids": ["r1"],
                "raw_items": [],
                "external_supplies": []
            },
            "request": { "target_item": "製品", "target_rate_per_min": 30.0 }
        })
        .to_string();
        let (boundary, body) = multipart_body(&payload);

        let app = router::<()>();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/calculate")
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("id=\"steps\""));
        assert!(html.contains("テストレシピ"));
        assert!(html.contains("原料"));
        assert!(!html.contains("反復が収束しませんでした"));
    }

    #[tokio::test]
    async fn calculate_endpoint_returns_error_fragment_for_missing_payload() {
        let boundary = "----efrecipecalculatortestboundary2".to_string();
        let app = router::<()>();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/calculate")
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(format!("--{boundary}--\r\n")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("入力がありません"));
    }
}
