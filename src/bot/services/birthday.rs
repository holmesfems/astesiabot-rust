use crate::api::AppState;
use crate::bot::reply::{send_embed_reply, EmbedReply, MsgType};
use crate::engine::external_source::operator_birthdays::OperatorBirthdays;
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Utc};
use poise::serenity_prelude as serenity;
use std::sync::Arc;

/// 個別の反映先名。
const REFLECT_DICT: &[(&str, &str)] = &[("アステシア", "私"), ("アステジーニ", "エレナ")];
/// 「さん」ではなく「ちゃん」付けするオペレーター。
const CHAN_LIST: &[&str] = &["スズラン", "ポプカル", "シャマレ", "バブル"];

/// 表示名（日本語）に敬称を付ける。
fn reflect_name(ja_name: &str) -> String {
    if let Some((_, reflect)) = REFLECT_DICT.iter().find(|(name, _)| *name == ja_name) {
        return reflect.to_string();
    }
    if CHAN_LIST.contains(&ja_name) {
        format!("{ja_name}ちゃん")
    } else {
        format!("{ja_name}さん")
    }
}

/// 0件→空文字, 1件→そのまま, 複数件→「、」区切り＋末尾「と」。
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

/// 該当者がいなければ None。
pub fn check_birthday(birthdays: &OperatorBirthdays, now: DateTime<FixedOffset>) -> Option<EmbedReply> {
    let reflected: Vec<String> = birthdays
        .on(now.month(), now.day())
        .map(|op| reflect_name(&op.name))
        .collect();
    if reflected.is_empty() {
        return None;
    }

    let title = ":birthday:お誕生日:birthday:おめでとう:tada:！！".to_string();
    let message = format!(
        "今日は{}の誕生日よ！みんなでお祝い:tada:しましょ！",
        mention_str(&reflected)
    );
    Some(EmbedReply {
        title,
        chunks: vec![message],
        msg_type: MsgType::Ok,
        reply_marker: None,
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

/// 毎日 JST 0:00 に誕生日をチェックして送信し続けるループ。
/// bot起動中ずっと動き続ける想定なので、呼び出し側は `tokio::spawn` すること。
/// 誕生日は `state.external_source` がメモリに保持する最新の一覧を
/// チェックの都度参照する（定期fetchで更新されればここにも反映される）。
pub async fn run(ctx: serenity::Context, channel_id: serenity::ChannelId, state: Arc<AppState>) {
    loop {
        tokio::time::sleep(duration_until_next_midnight_jst()).await;
        let now_jst = Utc::now().with_timezone(&jst());
        let birthdays = state.external_source.operator_birthdays.get().await;
        if let Some(reply) = check_birthday(&birthdays, now_jst) {
            if let Err(e) = send_embed_reply(&ctx, channel_id, &reply).await {
                eprintln!("[birthday] 誕生日メッセージの送信に失敗しました: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::external_source::operator_birthdays::{BirthdayOperator, SEED_PATH};

    fn op(name: &str, month: u8, day: u8) -> BirthdayOperator {
        BirthdayOperator {
            char_id: format!("char_{name}"),
            name: name.to_string(),
            cn_name: name.to_string(),
            month,
            day,
        }
    }

    fn at(month: u32, day: u32) -> DateTime<FixedOffset> {
        jst().with_ymd_and_hms(2026, month, day, 0, 0, 0).single().unwrap()
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
        assert_eq!(reflect_name("バブル"), "バブルちゃん");
        assert_eq!(reflect_name("アステシア"), "私");
        assert_eq!(reflect_name("リード"), "リードさん");
    }

    #[test]
    fn check_birthday_message_format() {
        let birthdays = OperatorBirthdays {
            operators: vec![op("チョンユエ", 1, 1), op("オッダ", 1, 1), op("バブル", 1, 2)],
        };
        let reply = check_birthday(&birthdays, at(1, 1)).expect("Jan 1 has birthdays");
        assert_eq!(
            reply.title,
            ":birthday:お誕生日:birthday:おめでとう:tada:！！"
        );
        assert_eq!(
            reply.chunks,
            vec!["今日はチョンユエさんとオッダさんの誕生日よ！みんなでお祝い:tada:しましょ！".to_string()]
        );
        let reply = check_birthday(&birthdays, at(1, 2)).expect("Jan 2 has birthdays");
        assert_eq!(
            reply.chunks,
            vec!["今日はバブルちゃんの誕生日よ！みんなでお祝い:tada:しましょ！".to_string()]
        );

        // 誕生日オペレーターがいない日は None
        assert!(check_birthday(&birthdays, at(1, 6)).is_none());
    }

    /// Seedの実データで、1月1日（チョンユエ・オッダ）がお祝いされること。
    #[test]
    fn check_birthday_reads_seed() {
        let json = std::fs::read_to_string(SEED_PATH)
            .unwrap_or_else(|e| panic!("seed({SEED_PATH})の読み込みに失敗: {e}。先に`cargo run --bin regen_seeds`を実行すること"));
        let birthdays: OperatorBirthdays = serde_json::from_str(&json).expect("seed should parse");
        let reply = check_birthday(&birthdays, at(1, 1)).expect("Jan 1 has birthdays");
        assert_eq!(
            reply.chunks,
            vec!["今日はチョンユエさんとオッダさんの誕生日よ！みんなでお祝い:tada:しましょ！".to_string()]
        );
    }
}
