//! サイト共通のアイコン（ファビコン / ホーム画面 / PWA manifest / OGP の正方形画像）。
//!
//! ブラウザや iOS は `<link>` が無くても `/favicon.ico` と `/apple-touch-icon.png` を
//! ルートへ直接取りに来るので、各ツールの `/static` ではなくルート直下で配信する。
//! `<head>` に書くタグは `templates_shared/head_icons.html` にまとめ、各ページが include する。
//!
//! `static/` の画像は `assets/icon/generate.mjs` で元画像から生成して commit したもの
//! （手書きは `site.webmanifest` のみ）。数が少なく小さいので `include_bytes!` で
//! バイナリに埋め込む（ServeDir と違いカレントディレクトリに依存しない）。

use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

/// ファイル名にハッシュを付けていないので immutable にはせず、1日で再検証させる。
const CACHE_CONTROL: &str = "public, max-age=86400";

/// (パス, Content-Type, 中身)
const ICONS: &[(&str, &str, &[u8])] = &[
    ("/favicon.ico", "image/x-icon", include_bytes!("static/favicon.ico")),
    ("/favicon.svg", "image/svg+xml", include_bytes!("static/favicon.svg")),
    ("/apple-touch-icon.png", "image/png", include_bytes!("static/apple-touch-icon.png")),
    ("/icon-192.png", "image/png", include_bytes!("static/icon-192.png")),
    ("/icon-512.png", "image/png", include_bytes!("static/icon-512.png")),
    ("/icon-maskable-512.png", "image/png", include_bytes!("static/icon-maskable-512.png")),
    ("/site.webmanifest", "application/manifest+json", include_bytes!("static/site.webmanifest")),
];

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    ICONS
        .iter()
        .fold(Router::new(), |router, &(path, content_type, body)| {
            router.route(
                path,
                get(move || async move {
                    (
                        [
                            (header::CONTENT_TYPE, content_type),
                            (header::CACHE_CONTROL, CACHE_CONTROL),
                        ],
                        body,
                    )
                        .into_response()
                }),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn every_icon_is_served_with_its_content_type() {
        for &(path, content_type, body) in ICONS {
            let response = router::<()>()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(response.headers()[header::CONTENT_TYPE], content_type, "{path}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], CACHE_CONTROL, "{path}");
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(&bytes[..], body, "{path}");
        }
    }

    #[test]
    fn manifest_icons_point_to_served_paths() {
        let manifest: serde_json::Value =
            serde_json::from_slice(include_bytes!("static/site.webmanifest")).unwrap();
        let icons = manifest["icons"].as_array().unwrap();
        assert!(!icons.is_empty());
        for icon in icons {
            let src = icon["src"].as_str().unwrap();
            assert!(ICONS.iter().any(|&(path, _, _)| path == src), "{src}");
        }
    }
}
