// src/event_parser.rs

use std::collections::HashMap;

use crate::parser::ast::{ParsedEvent, Script};

#[derive(Default)]
pub struct EventRegistry {
    /// maps skript header text (normalized) -> canonical event key used by get_triggers(...)
    map: HashMap<String, String>,
}

impl EventRegistry {
    pub fn new() -> Self {
        let mut r = Self::default();

        // ---------------------------
        // Player events (canonical)
        // ---------------------------
        r.register_aliases("player join", &["join", "player join", "on join"]);
        r.register_aliases(
            "player leave",
            &["quit", "leave", "player quit", "player leave", "on quit", "on leave"],
        );
        r.register_aliases("player login", &["login", "player login", "on login"]);

        r.register_aliases("player chat", &["chat", "message", "player chat", "on chat"]);
        r.register_aliases("player move", &["move", "player move", "on move"]); // keep legacy/simple
        r.register_aliases("player teleport", &["teleport", "player teleport", "on teleport"]);
        r.register_aliases(
            "player change world",
            &[
                "change world",
                "world change",
                "player change world",
                "on change world",
            ],
        );
        r.register_aliases(
            "player gamemode change",
            &[
                "gamemode change",
                "game mode change",
                "player gamemode change",
                "on gamemode change",
            ],
        );
        r.register_aliases(
            "player command send",
            &["command", "player command", "player command send", "on command"],
        );
        r.register_aliases(
            "player interact",
            &[
                "right click",
                "left click",
                "click",
                "interact",
                "player interact",
                "on right click",
                "on interact",
            ],
        );

        // Player death
        r.register_aliases(
            "player death",
            &["death", "player death", "death of player", "on death", "on death of player"],
        );

        // ---------------------------
        // New movement canonicals
        // ---------------------------
        // These are not strictly required (fallback parsing handles them),
        // but registering them makes them visible for exact matches too.
        r.register_aliases("entity move", &["entity move", "on entity move"]);
        r.register_aliases("entity rotate", &["entity rotate", "on entity rotate"]);
        r.register_aliases(
            "entity move or rotate",
            &["entity move or rotate", "on entity move or rotate"],
        );
        r.register_aliases("player step on", &["player step on", "on player step on"]);

        // ---------------------------
        // Block events
        // ---------------------------
        r.register_aliases("block break", &["break", "block break", "on break"]);
        r.register_aliases("block place", &["place", "block place", "on place"]);
        r.register_aliases("block burn", &["burn", "block burn", "on burn"]);
        r.register_aliases("block can build", &["can build", "block can build", "on can build"]);

        // ---------------------------
        // Chunk events
        // ---------------------------
        r.register_aliases("chunk load", &["chunk load", "on chunk load"]);
        r.register_aliases("chunk save", &["chunk save", "on chunk save"]);
        r.register_aliases("chunk send", &["chunk send", "on chunk send"]);

        // ---------------------------
        // Server events
        // ---------------------------
        r.register_aliases("server broadcast", &["broadcast", "server broadcast", "on broadcast"]);
        r.register_aliases(
            "server command",
            &["server command", "console command", "on server command"],
        );

        r
    }

    /// Register one mapping: header text -> canonical key
    pub fn register(&mut self, skript: &str, canonical_event_key: &str) {
        self.map
            .insert(normalize(skript), canonical_event_key.to_string());
    }

    /// Convenience: register multiple aliases to the same canonical key
    pub fn register_aliases(&mut self, canonical_event_key: &str, aliases: &[&str]) {
        // Also register the canonical key itself as a valid header
        self.register(canonical_event_key, canonical_event_key);

        for a in aliases {
            self.register(a, canonical_event_key);
        }
    }

    pub fn parse_header(&self, header_raw: &str) -> Option<ParsedEvent> {
        // Normalize the raw header (already "after on " and before ":")
        let key = normalize(header_raw);

        // 1) Exact alias map hit (current behavior)
        if let Some(event_name) = self.map.get(&key).cloned() {
            return Some(ParsedEvent {
                event_name,
                args: vec![],
            });
        }

        // 2) [on] (step|walk)[ing] (on|over) %*itemtypes%
        // Examples supported:
        // "stepping on stone", "walking over grass block", "step on dirt", "walk over oak planks"
        if let Some(rest) = strip_step_on_prefix(&key) {
            let itemtypes_text = rest.trim().to_string();
            if !itemtypes_text.is_empty() {
                return Some(ParsedEvent {
                    event_name: "player step on".to_string(),
                    args: vec![itemtypes_text],
                });
            }
        }

        // 3) [on] %entitydata% move/rotate/combined (v1: player/entity)
        if let Some(rest) = key.strip_prefix("player ") {
            return parse_entity_move_header("player", rest);
        }
        if let Some(rest) = key.strip_prefix("entity ") {
            return parse_entity_move_header("entity", rest);
        }

        None
    }
}

pub fn attach_events(script: &mut Script, reg: &EventRegistry) {
    for trig in &mut script.triggers {
        trig.header.event = reg.parse_header(&trig.header.raw);
    }
}

fn normalize(s: &str) -> String {
    s.trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_step_on_prefix(key: &str) -> Option<&str> {
    key.strip_prefix("stepping on ")
        .or_else(|| key.strip_prefix("stepping over "))
        .or_else(|| key.strip_prefix("walking on "))
        .or_else(|| key.strip_prefix("walking over "))
        .or_else(|| key.strip_prefix("step on "))
        .or_else(|| key.strip_prefix("step over "))
        .or_else(|| key.strip_prefix("walk on "))
        .or_else(|| key.strip_prefix("walk over "))
}

fn parse_entity_move_header(entity_filter: &str, rest: &str) -> Option<ParsedEvent> {
    let r = rest.trim();

    // rotate keywords
    let is_rotate = r == "rotate" || r == "turn around" || r == "turning around";

    // move keywords
    let is_move = r == "move" || r == "walk" || r == "step";

    // combined keywords ("move or rotate" etc). Support both orders and "turn around".
    let is_move_or_rotate = matches!(
        r,
        "move or rotate"
            | "walk or rotate"
            | "step or rotate"
            | "rotate or move"
            | "rotate or walk"
            | "rotate or step"
            | "move or turn around"
            | "walk or turn around"
            | "step or turn around"
            | "turn around or move"
            | "turn around or walk"
            | "turn around or step"
            | "move or turning around"
            | "walk or turning around"
            | "step or turning around"
            | "turning around or move"
            | "turning around or walk"
            | "turning around or step"
    );

    if is_move_or_rotate {
        return Some(ParsedEvent {
            event_name: "entity move or rotate".to_string(),
            args: vec![entity_filter.to_string()],
        });
    }

    if is_rotate {
        return Some(ParsedEvent {
            event_name: "entity rotate".to_string(),
            args: vec![entity_filter.to_string()],
        });
    }

    if is_move {
        return Some(ParsedEvent {
            event_name: "entity move".to_string(),
            args: vec![entity_filter.to_string()],
        });
    }

    None
}
