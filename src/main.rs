use astesiabot_rust::api::{run_api, AppState};
use astesiabot_rust::bot::run_bot;
use astesiabot_rust::bot::services::moderation::ModerationState;
use astesiabot_rust::engine;
use chrono::{FixedOffset, TimeZone, Utc};
use std::sync::Arc;

/// 外部サイト情報の定期再fetchを行う時刻（可用性確保のため、利用者が少ない深夜帯に寄せる）。
const OUTER_SOURCE_REFRESH_HOUR_JST: u32 = 3;

fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("valid JST offset")
}

/// 次の JST 3:00 までの Duration。
fn duration_until_next_refresh_jst() -> std::time::Duration {
    let offset = jst();
    let now_jst = Utc::now().with_timezone(&offset);
    let today_3am = offset
        .from_local_datetime(
            &now_jst
                .date_naive()
                .and_hms_opt(OUTER_SOURCE_REFRESH_HOUR_JST, 0, 0)
                .expect("valid time"),
        )
        .single()
        .expect("JST has no DST, always unambiguous");
    let next_3am = if today_3am > now_jst {
        today_3am
    } else {
        today_3am + chrono::Duration::days(1)
    };
    (next_3am - now_jst)
        .to_std()
        .unwrap_or(std::time::Duration::from_secs(1))
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN not set");

    // 起動時に一度だけ求人データをロード。bot と api で共有する。
    let recruit_engine = engine::recruit::RecruitEngine::load().expect("求人データのロードに失敗");
    let moderation = ModerationState::from_env();
    // 外部サイト情報も起動時に一括fetch（失敗時の扱いは Source::load 参照）。
    let outer_source = engine::outer_source::OuterSourceRegistry::load().await;
    // 理性価値表もここで一括計算（グローバル版・大陸版とも）。
    let risei_calculator = engine::risei_calculator_engine::RiseiCalculatorEngine::load(&outer_source)
        .await
        .expect("理性価値表の初期計算に失敗");
    let fk_data_search = engine::fk_data_search::FkDataSearchEngine::new();
    let state = Arc::new(AppState {
        recruit: recruit_engine,
        moderation,
        outer_source,
        risei_calculator,
        fk_data_search,
    });
    let bot_state = state.clone();

    let refresh_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(duration_until_next_refresh_jst()).await;
            refresh_state.outer_source.refresh_all().await;
        }
    });

    println!("astesia-bot rust ready!");

    tokio::select! {
        _ = run_bot(token, bot_state) => {},
        _ = run_api(state) => {},
    }
}
