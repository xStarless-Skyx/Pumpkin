// src/runtime/value.rs

// Re-export the canonical Value type so the rest of runtime can import from here.
pub use crate::registry::expressions::Value;

impl Value {
    /// Lossy string conversion for messages/chat/interpolation.
    /// - Null => "" (empty)
    /// - Numbers => no trailing .0 for integers, empty if NaN/Inf
    pub fn to_string_lossy(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Number(n) => {
                if !n.is_finite() {
                    return String::new();
                }
                if n.fract().abs() < 1e-9 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::List(items) => items
                .iter()
                .map(|v| v.to_string_lossy())
                .collect::<Vec<_>>()
                .join(", "),
            Value::Null => String::new(),
        }
    }
}
