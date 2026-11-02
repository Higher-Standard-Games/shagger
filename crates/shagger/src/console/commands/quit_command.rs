//! SHAG `src/engine/console/commands/QuitCommand.ts`. The browser reloads
//! the page; a native game closes.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;

pub struct QuitCommand<C> {
    def: CommandDef,
    on_quit: fn(&mut C),
}

impl<C> QuitCommand<C> {
    #[must_use]
    pub fn new(on_quit: fn(&mut C)) -> Self {
        Self {
            def: CommandDef {
                name: "quit",
                aliases: &["exit", "q"],
                description: "Quit the game",
                ..CommandDef::default()
            },
            on_quit,
        }
    }
}

impl<C> Command<C> for QuitCommand<C> {
    fn definition(&self) -> &CommandDef {
        &self.def
    }
    fn execute(&mut self, _: &[String], _: &mut CommandRegistry<C>, ctx: &mut C) -> CommandOutput {
        (self.on_quit)(ctx);
        "Quitting...".into()
    }
}
