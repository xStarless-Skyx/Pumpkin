use dashmap::DashMap;
use once_cell::sync::Lazy;

static OPTIONS: Lazy<DashMap<String, String>> = Lazy::new(DashMap::new);

pub fn set_option(key: &str, value: String) {
    OPTIONS.insert(key.to_string(), value);
}

pub fn get_option(key: &str) -> Option<String> {
    OPTIONS.get(key).map(|v| v.value().clone())
}

pub fn clear_options() {
    OPTIONS.clear();
}

pub fn register() {}
