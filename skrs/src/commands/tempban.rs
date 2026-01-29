use std::sync::Arc;

use pumpkin::command::args::{message::MsgArgConsumer, simple::SimpleArgConsumer, Arg, ConsumedArgs};
use pumpkin::command::tree::builder::argument;
use pumpkin::command::tree::CommandTree;
use pumpkin::command::{CommandExecutor, CommandSender};
use pumpkin::command::dispatcher::CommandError;
use pumpkin::data::banlist_serializer::BannedPlayerEntry;
use pumpkin::data::SaveJSONConfiguration;
use pumpkin::plugin::Context;
use pumpkin::server::Server;
use pumpkin_util::text::TextComponent;
use pumpkin::net::GameProfile;
use uuid::Uuid;
use time::{Duration, OffsetDateTime};

const NAME: &str = "tempban";
const DESCRIPTION: &str = "Temporarily ban a player for a duration (e.g. 1d, 2h, 30m, 10s).";

const ARG_TARGET: &str = "target";
const ARG_DURATION: &str = "duration";
const ARG_REASON: &str = "reason";

pub async fn register_tempban_command(context: Arc<Context>) -> Result<(), String> {
    let tree = CommandTree::new([NAME], DESCRIPTION).then(
        argument(ARG_TARGET, SimpleArgConsumer).then(
            argument(ARG_DURATION, SimpleArgConsumer)
                .then(argument(ARG_REASON, MsgArgConsumer).execute(Executor))
                .execute(Executor),
        ),
    );

    context.register_command(tree, "skript:command.tempban").await;
    Ok(())
}

struct Executor;

impl CommandExecutor for Executor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> pumpkin::command::CommandResult<'a> {
        Box::pin(async move {
            let Some(Arg::Simple(target_raw)) = args.get(&ARG_TARGET) else {
                return Err(CommandError::InvalidConsumption(Some(ARG_TARGET.into())));
            };
            let Some(Arg::Simple(duration_raw)) = args.get(&ARG_DURATION) else {
                return Err(CommandError::InvalidConsumption(Some(ARG_DURATION.into())));
            };
            let target_name = target_raw.trim();

            let reason = match args.get(&ARG_REASON) {
                Some(Arg::Msg(r)) => r.clone(),
                _ => "Temporarily banned.".to_string(),
            };

            log::info!(
                "[skrs] tempban requested by {}: duration='{}' reason='{}' targets={}",
                sender,
                duration_raw,
                reason,
                target_name
            );

            let duration_secs = parse_duration_seconds(duration_raw);
            if duration_secs.is_none() {
                log::warn!(
                    "[skrs] tempban invalid duration: '{}'",
                    duration_raw
                );
                sender
                    .send_message(TextComponent::text(
                        "Invalid duration. Use e.g. 10s, 5m, 2h, 1d.",
                    ))
                    .await;
                return Ok(());
            }

            let duration_secs = duration_secs.unwrap();
            if duration_secs <= 0 {
                // Unban (allow offline)
                let uuid_target = target_name.parse::<Uuid>().ok();
                let name_lc = target_name.to_ascii_lowercase();

                let mut banned_players = server.data.banned_player_list.write().await;
                let before = banned_players.banned_players.len();
                banned_players.banned_players.retain(|entry| {
                    if let Some(uuid) = uuid_target {
                        if entry.uuid == uuid {
                            return false;
                        }
                    }
                    entry.name.to_ascii_lowercase() != name_lc
                });
                let after = banned_players.banned_players.len();
                banned_players.save();

                if before == after {
                    log::warn!("[skrs] tempban unban: no entry found for '{}'", target_name);
                    sender
                        .send_message(TextComponent::text(format!(
                            "No ban entry found for {}",
                            target_name
                        )))
                        .await;
                } else {
                    log::info!("[skrs] tempban unbanned {}", target_name);
                    sender
                        .send_message(TextComponent::text(format!(
                            "Unbanned {}",
                            target_name
                        )))
                        .await;
                }
                return Ok(());
            }

            let profile: GameProfile = if let Some(p) = server.get_player_by_name(target_name).await {
                p.gameprofile.clone()
            } else {
                sender
                    .send_message(TextComponent::text(format!(
                        "Player '{}' must be online to tempban",
                        target_name
                    )))
                    .await;
                log::warn!(
                    "[skrs] tempban failed: player '{}' not online",
                    target_name
                );
                return Ok(());
            };

            let expires = OffsetDateTime::now_utc() + Duration::seconds(duration_secs);
            let mut banned_players = server.data.banned_player_list.write().await;
            banned_players.banned_players.push(BannedPlayerEntry::new(
                &profile,
                sender.to_string(),
                Some(expires),
                reason.clone(),
            ));
            banned_players.save();
            log::info!(
                "[skrs] tempban banned {} ({}) until {}",
                profile.name,
                profile.id,
                expires
            );

            if let Some(p) = server.get_player_by_name(&profile.name).await {
                p.kick(
                    pumpkin::net::DisconnectReason::Kicked,
                    TextComponent::text(reason.clone()),
                )
                .await;
            }

            Ok(())
        })
    }
}

fn parse_duration_seconds(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = s.split_at(s.len().saturating_sub(1));
    let (value_str, unit) = if unit.chars().all(|c| c.is_ascii_alphabetic()) {
        (num_str, unit)
    } else {
        // No suffix provided: default to days.
        (s, "d")
    };

    let n: i64 = value_str.trim().parse().ok()?;
    let seconds = match unit.to_ascii_lowercase().as_str() {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        _ => return None,
    };
    Some(seconds)
}
