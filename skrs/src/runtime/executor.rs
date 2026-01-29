// src/runtime/executor.rs

use std::{thread, time::Duration};

use async_recursion::async_recursion;
use crate::parser::ast::{Block, Stmt};
use crate::registry::effects::get_effect;
use crate::runtime::context::ExecutionContext;
use crate::runtime::dispatcher::flush_outbox;
use crate::runtime::expr::eval_message_expr;
use crate::runtime::vars;
use crate::runtime::value::Value;
use pumpkin::command::CommandSender;
use pumpkin_data::item::Item;
use pumpkin_world::inventory::Inventory;
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub enum ExecFlow {
    Continue,
    Return(Value),
    Stop,
}

#[async_recursion]
pub async fn execute_block(block: &Block, ctx: &mut ExecutionContext<'_>) -> ExecFlow {
    for stmt in &block.statements {
        match execute_stmt(stmt, ctx).await {
            ExecFlow::Continue => {
                if ctx.is_command_event() && !ctx.outbox.is_empty() {
                    let outbox = ctx.drain_outbox();
                    let event_player = ctx.event_player();
                    flush_outbox(ctx.server, outbox, event_player).await;
                }
            }
            other => return other,
        }
    }
    ExecFlow::Continue
}

#[async_recursion]
async fn execute_stmt(stmt: &Stmt, ctx: &mut ExecutionContext<'_>) -> ExecFlow {
    match stmt {
        Stmt::Raw { raw, .. } => execute_line(raw, ctx).await,

        Stmt::If {
            condition_raw,
            then_block,
            else_block,
            ..
        } => {
            if eval_condition(condition_raw, ctx).await {
                match execute_block(then_block, ctx).await {
                    ExecFlow::Continue => {}
                    other => return other,
                }
            } else if let Some(eb) = else_block {
                match execute_block(eb, ctx).await {
                    ExecFlow::Continue => {}
                    other => return other,
                }
            }
            ExecFlow::Continue
        }

        Stmt::LoopTimes { count_raw, body, .. } => {
            let s = eval_message_expr(count_raw, ctx).to_string_lossy();
            let n: i64 = s.trim().parse::<i64>().unwrap_or(0);
            if n <= 0 {
                return ExecFlow::Continue;
            }

            // Optional: expose a simple loop counter as local {_i} (1-based).
            let old_i = ctx.locals.get("i").cloned();
            let old_loop_index = ctx.loop_index;

            for idx in 1..=n {
                ctx.locals.insert(
                    "i".to_string(),
                    crate::runtime::value::Value::Number(idx as f64),
                );
                ctx.loop_index = Some(idx as usize);
                match execute_block(body, ctx).await {
                    ExecFlow::Continue => {}
                    other => {
                        ctx.loop_index = old_loop_index;
                        return other;
                    }
                }
            }

            // restore previous {_i}
            match old_i {
                Some(v) => {
                    ctx.locals.insert("i".to_string(), v);
                }
                None => {
                    ctx.locals.remove("i");
                }
            }
            ctx.loop_index = old_loop_index;
            ExecFlow::Continue
        }

        Stmt::LoopAllPlayers { body, .. } => {
            // You implement this helper in ExecutionContext:
            // fn all_players(&self) -> Vec<Arc<Player>>
            let players = ctx.all_players();

            // Save/restore current loop-player
            let old_loop = ctx.loop_player.clone();

            for p in players {
                ctx.loop_player = Some(p);
                match execute_block(body, ctx).await {
                    ExecFlow::Continue => {}
                    other => {
                        ctx.loop_player = old_loop;
                        return other;
                    }
                }
            }

            ctx.loop_player = old_loop;
            ExecFlow::Continue
        }

        Stmt::LoopList { list_raw, body, .. } => {
            let items = resolve_list_values(list_raw, ctx);
            if items.is_empty() {
                return ExecFlow::Continue;
            }

            let old_loop_value = ctx.loop_value.clone();
            let old_loop_index = ctx.loop_index;

            for (idx, v) in items.into_iter().enumerate() {
                ctx.loop_value = Some(v);
                ctx.loop_index = Some(idx + 1);
                match execute_block(body, ctx).await {
                    ExecFlow::Continue => {}
                    other => {
                        ctx.loop_value = old_loop_value;
                        ctx.loop_index = old_loop_index;
                        return other;
                    }
                }
            }

            ctx.loop_value = old_loop_value;
            ctx.loop_index = old_loop_index;
            ExecFlow::Continue
        }
    }
}

