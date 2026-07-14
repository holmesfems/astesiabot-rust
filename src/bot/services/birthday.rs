use crate::bot::data::Error;
use crate::bot::reply::{send_embed_reply, EmbedReply, MsgType};
use crate::engine::operator_names::OperatorNames;
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Utc};
use poise::serenity_prelude as serenity;
use std::collections::HashMap;

/// 個別の反映先名（Python の reflectDict 相当）。
const REFLECT_DICT: &[(&str, &str)] = &[("アステシア", "私"), ("アステジーニ", "エレナ")];
/// 「さん」ではなく「ちゃん」付けするオペレーター（Python の chanList 相当）。
const CHAN_LIST: &[&str] = &["スズラン", "ポプカル", "シャマレ", "バブル"];

/// 日付（"N月N日"）→ 誕生日オペレーターの中国語名リスト。
/// happybirthday.py の birthdayRevDict 相当。
pub struct BirthdayData {
    by_date: HashMap<String, Vec<String>>,
}

impl BirthdayData {
    pub fn load() -> Result<Self, Error> {
        let by_date: HashMap<String, Vec<String>> =
            serde_yaml::from_str(&std::fs::read_to_string("data/birthdayRev.yaml")?)?;
        Ok(Self { by_date })
    }
}

/// Python の reflectName 相当。中国語名→表示名（日本語化＋敬称付け）。
fn reflect_name(names: &OperatorNames, cn_name: &str) -> String {
    let ja_name = names.to_ja(cn_name);
    if let Some((_, reflect)) = REFLECT_DICT.iter().find(|(name, _)| *name == ja_name) {
        return reflect.to_string();
    }
    if CHAN_LIST.contains(&ja_name) {
        format!("{ja_name}ちゃん")
    } else {
        format!("{ja_name}さん")
    }
}

/// Python の mentionStr 相当。0件→空文字, 1件→そのまま, 複数件→「、」区切り＋末尾「と」。
fn mention_str(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        _ => {
            let (last, rest) = names.split_last().expect("names is non-empty here");
            format!("{}と{last}", rest.join("、"))
        }
    }
}

/// Python の checkBirthday 相当。該当者がいなければ None。
pub fn check_birthday(
    data: &BirthdayData,
    names: &OperatorNames,
    now: DateTime<FixedOffset>,
) -> Option<EmbedReply> {
    let key = format!("{}月{}日", now.month(), now.day());
    let birth_operators = data.by_date.get(&key)?;
    if birth_operators.is_empty() {
        return None;
    }

    let reflected: Vec<String> = birth_operators
        .iter()
        .map(|cn_name| reflect_name(names, cn_name))
        .collect();

    let title = ":birthday:お誕生日:birthday:おめでとう:tada:！！".to_string();
    let message = format!(
        "今日は{}の誕生日よ！みんなでお祝い:tada:しましょ！",
        mention_str(&reflected)
    );
    Some(EmbedReply {
        title,
        chunks: vec![message],
        msg_type: MsgType::Ok,
    })
}

fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("valid JST offset")
}

/// 次の JST 0:00 までの Duration。
fn duration_until_next_midnight_jst() -> std::time::Duration {
    let offset = jst();
    let now_jst = Utc::now().with_timezone(&offset);
    let tomorrow = now_jst.date_naive() + chrono::Duration::days(1);
    let next_midnight = offset
        .from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).expect("valid time"))
        .single()
        .expect("JST has no DST, always unambiguous");
    (next_midnight - now_jst)
        .to_std()
        .unwrap_or(std::time::Duration::from_secs(1))
}

/// 毎日 JST 0:00 に誕生日をチェックして送信し続けるループ（Python の
/// `@tasks.loop(time=datetime.time(hour=0, minute=0, tzinfo=JST))` 相当）。
/// bot起動中ずっと動き続ける想定なので、呼び出し側は `tokio::spawn` すること。
pub async fn run(ctx: serenity::Context, channel_id: serenity::ChannelId) {
    let data = match BirthdayData::load() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[birthday] birthdayRev.yaml の読み込みに失敗しました: {e}");
            return;
        }
    };
    let names = OperatorNames::load().await;

    loop {
        tokio::time::sleep(duration_until_next_midnight_jst()).await;
        let now_jst = Utc::now().with_timezone(&jst());
        if let Some(reply) = check_birthday(&data, &names, now_jst) {
            if let Err(e) = send_embed_reply(&ctx, channel_id, &reply).await {
                eprintln!("[birthday] 誕生日メッセージの送信に失敗しました: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> OperatorNames {
        OperatorNames::empty_for_test()
    }

    #[test]
    fn mention_str_joins_with_japanese_conjunctions() {
        assert_eq!(mention_str(&[]), "");
        assert_eq!(mention_str(&["リード".to_string()]), "リード");
        assert_eq!(
            mention_str(&["リード".to_string(), "バブル".to_string()]),
            "リードとバブル"
        );
        assert_eq!(
            mention_str(&[
                "リード".to_string(),
                "バブル".to_string(),
                "シージ".to_string()
            ]),
            "リード、バブルとシージ"
        );
    }

    #[test]
    fn reflect_name_applies_chan_and_overrides() {
        let names = names();
        assert_eq!(reflect_name(&names, "バブル"), "バブルちゃん");
        assert_eq!(reflect_name(&names, "アステシア"), "私");
        assert_eq!(reflect_name(&names, "リード"), "リードさん");
    }

    #[test]
    fn check_birthday_reads_real_data_and_matches_python_format() {
        let data = BirthdayData::load().expect("data/birthdayRev.yaml should load");
        let names = names();
        // data/birthdayRev.yaml の "1月1日: [重岳, 奥达]" に対応
        // （CN→JA変換は空辞書なのでCN名のまま2件連結される）。
        let jan1 = jst()
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .unwrap();
        let reply = check_birthday(&data, &names, jan1).expect("Jan 1 has birthdays");
        assert_eq!(reply.title, ":birthday:お誕生日:birthday:おめでとう:tada:！！");
        assert_eq!(
            reply.chunks,
            vec!["今日は重岳さんと奥达さんの誕生日よ！みんなでお祝い:tada:しましょ！".to_string()]
        );

        // 誕生日オペレーターがいない日は None（1月6日はdata/birthdayRev.yamlに無い）
        let no_birthday_day = jst()
            .with_ymd_and_hms(2026, 1, 6, 0, 0, 0)
            .single()
            .unwrap();
        assert!(check_birthday(&data, &names, no_birthday_day).is_none());
    }
}
