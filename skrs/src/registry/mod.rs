// src/registry/mod.rs

pub mod variables;
pub mod types;
pub mod expressions;
pub mod conditions;
pub mod effects;
pub mod events;
pub mod options;
pub mod sections;
pub mod structures;
pub mod functions;

pub fn register_all() {
    variables::register();
    types::register();
    options::register();

    expressions::register_all_expressions();
    conditions::register();

    effects::register_all_effects();
    events::register();
    
    sections::register();
    structures::register();
    functions::register();
}
