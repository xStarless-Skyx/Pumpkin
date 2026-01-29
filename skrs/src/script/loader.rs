// src/script/loader.rs

use std::fs;
use std::path::Path;

use super::script::{Script, ScriptSource};

use crate::parser::ast::{Block, Stmt};
use crate::parser::line_parser::{parse_script, LineParseError};
use crate::parser::event_parser::{attach_events, EventRegistry};
use crate::registry::effects::get_effect;
use crate::util::diagnostics::SkriptError;

/// Result of loading scripts from disk.
#[derive(Debug, Default)]
pub struct LoadScriptsResult {
    pub scripts: Vec<Script>,
    pub errors: Vec<SkriptError>,
}

/// Loads all enabled `.sk` files from `plugins/skrs/scripts/`.
/// Files whose *filename* starts with `-` are considered disabled and are ignored.
pub fn load_scripts() -> LoadScriptsResult {
    let dir = Path::new("plugins/skrs/scripts");

    // Make sure the folder exists
    if let Err(e) = fs::create_dir_all(dir) {
        log::error!("Failed to create scripts directory: {e}");
        return LoadScriptsResult::default();
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            log::error!("Failed to read scripts directory: {e}");
            return LoadScriptsResult::default();
        }
    };

    let mut out = LoadScriptsResult::default();

    for entry in entries.flatten() {
        let path = entry.path();

        // Only load .sk files (case-insensitive)
        let is_sk = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("sk"))
            .unwrap_or(false);

        if !is_sk {
            continue;
        }

        // Ignore disabled scripts: "-something.sk"
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if file_name.starts_with('-') {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();

        match fs::read_to_string(&path) {
            Ok(content) => match parse_script_file(&content, path_str.clone()) {
                Ok((script, mut errs)) => {
                    // If validation errors exist, keep errors and skip registering that script
                    if errs.is_empty() {
                        out.scripts.push(script);
                    }
                    out.errors.append(&mut errs);
                }
                Err(err) => {
                    // Parse failure (indent issues, etc.)
                    out.errors.push(make_parse_error(&content, &path_str, err));
                }
            },
            Err(e) => {
                log::error!("Failed to read script {:?}: {e}", path);
                out.errors.push(
                    SkriptError::new(
                        path_str.clone(),
                        1,
                        1,
                        "".to_string(),
                        format!("Failed to read file: {e}"),
                    )
                    .with_help("Check file permissions and that the file isn't locked."),
                );
            }
        }
    }

    out
}

/// Parses and validates a single `.sk` file.
fn parse_script_file(
    contents: &str,
    path: String,
) -> Result<(Script, Vec<SkriptError>), LineParseError> {
    // Parse into AST script
    let mut parsed = parse_script(contents)?;
    let reg = EventRegistry::new();
    attach_events(&mut parsed, &reg);

    // Wrap it into Script that also remembers the source
    let script = Script::from_ast(ScriptSource::File { path: path.clone() }, parsed);

    // Validate: catch unknown effects early (Skript-style reload errors)
    let errors = validate_unknown_effects(contents, &path, &script);

    Ok((script, errors))
}

fn validate_unknown_effects(contents: &str, path: &str, script: &Script) -> Vec<SkriptError> {
    let lines: Vec<&str> = contents.lines().collect();
    let mut out = Vec::new();

    for trig in &script.triggers {
        validate_block(&trig.body, &lines, path, script, &mut out);
    }

    for func in &script.functions {
        validate_block(&func.body, &lines, path, script, &mut out);
    }

    for cmd in &script.commands {
        validate_block(&cmd.trigger, &lines, path, script, &mut out);
    }

    out
}

fn validate_block(
    block: &Block,
    lines: &[&str],
    path: &str,
    script: &Script,
    out: &mut Vec<SkriptError>,
) {
    for stmt in &block.statements {
        match stmt {
            Stmt::Raw { raw, span } => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // First token is effect name
                let mut parts = trimmed.splitn(2, char::is_whitespace);
                let effect_name = match parts.next() {
                    Some(x) if !x.is_empty() => x,
                    _ => continue,
                };

                if get_effect(effect_name).is_none()
                    && !is_builtin_statement(effect_name)
                    && !is_function_call(trimmed, script)
                {
                    let line_idx = span.line.saturating_sub(1);
                    let line_text = lines.get(line_idx).copied().unwrap_or("");

                    out.push(
                        SkriptError::new(
                            path.to_string(),
                            span.line,
                            span.col,
                            line_text.to_string(),
                            format!("Unknown effect: {effect_name}"),
                        )
                        .with_help(
                            "This effect is not registered. Implement/register it (e.g. add/remove) or check spelling.",
                        ),
                    );
                }
            }

            Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                validate_block(then_block, lines, path, script, out);
                if let Some(eb) = else_block {
                    validate_block(eb, lines, path, script, out);
                }
            }

            Stmt::LoopAllPlayers { body, .. } => {
                validate_block(body, lines, path, script, out);
            }

            Stmt::LoopTimes { body, .. } => {
                validate_block(body, lines, path, script, out);
            }

            Stmt::LoopList { body, .. } => {
                validate_block(body, lines, path, script, out);
            }
        }
    }
}

fn is_builtin_statement(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "if" | "else" | "loop" | "return" | "stop" | "wait" | "make"
    )
}

fn is_function_call(line: &str, script: &Script) -> bool {
    if let Some((fname, _args)) = parse_function_call(line) {
        return script
            .functions
            .iter()
            .any(|f| f.name.eq_ignore_ascii_case(&fname));
    }
    false
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

fn make_parse_error(contents: &str, path: &str, e: LineParseError) -> SkriptError {
    let (line, msg) = match e {
        LineParseError::IndentMismatch { line } => (line, "Indent mismatch".to_string()),
        LineParseError::UnexpectedIndent { line } => (line, "Unexpected indent".to_string()),
    };

    let line_text = contents
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or("")
        .to_string();

    SkriptError::new(path.to_string(), line, 1, line_text, msg)
        .with_help("Check indentation. Blocks must be consistently indented under triggers/if statements.")
}
