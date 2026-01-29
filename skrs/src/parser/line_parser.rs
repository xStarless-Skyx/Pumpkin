// src/parser/line_parser.rs

use crate::parser::ast::{
    Block, CommandArg, CommandDef, FunctionDef, OptionEntry, Param, Script, Span, Stmt, Trigger,
    TriggerHeader, VarInit,
};

#[derive(Debug)]
pub enum LineParseError {
    IndentMismatch { line: usize },
    UnexpectedIndent { line: usize },
}

fn strip_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn indent_width(line: &str) -> usize {
    let mut n = 0;
    for ch in line.chars() {
        match ch {
            ' ' => n += 1,
            '\t' => n += 4,
            _ => break,
        }
    }
    n
}

fn parse_trigger_header(line: &str) -> Option<String> {
    if !line.trim_end().ends_with(':') {
        return None;
    }
    let core = line.trim_end().trim_end_matches(':').trim();
    let lower = core.to_ascii_lowercase();
    if lower.starts_with("on ") {
        let rest = core[3..].trim().to_string();
        if rest.is_empty() { None } else { Some(rest) }
    } else {
        None
    }
}

fn is_else_line(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("else:")
}

fn parse_if_header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.ends_with(':') {
        return None;
    }
    let core = t.trim_end_matches(':').trim();
    let lower = core.to_ascii_lowercase();
    if lower.starts_with("if ") {
        let cond = core[3..].trim().to_string();
        if cond.is_empty() { None } else { Some(cond) }
    } else {
        None
    }
}

fn parse_else_if_header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.ends_with(':') {
        return None;
    }
    let core = t.trim_end_matches(':').trim();
    let lower = core.to_ascii_lowercase();
    if lower.starts_with("else if ") {
        let cond = core[8..].trim().to_string(); // len("else if ") == 8
        if cond.is_empty() { None } else { Some(cond) }
    } else {
        None
    }
}

fn parse_loop_all_players_header(line: &str) -> bool {
    let t = line.trim();
    if !t.ends_with(':') {
        return false;
    }
    let core = t.trim_end_matches(':').trim();
    core.eq_ignore_ascii_case("loop all players")
}

/// Parses: "loop <count> times:"
fn parse_loop_times_header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.ends_with(':') {
        return None;
    }
    let core = t.trim_end_matches(':').trim();
    let lower = core.to_ascii_lowercase();

    if !lower.starts_with("loop ") {
        return None;
    }
    if !lower.ends_with(" times") {
        return None;
    }

    // remove "loop " (5 chars) and " times" (5 chars)
    let inner = core[5..core.len() - 5].trim();
    if inner.is_empty() { None } else { Some(inner.to_string()) }
}

/// Parses: "loop <list-expr>:"
fn parse_loop_list_header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.ends_with(':') {
        return None;
    }
    let core = t.trim_end_matches(':').trim();
    let lower = core.to_ascii_lowercase();
    if !lower.starts_with("loop ") {
        return None;
    }
    // Exclude "loop all players" and "loop <count> times"
    if parse_loop_all_players_header(line) || parse_loop_times_header(line).is_some() {
        return None;
    }
    let inner = core[5..].trim();
    if inner.is_empty() { None } else { Some(inner.to_string()) }
}

fn next_nonempty_noncomment_line(lines: &[String], mut i: usize) -> Option<(usize, String)> {
    while i < lines.len() {
        let l0 = strip_comment(&lines[i])
            .trim_end()
            .trim_end_matches('\r')
            .to_string();
        if l0.trim().is_empty() {
            i += 1;
            continue;
        }
        return Some((i, l0));
    }
    None
}

