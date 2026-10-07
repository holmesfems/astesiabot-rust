//! `/api/risei/operators/*`: オペレーター消費素材（`/operatormastercost` `/operatorelitecost`
//! `/operatormodulecost` `/operatorcostlist`）のハンドラ。
//!
//! 計算は`engine::operator_cost_calc::calc`のDTOをそのままJSONで返す（整形はしない）。
//! botと同じく`server`引数は持たず、換算サーバはオペレーター（モジュールはモジュール）の
//! `cn_only`で決まる。応答には`values_server`("global"/"mainland"。1つのサーバに決まらない
//! ランキング・モジュール混在は"mixed")を添え、`updated_at`は使った理性価値表の最終更新
//! (混在時は古い方)を返す。

use super::{
    check_params, endpoint_usage, required, rfc3339, suggest, ApiError, ApiResult, EndpointSpec, ParamSpec, Params,
    RiseiApiState, PREFIX,
};
use crate::engine::operator_cost_calc::calc::{
    cost_list_by_elite, cost_list_cost_of_cn_only, cost_list_cost_of_global, cost_list_master_stats,
    operator_elite_cost, operator_module_cost, skill_master_cost,
};
use crate::engine::operator_cost_calc::dto::OperatorCostError;
use crate::engine::operator_cost_calc::AllOperatorsInfo;
use crate::engine::risei_calculator_engine::Server;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const OPERATOR_PARAM: ParamSpec = ParamSpec {
    name: "operator",
    required: true,
    description: "オペレーター名(日本語。大陸先行オペレーターも日本語名。例: ブレイズ)。不明な名前は did_you_mean に近い候補を返す",
};
const SKILL_PARAM: ParamSpec = ParamSpec {
    name: "skill",
    required: true,
    description: "何番目のスキルか(1〜3の整数)",
};
const STAR_PARAM: ParamSpec = ParamSpec {
    name: "star",
    required: false,
    description: "星の数(4〜6の整数)。kind=elite / mastery では必須。unimplemented_total / implemented_total では指定不可",
};
const ONLY_RECENT_PARAM: ParamSpec = ParamSpec {
    name: "only_recent",
    required: false,
    description: "true なら直近実装/将来実装(大陸版先行)オペレーターのみ(既定 false)。kind=elite / mastery のみ。\
                  順位は絞り込み前の全体順位のまま",
};

pub(super) const OPERATOR_MASTERY: EndpointSpec = EndpointSpec {
    path: "/api/risei/operators/mastery",
    description: "オペレーターのスキル特化1〜3の消費素材(/operatormastercost)。特化ごと・合計の素材個数と理性価値、\
                  合計の中級素材換算、同じ星の特化ランキング内の順位(ranking)を返す",
    params: &[OPERATOR_PARAM, SKILL_PARAM],
};
pub(super) const OPERATOR_ELITE: EndpointSpec = EndpointSpec {
    path: "/api/risei/operators/elite",
    description: "オペレーターの昇進1・2の消費素材(/operatorelitecost)。★5/6の非昇格オペレーターは\
                  昇進素材ランキング内の順位(ranking)も返す",
    params: &[OPERATOR_PARAM],
};
pub(super) const OPERATOR_MODULE: EndpointSpec = EndpointSpec {
    path: "/api/risei/operators/module",
    description: "オペレーターのモジュール消費素材(/operatormodulecost)。モジュールごとにStage.1〜3(phases)と合計。\
                  モジュールが無いオペレーターは404",
    params: &[OPERATOR_PARAM],
};
pub(super) const OPERATOR_LISTS: EndpointSpec = EndpointSpec {
    path: "/api/risei/operators/lists/{kind}",
    description: "オペレーター消費素材の各種ランキング・統計(/operatorcostlist)。kindは operator_list_kinds 参照",
    params: &[STAR_PARAM, ONLY_RECENT_PARAM],
};

/// `/api/risei/operators/lists/{kind}`の種類（`bot/commands/operator_cost_calc/operatorcostlist.rs`の
/// `CostListSelection`相当。星の違いは`star`引数に畳んでいる）。
#[derive(Clone, Copy, PartialEq)]
enum OperatorListKind {
    Elite,
    Mastery,
    UnimplementedTotal,
    ImplementedTotal,
}

impl OperatorListKind {
    const ALL: [OperatorListKind; 4] = [Self::Elite, Self::Mastery, Self::UnimplementedTotal, Self::ImplementedTotal];

