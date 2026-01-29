// src/util/diagnostics.rs

use pumpkin::command::CommandSender;
use pumpkin_util::text::TextComponent;

/// A user-facing diagnostic that can be shown in chat.
#[derive(Clone, Debug)]
pub struct SkriptError {
    pub file: String,
    pub line: usize, // 1-based
    pub col: usize,  // 1-based (best-effort)
    pub line_text: String,
    pub message: String,
    pub help: Option<String>,
}

impl SkriptError {
    pub fn new(file: impl Into<String>, line: usize, col: usize, line_text: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            line,
            col,
            line_text: line_text.into(),
            message: message.into(),
            help: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

/// Send a Skript-style diagnostic to the given sender.
pub async fn send_error(sender: &CommandSender, err: &SkriptError) {
    sender
        .send_message(TextComponent::text(format!(
            "§c[skrs] Error in {} line {}:",
            std::path::Path::new(&err.file)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&err.file),
            err.line
        )))
        .await;

    sender
        .send_message(TextComponent::text(format!("§c{}", err.message)))
        .await;

    let line = err.line_text.trim_end_matches(['\r', '\n']);
    sender
        .send_message(TextComponent::text(format!("§7  {}", line)))
        .await;

    let col = err.col.max(1);
    let caret_pos = col.saturating_sub(1).min(line.chars().count());
    let mut caret = String::new();
    caret.push_str("§7  ");
    caret.push_str(&" ".repeat(caret_pos));
    caret.push_str("§c^");
    sender.send_message(TextComponent::text(caret)).await;

    if let Some(help) = &err.help {
        sender
            .send_message(TextComponent::text(format!("§eHelp: {}", help)))
            .await;
    }
}