/// Parse an indented block.
/// - The block indent is determined by the first non-empty line after `start_index`.
/// - All statements in this block must have EXACTLY that indent.
/// - Deeper indent is only allowed when consumed by `if` or `loop` bodies.
fn parse_indented_block(
    lines: &[String],
    start_index: usize,
    parent_indent: usize,
) -> Result<(Block, usize), LineParseError> {
    let Some((first_i, first_line)) = next_nonempty_noncomment_line(lines, start_index) else {
        return Ok((Block { statements: vec![] }, start_index));
    };

    let block_indent = indent_width(&first_line);
    if block_indent <= parent_indent {
        // Indent didn't increase -> no block here
        return Ok((Block { statements: vec![] }, start_index));
    }

    let mut statements: Vec<Stmt> = Vec::new();
    let mut i = first_i;

    while i < lines.len() {
        let l0 = strip_comment(&lines[i]).trim_end().trim_end_matches('\r');
        if l0.trim().is_empty() {
            i += 1;
            continue;
        }

        let ind = indent_width(l0);

        if ind < block_indent {
            break; // block ends
        }
        if ind > block_indent {
            // deeper indent without a consumer statement
            return Err(LineParseError::IndentMismatch { line: i + 1 });
        }

        let trimmed = l0.trim();
        let span = Span { line: i + 1, col: ind + 1 };

        // -----------------------
        // if / else / else if
        // -----------------------
        if let Some(cond_raw) = parse_if_header(trimmed) {
            let (then_block, after_then) = parse_indented_block(lines, i + 1, block_indent)?;

            // parse optional else/else-if chain
            let (else_block, next_i) = parse_else_chain(lines, after_then, block_indent)?;

            statements.push(Stmt::If {
                condition_raw: cond_raw,
                then_block,
                else_block,
                span,
            });

            i = next_i;
            continue;
        }

        // -----------------------
        // loop all players:
        // -----------------------
        if parse_loop_all_players_header(trimmed) {
            let (body, next_i) = parse_indented_block(lines, i + 1, block_indent)?;
            statements.push(Stmt::LoopAllPlayers { body, span });
            i = next_i;
            continue;
        }

        // -----------------------
        // loop <count> times:
        // -----------------------
        if let Some(count_raw) = parse_loop_times_header(trimmed) {
            let (body, next_i) = parse_indented_block(lines, i + 1, block_indent)?;
            statements.push(Stmt::LoopTimes { count_raw, body, span });
            i = next_i;
            continue;
        }

        // -----------------------
        // loop <list-expr>:
        // -----------------------
        if let Some(list_raw) = parse_loop_list_header(trimmed) {
            let (body, next_i) = parse_indented_block(lines, i + 1, block_indent)?;
            statements.push(Stmt::LoopList { list_raw, body, span });
            i = next_i;
            continue;
        }

        // Normal raw statement
        statements.push(Stmt::Raw {
            raw: trimmed.to_string(),
            span,
        });
        i += 1;
    }

    Ok((Block { statements }, i))
}

/// Parses a chain after an if-then block:
/// - else:
/// - else if <cond>:
///
/// Returns (else_block, next_index_after_chain)
fn parse_else_chain(
    lines: &[String],
    start_index: usize,
    if_indent: usize,
) -> Result<(Option<Block>, usize), LineParseError> {
    let Some((k, k_line)) = next_nonempty_noncomment_line(lines, start_index) else {
        return Ok((None, start_index));
    };

    let k0 = strip_comment(&k_line).trim_end().trim_end_matches('\r');
    let k_ind = indent_width(k0);

    // must be at same indent as the if-line
    if k_ind != if_indent {
        return Ok((None, start_index));
    }

    // else:
    if is_else_line(k0.trim()) {
        let (eb, after_else) = parse_indented_block(lines, k + 1, if_indent)?;
        return Ok((Some(eb), after_else));
    }

    // else if <cond>:
    if let Some(cond) = parse_else_if_header(k0.trim()) {
        let (elif_then, after_elif_then) = parse_indented_block(lines, k + 1, if_indent)?;

        // recursively allow else / else-if after this else-if
        let (elif_else, after_chain) = parse_else_chain(lines, after_elif_then, if_indent)?;

        let nested_if = Stmt::If {
            condition_raw: cond,
            then_block: elif_then,
            else_block: elif_else,
            span: Span { line: k + 1, col: if_indent + 1 },
        };

        return Ok((Some(Block { statements: vec![nested_if] }), after_chain));
    }

    Ok((None, start_index))
}

