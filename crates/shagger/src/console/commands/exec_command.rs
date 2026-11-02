//! SHAG `src/engine/console/commands/ExecCommand.ts`: runs a `.cfg` file
//! line by line. The browser fetches it from the game's folder; here it is
//! read from the config directory.

use std::path::PathBuf;

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;

pub struct ExecCommand {
    def: CommandDef,
    dir: Option<PathBuf>,
}

impl ExecCommand {
    #[must_use]
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            def: CommandDef {
                name: "exec",
                description: "Execute a config file",
                usage: Some("exec <filename>"),
                ..CommandDef::default()
            },
            dir,
        }
    }
}

impl<C> Command<C> for ExecCommand {
    fn definition(&self) -> &CommandDef {
        &self.def
    }
    fn execute(&mut self, args: &[String], registry: &mut CommandRegistry<C>, ctx: &mut C) -> CommandOutput {
        let Some(file) = args.first() else {
            return "Usage: exec <filename>".into();
        };
        let filename = if file.ends_with(".cfg") {
            file.clone()
        } else {
            format!("{file}.cfg")
        };
        let path = self
            .dir
            .as_ref()
            .map_or_else(|| PathBuf::from(&filename), |d| d.join(&filename));
        let Ok(content) = std::fs::read_to_string(&path) else {
            return format!("Config not found: {filename}").into();
        };
        let mut executed = 0;
        for line in content.split('\n') {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }
            registry.execute(trimmed, ctx);
            executed += 1;
        }
        format!("Executed {filename}: {executed} commands").into()
    }
}
