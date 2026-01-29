// src/runtime/dispatcher.rs

use std::sync::Arc;

use futures::future::BoxFuture;
use pumpkin::{
    plugin::{
        block::{
            block_break::BlockBreakEvent,
            block_burn::BlockBurnEvent,
            block_can_build::BlockCanBuildEvent,
            block_place::BlockPlaceEvent,
        },
        world::{chunk_load::ChunkLoad, chunk_save::ChunkSave, chunk_send::ChunkSend},
        player::{
            player_change_world::PlayerChangeWorldEvent,
        player_chat::PlayerChatEvent,
        player_command_send::PlayerCommandSendEvent,
        player_death::PlayerDeathEvent,
        player_gamemode_change::PlayerGamemodeChangeEvent,
        player_interact_event::PlayerInteractEvent,
        player_join::PlayerJoinEvent,
        player_leave::PlayerLeaveEvent,
        player_login::PlayerLoginEvent,
            player_move::PlayerMoveEvent,
            player_teleport::PlayerTeleportEvent,
        },
        server::{server_broadcast::ServerBroadcastEvent, server_command::ServerCommandEvent},
        Context, EventHandler, EventPriority,
    },
    server::Server,
};
use pumpkin_api_macros::with_runtime;

use crate::registry::events::get_triggers;
use crate::runtime::context::{ExecutionContext, QueuedSend, SendTarget, SkriptEvent};
use crate::runtime::executor::execute_block;

/// Run all triggers registered for `key` and return queued sends + resolved event-player.
/// Exported for rotate_poller.
pub(crate) async fn run_triggers<'a>(
    key: &str,
    server: &'a Server,
    event: SkriptEvent<'a>,
) -> (Vec<QueuedSend>, Option<Arc<pumpkin::entity::player::Player>>) {
    let triggers = get_triggers(key);

    let mut ctx = ExecutionContext::new(server, event);
    for trigger in triggers {
        let _ = execute_block(&trigger.body, &mut ctx).await;
    }

    let outbox = ctx.drain_outbox();
    let event_player = ctx.event_player();
    (outbox, event_player)
}

/// Flush queued sends.
/// Exported for rotate_poller.
pub(crate) async fn flush_outbox(
    server: &Server,
    outbox: Vec<QueuedSend>,
    event_player: Option<Arc<pumpkin::entity::player::Player>>,
) {
    for item in outbox {
        match item.target {
            SendTarget::Player => {
                if let Some(p) = event_player.as_ref() {
                    p.send_system_message(&item.component).await;
                } else {
                    // Fallback for console-triggered commands with no player
                    log::info!("{}", item.component.to_pretty_console());
                }
            }
            SendTarget::SpecificPlayer(p) => {
                p.send_system_message(&item.component).await;
            }
            SendTarget::AllPlayers => {
                let players = server.get_all_players().await;
                for p in players {
                    p.send_system_message(&item.component).await;
                }
            }
        }
    }
}

// ---------------- Handlers ----------------

macro_rules! make_handler {
    ($name:ident, $event_ty:ty, $trigger_key:expr, $variant:ident) => {
        pub struct $name;

        #[with_runtime(global)]
        impl EventHandler<$event_ty> for $name {
            fn handle_blocking<'a>(
                &'a self,
                server: &'a Arc<Server>,
                event: &'a mut $event_ty,
            ) -> BoxFuture<'a, ()> {
                let (outbox, event_player) =
                    run_triggers($trigger_key, server.as_ref(), SkriptEvent::$variant(event))
                        .await;
                Box::pin(async move { flush_outbox(server.as_ref(), outbox, event_player).await })
                    as BoxFuture<'_, ()>
            }
        }
    };
}

// Player
make_handler!(SkriptJoinHandler, PlayerJoinEvent, "player join", PlayerJoin);
make_handler!(SkriptLeaveHandler, PlayerLeaveEvent, "player leave", PlayerLeave);
make_handler!(SkriptLoginHandler, PlayerLoginEvent, "player login", PlayerLogin);
make_handler!(SkriptChatHandler, PlayerChatEvent, "player chat", PlayerChat);
make_handler!(SkriptDeathHandler, PlayerDeathEvent, "player death", PlayerDeath);

pub struct SkriptMoveHandler;

fn angle_delta(a: f32, b: f32) -> f32 {
    let mut d = (a - b) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d.abs()
}

