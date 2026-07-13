use crate::api::AppState;
use std::sync::Arc;

/// 全コマンド・ハンドラで共有する状態。
pub struct Data {
    pub state: Arc<AppState>,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;