// -----------------------
// existing line executor
// -----------------------
pub async fn execute_line(line: &str, ctx: &mut ExecutionContext<'_>) -> ExecFlow {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return ExecFlow::Continue;
    }

    if trimmed.eq_ignore_ascii_case("stop") {
        return ExecFlow::Stop;
    }

    if let Some(rest) = trimmed.strip_prefix("return") {
        let expr = rest.trim();
        let value = if expr.is_empty() {
            Value::Null
        } else {
            eval_message_expr(expr, ctx)
        };
        return ExecFlow::Return(value);
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let effect_name = match parts.next() {
        Some(x) if !x.is_empty() => x,
        _ => return ExecFlow::Continue,
    };

    let args = parts.next().unwrap_or("").trim();

    // wait <duration>
    if effect_name.eq_ignore_ascii_case("wait") {
        if let Some(dur) = parse_duration(args, ctx) {
            if !ctx.outbox.is_empty() {
                let outbox = ctx.drain_outbox();
                let event_player = ctx.event_player();
                flush_outbox(ctx.server, outbox, event_player).await;
            }
            sleep_maybe(dur).await;
        }
        return ExecFlow::Continue;
    }

    // make console execute command "<cmd>"
    if effect_name.eq_ignore_ascii_case("make") {
        if let Some(cmd_raw) = parse_make_console_command(args) {
            let mut cmd = eval_message_expr(&cmd_raw, ctx).to_string_lossy();
            if let Some(stripped) = cmd.strip_prefix('/') {
                cmd = stripped.to_string();
            }
            log::info!("[skrs] console command: {}", cmd);
            let dispatcher = ctx.server.command_dispatcher.read().await;
            if let Err(e) = dispatcher
                .dispatch_command(&CommandSender::Console, ctx.server, &cmd)
                .await
            {
                log::warn!("[skrs] console command failed: {:?}", e);
            }
        }
        return ExecFlow::Continue;
    }

    // function call as statement (e.g., runDefaultLoad())
    if let Some((fname, fargs)) = parse_function_call(trimmed) {
        if let Some(func) = crate::registry::functions::get_function(&fname) {
            let _ = crate::runtime::functions::call_function_blocking(func, fargs, ctx);
            return ExecFlow::Continue;
        }
    }

    if let Some(effect) = get_effect(effect_name) {
        effect(args, ctx);
    } else {
        log::warn!("Unknown effect: {}", effect_name);
    }
    ExecFlow::Continue
}

async fn sleep_maybe(dur: Duration) {
    if tokio::runtime::Handle::try_current().is_ok() {
        sleep(dur).await;
    } else {
        thread::sleep(dur);
    }
}

// -----------------------
// MVP condition evaluator
// -----------------------

async fn eval_condition(cond: &str, ctx: &mut ExecutionContext<'_>) -> bool {
    let c = cond.trim();
    if c.is_empty() {
        return false;
    }

    // IMPORTANT: check "is not set" before "is set"
    if let Some(left) = strip_suffix_ci(c, " is not set") {
        return !is_token_set(left.trim(), ctx);
    }

    if let Some(left) = strip_suffix_ci(c, " is set") {
        return is_token_set(left.trim(), ctx);
    }

    if let Some((left, right)) = split_contains_ci(c, " does not contain ") {
        return !contains_token(left, right, ctx).await;
    }
    if let Some((left, right)) = split_contains_ci(c, " doesn't contain ") {
        return !contains_token(left, right, ctx).await;
    }
    if let Some((left, right)) = split_contains_ci(c, " contains ") {
        return contains_token(left, right, ctx).await;
    }

    if let Some((left, right)) = split_perm_ci(c, " doesn't have permission ") {
        return !has_permission(left, right, ctx).await;
    }
    if let Some((left, right)) = split_perm_ci(c, " does not have permission ") {
        return !has_permission(left, right, ctx).await;
    }
    if let Some((left, right)) = split_perm_ci(c, " has permission ") {
        return has_permission(left, right, ctx).await;
    }

    if let Some((left, right)) = split_has_ci(c, " has ") {
        return player_has_item(left, right, ctx).await;
    }

    if let Some((a, b)) = split_compare_ci(c, ">=") {
        return compare_numbers(a, b, ctx, |l, r| l >= r);
    }
    if let Some((a, b)) = split_compare_ci(c, "<=") {
        return compare_numbers(a, b, ctx, |l, r| l <= r);
    }
    if let Some((a, b)) = split_compare_ci(c, ">") {
        return compare_numbers(a, b, ctx, |l, r| l > r);
    }
    if let Some((a, b)) = split_compare_ci(c, "<") {
        return compare_numbers(a, b, ctx, |l, r| l < r);
    }

    if let Some((a, b)) = split_is_more_than_ci(c) {
        return compare_numbers(a, b, ctx, |l, r| l > r);
    }
    if let Some((a, b)) = split_is_less_than_ci(c) {
        return compare_numbers(a, b, ctx, |l, r| l < r);
    }

    if let Some((a, b)) = split_is_not_ci(c) {
        let left = eval_message_expr(a.trim(), ctx).to_string_lossy();
        let right = eval_message_expr(b.trim(), ctx).to_string_lossy();
        if let (Some(ln), Some(rn)) = (parse_number(&left), parse_number(&right)) {
            return ln != rn;
        }
        return left != right;
    }

    if let Some((a, b)) = split_is_ci(c) {
        let left = eval_message_expr(a.trim(), ctx).to_string_lossy();
        let right = eval_message_expr(b.trim(), ctx).to_string_lossy();
        if let (Some(ln), Some(rn)) = (parse_number(&left), parse_number(&right)) {
            return (ln - rn).abs() < f64::EPSILON;
        }
        return left == right;
    }

    false
}

