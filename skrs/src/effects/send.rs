// src/effects/send.rs

use pumpkin_util::text::TextComponent;

use crate::runtime::context::{ExecutionContext, QueuedSend, SendTarget};
use crate::runtime::expr::eval_message_expr;

pub fn register() {
    crate::registry::effects::register_effect("send", effect_send);
}

pub fn effect_send(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    // Split: <message> to <target>
    let (msg_part, target_part) = match s.rsplit_once(" to ") {
        Some((left, right)) => (left.trim(), right.trim()),
        None => (s, "player"),
    };

    // Evaluate message expression
    let msg = eval_message_expr(msg_part, ctx).to_string_lossy();
    if msg.is_empty() {
        return;
    }

    // Strip wrapping quotes and apply legacy color codes
    let msg_unquoted = strip_wrapping_quotes_loose(msg.trim());
    let legacy = msg_unquoted.replace('&', "§");
    let component = TextComponent::text(legacy);

    // ✅ Queue (do NOT send here). Dispatcher flushes in-order.
    let target = match target_part.to_ascii_lowercase().as_str() {
        "player" => SendTarget::Player,

        // IMPORTANT: resolve loop-player NOW (queue-time), not at flush-time.
        "loop-player" | "loop player" => {
            if let Some(p) = ctx.get_loop_player() {
                SendTarget::SpecificPlayer(p)
            } else {
                log::warn!("send: target 'loop-player' but no loop-player is set");
                return;
            }
        }

        "victim" => {
            if let Some(p) = ctx.event_victim() {
                SendTarget::SpecificPlayer(p)
            } else {
                log::warn!("send: target 'victim' but no victim is set");
                return;
            }
        }

        "all players" | "all" => SendTarget::AllPlayers,

        "console" => {
            log::info!("{}", component.to_pretty_console());
            return;
        }

        other => {
            log::warn!("send: unsupported target '{}'", other);
            return;
        }
    };

    ctx.outbox.push(QueuedSend { target, component });
}

/// Strip one pair of wrapping quotes if the whole string is quoted.
fn strip_wrapping_quotes(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2 {
        let first = t.as_bytes()[0];
        let last = t.as_bytes()[t.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &t[1..t.len() - 1];
        }
    }
    t
}

/// Like strip_wrapping_quotes, but also removes a single stray leading/trailing quote.
fn strip_wrapping_quotes_loose(s: &str) -> String {
    let t = strip_wrapping_quotes(s).to_string();
    let trimmed = t.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let last = trimmed.as_bytes()[trimmed.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        return trimmed[1..].to_string();
    }
    if trimmed.ends_with('"') || trimmed.ends_with('\'') {
        return trimmed[..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

// Legacy parsing is handled by replacing & with § and letting the client render it.
