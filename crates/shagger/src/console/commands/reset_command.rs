//! SHAG `src/engine/console/commands/ResetCommand.ts`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;
use crate::core::cvar::with_cvars;

pub struct ResetCommand(CommandDef);

impl Default for ResetCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "reset",
            description: "Reset a cvar to its default value",
            usage: Some("reset <cvar>"),
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for ResetCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, args: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        let Some(name) = args.first() else {
            return "Usage: reset <cvar>".into();
        };
        with_cvars(|c| {
            if !c.exists(name) {
                return format!("Unknown cvar: {name}").into();
            }
            if c.reset(name) {
                // A preloaded value is dropped; `get` is then undefined.
                let shown = c.get(name).map_or_else(|| "undefined".to_owned(), ToString::to_string);
                format!("{name} reset to {shown}").into()
            } else {
                format!("Failed to reset {name}").into()
            }
        })
    }
    fn completions(&self, _: &str, args: &[String]) -> Option<Vec<String>> {
        Some(if args.is_empty() {
            with_cvars(|c| c.get_names())
        } else {
            Vec::new()
        })
    }
}
