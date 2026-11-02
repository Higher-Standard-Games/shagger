//! SHAG `src/engine/console/commands/EchoCommand.ts`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;

pub struct EchoCommand(CommandDef);

impl Default for EchoCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "echo",
            description: "Print text to console",
            usage: Some("echo <text>"),
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for EchoCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, args: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        CommandOutput::Text(args.join(" "))
    }
}
