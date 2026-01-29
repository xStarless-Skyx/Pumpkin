use pumpkin_macros::{Event, cancellable};
use pumpkin_util::math::vector3::Vector3;
use std::sync::Arc;

use crate::entity::player::Player;

use super::PlayerEvent;

/// An event that occurs when a player moves.
///
/// If the event is cancelled, the player will not be allowed to move.
///
/// This event contains information about the player, the position from which the player moved, and the position to which the player moved.
#[cancellable]
#[derive(Event, Clone)]
pub struct PlayerMoveEvent {
    /// The player who moved.
    pub player: Arc<Player>,

    /// The position from which the player moved.
    pub from: Vector3<f64>,

    /// The position to which the player moved.
    pub to: Vector3<f64>,

    /// The yaw the player rotated from.
    pub from_yaw: f32,

    /// The pitch the player rotated from.
    pub from_pitch: f32,

    /// The yaw the player rotated to.
    pub to_yaw: f32,

    /// The pitch the player rotated to.
    pub to_pitch: f32,
}

impl PlayerMoveEvent {
    /// Creates a new instance of `PlayerMoveEvent`.
    ///
    /// # Arguments
    /// - `player`: A reference to the player who moved.
    /// - `from`: The position from which the player moved.
    /// - `to`: The position to which the player moved.
    ///
    /// # Returns
    /// A new instance of `PlayerMoveEvent`.
    pub fn new(
        player: Arc<Player>,
        from: Vector3<f64>,
        to: Vector3<f64>,
        from_yaw: f32,
        from_pitch: f32,
        to_yaw: f32,
        to_pitch: f32,
    ) -> Self {
        Self {
            player,
            from,
            to,
            from_yaw,
            from_pitch,
            to_yaw,
            to_pitch,
            cancelled: false,
        }
    }
}

impl PlayerEvent for PlayerMoveEvent {
    fn get_player(&self) -> &Arc<Player> {
        &self.player
    }
}