    fn key(self) -> &'static str {
        match self {
            Self::Elite => "elite",
            Self::Mastery => "mastery",
            Self::UnimplementedTotal => "unimplemented_total",
            Self::ImplementedTotal => "implemented_total",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Elite => "昇進素材価値表(★4〜6。star必須。昇進素材の理性価値の降順。SoCは含まない)",
            Self::Mastery => "特化統計(★4〜6。star必須。最も重い/軽い特化・Top10・平均。only_recent=trueなら直近実装のみの順位表)",
            Self::UnimplementedTotal => "未実装(大陸版先行)オペレーターの消費素材合計(大陸版の理性価値で換算)",
            Self::ImplementedTotal => "実装済オペレーターの消費素材合計(グローバル版の理性価値で換算)",
        }
    }

    /// star / only_recent を取る種類か。
    fn takes_params(self) -> bool {
        matches!(self, Self::Elite | Self::Mastery)
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

pub(super) fn list_kinds_json() -> Vec<Value> {
    OperatorListKind::ALL
        .iter()
        .map(|k| json!({ "kind": k.key(), "name": k.title(), "path": format!("{PREFIX}/operators/lists/{}", k.key()) }))
        .collect()
}

/// 存在しないパスへのサジェスト候補用。
pub(super) fn list_paths() -> Vec<String> {
    OperatorListKind::ALL.iter().map(|k| format!("{PREFIX}/operators/lists/{}", k.key())).collect()
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

pub(super) async fn mastery(State(state): State<RiseiApiState>, Query(params): Params) -> ApiResult {
    check_params(&params, &OPERATOR_MASTERY)?;
    let operator = required(&params, "operator", &OPERATOR_MASTERY)?;
    let skill = parse_skill(&params)?;

    let (info, values) = AllOperatorsInfo::snapshot(&state.external_source, &state.risei).await;
    let dto = skill_master_cost(&info, &values, operator, skill)
        .map_err(|e| calc_error(e, operator, &info, &OPERATOR_MASTERY))?;
    let servers = [server_for(dto.cn_only)];
    Ok(Json(operator_envelope(&state, &servers, json!({ "query": operator }), &dto).await))
}

pub(super) async fn elite(State(state): State<RiseiApiState>, Query(params): Params) -> ApiResult {
    check_params(&params, &OPERATOR_ELITE)?;
    let operator = required(&params, "operator", &OPERATOR_ELITE)?;

    let (info, values) = AllOperatorsInfo::snapshot(&state.external_source, &state.risei).await;
    let dto = operator_elite_cost(&info, &values, operator)
        .map_err(|e| calc_error(e, operator, &info, &OPERATOR_ELITE))?;
    let servers = [server_for(dto.cn_only)];
    Ok(Json(operator_envelope(&state, &servers, json!({ "query": operator }), &dto).await))
}

pub(super) async fn module(State(state): State<RiseiApiState>, Query(params): Params) -> ApiResult {
    check_params(&params, &OPERATOR_MODULE)?;
    let operator = required(&params, "operator", &OPERATOR_MODULE)?;

    let (info, values) = AllOperatorsInfo::snapshot(&state.external_source, &state.risei).await;
    let dto = operator_module_cost(&info, &values, operator)
        .map_err(|e| calc_error(e, operator, &info, &OPERATOR_MODULE))?;
    // 換算サーバはモジュールごと(大陸版限定モジュールだけ大陸版の価値)。
    let servers: Vec<Server> = dto.modules.iter().map(|m| server_for(m.cn_only)).collect();
    Ok(Json(operator_envelope(&state, &servers, json!({ "query": operator }), &dto).await))
}

pub(super) async fn lists_index() -> Json<Value> {
    Json(json!({ "operator_list_kinds": list_kinds_json(), "usage": endpoint_usage(&OPERATOR_LISTS) }))
}

pub(super) async fn lists(
    State(state): State<RiseiApiState>,
    Path(kind): Path<String>,
    Query(params): Params,
) -> ApiResult {
    let Some(kind) = OperatorListKind::from_key(&kind) else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, format!("不明なオペレーター消費素材表の種類です: {kind}"))
            .with("did_you_mean", suggest(&kind, OperatorListKind::ALL.iter().map(|k| k.key())))
            .with("operator_list_kinds", list_kinds_json()));
    };
    check_params(&params, &OPERATOR_LISTS)?;
    if !kind.takes_params() && !params.is_empty() {
        let mut keys: Vec<&str> = params.keys().map(String::as_str).collect();
        keys.sort_unstable();
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("kind={} は引数を取りません: {}", kind.key(), keys.join(", ")),
        )
        .with("usage", endpoint_usage(&OPERATOR_LISTS)));
    }
    let meta = |extra: Value| {
        let mut meta = json!({ "kind": kind.key(), "title": kind.title() });
        if let (Value::Object(base), Value::Object(extra)) = (&mut meta, extra) {
            base.extend(extra);
        }
        meta
    };

    let (info, values) = AllOperatorsInfo::snapshot(&state.external_source, &state.risei).await;
    // ランキングは★の全オペレーターが対象でcn_onlyが混在するため"mixed"。
    let mixed = [Server::Global, Server::Mainland];
    let response = match kind {
        OperatorListKind::Elite => {
            let star = parse_star(&params)?;
            let only_recent = parse_only_recent(&params)?;
            let dto = cost_list_by_elite(&info, &values, star, only_recent);
            operator_envelope(&state, &mixed, meta(json!({ "only_recent": only_recent })), &dto).await
        }
        OperatorListKind::Mastery => {
            let star = parse_star(&params)?;
            let only_recent = parse_only_recent(&params)?;
            let dto = cost_list_master_stats(&info, &values, star, only_recent).map_err(|e| {
                ApiError::new(StatusCode::NOT_FOUND, e.to_string()).with("usage", endpoint_usage(&OPERATOR_LISTS))
            })?;
            operator_envelope(&state, &mixed, meta(json!({ "only_recent": only_recent })), &dto).await
        }
        OperatorListKind::UnimplementedTotal => {
            let dto = cost_list_cost_of_cn_only(&info, &values);
            operator_envelope(&state, &[Server::Mainland], meta(json!({})), &dto).await
        }
        OperatorListKind::ImplementedTotal => {
            let dto = cost_list_cost_of_global(&info, &values);
            operator_envelope(&state, &[Server::Global], meta(json!({})), &dto).await
        }
    };
    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// 共通部