pub fn parse_script(source: &str) -> Result<Script, LineParseError> {
    let mut options = Vec::new();
    let mut variables = Vec::new();
    let mut functions = Vec::new();
    let mut commands = Vec::new();
    let mut triggers = Vec::new();
    let mut i = 0usize;
    let lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();

    while i < lines.len() {
        let raw_line = strip_comment(&lines[i]).trim_end().trim_end_matches('\r');
        if raw_line.trim().is_empty() {
            i += 1;
            continue;
        }

        let indent = indent_width(raw_line);
        if indent != 0 {
            return Err(LineParseError::UnexpectedIndent { line: i + 1 });
        }

        let trimmed = raw_line.trim();
        if is_options_header(trimmed) {
            let (opts, next_i) = parse_options_block(&lines, i + 1, indent)?;
            options.extend(opts);
            i = next_i;
            continue;
        }

        if is_variables_header(trimmed) {
            let (vars, next_i) = parse_variables_block(&lines, i + 1, indent)?;
            variables.extend(vars);
            i = next_i;
            continue;
        }

        if let Some((func, next_i)) = parse_function_def(&lines, i, indent)? {
            functions.push(func);
            i = next_i;
            continue;
        }

        if let Some((cmd, next_i)) = parse_command_def(&lines, i, indent)? {
            commands.push(cmd);
            i = next_i;
            continue;
        }

        if let Some(header_raw) = parse_trigger_header(trimmed) {
            let span = Span { line: i + 1, col: 1 };

            let (body, next_i) = parse_indented_block(&lines, i + 1, 0)?;

            triggers.push(Trigger {
                header: TriggerHeader { raw: header_raw, event: None },
                body,
                span,
            });

            i = next_i;
        } else {
            i += 1; // ignore top-level non-trigger lines in MVP
        }
    }

    Ok(Script {
        options,
        variables,
        functions,
        commands,
        triggers,
    })
}

fn is_options_header(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("options:")
}

fn is_variables_header(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("variables:")
}

fn parse_options_block(
    lines: &[String],
    start_index: usize,
    parent_indent: usize,
) -> Result<(Vec<OptionEntry>, usize), LineParseError> {
    let Some((first_i, first_line)) = next_nonempty_noncomment_line(lines, start_index) else {
        return Ok((Vec::new(), start_index));
    };

    let block_indent = indent_width(&first_line);
    if block_indent <= parent_indent {
        return Ok((Vec::new(), start_index));
    }

    let mut out = Vec::new();
    let mut i = first_i;

    while i < lines.len() {
        let l0 = strip_comment(&lines[i]).trim_end().trim_end_matches('\r');
        if l0.trim().is_empty() {
            i += 1;
            continue;
        }

        let ind = indent_width(l0);
        if ind < block_indent {
            break;
        }
        if ind > block_indent {
            return Err(LineParseError::IndentMismatch { line: i + 1 });
        }

        let trimmed = l0.trim();
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_string();
            let val = v.trim().to_string();
            if !key.is_empty() {
                out.push(OptionEntry {
                    key,
                    value_raw: val,
                    span: Span { line: i + 1, col: ind + 1 },
                });
            }
        }

        i += 1;
    }

    Ok((out, i))
}

