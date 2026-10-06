//! 課金パックの理性効率（Python `CalculatorManager.KakinPack`）。
//!
//! 元はbotの`/riseikakin`コマンド層に閉じていたが、REST API(`GET /api/risei/lists/kakin`)からも
//! 同じ計算結果を返すためengineへ移した（api は bot に依存しないため）。整形(embed/xlsx)は
//! 引き続きbot側の責務。計算式はbot時代から変えていない。
//!
//! 理性価値の重いドロップデータには触れず、[`RiseiValues`](理性価値表のキャッシュ)と価格表
//! (`data/risei/price_kakin.yaml`)だけで計算する。毎回の計算は掛け算と足し算だけなので
//! パック効率自体はキャッシュしない（理性価値表の更新タイミングにそのまま追従する）。
//!
//! グローバル版のみ対応。Mainland版の課金パック対応は元々使用頻度が低くオミット済み
//! （`ref_python/RiseiCalculatorBot-main/riseicalculator2/Design.md`参照）。

use super::values::RiseiValues;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const KAKIN_LIST_PATH: &str = "data/risei/price_kakin.yaml";
const CONST_GACHA_PATH: &str = "data/risei/const_gacha.yaml";

/// 課金パック一覧の基準として使う恒常パック名（Python `KakinPack.__init__` の`basicPackName`）。
pub const BASIC_PACK_NAME: &str = "10000円恒常パック";

/// `data/risei/price_kakin.yaml` の1パック分（Python `getKakinList`の値側）。
#[derive(Debug, Clone, Deserialize)]
pub struct KakinPackDef {
    pub price: f64,
    #[serde(rename = "isConstant")]
    pub is_constant: bool,
    /// 日本語名→個数。順序は表示順に使うため`IndexMap`でYAML記載順を保持する。
    pub contents: IndexMap<String, f64>,
}

/// 軽量な価格表のみ依存。起動時ロードのStaticDataとは別に、初回アクセス時に一度だけ読み込む
/// （Design.md「autoCompletion_riseikakinのみ価格表依存で早く動く」の通り）。
pub fn kakin_list() -> &'static IndexMap<String, KakinPackDef> {
    static LIST: OnceLock<IndexMap<String, KakinPackDef>> = OnceLock::new();
    LIST.get_or_init(|| {
        let s = std::fs::read_to_string(KAKIN_LIST_PATH)
            .expect("price_kakin.yamlの読み込みに失敗しました");
        serde_yaml::from_str(&s).expect("price_kakin.yamlのパースに失敗しました")
    })
}

fn const_gacha() -> &'static HashMap<String, f64> {
    static GACHA: OnceLock<HashMap<String, f64>> = OnceLock::new();
    GACHA.get_or_init(|| {
        let s = std::fs::read_to_string(CONST_GACHA_PATH)
            .expect("const_gacha.yamlの読み込みに失敗しました");
        serde_yaml::from_str(&s).expect("const_gacha.yamlのパースに失敗しました")
    })
}

/// 計算済みの課金パック理性効率（Python `CalculatorManager.KakinPack`）。
#[derive(Debug, Clone)]
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

fn basic_pack() -> &'static KakinPackDef {
    kakin_list()
        .get(BASIC_PACK_NAME)
        .expect("price_kakin.yamlに基準パック'10000円恒常パック'が存在しません")
}

fn pack_value(contents: &IndexMap<String, f64>, values: &RiseiValues) -> f64 {
    contents
        .iter()
        .map(|(ja, count)| values.get_value_from_ja(ja) * count)
        .sum()
}

fn pack_gacha_count(contents: &IndexMap<String, f64>) -> f64 {
    contents
        .iter()
        .map(|(ja, count)| const_gacha().get(ja).copied().unwrap_or(0.0) * count)
        .sum()
}

/// 理性価値を「基準パック(10000円恒常)と同じレートで買ったら何円か」に換算する
/// （[`KakinPack::total_real_money`]の式。アイテム単位にも使う）。
fn to_real_money(value: f64, values: &RiseiValues) -> f64 {
    let basic = basic_pack();
    value / pack_value(&basic.contents, values) * basic.price
}

