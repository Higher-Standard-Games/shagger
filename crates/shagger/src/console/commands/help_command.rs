//! SHAG `src/engine/console/commands/HelpCommand.ts`: the commands grouped,
//! or one command or cvar in detail. The browser marks names up as HTML for
//! its hover help; the console here prints plain text.

use std::collections::BTreeMap;

use crate::console::command::{Command, CommandDef, CommandOutput};
use crate::console::command_registry::CommandRegistry;
use crate::core::cvar::with_cvars;

/// `CATEGORY_BY_NAME`.
const CATEGORIES: [(&str, &[&str]); 4] = [
    ("System", &["help", "clear", "version", "con_size", "console_debug"]),
    (
        "Cvars",
        &["cvarlist", "toggle", "reset", "resetall", "set", "cvarexport"],
    ),
    (
        "Debug",
        &[
            "debug",
            "camera",
            "map",
            "weather",
            "wind",
            "audio",
            "time",
            "export",
            "shader",
            "bsp",
            "pvs",
            "fog",
            "pk3",
            "mem",
            "scene",
            "ui",
            "hud",
            "show_fps",
            "freefly_speed",
            "freefly_collision",
        ],
    ),
    (
        "Game",
        &[
            "game", "plant", "quest", "spawn", "give", "save", "load", "teleport", "unlock", "custom",
        ],
    ),
];
const CATEGORY_ORDER: [&str; 5] = ["System", "Cvars", "Game", "Debug", "Other"];

fn category_of(name: &str) -> &'static str {
    CATEGORIES
        .iter()
        .find(|(_, names)| names.contains(&name))
        .map_or("Other", |(c, _)| c)
}

pub struct HelpCommand(CommandDef);

impl Default for HelpCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "help",
            aliases: &["?", "commands"],
            description: "Display help for commands",
            usage: Some("help [command]"),
            ..CommandDef::default()
        })
    }
}

fn describe_one<C>(registry: &CommandRegistry<C>, name: &str) -> String {
    if registry.is_cvar(name)
        && let Some(text) = with_cvars(|c| {
            c.get_cvar(name).map(|cvar| {
                let description = if cvar.description.is_empty() {
                    "Console variable"
                } else {
                    &cvar.description
                };
                let kind = format!("{:?}", cvar.kind).to_lowercase();
                let mut help = format!("{} - {description}\nType: {kind}\nValue: {}", cvar.name, cvar.value);
                if cvar.value != cvar.default_value {
                    help += &format!(" (default: {})", cvar.default_value);
                }
                help += "\n";
                if cvar.min.is_some() || cvar.max.is_some() {
                    let n = |v: Option<f64>, inf: &str| {
                        v.map_or_else(|| inf.to_owned(), crate::shared::js::js_number_to_string)
                    };
                    help += &format!("Range: {} to {}\n", n(cvar.min, "-∞"), n(cvar.max, "∞"));
                }
                help += &format!("Usage: {name} [value]");
                help
            })
        })
    {
        return text;
    }
    let Some(def) = registry.definition(name) else {
        return format!("Unknown command: {name}");
    };
    let mut help = format!("{} - {}\n", def.name, def.description);
    if let Some(usage) = def.usage {
        help += &format!("Usage: {usage}\n");
    }
    if !def.subcommands.is_empty() {
        help += "Subcommands:\n";
        for sub in &def.subcommands {
            help += &format!("  {:<12} {}\n", sub.name, sub.description);
        }
    }
    help.trim().to_owned()
}

fn list_commands<C>(registry: &CommandRegistry<C>) -> String {
    let mut categories: BTreeMap<&str, Vec<&CommandDef>> = BTreeMap::new();
    let mut defs = registry.definitions();
    defs.sort_by(|a, b| a.name.cmp(b.name));
    for def in defs {
        let name = def.name.to_lowercase();
        if registry.is_cvar(&name) {
            continue;
        }
        categories.entry(category_of(&name)).or_default().push(def);
    }
    let rule = "═══════════════════════════════════════";
    let mut help = format!("{rule}\n           CONSOLE COMMANDS\n{rule}\n\n");
    for category in CATEGORY_ORDER {
        let Some(cmds) = categories.get(category) else { continue };
        help += &format!("▸ {category}\n");
        for def in cmds {
            help += &format!("  {:<14} {}\n", def.name, def.description);
        }
        help += "\n";
    }
    help += "▸ Console Variables\n";
    help += "  Use cvarlist to see all cvars (r_*, s_*, g_*)\n";
    help += "  Use help <cvar> for cvar details\n\n";
    help += "───────────────────────────────────────\n";
    help += "Type help <command> for detailed info.\n";
    help += "Use Tab for autocomplete.";
    help
}

impl<C> Command<C> for HelpCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(&mut self, args: &[String], registry: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        match args.first() {
            None => list_commands(registry).into(),
            Some(name) => describe_one(registry, &name.to_lowercase()).into(),
        }
    }
}
