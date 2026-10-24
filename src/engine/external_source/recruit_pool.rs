//! 公開求人のプール（募集可能オペレーター一覧）。
//!
//! 名前と★は `gacha_table.json` の `recruitDetail`（ゲーム内「公開求人説明」画面の文章
//! そのもの）から、職業・位置・タグは `character_table.json` から引く。CN/JPの両方を
//! 取得し、CNにあってJPに無いもの（CNで先に追加された分）を`cn_only`に分ける。
//! `cn_only`をいつJPのプールとして扱うかは、recruit側の`data/recruitment/jp_release.yaml`
//! （手動設定の反映日）が決める。
//!
//! `recruitDetail`は人が書いた表示用の文章なので表記ゆれがある（CNは改行が文字の`\n`、
//! JPは`/`の前後のスペース抜けや全角スペース混入）。[`parse_recruit_detail`]はそれらを
//! 吸収する。一方、名前がcharacter_tableで引けない・知らないタグが出てきた場合は
//! **fetch失敗として扱う**（黙ってそのオペレーターを落とすと計算結果が気づかれずに
//! 間違うため。失敗なら`Source`が直前の値/Seedを使い続ける）。

use super::cache::write_seed_file;
use super::http::{client, fetch_typed_with_retry};
use super::{BoxFuture, FetchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const GACHA_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/gacha_table.json";
const GACHA_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/gacha_table.json";
const CHAR_TABLE_URL_CN: &str = "https://raw.githubusercontent.com/Kengxxiao/ArknightsGameData/master/zh_CN/gamedata/excel/character_table.json";
const CHAR_TABLE_URL_JP: &str = "https://raw.githubusercontent.com/ArknightsAssets/ArknightsGamedata/refs/heads/master/jp/gamedata/excel/character_table.json";

/// Seedの保存先。`cargo run --bin regen_seeds` で手動生成し、git commitして
/// リポジトリに含めておく（起動時fetchが失敗した場合のフォールバック用）。
pub const SEED_PATH: &str = "data/seed/recruit_pool.json";

/// 求人計算が知っているタグ一覧（職業・位置・その他）。ここに無いタグが出てきたら
/// fetch失敗にする（tagList.jsonへの追加漏れに気づけるように）。
const TAG_LIST_JSON: &str = include_str!("../../../data/recruitment/tagList.json");
/// JPのcharacter_tableに無い（JP未実装の）オペレーターのタグをCN→JAに訳す辞書。
/// OCRのmatcherと同じファイルを使う。
const TAG_ZH_TO_JA_YAML: &str = include_str!("../../../data/recruitment/tagZhToJa.yaml");
/// JP未実装オペレーターの仮訳（operator_dataと同じファイル）。
const CUSTOM_OPERATOR_ZH_TO_JA_YAML: &str = include_str!("../../../data/customOperatorZhToJa.yaml");

const PROFESSION_TO_JOB: &[(&str, &str)] = &[
    ("PIONEER", "先鋒"),
    ("WARRIOR", "前衛"),
    ("TANK", "重装"),
    ("SNIPER", "狙撃"),
    ("CASTER", "術師"),
    ("MEDIC", "医療"),
    ("SUPPORT", "補助"),
    ("SPECIAL", "特殊"),
];

const POSITION_TO_TAG: &[(&str, &str)] = &[("MELEE", "近距離"), ("RANGED", "遠距離")];

/// 公開求人のプール。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RecruitPool {
    /// JPの募集可能一覧に載っているオペレーター（掲載順）。
    pub jp: Vec<PoolOperator>,
    /// CNの募集可能一覧にあってJPに無いオペレーター（CNの掲載順）。
    pub cn_only: Vec<PoolOperator>,
}

/// 求人対象のオペレーター1体分。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PoolOperator {
    pub char_id: String,
    /// 表示名。JPのcharacter_table → customOperatorZhToJa.yaml → CN名 の順で決める。
    pub name: String,
    pub cn_name: String,
    /// 職業（tagList.jsonの`jobTags`の表記）。
    pub job: String,
    /// 位置（近距離/遠距離）+ character_tableの`tagList`。エリート/上級エリートは★から
    /// 判定する（recruit側の`satisfy_tags`）ので含めない。
    pub tags: Vec<String>,
    pub stars: u8,
}