// ---------------------------------------------------------------------------

/// botの`ValueSet::for_cn_only`と同じ選択（大陸版限定なら大陸版の理性価値）。
fn server_for(cn_only: bool) -> Server {
    if cn_only {
        Server::Mainland
    } else {
        Server::Global
    }
}

/// 応答の共通ヘッダ(`values_server`/`updated_at` + 固有メタ情報)の後ろにDTOのフィールドを並べる。
/// `servers`は換算に使ったサーバ(1種類ならそのサーバ名、複数なら"mixed")。`updated_at`は
/// そのうち最も古い最終更新時刻。
async fn operator_envelope(state: &RiseiApiState, servers: &[Server], meta: Value, body: &impl Serialize) -> Value {
    let mut distinct: Vec<Server> = Vec::new();
    for server in servers {
        if !distinct.contains(server) {
            distinct.push(*server);
        }
    }
    let mut updated_at = None;
    for server in &distinct {
        let t = state.risei.last_updated(*server).await;
        updated_at = Some(updated_at.map_or(t, |prev: chrono::DateTime<chrono::Utc>| prev.min(t)));
    }
    let values_server = match distinct.as_slice() {
        [only] => json!(only),
        _ => json!("mixed"),
    };

    let mut out = Map::new();
    out.insert("values_server".to_string(), values_server);
    out.insert("updated_at".to_string(), json!(updated_at.map(rfc3339)));
    if let Value::Object(meta) = meta {
        out.extend(meta);
    }
    if let Value::Object(body) = json!(body) {
        out.extend(body);
    }
    Value::Object(out)
}

/// 計算関数のエラー→JSON。オペレーター名の不一致は近い候補(`did_you_mean`)を添える。
/// それ以外(スキル番号範囲外・★3の特化・モジュール無し等)もbotと同じ文言のまま404で返す。
fn calc_error(error: OperatorCostError, operator: &str, info: &AllOperatorsInfo, endpoint: &EndpointSpec) -> ApiError {
    let mut api = ApiError::new(StatusCode::NOT_FOUND, error.to_string());
    if let OperatorCostError::OperatorNotFound(_) = error {
        api = api.with("did_you_mean", suggest(operator, info.data.operators.values().map(|op| op.name.as_str())));
    }
    api.with("usage", endpoint_usage(endpoint))
}

fn bad_request(message: String, endpoint: &EndpointSpec) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, message).with("usage", endpoint_usage(endpoint))
}

fn parse_skill(params: &HashMap<String, String>) -> Result<u32, ApiError> {
    let value = required(params, "skill", &OPERATOR_MASTERY)?;
    match value.parse::<u32>() {
        Ok(n) if (1..=3).contains(&n) => Ok(n),
        _ => Err(bad_request(format!("skill は1〜3の整数です: {value}"), &OPERATOR_MASTERY)),
    }
}

fn parse_star(params: &HashMap<String, String>) -> Result<u32, ApiError> {
    let value = required(params, "star", &OPERATOR_LISTS)?;
    match value.parse::<u32>() {
        Ok(n) if (4..=6).contains(&n) => Ok(n),
        _ => Err(bad_request(format!("star は4〜6の整数です: {value}"), &OPERATOR_LISTS)),
    }
}

fn parse_only_recent(params: &HashMap<String, String>) -> Result<bool, ApiError> {
    match params.get("only_recent").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(other) => Err(bad_request(format!("only_recent は true か false です: {other}"), &OPERATOR_LISTS)),
    }
}
