// Discord TTS Bot
// Copyright (C) 2021-Present David Thomas
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published
// by the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use songbird::error::JoinError;

use poise::serenity_prelude::{self as serenity, builder::*};

use crate::{
    funcs::random_footer,
    require, require_guild,
    structs::{Command, CommandResult, Context, JoinVCToken, Result},
    traits::{PoiseContextExt, SongbirdManagerExt},
};

async fn channel_check(ctx: &Context<'_>, author_vc: Option<serenity::ChannelId>) -> Result<bool> {
    let guild_id = ctx.guild_id().unwrap();
    let setup_id = ctx.data().guilds_db.get(guild_id.into()).await?.channel;

    let channel_id = Some(ctx.channel_id());
    if setup_id == channel_id || author_vc == channel_id {
        return Ok(true);
    }

    let msg = if let Some(setup_id) = setup_id
        && require_guild!(ctx, Ok(false))
            .channels
            .contains_key(&setup_id)
    {
        ctx.gettext("You ran this command in the wrong channel, please move to <#{channel_id}>.")
            .replace("{channel_id}", &setup_id.to_string())
    } else {
        String::from(ctx.gettext("You haven't setup the bot, please run /setup!"))
    };

    ctx.send_error(msg).await?;
    Ok(false)
}

/// Joins the voice channel you're in!
#[poise::command(
    category = "Main Commands",
    guild_only,
    prefix_command,
    slash_command,
    required_bot_permissions = "SEND_MESSAGES | EMBED_LINKS"
)]
pub async fn join(ctx: Context<'_>) -> CommandResult {
    let author_vc = require!(
        ctx.author_vc(),
        ctx.send_error(
            ctx.gettext("I cannot join your voice channel unless you are in one!")
                .to_owned()
        )
        .await
        .map(drop)
    );

    if !channel_check(&ctx, Some(author_vc)).await? {
        return Ok(());
    }

    let guild_id = ctx.guild_id().unwrap();
    let (bot_id, bot_face) = {
        let current_user = ctx.cache().current_user();
        (current_user.id, current_user.face())
    };

    let bot_member = guild_id.member(ctx, bot_id).await?;
    if let Some(communication_disabled_until) = bot_member.communication_disabled_until {
        if communication_disabled_until > serenity::Timestamp::now() {
            let msg = ctx.gettext("I am timed out, please ask a moderator to remove the timeout");
            ctx.send_error(msg.to_owned()).await?;
            return Ok(());
        }
    }

    let author = ctx.author();
    let member = guild_id.member(ctx, author.id).await?;
    let channel = author_vc.to_channel(ctx).await?.guild().unwrap();

    let missing_permissions = (serenity::Permissions::VIEW_CHANNEL
        | serenity::Permissions::CONNECT
        | serenity::Permissions::SPEAK)
        - channel.permissions_for_user(ctx, bot_id)?;

    if !missing_permissions.is_empty() {
        let msg = ctx.gettext("I do not have permission to TTS in your voice channel, please ask a server administrator to give me: {missing_permissions}")
            .replace("{missing_permissions}", &missing_permissions.get_permission_names().join(", "));

        ctx.send_error(msg).await?;
        return Ok(());
    }

    let data = ctx.data();
    if let Some(bot_vc) = data.songbird.get(guild_id) {
        let bot_channel_id = bot_vc.lock().await.current_channel();
        if let Some(bot_channel_id) = bot_channel_id {
            if bot_channel_id.0.get() == author_vc.get() {
                ctx.say(ctx.gettext("I am already in your voice channel!"))
                    .await?;
                return Ok(());
            };

            ctx.say(
                ctx.gettext("I am already in <#{channel_id}>!")
                    .replace("{channel_id}", &bot_channel_id.0.to_string()),
            )
            .await?;
            return Ok(());
        }
    };

    {
        let _typing = ctx.defer_or_broadcast().await?;

        let join_vc_lock = JoinVCToken::acquire(data, guild_id);
        let join_vc_result = data
            .songbird
            .join_vc(join_vc_lock.lock().await, author_vc)
            .await;

        if let Err(err) = join_vc_result {
            return if let JoinError::TimedOut = err {
                let msg = ctx.gettext("I failed to join your voice channel, please check I have the right permissions and try again!");
                ctx.send_error(msg.to_owned()).await?;
                Ok(())
            } else {
                Err(err.into())
            };
        };
    }

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::default()
                .title(ctx.gettext("Joined your voice channel!"))
                .description(ctx.gettext("Just type normally and TTS Bot will say your messages!"))
                .thumbnail(bot_face)
                .author(CreateEmbedAuthor::new(member.display_name()).icon_url(author.face()))
                .footer(CreateEmbedFooter::new(random_footer(
                    &data.config.main_server_invite,
                    bot_id,
                    ctx.current_catalog(),
                ))),
        ),
    )
    .await?;
    Ok(())
}

/// Leaves voice channel TTS Bot is in!
#[poise::command(
    category = "Main Commands",
    guild_only,
    prefix_command,
    slash_command,
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn leave(ctx: Context<'_>) -> CommandResult {
    let (guild_id, author_vc) = {
        let guild = require_guild!(ctx);
        let channel_id = guild
            .voice_states
            .get(&ctx.author().id)
            .and_then(|vs| vs.channel_id);

        (guild.id, channel_id)
    };

    let data = ctx.data();
    let bot_vc = {
        if let Some(handler) = data.songbird.get(guild_id) {
            handler.lock().await.current_channel()
        } else {
            None
        }
    };

    if let Some(bot_vc) = bot_vc {
        if !channel_check(&ctx, author_vc).await? {
        } else if author_vc.map_or(true, |author_vc| bot_vc.0.get() != author_vc.get()) {
            ctx.say(ctx.gettext(
                "Error: You need to be in the same voice channel as me to make me leave!",
            ))
            .await?;
        } else {
            data.songbird.remove(guild_id).await?;
            data.last_to_xsaid_tracker.remove(&guild_id);

            ctx.say(ctx.gettext("Left voice channel!")).await?;
        }
    } else {
        ctx.say(ctx.gettext("Error: How do I leave a voice channel if I am not in one?"))
            .await?;
    }

    Ok(())
}

/// Clears the message queue!
#[poise::command(
    aliases("skip"),
    category = "Main Commands",
    guild_only,
    prefix_command,
    slash_command,
    required_bot_permissions = "SEND_MESSAGES | ADD_REACTIONS"
)]
pub async fn clear(ctx: Context<'_>) -> CommandResult {
    if !channel_check(&ctx, ctx.author_vc()).await? {
        return Ok(());
    }

    let guild_id = ctx.guild_id().unwrap();
    if let Some(call_lock) = ctx.data().songbird.get(guild_id) {
        call_lock.lock().await.queue().stop();

        match ctx {
            poise::Context::Prefix(ctx) => {
                // Prefixed command, just add a thumbsup reaction
                ctx.msg.react(ctx.serenity_context(), '👍').await?;
            }
            poise::Context::Application(_) => {
                // Slash command, no message to react to, just say thumbsup
                ctx.say('👍').await?;
            }
        }
    } else {
        ctx.say(ctx.gettext("**Error**: I am not in a voice channel!"))
            .await?;
    };

    Ok(())
}

pub fn commands() -> [Command; 3] {
    [join(), leave(), clear()]
}
