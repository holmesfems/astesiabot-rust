//! Cloudflare を通ったリクエストかを `X-Origin-Secret` ヘッダで確かめる。
//!
//! Cloudflare の Transform Rules がオリジン宛てに `X-Origin-Secret: <秘密の値>` を付け、
//! ここで環境変数 `ORIGIN_SECRET` と比べる。Heroku の DNS ターゲットを直接叩いて
//! `Host: astesiabot.com` を名乗る経路（Cloudflare のレート制限を素通りする）を塞ぐため。
//!
//! - `ORIGIN_SECRET` 未設定なら何もしない（手元・serve_web はそのまま）。
//! - `ORIGIN_SECRET_ENFORCE=1` のときだけ 403 で拒否し、それ以外は食い違いをログに出すだけ
//!   （Cloudflare の設定ずれでサイト全体が 403 にならないよう、ログで確かめてから切り替える）。
//! - `*.herokuapp.com` 宛ては Cloudflare を通らないので対象外（旧ホストは無効化まで今の動きのまま）。
//!   `/health` も死活監視用に対象外。

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const HEADER_NAME: &str = "x-origin-secret";

/// 確かめないパス。
const EXCLUDED_PATHS: &[&str] = &["/health"];

pub(crate) struct OriginSecret {
    secret: String,
    enforce: bool,
    /// 一致したリクエストを最初の1回だけログに出す（ヘッダが届いているかの確認用）。
    logged_first_match: AtomicBool,
}

impl OriginSecret {
    fn new(secret: &str, enforce: bool) -> Option<Self> {
        let secret = secret.trim();
        (!secret.is_empty()).then(|| Self {
            secret: secret.to_string(),
            enforce,
            logged_first_match: AtomicBool::new(false),
        })
    }

    pub(crate) fn from_env() -> Option<Self> {
        let enforce = std::env::var("ORIGIN_SECRET_ENFORCE").is_ok_and(|v| v.trim() == "1");
        Self::new(&std::env::var("ORIGIN_SECRET").ok()?, enforce)
    }
}

/// 長さ以外で比較時間が変わらないように比べる。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

enum Verdict {
    /// 確かめる対象外（旧ホスト・除外パス）。
    Skip,
    Match,
    Mismatch,
}

fn verdict(config: &OriginSecret, headers: &HeaderMap, path: &str) -> Verdict {
    if EXCLUDED_PATHS.contains(&path) {
        return Verdict::Skip;
    }
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if host.is_some_and(super::legacy_host_redirect::is_herokuapp_host) {
        return Verdict::Skip;
    }
    let given = headers.get(HEADER_NAME).map(|v| v.as_bytes()).unwrap_or_default();
    if constant_time_eq(given, config.secret.as_bytes()) {
        Verdict::Match
    } else {
        Verdict::Mismatch
    }
}

pub(crate) async fn check_origin_secret(
    State(config): State<Arc<OriginSecret>>,
    req: Request,
    next: Next,
) -> Response {
    match verdict(&config, req.headers(), req.uri().path()) {
        Verdict::Skip => {}
        Verdict::Match => {
            if !config.logged_first_match.swap(true, Ordering::Relaxed) {
                eprintln!("X-Origin-Secret: 一致するリクエストを受けました（以降は出しません）");
            }
        }
        Verdict::Mismatch => {
            let host = req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("-");
            let present = req.headers().contains_key(HEADER_NAME);
            eprintln!(
                "X-Origin-Secret: 不一致（{}） host={host} path={} enforce={}",
                if present { "値が違う" } else { "ヘッダ無し" },
                req.uri().path(),
                config.enforce,
            );
            if config.enforce {
                return (StatusCode::FORBIDDEN, "403 Forbidden").into_response();
            }
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn send(enforce: bool, host: &str, path: &str, secret: Option<&str>) -> StatusCode {
        let config = Arc::new(OriginSecret::new("s3cret", enforce).unwrap());
        let app = Router::new()
            .fallback(get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(config, check_origin_secret));
        let mut req = Request::get(path).header(header::HOST, host);
        if let Some(s) = secret {
            req = req.header(HEADER_NAME, s);
        }
        app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap().status()
    }

    #[tokio::test]
    async fn enforce_rejects_missing_or_wrong_secret() {
        assert_eq!(send(true, "astesiabot.com", "/", Some("s3cret")).await, StatusCode::OK);
        assert_eq!(send(true, "astesiabot.com", "/", None).await, StatusCode::FORBIDDEN);
        assert_eq!(send(true, "astesiabot.com", "/", Some("wrong")).await, StatusCode::FORBIDDEN);
        assert_eq!(send(true, "astesiabot.com", "/", Some("s3cre")).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn log_only_mode_passes_everything() {
        assert_eq!(send(false, "astesiabot.com", "/", None).await, StatusCode::OK);
        assert_eq!(send(false, "astesiabot.com", "/", Some("wrong")).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn herokuapp_and_health_are_exempt() {
        assert_eq!(send(true, "foo-123.herokuapp.com", "/recruitment/", None).await, StatusCode::OK);
        assert_eq!(send(true, "astesiabot.com", "/health", None).await, StatusCode::OK);
    }

    #[test]
    fn empty_secret_disables_check() {
        assert!(OriginSecret::new("  ", true).is_none());
    }
}