pub fn fetch() -> BoxFuture<'static, Result<RecruitPool, FetchError>> {
    Box::pin(fetch_impl())
}

pub fn update_seed() -> BoxFuture<'static, Result<(), FetchError>> {
    Box::pin(async {
        let data = fetch_impl().await?;
        write_seed_file(SEED_PATH, &data)
    })
}

#[derive(Deserialize)]
struct RawGachaTable {
    #[serde(rename = "recruitDetail")]
    recruit_detail: String,
}

/// character_table.jsonの1エントリのうち、求人に必要なフィールドだけ。
/// トークン・罠等の`char_`以外のエントリも同じ型で読むため全て省略可能にする。
#[derive(Deserialize)]
struct RawChar {
    #[serde(default)]
    name: String,
    /// CN/JPとも現状は"TIER_n"だが、int（0始まり）の版もあったので両方受ける。
    #[serde(default)]
    rarity: Value,
    #[serde(default)]
    profession: String,
    #[serde(default)]
    position: String,
    #[serde(default, rename = "tagList")]
    tag_list: Option<Vec<String>>,
    #[serde(default, rename = "isNotObtainable")]
    is_not_obtainable: bool,
}

type CharTable = HashMap<String, RawChar>;

async fn fetch_impl() -> Result<RecruitPool, FetchError> {
    let client = client();
    let (cn_gacha, jp_gacha, cn_chars, jp_chars) = tokio::try_join!(
        fetch_typed_with_retry::<RawGachaTable>(&client, GACHA_TABLE_URL_CN),
        fetch_typed_with_retry::<RawGachaTable>(&client, GACHA_TABLE_URL_JP),
        fetch_typed_with_retry::<CharTable>(&client, CHAR_TABLE_URL_CN),
        fetch_typed_with_retry::<CharTable>(&client, CHAR_TABLE_URL_JP),
    )?;
    build(&cn_gacha.recruit_detail, &jp_gacha.recruit_detail, &cn_chars, &jp_chars)
}

/// `recruitDetail`から（★, 名前）を掲載順に取り出す。
///
/// 「★だけの行」で★の節が始まり、`-----`の区切り線か次の★行で終わる。節の中の行は
/// リッチテキストのタグ（`<@rc.eml>…</>`。緑字＝公開求人限定）を取り除いて`/`で分割する。
/// CNは改行が文字の`\n`のまま入っているので先に本物の改行へ直す。全角スペースは
/// `str::trim`が落とす（Unicodeの空白扱い）。
pub(crate) fn parse_recruit_detail(text: &str) -> Vec<(u8, String)> {
    let normalized = text.replace("\\n", "\n");
    let mut current_star: Option<u8> = None;
    let mut out = Vec::new();
    for line in normalized.lines() {
        let line = line.trim();
        if !line.is_empty() && line.chars().all(|c| c == '★') {
            current_star = Some(line.chars().count() as u8);
            continue;
        }
        if line.starts_with("---") {
            current_star = None;
            continue;
        }
        let Some(star) = current_star else { continue };
        for name in strip_rich_text(line).split('/') {
            let name = name.trim();
            if !name.is_empty() {
                out.push((star, name.to_string()));
            }
        }
    }
    out
}

/// `<@rc.eml>名前</>`のようなリッチテキストのタグを取り除く。
fn strip_rich_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// tagList.json のうち、ここで使うフィールドだけ。
#[derive(Deserialize)]
struct TagListJson {
    #[serde(rename = "jobTags")]
    job_tags: Vec<String>,
    #[serde(rename = "positionTags")]
    position_tags: Vec<String>,
    #[serde(rename = "otherTags")]
    other_tags: Vec<String>,
}

/// 埋め込み辞書（タグ一覧・CN→JAタグ訳・オペレーター名の仮訳）。
struct Dictionaries {
    job_tags: HashSet<String>,
    /// 位置タグ + その他タグ（オペレーターの`tags`に入ってよいもの）。
    operator_tags: HashSet<String>,
    tag_zh_to_ja: HashMap<String, String>,
    custom_names: HashMap<String, String>,
}

