use super::send_reply_with_attachment;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::bot::utils::xlsx::{build_kakin_export_xlsx, KakinExportPack};
use crate::engine::risei_calculator_engine::values::RiseiValues;
use crate::engine::risei_calculator_engine::Server;
use indexmap::IndexMap;
use poise::serenity_prelude as serenity;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const KAKIN_XLSX_FILENAME: &str = "kakinList.xlsx";
/// [`value_block`]と同じ並び順(Python `KakinPack.targetValueList`)。
const KAKIN_TARGET_COLUMNS: [&str; 7] =
    ["総合効率", "ガチャ効率", "パック値段", "合計理性価値", "純正源石換算", "マネー換算", "ガチャ数"];

// このファイルは課金パック調査(riseikakin)専用の関心事のため、理性価値計算エンジン
// (risei_calculator_engine)には置かず、コマンド層に閉じている。riseilistsが使う
// 契約賞金引換証(CC)は複数コマンドから使われる共有ロジックのためengine側に残っている
// （`engine/risei_calculator_engine/cc_exchange.rs`）。

const KAKIN_LIST_PATH: &str = "data/risei/price_kakin.yaml";
const CONST_GACHA_PATH: &str = "data/risei/const_gacha.yaml";

/// 課金パック一覧の基準として使う恒常パック名（Python `KakinPack.__init__` の`basicPackName`）。
/// グローバル版のみ対応のため固定値。Mainland版の課金パック対応は元々使用頻度が低くオミット済み
/// （`ref_python/RiseiCalculatorBot-main/riseicalculator2/Design.md`参照）。
const BASIC_PACK_NAME: &str = "10000円恒常パック";

/// riseikakinのtarget="全体比較(グローバル)"相当（Python `totalJATuple`）。
const TOTAL_TARGETS: [&str; 2] = ["全体比較(グローバル)", "Total_Global"];

/// `data/risei/price_kakin.yaml` の1パック分（Python `getKakinList`の値側）。
#[derive(Debug, Clone, Deserialize)]
struct KakinPackDef {
    price: f64,
    #[serde(rename = "isConstant")]
    is_constant: bool,
    /// 日本語名→個数。順序は表示順に使うため`IndexMap`でYAML記載順を保持する。
    contents: IndexMap<String, f64>,
}

