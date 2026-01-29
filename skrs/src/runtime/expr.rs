// src/runtime/expr.rs

use crate::runtime::{context::ExecutionContext, value::Value, vars};
use crate::runtime::functions::call_function_blocking;
use crate::registry::functions::get_function;
use crate::registry::options::get_option;

pub fn eval_message_expr(input: &str, ctx: &mut ExecutionContext<'_>) -> Value {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }

    // Only treat " & " (space-amp-space) as concatenation.
    let parts: Vec<&str> = if trimmed.contains(" & ") {
        trimmed
            .split(" & ")
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect()
    } else {
        vec![trimmed]
    };

    let mut out = String::new();

    for part in parts {
        // Whole-token variable: "{_x}"
        if let Some(var_key) = parse_var_token(part) {
            let key = resolve_var_key(&var_key, ctx);
            out.push_str(&read_var(&key, ctx).to_string_lossy());
            continue;
        }

        // Function call token
        if let Some((fname, fargs)) = parse_function_call(part) {
            if let Some(func) = get_function(&fname) {
                let v = call_function_blocking(func, fargs, ctx);
                out.push_str(&v.to_string_lossy());
                continue;
            }
        }

        // arg-<n>
        if let Some(arg_val) = ctx.get_arg_value(part) {
            out.push_str(&arg_val.to_string_lossy());
            continue;
        }

        // lowercase <expr>
        if let Some(rest) = part.strip_prefix("lowercase ") {
            let v = eval_message_expr(rest, ctx).to_string_lossy().to_ascii_lowercase();
            out.push_str(&v);
            continue;
        }

        // uuid/name of player/victim
        if eq_ci(part, "uuid of player") {
            if let Some(p) = ctx.event_player() {
                out.push_str(&p.gameprofile.id.to_string());
            }
            continue;
        }
        if eq_ci(part, "name of player") {
            if let Some(name) = ctx.event_player_name() {
                out.push_str(&name);
            }
            continue;
        }
        if eq_ci(part, "uuid of victim") {
            if let Some(p) = ctx.event_victim() {
                out.push_str(&p.gameprofile.id.to_string());
            }
            continue;
        }
        if eq_ci(part, "name of victim") {
            if let Some(name) = ctx.event_victim_name() {
                out.push_str(&name);
            }
            continue;
        }

        // loop-value / loop-number
        if eq_ci(part, "loop-value") {
            if let Some(v) = ctx.loop_value.as_ref() {
                out.push_str(&v.to_string_lossy());
            }
            continue;
        }
        if eq_ci(part, "loop-number") {
            if let Some(n) = ctx.loop_index {
                out.push_str(&n.to_string());
            }
            continue;
        }

        // Expressions: event-player / loop-player / message
        if eq_ci(part, "player") || eq_ci(part, "event-player") || eq_ci(part, "event player") {
            if let Some(name) = ctx.event_player_name() {
                out.push_str(&name);
            }
            continue;
        }

        if eq_ci(part, "loop-player") || eq_ci(part, "loop player") {
            if let Some(name) = ctx.loop_player_name() {
                out.push_str(&name);
            }
            continue;
        }

        if eq_ci(part, "victim") {
            if let Some(name) = ctx.event_victim_name() {
                out.push_str(&name);
            }
            continue;
        }

        if eq_ci(part, "message") {
            if let Some(msg) = ctx.event_message() {
                out.push_str(&msg);
            }
            continue;
        }

        if eq_ci(part, "now") {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            out.push_str(&now.to_string());
            continue;
        }

        if let Some(rest) = part.strip_prefix("difference between ") {
            if let Some(left) = rest.strip_suffix(" and now") {
                let left_val = eval_message_expr(left, ctx).to_string_lossy();
                let left_num = parse_number(&left_val);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                let diff = (now - left_num).abs();
                out.push_str(&diff.to_string());
                continue;
            }
        }

        if let Some(seconds) = parse_timespan_to_seconds(part) {
            out.push_str(&seconds.to_string());
            continue;
        }

        // Otherwise normal text (maybe quoted)
        let mut s = unquote(part);

        // %player% and %event-player% (aliases)
        if s.contains("%player%") || s.contains("%event-player%") {
            if let Some(name) = ctx.event_player_name() {
                s = s.replace("%player%", &name);
                s = s.replace("%event-player%", &name);
            }
        }

        // %loop-player%
        if s.contains("%loop-player%") {
            if let Some(name) = ctx.loop_player_name() {
                s = s.replace("%loop-player%", &name);
            }
        }

        // %loop-value% / %loop-number%
        if s.contains("%loop-value%") {
            if let Some(v) = ctx.loop_value.as_ref() {
                s = s.replace("%loop-value%", &v.to_string_lossy());
            }
        }
        if s.contains("%loop-number%") {
            if let Some(n) = ctx.loop_index {
                s = s.replace("%loop-number%", &n.to_string());
            }
        }

        // %victim%
        if s.contains("%victim%") {
            if let Some(name) = ctx.event_victim_name() {
                s = s.replace("%victim%", &name);
            }
        }

        // ✅ %message%
        if s.contains("%message%") {
            if let Some(msg) = ctx.event_message() {
                s = s.replace("%message%", &msg);
            }
        }

        // %{var}% placeholders
        s = replace_var_placeholders(&s, ctx);
        s = replace_arg_placeholders(&s, ctx);
        s = replace_option_placeholders(&s);
        // If replacements introduced wrapping quotes, strip again
        s = unquote(&s);
        // Second pass to resolve placeholders introduced by variables/options.
        s = replace_var_placeholders(&s, ctx);
        s = replace_arg_placeholders(&s, ctx);
        s = replace_option_placeholders(&s);
        s = unquote(&s);

        out.push_str(&s);
    }

    Value::String(out)
}

