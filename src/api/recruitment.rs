use super::AppState;
use crate::engine::recruit::{format, RecruitEngine};
use axum::http::HeaderMap;
use axum::response::Redirect;
use axum::{extract::State, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use utoipa::ToSchema;

/// Python の OCRRawData に対応。
#[derive(Deserialize, ToSchema)]
pub(super) struct OcrRawData {
    text: String,
    #[serde(rename = "pickupOperators", default)]
    pickup_operators: Option<Vec<String>>,
    /// ショートカット自身の版（Rust 版で追加）。整数または整数文字列。
    /// 送らない旧ショートカットには `reply` 末尾で更新を通知する。
    #[serde(rename = "shortcutVersion", default)]
    #[schema(value_type = Option<u32>)]
    shortcut_version: Option<serde_json::Value>,
}

/// Python の TagReplyData に対応（`tagCount` / `update` は Rust 版で追加）。
#[derive(Serialize, ToSchema)]
pub(super) struct TagReplyData {
    title: String,
    reply: String,
    /// 計算に使ったタグの数（8個超は8に切り詰め後の数。タグ無しは0）。
    /// ショートカット側で「5個未満ならOCRの読み漏れを疑う」等の判定に使う。
    #[serde(rename = "tagCount")]
    tag_count: usize,
    /// ショートカットが最新版でない時だけ載る。
    #[serde(skip_serializing_if = "Option::is_none")]
    update: Option<ShortcutUpdate>,
}

#[derive(Serialize, ToSchema)]
pub(super) struct ShortcutUpdate {
    #[serde(rename = "latestVersion")]
    latest_version: u32,
    /// 入手用の固定URL（`SHORTCUT_PATH`。iCloud リンクへリダイレクトする）。
    url: String,
    message: String,
}

/// `data/recruitment/shortcut.yaml`。スキーマはファイル冒頭コメント参照。
#[derive(Deserialize)]
struct ShortcutInfo {
    latest_version: u32,
    url: String,
}

const SHORTCUT_YAML: &str = include_str!("../../data/recruitment/shortcut.yaml");

/// 公開求人ショートカットの入手用固定URL。iCloud リンクは共有し直すたびに変わるため、
/// 通知文・トップページのカードはこちらを指し、差し替えは shortcut.yaml の1か所で済ませる。
pub(super) const SHORTCUT_PATH: &str = "/shortcut/recruitment";

fn shortcut_info() -> &'static ShortcutInfo {
    static INFO: OnceLock<ShortcutInfo> = OnceLock::new();
    INFO.get_or_init(|| serde_yaml::from_str(SHORTCUT_YAML).expect("shortcut.yaml should parse"))
}

/// `GET /shortcut/recruitment` → shortcut.yaml の iCloud リンクへ。
/// 行き先は版ごとに変わるので 301 ではなく 307（ブラウザにキャッシュさせない）。
pub(super) fn shortcut_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(
        SHORTCUT_PATH,
        get(|| async { Redirect::temporary(&shortcut_info().url) }),
    )
}

