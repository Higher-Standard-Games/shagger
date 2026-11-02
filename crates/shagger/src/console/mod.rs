//! SHAG `src/engine/console`: commands and the registry that runs them.
//!
//! The browser discovers `*Command.ts` files; Rust lists them
//! ([`commands::register_engine_commands`]). Commands are generic over the
//! game's context `C`, which the game hands to every run.

pub mod command;
pub mod command_registry;
pub mod commands;
