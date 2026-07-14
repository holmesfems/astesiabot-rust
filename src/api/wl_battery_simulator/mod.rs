mod battery_sim;
mod optimizer;

use askama::Template;
use axum::extract::Multipart;
use axum::response::{Html, Redirect};
use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

use optimizer::OptimizationResult;

const STATIC_DIR: &str = "src/api/wl_battery_simulator/static";

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    required_power: Option<i64>,
    storage_margin: i64,
    use_margin_under_5: bool,
    error_html: String,
    result_html: String,
    chart_html: String,
}

#[derive(Template)]
#[template(path = "result.html")]
struct ResultTemplate<'a> {
    result: &'a OptimizationResult,
}

#[derive(Template)]
#[template(path = "chart.html")]
struct ChartTemplate<'a> {
    result: Option<&'a OptimizationResult>,
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate {
    error: Option<String>,
}

async fn index() -> Html<String> {
    let error_html = ErrorTemplate { error: None }.render().unwrap();
    let chart_html = ChartTemplate { result: None }.render().unwrap();
    let page = IndexTemplate {
        required_power: None,
        storage_margin: 10_000,
        use_margin_under_5: true,
        error_html,
        result_html: String::new(),
        chart_html,
    }
    .render()
    .unwrap();
    Html(page)
}

async fn calculate_redirect() -> Redirect {
    Redirect::to("/WLBatterySimulator")
}

fn render_error_fragment(error: String) -> Html<String> {
    let error_html = ErrorTemplate { error: Some(error) }.render().unwrap();
    Html(format!(
        r#"<div id="error" class="error" hx-swap-oob="outerHTML">
    {error_html}
</div>"#
    ))
}

fn render_result_fragment(result: OptimizationResult) -> Html<String> {
    let result_html = ResultTemplate { result: &result }.render().unwrap();
    let chart_html = ChartTemplate { result: Some(&result) }.render().unwrap();
    Html(format!(
        r#"<div id="error" class="error" hx-swap-oob="outerHTML" hidden="true">
</div>
<div id="result" class="result" hx-swap-oob="outerHTML">
    {result_html}
</div>
<section id="tvChart" class="card full" hx-swap-oob="outerHTML">
    {chart_html}
</section>"#
    ))
}

async fn calculate(mut multipart: Multipart) -> Html<String> {
    let mut required_power: Option<i64> = None;
    let mut storage_margin: Option<i64> = None;
    let mut use_margin_under_5 = false;
    let mut blueprint_id = String::from("...");

    while let Ok(Some(field)) = multipart.next_field().await {
        let Some(name) = field.name().map(|s| s.to_string()) else {
            continue;
        };
        let Ok(text) = field.text().await else {
            continue;
        };
        match name.as_str() {
            "required_power" => required_power = text.parse().ok(),
            "storage_margin" => storage_margin = text.parse().ok(),
            "use_margin_under_5" => use_margin_under_5 = matches!(text.as_str(), "on" | "true" | "1"),
            "blueprintId" => blueprint_id = text,
            _ => {}
        }
    }

    let (required_power, storage_margin) = match (required_power, storage_margin) {
        (Some(r), Some(s)) => (r, s),
        _ => return render_error_fragment("入力値が不正です。".to_string()),
    };

    match optimizer::optimize(required_power, storage_margin, use_margin_under_5, &blueprint_id) {
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
