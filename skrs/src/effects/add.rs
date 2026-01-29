// src/effects/add.rs

use crate::runtime::{
    context::ExecutionContext,
    expr::eval_message_expr,
    value::Value,
    vars,
};

pub fn register() {
    // (add|give)
    crate::registry::effects::register_effect("add", effect_add);
    crate::registry::effects::register_effect("give", effect_add);
}

/// Supported:
///   add <amount> to <var>
///   give <amount> to <var>
///   give <amount> <var>
/// Examples:
///   add 5 to {_x}
///   give 1 to {kills::%player%}
///   give 3 {_x}
pub fn effect_add(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    // Accept either:
    //  - "<amount> to <target>"
    //  - "<amount> <target>"
    let (amount_part, target_part) = if let Some((l, r)) = s.rsplit_once(" to ") {
        (l.trim(), r.trim())
    } else if let Some((l, r)) = split_first_whitespace(s) {
        (l.trim(), r.trim())
    } else {
        log::warn!(
            "add/give: expected 'add <amount> to <var>' or 'give <amount> <var>', got '{}'",
            s
        );
        return;
    };

    if amount_part.is_empty() || target_part.is_empty() {
        return;
    }

    // Target must be { ... }
    let var_key = match parse_var_token(target_part) {
        Some(k) => k,
        None => {
            log::warn!(
                "add/give: expected variable like {{_x}} or {{path::key}}, got '{}'",
                target_part
            );
            return;
        }
    };
    let var_key = crate::runtime::expr::resolve_var_key(&var_key, ctx);

    // List append: add <value> to {list::*}
    if is_list_key(&var_key) || matches!(read_var(&var_key, ctx), Some(Value::List(_))) {
        let mut list = match read_var(&var_key, ctx) {
            Some(Value::List(items)) => items,
            _ => Vec::new(),
        };

        let values = split_list_values(amount_part, ctx);
        for v in values {
            list.push(Value::String(v));
        }

        write_var(&var_key, Value::List(list), ctx);
        return;
    }

    let delta_str = eval_message_expr(amount_part, ctx).to_string_lossy().to_string();
    let delta = parse_number(&delta_str);

    let current = value_to_number(read_var(&var_key, ctx).as_ref());
    let new_num = current + delta;

    write_var(&var_key, number_to_value(new_num), ctx);
}

// ---------------- helpers ----------------

fn split_first_whitespace(s: &str) -> Option<(&str, &str)> {
    let mut it = s.splitn(2, char::is_whitespace);
    let a = it.next()?.trim();
    let b = it.next()?.trim();
    if a.is_empty() || b.is_empty() {
        None
    } else {
        Some((a, b))
    }
}

/// Parses "{_x}" or "{money::player}" into "_x" / "money::player"
fn parse_var_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() >= 3 && t.starts_with('{') && t.ends_with('}') {
        Some(t[1..t.len() - 1].trim().to_string())
    } else {
        None
    }
}

fn parse_number(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

fn value_to_number(v: Option<&Value>) -> f64 {
    v.map(|x| x.to_string_lossy())
        .unwrap_or_default()
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0)
}

fn number_to_value(n: f64) -> Value {
    if n.is_finite() {
        Value::Number(n)
    } else {
        Value::Number(0.0)
    }
}

fn read_var(key: &str, ctx: &ExecutionContext<'_>) -> Option<Value> {
    if let Some(rest) = key.strip_prefix('_') {
        ctx.locals.get(rest).cloned()
    } else {
        vars::get_global(key)
    }
}

fn write_var(key: &str, value: Value, ctx: &mut ExecutionContext<'_>) {
    if let Some(rest) = key.strip_prefix('_') {
        ctx.locals.insert(rest.to_string(), value);
    } else {
        vars::set_global(key, value);
    }
}

fn is_list_key(key: &str) -> bool {
    key.ends_with("::*")
}

fn split_list_values(s: &str, ctx: &mut ExecutionContext<'_>) -> Vec<String> {
    let raw = eval_message_expr(s, ctx).to_string_lossy();
    let mut out = Vec::new();
    for part in raw.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        // Handle "and" in the last segment
        if p.contains(" and ") {
            for sub in p.split(" and ") {
                let s = sub.trim();
                if !s.is_empty() {
                    out.push(s.to_string());
                }
            }
        } else {
            out.push(p.to_string());
        }
    }
    if out.is_empty() && !raw.trim().is_empty() {
        out.push(raw.trim().to_string());
    }
    out
}
