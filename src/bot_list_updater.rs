use std::{num::NonZeroU16, sync::Arc};

use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE};

use self::serenity::{
    json::{json, to_vec},
    UserId,
};
use poise::serenity_prelude as serenity;

use crate::{require, structs::BotListTokens, Result};

pub struct BotListUpdater {
    cache: Arc<serenity::cache::Cache>,
    reqwest: reqwest::Client,
    tokens: BotListTokens,
}

struct BotListReq {
    url: String,
    body: Vec<u8>,
    token: HeaderValue,
}

impl BotListUpdater {
    #[must_use]
    pub fn new(
        reqwest: reqwest::Client,
        cache: Arc<serenity::cache::Cache>,
        tokens: BotListTokens,
    ) -> Self {
        Self {
            cache,
            reqwest,
            tokens,
        }
    }

    fn top_gg_data(
        &self,
        bot_id: UserId,
        guild_count: usize,
        shard_count: NonZeroU16,
    ) -> Option<BotListReq> {
        self.tokens.top_gg.as_deref().map(|token| BotListReq {
            url: format!("https://top.gg/api/bots/{bot_id}/stats"),
            token: HeaderValue::from_str(token).unwrap(),
            body: to_vec(&json!({
                "server_count": guild_count,
                "shard_count": shard_count,
            }))
            .unwrap(),
        })
    }

    fn discord_bots_gg_data(
        &self,
        bot_id: UserId,
        guild_count: usize,
        shard_count: NonZeroU16,
    ) -> Option<BotListReq> {
        self.tokens
            .discord_bots_gg
            .as_deref()
            .map(|token| BotListReq {
                url: format!("https://discord.bots.gg/api/v1/bots/{bot_id}/stats"),
                token: HeaderValue::from_str(token).unwrap(),
                body: to_vec(&json!({
                    "guildCount": guild_count,
                    "shardCount": shard_count,
                }))
                .unwrap(),
            })
    }

    fn bots_on_discord_data(&self, bot_id: UserId, guild_count: usize) -> Option<BotListReq> {
        self.tokens
            .bots_on_discord
            .as_deref()
            .map(|token| BotListReq {
                url: format!("https://bots.ondiscord.xyz/bot-api/bots/{bot_id}/guilds"),
                body: to_vec(&json!({"guildCount": guild_count})).unwrap(),
                token: HeaderValue::from_str(token).unwrap(),
            })
    }
}

impl crate::Looper for BotListUpdater {
    const NAME: &'static str = "Bot List Updater";
    const MILLIS: u64 = 1000 * 60 * 60;

    async fn loop_func(&self) -> Result<()> {
        let perform = |req| async move {
            if let Some(BotListReq { url, body, token }) = req {
                let headers = reqwest::header::HeaderMap::from_iter([
                    (AUTHORIZATION, token),
                    (CONTENT_TYPE, HeaderValue::from_static("application/json")),
                ]);

                let request = self.reqwest.post(url).body(body).headers(headers);

                let err = require!(match request.send().await {
                    Ok(resp) => resp.error_for_status().err(),
                    Err(err) => Some(err),
                });

                tracing::error!("{} Error: {:?}", Self::NAME, err);
            }
        };

        let shard_count = self.cache.shard_count();
        let bot_id = self.cache.current_user().id;
        let guild_count = self.cache.guild_count();

        perform(self.bots_on_discord_data(bot_id, guild_count)).await;
        perform(self.top_gg_data(bot_id, guild_count, shard_count)).await;
        perform(self.discord_bots_gg_data(bot_id, guild_count, shard_count)).await;

        Ok(())
    }
}
