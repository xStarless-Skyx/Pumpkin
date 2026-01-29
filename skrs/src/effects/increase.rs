// src/effects/increase.rs

use crate::runtime::{
    context::ExecutionContext,
    expr::eval_message_expr,
    value::Value,
    vars,
};

pub fn register() {
    crate::registry::effects::register_effect("increase", effect_increase);
}

/// increase <var> by <amount>
/// Example: increase {_x} by 5
pub fn effect_increase(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    let (var_part, amt_part) = match s.split_once(" by ") {
        Some((l, r)) => (l.trim(), r.trim()),
        None => {
            log::warn!("increase: expected 'increase <var> by <amount>', got '{}'", s);
            return;
        }
    };

    let key = match parse_var_token(var_part) {
        Some(k) => k,
        None => {
            log::warn!(
                "increase: expected variable like {{_x}} or {{path::key}}, got '{}'",
                var_part
            );
            return;
        }
    };
    let key = crate::runtime::expr::resolve_var_key(&key, ctx);

    let amt_str = eval_message_expr(amt_part, ctx).to_string_lossy().to_string();
    let amt = parse_number(&amt_str);

    let cur = value_to_number(read_var(&key, ctx).as_ref());
    let new_num = cur + amt;

    write_var(&key, number_to_value(new_num), ctx);
}

// ---------------- helpers ----------------

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
