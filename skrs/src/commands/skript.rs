// src/commands/skript.rs

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
};
use futures::executor::block_on;

use pumpkin::{
    command::{
        args::{Arg, ConsumedArgs},
        dispatcher::CommandError,
        tree::builder::{argument, literal},
        tree::CommandTree,
        CommandExecutor,
        CommandSender,
    },
    plugin::Context,
    server::Server,
};

use pumpkin_util::{
    PermissionLvl,
    permission::{Permission, PermissionDefault},
    text::TextComponent,
};

#[derive(Copy, Clone)]
struct ServerPtr(*const pumpkin::server::Server);

// Server lives for the process lifetime; safe for deferred registration.
unsafe impl Send for ServerPtr {}
unsafe impl Sync for ServerPtr {}

impl ServerPtr {
    unsafe fn as_ref(&self) -> &Server {
        unsafe { &*self.0 }
    }
}

use crate::{
    commands::args::AskServerStringArgConsumer,
    commands::script_commands::register_script_commands,
    registry::{functions as func_registry, options as options_registry},
    script::manager::SharedScriptManager,
    script::script::Script,
    util::diagnostics::send_error,
};
use crate::runtime::context::SkriptEvent;
use crate::runtime::expr::eval_message_expr;
use crate::runtime::value::Value;

const DESCRIPTION: &str = "Manage skrs scripts (enable/disable/reload/list).";
const PLUGIN_NS: &str = "skrs";
const PERM_KEY: &str = "skript";

fn full_perm_node() -> String {
    format!("{PLUGIN_NS}:{PERM_KEY}")
}

/// Align with PermissionDefault::Op(PermissionLvl::Two)
fn is_op(sender: &CommandSender) -> bool {
    sender.is_console() || sender.permission_lvl() >= PermissionLvl::Two
}

fn parse_var_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() >= 3 && t.starts_with('{') && t.ends_with('}') {
        Some(t[1..t.len() - 1].trim().to_string())
    } else {
        None
    }
}


async fn apply_loaded_scripts(server: &Server, scripts: &[Script]) -> Vec<crate::parser::ast::CommandDef> {
    options_registry::clear_options();
    func_registry::clear_functions();

    let mut all_commands = Vec::new();

    for script in scripts {
        // options
        {
            let mut ctx = crate::runtime::context::ExecutionContext::new(
                server,
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
                server,
                SkriptEvent::Command { player: None },
            );
            let mut list_inits: HashMap<String, Vec<Value>> = HashMap::new();
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
    }

    all_commands
}

fn schedule_script_command_registration(
    server: &Server,
    commands: Vec<crate::parser::ast::CommandDef>,
) {
    if commands.is_empty() {
        return;
    }
    let server_ptr = ServerPtr(server as *const Server);
    std::thread::spawn(move || {
        let mut attempts = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(25));
            let server = unsafe { server_ptr.as_ref() };
            let result = block_on(register_script_commands(server, commands.clone()));
            match result {
                Ok(()) => break,
                Err(e) => {
                    attempts += 1;
                    if attempts >= 10 {
                        log::error!("Failed to register script commands: {e}");
                        break;
                    }
                }
            }
        }
    });
}

/// Wrapper executor that blocks non-OP senders (optional)
struct OpOnly<E> {
    inner: E,
}

impl<E: CommandExecutor> CommandExecutor for OpOnly<E> {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a HashMap<&'a str, Arg<'a>>,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandError>> + Send + 'a>> {
        Box::pin(async move {
            if !is_op(sender) {
                sender
                    .send_message(TextComponent::text(
                        "§cYou do not have permission to use this command. (OP only)",
                    ))
                    .await;
                return Ok(());
            }

            self.inner.execute(sender, server, args).await
        })
    }
}

/// Extract `<script>` from consumed args.
///
/// In your Pumpkin version, “string-like” args come through as:
/// - Arg::Msg(String)  (common for message / greedy string consumers)
/// - Arg::Simple(&str) (common for word-like consumers)
fn get_script_arg(args: &ConsumedArgs<'_>) -> Option<String> {
    match args.get("script")? {
        Arg::Msg(s) => Some(s.clone()),
        Arg::Simple(s) => Some((*s).to_string()),
        Arg::Item(s) => Some((*s).to_string()),
        Arg::ResourceLocation(s) => Some((*s).to_string()),
        _ => None,
    }
}

/// ---------------------------
/// Executors
/// ---------------------------

struct BaseExec {
    scripts: SharedScriptManager,
}

impl CommandExecutor for BaseExec {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandError>> + Send + 'a>> {
        let scripts = self.scripts.clone();
        Box::pin(async move {
            let msg = {
                let s = scripts.read().await;
                let loaded = s.list().into_iter().filter(|x| x.loaded).count();
                format!("skrs: {loaded} scripts loaded. Try /skript list.")
            };
            sender.send_message(TextComponent::text(msg)).await;
            Ok(())
        })
    }
}

struct ListExec {
    scripts: SharedScriptManager,
}

impl CommandExecutor for ListExec {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        _server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandError>> + Send + 'a>> {
        let scripts = self.scripts.clone();
        Box::pin(async move {
            let lines: Vec<String> = {
                let mut s = scripts.write().await;
                let found = s.sync_from_disk().unwrap_or(0);

                let mut list = s.list();
                list.sort_by(|a, b| a.name.cmp(&b.name));

                if list.is_empty() {
                    vec![format!(
                        "No scripts found on disk. (dir: {:?}, found={found})",
                        s.scripts_dir()
                    )]
                } else {
                    let mut out = Vec::with_capacity(list.len() + 1);
                    out.push(format!(
                        "Scripts (found on disk: {found}, dir: {:?}):",
                        s.scripts_dir()
                    ));

                    for sc in list {
                        let status = if sc.disabled {
                            "disabled"
                        } else if sc.loaded {
                            "loaded"
                        } else {
                            "unloaded"
                        };

                        let extra = sc.error.as_deref().unwrap_or("");
                        if extra.is_empty() {
                            out.push(format!("- {} ({})", sc.name, status));
                        } else {
                            out.push(format!("- {} ({}) ERROR: {}", sc.name, status, extra));
                        }
                    }

                    out
                }
            };

            for line in lines {
                sender.send_message(TextComponent::text(line)).await;
            }
            Ok(())
        })
    }
}