fn parse_variables_block(
    lines: &[String],
    start_index: usize,
    parent_indent: usize,
) -> Result<(Vec<VarInit>, usize), LineParseError> {
    let Some((first_i, first_line)) = next_nonempty_noncomment_line(lines, start_index) else {
        return Ok((Vec::new(), start_index));
    };

    let block_indent = indent_width(&first_line);
    if block_indent <= parent_indent {
        return Ok((Vec::new(), start_index));
    }

    let mut out = Vec::new();
    let mut i = first_i;

    while i < lines.len() {
        let l0 = strip_comment(&lines[i]).trim_end().trim_end_matches('\r');
        if l0.trim().is_empty() {
            i += 1;
            continue;
        }

        let ind = indent_width(l0);
        if ind < block_indent {
            break;
        }
        if ind > block_indent {
            return Err(LineParseError::IndentMismatch { line: i + 1 });
        }

        let trimmed = l0.trim();
        let kv = if let Some(kv) = trimmed.split_once('=') {
            Some(kv)
        } else {
            // Skript-style variables often use ":"; split on the last colon to avoid "::" in keys.
            trimmed.rsplit_once(':')
        };
        if let Some((k, v)) = kv {
            let key = k.trim().to_string();
            let val = v.trim().to_string();
            if !key.is_empty() {
                out.push(VarInit {
                    key_raw: key,
                    value_raw: val,
                    span: Span { line: i + 1, col: ind + 1 },
                });
            }
        }

        i += 1;
    }

    Ok((out, i))
}

fn parse_function_def(
    lines: &[String],
    line_index: usize,
    parent_indent: usize,
) -> Result<Option<(FunctionDef, usize)>, LineParseError> {
    let raw_line = strip_comment(&lines[line_index]).trim_end().trim_end_matches('\r');
    let indent = indent_width(raw_line);
    if indent != parent_indent {
        return Ok(None);
    }

    let trimmed = raw_line.trim();
    let Some((name, params, return_type)) = parse_function_header(trimmed) else {
        return Ok(None);
    };

    let span = Span { line: line_index + 1, col: indent + 1 };
    let (body, next_i) = parse_indented_block(lines, line_index + 1, indent)?;

    Ok(Some((
        FunctionDef {
            name,
            params,
            return_type,
            body,
            span,
        },
        next_i,
    )))
}

fn parse_function_header(line: &str) -> Option<(String, Vec<Param>, Option<String>)> {
    let t = line.trim();
    if !t.ends_with(':') {
        return None;
    }
    let core = t.trim_end_matches(':').trim();
    let lower = core.to_ascii_lowercase();
    if !lower.starts_with("function ") {
        return None;
    }
    let rest = core[9..].trim(); // len("function ") == 9
    let lparen = rest.find('(')?;
    let rparen = rest.rfind(')')?;
    if rparen < lparen {
        return None;
    }
    let name = rest[..lparen].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let params_str = rest[lparen + 1..rparen].trim();
    let mut params = Vec::new();
    if !params_str.is_empty() {
        for raw in params_str.split(',') {
            let p = raw.trim();
            if p.is_empty() {
                continue;
            }
            let (pname, pty) = match p.split_once(':') {
                Some((a, b)) => (a.trim().to_string(), Some(b.trim().to_string())),
                None => (p.to_string(), None),
            };
            if !pname.is_empty() {
                params.push(Param { name: pname, ty: pty });
            }
        }
    }

    let tail = rest[rparen + 1..].trim();
    let return_type = if let Some(rt) = tail.strip_prefix("::") {
        let ty = rt.trim();
        if ty.is_empty() { None } else { Some(ty.to_string()) }
    } else {
        None
    };

    Some((name, params, return_type))
}

fn parse_command_def(
    lines: &[String],
    line_index: usize,
    parent_indent: usize,
) -> Result<Option<(CommandDef, usize)>, LineParseError> {
    let raw_line = strip_comment(&lines[line_index]).trim_end().trim_end_matches('\r');
    let indent = indent_width(raw_line);
    if indent != parent_indent {
        return Ok(None);
    }

    let trimmed = raw_line.trim();
    let Some((name, args)) = parse_command_header(trimmed) else {
        return Ok(None);
    };

    let span = Span { line: line_index + 1, col: indent + 1 };
    let (aliases, permission, trigger, next_i) =
        parse_command_block(lines, line_index + 1, indent)?;

    Ok(Some((
        CommandDef {
            name,
            args,
            aliases,
            permission,
            trigger,
            span,
        },
        next_i,
    )))
}

