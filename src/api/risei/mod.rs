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

mod handlers;
mod operator_cost;
mod params;
mod skill;
mod spec;
mod suggest;
#[cfg(test)]
mod tests;

use crate::engine::external_source::ExternalSourceRegistry;
use crate::engine::risei_calculator_engine::RiseiCalculatorEngine;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;

pub use skill::{SKILL_ZIP_PATH, skill_router};
use spec::{ParamSpec, EndpointSpec, endpoint_usage};
use params::{ApiError, ApiResult, Params, rfc3339, check_params, required};
use suggest::suggest;
use handlers::{index, materials, stages, events, lists_index, lists, not_found};

pub const PREFIX: &str = "/api/risei";

/// `/api/risei`のハンドラが共有する状態。`AppState`全体には依存しない
/// （テストや`serve_web`のようなbot無しの構成でも組み立てられるようにするため）。
#[derive(Clone)]
pub struct RiseiApiState {
    pub risei: Arc<RiseiCalculatorEngine>,
    pub external_source: Arc<ExternalSourceRegistry>,
}

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
        .route("/lists/{kind}", get(lists))
        .route("/operators/mastery", get(operator_cost::mastery))
        .route("/operators/elite", get(operator_cost::elite))
        .route("/operators/module", get(operator_cost::module))
        .route("/operators/lists", get(operator_cost::lists_index))
        .route("/operators/lists/", get(operator_cost::lists_index))
        .route("/operators/lists/{kind}", get(operator_cost::lists))
        .fallback(not_found)
        .with_state(state.clone());
    Router::new()
        // nest()の内側"/"は末尾スラッシュ無しにしか割り当たらないため、"/api/risei/"も明示する。
        .route("/api/risei/", get(index).with_state(state))
        .nest(PREFIX, inner)
}
