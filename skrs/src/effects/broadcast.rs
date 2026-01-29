use pumpkin_util::text::TextComponent;

use crate::runtime::context::{ExecutionContext, QueuedSend, SendTarget};
use crate::runtime::expr::eval_message_expr;

pub fn register() {
    crate::registry::effects::register_effect("broadcast", effect_broadcast);
}

pub fn effect_broadcast(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    let msg = eval_message_expr(s, ctx).to_string_lossy();
    if msg.is_empty() {
        return;
    }

    let msg_unquoted = strip_wrapping_quotes_loose(msg.trim());
    let legacy = msg_unquoted.replace('&', "§");
    let component = TextComponent::text(legacy);
    ctx.outbox.push(QueuedSend {
        target: SendTarget::AllPlayers,
        component,
    });
}

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
