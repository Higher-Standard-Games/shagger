//! SHAG `src/engine/console/commands/VersionCommand.ts`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;

pub struct VersionCommand(CommandDef);

impl Default for VersionCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "version",
            aliases: &["ver"],
            description: "Display engine version",
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for VersionCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, _: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        format!("SHAGGER v{}", env!("CARGO_PKG_VERSION")).into()
    }
}