struct ReloadAllExec {
    scripts: SharedScriptManager,
}

impl CommandExecutor for ReloadAllExec {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        _args: &'a ConsumedArgs<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandError>> + Send + 'a>> {
        let scripts = self.scripts.clone();
        Box::pin(async move {
            let report = {
                let mut s = scripts.write().await;
                s.reload_all()
            };

            match report {
                Ok(report) => {
                    let all_commands = apply_loaded_scripts(server, &report.scripts).await;
                    schedule_script_command_registration(server, all_commands);
                    let err_count = report.errors.len();
                    if err_count == 0 {
                        sender
                            .send_message(TextComponent::text(format!(
                                "§aReloaded {} trigger(s). (disabled scripts ignored)",
                                report.trigger_count
                            )))
                            .await;
                    } else {
                        sender
                            .send_message(TextComponent::text(format!(
                                "§cReloaded {} trigger(s) with {} error(s):",
                                report.trigger_count, err_count
                            )))
                            .await;
                        for err in &report.errors {
                            send_error(sender, err).await;
                        }
                    }
                }
                Err(e) => {
                    sender
                        .send_message(TextComponent::text(format!("§cReload failed: {e}")))
                        .await;
                }
            }

            Ok(())
        })
    }
}
struct DisableExec {
    scripts: SharedScriptManager,
}

impl CommandExecutor for DisableExec {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandError>> + Send + 'a>> {
        let scripts = self.scripts.clone();
        Box::pin(async move {
            let Some(name) = get_script_arg(args) else {
                sender
                    .send_message(TextComponent::text("Usage: /skript disable <script>"))
                    .await;
                return Ok(());
            };

            let msg = {
                let mut s = scripts.write().await;
                match s.disable(&name) {
                    Ok(()) => match s.reload_all() {
                        Ok(r) => {
                            let all_commands = apply_loaded_scripts(server, &r.scripts).await;
                            schedule_script_command_registration(server, all_commands);
                            format!(
                                "Disabled {} and reloaded {} trigger(s).",
                                name.trim(),
                                r.trigger_count
                            )
                        }
                        Err(e) => format!("Disabled {} but reload failed: {e}", name.trim()),
                    },
                    Err(e) => format!("Disable failed: {e}"),
                }
            };

            sender.send_message(TextComponent::text(msg)).await;
            Ok(())
        })
    }
}

struct EnableExec {
    scripts: SharedScriptManager,
}

impl CommandExecutor for EnableExec {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a ConsumedArgs<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandError>> + Send + 'a>> {
        let scripts = self.scripts.clone();
        Box::pin(async move {
            let Some(name) = get_script_arg(args) else {
                sender
                    .send_message(TextComponent::text("Usage: /skript enable <script>"))
                    .await;
                return Ok(());
            };

            let msg = {
                let mut s = scripts.write().await;
                match s.enable(&name) {
                    Ok(()) => match s.reload_all() {
                        Ok(r) => {
                            let all_commands = apply_loaded_scripts(server, &r.scripts).await;
                            schedule_script_command_registration(server, all_commands);
                            format!(
                                "Enabled {} and reloaded {} trigger(s).",
                                name.trim(),
                                r.trigger_count
                            )
                        }
                        Err(e) => format!("Enabled {} but reload failed: {e}", name.trim()),
                    },
                    Err(e) => format!("Enable failed: {e}"),
                }
            };

            sender.send_message(TextComponent::text(msg)).await;
            Ok(())
        })
    }
}

/// ---------------------------
/// Tree
/// ---------------------------

pub fn build_skript_command_tree(scripts: SharedScriptManager) -> CommandTree {
    CommandTree::new(vec!["skript", "sk"], DESCRIPTION)
        .execute(OpOnly {
            inner: BaseExec {
                scripts: scripts.clone(),
            },
        })
        .then(literal("list").execute(OpOnly {
            inner: ListExec {
                scripts: scripts.clone(),
            },
        }))
        .then(literal("reload").execute(OpOnly {
            inner: ReloadAllExec {
                scripts: scripts.clone(),
            },
        }))
        .then(
            literal("disable").then(
                argument("script", AskServerStringArgConsumer).execute(OpOnly {
                    inner: DisableExec {
                        scripts: scripts.clone(),
                    },
                }),
            ),
        )
        .then(
            literal("enable").then(
                argument("script", AskServerStringArgConsumer).execute(OpOnly {
                    inner: EnableExec { scripts },
                }),
            ),
        )
}

/// ---------------------------
/// Registration
/// ---------------------------

pub async fn register_skript_commands(
    context: &Context,
    scripts: SharedScriptManager,
) -> Result<(), String> {
    let node = full_perm_node();

    context
        .register_permission(Permission {
            node: node.clone(),
            description: "Allows using /skript (/sk)".to_string(),
            default: PermissionDefault::Op(PermissionLvl::Two),
            children: HashMap::new(),
        })
        .await?;

    let tree = build_skript_command_tree(scripts);
    context.register_command(tree, node).await;

    Ok(())
}
