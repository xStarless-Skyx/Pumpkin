// src/parser/ast.rs
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug)]
pub struct Script {
    pub options: Vec<OptionEntry>,
    pub variables: Vec<VarInit>,
    pub functions: Vec<FunctionDef>,
    pub commands: Vec<CommandDef>,
    pub triggers: Vec<Trigger>,
}

#[derive(Clone, Debug)]
pub struct Trigger {
    pub header: TriggerHeader,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TriggerHeader {
    /// The raw text after `on ` and before `:`
    pub raw: String,
    /// Parsed event match (filled by event_parser)
    pub event: Option<ParsedEvent>,
}

#[derive(Clone, Debug)]
pub struct ParsedEvent {
    pub event_name: String,
    pub args: Vec<String>, // later: Vec<Expr>
}

#[derive(Clone, Debug)]
pub struct Block {
    pub statements: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Raw {
        raw: String,
        span: Span,
    },
    If {
        condition_raw: String,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },

    /// loop all players:
    LoopAllPlayers {
        body: Block,
        span: Span,
    },

    /// loop <count> times:
    LoopTimes {
        count_raw: String,
        body: Block,
        span: Span,
    },

    /// loop <list-expr>:
    LoopList {
        list_raw: String,
        body: Block,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct OptionEntry {
    pub key: String,
    pub value_raw: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct VarInit {
    pub key_raw: String,
    pub value_raw: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Option<String>,
}

#[derive(Clone, Debug)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct CommandArg {
    pub name: String,
    pub ty: Option<String>,
    pub optional: bool,
}

#[derive(Clone, Debug)]
pub struct CommandDef {
    pub name: String,
    pub args: Vec<CommandArg>,
    pub aliases: Vec<String>,
    pub permission: Option<String>,
    pub trigger: Block,
    pub span: Span,
}

impl fmt::Display for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Script(triggers={})", self.triggers.len())
    }
}
