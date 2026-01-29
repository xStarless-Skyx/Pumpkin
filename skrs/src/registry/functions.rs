use std::sync::Arc;

use dashmap::DashMap;
use once_cell::sync::Lazy;

use crate::parser::ast::FunctionDef;

static FUNCTIONS: Lazy<DashMap<String, Arc<FunctionDef>>> = Lazy::new(DashMap::new);

pub fn register_function(func: FunctionDef) {
    FUNCTIONS.insert(func.name.to_ascii_lowercase(), Arc::new(func));
}

pub fn get_function(name: &str) -> Option<Arc<FunctionDef>> {
    FUNCTIONS.get(&name.to_ascii_lowercase()).map(|f| f.value().clone())
}

pub fn clear_functions() {
    FUNCTIONS.clear();
}

pub fn register() {}
