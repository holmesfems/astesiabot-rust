pub mod channels;
pub mod commands;
pub mod data;
pub mod handler;
pub mod reply;
pub mod services;

use crate::api::AppState;
use data::Data;
use poise::serenity_prelude as serenity;
use std::sync::Arc;

pub async fn run_bot(token: String, state: Arc<AppState>) {
    // スラッシュコマンドは MESSAGE_CONTENT 不要だが、
    // 汎用メッセージハンドラで本文を読むため MESSAGE_CONTENT を足す。
    let intents =
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            // 大元の振り分け。コマンドを増やすときはここに足す。
            commands: vec![
                commands::ping::ping(),
                commands::echo::echo(),
                commands::add::add(),
            ],
            // 汎用メッセージハンドラの登録
            event_handler: |ctx, event, framework, data| {
                Box::pin(handler::event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data { state })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await
        .expect("client error");
    client.start().await.expect("bot error");
}