fn parse_command_header(line: &str) -> Option<(String, Vec<CommandArg>)> {
    let t = line.trim();
    if !t.ends_with(':') {
        return None;
    }
    let core = t.trim_end_matches(':').trim();
    let lower = core.to_ascii_lowercase();
    if !lower.starts_with("command ") {
        return None;
    }
    let rest = core[8..].trim(); // len("command ") == 8
    let mut name: Option<String> = None;
    let mut remainder = rest.to_string();
    for (idx, part) in rest.split_whitespace().enumerate() {
        if part.starts_with('/') {
            name = Some(part.trim_start_matches('/').to_string());
            // compute remainder after this token
            let mut iter = rest.split_whitespace();
            for _ in 0..=idx {
                iter.next();
            }
            remainder = iter.collect::<Vec<_>>().join(" ");
            break;
        }
    }
    let name = name?;
    let args = parse_command_args(remainder.trim());
    Some((name, args))
}

fn parse_command_args(s: &str) -> Vec<CommandArg> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'<' {
            if let Some(end) = s[i + 2..].find(">]") {
                let inner = &s[i + 2..i + 2 + end];
                push_command_arg(inner, true, &mut out);
                i = i + 2 + end + 2;
                continue;
            }
        }
        if bytes[i] == b'<' {
            if let Some(end) = s[i + 1..].find('>') {
                let inner = &s[i + 1..i + 1 + end];
                push_command_arg(inner, false, &mut out);
                i = i + 1 + end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn push_command_arg(inner: &str, optional: bool, out: &mut Vec<CommandArg>) {
    let raw = inner.trim();
    if raw.is_empty() {
        return;
    }
    let (name, ty) = if let Some((a, b)) = raw.split_once(':') {
        (a.trim().to_string(), Some(b.trim().to_string()))
    } else {
        (raw.to_string(), Some(raw.to_string()))
    };
    out.push(CommandArg { name, ty, optional });
}

fn parse_command_block(
    lines: &[String],
    start_index: usize,
    parent_indent: usize,
) -> Result<(Vec<String>, Option<String>, Block, usize), LineParseError> {
    let Some((first_i, first_line)) = next_nonempty_noncomment_line(lines, start_index) else {
        return Ok((Vec::new(), None, Block { statements: vec![] }, start_index));
    };

    let block_indent = indent_width(&first_line);
    if block_indent <= parent_indent {
        return Ok((Vec::new(), None, Block { statements: vec![] }, start_index));
    }

    let mut aliases: Vec<String> = Vec::new();
    let mut permission: Option<String> = None;
    let mut trigger: Option<Block> = None;
    let mut i = first_i;

    while i < lines.len() {
        let l0 = strip_comment(&lines[i]).trim_end().trim_end_matches('\r');
        if l0.trim().is_empty() {
            i += 1;
            continue;
        }

        let ind = indent_width(l0);
        if ind < block_indent {
            break;
        }
        if ind > block_indent {
            return Err(LineParseError::IndentMismatch { line: i + 1 });
        }

        let trimmed = l0.trim();

        if let Some(rest) = trimmed.strip_prefix("aliases:") {
            for item in rest.split(',') {
                let mut s = item.trim().to_string();
                if s.is_empty() {
                    continue;
                }
                if s.starts_with('/') {
                    s = s.trim_start_matches('/').to_string();
                }
                aliases.push(s);
            }
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("permission:") {
            let p = rest.trim();
            if !p.is_empty() {
                permission = Some(p.to_string());
            }
            i += 1;
            continue;
        }

        if trimmed.eq_ignore_ascii_case("trigger:") {
            let (body, next_i) = parse_indented_block(lines, i + 1, block_indent)?;
            trigger = Some(body);
            i = next_i;
            continue;
        }

        i += 1;
    }

    Ok((aliases, permission, trigger.unwrap_or(Block { statements: vec![] }), i))
}
