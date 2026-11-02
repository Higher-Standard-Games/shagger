//! SHAG `src/engine/console/commands/ResetAllCommand.ts`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;
use crate::core::cvar::with_cvars;

pub struct ResetAllCommand(CommandDef);

impl Default for ResetAllCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "resetall",
            description: "Reset all cvars to default values",
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for ResetAllCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, _: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        with_cvars(|c| c.reset_all());
        "All cvars reset to default values".into()
    }
}
