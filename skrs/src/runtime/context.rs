// src/runtime/context.rs

use std::{collections::HashMap, sync::Arc};

use futures::executor;
use pumpkin::entity::player::Player;
use pumpkin::server::Server;
use pumpkin_util::text::TextComponent;

use crate::runtime::value::Value;

// ─────────────────────────────────────────────
// Player events
// ─────────────────────────────────────────────
use pumpkin::plugin::player::{
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
};

// ─────────────────────────────────────────────
// Block events
// ─────────────────────────────────────────────
use pumpkin::plugin::block::{
    block_break::BlockBreakEvent,
    block_burn::BlockBurnEvent,
    block_can_build::BlockCanBuildEvent,
    block_place::BlockPlaceEvent,
};

// ─────────────────────────────────────────────
// Chunk events (Pumpkin naming: ChunkLoad / ChunkSave / ChunkSend)
// ─────────────────────────────────────────────
use pumpkin::plugin::world::{
    chunk_load::ChunkLoad,
    chunk_save::ChunkSave,
    chunk_send::ChunkSend,
};

// ─────────────────────────────────────────────
// Server events
// ─────────────────────────────────────────────
use pumpkin::plugin::server::{
    server_broadcast::ServerBroadcastEvent,
    server_command::ServerCommandEvent,
};

/// Legacy synthetic rotate event (used only if Pumpkin move lacks yaw/pitch).
#[derive(Clone)]
pub struct PlayerRotateEvent {
    pub player: Arc<Player>,
    pub from_yaw: f32,
    pub from_pitch: f32,
    pub to_yaw: f32,
    pub to_pitch: f32,
}

