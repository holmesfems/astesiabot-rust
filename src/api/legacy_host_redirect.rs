//! 旧ホスト（`*.herokuapp.com` / `www.<正規ホスト>`）から正規ドメインへの 301 リダイレクト。
//!
//! 正規ドメインは `PUBLIC_BASE_URL`（例: `https://astesiabot.com`）。未設定なら何もしない。
//! 対象は GET/HEAD のみ。POST をリダイレクトするとクライアントが GET に化けさせて
//! 本文を捨てる（→405）か、追従せず空応答になるため、API は旧ホストのまま動かす。
//! 念のため API（ショートカットから叩かれる `/recruitment/`）と死活監視用の `/health` は
//! メソッドに関係なくパスでも除外する。

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

/// リダイレクトしないパス（旧ホストでもそのまま応答する）。
const EXCLUDED_PATHS: &[&str] = &["/recruitment/", "/health"];

/// `PUBLIC_BASE_URL` を正規化したもの。`run_api` 起動時に1回だけ読む。
#[derive(Clone)]
pub(crate) struct CanonicalBase {
    /// 末尾スラッシュなしの起点（例: `https://astesiabot.com`）。
    base: String,
    /// ホスト部（例: `astesiabot.com`）。`www.` 付きの判定に使う。
    host: String,
}

impl CanonicalBase {
    pub(crate) fn new(base: &str) -> Option<Self> {
        let base = base.trim().trim_end_matches('/');
        let host = base.split_once("://")?.1.split('/').next()?;
        let host = strip_port(host).to_ascii_lowercase();
        if host.is_empty() {
            return None;
        }
        Some(Self {
            base: base.to_string(),
            host,
        })
    }

    pub(crate) fn from_env() -> Option<Self> {
        Self::new(&std::env::var("PUBLIC_BASE_URL").ok()?)
    }
}

fn strip_port(host: &str) -> &str {
    host.split(':').next().unwrap_or(host)
}

/// リダイレクトすべきなら飛び先の絶対URLを返す。
fn redirect_target(
    canonical: &CanonicalBase,
    method: &Method,
    host: Option<&str>,
    path_and_query: &str,
) -> Option<String> {
    if method != Method::GET && method != Method::HEAD {
        return None;
    }
    let path = path_and_query.split('?').next().unwrap_or(path_and_query);
    if EXCLUDED_PATHS.contains(&path) {
        return None;
    }
    let host = strip_port(host?).to_ascii_lowercase();
    let is_legacy = host.ends_with(".herokuapp.com")
        || host.strip_prefix("www.") == Some(canonical.host.as_str());
    is_legacy.then(|| format!("{}{}", canonical.base, path_and_query))
}

pub(crate) async fn redirect_legacy_host(
    State(canonical): State<Arc<CanonicalBase>>,
    req: Request,
    next: Next,
) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    match redirect_target(&canonical, req.method(), host, path_and_query)
        .and_then(|url| HeaderValue::from_str(&url).ok())
    {
        // axum の Redirect::permanent は 308 なので、301 は自前で組む。
        Some(location) => (
            StatusCode::MOVED_PERMANENTLY,
            [(header::LOCATION, location)],
        )
            .into_response(),
        None => next.run(req).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;

    fn app() -> Router {
        let canonical = Arc::new(CanonicalBase::new("https://astesiabot.com/").unwrap());
        Router::new()
            .route("/", get(|| async { "top" }))
            .route("/health", get(|| async { "ok" }))
            .route("/recruitment/", post(|| async { "api" }))
            .route("/TestRunner/en", get(|| async { "tr" }))
            .layer(axum::middleware::from_fn_with_state(
                canonical,
                redirect_legacy_host,
            ))
    }

    async fn send(method: Method, host: &str, uri: &str) -> Response {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        app().oneshot(req).await.unwrap()
    }

    fn location(res: &Response) -> &str {
        res.headers()[header::LOCATION].to_str().unwrap()
    }

    #[tokio::test]
    async fn herokuapp_get_redirects_with_path_and_query() {
        let res = send(Method::GET, "foo-123.herokuapp.com", "/TestRunner/en?a=1&b=2").await;
        assert_eq!(res.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(location(&res), "https://astesiabot.com/TestRunner/en?a=1&b=2");
    }

    #[tokio::test]
    async fn herokuapp_head_redirects() {
        let res = send(Method::HEAD, "foo-123.herokuapp.com", "/").await;
        assert_eq!(res.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(location(&res), "https://astesiabot.com/");
    }

    #[tokio::test]
    async fn www_redirects_to_bare_domain() {
        let res = send(Method::GET, "WWW.astesiabot.com", "/TestRunner/en?x=1").await;
        assert_eq!(res.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(location(&res), "https://astesiabot.com/TestRunner/en?x=1");
    }

    #[tokio::test]
    async fn recruitment_api_is_not_redirected() {
        let res = send(Method::POST, "foo-123.herokuapp.com", "/recruitment/").await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_is_not_redirected() {
        let res = send(Method::GET, "foo-123.herokuapp.com", "/health").await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn canonical_host_is_not_redirected() {
        let res = send(Method::GET, "astesiabot.com", "/TestRunner/en").await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[test]
    fn excluded_path_check_ignores_query() {
        let c = CanonicalBase::new("https://astesiabot.com").unwrap();
        let t = redirect_target(&c, &Method::GET, Some("a.herokuapp.com"), "/health?x=1");
        assert_eq!(t, None);
    }

    #[test]
    fn invalid_base_is_ignored() {
        assert!(CanonicalBase::new("").is_none());
        assert!(CanonicalBase::new("astesiabot.com").is_none());
    }
}
