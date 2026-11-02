//! SHAG `src/engine/console/commands`: the engine's commands that do not
//! need the renderer or the world. The rest come with what they drive.

use std::path::PathBuf;

use super::command_registry::CommandRegistry;

pub mod clear_command;
pub mod cvar_export_command;
pub mod cvar_list_command;
pub mod echo_command;
pub mod exec_command;
pub mod help_command;
pub mod quit_command;
pub mod reset_all_command;
pub mod reset_command;
pub mod set_command;
pub mod toggle_command;
pub mod version_command;

/// `commands/index.ts`'s discovery, as a list. `config_dir` is where `exec`
/// finds `.cfg` files; `quit` runs `on_quit`.
pub fn register_engine_commands<C: 'static>(
    registry: &mut CommandRegistry<C>,
    config_dir: Option<PathBuf>,
    on_quit: fn(&mut C),
) {
    registry.register(Box::new(clear_command::ClearCommand::default()));
    registry.register(Box::new(cvar_export_command::CvarExportCommand::default()));
    registry.register(Box::new(cvar_list_command::CvarListCommand::default()));
    registry.register(Box::new(echo_command::EchoCommand::default()));
    registry.register(Box::new(exec_command::ExecCommand::new(config_dir)));
    registry.register(Box::new(help_command::HelpCommand::default()));
    registry.register(Box::new(quit_command::QuitCommand::new(on_quit)));
    registry.register(Box::new(reset_all_command::ResetAllCommand::default()));
    registry.register(Box::new(reset_command::ResetCommand::default()));
    registry.register(Box::new(set_command::SetCommand::default()));
    registry.register(Box::new(toggle_command::ToggleCommand::default()));
    registry.register(Box::new(version_command::VersionCommand::default()));
}
