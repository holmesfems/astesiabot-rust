use poise::serenity_prelude as serenity;
use std::str::FromStr;

/// 環境変数からDiscordのID系の値を読み込む。未設定・不正値なら起動時にpanicする
/// （各サービスの `from_env()` 相当から呼ばれる想定）。
fn parse_id_env<T: FromStr>(key: &str) -> T {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("{key} not set"))
        .parse()
        .unwrap_or_else(|_| panic!("{key} is not a valid id"))
}

pub fn channel_id_env(key: &str) -> serenity::ChannelId {
    parse_id_env(key)
}

pub fn guild_id_env(key: &str) -> serenity::GuildId {
    parse_id_env(key)
}

pub fn role_id_env(key: &str) -> serenity::RoleId {
    parse_id_env(key)
}
