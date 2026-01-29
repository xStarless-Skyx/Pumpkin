// src/registry/expressions.rs

use std::sync::Arc;

use dashmap::DashMap;
use once_cell::sync::Lazy;

use crate::runtime::context::ExecutionContext;

#[derive(Clone, Debug)]
pub enum Value {
    String(String),
    Number(f64),
    Bool(bool),
    List(Vec<Value>),
    Null,
}

impl Value {
    pub fn as_string_lossy(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::List(items) => items
                .iter()
                .map(|v| v.as_string_lossy())
                .collect::<Vec<_>>()
                .join(", "),
            Value::Null => "null".to_string(),
        }
    }
}

pub trait Expression: Send + Sync {
    fn eval(&self, ctx: &mut ExecutionContext<'_>) -> Value;
}

// A factory tries to parse `input` and, if it matches, returns an Expression.
type ExprFactory = Arc<dyn Fn(&str) -> Option<Arc<dyn Expression>> + Send + Sync>;

static EXPRESSION_FACTORIES: Lazy<DashMap<&'static str, ExprFactory>> =
    Lazy::new(DashMap::new);

pub fn register_expression(name: &'static str, factory: ExprFactory) {
    EXPRESSION_FACTORIES.insert(name, factory);
}

pub fn try_parse_expression(input: &str) -> Option<Arc<dyn Expression>> {
    // MVP: try all factories (slow but OK for now)
    for f in EXPRESSION_FACTORIES.iter() {
        if let Some(expr) = (f.value())(input) {
            return Some(expr);
        }
    }
    None
}

/// Call once at startup from registry::register_all()
pub fn register_all_expressions() {
    register_literal_string();
    register_literal_number();
}

/// -------------------------
/// Built-in expressions (MVP)
/// -------------------------

fn register_literal_string() {
    register_expression(
        "literal_string",
        Arc::new(|input: &str| {
            let s = input.trim();
            if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
                let inner = &s[1..s.len() - 1];
                Some(Arc::new(LiteralString(inner.to_string())) as Arc<dyn Expression>)
            } else {
                None
            }
        }),
    );
}

struct LiteralString(String);

impl Expression for LiteralString {
    fn eval(&self, _ctx: &mut ExecutionContext<'_>) -> Value {
        Value::String(self.0.clone())
    }
}

fn register_literal_number() {
    register_expression(
        "literal_number",
        Arc::new(|input: &str| {
            let s = input.trim();
            match s.parse::<f64>() {
                Ok(n) => Some(Arc::new(LiteralNumber(n)) as Arc<dyn Expression>),
                Err(_) => None,
            }
        }),
    );
}

struct LiteralNumber(f64);

impl Expression for LiteralNumber {
    fn eval(&self, _ctx: &mut ExecutionContext<'_>) -> Value {
        Value::Number(self.0)
    }
}
