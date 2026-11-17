# Cvars and console

> Behaviour: SHAG's [Cvar system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/cvar-system.md) and [Console system](https://github.com/Higher-Standard-Games/shag/blob/main/docs/manual/guides/console-system.md). The validation, clamping, listing and `exportConfig`/`execConfig` rules are the same.

Source: `crates/shagger/src/core/cvar.rs`, `crates/shagger/src/console/`, `crates/shagger/src/debug/debug_console.rs`.

## The registry

SHAG keeps one static `Cvar` registry per page. SHAGGER keeps one per thread, behind `with_cvars`; the game thread is the one that matters. `Cvars` is also a plain value, for tests and tools that want their own.

```rust
use shagger::core::cvar::{CvarOptions, init_cvars, with_cvars};

// Cvar.init(): load what the file holds. `true` keeps `developer` at 0,
// as a production client does.
init_cvars(Some(config_dir.join("cvars.json")), true);

with_cvars(|c| {
    c.register("g_example", 1.0, CvarOptions::describe("Example game setting").range(0.0, 4.0));
    c.set("g_example", 2.5);
    let value = c.get_number("g_example");
});
```

Do not call `with_cvars` again inside its closure: the registry is borrowed for the closure's length.

### What differs from SHAG

- **Storage is a file.** `Cvars::new(storage, ...)` loads and saves a JSON file instead of `localStorage["engine_cvars"]`. It holds the same object, so a browser's settings can be copied over as they are. It is written whole or not at all ([Native runtime](./native-runtime.md#whole-file-writes)).
- **No `onChange` callbacks.** A system that cares reads the value when it draws or simulates. `Cvars::serial()` is bumped by every change, so a system that caches what it read can tell when to read again. `audio::audio_cvars::apply_audio_cvars` works this way.
- **Values are a tagged enum.** `CvarValue` is `Bool`, `Number(f64)` or `String`, as JavaScript holds them. `CvarType` (`Bool`, `Int`, `Float`, `String`) is detected from the default unless `CvarOptions::kind` sets it.
- **Flags are a struct.** `CvarFlags { persist, readonly, cheat, hidden }`; `persist` defaults to true.

## Commands

A command implements `console::command::Command<C>`, where `C` is the game's own context, handed to every run:

```rust
use shagger::console::command::{Command, CommandDef, CommandOutput};
use shagger::console::command_registry::CommandRegistry;

struct Hello {
    def: CommandDef,
}

impl<C> Command<C> for Hello {
    fn definition(&self) -> &CommandDef {
        &self.def
    }
    fn execute(&mut self, args: &[String], _: &mut CommandRegistry<C>, _: &mut C) -> CommandOutput {
        format!("hello {}", args.join(" ")).into()
    }
}
```

`CommandOutput` is `None`, `Text`, `Fail` (SHAG's `fail(...)`, which batch runs turn into a failure) or `Clear` (the browser's `"__CLEAR__"` marker).

SHAG discovers `*Command.ts` files; Rust lists them. `console::commands::register_engine_commands(registry, config_dir, on_quit)` registers the engine's own: `clear` (`cls`), `cvarexport`, `cvarlist`, `echo`, `exec`, `help` (`?`, `commands`), `quit` (`exit`, `q`), `resetall`, `reset`, `set`, `toggle` and `version` (`ver`). `exec` reads `.cfg` files from `config_dir`. Commands that need the renderer or the world are registered by the game with what they drive.

`CommandRegistry::execute_checked` returns whether the command succeeded along with its output. A registered cvar is also a command: `name` shows it and `name value` sets it, as in SHAG.

## The drop-down console

`debug::debug_console::DebugConsole` is SHAG's `DebugConsole.ts` drawn with egui. It keeps its history in a file (`DebugConsole::new(Some(path))`) where the browser uses `localStorage["debug_console_history"]`, completes with Tab as SHAG does, and sizes itself by `con_size`. The game opens it on the backquote key and draws it each frame with `DebugConsole::ui(ctx, registry, game)`.
