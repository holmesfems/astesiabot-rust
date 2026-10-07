//! 理性価値計算(riseiXXX系スラッシュコマンド)のREST API（`GET /api/risei/*`）。
//!
//! 理性効率動画づくりでClaudeが直接叩く用途。認証なしのグローバル公開で、全てGET+クエリ
//! (claude.aiのweb fetchはGETしか使えないため)。値は常に`RiseiCalculatorEngine`の
//! キャッシュを読むだけで、更新はmain.rsの定期ループ(120分毎)の責務。
//!
//! - 整形はしない。engineのDTO(`Serialize`)をそのままJSONで返す(Discord embed / GPT function
//!   calling に続く3つ目の表現層)。効率は 1.0 = 100% の生の値。
//! - 呼び出し側(AI)が間違えたときに自力で直せるよう、エラーは常にJSONで返し、
//!   `did_you_mean`(近い候補)と`usage`(そのエンドポイントの引数一覧)を添える。
//!   未知のクエリ引数も黙って無視せず400にする(`is_global=false`のような旧コマンド引数を
//!   渡して「グローバル版のつもりが…」という取り違えを防ぐため)。
//! - riseikakin(課金パック)は全体比較(グローバル版)だけを`lists/kakin`として返す。
//!   `/riseikakin`自体がグローバル版専用なので、`server=mainland`は400にする。
//! - オペレーター消費素材(`/operatormastercost` `/operatorelitecost` `/operatormodulecost`
//!   `/operatorcostlist`)は`operators/*`として返す([`operator_cost`]に実装)。`server`引数は無く、
//!   botと同じくオペレーターの`cn_only`で換算サーバを切り替える(応答の`cn_only`/`values_server`)。
//!   `AppState`には依存せず、[`RiseiApiState`](理性価値エンジン+外部ソースレジストリ)だけで動く。
//!
//! アクセス方法のskill(`src/api/risei_skill/SKILL.md`)は`GET /api/risei/skill.zip`で配布する
//! (トップページのカードからリンク)。こちらは状態を持たないので[`skill_router`]として
//! `web_ui_router()`側に載せる(serve_webでも見えるように)。
//!
//! ルート一覧・引数の定義は[`ENDPOINTS`]が単一の正本で、`GET /api/risei`(使い方一覧)・
//! 引数チェック・存在しないパスへのサジェストが全てここから組み立てる。

mod operator_cost;

use crate::engine::external_source::ExternalSourceRegistry;
use crate::engine::risei_calculator_engine::kakin::kakin_comparison;
use crate::engine::risei_calculator_engine::{RiseiCalculatorEngine, Server, CC_NUMBER};
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub const PREFIX: &str = "/api/risei";
pub const SKILL_ZIP_PATH: &str = "/api/risei/skill.zip";
/// zip内のフォルダ名(=skill名)。展開してそのまま`.claude/skills/`に置ける形にする。
const SKILL_NAME: &str = "arknights-risei-api";
const SKILL_MD_PATH: &str = "src/api/risei_skill/SKILL.md";
/// `/api/risei`のハンドラが共有する状態。`AppState`全体には依存しない
/// （テストや`serve_web`のようなbot無しの構成でも組み立てられるようにするため）。
#[derive(Clone)]
pub struct RiseiApiState {
    pub risei: Arc<RiseiCalculatorEngine>,
    pub external_source: Arc<ExternalSourceRegistry>,
}

const MAX_SUGGESTIONS: usize = 5;

pub(super) struct ParamSpec {
    pub(super) name: &'static str,
    pub(super) required: bool,
    pub(super) description: &'static str,
}

pub(super) struct EndpointSpec {
    pub(super) path: &'static str,
    pub(super) description: &'static str,
    pub(super) params: &'static [ParamSpec],
}

const SERVER_PARAM: ParamSpec = ParamSpec {
    name: "server",
    required: false,
    description: "global(既定。グローバル版=日本版基準) / mainland(大陸版基準。新ステージ・新素材込み)",
};
const LIMIT_PARAM: ParamSpec = ParamSpec {
    name: "limit",
    required: false,
    description: "返すステージ数の上限(省略時は全件)",
};

