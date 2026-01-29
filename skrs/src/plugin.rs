// src/plugin.rs

use std::sync::Arc;

use pumpkin::plugin::Context;

use crate::commands::skript::register_skript_commands;
use crate::commands::tempban::register_tempban_command;
use crate::registry::events::register_triggers;
use crate::runtime::dispatcher::register_event_listeners;
use crate::runtime::dispatcher::{flush_outbox, run_triggers};
use crate::script::loader::load_scripts;
use crate::commands::script_commands::register_script_commands;
use crate::registry::{functions as func_registry, options as options_registry};
use crate::runtime::context::SkriptEvent;
use crate::runtime::expr::eval_message_expr;
use crate::runtime::value::Value;
use crate::{PLUGIN_NAME, Plugin};

const VARS_CSV_PATH: &str = "plugins/skrs/variables.csv";

pub async fn on_load_impl(plugin: &mut Plugin, context: Arc<Context>) -> Result<(), String> {
    context.init_log();

    eprintln!("*** {PLUGIN_NAME} on_load_impl() reached ***");
    log::info!("{PLUGIN_NAME} loading...");
    crate::registry::effects::register_all_effects();

    crate::registry::register_all();

    if let Err(e) = crate::runtime::vars::load_from_csv(VARS_CSV_PATH) {
        log::warn!("Failed to load variables from {VARS_CSV_PATH}: {e}");
    }

    // ✅ registers permission + registers /skript and /sk (aliases)
    register_skript_commands(&context, plugin.scripts.clone()).await?;
    log::info!("Registered commands: /skript and /sk");
    register_tempban_command(context.clone()).await?;
    log::info!("Registered command: /tempban");

    // load scripts from disk
    let result = load_scripts();

    log::info!(
        "Loaded {} script(s) ({} error(s))",
        result.scripts.len(),
        result.errors.len()
    );

    // clear options/functions and re-register from scripts
    options_registry::clear_options();
    func_registry::clear_functions();

    // collect triggers from all loaded scripts
    let mut all_triggers = Vec::new();
    let mut all_commands = Vec::new();

    for script in &result.scripts {
        // options
        {
            let mut ctx = crate::runtime::context::ExecutionContext::new(
                context.server.as_ref(),
                SkriptEvent::Command { player: None },
            );
            for opt in &script.options {
                let val = eval_message_expr(&opt.value_raw, &mut ctx).to_string_lossy();
                options_registry::set_option(&opt.key, val);
            }
        }

        // variables (best-effort)
        {
            let mut ctx = crate::runtime::context::ExecutionContext::new(
                context.server.as_ref(),
                SkriptEvent::Command { player: None },
            );
            let mut list_inits: std::collections::HashMap<String, Vec<Value>> =
                std::collections::HashMap::new();
            for var in &script.variables {
                if let Some(key) = parse_var_token(&var.key_raw) {
                    if key.contains('%') {
                        continue;
                    }
                    let value = eval_message_expr(&var.value_raw, &mut ctx);
                    if key.ends_with("::*") {
                        if crate::runtime::vars::has_global(&key) {
                            continue;
                        }
                        list_inits.entry(key).or_default().push(value);
                    } else if let Some(rest) = key.strip_prefix('_') {
                        ctx.locals.insert(rest.to_string(), value);
                    } else if !crate::runtime::vars::has_global(&key) {
                        crate::runtime::vars::set_global(&key, value);
                    }
                }
            }
            for (key, items) in list_inits {
                if !crate::runtime::vars::has_global(&key) {
                    crate::runtime::vars::set_global(&key, Value::List(items));
                }
            }
        }

        // functions
        for func in &script.functions {
            func_registry::register_function(func.clone());
        }

        // commands
        all_commands.extend(script.commands.clone());

        all_triggers.extend(script.triggers.clone());
    }

    log::info!("Registered {} trigger(s)", all_triggers.len());
    register_triggers(all_triggers);

    // fire "on load" triggers
    {
        let (outbox, event_player) =
            run_triggers("load", context.server.as_ref(), SkriptEvent::Command { player: None })
                .await;
        flush_outbox(context.server.as_ref(), outbox, event_player).await;
    }

    // register script commands
    if let Err(e) = register_script_commands(context.server.as_ref(), all_commands).await {
        log::error!("Failed to register script commands: {e}");
    }

    // log any load errors
    for e in &result.errors {
        log::error!("Script error: {}:{}:{}", e.file, e.line, e.message);
    }

    // register pumpkin event listeners
    register_event_listeners(context.clone()).await;

    log::info!("{PLUGIN_NAME} loaded.");
    Ok(())
}

pub async fn on_unload_impl(_plugin: &mut Plugin, _context: Arc<Context>) -> Result<(), String> {
    log::info!("{PLUGIN_NAME} unloading...");
    if let Err(e) = crate::runtime::vars::save_to_csv(VARS_CSV_PATH) {
        log::warn!("Failed to save variables to {VARS_CSV_PATH}: {e}");
    }
    Ok(())
}

fn parse_var_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() >= 3 && t.starts_with('{') && t.ends_with('}') {
        Some(t[1..t.len() - 1].trim().to_string())
    } else {
        None
    }
}