fn split_is_not_ci(s: &str) -> Option<(&str, &str)> {
    let lower = s.to_ascii_lowercase();
    let needle = " is not ";
    let idx = lower.find(needle)?;
    Some((&s[..idx], &s[idx + needle.len()..]))
}

fn split_is_ci(s: &str) -> Option<(&str, &str)> {
    let lower = s.to_ascii_lowercase();
    let needle = " is ";
    let idx = lower.find(needle)?;
    Some((&s[..idx], &s[idx + needle.len()..]))
}

fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let sl = s.to_ascii_lowercase();
    let su = suffix.to_ascii_lowercase();
    if sl.ends_with(&su) {
        Some(&s[..s.len() - suffix.len()])
    } else {
        None
    }
}

/// Returns true only if the token exists AND its value is not Null.
fn is_token_set(token: &str, ctx: &mut ExecutionContext<'_>) -> bool {
    if let Some(key) = parse_var_token(token) {
        let key = crate::runtime::expr::resolve_var_key(&key, ctx);
        // locals: {_x} stored as "x" in ctx.locals
        if let Some(rest) = key.strip_prefix('_') {
            return matches!(ctx.locals.get(rest), Some(v) if !matches!(v, Value::Null));
        }
        return matches!(vars::get_global(&key), Some(v) if !matches!(v, Value::Null));
    }

    if token.trim().to_ascii_lowercase().starts_with("arg-") {
        return ctx.get_arg_value(token).is_some();
    }

    false
}

fn parse_var_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() >= 3 && t.starts_with('{') && t.ends_with('}') {
        Some(t[1..t.len() - 1].trim().to_string())
    } else {
        None
    }
}

fn split_contains_ci<'a>(s: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let lower = s.to_ascii_lowercase();
    let idx = lower.find(needle)?;
    Some((&s[..idx], &s[idx + needle.len()..]))
}

fn split_compare_ci<'a>(s: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let lower = s.to_ascii_lowercase();
    let idx = lower.find(op)?;
    Some((&s[..idx], &s[idx + op.len()..]))
}

fn split_is_more_than_ci(s: &str) -> Option<(&str, &str)> {
    let lower = s.to_ascii_lowercase();
    let needle = " is more than ";
    let idx = lower.find(needle)?;
    Some((&s[..idx], &s[idx + needle.len()..]))
}

fn split_is_less_than_ci(s: &str) -> Option<(&str, &str)> {
    let lower = s.to_ascii_lowercase();
    let needle = " is less than ";
    let idx = lower.find(needle)?;
    Some((&s[..idx], &s[idx + needle.len()..]))
}

fn split_perm_ci<'a>(s: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let lower = s.to_ascii_lowercase();
    let idx = lower.find(needle)?;
    Some((&s[..idx], &s[idx + needle.len()..]))
}

fn split_has_ci<'a>(s: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let lower = s.to_ascii_lowercase();
    let idx = lower.find(needle)?;
    Some((&s[..idx], &s[idx + needle.len()..]))
}

fn compare_numbers(
    a: &str,
    b: &str,
    ctx: &mut ExecutionContext<'_>,
    cmp: impl Fn(f64, f64) -> bool,
) -> bool {
    let left = eval_message_expr(a.trim(), ctx).to_string_lossy();
    let right = eval_message_expr(b.trim(), ctx).to_string_lossy();
    let ln = parse_number(&left).unwrap_or(0.0);
    let rn = parse_number(&right).unwrap_or(0.0);
    cmp(ln, rn)
}

fn parse_number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

