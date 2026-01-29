use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;

use pumpkin::command::tree::builder::argument;
use pumpkin::command::{CommandExecutor, CommandSender};
use pumpkin::server::Server;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionLvl};

use crate::commands::args::AskServerStringArgConsumer;
use crate::parser::ast::{Block, CommandArg, CommandDef, Stmt};
use crate::runtime::context::{ExecutionContext, SkriptEvent};
use crate::runtime::executor::execute_block;
use crate::runtime::value::Value;

static REGISTERED_COMMANDS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub async fn register_script_commands(
    server: &Server,
    commands: Vec<CommandDef>,
) -> Result<(), String> {
    // Unregister previously registered script commands (prevents stale/duplicate entries).
    let old = {
        let mut guard = REGISTERED_COMMANDS.lock().unwrap();
        std::mem::take(&mut *guard)
    };
    if !old.is_empty() {
        let mut attempts = 0;
        loop {
            if let Ok(mut dispatcher) = server.command_dispatcher.try_write() {
                for name in &old {
                    dispatcher.unregister(name);
                }
                break;
            }
            attempts += 1;
            if attempts >= 5 {
                log::warn!("script command unregister skipped (dispatcher busy)");
                break;
            }
            // small spin/yield before retrying (no tokio runtime needed)
            std::thread::yield_now();
        }
    }

    for cmd in commands {
        let name = cmd.name.clone();
        register_one_command_with_retry(server, cmd).await?;
        REGISTERED_COMMANDS.lock().unwrap().push(name);
    }
    Ok(())
}

async fn register_one_command_with_retry(server: &Server, cmd: CommandDef) -> Result<(), String> {
    let mut attempts = 0;
    loop {
        match register_one_command(server, cmd.clone()).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                attempts += 1;
                if attempts >= 5 {
                    return Err(e);
                }
                std::thread::yield_now();
            }
        }
    }
}

async fn register_one_command(server: &Server, cmd: CommandDef) -> Result<(), String> {
    let cmd_arc = Arc::new(cmd);
    let mut names = vec![cmd_arc.name.clone()];
    names.extend(cmd_arc.aliases.iter().cloned());

    let executor = ScriptCommandExecutor {
        cmd: Arc::clone(&cmd_arc),
        server_ptr: ServerPtr(server as *const Server),
    };

    let mut tree = pumpkin::command::tree::CommandTree::new(names, "Skript command");

    if cmd_arc.args.is_empty() {
        tree = tree.execute(executor);
    } else {
        let mut current: Option<pumpkin::command::tree::builder::NonLeafNodeBuilder> = None;
        for (idx, arg) in cmd_arc.args.iter().enumerate() {
            let node = argument(arg_name(idx), consumer_for(arg));
            if let Some(cur) = current {
                let cur = if arg.optional {
                    cur.execute(ScriptCommandExecutor {
                        cmd: Arc::clone(&cmd_arc),
                        server_ptr: ServerPtr(server as *const Server),
                    })
                } else {
                    cur
                };
                current = Some(cur.then(node));
            } else {
                if arg.optional {
                    tree = tree.execute(ScriptCommandExecutor {
                        cmd: Arc::clone(&cmd_arc),
                        server_ptr: ServerPtr(server as *const Server),
                    });
                }
                current = Some(node);
            }
        }
        if let Some(cur) = current {
            tree = tree.then(cur.execute(ScriptCommandExecutor {
                cmd: Arc::clone(&cmd_arc),
                server_ptr: ServerPtr(server as *const Server),
            }));
        }
    }

    // Register permission for the command
    let perm_node = command_permission_node(&cmd_arc);
    let mut attempts = 0;
    loop {
        if let Ok(mut perm_registry) = server.permission_registry.try_write() {
            if !perm_registry.has_permission(&perm_node) {
                // If no permission is specified, allow everyone by default.
                let default = match cmd_arc.permission.as_deref() {
                    None => PermissionDefault::Allow,
                    Some(p) if p.eq_ignore_ascii_case("op") => {
                        PermissionDefault::Op(PermissionLvl::Two)
                    }
                    _ => PermissionDefault::Op(PermissionLvl::Two),
                };
                let perm = Permission::new(&perm_node, "Skript command permission", default);
                let _ = perm_registry.register_permission(perm);
            }
            break;
        }
        attempts += 1;
        if attempts >= 5 {
            return Err("permission registry busy".to_string());
        }
        std::thread::yield_now();
    }

    let mut attempts = 0;
    loop {
        if let Ok(mut dispatcher) = server.command_dispatcher.try_write() {
            dispatcher.register(tree, perm_node);
            return Ok(());
        }
        attempts += 1;
        if attempts >= 5 {
            return Err("dispatcher busy".to_string());
        }
        std::thread::yield_now();
    }
}


