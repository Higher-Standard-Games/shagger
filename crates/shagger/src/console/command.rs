//! SHAG `src/engine/console/Command.ts`.

use super::command_registry::CommandRegistry;

/// `ArgumentDef`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArgumentDef {
    pub name: &'static str,
    pub required: bool,
}

/// `SubcommandDef`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubcommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub args: Vec<ArgumentDef>,
}

/// `CommandDef`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: Option<&'static str>,
    pub aliases: &'static [&'static str],
    /// Only with `developer` set.
    pub developer: bool,
    pub subcommands: Vec<SubcommandDef>,
}

/// `CommandOutput`: text, nothing, or `fail(...)`. `Clear` is the
/// browser's `"__CLEAR__"` marker for the console to empty itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutput {
    None,
    Text(String),
    Fail(String),
    Clear,
}

impl From<String> for CommandOutput {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}
impl From<&str> for CommandOutput {
    fn from(s: &str) -> Self {
        Self::Text(s.to_owned())
    }
}

/// `Command`.
pub trait Command<C> {
    fn definition(&self) -> &CommandDef;

    /// `execute(args)`: the registry is there for commands that run or list
    /// others (`exec`, `help`).
    fn execute(&mut self, args: &[String], registry: &mut CommandRegistry<C>, ctx: &mut C) -> CommandOutput;

    /// `getCompletions`, beyond the subcommands.
    fn completions(&self, _input: &str, _args: &[String]) -> Option<Vec<String>> {
        None
    }

    /// `getHelp`.
    fn help(&self) -> String {
        let def = self.definition();
        let mut lines = vec![format!("{} - {}", def.name, def.description)];
        if let Some(usage) = def.usage {
            lines.push(format!("Usage: {usage}"));
        }
        if !def.aliases.is_empty() {
            lines.push(format!("Aliases: {}", def.aliases.join(", ")));
        }
        if !def.subcommands.is_empty() {
            lines.push("Subcommands:".into());
            for sub in &def.subcommands {
                let mut line = format!("  {}", sub.name);
                if !sub.args.is_empty() {
                    let args: Vec<String> = sub
                        .args
                        .iter()
                        .map(|a| {
                            if a.required {
                                format!("<{}>", a.name)
                            } else {
                                format!("[{}]", a.name)
                            }
                        })
                        .collect();
                    line += &format!(" {}", args.join(" "));
                }
                line += &format!(" - {}", sub.description);
                lines.push(line);
            }
        }
        lines.join("\n")
    }
}
