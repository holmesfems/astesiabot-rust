use super::{fmt_percent, fmt_value, send_reply, server_from_bool};
use crate::api::AppState;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::engine::outer_source::item_names::ItemNames;
use crate::engine::risei_calculator_engine::server::{item_rarity2, item_rarity3};
use crate::engine::risei_calculator_engine::values::RiseiValues;
use crate::engine::risei_calculator_engine::Server;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

// 契約賞金引換証(CC)一覧はriseilists専用の関心事のため、理性価値計算エンジン
// (risei_calculator_engine)には置かずコマンド層に閉じている
// （`bot/commands/risei/riseikakin.rs`が課金パックをコマンド層に閉じているのと同じ理由）。

const PRICE_PATH: &str = "data/risei/price.yaml";
const PRICE_SPECIAL_PATH: &str = "data/risei/price_special.yaml";
const PRICE_CC_PATH: &str = "data/risei/price_cc.yaml";

/// 契約賞金引換証の現行シーズン番号（Python `listInfo.__ccNumber`）。
/// コマンド説明文の表示にのみ使う定数なので、シーズンが変わったらここを直接書き換える。
/// 下の `RiseiListTarget::CcList` の `#[name]` 表示文字列
/// （poise::ChoiceParameterはコンパイル時文字列リテラルしか取れない）と手動で同期させること。
const CC_NUMBER: &str = "4";

/// 契約賞金引換証(CC)の1アイテム分の交換レート（Python `listInfo.CCExchangeItem`）。
/// 結晶交換所(Pinch Out)相当のデータ・実装はオミット済み。
#[derive(Debug, Clone, Deserialize)]
struct CCExchangeItem {
    /// 中国語名（`RiseiValues`のキーと合わせるため）。
    name: String,
    /// 在庫数。"∞"はそのまま特別扱いする以外は表示に使わない。
    quantity: String,
    value: f64,
}

impl CCExchangeItem {
    /// Python `CCExchangeItem.fullname`。
    fn full_name(&self, item_names: &ItemNames) -> String {
        let ja = item_names.zh_to_ja(&self.name);
        if self.quantity == "∞" {
            format!("{ja}({})", self.quantity)
        } else {
            ja.to_string()
        }
    }

    fn efficiency(&self, values: &RiseiValues) -> f64 {
        values.get_value_from_zh(&self.name) / self.value
    }

    fn std_dev_efficiency(&self, values: &RiseiValues) -> f64 {
        values.get_std_dev_from_zh(&self.name) / self.value
    }
}

/// 軽量な価格表のみ依存。起動時ロードのengine::StaticDataとは別に、初回アクセス時に
/// 一度だけ読み込む（kakinと同じ方針。riseilists以外の消費者が無いためengineに置く必要がない）。
fn price() -> &'static HashMap<String, f64> {
    static PRICE: OnceLock<HashMap<String, f64>> = OnceLock::new();
    PRICE.get_or_init(|| {
        let s = std::fs::read_to_string(PRICE_PATH).expect("price.yamlの読み込みに失敗しました");
        serde_yaml::from_str(&s).expect("price.yamlのパースに失敗しました")
    })
}

fn price_special() -> &'static HashMap<String, f64> {
    static PRICE_SPECIAL: OnceLock<HashMap<String, f64>> = OnceLock::new();
    PRICE_SPECIAL.get_or_init(|| {
        let s = std::fs::read_to_string(PRICE_SPECIAL_PATH).expect("price_special.yamlの読み込みに失敗しました");
        serde_yaml::from_str(&s).expect("price_special.yamlのパースに失敗しました")
    })
}

fn price_cc() -> &'static Vec<CCExchangeItem> {
    static PRICE_CC: OnceLock<Vec<CCExchangeItem>> = OnceLock::new();
    PRICE_CC.get_or_init(|| {
        let s = std::fs::read_to_string(PRICE_CC_PATH).expect("price_cc.yamlの読み込みに失敗しました");
        serde_yaml::from_str(&s).expect("price_cc.yamlのパースに失敗しました")
    })
}

/// 理性/時間価値一覧の1項目（Python版は`(String,f64,f64)`のタプルだったが、各値の
/// 意味が読み手から追えるよう名前付きにした）。
pub struct ValueEntry {
    pub name_ja: String,
    pub value: f64,
    pub std_dev: f64,
}

/// 資格証・CC等の交換効率一覧の1項目。
pub struct TicketEfficiency {
    pub name_ja: String,
    pub efficiency: f64,
    pub std_dev: f64,
}

/// 対象アイテム集合＋価格表から交換効率一覧を計算する（te2/te3/special_list共通）。
/// 総合効率の降順ソート済み。
fn ticket_efficiency_list(items: &[&str], price: &HashMap<String, f64>, values: &RiseiValues) -> Vec<TicketEfficiency> {
    let mut list: Vec<TicketEfficiency> = items
        .iter()
        .filter_map(|zh| {
            let p = price.get(*zh)?;
            if *p == 0.0 {
                return None;
            }
            Some(TicketEfficiency {
                name_ja: values.item_names.zh_to_ja(zh).to_string(),
                efficiency: values.get_value_from_zh(zh) / p,
                std_dev: values.get_std_dev_from_zh(zh) / p,
            })
        })
        .collect();
    list.sort_by(|a, b| b.efficiency.total_cmp(&a.efficiency));
    list
}