const INDEX: EndpointSpec = EndpointSpec {
    path: "/api/risei",
    description: "このAPIの使い方一覧(エンドポイント・引数・素材カテゴリ・効率表の種類・最終更新時刻)",
    params: &[],
};
const MATERIALS: EndpointSpec = EndpointSpec {
    path: "/api/risei/materials",
    description: "昇進素材カテゴリ別の効率の良い恒常ステージ(/riseimaterials)。総合効率の降順。\
                  大陸版先行カテゴリ(new)を指定すると自動的に大陸版基準になる",
    params: &[
        ParamSpec {
            name: "target",
            required: true,
            description: "素材カテゴリ。日本語名(例: 砥石)またはキー(中国語名。例: 研磨石)。一覧は GET /api/risei の material_categories",
        },
        SERVER_PARAM,
        LIMIT_PARAM,
    ],
};
const STAGES: EndpointSpec = EndpointSpec {
    path: "/api/risei/stages",
    description: "恒常ステージ(メイン・恒常サイドストーリー)の効率(/riseistages)。stageは前方一致で名前順。\
                  グローバル版に無いステージは大陸版へフォールバックする(effective_serverで判別)",
    params: &[
        ParamSpec {
            name: "stage",
            required: true,
            description: "ステージ名(前方一致。大文字小文字を区別する。例: 1-7, 12-17, R8-)",
        },
        SERVER_PARAM,
        LIMIT_PARAM,
    ],
};
const EVENTS: EndpointSpec = EndpointSpec {
    path: "/api/risei/events",
    description: "期間限定イベントステージの効率(/riseievents)。過去の開催済み・未開催イベントも対象。\
                  stageは前方一致で名前順",
    params: &[
        ParamSpec {
            name: "stage",
            required: true,
            description: "ステージ名(前方一致。大文字小文字を区別する。例: SV-8, IW-)",
        },
        SERVER_PARAM,
        LIMIT_PARAM,
    ],
};
const LISTS: EndpointSpec = EndpointSpec {
    path: "/api/risei/lists/{kind}",
    description: "各種効率表(/riseilists)。kindは list_kinds 参照。効率表は効率の降順。\
                  kind=kakin は課金パック効率(/riseikakin の全体比較)で、server=global のみ対応",
    params: &[SERVER_PARAM],
};

const ENDPOINTS: &[&EndpointSpec] = &[
    &INDEX,
    &MATERIALS,
    &STAGES,
    &EVENTS,
    &LISTS,
    &operator_cost::OPERATOR_MASTERY,
    &operator_cost::OPERATOR_ELITE,
    &operator_cost::OPERATOR_MODULE,
    &operator_cost::OPERATOR_LISTS,
];

/// `/api/risei/lists/{kind}`の種類（`bot/commands/risei/riseilists.rs`の`RiseiListTarget`相当）。
#[derive(Clone, Copy)]
enum ListKind {
    BaseMaps,
    Values,
    Te2,
    Te3,
    Special,
    Cc,
    Kakin,
}

impl ListKind {
    const ALL: [ListKind; 7] =
        [Self::BaseMaps, Self::Values, Self::Te2, Self::Te3, Self::Special, Self::Cc, Self::Kakin];

