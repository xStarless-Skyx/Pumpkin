// src/registry/effects.rs

use once_cell::sync::Lazy;
use dashmap::DashMap;

use crate::runtime::context::ExecutionContext;

pub type EffectFn = fn(&str, &mut ExecutionContext<'_>);

pub static EFFECTS: Lazy<DashMap<&'static str, EffectFn>> = Lazy::new(DashMap::new);

pub fn register_effect(name: &'static str, func: EffectFn) {
    EFFECTS.insert(name, func);
}

pub fn get_effect(name: &str) -> Option<EffectFn> {
    EFFECTS.get(name).map(|e| *e)
}

pub fn register_all_effects() {
    crate::effects::send::register();
    crate::effects::set::register();
    crate::effects::broadcast::register();
    crate::effects::replace::register();

    crate::effects::add::register();
    crate::effects::increase::register();
    crate::effects::remove::register();
    crate::effects::reset::register();
}
