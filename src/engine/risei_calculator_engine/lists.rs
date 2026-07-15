//! riseilists相当のDTO+計算(Python `CalculatorManager.riseilists`の各分岐相当)。
//! 整形(Discord embed化/GPT向けJSON化)は呼び出し側の責務。元は
//! `bot/commands/risei/riseilists.rs`にあったが、bot/apiの両方(Discordコマンド・GPT function
//! calling)から参照するためengineへ移した。`RiseiListTarget`(poise::ChoiceParameter)は
//! Discordコマンド専用の選択肢型なのでコマンド層に残している。

use super::server::{item_rarity2, item_rarity3};
use super::values::RiseiValues;
use super::{RiseiCalculatorEngine, Server};
use crate::engine::outer_source::item_names::ItemNames;
use crate::engine::outer_source::OuterSourceRegistry;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

const PRICE_PATH: &str = "data/risei/price.yaml";
const PRICE_SPECIAL_PATH: &str = "data/risei/price_special.yaml";
const PRICE_CC_PATH: &str = "data/risei/price_cc.yaml";

/// 契約賞金引換証の現行シーズン番号(Python `listInfo.__ccNumber`)。表示用定数のため
/// コマンド層(`bot/commands/risei/riseilists.rs`)の`CC_NUMBER`と手動で同期させること。
pub const CC_NUMBER: &str = "4";

/// 契約賞金引換証(CC)の1アイテム分の交換レート(Python `listInfo.CCExchangeItem`)。
/// 結晶交換所(Pinch Out)相当のデータ・実装はオミット済み。
#[derive(Debug, Clone, Deserialize)]
struct CCExchangeItem {
    /// 中国語名(`RiseiValues`のキーと合わせるため)。
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

/// 軽量な価格表のみ依存。起動時ロードのStaticDataとは別に、初回アクセス時に一度だけ読み込む。
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

/// 理性/時間価値一覧の1項目(Python版は`(String,f64,f64)`のタプルだったが、各値の
/// 意味が読み手から追えるよう名前付きにした)。
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

/// 対象アイテム集合＋価格表から交換効率一覧を計算する(te2/te3/special_list共通)。
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

impl RiseiCalculatorEngine {
    /// riseilists(basemaps)相当。
    pub async fn base_maps(&self, outer_source: &OuterSourceRegistry, server: Server) -> BTreeMap<String, String> {
        self.snapshot(server, outer_source).await.base_stage_display
    }

    /// riseilists(san_value_lists)相当。
    pub async fn value_list(&self, outer_source: &OuterSourceRegistry, server: Server) -> Vec<ValueEntry> {
        let snapshot = self.snapshot(server, outer_source).await;
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
    pub async fn te2_list(&self, outer_source: &OuterSourceRegistry, server: Server) -> Vec<TicketEfficiency> {
        let snapshot = self.snapshot(server, outer_source).await;
        ticket_efficiency_list(&item_rarity2(server), price(), &snapshot.values)
    }

    /// riseilists(te3list)相当。
    pub async fn te3_list(&self, outer_source: &OuterSourceRegistry, server: Server) -> Vec<TicketEfficiency> {
        let snapshot = self.snapshot(server, outer_source).await;
        ticket_efficiency_list(&item_rarity3(server), price(), &snapshot.values)
    }

    /// riseilists(special_list)相当。初級・上級両方の資格証対象アイテムを特別引換証価格で評価する。
    pub async fn special_list(&self, outer_source: &OuterSourceRegistry, server: Server) -> Vec<TicketEfficiency> {
        let snapshot = self.snapshot(server, outer_source).await;
        let mut items = item_rarity2(server);
        items.extend(item_rarity3(server));
        ticket_efficiency_list(&items, price_special(), &snapshot.values)
    }

    /// riseilists(cclist)相当。
    pub async fn cc_list(&self, outer_source: &OuterSourceRegistry, server: Server) -> Vec<TicketEfficiency> {
        let snapshot = self.snapshot(server, outer_source).await;
        exchange_efficiency_list(price_cc(), &snapshot.values)
    }
}