#[with_runtime(global)]
impl EventHandler<PlayerMoveEvent> for SkriptMoveHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut PlayerMoveEvent,
    ) -> BoxFuture<'a, ()> {
        // --- position delta
        let dx = event.to.x - event.from.x;
        let dy = event.to.y - event.from.y;
        let dz = event.to.z - event.from.z;
        let dist2 = dx * dx + dy * dy + dz * dz;

        // --- blockpos change (best for "step/walk")
        let from_bx = event.from.x.floor() as i32;
        let from_by = event.from.y.floor() as i32;
        let from_bz = event.from.z.floor() as i32;

        let to_bx = event.to.x.floor() as i32;
        let to_by = event.to.y.floor() as i32;
        let to_bz = event.to.z.floor() as i32;

        let block_changed = from_bx != to_bx || from_by != to_by || from_bz != to_bz;

        let yaw_delta = angle_delta(event.to_yaw, event.from_yaw);
        let pitch_delta = (event.to_pitch - event.from_pitch).abs();
        let rotated = yaw_delta > 0.01 || pitch_delta > 0.01;

        // thresholds:
        // - If block changed, always treat as moved (true step/walk).
        // - Otherwise require a small meaningful delta to avoid jitter spam.
        let moved = block_changed || dist2 > 0.0004; // ~0.02 blocks

        // If neither moved nor rotated, ignore (prevents spam)
        if !moved && !rotated {
            return Box::pin(async move {}) as BoxFuture<'_, ()>;
        }

        let mut outbox_all: Vec<QueuedSend> = Vec::new();
        let mut event_player: Option<Arc<pumpkin::entity::player::Player>> = None;

        // Keep legacy "player move" (backwards compatibility)
        if moved {
                let (outbox, ep) =
                run_triggers("player move", server.as_ref(), SkriptEvent::PlayerMove(event)).await;
            outbox_all.extend(outbox);
            event_player = event_player.or(ep);
        }

        // New canonical: entity move
        if moved {
            let (outbox, ep) =
                run_triggers("entity move", server.as_ref(), SkriptEvent::PlayerMove(event)).await;
            outbox_all.extend(outbox);
            event_player = event_player.or(ep);
        }

        // New canonical: entity rotate
        if rotated {
            let (outbox, ep) =
                run_triggers("entity rotate", server.as_ref(), SkriptEvent::PlayerMove(event)).await;
            outbox_all.extend(outbox);
            event_player = event_player.or(ep);
        }

        // Combined listens to either (currently behaves like "move" here)
        if moved || rotated {
            let (outbox, ep) = run_triggers(
                "entity move or rotate",
                server.as_ref(),
                SkriptEvent::PlayerMove(event),
            )
            .await;
            outbox_all.extend(outbox);
            event_player = event_player.or(ep);
        }

        // Step-on triggers only make sense when block position changes.
        // NOTE: itemtype matching not enforced yet; we just dispatch the canonical.
        if block_changed {
            let (outbox, ep) =
                run_triggers("player step on", server.as_ref(), SkriptEvent::PlayerMove(event)).await;
            outbox_all.extend(outbox);
            event_player = event_player.or(ep);
        }

        Box::pin(async move { flush_outbox(server.as_ref(), outbox_all, event_player).await })
            as BoxFuture<'_, ()>
    }
}

make_handler!(SkriptTeleportHandler, PlayerTeleportEvent, "player teleport", PlayerTeleport);
make_handler!(
    SkriptChangeWorldHandler,
    PlayerChangeWorldEvent,
    "player change world",
    PlayerChangeWorld
);
make_handler!(
    SkriptGamemodeChangeHandler,
    PlayerGamemodeChangeEvent,
    "player gamemode change",
    PlayerGamemodeChange
);
make_handler!(
    SkriptCommandSendHandler,
    PlayerCommandSendEvent,
    "player command send",
    PlayerCommandSend
);
make_handler!(SkriptInteractHandler, PlayerInteractEvent, "player interact", PlayerInteract);

// Block
make_handler!(SkriptBlockBreakHandler, BlockBreakEvent, "block break", BlockBreak);
make_handler!(SkriptBlockPlaceHandler, BlockPlaceEvent, "block place", BlockPlace);
make_handler!(SkriptBlockBurnHandler, BlockBurnEvent, "block burn", BlockBurn);
make_handler!(
    SkriptBlockCanBuildHandler,
    BlockCanBuildEvent,
    "block can build",
    BlockCanBuild
);

// Chunk
make_handler!(SkriptChunkLoadHandler, ChunkLoad, "chunk load", ChunkLoad);
make_handler!(SkriptChunkSaveHandler, ChunkSave, "chunk save", ChunkSave);
make_handler!(SkriptChunkSendHandler, ChunkSend, "chunk send", ChunkSend);

// Server
make_handler!(
    SkriptServerBroadcastHandler,
    ServerBroadcastEvent,
    "server broadcast",
    ServerBroadcast
);
make_handler!(
    SkriptServerCommandHandler,
    ServerCommandEvent,
    "server command",
    ServerCommand
);

/// Register all Pumpkin event handlers we need.
pub async fn register_event_listeners(context: Arc<Context>) {
    crate::registry::register_all();

    // Player
    context.register_event(Arc::new(SkriptJoinHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptLeaveHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptLoginHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptChatHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptDeathHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptMoveHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptTeleportHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptChangeWorldHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptGamemodeChangeHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptCommandSendHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptInteractHandler), EventPriority::Lowest, true).await;

    // Block
    context.register_event(Arc::new(SkriptBlockBreakHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptBlockPlaceHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptBlockBurnHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptBlockCanBuildHandler), EventPriority::Lowest, true).await;

    // Chunk
    context.register_event(Arc::new(SkriptChunkLoadHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptChunkSaveHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptChunkSendHandler), EventPriority::Lowest, true).await;

    // Server
    context.register_event(Arc::new(SkriptServerBroadcastHandler), EventPriority::Lowest, true).await;
    context.register_event(Arc::new(SkriptServerCommandHandler), EventPriority::Lowest, true).await;
}
