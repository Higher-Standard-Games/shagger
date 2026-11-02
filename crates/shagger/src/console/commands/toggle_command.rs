//! SHAG `src/engine/console/commands/ToggleCommand.ts`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;
use crate::core::cvar::{CvarType, with_cvars};

pub struct ToggleCommand(CommandDef);

impl Default for ToggleCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "toggle",
            description: "Toggle a boolean cvar",
            usage: Some("toggle <cvar>"),
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for ToggleCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, args: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        let Some(name) = args.first() else {
            return "Usage: toggle <cvar>".into();
        };
        with_cvars(|c| {
            if !c.exists(name) {
                return format!("Unknown cvar: {name}").into();
            }
            if c.toggle(name) {
                format!("{name} = {}", c.get(name).map(ToString::to_string).unwrap_or_default()).into()
            } else {
                format!("Failed to toggle {name}").into()
            }
        })
    }
    fn completions(&self, _: &str, args: &[String]) -> Option<Vec<String>> {
        Some(if args.is_empty() {
            with_cvars(|c| {
                c.get_names()
                    .into_iter()
                    .filter(|n| c.get_cvar(n).is_some_and(|v| v.kind == CvarType::Bool))
                    .collect()
            })
        } else {
            Vec::new()
        })
    }
}
