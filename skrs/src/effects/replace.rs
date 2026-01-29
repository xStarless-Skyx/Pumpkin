use crate::runtime::{context::ExecutionContext, expr::eval_message_expr, value::Value, vars};

pub fn register() {
    crate::registry::effects::register_effect("replace", effect_replace);
}

/// replace [all] "<needle>" in <var> with "<replacement>"
/// replace [all] "<needle>" with "<replacement>" in <var>
pub fn effect_replace(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    let rest = s.strip_prefix("all ").map(|r| r.trim()).unwrap_or(s);

    // Support both orders:
    // 1) <needle> in <var> with <replacement>
    // 2) <needle> with <replacement> in <var>
    let (needle_part, var_part, repl_part) = if let Some((a, b)) = rest.split_once(" in ") {
        let (var_part, repl_part) = match b.split_once(" with ") {
            Some(x) => x,
            None => {
                log::warn!("replace: expected '... in <var> with <replacement>', got '{}'", s);
                return;
            }
        };
        (a, var_part, repl_part)
    } else if let Some((a, b)) = rest.split_once(" with ") {
        let (repl_part, var_part) = match b.split_once(" in ") {
            Some(x) => x,
            None => {
                log::warn!("replace: expected '... with <replacement> in <var>', got '{}'", s);
                return;
            }
        };
        (a, var_part, repl_part)
    } else {
        log::warn!("replace: expected 'replace <needle> in <var> with <replacement>' (or with/in order), got '{}'", s);
        return;
    };

    let key = match parse_var_token(var_part.trim()) {
        Some(k) => k,
        None => {
            log::warn!("replace: expected variable like {{_x}} or {{path::key}}, got '{}'", var_part);
            return;
        }
    };
    let key = crate::runtime::expr::resolve_var_key(&key, ctx);

    let needle = eval_message_expr(needle_part.trim(), ctx).to_string_lossy();
    let replacement = eval_message_expr(repl_part.trim(), ctx).to_string_lossy();

    let cur = read_var(&key, ctx);
    let new_val = match cur {
        Some(Value::String(cur_s)) => Value::String(cur_s.replace(&needle, &replacement)),
        Some(v) => v,
        None => Value::Null,
    };

    write_var(&key, new_val, ctx);
}

fn parse_var_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() >= 3 && t.starts_with('{') && t.ends_with('}') {
        Some(t[1..t.len() - 1].trim().to_string())
    } else {
        None
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