    fn key(self) -> &'static str {
        match self {
            Self::BaseMaps => "base_maps",
            Self::Values => "values",
            Self::Te2 => "te2",
            Self::Te3 => "te3",
            Self::Special => "special",
            Self::Cc => "cc",
            Self::Kakin => "kakin",
        }
    }

    fn title(self) -> String {
        match self {
            Self::BaseMaps => "基準マップ(素材カテゴリ→基準ステージ)".to_string(),
            Self::Values => "理性価値表(アイテムごとの理性価値±標準偏差)".to_string(),
            Self::Te2 => "初級資格証効率表".to_string(),
            Self::Te3 => "上級資格証効率表".to_string(),
            Self::Special => "特別引換証効率表".to_string(),
            Self::Cc => format!("契約賞金引換効率表(CC#{CC_NUMBER})"),
            Self::Kakin => "課金パック効率(グローバル版。期間限定パックを総合効率の降順＋比較用の恒常パック)".to_string(),
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// 全エラー共通のJSON応答。
pub(super) struct ApiError {
    status: StatusCode,
    body: Value,
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

/// `/api/risei`以下のルーター。AppStateに依存せず[`RiseiApiState`]だけで動く(テストも同様)。
pub fn router<S>(state: RiseiApiState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let inner = Router::new()
        .route("/", get(index))
        .route("/materials", get(materials))
        .route("/stages", get(stages))
        .route("/events", get(events))
        .route("/lists", get(lists_index))
        .route("/lists/", get(lists_index))
        .route("/lists/:kind", get(lists))
        .route("/operators/mastery", get(operator_cost::mastery))
        .route("/operators/elite", get(operator_cost::elite))
        .route("/operators/module", get(operator_cost::module))
        .route("/operators/lists", get(operator_cost::lists_index))
        .route("/operators/lists/", get(operator_cost::lists_index))
        .route("/operators/lists/:kind", get(operator_cost::lists))
        .fallback(not_found)
        .with_state(state.clone());
    Router::new()
        // nest()の内側"/"は末尾スラッシュ無しにしか割り当たらないため、"/api/risei/"も明示する。
        .route("/api/risei/", get(index).with_state(state))
        .nest(PREFIX, inner)
}

/// skill配布(`GET /api/risei/skill.zip`)だけのルーター。状態に依存しないので
/// `web_ui_router()`に載せる。`/api/risei`の他のルート([`router`])とはパスが重ならない
/// (静的パスはnestのワイルドカードより優先される)。
pub fn skill_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(SKILL_ZIP_PATH, get(skill_zip))
}

/// skillのzipをリクエストごとにディスクから組み立てる(test_runnerのskill.zipと同じ方式。
/// SKILL.mdを直しても再ビルド不要で、配布物が古くなることも無い)。
fn build_skill_zip() -> std::io::Result<Vec<u8>> {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let data = std::fs::read(SKILL_MD_PATH)?;
    writer
        .start_file(format!("{SKILL_NAME}/SKILL.md"), options)
        .map_err(std::io::Error::other)?;
    writer.write_all(&data)?;
    Ok(writer.finish().map_err(std::io::Error::other)?.into_inner())
}

async fn skill_zip() -> Response {
    match build_skill_zip() {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/zip".to_string()),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{SKILL_NAME}.zip\"")),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => {
            // デプロイ事故（ファイル欠落等）を黙って隠さない。
            eprintln!("risei skill.zip の組み立てに失敗しました: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to build skill.zip").into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

async fn index(State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>) -> Json<Value> {
    let categories = engine.stage_category();
    let category_json = |group: &str, map: &std::collections::BTreeMap<String, _>| -> Vec<Value> {
        map.iter()
            .map(|(key, info): (&String, &crate::engine::risei_calculator_engine::server::StageCategoryInfo)| {
                json!({ "key": key, "name": info.to_ja, "group": group })
            })
            .collect()
    };
    let mut material_categories = category_json("main", &categories.main);
    material_categories.extend(category_json("new", &categories.new));

    Json(json!({
        "name": "astesiabot 理性価値計算 API",
        "description": "Discord bot の /riseimaterials /riseistages /riseievents /riseilists /riseikakin(全体比較のみ) と同じ計算結果をJSONで返す。\
                        値はサーバー内のキャッシュで、120分ごとにpenguin-statsのドロップデータから再計算される",
        "notes": [
            "効率・ドロップ率などの比率は 1.0 = 100% の生の値",
            "time_cost は倍速プレイ時のクリア時間(秒)。drop_per_minute は倍速で1分あたりの入手数(中級素材換算)。どちらもクリア時間データが無いステージは null",
            "confidence_3sigma は効率の99%信頼区間(3σ)の幅。基準マップは0",
            "max_times はドロップ統計の試行数(サンプル数)",
            "効率表(lists)の std_dev は1σ。Discord版の「±」表示は2σ(std_dev×2)",
            "material_categories の group=new は大陸版先行カテゴリ。指定すると自動的に大陸版基準で計算される",
            "名前に'復刻'等が付くものは同名ステージの別開催",
            "operators/* の素材個数は[{name,count}]。risei_value は理性価値(理性換算)、total_r2_items は中級素材換算後の個数。換算サーバはbotと同じくオペレーターごとに決まり(cn_only=大陸版限定なら大陸版の理性価値)、server引数は無い",
            "エラー時は {error, did_you_mean?, usage?} を返す。did_you_mean は近い候補",
        ],
        "endpoints": ENDPOINTS.iter().map(|e| endpoint_usage(e)).collect::<Vec<_>>(),
        "skill": SKILL_ZIP_PATH,
        "servers": ["global", "mainland"],
        "material_categories": material_categories,
        "list_kinds": list_kinds_json(),
        "operator_list_kinds": operator_cost::list_kinds_json(),
        "updated_at": {
            "global": rfc3339(engine.last_updated(Server::Global).await),
            "mainland": rfc3339(engine.last_updated(Server::Mainland).await),
        },
    }))
}

async fn materials(State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>, Query(params): Params) -> ApiResult {
    check_params(&params, &MATERIALS)?;
    let target = required(&params, "target", &MATERIALS)?;
    let server = parse_server(&params, &MATERIALS)?;
    let limit = parse_limit(&params, &MATERIALS)?;

    let Some(key) = engine.resolve_category_key(target) else {
        let file = engine.stage_category();
        let all: Vec<(&String, &str)> = file
            .main
            .iter()
            .chain(file.new.iter())
            .map(|(key, info)| (key, info.to_ja.as_str()))
            .collect();
        return Err(ApiError::new(StatusCode::NOT_FOUND, format!("不明な素材カテゴリです: {target}"))
            .with("did_you_mean", suggest(target, all.iter().map(|(_, name)| *name)))
            .with(
                "available",
                all.iter().map(|(key, name)| json!({ "key": key, "name": name })).collect::<Vec<_>>(),
            ));
    };
    let mut result = engine
        .material_search(server, &key)
        .await
        .map_err(|msg| ApiError::new(StatusCode::NOT_FOUND, msg))?;
    truncate(&mut result.stages, limit);
    let updated_at = engine.last_updated(result.effective_server).await;
    Ok(Json(envelope(server, updated_at, json!({ "category_key": key }), &result)))
}

async fn stages(State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>, Query(params): Params) -> ApiResult {
    check_params(&params, &STAGES)?;
    let stage = required(&params, "stage", &STAGES)?;
    let server = parse_server(&params, &STAGES)?;
    let limit = parse_limit(&params, &STAGES)?;

    let mut result = match engine.stage_search(server, stage).await {
        Ok(result) => result,
        Err(msg) => return Err(stage_not_found(&engine, stage, StageKind::Main, msg).await),
    };
    truncate(&mut result.stages, limit);
    let updated_at = engine.last_updated(result.effective_server).await;
    Ok(Json(envelope(server, updated_at, json!({ "query": stage }), &result)))
}

async fn events(State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>, Query(params): Params) -> ApiResult {
    check_params(&params, &EVENTS)?;
    let stage = required(&params, "stage", &EVENTS)?;
    let server = parse_server(&params, &EVENTS)?;
    let limit = parse_limit(&params, &EVENTS)?;

    let mut stages = match engine.event_search(server, stage).await {
        Ok(stages) => stages,
        Err(msg) => return Err(stage_not_found(&engine, stage, StageKind::Event, msg).await),
    };
    truncate(&mut stages, limit);
    let updated_at = engine.last_updated(server).await;
    Ok(Json(envelope(
        server,
        updated_at,
        json!({ "query": stage }),
        &json!({ "effective_server": server, "stages": stages }),
    )))
}

async fn lists_index() -> Json<Value> {
    Json(json!({ "list_kinds": list_kinds_json(), "usage": endpoint_usage(&LISTS) }))
}

async fn lists(
    State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>,
    Path(kind): Path<String>,
    Query(params): Params,
) -> ApiResult {
    let Some(kind) = ListKind::from_key(&kind) else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, format!("不明な効率表の種類です: {kind}"))
            .with("did_you_mean", suggest(&kind, ListKind::ALL.iter().map(|k| k.key())))
            .with("list_kinds", list_kinds_json()));
    };
    check_params(&params, &LISTS)?;
    let server = parse_server(&params, &LISTS)?;

    if let ListKind::Kakin = kind {
        return kakin(&engine, server).await;
    }
    let items = match kind {
        ListKind::BaseMaps => {
            // キーは中国語のカテゴリキーなので、materialsのcategoryと同じ日本語名を添える。
            let file = engine.stage_category();
            json!(engine
                .base_maps(server)
                .await
                .into_iter()
                .map(|(key, stage)| {
                    let name = file.main.get(&key).or_else(|| file.new.get(&key)).map(|info| info.to_ja.clone());
                    json!({ "category_key": key, "category": name.unwrap_or_else(|| key.clone()), "stage": stage })
                })
                .collect::<Vec<_>>())
        }
        ListKind::Values => json!(engine.value_list(server).await),
        ListKind::Te2 => json!(engine.te2_list(server).await),
        ListKind::Te3 => json!(engine.te3_list(server).await),
        ListKind::Special => json!(engine.special_list(server).await),
        ListKind::Cc => json!(engine.cc_list(server).await),
        ListKind::Kakin => unreachable!("上で処理済み"),
    };
    let updated_at = engine.last_updated(server).await;
    Ok(Json(envelope(
        server,
        updated_at,
        json!({ "kind": kind.key(), "title": kind.title() }),
        &json!({ "effective_server": server, "items": items }),
    )))
}

