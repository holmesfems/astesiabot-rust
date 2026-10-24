use super::AppState;
use crate::engine::recruit::{format, RecruitEngine};
use axum::http::{HeaderMap, StatusCode};
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
    INFO.get_or_init(|| serde_saphyr::from_str(SHORTCUT_YAML).expect("shortcut.yaml should parse"))
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
/// pickupOperators の入力しづらい表記の別名 → 正式表記。
/// ショートカット等から手入力されるため、記号を打たなくても指定できるようにする。
const PICKUP_ALIASES: &[(&str, &str)] = &[("GALLUS2", "GALLUS²")];

/// pickupOperators の各名前を正式表記へ寄せる（前後空白の除去 + 別名の置き換え）。
fn normalize_pickup_operators(names: Vec<String>) -> Vec<String> {
    names
        .into_iter()
        .map(|name| {
            let trimmed = name.trim();
            PICKUP_ALIASES
                .iter()
                .find(|(alias, _)| *alias == trimmed)
                .map_or_else(|| trimmed.to_string(), |(_, official)| official.to_string())
        })
        .collect()
}

/// `text` の上限（文字数）。求人画面1枚のOCR結果は200文字前後なので、英語版クライアントや
/// 背景の文字の拾い込みを見込んでも十分な余裕がある。タグ抽出は行数・語数に比例して
/// 重くなる（2MBで0.2秒前後）ため、認証なしで叩ける入口で量を絞る。
const MAX_TEXT_CHARS: usize = 2_000;

/// `/recruitment/` のリクエスト本文の上限。text の上限＋pickupOperators 等に十分な大きさで、
/// axum 既定の 2MB を丸ごと受け取ってから弾くのを避ける。超えると axum が 413 を返す。
pub(super) const MAX_BODY_BYTES: usize = 64 * 1024;
const _: () = assert!(MAX_BODY_BYTES > MAX_TEXT_CHARS * 4, "本文の上限はtextの上限(UTF-8最大4バイト/文字)より大きいこと");

/// `text` が長すぎる場合のエラー応答（ショートカットがそのまま表示できる通常の形）。
fn reject_too_long_text(text: &str) -> Option<TagReplyData> {
    (text.chars().count() > MAX_TEXT_CHARS).then(|| TagReplyData {
        title: "エラー".to_string(),
        reply: format!("テキストが長すぎます（{MAX_TEXT_CHARS}文字まで）"),
        tag_count: 0,
        update: None,
    })
}

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
    let data = engine.data();
    let results = data.calculate(&matches, is_global, 4, pickup);

    let sorted_input = data.normalize_names(&matches);
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
        (status = 200, description = "タグ抽出結果", body = TagReplyData),
        (status = 400, description = "text が長すぎる（2000文字超）", body = TagReplyData),
        (status = 413, description = "リクエスト本文が大きすぎる（64KiB超）")
    )
)]
pub async fn do_recruitment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(data): Json<OcrRawData>,
) -> (StatusCode, Json<TagReplyData>) {
    if let Some(error) = reject_too_long_text(&data.text) {
        return (StatusCode::BAD_REQUEST, Json(error));
    }
    let pickup = data.pickup_operators.map(normalize_pickup_operators);
    let mut reply = build_tag_reply(&state.recruit, &data.text, pickup.as_deref());
    let version = parse_version(data.shortcut_version.as_ref());
    let link = format!("{}{}", super::base_url(&headers), SHORTCUT_PATH);
    attach_update_notice(&mut reply, shortcut_info().latest_version, version, &link);
    (StatusCode::OK, Json(reply))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn too_long_text_is_rejected_by_char_count() {
        // 境界ちょうどは通す（全角でも1文字＝1と数える。UTF-8のバイト数ではない）
        assert!(reject_too_long_text(&"狙".repeat(MAX_TEXT_CHARS)).is_none());
        let error = reject_too_long_text(&"狙".repeat(MAX_TEXT_CHARS + 1)).expect("over the limit");
        assert_eq!(error.title, "エラー");
        assert_eq!(error.tag_count, 0);
        // 実際の求人画面1枚ぶんのOCR結果は余裕で通る
        let screen = "820056\n1378\n40320\n3192\n連絡回数2/3\n08:13:30 連絡中\n募集説明\n募集期間\n01:00:00\n\
                      募集可能範囲\n★1 ★2 ★3\n★4\n募集条件\n(最大3個)\n狙撃タイプ\n重装タイプ\n補助タイプ\n\
                      術師タイプ\n遠距離\nHUMAN RESOURCES DEPARTMENT\nタグをリセット\n募集予算\n999+/1\nRHODES ISLAND";
        assert!(reject_too_long_text(screen).is_none());
    }

    #[tokio::test]
    async fn body_over_limit_is_rejected_before_parsing() {
        // run_api と同じ掛け方で、本文が上限を超えると 413 になることを確認する
        let app: Router = Router::new().route(
            "/recruitment/",
            axum::routing::post(|Json(_): Json<serde_json::Value>| async { "ok" })
                .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_BYTES)),
        );
        let body = format!(r#"{{"text":"{}"}}"#, "a".repeat(MAX_BODY_BYTES));
        let res = app
            .oneshot(
                Request::post("/recruitment/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn tag_count_reflects_matched_tags() {
        let engine = RecruitEngine::load_from_seed();

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
    fn repeated_pickup_names_do_not_change_reply() {
        let engine = RecruitEngine::load_from_seed();
        let text = "狙撃タイプ\n遠距離\n範囲攻撃\n減速\n火力";
        let single = vec!["シラユキ".to_string()];
        let mut repeated = vec!["シラユキ".to_string(); 1000];
        repeated.push("存在しない".to_string());

        let without = build_tag_reply(&engine, text, None);
        let with_single = build_tag_reply(&engine, text, Some(&single));
        let with_repeated = build_tag_reply(&engine, text, Some(&repeated));
        assert_ne!(with_single.reply, without.reply);
        assert_eq!(with_single.reply, with_repeated.reply);
        assert_eq!(with_single.title, with_repeated.title);
    }

    #[test]
    fn pickup_alias_redirects_to_official_name() {
        let names = vec!["GALLUS2".to_string(), " エクシア ".to_string(), "GALLUS²".to_string()];
        assert_eq!(normalize_pickup_operators(names), vec!["GALLUS²", "エクシア", "GALLUS²"]);
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
