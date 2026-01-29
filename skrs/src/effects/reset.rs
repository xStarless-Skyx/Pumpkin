// src/effects/reset.rs

use crate::runtime::{context::ExecutionContext, value::Value, vars};

pub fn register() {
    crate::registry::effects::register_effect("reset", effect_reset);
}

/// reset {_x}
/// reset {global::thing}
pub fn effect_reset(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    let key = match parse_var_token(s) {
        Some(k) => k,
        None => {
            log::warn!("reset: expected variable like {{_x}} or {{path::key}}, got '{}'", s);
            return;
        }
    };
    let key = crate::runtime::expr::resolve_var_key(&key, ctx);

    // "Reset" = set to None (default)
    if let Some(rest) = key.strip_prefix('_') {
        ctx.locals.insert(rest.to_string(), Value::Null);
    } else {
        vars::set_global(&key, Value::Null);
    }
}

fn parse_var_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() >= 3 && t.starts_with('{') && t.ends_with('}') {
        Some(t[1..t.len() - 1].trim().to_string())
    } else {
        None
    }
}
