use crate::api::AppState;
use crate::bot::handler::ChannelRouting;
use std::sync::Arc;

/// 全コマンド・ハンドラで共有する状態。
pub struct Data {
    pub state: Arc<AppState>,
    /// チャンネル別振り分け（handler.rs）で使うチャンネルID群。定義・解決ロジックは
    /// handler.rs 側に閉じている。
    pub channel_routing: ChannelRouting,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;
