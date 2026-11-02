//! SHAG `src/engine/console/CommandRegistry.ts`: commands by name and
//! alias, cvars as commands (`name` shows it, `name value` sets it), and
//! completion.

use std::collections::{BTreeMap, HashMap};

use super::command::{Command, CommandDef, CommandOutput};
use crate::core::cvar::{CvarValue, with_cvars};

/// `CommandExecResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandExecResult {
    pub ok: bool,
    pub output: CommandOutput,
}

/// `JSON.stringify(Cvar.get(name))`.
#[must_use]
pub fn json_value(value: &CvarValue) -> String {
    match value {
        CvarValue::String(s) => serde_json::Value::String(s.clone()).to_string(),
        other => other.to_string(),
    }
}

fn developer() -> bool {
    with_cvars(|c| c.get_bool("developer"))
}

/// `CommandRegistry`.
pub struct CommandRegistry<C> {
    commands: BTreeMap<String, Option<Box<dyn Command<C>>>>,
    definitions: BTreeMap<String, CommandDef>,
    /// alias → primary name.
    aliases: HashMap<String, String>,
}

impl<C> Default for CommandRegistry<C> {
    fn default() -> Self {
        Self {
            commands: BTreeMap::new(),
            definitions: BTreeMap::new(),
            aliases: HashMap::new(),
        }
    }
}

impl<C> CommandRegistry<C> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `register(command)`.
    pub fn register(&mut self, command: Box<dyn Command<C>>) {
        let def = command.definition().clone();
        let key = def.name.to_lowercase();
        for alias in def.aliases {
            self.aliases.insert(alias.to_lowercase(), key.clone());
        }
        self.definitions.insert(key.clone(), def);
        self.commands.insert(key, Some(command));
    }

    /// `unregister`.
    pub fn unregister(&mut self, name: &str) {
        let key = name.to_lowercase();
        if let Some(def) = self.definitions.remove(&key) {
            for alias in def.aliases {
                self.aliases.remove(&alias.to_lowercase());
            }
        }
        self.commands.remove(&key);
    }

    fn primary(&self, name: &str) -> String {
        let key = name.to_lowercase();
        self.aliases.get(&key).cloned().unwrap_or(key)
    }

    /// `execute`: the output only.
    pub fn execute(&mut self, input: &str, ctx: &mut C) -> CommandOutput {
        self.execute_checked(input, ctx).output
    }

    /// `executeChecked`.
    pub fn execute_checked(&mut self, input: &str, ctx: &mut C) -> CommandExecResult {
        let ok = |output| CommandExecResult { ok: true, output };
        let parts = parse_input(input.trim());
        let Some((name, args)) = parts.split_first() else {
            return ok(CommandOutput::None);
        };
        let key = self.primary(name);
        if let Some(def) = self.definitions.get(&key) {
            if def.developer && !developer() {
                return CommandExecResult {
                    ok: false,
                    output: CommandOutput::Fail(format!("Command '{name}' requires developer mode")),
                };
            }
            // Out of the map while it runs, so it can use the registry.
            let Some(mut command) = self.commands.get_mut(&key).and_then(Option::take) else {
                return CommandExecResult {
                    ok: false,
                    output: CommandOutput::Fail(format!("Command '{name}' is already running")),
                };
            };
            let output = command.execute(args, self, ctx);
            if let Some(slot) = self.commands.get_mut(&key) {
                *slot = Some(command);
            }
            return normalize(output);
        }
        if with_cvars(|c| c.exists(&key)) {
            return normalize(handle_cvar(&key, args));
        }
        CommandExecResult {
            ok: false,
            output: CommandOutput::Fail(format!("Unknown command: {name}")),
        }
    }

    /// `getCompletions`.
    #[must_use]
    pub fn completions(&self, input: &str) -> Vec<String> {
        let lower = input.trim().to_lowercase();
        let parts = parse_input(&lower);
        let dev = developer();
        let mut out = Vec::new();
        if parts.len() <= 1 {
            let prefix = parts.first().map_or("", String::as_str);
            for (name, def) in &self.definitions {
                if (!def.developer || dev) && name.starts_with(prefix) {
                    out.push(name.clone());
                }
            }
            for (alias, primary) in &self.aliases {
                let hidden = self.definitions.get(primary).is_some_and(|d| d.developer && !dev);
                if !hidden && alias.starts_with(prefix) {
                    out.push(alias.clone());
                }
            }
            out.extend(
                with_cvars(|c| c.get_names())
                    .into_iter()
                    .filter(|n| n.starts_with(prefix)),
            );
        } else if let Some(name) = parts.first() {
            let key = self.primary(name);
            if let Some(Some(command)) = self.commands.get(&key)
                && let Some(custom) = command.completions(input, &parts[1..])
            {
                return custom;
            }
            if let Some(def) = self.definitions.get(&key)
                && parts.len() == 2
            {
                for sub in &def.subcommands {
                    if sub.name.starts_with(parts[1].as_str()) {
                        out.push(sub.name.to_owned());
                    }
                }
            }
        }
        out.sort();
        out
    }

    /// `getAllDefinitions`, without the developer's when it is off.
    #[must_use]
    pub fn definitions(&self) -> Vec<&CommandDef> {
        let dev = developer();
        self.definitions.values().filter(|d| !d.developer || dev).collect()
    }

    /// `getDefinition`.
    #[must_use]
    pub fn definition(&self, name: &str) -> Option<&CommandDef> {
        self.definitions.get(&self.primary(name))
    }

    /// `isCvar`.
    #[must_use]
    pub fn is_cvar(&self, name: &str) -> bool {
        let key = name.to_lowercase();
        !self.definitions.contains_key(&key) && with_cvars(|c| c.exists(&key))
    }

    /// `getHelp`.
    #[must_use]
    pub fn help(&self, name: &str) -> Option<String> {
        let key = self.primary(name);
        if let Some(Some(command)) = self.commands.get(&key) {
            return Some(command.help());
        }
        with_cvars(|c| {
            c.get_cvar(&key).map(|cvar| {
                let description = if cvar.description.is_empty() {
                    "Console variable"
                } else {
                    &cvar.description
                };
                format!("{key} - {description}\nCurrent value: {}", cvar.value)
            })
        })
    }
}

