// src/effects/remove.rs

use crate::runtime::{
    context::ExecutionContext,
    expr::eval_message_expr,
    value::Value,
    vars,
};
use pumpkin_data::item::Item;
use pumpkin_world::inventory::Inventory;

pub fn register() {
    // (remove|subtract) X from Y
    crate::registry::effects::register_effect("remove", effect_remove);
    crate::registry::effects::register_effect("subtract", effect_remove);

    // (reduce|decrease) Y by X
    crate::registry::effects::register_effect("reduce", effect_decrease_by);
    crate::registry::effects::register_effect("decrease", effect_decrease_by);
}

/// remove <amount> from <var>
/// subtract <amount> from <var>
/// remove all <text> from <var>   (string-only convenience)
pub fn effect_remove(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    // Special: "all " / "every "
    if let Some(rest) = s.strip_prefix("all ").or_else(|| s.strip_prefix("every ")) {
        // remove all <thing> from <var>
        let (thing_part, var_part) = match rest.split_once(" from ") {
            Some((l, r)) => (l.trim(), r.trim()),
            None => {
                log::warn!(
                    "remove all: expected 'remove all <thing> from <var>', got '{}'",
                    s
                );
                return;
            }
        };

        let key = match parse_var_token(var_part) {
            Some(k) => k,
            None => {
                log::warn!(
                    "remove all: expected variable like {{_x}} or {{path::key}}, got '{}'",
                    var_part
                );
                return;
            }
        };
        let key = crate::runtime::expr::resolve_var_key(&key, ctx);

        let thing = eval_message_expr(thing_part, ctx).to_string_lossy().to_string();
        let cur = read_var(&key, ctx);

        let new_val = match cur {
            Some(Value::String(cur_s)) => Value::String(cur_s.replace(&thing, "").into()),
            Some(Value::List(items)) => {
                let filtered = items
                    .into_iter()
                    .filter(|v| v.to_string_lossy() != thing)
                    .collect();
                Value::List(filtered)
            }
            Some(v) => v, // non-string: no-op for now
            None => Value::Null,
        };

        write_var(&key, new_val, ctx);
        return;
    }

    // Normal: remove/subtract <amount> from <var>
    let (amt_part, var_part) = match s.split_once(" from ") {
        Some((l, r)) => (l.trim(), r.trim()),
        None => {
            log::warn!(
                "remove/subtract: expected 'remove <amount> from <var>', got '{}'",
                s
            );
            return;
        }
    };

    // Inventory removal: remove <amount> <item> from player
    if var_part.eq_ignore_ascii_case("player") {
        if let Some(p) = ctx.event_player() {
            if let Some((amount, item)) = parse_item_amount(amt_part, ctx) {
                let _ = remove_items_from_inventory(&p, item, amount);
            }
        }
        return;
    }

    let key = match parse_var_token(var_part) {
        Some(k) => k,
        None => {
            log::warn!(
                "remove/subtract: expected variable like {{_x}} or {{path::key}}, got '{}'",
                var_part
            );
            return;
        }
    };
    let key = crate::runtime::expr::resolve_var_key(&key, ctx);

    // If target is a list, remove matching values
    if is_list_key(&key) {
        let needle = eval_message_expr(amt_part, ctx).to_string_lossy().to_string();
        let cur = read_var(&key, ctx);
        let new_val = match cur {
            Some(Value::List(items)) => {
                let filtered = items
                    .into_iter()
                    .filter(|v| v.to_string_lossy() != needle)
                    .collect();
                Value::List(filtered)
            }
            Some(v) => v,
            None => Value::Null,
        };
        write_var(&key, new_val, ctx);
        return;
    }

    let amt_str = eval_message_expr(amt_part, ctx).to_string_lossy().to_string();
    let amt = parse_number(&amt_str);

    let cur = value_to_number(read_var(&key, ctx).as_ref());
    let new_num = cur - amt;

    write_var(&key, number_to_value(new_num), ctx);
}

/// decrease <var> by <amount>
/// reduce <var> by <amount>
pub fn effect_decrease_by(arg_text: &str, ctx: &mut ExecutionContext<'_>) {
    let s = arg_text.trim();
    if s.is_empty() {
        return;
    }

    let (var_part, amt_part) = match s.split_once(" by ") {
        Some((l, r)) => (l.trim(), r.trim()),
        None => {
            log::warn!("decrease/reduce: expected '<var> by <amount>', got '{}'", s);
            return;
        }
    };

    let key = match parse_var_token(var_part) {
        Some(k) => k,
        None => {
            log::warn!(
                "decrease/reduce: expected variable like {{_x}} or {{path::key}}, got '{}'",
                var_part
            );
            return;
        }
    };
    let key = crate::runtime::expr::resolve_var_key(&key, ctx);

    let amt_str = eval_message_expr(amt_part, ctx).to_string_lossy().to_string();
    let amt = parse_number(&amt_str);

    let cur = value_to_number(read_var(&key, ctx).as_ref());
    let new_num = cur - amt;

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

fn is_list_key(key: &str) -> bool {
    key.ends_with("::*")
}

fn parse_item_amount(s: &str, ctx: &mut ExecutionContext<'_>) -> Option<(u8, &'static Item)> {
    let text = eval_message_expr(s, ctx).to_string_lossy();
    let mut parts = text.split_whitespace();
    let first = parts.next()?;
    let (amount, rest) = if let Ok(n) = first.parse::<u8>() {
        (n, parts.collect::<Vec<_>>().join(" "))
    } else {
        (1, std::iter::once(first).chain(parts).collect::<Vec<_>>().join(" "))
    };
    let key = normalize_item_key(&rest);
    let item = Item::from_registry_key(&format!("minecraft:{key}"))?;
    Some((amount, item))
}

fn normalize_item_key(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

fn remove_items_from_inventory(
    player: &pumpkin::entity::player::Player,
    item: &'static Item,
    mut amount: u8,
) -> bool {
    let inv = player.inventory();
    let fut = async {
        for slot in 0..inv.size() {
            if amount == 0 {
                break;
            }
            let stack = inv.get_stack(slot).await;
            let mut stack = stack.lock().await;
            if stack.get_item().id != item.id || stack.item_count == 0 {
                continue;
            }
            let take = amount.min(stack.item_count);
            stack.item_count -= take;
            amount -= take;
        }
        amount == 0
    };
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build runtime for inventory removal");
            rt.block_on(fut)
        }
    }
}