/// 契約賞金引換証(CC)の交換効率一覧を計算する。総合効率の降順ソート済み。
fn exchange_efficiency_list(items: &[CCExchangeItem], values: &RiseiValues) -> Vec<TicketEfficiency> {
    let mut list: Vec<TicketEfficiency> = items
        .iter()
        .map(|item| TicketEfficiency {
            name_ja: item.full_name(&values.item_names),
            efficiency: item.efficiency(values),
            std_dev: item.std_dev_efficiency(values),
        })
        .collect();
    list.sort_by(|a, b| b.efficiency.total_cmp(&a.efficiency));
    list
}

/// riseilists(basemaps)相当。
pub async fn base_maps(state: &AppState, server: Server) -> BTreeMap<String, String> {
    state.risei_calculator.snapshot(server, &state.outer_source).await.base_stage_display
}

/// riseilists(san_value_lists)相当。
pub async fn value_list(state: &AppState, server: Server) -> Vec<ValueEntry> {
    let snapshot = state.risei_calculator.snapshot(server, &state.outer_source).await;
    snapshot
        .values
        .value_target
        .iter()
        .map(|zh| ValueEntry {
            name_ja: snapshot.values.item_names.zh_to_ja(zh).to_string(),
            value: snapshot.values.get_value_from_zh(zh),
            std_dev: snapshot.values.get_std_dev_from_zh(zh),
        })
        .collect()
}

/// riseilists(te2list)相当。
pub async fn te2_list(state: &AppState, server: Server) -> Vec<TicketEfficiency> {
    let snapshot = state.risei_calculator.snapshot(server, &state.outer_source).await;
    ticket_efficiency_list(&item_rarity2(server), price(), &snapshot.values)
}

/// riseilists(te3list)相当。
pub async fn te3_list(state: &AppState, server: Server) -> Vec<TicketEfficiency> {
    let snapshot = state.risei_calculator.snapshot(server, &state.outer_source).await;
    ticket_efficiency_list(&item_rarity3(server), price(), &snapshot.values)
}

/// riseilists(special_list)相当。初級・上級両方の資格証対象アイテムを特別引換証価格で評価する。
pub async fn special_list(state: &AppState, server: Server) -> Vec<TicketEfficiency> {
    let snapshot = state.risei_calculator.snapshot(server, &state.outer_source).await;
    let mut items = item_rarity2(server);
    items.extend(item_rarity3(server));
    ticket_efficiency_list(&items, price_special(), &snapshot.values)
}

/// riseilists(cclist)相当。
pub async fn cc_list(state: &AppState, server: Server) -> Vec<TicketEfficiency> {
    let snapshot = state.risei_calculator.snapshot(server, &state.outer_source).await;
    exchange_efficiency_list(price_cc(), &snapshot.values)
}

#[derive(Debug, poise::ChoiceParameter)]
pub enum RiseiListTarget {
    #[name = "基準マップ"]
    BaseMaps,
    #[name = "理性価値表"]
    SanValueList,
    #[name = "初級資格証効率表"]
    Te2List,
    #[name = "上級資格証効率表"]
    Te3List,
    #[name = "特別引換証効率表"]
    SpecialList,
    // poise::ChoiceParameterの#[name]はコンパイル時文字列リテラルのみ。
    // CC_NUMBER定数の値が変わったらここも手動で同期させること。
    #[name = "契約賞金引換効率表(CC#4)"]
    CcList,
}

fn ticket_list_chunks(list: Vec<TicketEfficiency>) -> Vec<String> {
    let lines: Vec<String> = list
        .iter()
        .map(|item| format!("{}: {} ± {}", item.name_ja, fmt_percent(item.efficiency), fmt_percent(item.std_dev * 2.0)))
        .collect();
    vec![format!("```\n{}\n```", lines.join("\n"))]
}

/// 理性効率表を出力します。
#[poise::command(slash_command)]
pub async fn riseilists(
    ctx: Context<'_>,
    #[description = "表示する効率表を選んでください"] target: RiseiListTarget,
    #[description = "True:グローバル版基準の計算(デフォルト)、False:大陸版の新ステージと新素材を入れた計算"] is_global: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let server = server_from_bool(is_global.unwrap_or(true));
    let state = ctx.data().state.clone();

    let (title, chunks) = match target {
        RiseiListTarget::BaseMaps => {
            let map = base_maps(&state, server).await;
            let body = map
                .iter()
                .map(|(category, stage)| format!("{category}: {stage}"))
                .collect::<Vec<_>>()
                .join("\n");
            ("基準ステージ表示".to_string(), vec![format!("```\n{body}\n```")])
        }
        RiseiListTarget::SanValueList => {
            let values = value_list(&state, server).await;
            let lines: Vec<String> = values
                .iter()
                .map(|entry| format!("{}: {} ± {}", entry.name_ja, fmt_value(entry.value), fmt_value(entry.std_dev * 2.0)))
                .collect();
            ("理性価値一覧".to_string(), vec![format!("```\n{}\n```", lines.join("\n"))])
        }
        RiseiListTarget::Te2List => ("初級資格証効率".to_string(), ticket_list_chunks(te2_list(&state, server).await)),
        RiseiListTarget::Te3List => ("上級資格証効率".to_string(), ticket_list_chunks(te3_list(&state, server).await)),
        RiseiListTarget::SpecialList => (
            "特別引換証効率".to_string(),
            ticket_list_chunks(special_list(&state, server).await),
        ),
        RiseiListTarget::CcList => (
            format!("契約賞金引換効率(CC#{CC_NUMBER})"),
            ticket_list_chunks(cc_list(&state, server).await),
        ),
    };

    send_reply(
        ctx,
        EmbedReply {
            title,
            chunks,
            msg_type: MsgType::Ok,
        },
    )
    .await
}