/// 軽量な価格表のみ依存。起動時ロードのengine::StaticDataとは別に、初回アクセス時に
/// 一度だけ読み込む（Design.md「autoCompletion_riseikakinのみ価格表依存で早く動く」の通り、
/// このコマンドは理性価値計算エンジンの重いドロップデータに触れる必要が無い）。
fn kakin_list() -> &'static IndexMap<String, KakinPackDef> {
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
#[derive(Clone)]
struct KakinPack {
    name: String,
    price: f64,
    contents: Vec<(String, f64)>,
    total_value: f64,
    total_originium: f64,
    total_real_money: f64,
    total_efficiency: f64,
    gacha_count: f64,
    gacha_efficiency: f64,
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

/// Python `KakinPack.__init__`。基準パック(`BASIC_PACK_NAME`)は`price_kakin.yaml`に
/// 必ず存在する前提。
fn build_kakin_pack(name: &str, def: &KakinPackDef, values: &RiseiValues) -> KakinPack {
    let total_value = pack_value(&def.contents, values);
    let total_originium = total_value / values.get_value_from_ja("純正源石");

    let basic = kakin_list()
        .get(BASIC_PACK_NAME)
        .expect("price_kakin.yamlに基準パック'10000円恒常パック'が存在しません");
    let basic_value = pack_value(&basic.contents, values);
    let total_real_money = total_value / basic_value * basic.price;
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

/// riseikakin の target 引数のオートコンプリート相当（Python `autoCompletion_riseikakin`）。
/// まず期間限定パック名(+全体比較)を部分一致で探し、何も無ければ恒常パック名にフォールバックする。
async fn autocomplete_kakin_target(
    _ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    const TOTAL_OPTION: (&str, &str) = ("全体比較(グローバル)", "Total_Global");
    let limited: Vec<(String, String)> =
        std::iter::once((TOTAL_OPTION.0.to_string(), TOTAL_OPTION.1.to_string()))
            .chain(
                kakin_list()
                    .iter()
                    .filter(|(_, def)| !def.is_constant)
                    .map(|(name, _)| (name.clone(), name.clone())),
            )
            .filter(|(name, _)| name.contains(partial))
            .take(25)
            .collect();
    let names = if !limited.is_empty() {
        limited
    } else {
        kakin_list()
            .iter()
            .filter(|(_, def)| def.is_constant)
            .map(|(name, _)| (name.clone(), name.clone()))
            .filter(|(name, _)| name.contains(partial))
            .take(25)
            .collect()
    };
    names
        .into_iter()
        .map(|(name, value)| serenity::AutocompleteChoice::new(name, value))
        .collect()
}

/// YAMLの個数表示用。整数値なら小数点無しで表示する（Python `str(count)`相当の簡略版）。
fn format_count(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn contents_block(contents: &[(String, f64)]) -> String {
    let lines: Vec<String> = contents
        .iter()
        .map(|(name, count)| format!("{name} × {}", format_count(*count)))
        .collect();
    format!("```\n{}\n```\n", lines.join("\n"))
}

/// Python `KakinPack.strBlock`。
fn value_block(pack: &KakinPack) -> String {
    let lines = [
        format!("総合効率    : {:.2}%", pack.total_efficiency * 100.0),
        format!("ガチャ効率  : {:.2}%", pack.gacha_efficiency * 100.0),
        format!("パック値段  : {:.0}円", pack.price),
        format!("合計理性価値: {:.2}", pack.total_value),
        format!("純正源石換算: {:.2}", pack.total_originium),
        format!("マネー換算  : {:.2}円", pack.total_real_money),
        format!("ガチャ数    : {:.2}", pack.gacha_count),
    ];
    format!("```\n{}\n```\n", lines.join("\n"))
}

/// Python `riseikakin`の`constantStrBlock`（参考用課金効率一覧）。
fn constant_block(constants: &[KakinPack]) -> String {
    let lines: Vec<String> = constants
        .iter()
        .map(|pack| format!("{}: {:.2}%", pack.name, pack.total_efficiency * 100.0))
        .collect();
    format!("参考用課金効率:```\n{}\n```", lines.join("\n"))
}

/// xlsx出力(`csv_file`オプション)用にKakinPackを畳み込む（Python `KakinPack.targetValueList`相当）。
fn to_export_pack(pack: &KakinPack) -> KakinExportPack {
    KakinExportPack {
        name: pack.name.clone(),
        contents: pack.contents.iter().cloned().collect(),
        target_values: vec![
            pack.total_efficiency,
            pack.gacha_efficiency,
            pack.price,
            pack.total_value,
            pack.total_originium,
            pack.total_real_money,
            pack.gacha_count,
        ],
    }
}

/// 列に出す素材名の集合（登場順で重複排除。Python `getMaterialSet`は`set`のため
/// 順序不定だが、内容が合っていればよいのでここでは決定的な順序にしている）。
fn material_columns(packs: &[KakinPack]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut columns = Vec::new();
    for pack in packs {
        for (name, _) in &pack.contents {
            if seen.insert(name.clone()) {
                columns.push(name.clone());
            }
        }
    }
    columns
}

/// riseikakinのxlsx添付を組み立てる（Python `listToCSV`相当）。
fn build_kakin_attachment(packs: &[KakinPack], values: &RiseiValues) -> Result<serenity::CreateAttachment, Error> {
    let columns = material_columns(packs);
    let value_row: Vec<f64> = columns.iter().map(|ja| values.get_value_from_ja(ja)).collect();
    let export_packs: Vec<KakinExportPack> = packs.iter().map(to_export_pack).collect();
    let bytes = build_kakin_export_xlsx(&columns, &KAKIN_TARGET_COLUMNS, &export_packs, &value_row)?;
    Ok(serenity::CreateAttachment::bytes(bytes, KAKIN_XLSX_FILENAME))
}

/// 課金理性効率表を出力します。
#[poise::command(slash_command)]
pub async fn riseikakin(
    ctx: Context<'_>,
    #[description = "表示する効率表を選んでください"]
    #[autocomplete = "autocomplete_kakin_target"]
    target: String,
    #[description = "true:パック内容をxlsxで添付"]
    csv_file: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let state = ctx.data().state.clone();
    let snapshot = state
        .risei_calculator
        .snapshot(Server::Global, &state.external_source)
        .await;
    let values = &snapshot.values;
    let want_csv = csv_file.unwrap_or(false);

    let constants: Vec<KakinPack> = kakin_list()
        .iter()
        .filter(|(_, def)| def.is_constant)
        .map(|(name, def)| build_kakin_pack(name, def, values))
        .collect();

    let (reply, attachment) = if TOTAL_TARGETS.contains(&target.as_str()) {
        let mut limited: Vec<KakinPack> = kakin_list()
            .iter()
            .filter(|(_, def)| !def.is_constant)
            .map(|(name, def)| build_kakin_pack(name, def, values))
            .collect();
        limited.sort_by(|a, b| b.total_efficiency.total_cmp(&a.total_efficiency));

        let mut chunks: Vec<String> = limited
            .iter()
            .map(|pack| format!("{}:{}", pack.name, value_block(pack)))
            .collect();
        chunks.push(constant_block(&constants));
        let attachment = if want_csv {
            let all_packs: Vec<KakinPack> = limited.iter().chain(constants.iter()).cloned().collect();
            Some(build_kakin_attachment(&all_packs, values)?)
        } else {
            None
        };
        let reply = EmbedReply {
            title: "課金パック比較".to_string(),
            chunks,
            msg_type: MsgType::Ok,
            reply_marker: None,
        };
        (reply, attachment)
    } else {
        match kakin_list().get(&target) {
            None => (EmbedReply::error(&format!("存在しない課金パック：{target}")), None),
            Some(def) => {
                let pack = build_kakin_pack(&target, def, values);
                let chunks = vec![
                    format!("内容物:{}", contents_block(&pack.contents)),
                    format!("理性価値情報:{}", value_block(&pack)),
                    constant_block(&constants),
                ];
                let attachment = if want_csv {
                    let mut all_packs = vec![pack.clone()];
                    all_packs.extend(constants.iter().cloned());
                    Some(build_kakin_attachment(&all_packs, values)?)
                } else {
                    None
                };
                let reply = EmbedReply {
                    title: pack.name.clone(),
                    chunks,
                    msg_type: MsgType::Ok,
                    reply_marker: None,
                };
                (reply, attachment)
            }
        }
    };
    send_reply_with_attachment(ctx, reply, attachment).await
}
