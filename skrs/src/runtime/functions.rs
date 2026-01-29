use std::collections::HashMap;
use std::sync::Arc;

use crate::parser::ast::FunctionDef;
use crate::runtime::context::ExecutionContext;
use crate::runtime::executor::{execute_block, ExecFlow};
use crate::runtime::expr::eval_message_expr;
use crate::runtime::value::Value;

pub async fn call_function(
    func: Arc<FunctionDef>,
    arg_exprs: Vec<String>,
    ctx: &mut ExecutionContext<'_>,
) -> Value {
    let mut locals: HashMap<String, Value> = HashMap::new();
    for (idx, param) in func.params.iter().enumerate() {
        let val = arg_exprs
            .get(idx)
            .map(|s| eval_message_expr(s, ctx))
            .unwrap_or(Value::Null);
        locals.insert(param.name.clone(), val);
    }

    let old_locals = std::mem::replace(&mut ctx.locals, locals);
    let old_loop_player = ctx.loop_player.clone();
    let old_loop_value = ctx.loop_value.clone();
    let old_loop_index = ctx.loop_index;

    let ret = match execute_block(&func.body, ctx).await {
        ExecFlow::Return(v) => v,
        _ => Value::Null,
    };

    ctx.locals = old_locals;
    ctx.loop_player = old_loop_player;
    ctx.loop_value = old_loop_value;
    ctx.loop_index = old_loop_index;

    ret
}

pub fn call_function_blocking(
    func: Arc<FunctionDef>,
    arg_exprs: Vec<String>,
    ctx: &mut ExecutionContext<'_>,
) -> Value {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(call_function(func, arg_exprs, ctx))),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build runtime for function call");
            rt.block_on(call_function(func, arg_exprs, ctx))
        }
    }
}