/// `lists/kakin`。他の効率表と違い`items`ではなく`baselines`/`packs`を返す
/// （`/riseikakin`の全体比較が「期間限定パック一覧＋参考用の恒常パック」の2部構成のため）。
async fn kakin(engine: &RiseiCalculatorEngine, server: Server) -> ApiResult {
    if server != Server::Global {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "kakin(課金パック効率)はグローバル版のみ対応です。server を省略するか global にしてください",
        )
        .with("did_you_mean", ["global"])
        .with("usage", endpoint_usage(&LISTS)));
    }
    // updated_atは計算に使った理性価値表そのものの時刻を出す(別途last_updatedを読むと、
    // 間に再計算が挟まったときに値と時刻がずれ得るため)。
    let snapshot = engine.snapshot(server).await;
    let comparison = kakin_comparison(&snapshot.values);
    let kind = ListKind::Kakin;
    Ok(Json(envelope(
        server,
        snapshot.last_updated,
        json!({ "kind": kind.key(), "title": kind.title() }),
        &json!({ "effective_server": server, "baselines": comparison.baselines, "packs": comparison.packs }),
    )))
}

async fn not_found(OriginalUri(uri): OriginalUri) -> ApiError {
    let path = uri.path();
    let mut candidates: Vec<String> = ENDPOINTS
        .iter()
        .filter(|e| !e.path.contains('{'))
        .map(|e| e.path.to_string())
        .collect();
    candidates.extend(ListKind::ALL.iter().map(|k| format!("{PREFIX}/lists/{}", k.key())));
    candidates.extend(operator_cost::list_paths());
    ApiError::new(StatusCode::NOT_FOUND, format!("存在しないエンドポイントです: {path}"))
        .with("did_you_mean", suggest(path, candidates.iter().map(String::as_str)))
        .with("index", PREFIX)
}

