use pumpkin_macros::{Event, cancellable};
use pumpkin_util::text::TextComponent;
use std::sync::Arc;

use crate::entity::player::Player;

use super::PlayerEvent;

/// An event that occurs when a player dies.
///
/// This event contains information about the player and the death message.
#[cancellable]
#[derive(Event, Clone)]
pub struct PlayerDeathEvent {
    /// The player who died.
    pub player: Arc<Player>,

    /// The death message sent to players.
    pub death_message: TextComponent,
}

impl PlayerDeathEvent {
    /// Creates a new instance of `PlayerDeathEvent`.
    ///
    /// # Arguments
    /// - `player`: A reference to the player who died.
    /// - `death_message`: The death message sent to players.
    ///
    /// # Returns
    /// A new instance of `PlayerDeathEvent`.
    pub fn new(player: Arc<Player>, death_message: TextComponent) -> Self {
        Self {
            player,
            death_message,
            cancelled: false,
        }
    }
}

impl PlayerEvent for PlayerDeathEvent {
    fn get_player(&self) -> &Arc<Player> {
        &self.player
    }
}