fn command_permission_node(cmd: &CommandDef) -> String {
    if let Some(p) = &cmd.permission {
        if p.eq_ignore_ascii_case("op") {
            return "skript:command.op".to_string();
        }
        return p.clone();
    }
    format!("skript:command.{}", cmd.name)
}

fn arg_name(idx: usize) -> String {
    format!("arg{}", idx + 1)
}

fn consumer_for(arg: &CommandArg) -> impl pumpkin::command::args::ArgumentConsumer + use<> {
    let _ = arg;
    AskServerStringArgConsumer
}

struct ScriptCommandExecutor {
    cmd: Arc<CommandDef>,
    server_ptr: ServerPtr,
}

#[derive(Copy, Clone)]
struct ServerPtr(*const Server);

// Server lives for the process lifetime; safe for deferred execution.
unsafe impl Send for ServerPtr {}
unsafe impl Sync for ServerPtr {}

impl ServerPtr {
    unsafe fn as_ref(&self) -> &Server {
        unsafe { &*self.0 }
    }
}

impl CommandExecutor for ScriptCommandExecutor {
    fn execute<'a>(
        &'a self,
        sender: &'a CommandSender,
        server: &'a Server,
        args: &'a HashMap<&'a str, pumpkin::command::args::Arg<'a>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), pumpkin::command::dispatcher::CommandError>> + Send + 'a>>
    {
        let cmd = Arc::clone(&self.cmd);
        let server_ptr = self.server_ptr;
        Box::pin(async move {
            let player = sender.as_player();
            let mut arg_map: HashMap<String, Value> = HashMap::new();
            for (idx, _) in cmd.args.iter().enumerate() {
                let key = arg_name(idx);
                if let Some(arg) = args.get(key.as_str()) {
                    let v = arg_to_value(arg);
                    arg_map.insert(format!("arg-{}", idx + 1).to_ascii_lowercase(), v);
                }
            }

            if block_contains_wait(&cmd.trigger) {
                std::thread::spawn(move || {
                    let server = unsafe { server_ptr.as_ref() };
                    let _ = futures::executor::block_on(async move {
                        let mut ctx =
                            ExecutionContext::new(server, SkriptEvent::Command { player })
                                .with_args(arg_map);
                        let _ = execute_block(&cmd.trigger, &mut ctx).await;
                        let outbox = ctx.drain_outbox();
                        let event_player = ctx.event_player();
                        crate::runtime::dispatcher::flush_outbox(server, outbox, event_player)
                            .await;
                    });
                });
            } else {
                let mut ctx = ExecutionContext::new(server, SkriptEvent::Command { player })
                    .with_args(arg_map);
                let _ = execute_block(&cmd.trigger, &mut ctx).await;
                let outbox = ctx.drain_outbox();
                let event_player = ctx.event_player();
                crate::runtime::dispatcher::flush_outbox(server, outbox, event_player).await;
            }

            Ok(())
        })
    }
}

fn block_contains_wait(block: &Block) -> bool {
    for stmt in &block.statements {
        match stmt {
            Stmt::Raw { raw, .. } => {
                let trimmed = raw.trim();
                if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("wait") {
                    return true;
                }
            }
            Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                if block_contains_wait(then_block) {
                    return true;
                }
                if let Some(eb) = else_block {
                    if block_contains_wait(eb) {
                        return true;
                    }
                }
            }
            Stmt::LoopAllPlayers { body, .. }
            | Stmt::LoopTimes { body, .. }
            | Stmt::LoopList { body, .. } => {
                if block_contains_wait(body) {
                    return true;
                }
            }
        }
    }
    false
}

fn arg_to_value(arg: &pumpkin::command::args::Arg<'_>) -> Value {
    match arg {
        pumpkin::command::args::Arg::Msg(s) => Value::String(s.clone()),
        pumpkin::command::args::Arg::Simple(s) => Value::String((*s).to_string()),
        pumpkin::command::args::Arg::Num(Ok(n)) => Value::Number(n.to_f64()),
        _ => Value::Null,
    }
}

trait NumberToF64 {
    fn to_f64(&self) -> f64;
}

impl NumberToF64 for pumpkin::command::args::bounded_num::Number {
    fn to_f64(&self) -> f64 {
        match self {
            pumpkin::command::args::bounded_num::Number::F64(v) => *v,
            pumpkin::command::args::bounded_num::Number::F32(v) => f64::from(*v),
            pumpkin::command::args::bounded_num::Number::I32(v) => f64::from(*v),
            pumpkin::command::args::bounded_num::Number::I64(v) => *v as f64,
        }
    }
}
