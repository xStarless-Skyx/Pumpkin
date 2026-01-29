// src/script/script.rs

use crate::parser::ast;

/// A loaded `.sk` file + its parsed AST.
#[derive(Debug, Clone)]
pub struct Script {
    /// Where the script came from (useful for errors/reload logging).
    pub source: ScriptSource,

    /// Options declared in the script.
    pub options: Vec<ast::OptionEntry>,

    /// Variable initializers declared in the script.
    pub variables: Vec<ast::VarInit>,

    /// Function definitions.
    pub functions: Vec<ast::FunctionDef>,

    /// Command definitions.
    pub commands: Vec<ast::CommandDef>,

    /// Parsed triggers (event blocks) in AST form.
    pub triggers: Vec<ast::Trigger>,
}

/// Where a script came from.
#[derive(Debug, Clone)]
pub enum ScriptSource {
    /// Loaded from a file on disk.
    File { path: String },

    /// Loaded from an in-memory string (useful for tests later).
    Inline { name: String },
}

impl Script {
    pub fn new(source: ScriptSource) -> Self {
        Self {
            source,
            options: Vec::new(),
            variables: Vec::new(),
            functions: Vec::new(),
            commands: Vec::new(),
            triggers: Vec::new(),
        }
    }

    /// Convenience for turning a parsed AST Script into a loaded Script with source metadata.
    pub fn from_ast(source: ScriptSource, parsed: ast::Script) -> Self {
        Self {
            source,
            options: parsed.options,
            variables: parsed.variables,
            functions: parsed.functions,
            commands: parsed.commands,
            triggers: parsed.triggers,
        }
    }
}