impl Dictionaries {
    fn embedded() -> Result<Self, FetchError> {
        let tag_list: TagListJson = serde_json::from_str(TAG_LIST_JSON)?;
        let tag_zh_to_ja: HashMap<String, String> = serde_saphyr::from_str(TAG_ZH_TO_JA_YAML)?;
        let custom_names: Option<HashMap<String, String>> = serde_saphyr::from_str(CUSTOM_OPERATOR_ZH_TO_JA_YAML)?;
        Ok(Self {
            job_tags: tag_list.job_tags.into_iter().collect(),
            operator_tags: tag_list.position_tags.into_iter().chain(tag_list.other_tags).collect(),
            tag_zh_to_ja,
            custom_names: custom_names.unwrap_or_default(),
        })
    }
}

fn build(cn_detail: &str, jp_detail: &str, cn_chars: &CharTable, jp_chars: &CharTable) -> Result<RecruitPool, FetchError> {
    let dict = Dictionaries::embedded()?;
    let jp_ids = resolve_ids(&parse_recruit_detail(jp_detail), jp_chars, "JP")?;
    let cn_ids = resolve_ids(&parse_recruit_detail(cn_detail), cn_chars, "CN")?;
    if jp_ids.is_empty() || cn_ids.is_empty() {
        return Err(format!("recruitDetailからオペレーターを読み取れませんでした（JP {}体 / CN {}体）", jp_ids.len(), cn_ids.len()).into());
    }

    let jp_set: HashSet<&str> = jp_ids.iter().map(|(id, _)| id.as_str()).collect();
    let jp = jp_ids
        .iter()
        .map(|(id, star)| make_operator(id, *star, cn_chars, jp_chars, &dict))
        .collect::<Result<Vec<_>, _>>()?;
    let cn_only = cn_ids
        .iter()
        .filter(|(id, _)| !jp_set.contains(id.as_str()))
        .map(|(id, star)| make_operator(id, *star, cn_chars, jp_chars, &dict))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RecruitPool { jp, cn_only })
}

