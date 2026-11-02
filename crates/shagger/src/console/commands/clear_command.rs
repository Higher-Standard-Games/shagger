//! SHAG `src/engine/console/commands/ClearCommand.ts`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;

pub struct ClearCommand(CommandDef);

impl Default for ClearCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "clear",
            aliases: &["cls"],
            description: "Clear console output",
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for ClearCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, _: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        CommandOutput::Clear
    }
}
