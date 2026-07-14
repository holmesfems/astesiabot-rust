use poise::serenity_prelude as serenity;

/// 環境変数からチャンネルIDを読み込む。未設定・不正値なら起動時にpanicする
/// （各サービスの `from_env()` 相当から呼ばれる想定）。
pub fn channel_id_env(key: &str) -> serenity::ChannelId {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} not set"))
        .parse()
        .unwrap_or_else(|_| panic!("{key} is not a valid channel id"))
}
