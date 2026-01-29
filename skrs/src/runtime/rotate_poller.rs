// src/runtime/rotate_poller.rs

use std::{collections::HashSet, sync::Arc, time::Duration};

use dashmap::DashMap;
use once_cell::sync::Lazy;
use pumpkin::server::Server;

use crate::runtime::context::{PlayerRotateEvent, SkriptEvent};
use crate::runtime::dispatcher::{flush_outbox, run_triggers};

// Keyed by player name for now (portable; avoids UUID dependency/type issues)
static LAST_ROT: Lazy<DashMap<String, (f32, f32)>> = Lazy::new(DashMap::new);

pub fn start(server: Arc<Server>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build rotate_poller tokio runtime");

        rt.block_on(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(50)); // 20 TPS

            loop {
                tick.tick().await;

                let players = server.get_all_players().await;

                let mut online: HashSet<String> = HashSet::with_capacity(players.len());

                for p in players {
                    let key = p.gameprofile.name.clone();
                    online.insert(key.clone());

                    // rotation from entity state
                    let yaw: f32 = p.living_entity.entity.yaw.load();
                    let pitch: f32 = p.living_entity.entity.pitch.load();

                    let (prev_yaw, prev_pitch) = match LAST_ROT.get(&key) {
                        Some(v) => *v.value(),
                        None => {
                            LAST_ROT.insert(key.clone(), (yaw, pitch));
                            continue;
                        }
                    };

                    let delta: f32 = (yaw - prev_yaw).abs() + (pitch - prev_pitch).abs();

                    // DEBUG: if you want to *see* what delta is doing, log when it changes at all
                    // (you can delete this once confirmed)
                    if delta > 0.0 {
                        log::debug!(
                            "[rotate_poller] {} yaw {:.2}->{:.2} pitch {:.2}->{:.2} delta {:.2}",
                            key,
                            prev_yaw,
                            yaw,
                            prev_pitch,
                            pitch,
                            delta
                        );
                    }

                    // threshold to avoid spam
                    if delta <= 1.0 {
                        LAST_ROT.insert(key.clone(), (yaw, pitch));
                        continue;
                    }

                    let ev = PlayerRotateEvent {
                        player: Arc::clone(&p),
                        from_yaw: prev_yaw,
                        from_pitch: prev_pitch,
                        to_yaw: yaw,
                        to_pitch: pitch,
                    };

                    // Fire BOTH canonical + raw keys so it works even if attach_events() didn't run.

                    // Canonical
                    {
                        let (out1, ep1) = run_triggers(
                            "entity rotate",
                            server.as_ref(),
                            SkriptEvent::PlayerRotate(&ev),
                        )
                        .await;
                        flush_outbox(server.as_ref(), out1, ep1).await;

                        let (out2, ep2) = run_triggers(
                            "entity move or rotate",
                            server.as_ref(),
                            SkriptEvent::PlayerRotate(&ev),
                        )
                        .await;
                        flush_outbox(server.as_ref(), out2, ep2).await;
                    }

                    // Raw / legacy
                    {
                        let (out1, ep1) = run_triggers(
                            "player rotate",
                            server.as_ref(),
                            SkriptEvent::PlayerRotate(&ev),
                        )
                        .await;
                        flush_outbox(server.as_ref(), out1, ep1).await;

                        let (out2, ep2) = run_triggers(
                            "player move or rotate",
                            server.as_ref(),
                            SkriptEvent::PlayerRotate(&ev),
                        )
                        .await;
                        flush_outbox(server.as_ref(), out2, ep2).await;
                    }

                    LAST_ROT.insert(key, (yaw, pitch));
                }

                // cleanup
                let keys: Vec<String> = LAST_ROT.iter().map(|e| e.key().clone()).collect();
                for k in keys {
                    if !online.contains(&k) {
                        LAST_ROT.remove(&k);
                    }
                }
            }
        });
    });
}