/// Python `KakinPack.__init__`。基準パック(`BASIC_PACK_NAME`)は`price_kakin.yaml`に
/// 必ず存在する前提。
pub fn build_kakin_pack(name: &str, def: &KakinPackDef, values: &RiseiValues) -> KakinPack {
    let total_value = pack_value(&def.contents, values);
    let total_originium = total_value / values.get_value_from_ja("純正源石");

    let basic = basic_pack();
    let total_real_money = to_real_money(total_value, values);
    let total_efficiency = total_real_money / def.price;

    let gacha_count = pack_gacha_count(&def.contents);
    let basic_gacha_count = pack_gacha_count(&basic.contents);
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

/// 恒常パック（`/riseikakin`の「参考用課金効率」）。YAML記載順。
pub fn constant_packs(values: &RiseiValues) -> Vec<KakinPack> {
    kakin_list()
        .iter()
        .filter(|(_, def)| def.is_constant)
        .map(|(name, def)| build_kakin_pack(name, def, values))
        .collect()
}

/// 期間限定パック（`/riseikakin`の全体比較）。総合効率の降順。
pub fn limited_packs(values: &RiseiValues) -> Vec<KakinPack> {
    let mut packs: Vec<KakinPack> = kakin_list()
        .iter()
        .filter(|(_, def)| !def.is_constant)
        .map(|(name, def)| build_kakin_pack(name, def, values))
        .collect();
    packs.sort_by(|a, b| b.total_efficiency.total_cmp(&a.total_efficiency));
    packs
}

/// REST API用の全体比較（`/riseikakin`の全体比較(グローバル)をDTOにしたもの）。
#[derive(Debug, Clone, Serialize)]
pub struct KakinComparison {
    /// 比較用の恒常パック（「参考用課金効率」）。YAML記載順。
    pub baselines: Vec<KakinBaseline>,
    /// 期間限定パック。総合効率の降順。
    pub packs: Vec<KakinPackView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KakinBaseline {
    pub name: String,
    pub total_efficiency: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KakinPackView {
    pub name: String,
    pub price_jpy: f64,
    /// マネー換算（10000円恒常パックと同じレートで買ったら何円か）。
    pub value_jpy: f64,
    pub total_efficiency: f64,
    pub gacha_efficiency: f64,
    pub total_value: f64,
    pub total_originium: f64,
    pub gacha_count: f64,
    pub contents: Vec<KakinContentView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KakinContentView {
    pub name: String,
    pub count: f64,
    /// このアイテム分のマネー換算。パック内の合計は[`KakinPackView::value_jpy`]と一致する。
    pub value_jpy: f64,
}

pub fn kakin_comparison(values: &RiseiValues) -> KakinComparison {
    let baselines = constant_packs(values)
        .into_iter()
        .map(|pack| KakinBaseline { name: pack.name, total_efficiency: pack.total_efficiency })
        .collect();
    let packs = limited_packs(values)
        .into_iter()
        .map(|pack| KakinPackView {
            contents: pack
                .contents
                .iter()
                .map(|(name, count)| KakinContentView {
                    name: name.clone(),
                    count: *count,
                    value_jpy: to_real_money(values.get_value_from_ja(name) * count, values),
                })
                .collect(),
            name: pack.name,
            price_jpy: pack.price,
            value_jpy: pack.total_real_money,
            total_efficiency: pack.total_efficiency,
            gacha_efficiency: pack.gacha_efficiency,
            total_value: pack.total_value,
            total_originium: pack.total_originium,
            gacha_count: pack.gacha_count,
        })
        .collect();
    KakinComparison { baselines, packs }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::ExternalSourceRegistry;
    use crate::engine::risei_calculator_engine::{RiseiCalculatorEngine, Server};

    /// 理性価値0と分かっているアイテム名(コスメ等)の一覧。テストでしか使わない
    /// (計算では価値0のアイテムは元々0として足されるだけなので、実行時に読む必要が無い)。
    const ZERO_VALUE_ITEMS_PATH: &str = "data/risei/zero_value_items.yaml";

    /// `get_value_from_ja`は知らない名前を黙って0にするため、YAMLの誤字や理性価値表に無い
    /// アイテムで効率が低く出ても気づけない。価値0は`zero_value_items.yaml`での明示を必須にする。
    #[tokio::test]
    async fn every_pack_item_has_risei_value() {
        let zero_value_items: Vec<String> = serde_yaml::from_str(
            &std::fs::read_to_string(ZERO_VALUE_ITEMS_PATH).expect("zero_value_items.yamlを読める"),
        )
        .expect("zero_value_items.yamlは文字列のリスト");
        let outer_source = ExternalSourceRegistry::load(true).await;
        let engine = RiseiCalculatorEngine::load(&outer_source).await.expect("seedから理性価値表を計算できる");
        let values = engine.snapshot(Server::Global).await.values;

        let mut problems = Vec::new();
        for (pack, def) in kakin_list() {
            for item in def.contents.keys() {
                if values.get_value_from_ja(item) <= 0.0 && !zero_value_items.contains(item) {
                    problems.push(format!(
                        "price_kakin.yaml {pack}: 「{item}」の理性価値が0。コスメなら {ZERO_VALUE_ITEMS_PATH} に追加、\
                         そうでなければ名前の誤字か const_values.yaml への追加漏れ"
                    ));
                }
            }
        }
        // 一覧が陳腐化していないこと(理性価値が付いたアイテムが残っていると、そのアイテムの
        // 名前の誤字を見逃す原因になる)
        for item in &zero_value_items {
            let value = values.get_value_from_ja(item);
            if value > 0.0 {
                problems.push(format!("{ZERO_VALUE_ITEMS_PATH}: 「{item}」は理性価値{value}があるので一覧から消す"));
            }
        }
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }
}
