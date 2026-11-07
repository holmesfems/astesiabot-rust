//! `/api/risei/*` のハンドラ（operators/* は `operator_cost`）。

use crate::engine::risei_calculator_engine::kakin::{jst_now, kakin_comparison};
use crate::engine::risei_calculator_engine::{RiseiCalculatorEngine, Server};
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use serde_json::{json, Map, Value};
use super::operator_cost;
use super::spec::{MATERIALS, STAGES, EVENTS, LISTS, ENDPOINTS, ListKind, endpoint_usage};
use super::params::{
    ApiError,
    ApiResult,
    Params,
    rfc3339,
    check_params,
    required,
    parse_server,
    parse_upcoming,
    parse_limit,
    truncate,
};
use super::suggest::{StageKind, stage_not_found, suggest};
use super::skill::SKILL_ZIP_PATH;
use super::{RiseiApiState, PREFIX};

pub(super) async fn index(State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>) -> Json<Value> {
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

pub(super) async fn materials(State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>, Query(params): Params) -> ApiResult {
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

pub(super) async fn stages(State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>, Query(params): Params) -> ApiResult {
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

pub(super) async fn events(State(RiseiApiState { risei: engine, .. }): State<RiseiApiState>, Query(params): Params) -> ApiResult {
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

pub(super) async fn lists_index() -> Json<Value> {
    Json(json!({ "list_kinds": list_kinds_json(), "usage": endpoint_usage(&LISTS) }))
}

pub(super) async fn lists(
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
    let upcoming = parse_upcoming(&params)?;

    if let ListKind::Kakin = kind {
        return kakin(&engine, server, upcoming).await;
    }
    if params.contains_key("upcoming") {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "upcoming は kind=kakin のみの引数です")
            .with("usage", endpoint_usage(&LISTS)));
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
pub(super) async fn kakin(engine: &RiseiCalculatorEngine, server: Server, upcoming: bool) -> ApiResult {
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
    let comparison = kakin_comparison(&snapshot.values, jst_now(), upcoming);
    let kind = ListKind::Kakin;
    Ok(Json(envelope(
        server,
        snapshot.last_updated,
        json!({ "kind": kind.key(), "title": kind.title() }),
        &json!({ "effective_server": server, "baselines": comparison.baselines, "packs": comparison.packs }),
    )))
}

pub(super) async fn not_found(OriginalUri(uri): OriginalUri) -> ApiError {
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

pub(super) fn list_kinds_json() -> Vec<Value> {
    ListKind::ALL
        .iter()
        .map(|k| json!({ "kind": k.key(), "name": k.title(), "path": format!("{PREFIX}/lists/{}", k.key()) }))
        .collect()
}
