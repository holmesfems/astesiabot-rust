use super::values::RiseiValues;
use crate::engine::outer_source::item_names::ItemNames;
use indexmap::IndexMap;
use serde::Deserialize;
use std::collections::HashMap;

/// 契約賞金引換証の現行シーズン番号（Python `listInfo.__ccNumber`）。
/// コマンド説明文の表示にのみ使う定数なので、シーズンが変わったらここを直接書き換える。
/// `riseilists.rs` の `RiseiListTarget::CcList` の `#[name]` 表示文字列
/// （poise::ChoiceParameterはコンパイル時文字列リテラルしか取れない）と手動で同期させること。
pub const CC_NUMBER: &str = "4";

/// 課金パック一覧の基準として使う恒常パック名（Python `KakinPack.__init__` の`basicPackName`。
/// グローバル版のみ対応のため固定値。Mainland版の課金パック対応は元々使用頻度が低くオミット済み
/// （`Design.md`参照）。
const BASIC_PACK_NAME: &str = "10000円恒常パック";

/// 契約賞金引換証(CC)の1アイテム分の交換レート（Python `listInfo.CCExchangeItem`）。
/// 結晶交換所(Pinch Out)相当のデータ・実装はオミット済み。
#[derive(Debug, Clone, Deserialize)]
pub struct CCExchangeItem {
    /// 中国語名（`RiseiValues`のキーと合わせるため）。
    pub name: String,
    /// 在庫数。"∞"はそのまま特別扱いする以外は表示に使わない。
    pub quantity: String,
    pub value: f64,
}

impl CCExchangeItem {
    /// Python `CCExchangeItem.fullname`。
    pub fn full_name(&self, item_names: &ItemNames) -> String {
        let ja = item_names.zh_to_ja(&self.name);
        if self.quantity == "∞" {
            format!("{ja}({})", self.quantity)
        } else {
            ja.to_string()
        }
    }

    pub fn efficiency(&self, values: &RiseiValues) -> f64 {
        values.get_value_from_zh(&self.name) / self.value
    }

    pub fn std_dev_efficiency(&self, values: &RiseiValues) -> f64 {
        values.get_std_dev_from_zh(&self.name) / self.value
    }
}

/// `data/risei/price_kakin.yaml` の1パック分（Python `getKakinList`の値側）。
#[derive(Debug, Clone, Deserialize)]
pub struct KakinPackDef {
    pub price: f64,
    #[serde(rename = "isConstant")]
    pub is_constant: bool,
    /// 日本語名→個数。順序は表示順に使うため`IndexMap`でYAML記載順を保持する。
    pub contents: IndexMap<String, f64>,
}

/// 計算済みの課金パック理性効率（Python `CalculatorManager.KakinPack`）。
pub struct KakinPack {
    pub name: String,
    pub price: f64,
    pub contents: Vec<(String, f64)>,
    pub total_value: f64,
    pub total_originium: f64,
    pub total_real_money: f64,
    pub total_efficiency: f64,
    pub gacha_count: f64,
    pub gacha_efficiency: f64,
}

fn pack_value(contents: &IndexMap<String, f64>, values: &RiseiValues) -> f64 {
    contents.iter().map(|(ja, count)| values.get_value_from_ja(ja) * count).sum()
}

fn pack_gacha_count(contents: &IndexMap<String, f64>, const_gacha: &HashMap<String, f64>) -> f64 {
    contents
        .iter()
        .map(|(ja, count)| const_gacha.get(ja).copied().unwrap_or(0.0) * count)
        .sum()
}

/// Python `KakinPack.__init__`。基準パック(`BASIC_PACK_NAME`)は`kakin_list`に必ず
/// 存在する前提（`price_kakin.yaml`の必須エントリ）。
pub fn build_kakin_pack(
    name: &str,
    def: &KakinPackDef,
    values: &RiseiValues,
    kakin_list: &IndexMap<String, KakinPackDef>,
    const_gacha: &HashMap<String, f64>,
) -> KakinPack {
    let total_value = pack_value(&def.contents, values);
    let total_originium = total_value / values.get_value_from_ja("純正源石");

    let basic = kakin_list
        .get(BASIC_PACK_NAME)
        .expect("price_kakin.yamlに基準パック'10000円恒常パック'が存在しません");
    let basic_value = pack_value(&basic.contents, values);
    let total_real_money = total_value / basic_value * basic.price;
    let total_efficiency = total_real_money / def.price;

    let gacha_count = pack_gacha_count(&def.contents, const_gacha);
    let basic_gacha_count = pack_gacha_count(&basic.contents, const_gacha);
    let gacha_efficiency = gacha_count / def.price * basic.price / basic_gacha_count;

    KakinPack {
        name: name.to_string(),
        price: def.price,
        contents: def.contents.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        total_value,
        total_originium,
        total_real_money,
        total_efficiency,
        gacha_count,
        gacha_efficiency,
    }
}
