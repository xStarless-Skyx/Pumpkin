// src/effects/set.rs

use crate::runtime::{
    context::ExecutionContext,
    expr::eval_message_expr,
    value::Value,
    vars,
};

pub fn register() {
    crate::registry::effects::register_effect("set", effect_set);
    crate::registry::effects::register_effect("delete", effect_delete);
    crate::registry::effects::register_effect("clear", effect_delete); // ✅ alias
}

/// set {_x} to "hello"
/// set {global::thing} to "works"
pub fn effect_set(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    // Expect: <var> to <value>
    let (left, right) = match s.split_once(" to ") {
        Some((l, r)) => (l.trim(), r.trim()),
        None => {
            log::warn!("set: expected 'set <var> to <value>', got '{}'", s);
            return;
        }
    };

    let var_key = match parse_var_token(left) {
        Some(k) => k,
        None => {
            log::warn!("set: expected variable like {{_x}} or {{path::key}}, got '{}'", left);
            return;
        }
    };
    let var_key = crate::runtime::expr::resolve_var_key(&var_key, ctx);

    // List assignment: set {_list::*} to {other::*}
    if is_list_key(&var_key) {
        if let Some(src_key) = parse_var_token(right) {
            if is_list_key(&src_key) {
                let list_val = read_var(&src_key, ctx);
                if let Value::List(items) = list_val {
                    write_var(&var_key, Value::List(items), ctx);
                    return;
                }
            }
        }
        // Fallback: set list to single value
        let v = eval_message_expr(right, ctx);
        write_var(&var_key, Value::List(vec![v]), ctx);
        return;
    }

    // Otherwise treat RHS as a message expression
    let value = eval_message_expr(right, ctx);
    write_var(&var_key, value, ctx);
}

/// delete {_x}
/// delete {global::thing}
pub fn effect_delete(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    let var_key = match parse_var_token(s) {
        Some(k) => k,
        None => {
            log::warn!("delete: expected variable like {{_x}} or {{path::key}}, got '{}'", s);
            return;
        }
    };
    let var_key = crate::runtime::expr::resolve_var_key(&var_key, ctx);

    delete_var(&var_key, ctx);
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

fn write_var(key: &str, value: Value, ctx: &mut ExecutionContext<'_>) {
    // Local vars are {_name} -> store under "name" in ctx.locals
    if let Some(rest) = key.strip_prefix('_') {
        ctx.locals.insert(rest.to_string(), value);
    } else {
        // Global vars are {something::path}
        vars::set_global(key, value);
    }
}

fn delete_var(key: &str, ctx: &mut ExecutionContext<'_>) {
    if let Some(rest) = key.strip_prefix('_') {
        ctx.locals.remove(rest);
    } else {
        vars::del_global(key);
    }
}

fn is_list_key(key: &str) -> bool {
    key.ends_with("::*")
}

fn read_var(key: &str, ctx: &ExecutionContext<'_>) -> Value {
    if let Some(rest) = key.strip_prefix('_') {
        ctx.locals.get(rest).cloned().unwrap_or(Value::Null)
    } else {
        vars::get_global(key).unwrap_or(Value::Null)
    }
}