// ---------------------------------------------------------------------------
// 共通部
// ---------------------------------------------------------------------------

/// 応答の共通ヘッダ部(server/updated_at + エンドポイント固有のメタ情報)の後ろに
/// 結果本体(`body`。オブジェクト)のフィールドを並べる。
fn envelope(server: Server, updated_at: chrono::DateTime<chrono::Utc>, meta: Value, body: &impl Serialize) -> Value {
    let mut out = Map::new();
    out.insert("server".to_string(), json!(server));
    out.insert("updated_at".to_string(), json!(rfc3339(updated_at)));
    if let Value::Object(meta) = meta {
        out.extend(meta);
    }
    if let Value::Object(body) = json!(body) {
        out.extend(body);
    }
    Value::Object(out)
}

pub(super) fn rfc3339(time: chrono::DateTime<chrono::Utc>) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(super) fn endpoint_usage(endpoint: &EndpointSpec) -> Value {
    json!({
        "path": endpoint.path,
        "method": "GET",
        "description": endpoint.description,
        "params": endpoint
            .params
            .iter()
            .map(|p| json!({ "name": p.name, "required": p.required, "description": p.description }))
            .collect::<Vec<_>>(),
    })
}

fn list_kinds_json() -> Vec<Value> {
    ListKind::ALL
        .iter()
        .map(|k| json!({ "kind": k.key(), "name": k.title(), "path": format!("{PREFIX}/lists/{}", k.key()) }))
        .collect()
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

fn parse_server(params: &HashMap<String, String>, endpoint: &EndpointSpec) -> Result<Server, ApiError> {
    match params.get("server").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("global") => Ok(Server::Global),
        Some("mainland") => Ok(Server::Mainland),
        Some(other) => Err(ApiError::new(StatusCode::BAD_REQUEST, format!("server は global か mainland です: {other}"))
            .with("did_you_mean", suggest(other, ["global", "mainland"]))
            .with("usage", endpoint_usage(endpoint))),
    }
}

