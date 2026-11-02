//! SHAG `src/engine/console/commands/CvarListCommand.ts`.

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::{CommandRegistry, json_value};
use crate::core::cvar::with_cvars;

pub struct CvarListCommand(CommandDef);

impl Default for CvarListCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "cvarlist",
            description: "List all console variables",
            usage: Some("cvarlist [filter]"),
            ..CommandDef::default()
        })
    }
}

impl<C> Command<C> for CvarListCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, args: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        let filter = args.first().map(String::as_str);
        with_cvars(|c| {
            let cvars = c.list(filter);
            if cvars.is_empty() {
                return match filter {
                    Some(f) => format!("No cvars matching \"{f}\"").into(),
                    None => "No cvars registered".into(),
                };
            }
            let mut lines = vec![format!(
                "{} cvar{}{}:",
                cvars.len(),
                if cvars.len() == 1 { "" } else { "s" },
                filter.map(|f| format!(" matching \"{f}\"")).unwrap_or_default()
            )];
            for cvar in cvars {
                let mark = if cvar.value == cvar.default_value { ' ' } else { '*' };
                lines.push(format!("{mark} {} = {}", cvar.name, json_value(&cvar.value)));
            }
            lines.join("\n").into()
        })
    }
    fn completions(&self, _: &str, args: &[String]) -> Option<Vec<String>> {
        Some(if args.is_empty() {
            ["r_", "s_", "g_", "ui_", "cam_", "con_", "m_"]
                .map(str::to_owned)
                .to_vec()
        } else {
            Vec::new()
        })
    }
}
