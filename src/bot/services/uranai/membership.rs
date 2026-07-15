use crate::bot::utils::{guild_id_env, role_id_env};
use poise::serenity_prelude as serenity;

/// 課金ロール判定用の設定。起動時に一度だけ環境変数から解決する。
pub struct MembershipConfig {
    guild_id: serenity::GuildId,
    youtube_role_id: serenity::RoleId,
    booster_role_id: serenity::RoleId,
}

impl MembershipConfig {
    pub fn from_env() -> Self {
        Self {
            guild_id: guild_id_env("GUILD_ID_F"),
            youtube_role_id: role_id_env("ROLE_ID_YOUTUBE_MEMBER"),
            booster_role_id: role_id_env("ROLE_ID_SERVER_BOOSTER"),
        }
    }
}

/// 課金ロール（YouTubeメンバーシップ or サーバーブースト）を持っているか判定する。
/// 占い館チャンネル自体がDiscord側のロール権限で書き込み制限されている想定なので、
/// ここでの判定は防御的な二重チェックの位置づけ（NGなら無反応でよい）。
pub fn check(config: &MembershipConfig, msg: &serenity::Message) -> bool {
    if msg.guild_id != Some(config.guild_id) {
        return false;
    }
    let Some(member) = msg.member.as_deref() else {
        return false;
    };
    member.roles.contains(&config.youtube_role_id) || member.roles.contains(&config.booster_role_id)
}
