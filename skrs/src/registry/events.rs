// src/registry/events.rs

use dashmap::DashMap;
use once_cell::sync::Lazy;

use crate::parser::ast::Trigger;
use crate::script::script::Script;

/// Global registry:
/// "player join" -> Vec<Trigger>
pub static EVENT_TRIGGERS: Lazy<DashMap<String, Vec<Trigger>>> = Lazy::new(DashMap::new);

#[inline]
fn norm_event(event: &str) -> String {
    event.trim().to_ascii_lowercase()
}

/// Extract the registry key from a parsed trigger.
/// Prefer the parsed canonical event name if available.
/// Fallback to normalized raw header (older triggers / unknown headers).
#[inline]
fn trigger_event_key(trigger: &Trigger) -> String {
    if let Some(ev) = &trigger.header.event {
        norm_event(&ev.event_name)
    } else {
        norm_event(&trigger.header.raw)
    }
}

/// Register triggers (adds onto existing registry).
/// Prefer `replace_all_*` for reload.
pub fn register_triggers(triggers: Vec<Trigger>) {
    for trigger in triggers {
        let key = trigger_event_key(&trigger);

        EVENT_TRIGGERS.entry(key).or_default().push(trigger);
    }
}

/// Clear all triggers (used by reload).
pub fn clear_triggers() {
    EVENT_TRIGGERS.clear();
}

/// Replace registry contents with exactly these triggers.
pub fn replace_all_triggers(triggers: Vec<Trigger>) {
    clear_triggers();
    register_triggers(triggers);
}

/// Replace registry contents with triggers from loaded scripts.
/// Returns total number of triggers inserted.
pub fn replace_all_triggers_from_scripts(scripts: Vec<Script>) -> usize {
    clear_triggers();

    let mut total = 0usize;

    for script in scripts {
        for trigger in script.triggers {
            let key = trigger_event_key(&trigger);

            EVENT_TRIGGERS.entry(key).or_default().push(trigger);

            total += 1;
        }
    }

    total
}

/// Get triggers for an event (canonical key).
pub fn get_triggers(event: &str) -> Vec<Trigger> {
    EVENT_TRIGGERS
        .get(&norm_event(event))
        .map(|v| v.clone())
        .unwrap_or_default()
}

pub fn register() {
    // Nothing to do at startup; triggers are loaded from scripts.
}
