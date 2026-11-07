//! 共通のエラー応答とクエリ引数の検証。

use crate::engine::risei_calculator_engine::Server;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use super::spec::{EndpointSpec, LISTS, endpoint_usage};
use super::suggest::suggest;

/// 全エラー共通のJSON応答。
pub(super) struct ApiError {
    pub(super) status: StatusCode,
    pub(super) body: Value,
}

impl ApiError {
    pub(super) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, body: json!({ "error": message.into() }) }
    }

    pub(super) fn with(mut self, key: &str, value: impl Serialize) -> Self {
        self.body[key] = json!(value);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub(super) type ApiResult = Result<Json<Value>, ApiError>;

pub(super) type Params = Query<HashMap<String, String>>;

pub(super) fn rfc3339(time: chrono::DateTime<chrono::Utc>) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(super) fn check_params(params: &HashMap<String, String>, endpoint: &EndpointSpec) -> Result<(), ApiError> {
    let mut unknown: Vec<&String> = params
        .keys()
        .filter(|key| !endpoint.params.iter().any(|p| p.name == key.as_str()))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    unknown.sort();
    let did_you_mean: Vec<String> = unknown
        .iter()
        .flat_map(|key| suggest(key, endpoint.params.iter().map(|p| p.name)))
        .collect();
    Err(ApiError::new(
        StatusCode::BAD_REQUEST,
        format!(
            "未知のクエリ引数です: {}",
            unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ),
    )
    .with("did_you_mean", did_you_mean)
    .with("usage", endpoint_usage(endpoint)))
}

pub(super) fn required<'a>(
    params: &'a HashMap<String, String>,
    name: &str,
    endpoint: &EndpointSpec,
) -> Result<&'a str, ApiError> {
    match params.get(name).map(|v| v.trim()) {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(ApiError::new(StatusCode::BAD_REQUEST, format!("必須引数 {name} がありません"))
            .with("usage", endpoint_usage(endpoint))),
    }
}

pub(super) fn parse_server(params: &HashMap<String, String>, endpoint: &EndpointSpec) -> Result<Server, ApiError> {
    match params.get("server").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("global") => Ok(Server::Global),
        Some("mainland") => Ok(Server::Mainland),
        Some(other) => Err(ApiError::new(StatusCode::BAD_REQUEST, format!("server は global か mainland です: {other}"))
            .with("did_you_mean", suggest(other, ["global", "mainland"]))
            .with("usage", endpoint_usage(endpoint))),
    }
}

pub(super) fn parse_upcoming(params: &HashMap<String, String>) -> Result<bool, ApiError> {
    match params.get("upcoming").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(other) => Err(ApiError::new(StatusCode::BAD_REQUEST, format!("upcoming は true か false です: {other}"))
            .with("usage", endpoint_usage(&LISTS))),
    }
}

pub(super) fn parse_limit(params: &HashMap<String, String>, endpoint: &EndpointSpec) -> Result<Option<usize>, ApiError> {
    match params.get("limit").map(|v| v.trim()) {
        None | Some("") => Ok(None),
        Some(v) => match v.parse::<usize>() {
            Ok(n) if n > 0 => Ok(Some(n)),
            _ => Err(ApiError::new(StatusCode::BAD_REQUEST, format!("limit は1以上の整数です: {v}"))
                .with("usage", endpoint_usage(endpoint))),
        },
    }
}

pub(super) fn truncate<T>(list: &mut Vec<T>, limit: Option<usize>) {
    if let Some(limit) = limit {
        list.truncate(limit);
    }
}
