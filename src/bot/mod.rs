pub mod commands;
pub mod data;
pub mod handler;

use data::Data;
use poise::serenity_prelude as serenity;

pub async fn run_bot(token: String) {
    // 汎用ハンドラで本文を読むなら MESSAGE_CONTENT を足す。
    // スラッシュコマンドのみなら non_privileged() だけでよい。
    let intents =
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            // ← 大元の振り分け。コマンドを増やすときはここに足す。
            commands: vec![
                commands::ping::ping(),
                commands::echo::echo(),
                commands::add::add(),
            ],
            // ← 汎用メッセージハンドラの登録
            event_handler: |ctx, event, framework, data| {
                Box::pin(handler::event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {})
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await
        .expect("client error");
    client.start().await.expect("bot error");
}