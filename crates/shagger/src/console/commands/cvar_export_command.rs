//! SHAG `src/engine/console/commands/CvarExportCommand.ts`. The browser
//! copies the config to the clipboard; here it prints it, to copy from the
//! console or keep as a `.cfg` for `exec`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;
use crate::core::cvar::with_cvars;

pub struct CvarExportCommand(CommandDef);

impl Default for CvarExportCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "cvarexport",
            description: "Export modified cvars as a config",
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for CvarExportCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, _: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        let config = with_cvars(|c| c.export_config());
        if config.trim().is_empty() {
            return "No modified cvars to export".into();
        }
        let count = config.lines().filter(|l| !l.trim().is_empty()).count();
        format!("{config}\nExported {count} cvars").into()
    }
}