/// `recruitDetail`の名前をcharacter_tableのIDへ解決する（CN/JPの突き合わせをIDで行うため。
/// 名前で突き合わせると「THRM-EX」→「サーマル-EX」のような表記変更で別人扱いになる）。
/// 入手不可（`isNotObtainable`）のエントリは予備隊員等の同名が重複するので除外する。
fn resolve_ids(entries: &[(u8, String)], chars: &CharTable, region: &str) -> Result<Vec<(String, u8)>, FetchError> {
    let mut name_to_ids: HashMap<&str, Vec<&str>> = HashMap::new();
    for (id, c) in chars {
        if id.starts_with("char_") && !c.is_not_obtainable {
            name_to_ids.entry(c.name.as_str()).or_default().push(id.as_str());
        }
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for (star, name) in entries {
        let id = match name_to_ids.get(name.as_str()).map(Vec::as_slice) {
            Some([id]) => *id,
            Some(ids) => return Err(format!("{region}のrecruitDetailにある「{name}」がcharacter_tableで複数のIDに当たります: {ids:?}").into()),
            None => return Err(format!("{region}のrecruitDetailにある「{name}」がcharacter_tableに見つかりません").into()),
        };
        if seen.insert(id) {
            out.push((id.to_string(), *star));
        }
    }
    Ok(out)
}

fn make_operator(id: &str, listed_star: u8, cn_chars: &CharTable, jp_chars: &CharTable, dict: &Dictionaries) -> Result<PoolOperator, FetchError> {
    let cn = cn_chars.get(id);
    let jp = jp_chars.get(id);
    let cn_name = cn.or(jp).map(|c| c.name.clone()).unwrap_or_default();
    let name = match jp {
        Some(jp) => jp.name.clone(),
        None => dict.custom_names.get(&cn_name).cloned().unwrap_or_else(|| cn_name.clone()),
    };

    // JPのcharacter_tableにあればJPのタグ（そのまま日本語）、無ければCNのタグを訳す。
    let (source, translate) = match (jp, cn) {
        (Some(jp), _) => (jp, false),
        (None, Some(cn)) => (cn, true),
        (None, None) => return Err(format!("{id}がcharacter_tableに見つかりません").into()),
    };

    let job = lookup(PROFESSION_TO_JOB, &source.profession)
        .ok_or_else(|| format!("{name}: 知らない職業 {}", source.profession))?;
    if !dict.job_tags.contains(job) {
        return Err(format!("{name}: 職業「{job}」がtagList.jsonのjobTagsにありません").into());
    }
    let position = lookup(POSITION_TO_TAG, &source.position)
        .ok_or_else(|| format!("{name}: 知らない位置 {}", source.position))?;

    let mut tags = vec![position.to_string()];
    for tag in source.tag_list.iter().flatten() {
        let tag = if translate {
            dict.tag_zh_to_ja
                .get(tag)
                .cloned()
                .ok_or_else(|| format!("{name}: タグ「{tag}」がtagZhToJa.yamlにありません"))?
        } else {
            tag.clone()
        };
        if !dict.operator_tags.contains(&tag) {
            return Err(format!("{name}: タグ「{tag}」がtagList.jsonにありません").into());
        }
        tags.push(tag);
    }

    let stars = parse_stars(&source.rarity).ok_or_else(|| format!("{name}: rarityを解釈できません: {}", source.rarity))?;
    if stars != listed_star {
        return Err(format!("{name}: recruitDetailでは★{listed_star}なのにcharacter_tableでは★{stars}です").into());
    }

    Ok(PoolOperator {
        char_id: id.to_string(),
        name,
        cn_name,
        job: job.to_string(),
        tags,
        stars,
    })
}

fn lookup(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// "TIER_n" / int（0始まり、+1する）のどちらでも★の数を解決する。
fn parse_stars(rarity: &Value) -> Option<u8> {
    match rarity {
        Value::String(s) => s.strip_prefix("TIER_")?.parse().ok(),
        Value::Number(n) => n.as_u64().map(|v| v as u8 + 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names(entries: &[(u8, String)]) -> Vec<(u8, &str)> {
        entries.iter().map(|(s, n)| (*s, n.as_str())).collect()
    }

    #[test]
    fn parses_jp_style_detail_with_spacing_quirks() {
        // JPは本物の改行。ヘッダの「★6」は★だけの行ではないので節として扱わない。
        // `/`前後のスペース抜け・全角スペース混入もそのまま通ること。
        let text = "<@rc.title>公開求人説明</>\n\
                    <@rc.em>求人結果に「★6オペレーター」が必ず出現します。</>\n\n\
                    ★\n<@rc.eml>Lancet-2</> / <@rc.eml>Castle-3</>\n\
                    --------------------\n\
                    ★★★★★\nイグゼキュター/ ワイフー / シャマレ/ エリジウム /\u{3000}レオンハルト\n\
                    --------------------\n\
                    ★★★★★★\nエクシア / シージ";
        assert_eq!(
            names(&parse_recruit_detail(text)),
            vec![
                (1, "Lancet-2"),
                (1, "Castle-3"),
                (5, "イグゼキュター"),
                (5, "ワイフー"),
                (5, "シャマレ"),
                (5, "エリジウム"),
                (5, "レオンハルト"),
                (6, "エクシア"),
                (6, "シージ"),
            ]
        );
    }

    #[test]
    fn parses_cn_style_detail_with_literal_backslash_n() {
        // CNは★の後ろの改行が文字の`\n`（バックスラッシュ+n）のまま入っている。
        let text = "<@rc.subtitle>※全部可能出现的干员※</>\n\n\
                    ★\\n<@rc.eml>Lancet-2</> / <@rc.eml>GALLUS²</>\n\
                    --------------------\n\
                    ★★★★★★\\n能天使 / 帕拉斯";
        assert_eq!(
            names(&parse_recruit_detail(text)),
            vec![(1, "Lancet-2"), (1, "GALLUS²"), (6, "能天使"), (6, "帕拉斯")]
        );
    }

    fn char_entry(name: &str, rarity: Value, profession: &str, position: &str, tags: &[&str]) -> RawChar {
        RawChar {
            name: name.to_string(),
            rarity,
            profession: profession.to_string(),
            position: position.to_string(),
            tag_list: Some(tags.iter().map(|t| t.to_string()).collect()),
            is_not_obtainable: false,
        }
    }

    /// CN: エクシア・パラス・GALLUS²、JP: エクシアだけプール入り（パラスはキャラとしてはJP実装済み、
    /// GALLUS²はJP未実装）。
    fn sample_tables() -> (CharTable, CharTable) {
        let mut cn = CharTable::new();
        cn.insert("char_103_angel".into(), char_entry("能天使", json!("TIER_6"), "SNIPER", "RANGED", &["输出"]));
        cn.insert("char_485_pallas".into(), char_entry("帕拉斯", json!("TIER_6"), "WARRIOR", "MELEE", &["输出", "支援"]));
        cn.insert("char_4227_gallus".into(), char_entry("GALLUS²", json!("TIER_1"), "CASTER", "RANGED", &["支援机械", "削弱"]));
        // 同名の入手不可エントリ（予備隊員等）は名前解決から除外されること。
        let mut dummy = char_entry("能天使", json!("TIER_1"), "SNIPER", "RANGED", &[]);
        dummy.is_not_obtainable = true;
        cn.insert("char_999_dummy".into(), dummy);

        let mut jp = CharTable::new();
        jp.insert("char_103_angel".into(), char_entry("エクシア", json!("TIER_6"), "SNIPER", "RANGED", &["火力"]));
        jp.insert("char_485_pallas".into(), char_entry("パラス", json!(5), "WARRIOR", "MELEE", &["火力", "支援"]));
        (cn, jp)
    }

    const CN_DETAIL: &str = "★\\nGALLUS²\n--------------------\n★★★★★★\\n能天使 / 帕拉斯";
    const JP_DETAIL: &str = "★★★★★★\nエクシア";

    #[test]
    fn builds_jp_pool_and_cn_only_additions_by_char_id() {
        let (cn, jp) = sample_tables();
        let pool = build(CN_DETAIL, JP_DETAIL, &cn, &jp).expect("should build");

        assert_eq!(
            pool.jp,
            vec![PoolOperator {
                char_id: "char_103_angel".into(),
                name: "エクシア".into(),
                cn_name: "能天使".into(),
                job: "狙撃".into(),
                tags: vec!["遠距離".into(), "火力".into()],
                stars: 6,
            }]
        );
        // CNの掲載順。JPのcharacter_tableにあるパラスはJPの名前・タグ、
        // 無いGALLUS²はcustomOperatorZhToJaの仮訳とCNタグの翻訳を使う。
        assert_eq!(
            pool.cn_only,
            vec![
                PoolOperator {
                    char_id: "char_4227_gallus".into(),
                    name: "GALLUS²".into(),
                    cn_name: "GALLUS²".into(),
                    job: "術師".into(),
                    tags: vec!["遠距離".into(), "ロボット".into(), "弱化".into()],
                    stars: 1,
                },
                PoolOperator {
                    char_id: "char_485_pallas".into(),
                    name: "パラス".into(),
                    cn_name: "帕拉斯".into(),
                    job: "前衛".into(),
                    tags: vec!["近距離".into(), "火力".into(), "支援".into()],
                    stars: 6,
                },
            ]
        );
    }

    #[test]
    fn unknown_name_fails_instead_of_dropping_operator() {
        let (cn, jp) = sample_tables();
        let err = build(CN_DETAIL, "★★★★★★\nエクシア / 誰か", &cn, &jp).unwrap_err();
        assert!(err.to_string().contains("誰か"), "{err}");
    }

    #[test]
    fn unknown_tag_fails() {
        let (cn, mut jp) = sample_tables();
        jp.insert("char_103_angel".into(), char_entry("エクシア", json!("TIER_6"), "SNIPER", "RANGED", &["新タグ"]));
        let err = build(CN_DETAIL, JP_DETAIL, &cn, &jp).unwrap_err();
        assert!(err.to_string().contains("新タグ"), "{err}");
    }

    #[test]
    fn star_mismatch_between_detail_and_char_table_fails() {
        let (cn, jp) = sample_tables();
        let err = build(CN_DETAIL, "★★★★★\nエクシア", &cn, &jp).unwrap_err();
        assert!(err.to_string().contains("★5"), "{err}");
    }

    /// 実ネットワークでの疎通確認。`cargo test -- --ignored` で明示実行する。
    #[tokio::test]
    #[ignore]
    async fn fetch_gets_real_pool() {
        let pool = fetch().await.expect("fetch should succeed against real network");
        assert!(pool.jp.len() > 100, "JP pool too small: {}", pool.jp.len());
        println!("JP {}体 / CN追加分 {:?}", pool.jp.len(), pool.cn_only.iter().map(|o| &o.name).collect::<Vec<_>>());
    }
}
