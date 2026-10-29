//! 課金パックの理性効率。
//!
//! 元はbotの`/riseikakin`コマンド層に閉じていたが、REST API(`GET /api/risei/lists/kakin`)からも
//! 同じ計算結果を返すためengineへ移した（api は bot に依存しないため）。整形(embed/xlsx)は
//! 引き続きbot側の責務。計算式はbot時代から変えていない。
//!
//! 理性価値の重いドロップデータには触れず、[`RiseiValues`](理性価値表のキャッシュ)と価格表
//! (`data/risei/price_kakin.yaml`)だけで計算する。毎回の計算は掛け算と足し算だけなので
//! パック効率自体はキャッシュしない（理性価値表の更新タイミングにそのまま追従する）。
//!
//! グローバル版のみ対応。Mainland版の課金パックは使用頻度が低いため対応していない。

use super::values::RiseiValues;
use chrono::{Duration, FixedOffset, NaiveDateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const KAKIN_LIST_PATH: &str = "data/risei/price_kakin.yaml";
const CONST_GACHA_PATH: &str = "data/risei/const_gacha.yaml";

/// 課金パック一覧の基準として使う恒常パック名。
pub const BASIC_PACK_NAME: &str = "10000円恒常パック";

/// `data/risei/price_kakin.yaml` の1パック分。
#[derive(Debug, Clone, Deserialize)]
pub struct KakinPackDef {
    pub price: f64,
    #[serde(rename = "isConstant")]
    pub is_constant: bool,
    /// 販売期間(日本時間。`2026-10-28 16:00`の形)。無ければその側は無制限。
    #[serde(default, deserialize_with = "deserialize_jst")]
    pub start: Option<NaiveDateTime>,
    /// 販売終了(日本時間)。公式の「〜3:59」をそのまま書き、その1分間も販売中とみなす。
    #[serde(default, deserialize_with = "deserialize_jst")]
    pub end: Option<NaiveDateTime>,
    /// 日本語名→個数。順序は表示順に使うため`IndexMap`でYAML記載順を保持する。
    pub contents: IndexMap<String, f64>,
}

const SALE_TIME_FORMAT: &str = "%Y-%m-%d %H:%M";

fn deserialize_jst<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<NaiveDateTime>, D::Error> {
    let s = String::deserialize(d)?;
    NaiveDateTime::parse_from_str(&s, SALE_TIME_FORMAT)
        .map(Some)
        .map_err(|e| serde::de::Error::custom(format!("日時は「{SALE_TIME_FORMAT}」(日本時間)で書く: {s}: {e}")))
}

impl KakinPackDef {
    /// `now`(日本時間)に販売中か。`end`は分単位の表記なので、その分の終わりまで含める。
    pub fn is_on_sale(&self, now: NaiveDateTime) -> bool {
        self.start.is_none_or(|start| start <= now)
            && self.end.is_none_or(|end| now < end + Duration::minutes(1))
    }
}

/// 販売期間の判定に使う今の日本時間。
pub fn jst_now() -> NaiveDateTime {
    Utc::now().with_timezone(&FixedOffset::east_opt(9 * 3600).expect("valid JST offset")).naive_local()
}

/// 軽量な価格表のみ依存。起動時ロードのStaticDataとは別に、初回アクセス時に一度だけ読み込む
/// （Design.md「autoCompletion_riseikakinのみ価格表依存で早く動く」の通り）。
pub fn kakin_list() -> &'static IndexMap<String, KakinPackDef> {
    static LIST: OnceLock<IndexMap<String, KakinPackDef>> = OnceLock::new();
    LIST.get_or_init(|| {
        let s = std::fs::read_to_string(KAKIN_LIST_PATH)
            .expect("price_kakin.yamlの読み込みに失敗しました");
        serde_saphyr::from_str(&s).expect("price_kakin.yamlのパースに失敗しました")
    })
}

fn const_gacha() -> &'static HashMap<String, f64> {
    static GACHA: OnceLock<HashMap<String, f64>> = OnceLock::new();
    GACHA.get_or_init(|| {
        let s = std::fs::read_to_string(CONST_GACHA_PATH)
            .expect("const_gacha.yamlの読み込みに失敗しました");
        serde_saphyr::from_str(&s).expect("const_gacha.yamlのパースに失敗しました")
    })
}