fn normalize(output: CommandOutput) -> CommandExecResult {
    CommandExecResult {
        ok: !matches!(output, CommandOutput::Fail(_)),
        output,
    }
}

/// `handleCvar`.
fn handle_cvar(name: &str, args: &[String]) -> CommandOutput {
    with_cvars(|c| {
        if args.is_empty() {
            return CommandOutput::Text(c.format_info(name));
        }
        let value = args.join(" ");
        if c.set(name, value) {
            let shown = c.get(name).map(json_value).unwrap_or_default();
            CommandOutput::Text(format!("{name} = {shown}"))
        } else {
            CommandOutput::Fail(format!("Failed to set {name}"))
        }
    })
}

/// `parseInput`: split on spaces, keeping quoted runs (the quotes dropped).
#[must_use]
pub fn parse_input(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in input.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == ' ' && !in_quotes {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::commands::register_engine_commands;
    use crate::core::cvar::{CvarOptions, init_cvars};

    #[derive(Default)]
    struct Game {
        quit: bool,
    }

    fn setup() -> CommandRegistry<Game> {
        init_cvars(None, false);
        with_cvars(|c| {
            c.register("cl_fov", 90, CvarOptions::default().range(80.0, 110.0));
            c.register("r_bloom", true, CvarOptions::default());
            c.register("name", "Washiki", CvarOptions::default());
        });
        let mut registry = CommandRegistry::new();
        register_engine_commands(&mut registry, None, |g: &mut Game| g.quit = true);
        registry
    }

    #[test]
    fn cvars_answer_as_commands() {
        let mut r = setup();
        let mut g = Game::default();
        assert_eq!(
            r.execute("cl_fov 100", &mut g),
            CommandOutput::Text("cl_fov = 100".into())
        );
        assert_eq!(
            r.execute("cl_fov", &mut g),
            CommandOutput::Text("\"cl_fov\" is \"100\" (default: \"90\") [80..110]".into())
        );
        assert_eq!(
            r.execute("set name \"Ramen Cat\"", &mut g),
            CommandOutput::Text("name = \"Ramen Cat\"".into())
        );
        assert_eq!(
            r.execute("toggle r_bloom", &mut g),
            CommandOutput::Text("r_bloom = false".into())
        );
        let bad = r.execute_checked("cl_fov abc", &mut g);
        assert!(!bad.ok);
        assert!(!r.execute_checked("nope", &mut g).ok);
        assert_eq!(r.execute("cls", &mut g), CommandOutput::Clear);
        r.execute("q", &mut g);
        assert!(g.quit);
    }

    #[test]
    fn completion_and_help() {
        let r = setup();
        assert_eq!(r.completions("cl_f"), vec!["cl_fov".to_owned()]);
        assert!(r.completions("re").contains(&"resetall".to_owned()));
        let mut r = r;
        let mut g = Game::default();
        let CommandOutput::Text(help) = r.execute("help", &mut g) else {
            panic!()
        };
        assert!(help.contains("▸ Cvars\n  cvarexport"));
        let CommandOutput::Text(one) = r.execute("help cl_fov", &mut g) else {
            panic!()
        };
        assert!(
            one.starts_with("cl_fov - Console variable\nType: int\nValue: 90"),
            "{one}"
        );
        assert_eq!(parse_input("say \"hello there\"  x"), vec!["say", "hello there", "x"]);
    }

    #[test]
    fn exec_runs_a_config_file() {
        let mut r = setup();
        let dir = std::env::temp_dir().join(format!("shagger-exec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("autoexec.cfg"), "// mine\ncl_fov 105\nset name Soba\n").unwrap();
        r.register(Box::new(crate::console::commands::exec_command::ExecCommand::new(
            Some(dir.clone()),
        )));
        let mut g = Game::default();
        assert_eq!(
            r.execute("exec autoexec", &mut g),
            CommandOutput::Text("Executed autoexec.cfg: 2 commands".into())
        );
        assert_eq!(with_cvars(|c| c.get_number("cl_fov")), 105.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
