// src/lib.rs

use std::sync::Arc;

use pumpkin::plugin::Context;
use pumpkin_api_macros::{plugin_impl, plugin_method};

// Use Tokio RwLock for SharedScriptManager
use tokio::sync::RwLock;

pub mod script;
pub mod parser;
pub mod runtime;
pub mod registry;
pub mod effects;
pub mod commands;
pub mod util;

mod plugin;

pub const PLUGIN_NAME: &str = "skrs";

use crate::script::manager::{ScriptManager, SharedScriptManager};

#[plugin_method]
async fn on_load(&mut self, context: Arc<Context>) -> Result<(), String> {
    crate::plugin::on_load_impl(self, context).await
}

#[plugin_method]
async fn on_unload(&mut self, context: Arc<Context>) -> Result<(), String> {
    crate::plugin::on_unload_impl(self, context).await
}

#[plugin_impl]
pub struct Plugin {
    pub scripts: SharedScriptManager,
}

impl Plugin {
    pub fn new() -> Self {
        Self {
            scripts: Arc::new(RwLock::new(ScriptManager::new())),
        }
    }
}

impl Default for Plugin {
    fn default() -> Self {
        Self::new()
    }
}