async fn contains_token(left: &str, right: &str, ctx: &mut ExecutionContext<'_>) -> bool {
    let left_trim = left.trim();
    let right_trim = right.trim();

    // List contains
    if let Some(key) = parse_var_token(left_trim) {
        let key = crate::runtime::expr::resolve_var_key(&key, ctx);
        if key.ends_with("::*") {
            if let Some(Value::List(items)) = vars::get_global(&key).or_else(|| {
                if let Some(rest) = key.strip_prefix('_') {
                    ctx.locals.get(rest).cloned()
                } else {
                    None
                }
            }) {
                let needles = split_or_values(right_trim, ctx);
                return items.iter().any(|v| needles.contains(&v.to_string_lossy()));
            }
        }
    }

    // String contains with OR
    let left_val = eval_message_expr(left_trim, ctx).to_string_lossy();
    let needles = split_or_values(right_trim, ctx);
    for n in needles {
        if left_val.contains(&n) {
            return true;
        }
    }
    false
}

fn split_or_values(s: &str, ctx: &mut ExecutionContext<'_>) -> Vec<String> {
    s.split(" or ")
        .map(|p| eval_message_expr(p.trim(), ctx).to_string_lossy())
        .filter(|x| !x.is_empty())
        .collect()
}

async fn has_permission(left: &str, right: &str, ctx: &mut ExecutionContext<'_>) -> bool {
    let left = left.trim();
    let perm = eval_message_expr(right.trim(), ctx).to_string_lossy();
    if perm.is_empty() {
        return false;
    }
    if left.eq_ignore_ascii_case("player") {
        if let Some(p) = ctx.event_player() {
            return p.has_permission(ctx.server, &perm).await;
        }
    }
    false
}

async fn player_has_item(left: &str, right: &str, ctx: &mut ExecutionContext<'_>) -> bool {
    let left = left.trim();
    if !left.eq_ignore_ascii_case("player") {
        return false;
    }
    let perm = eval_message_expr(right.trim(), ctx).to_string_lossy();
    if let Some(p) = ctx.event_player() {
        if let Some((amount, item)) = parse_item_amount(&perm) {
            let count = p.inventory().count(item).await;
            return count >= amount;
        }
    }
    false
}

fn parse_item_amount(s: &str) -> Option<(u8, &'static Item)> {
    let text = s.trim();
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

fn resolve_list_values(list_raw: &str, ctx: &mut ExecutionContext<'_>) -> Vec<Value> {
    let raw = list_raw.trim();
    if let Some(key) = parse_var_token(raw) {
        let key = crate::runtime::expr::resolve_var_key(&key, ctx);
        if let Some(rest) = key.strip_prefix('_') {
            if let Some(Value::List(items)) = ctx.locals.get(rest).cloned() {
                return items;
            }
        }
        if let Some(Value::List(items)) = vars::get_global(&key) {
            return items;
        }
    }
    Vec::new()
}

fn parse_duration(s: &str, ctx: &mut ExecutionContext<'_>) -> Option<Duration> {
    let mut parts = s.trim().split_whitespace();
    let amount_part = parts.next()?;
    let unit = parts.next()?.to_ascii_lowercase();
    let amount_text = eval_message_expr(amount_part, ctx).to_string_lossy();
    let n = amount_text.parse::<f64>().ok()?;
    let millis = match unit.as_str() {
        "tick" | "ticks" => (n * 50.0) as u64,
        "second" | "seconds" => (n * 1000.0) as u64,
        "minute" | "minutes" => (n * 60_000.0) as u64,
        "hour" | "hours" => (n * 3_600_000.0) as u64,
        _ => return None,
    };
    Some(Duration::from_millis(millis))
}

fn parse_make_console_command(args: &str) -> Option<String> {
    let trimmed = args.trim();
    let lower = trimmed.to_ascii_lowercase();
    let prefix = "console execute command ";
    if !lower.starts_with(prefix) {
        return None;
    }
    let cmd = trimmed[prefix.len()..].trim();
    Some(strip_wrapping_quotes(cmd).to_string())
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

fn parse_function_call(s: &str) -> Option<(String, Vec<String>)> {
    let t = s.trim();
    let lparen = t.find('(')?;
    let rparen = t.rfind(')')?;
    if rparen <= lparen {
        return None;
    }
    let name = t[..lparen].trim();
    if name.is_empty() {
        return None;
    }
    let args_str = t[lparen + 1..rparen].trim();
    let args = if args_str.is_empty() {
        Vec::new()
    } else {
        args_str
            .split(',')
            .map(|a| a.trim().to_string())
            .collect()
    };
    Some((name.to_string(), args))
}