pub fn resolve_var_key(raw: &str, ctx: &mut ExecutionContext<'_>) -> String {
    eval_message_expr(raw, ctx).to_string_lossy()
}

fn read_var(key: &str, ctx: &mut ExecutionContext<'_>) -> Value {
    // locals are {_x} stored under "x"
    if let Some(rest) = key.strip_prefix('_') {
        return ctx.locals.get(rest).cloned().unwrap_or(Value::Null);
    }

    // globals
    vars::get_global(key).unwrap_or(Value::Null)
}

fn read_var_opt(key: &str, ctx: &mut ExecutionContext<'_>) -> Option<Value> {
    if let Some(rest) = key.strip_prefix('_') {
        return ctx.locals.get(rest).cloned();
    }
    vars::get_global(key)
}

/// Parses "{_x}" / "{global::test}" into "_x" / "global::test"
fn parse_var_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() >= 3 && t.starts_with('{') && t.ends_with('}') {
        Some(t[1..t.len() - 1].trim().to_string())
    } else {
        None
    }
}

/// Replaces "%{...}%" occurrences inside a string.
fn replace_var_placeholders(s: &str, ctx: &mut ExecutionContext<'_>) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some((key_end, advance)) = find_close_placeholder(bytes, i + 2) {
                let key = s[i + 2..key_end].trim();
                let resolved = resolve_var_key(key, ctx);
                if let Some(arg_val) = ctx.get_arg_value(&resolved) {
                    out.push_str(&arg_val.to_string_lossy());
                } else if let Some(v) = read_var_opt(&resolved, ctx) {
                    out.push_str(&v.to_string_lossy());
                } else if let Some(opt) = get_option(&resolved) {
                    out.push_str(&opt);
                }
                i = key_end + advance; // skip closing delimiter
                continue;
            }
        }

        out.push(bytes[i] as char);
        i += 1;
    }

    out
}

fn replace_option_placeholders(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'{' && i + 2 < bytes.len() && bytes[i + 1] == b'@' {
            if let Some(end_brace) = s[i + 2..].find('}') {
                let key = s[i + 2..i + 2 + end_brace].trim();
                if let Some(val) = get_option(key) {
                    out.push_str(&val);
                }
                i = i + 2 + end_brace + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }

    out
}

/// Replaces "%arg-<n>%" occurrences inside a string.
fn replace_arg_placeholders(s: &str, ctx: &mut ExecutionContext<'_>) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(end) = s[i + 1..].find('%') {
                let token = &s[i + 1..i + 1 + end];
                let token_lc = token.to_ascii_lowercase();
                if token_lc.starts_with("arg-") {
                    if let Some(val) = ctx.get_arg_value(token) {
                        out.push_str(&val.to_string_lossy());
                        i = i + 1 + end + 1;
                        continue;
                    }
                } else if token_lc == "loop-number" {
                    if let Some(n) = ctx.loop_index {
                        out.push_str(&n.to_string());
                        i = i + 1 + end + 1;
                        continue;
                    }
                } else if token_lc == "loop-value" {
                    if let Some(v) = ctx.loop_value.as_ref() {
                        out.push_str(&v.to_string_lossy());
                        i = i + 1 + end + 1;
                        continue;
                    }
                } else if token_lc == "loop-player" {
                    if let Some(name) = ctx.loop_player_name() {
                        out.push_str(&name);
                        i = i + 1 + end + 1;
                        continue;
                    }
                }
                // Unknown token: keep as-is.
                let seg = &s[i..i + 1 + end + 1];
                out.push_str(seg);
                i = i + 1 + end + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }

    out
}

fn find_close_placeholder(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut j = start;
    let mut depth = 0usize;

    while j + 1 < bytes.len() {
        // Nested placeholder start: "%{"
        if bytes[j] == b'%' && bytes[j + 1] == b'{' {
            depth += 1;
            j += 2;
            continue;
        }

        // Close patterns
        if bytes[j] == b'}' && bytes[j + 1] == b'%' {
            if depth == 0 {
                return Some((j, 2)); // "}%" close
            }
            depth -= 1;
            j += 2;
            continue;
        }
        if bytes[j] == b'%' && bytes[j + 1] == b'}' {
            if depth == 0 {
                return Some((j, 2)); // "%}" close (after %player% expansion)
            }
            depth -= 1;
            j += 2;
            continue;
        }
        if j + 2 < bytes.len() && bytes[j] == b'%' && bytes[j + 1] == b'%' && bytes[j + 2] == b'}' {
            if depth == 0 {
                return Some((j, 3)); // "%%}" close (tolerate common typo)
            }
            depth = depth.saturating_sub(1);
            j += 3;
            continue;
        }

        j += 1;
    }
    None
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 {
        let first = t.as_bytes()[0];
        let last = t.as_bytes()[t.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return t[1..t.len() - 1].to_string();
        }
    }
    t.to_string()
}

fn eq_ci(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b)
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

fn parse_number(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

fn parse_timespan_to_seconds(s: &str) -> Option<f64> {
    let t = s.trim();
    let mut parts = t.split_whitespace();
    let num = parts.next()?.parse::<f64>().ok()?;
    let unit = parts.next()?.to_ascii_lowercase();
    let seconds = match unit.as_str() {
        "second" | "seconds" => num,
        "tick" | "ticks" => num / 20.0,
        "minute" | "minutes" => num * 60.0,
        "hour" | "hours" => num * 3600.0,
        _ => return None,
    };
    Some(seconds)
}