fn parse_limit(params: &HashMap<String, String>, endpoint: &EndpointSpec) -> Result<Option<usize>, ApiError> {
    match params.get("limit").map(|v| v.trim()) {
        None | Some("") => Ok(None),
        Some(v) => match v.parse::<usize>() {
            Ok(n) if n > 0 => Ok(Some(n)),
            _ => Err(ApiError::new(StatusCode::BAD_REQUEST, format!("limit は1以上の整数です: {v}"))
                .with("usage", endpoint_usage(endpoint))),
        },
    }
}

fn truncate<T>(list: &mut Vec<T>, limit: Option<usize>) {
    if let Some(limit) = limit {
        list.truncate(limit);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum StageKind {
    Main,
    Event,
}

/// ステージが見つからなかったときの404。候補は常に大陸版(最も先行していてステージ数が
/// 最多。Discordのオートコンプリートと同じ)から出す。恒常/イベントを取り違えている場合は
/// 正しい方のエンドポイントを`hint`で案内する。
async fn stage_not_found(engine: &RiseiCalculatorEngine, stage: &str, kind: StageKind, msg: String) -> ApiError {
    let snapshot = engine.snapshot(Server::Mainland).await;
    let info = &snapshot.stage_info;
    let (own, other) = match kind {
        StageKind::Main => (&info.main_code_to_stage, &info.event_code_to_stage),
        StageKind::Event => (&info.event_code_to_stage, &info.main_code_to_stage),
    };
    let mut error = ApiError::new(StatusCode::NOT_FOUND, msg)
        .with("did_you_mean", suggest(stage, own.keys().map(String::as_str)));
    let found_in_other = match kind {
        StageKind::Main => !snapshot.search_event_stage(stage).is_empty(),
        StageKind::Event => !snapshot.search_main_stage(stage).is_empty(),
    };
    if found_in_other {
        let (label, path) = match kind {
            StageKind::Main => ("イベントステージ", EVENTS.path),
            StageKind::Event => ("恒常ステージ", STAGES.path),
        };
        error = error.with("hint", format!("「{stage}」は{label}にあります: {path}?stage={stage}"));
    } else if !other.is_empty() {
        let other_suggestions = suggest(stage, other.keys().map(String::as_str));
        if !other_suggestions.is_empty() {
            let label = match kind {
                StageKind::Main => "did_you_mean_event",
                StageKind::Event => "did_you_mean_main",
            };
            error = error.with(label, other_suggestions);
        }
    }
    error
}

/// 近い候補を最大[`MAX_SUGGESTIONS`]件返す。大文字小文字を無視した前方一致を最優先し、
/// 残りは編集距離(入力長の半分か2の大きい方まで)の近い順。
pub(super) fn suggest<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let query = input.to_lowercase();
    let threshold = (query.chars().count() / 2).max(2);
    let mut scored: Vec<(usize, usize, &str)> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let lower = candidate.to_lowercase();
            let distance = if !query.is_empty() && lower.starts_with(&query) {
                0
            } else {
                levenshtein(&query, &lower)
            };
            (distance <= threshold).then_some((distance, candidate.chars().count(), candidate))
        })
        .collect();
    scored.sort();
    scored.dedup_by(|a, b| a.2 == b.2);
    scored.into_iter().take(MAX_SUGGESTIONS).map(|(_, _, c)| c.to_string()).collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // 期間限定パックは販売状況次第で0件もあり得る(price_kakin.yamlで全部コメントアウト)ので、
        // 件数には依存しない
        let packs = body["packs"].as_array().unwrap();
        let efficiencies: Vec<f64> = packs.iter().map(|p| p["total_efficiency"].as_f64().unwrap()).collect();
        assert!(efficiencies.windows(2).all(|w| w[0] >= w[1]), "総合効率の降順");
        assert!(packs.iter().all(|p| p["contents"].as_array().is_some_and(|c| !c.is_empty())));
        assert!(packs.iter().all(|p| p.get("note").is_none()));

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
}