/// `shortcutVersion` を整数として読む（数値・数値文字列どちらも受ける）。
/// 解釈できない値は「版を送っていない」扱いにする。
fn parse_version(v: Option<&serde_json::Value>) -> Option<u32> {
    match v? {
        serde_json::Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// 更新通知を付ける。計算結果（title/reply/tagCount）とは独立した後処理。
/// `latest_version` が 0 の間（版付きショートカットの公開前）は通知しない。
fn attach_update_notice(
    reply: &mut TagReplyData,
    latest_version: u32,
    version: Option<u32>,
    link: &str,
) {
    if latest_version == 0 {
        return;
    }
    if version.is_some_and(|v| v >= latest_version) {
        return;
    }
    let message = format!(
        "公開求人ショートカットの新しいバージョン(v{latest_version})があります。\n{link}"
    );
    if version.is_none() {
        reply.reply = format!("{}\n\n{}", reply.reply, message);
    }
    reply.update = Some(ShortcutUpdate {
        latest_version,
        url: link.to_string(),
        message,
    });
}

/// Web API の doRecruitment と完全一致する処理。
/// OCR生テキスト → タグ抽出 → 計算 → title/reply(responseForAI) の詰め替え。
/// recruit は計算だけを担い、API レスポンス表現への詰め替えは api 側の責務とする。
fn build_tag_reply(engine: &RecruitEngine, ocr_text: &str, pickup: Option<&[String]>) -> TagReplyData {
    let matched = engine.matcher.match_tag(ocr_text);

    // isEmpty チェック（Python: matchTag.isEmpty()）
    if matched.matches.is_empty() {
        return TagReplyData {
            title: "エラー".to_string(),
            reply: "タグがありません".to_string(),
            tag_count: 0,
            update: None,
        };
    }

    // matches を Vec 化。8個超なら先頭8個に切り詰め（Python の list(matches)[:8]）
    // 注意: 8個超は OCR 大誤爆時のみ。順序は Python set と一致しない（許容）。
    let mut matches: Vec<String> = matched.matches.into_iter().collect();
    if matches.len() > 8 {
        matches.truncate(8);
    }

    let is_global = matched.is_global;
    let results = engine.data.calculate(&matches, is_global, 4, pickup);

    let sorted_input = engine.data.normalize_names(&matches);
    let title = format::make_title(&sorted_input, is_global, true);

    let reply = if results.is_empty() {
        "★4以上になる組み合わせはありません".to_string()
    } else {
        format::response_for_ai(results)
    };

    TagReplyData {
        title,
        reply,
        tag_count: matches.len(),
        update: None,
    }
}

#[utoipa::path(
    post,
    path = "/recruitment/",
    request_body = OcrRawData,
    responses(
        (status = 200, description = "タグ抽出結果", body = TagReplyData)
    )
)]
pub async fn do_recruitment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(data): Json<OcrRawData>,
) -> Json<TagReplyData> {
    let pickup = data.pickup_operators.as_deref();
    let mut reply = build_tag_reply(&state.recruit, &data.text, pickup);
    let version = parse_version(data.shortcut_version.as_ref());
    let link = format!("{}{}", super::base_url(&headers), SHORTCUT_PATH);
    attach_update_notice(&mut reply, shortcut_info().latest_version, version, &link);
    Json(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn tag_count_reflects_matched_tags() {
        let engine = RecruitEngine::load().expect("recruit data should load");

        let reply = build_tag_reply(&engine, "狙撃タイプ\n工リート\n範囲攻撃\n火力\n減速", None);
        assert_eq!(reply.tag_count, 5);

        let reply = build_tag_reply(&engine, "狙撃タイプ\n範囲攻撃", None);
        assert_eq!(reply.tag_count, 2);

        let reply = build_tag_reply(&engine, "関係ない文字列", None);
        assert_eq!(reply.tag_count, 0);
        assert_eq!(reply.title, "エラー");

        let json = serde_json::to_value(&reply).unwrap();
        assert_eq!(json["tagCount"], 0);
        assert!(json.get("update").is_none());
    }

    #[test]
    fn shortcut_yaml_has_icloud_url() {
        assert!(shortcut_info().url.starts_with("https://www.icloud.com/shortcuts/"));
    }

    #[tokio::test]
    async fn shortcut_path_redirects_to_icloud() {
        let res = shortcut_router::<()>()
            .oneshot(Request::get(SHORTCUT_PATH).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(res.headers()[header::LOCATION], shortcut_info().url.as_str());
    }

    #[test]
    fn parse_version_accepts_number_and_string() {
        assert_eq!(parse_version(Some(&serde_json::json!(3))), Some(3));
        assert_eq!(parse_version(Some(&serde_json::json!(" 3 "))), Some(3));
        assert_eq!(parse_version(Some(&serde_json::json!("v3"))), None);
        assert_eq!(parse_version(Some(&serde_json::json!(-1))), None);
        assert_eq!(parse_version(None), None);
    }

    fn sample_reply() -> TagReplyData {
        TagReplyData {
            title: "t".to_string(),
            reply: "r".to_string(),
            tag_count: 5,
            update: None,
        }
    }

    const LINK: &str = "https://astesiabot.com/shortcut/recruitment";

    #[test]
    fn update_notice_by_version() {
        // 最新版・それより新しい版: 何も付かない
        for v in [2, 3] {
            let mut r = sample_reply();
            attach_update_notice(&mut r, 2, Some(v), LINK);
            assert!(r.update.is_none());
            assert_eq!(r.reply, "r");
        }

        // 古い版: update だけ付き、reply は変えない（新ショートカットが自分で表示する）
        let mut r = sample_reply();
        attach_update_notice(&mut r, 2, Some(1), LINK);
        let update = r.update.as_ref().unwrap();
        assert_eq!(update.latest_version, 2);
        assert_eq!(update.url, LINK);
        assert_eq!(r.reply, "r");

        // 版無し（旧ショートカット）: reply 末尾にも通知文
        let mut r = sample_reply();
        attach_update_notice(&mut r, 2, None, LINK);
        assert!(r.update.is_some());
        assert_eq!(
            r.reply,
            format!("r\n\n公開求人ショートカットの新しいバージョン(v2)があります。\n{LINK}")
        );
    }

    #[test]
    fn update_notice_disabled_while_latest_version_is_zero() {
        let mut r = sample_reply();
        attach_update_notice(&mut r, 0, None, LINK);
        assert!(r.update.is_none());
        assert_eq!(r.reply, "r");
    }
}