/// 計算済みの課金パック理性効率。
#[derive(Debug, Clone)]
pub struct KakinPack {
    pub name: String,
    pub price: f64,
    pub start: Option<NaiveDateTime>,
    pub end: Option<NaiveDateTime>,
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

/// パック定義から理性効率を計算する。基準パック(`BASIC_PACK_NAME`)は`price_kakin.yaml`に
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
        start: def.start,
        end: def.end,
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

/// `now`(日本時間)に販売中の期間限定パック。YAML記載順。
pub fn limited_pack_defs(now: NaiveDateTime) -> impl Iterator<Item = (&'static String, &'static KakinPackDef)> {
    kakin_list().iter().filter(move |(_, def)| !def.is_constant && def.is_on_sale(now))
}

/// 販売中の期間限定パック（`/riseikakin`の全体比較）。総合効率の降順。
pub fn limited_packs(values: &RiseiValues, now: NaiveDateTime) -> Vec<KakinPack> {
    let mut packs: Vec<KakinPack> = limited_pack_defs(now)
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
    /// 販売期間(日本時間、`2026-10-28T16:00:00+09:00`の形)。分からないパックは`null`。
    /// `sale_end`は公式表記の「〜3:59」そのままで、その分の終わりまで販売中。
    pub sale_start: Option<String>,
    pub sale_end: Option<String>,
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

fn to_iso_jst(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S+09:00").to_string()
}

pub fn kakin_comparison(values: &RiseiValues, now: NaiveDateTime) -> KakinComparison {
    let baselines = constant_packs(values)
        .into_iter()
        .map(|pack| KakinBaseline { name: pack.name, total_efficiency: pack.total_efficiency })
        .collect();
    let packs = limited_packs(values, now)
        .into_iter()
        .map(|pack| KakinPackView {
            sale_start: pack.start.map(to_iso_jst),
            sale_end: pack.end.map(to_iso_jst),
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

    fn jst(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, SALE_TIME_FORMAT).unwrap()
    }

    #[test]
    fn sale_period_includes_start_and_the_last_minute() {
        let def: KakinPackDef = serde_saphyr::from_str(
            "price: 2600\nisConstant: false\nstart: 2026-10-28 16:00\nend: 2026-11-25 03:59\ncontents:\n  純正源石: 9\n",
        )
        .unwrap();
        assert!(!def.is_on_sale(jst("2026-10-28 15:59")));
        assert!(def.is_on_sale(jst("2026-10-28 16:00")));
        assert!(def.is_on_sale(jst("2026-11-25 03:59") + Duration::seconds(59)));
        assert!(!def.is_on_sale(jst("2026-11-25 04:00")));
    }

    /// 書き間違い(開始と終了の取り違え、恒常パックへの期限)で黙って消えないように。
    #[test]
    fn sale_periods_in_price_list_are_consistent() {
        for (name, def) in kakin_list() {
            if def.is_constant {
                assert!(def.start.is_none() && def.end.is_none(), "{name}: 恒常パックに販売期間は書かない");
            }
            if let (Some(start), Some(end)) = (def.start, def.end) {
                assert!(start < end, "{name}: start が end より後");
            }
        }
    }

    /// `get_value_from_ja`は知らない名前を黙って0にするため、YAMLの誤字や理性価値表に無い
    /// アイテムで効率が低く出ても気づけない。価値0は`zero_value_items.yaml`での明示を必須にする。
    #[tokio::test]
    async fn every_pack_item_has_risei_value() {
        let zero_value_items: Vec<String> = serde_saphyr::from_str(
            &std::fs::read_to_string(ZERO_VALUE_ITEMS_PATH).expect("zero_value_items.yamlを読める"),
        )
        .expect("zero_value_items.yamlは文字列のリスト");
        let outer_source = ExternalSourceRegistry::load(true).await;
        let engine = RiseiCalculatorEngine::load(&outer_source).await.expect("seedから理性価値表を計算できる");
        let values = engine.snapshot(Server::Global).await.values;

        let mut problems = Vec::new();
        for (pack, def) in kakin_list() {
            for item in def.contents.keys() {
                // const_values.yamlに明示的に書かれていれば0でもよい（意図して0と定義したもの）
                if values.get_value_from_ja(item) <= 0.0
                    && !zero_value_items.contains(item)
                    && !values.const_values.contains_key(item.as_str())
                {
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
