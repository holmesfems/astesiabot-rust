use crate::bot::data::Error;
use chrono::{Datelike, FixedOffset, Utc};
use poise::serenity_prelude as serenity;

fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("valid JST offset")
}

fn anni_role_id_env() -> serenity::RoleId {
    std::env::var("ANNIROLEID")
        .unwrap_or_else(|_| panic!("ANNIROLEID not set"))
        .parse()
        .unwrap_or_else(|_| panic!("ANNIROLEID is not a valid role id"))
}

/// 1周年ロール付与。moderation とは無関係の独立機能。副作用のみで戻り値は無い。
pub async fn handle(ctx: &serenity::Context, msg: &serenity::Message) -> Result<(), Error> {
    // botには一周年ロールを付けない
    if msg.author.bot {
        return Ok(());
    }
    // DMではロールを付けない
    let Some(guild_id) = msg.guild_id else {
        return Ok(());
    };
    let Some(member) = msg.member.as_deref() else {
        return Ok(());
    };

    let anni_role_id = anni_role_id_env();
    // 一周年ロールがある場合、付けない
    if member.roles.contains(&anni_role_id) {
        return Ok(());
    }
    let Some(joined_at) = member.joined_at else {
        return Ok(());
    };

    let jst_offset = jst();
    let now_jst = Utc::now().with_timezone(&jst_offset);
    let joined_jst = joined_at.with_timezone(&jst_offset);

    let can_get_role = (now_jst.year() - joined_jst.year() >= 2)
        || (now_jst.year() - joined_jst.year() >= 1 && now_jst.month() >= joined_jst.month());
    if !can_get_role {
        return Ok(());
    }

    let has_role = ctx
        .cache
        .guild(guild_id)
        .map(|guild| guild.roles.contains_key(&anni_role_id))
        .unwrap_or(false);
    if !has_role {
        return Ok(());
    }

    ctx.http
        .add_member_role(guild_id, msg.author.id, anni_role_id, None)
        .await?;
    Ok(())
}
