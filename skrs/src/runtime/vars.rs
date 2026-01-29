use once_cell::sync::Lazy;
use dashmap::DashMap;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::Path;

use crate::runtime::value::Value;

pub static GLOBAL_VARS: Lazy<DashMap<String, Value>> = Lazy::new(DashMap::new);

pub fn get_global(key: &str) -> Option<Value> {
    GLOBAL_VARS.get(key).map(|v| v.value().clone())
}

pub fn set_global(key: &str, value: Value) {
    GLOBAL_VARS.insert(key.to_string(), value);
}

pub fn del_global(key: &str) {
    GLOBAL_VARS.remove(key);
}

pub fn has_global(key: &str) -> bool {
    GLOBAL_VARS.contains_key(key)
}

pub fn save_to_csv(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return Err(format!("failed to create vars dir: {e}"));
        }
    }

    let mut out = String::new();
    out.push_str("key,value\n");

    for entry in GLOBAL_VARS.iter() {
        let key = entry.key();
        let val = entry.value();
        let json = value_to_json(val);
        let json_str = serde_json::to_string(&json).unwrap_or_else(|_| "null".to_string());
        out.push_str(&format!("{},{}\n", csv_escape(key), csv_escape(&json_str)));
    }

    fs::write(path, out).map_err(|e| format!("failed to write vars csv: {e}"))
}

pub fn load_from_csv(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if !path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(path)
        .map_err(|e| format!("failed to read vars csv: {e}"))?;

    GLOBAL_VARS.clear();

    for (idx, line) in content.lines().enumerate() {
        if idx == 0 && line.trim_start().to_ascii_lowercase().starts_with("key,") {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let cols = parse_csv_line(line);
        if cols.len() < 2 {
            continue;
        }
        let key = cols[0].clone();
        let json_str = cols[1].clone();
        let val = match serde_json::from_str::<JsonValue>(&json_str) {
            Ok(j) => json_to_value(j),
            Err(_) => Value::String(json_str),
        };
        set_global(&key, val);
    }

    Ok(())
}

fn value_to_json(v: &Value) -> JsonValue {
    match v {
        Value::String(s) => JsonValue::String(s.clone()),
        Value::Number(n) => JsonValue::Number(serde_json::Number::from_f64(*n).unwrap_or_else(|| serde_json::Number::from(0))),
        Value::Bool(b) => JsonValue::Bool(*b),
        Value::List(items) => JsonValue::Array(items.iter().map(value_to_json).collect()),
        Value::Null => JsonValue::Null,
    }
}

fn json_to_value(v: JsonValue) -> Value {
    match v {
        JsonValue::String(s) => Value::String(s),
        JsonValue::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        JsonValue::Bool(b) => Value::Bool(b),
        JsonValue::Array(items) => Value::List(items.into_iter().map(json_to_value).collect()),
        JsonValue::Null => Value::Null,
        other => Value::String(other.to_string()),
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                out.push(cur);
                cur = String::new();
            }
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}
