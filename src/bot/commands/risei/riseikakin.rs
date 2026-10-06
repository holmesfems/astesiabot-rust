use super::send_reply_with_attachment;
use crate::bot::data::{Context, Error};
use crate::bot::reply::{EmbedReply, MsgType};
use crate::bot::utils::xlsx::{build_kakin_export_xlsx, KakinExportPack};
use crate::engine::risei_calculator_engine::kakin::{
    build_kakin_pack, constant_packs, kakin_list, limited_packs, KakinPack,
};
use crate::engine::risei_calculator_engine::values::RiseiValues;
use crate::engine::risei_calculator_engine::Server;
use poise::serenity_prelude as serenity;
use std::collections::HashSet;

const KAKIN_XLSX_FILENAME: &str = "kakinList.xlsx";
/// [`value_block`]と同じ並び順(Python `KakinPack.targetValueList`)。
const KAKIN_TARGET_COLUMNS: [&str; 7] =
    ["総合効率", "ガチャ効率", "パック値段", "合計理性価値", "純正源石換算", "マネー換算", "ガチャ数"];

// 計算(価格表の読み込み・パック効率)は`engine/risei_calculator_engine/kakin.rs`にある
// （REST API `GET /api/risei/lists/kakin` と共有するため）。ここは embed / xlsx への整形だけを持つ。

/// riseikakinのtarget="全体比較(グローバル)"相当（Python `totalJATuple`）。
const TOTAL_TARGETS: [&str; 2] = ["全体比較(グローバル)", "Total_Global"];

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

/// 全体比較(グローバル)のembed本文。期間限定パック(総合効率の降順)→参考用課金効率の順。
fn total_comparison_chunks(limited: &[KakinPack], constants: &[KakinPack]) -> Vec<String> {
    let mut chunks: Vec<String> = limited
        .iter()
        .map(|pack| format!("{}:{}", pack.name, value_block(pack)))
        .collect();
    chunks.push(constant_block(constants));
    chunks
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
        .snapshot(Server::Global)
        .await;
    let values = &snapshot.values;
    let want_csv = csv_file.unwrap_or(false);

    let constants = constant_packs(values);

    let (reply, attachment) = if TOTAL_TARGETS.contains(&target.as_str()) {
        let limited = limited_packs(values);
        let chunks = total_comparison_chunks(&limited, &constants);
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

#[cfg(test)]
mod tests {
    //! `/riseikakin`(全体比較・グローバル)のembed本文と`GET /api/risei/lists/kakin`の応答が
    //! 同じ理性価値表から全パック一致することの照合。embedは表示用に丸めた文字列なので、
    //! APIの生の値をembedと同じ書式で丸めて文字列比較する。
    use super::*;
    use crate::engine::external_source::ExternalSourceRegistry;
    use crate::engine::risei_calculator_engine::RiseiCalculatorEngine;
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tower::ServiceExt;

    /// embedの1チャンク(`name:```...```\n`)→(パック名, 項目名→値文字列)。
    fn parse_pack_chunk(chunk: &str) -> (String, HashMap<String, String>) {
        let (name, block) = chunk.split_once(":```\n").expect("パックのチャンク");
        let fields = block
            .lines()
            .filter_map(|line| line.split_once(": "))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        (name.to_string(), fields)
    }

    async fn compare(debug: bool) {
        let outer_source = ExternalSourceRegistry::load(debug).await;
        let engine = Arc::new(RiseiCalculatorEngine::load(&outer_source).await.expect("理性価値表を計算できる"));

        // Discord側: コマンドと同じ関数で本文を組み立てる
        let values = engine.snapshot(Server::Global).await.values;
        let chunks = total_comparison_chunks(&limited_packs(&values), &constant_packs(&values));
        let (constant_chunk, pack_chunks) = chunks.split_last().unwrap();

        // API側: curlと同じくHTTPで叩く
        let response = crate::api::risei::router::<()>(engine.clone())
            .oneshot(Request::builder().uri("/api/risei/lists/kakin").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let packs = body["packs"].as_array().unwrap();
        assert_eq!(packs.len(), pack_chunks.len(), "パック数");

        println!("updated_at={} effective_server={}", body["updated_at"], body["effective_server"]);
        println!("| # | パック | 総合効率 bot / API | ガチャ効率 bot / API | 値段 bot / API | マネー換算 bot / API | 一致 |");
        println!("|---|---|---|---|---|---|---|");
        let pct = |v: &Value| format!("{:.2}%", v.as_f64().unwrap() * 100.0);
        let mut all_ok = true;
        for (i, (chunk, pack)) in pack_chunks.iter().zip(packs).enumerate() {
            let (name, bot) = parse_pack_chunk(chunk);
            let api = [
                ("総合効率", pct(&pack["total_efficiency"])),
                ("ガチャ効率", pct(&pack["gacha_efficiency"])),
                ("パック値段", format!("{:.0}円", pack["price_jpy"].as_f64().unwrap())),
                ("マネー換算", format!("{:.2}円", pack["value_jpy"].as_f64().unwrap())),
                ("合計理性価値", format!("{:.2}", pack["total_value"].as_f64().unwrap())),
                ("純正源石換算", format!("{:.2}", pack["total_originium"].as_f64().unwrap())),
                ("ガチャ数", format!("{:.2}", pack["gacha_count"].as_f64().unwrap())),
            ];
            let ok = name == pack["name"].as_str().unwrap() && api.iter().all(|(k, v)| bot.get(*k) == Some(v));
            all_ok &= ok;
            println!(
                "| {} | {} | {} / {} | {} / {} | {} / {} | {} / {} | {} |",
                i + 1,
                name,
                bot["総合効率"], api[0].1,
                bot["ガチャ効率"], api[1].1,
                bot["パック値段"], api[2].1,
                bot["マネー換算"], api[3].1,
                if ok { "✅" } else { "❌" },
            );
            // contentsの換算額の合計はパックのマネー換算と一致する
            let sum: f64 = pack["contents"].as_array().unwrap().iter().map(|c| c["value_jpy"].as_f64().unwrap()).sum();
            assert!((sum - pack["value_jpy"].as_f64().unwrap()).abs() < 1e-6, "{name}: contents合計");
        }

        println!();
        println!("| 比較用(恒常) | bot | API | 一致 |");
        println!("|---|---|---|---|");
        let bot_constants: Vec<(String, String)> = constant_chunk
            .lines()
            .filter_map(|line| line.rsplit_once(": "))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let baselines = body["baselines"].as_array().unwrap();
        assert_eq!(bot_constants.len(), baselines.len(), "恒常パック数");
        for ((bot_name, bot_pct), base) in bot_constants.iter().zip(baselines) {
            let api_pct = pct(&base["total_efficiency"]);
            let ok = bot_name == base["name"].as_str().unwrap() && *bot_pct == api_pct;
            all_ok &= ok;
            println!("| {bot_name} | {bot_pct} | {api_pct} | {} |", if ok { "✅" } else { "❌" });
        }
        assert!(all_ok, "/riseikakin と API が一致しない項目がある");
    }

    #[tokio::test]
    async fn api_matches_discord_embed_on_seed() {
        compare(true).await;
    }

    /// 実データ(ネットワークfetch)で照合する。`cargo test api_matches_discord_embed_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn api_matches_discord_embed_live() {
        dotenvy::dotenv().ok();
        compare(false).await;
    }
}