/// Wrapper enum used by skrs to abstract over Pumpkin events.
pub enum SkriptEvent<'a> {
    // ── player ──
    PlayerJoin(&'a mut PlayerJoinEvent),
    PlayerLeave(&'a mut PlayerLeaveEvent),
    PlayerLogin(&'a mut PlayerLoginEvent),
    PlayerChat(&'a mut PlayerChatEvent),
    PlayerMove(&'a mut PlayerMoveEvent),
    PlayerTeleport(&'a mut PlayerTeleportEvent),
    PlayerDeath(&'a mut PlayerDeathEvent),
    PlayerChangeWorld(&'a mut PlayerChangeWorldEvent),
    PlayerGamemodeChange(&'a mut PlayerGamemodeChangeEvent),
    PlayerCommandSend(&'a mut PlayerCommandSendEvent),
    PlayerInteract(&'a mut PlayerInteractEvent),

    // ── synthetic ──
    PlayerRotate(&'a PlayerRotateEvent),

    // ── block ──
    BlockBreak(&'a mut BlockBreakEvent),
    BlockPlace(&'a mut BlockPlaceEvent),
    BlockBurn(&'a mut BlockBurnEvent),
    BlockCanBuild(&'a mut BlockCanBuildEvent),

    // ── chunk ──
    ChunkLoad(&'a mut ChunkLoad),
    ChunkSave(&'a mut ChunkSave),
    ChunkSend(&'a mut ChunkSend),

    // ── server ──
    ServerBroadcast(&'a mut ServerBroadcastEvent),
    ServerCommand(&'a mut ServerCommandEvent),

    // ── synthetic / non-Pumpkin ──
    Command {
        player: Option<Arc<Player>>,
    },
}

/// Where a queued `send` should go.
#[derive(Clone)]
pub enum SendTarget {
    /// Current event player (resolved at flush time)
    Player,

    /// A specific player resolved at queue time (used for loop-player)
    SpecificPlayer(Arc<Player>),

    /// All online players
    AllPlayers,
}

/// One queued chat send.
#[derive(Clone)]
pub struct QueuedSend {
    pub target: SendTarget,
    pub component: TextComponent,
}

pub struct ExecutionContext<'a> {
    pub server: &'a Server,
    pub event: SkriptEvent<'a>,

    /// Local variables for this trigger execution
    pub locals: HashMap<String, Value>,

    /// Queued sends (flushed after trigger finishes)
    pub outbox: Vec<QueuedSend>,

    /// loop-player for `loop all players:`
    pub loop_player: Option<Arc<Player>>,

    /// loop-value for `loop <list>:`
    pub loop_value: Option<Value>,

    /// loop-number for loops (1-based)
    pub loop_index: Option<usize>,

    /// cached command args: "arg-1" -> Value
    pub args: HashMap<String, Value>,
}

impl<'a> ExecutionContext<'a> {
    pub fn new(server: &'a Server, event: SkriptEvent<'a>) -> Self {
        Self {
            server,
            event,
            locals: HashMap::new(),
            outbox: Vec::new(),
            loop_player: None,
            loop_value: None,
            loop_index: None,
            args: HashMap::new(),
        }
    }

    pub fn with_args(mut self, args: HashMap<String, Value>) -> Self {
        self.args = args;
        self
    }

    pub fn drain_outbox(&mut self) -> Vec<QueuedSend> {
        std::mem::take(&mut self.outbox)
    }

    /// Used by `loop all players:` (keeps executor sync)
    pub fn all_players(&self) -> Vec<Arc<Player>> {
        executor::block_on(self.server.get_all_players())
    }

    /// Event message (only exists for chat)
    pub fn event_message(&mut self) -> Option<String> {
        match &mut self.event {
            SkriptEvent::PlayerChat(ev) => Some(ev.message.clone()),
            _ => None,
        }
    }

    /// Unified event-player lookup.
    /// Returns None for events that do not have a player.
    pub fn event_player(&mut self) -> Option<Arc<Player>> {
        match &mut self.event {
            // ── player events (always Arc<Player>) ──
            SkriptEvent::PlayerJoin(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerLeave(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerLogin(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerChat(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerMove(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerTeleport(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerDeath(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerChangeWorld(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerGamemodeChange(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerCommandSend(ev) => Some(Arc::clone(&ev.player)),
            SkriptEvent::PlayerInteract(ev) => Some(Arc::clone(&ev.player)),

            // ── synthetic rotate ──
            SkriptEvent::PlayerRotate(ev) => Some(Arc::clone(&ev.player)),

            // ── block events (mixed types!) ──
            // BlockBreakEvent.player: Option<Arc<Player>>
            SkriptEvent::BlockBreak(ev) => ev.player.as_ref().map(Arc::clone),

            // BlockPlaceEvent.player: Arc<Player>
            SkriptEvent::BlockPlace(ev) => Some(Arc::clone(&ev.player)),

            // BlockBurnEvent: no player
            SkriptEvent::BlockBurn(_) => None,

            // BlockCanBuildEvent.player: Arc<Player>
            SkriptEvent::BlockCanBuild(ev) => Some(Arc::clone(&ev.player)),

            // ── chunk & server events ──
            SkriptEvent::ChunkLoad(_)
            | SkriptEvent::ChunkSave(_)
            | SkriptEvent::ChunkSend(_)
            | SkriptEvent::ServerBroadcast(_)
            | SkriptEvent::ServerCommand(_) => None,

            SkriptEvent::Command { player } => player.as_ref().map(Arc::clone),
        }
    }

    pub fn event_player_name(&mut self) -> Option<String> {
        self.event_player().map(|p| p.gameprofile.name.clone())
    }

    pub fn event_victim(&mut self) -> Option<Arc<Player>> {
        match &mut self.event {
            SkriptEvent::PlayerDeath(ev) => Some(Arc::clone(&ev.player)),
            _ => None,
        }
    }

    pub fn event_victim_name(&mut self) -> Option<String> {
        self.event_victim().map(|p| p.gameprofile.name.clone())
    }

    pub fn event_attacker_name(&mut self) -> Option<String> {
        // Not wired yet; placeholder for damage events
        None
    }

    pub fn get_loop_player(&self) -> Option<Arc<Player>> {
        self.loop_player.as_ref().map(Arc::clone)
    }

    pub fn loop_player_name(&self) -> Option<String> {
        self.get_loop_player().map(|p| p.gameprofile.name.clone())
    }

    pub fn get_arg_value(&self, token: &str) -> Option<Value> {
        let key = token.trim().to_ascii_lowercase();
        self.args.get(&key).cloned()
    }

    pub fn is_command_event(&self) -> bool {
        matches!(&self.event, SkriptEvent::Command { .. })
    }
}
