//! SHAG `src/engine/console/commands/SetCommand.ts`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::{CommandRegistry, json_value};
use crate::core::cvar::with_cvars;

pub struct SetCommand(CommandDef);

impl Default for SetCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "set",
            description: "Set a cvar value",
            usage: Some("set <cvar> <value>"),
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for SetCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, args: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        if args.len() < 2 {
            return "Usage: set <cvar> <value>".into();
        }
        let name = &args[0];
        let value = args[1..].join(" ");
        with_cvars(|c| {
            if !c.exists(name) {
                return format!("Unknown cvar: {name}").into();
            }
            if c.set(name, value) {
                format!("{name} = {}", c.get(name).map(json_value).unwrap_or_default()).into()
            } else {
                format!("Failed to set {name}").into()
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
