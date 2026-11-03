//! SHAG `src/engine/debug/DebugConsole.ts`: the drop-down console. Command
//! history with the arrows (kept between runs), Tab completion (the common
//! prefix, then cycling, and a double Tab lists the matches), the log, and
//! `con_size`. The browser builds it from HTML over the canvas; this draws
//! it with egui over the game. The window opens and closes it on the
//! backquote key.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::console::command::CommandOutput;
use crate::console::command_registry::CommandRegistry;

const MAX_HISTORY: usize = 100;
const MAX_OUTPUT_LINES: usize = 500;
const DEFAULT_CONSOLE_SIZE: f32 = 0.5;
const DOUBLE_TAB: Duration = Duration::from_millis(600);

/// The kinds of line `log` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogType {
    Command,
    Result,
    Error,
    Warn,
    Info,
    System,
}

#[derive(Debug)]
pub struct DebugConsole {
    pub open: bool,
    pub lines: VecDeque<(LogType, String)>,
    /// Newest first.
    history: Vec<String>,
    history_index: isize,
    current_input: String,
    pub input: String,
    pub autocomplete_items: Vec<String>,
    pub autocomplete_index: isize,
    last_tab: Option<(Instant, String, Vec<String>)>,
    /// Share of the window's height (`con_size`).
    pub size: f32,
    /// Where the history is kept (`localStorage["debug_console_history"]`).
    history_file: Option<PathBuf>,
    /// Move the caret to the end on the next frame.
    caret_to_end: bool,
}

impl DebugConsole {
    #[must_use]
    pub fn new(history_file: Option<PathBuf>) -> Self {
        let history = history_file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
            .unwrap_or_default();
        Self {
            open: false,
            lines: VecDeque::new(),
            history,
            history_index: -1,
            current_input: String::new(),
            input: String::new(),
            autocomplete_items: Vec::new(),
            autocomplete_index: -1,
            last_tab: None,
            size: DEFAULT_CONSOLE_SIZE,
            history_file,
            caret_to_end: false,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// `log`.
    pub fn log(&mut self, kind: LogType, message: &str) {
        self.lines.push_back((kind, message.to_owned()));
        while self.lines.len() > MAX_OUTPUT_LINES {
            self.lines.pop_front();
        }
    }

    /// `executeCurrentInput`.
    pub fn submit<C>(&mut self, registry: &mut CommandRegistry<C>, ctx: &mut C) {
        let input = self.input.trim().to_owned();
        if input.is_empty() {
            return;
        }
        self.add_to_history(&input);
        self.input.clear();
        self.history_index = -1;
        self.current_input.clear();
        self.hide_autocomplete();
        self.log(LogType::Command, &input);
        if let Some(size) = con_size(&input) {
            let text = self.set_size(size);
            self.log(LogType::Result, &text);
            return;
        }
        match registry.execute(&input, ctx) {
            CommandOutput::None => {}
            CommandOutput::Text(t) => self.log(LogType::Result, &t),
            CommandOutput::Fail(t) => self.log(LogType::Error, &t),
            CommandOutput::Clear => self.lines.clear(),
        }
    }

    fn add_to_history(&mut self, command: &str) {
        if self.history.first().map(String::as_str) == Some(command) {
            return;
        }
        self.history.insert(0, command.to_owned());
        self.history.truncate(MAX_HISTORY);
        if let Some(file) = &self.history_file {
            let text = serde_json::to_string(&self.history).unwrap_or_default();
            let _ = crate::core::atomic_file::write_atomic(file, text.as_bytes());
        }
    }

    /// `navigateHistory`: 1 is older, -1 newer.
    pub fn navigate_history(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        if self.history_index == -1 && direction == 1 {
            self.current_input = self.input.clone();
        }
        self.history_index = (self.history_index + direction).clamp(-1, self.history.len() as isize - 1);
        self.input = if self.history_index == -1 {
            self.current_input.clone()
        } else {
            self.history[self.history_index as usize].clone()
        };
        self.caret_to_end = true;
    }

    /// `setSize`.
    pub fn set_size(&mut self, size: f32) -> String {
        if !(0.1..=1.0).contains(&size) {
            return format!("Invalid size: {size}. Must be between 0.1 and 1.0");
        }
        self.size = size;
        format!("Console size set to {:.0}%", size * 100.0)
    }

    /// `updateAutocomplete`.
    pub fn update_autocomplete<C>(&mut self, registry: &CommandRegistry<C>) {
        if self.input.is_empty() {
            self.hide_autocomplete();
            return;
        }
        self.autocomplete_items = registry.completions(&self.input);
        self.autocomplete_index = -1;
    }

    fn hide_autocomplete(&mut self) {
        self.autocomplete_items.clear();
        self.autocomplete_index = -1;
    }

    /// `handleTabComplete`.
    pub fn tab_complete<C>(&mut self, registry: &CommandRegistry<C>, now: Instant) {
        if self.input.is_empty() {
            return;
        }
        if self.autocomplete_items.is_empty() {
            self.update_autocomplete(registry);
            if self.autocomplete_items.is_empty() {
                return;
            }
        }
        if self.autocomplete_items.len() == 1 {
            self.select_autocomplete(0);
            return;
        }
        let prefix = longest_common_prefix(&self.autocomplete_items);
        if prefix.len() > self.input.trim_end().len() {
            self.input = prefix;
            self.caret_to_end = true;
            self.update_autocomplete(registry);
            self.last_tab = None;
            return;
        }
        let double = self.last_tab.as_ref().is_some_and(|(at, input, matches)| {
            *input == self.input && *matches == self.autocomplete_items && now.duration_since(*at) < DOUBLE_TAB
        });
        if double {
            let mut lines = vec!["Matches:".to_owned()];
            lines.extend(self.autocomplete_items.iter().map(|s| format!("  {s}")));
            self.log(LogType::System, &lines.join("\n"));
            self.last_tab = None;
            return;
        }
        self.last_tab = Some((now, self.input.clone(), self.autocomplete_items.clone()));
        self.autocomplete_index = (self.autocomplete_index + 1) % self.autocomplete_items.len() as isize;
    }

    /// `selectAutocomplete`.
    pub fn select_autocomplete(&mut self, index: usize) {
        if let Some(item) = self.autocomplete_items.get(index) {
            self.input = format!("{item} ");
            self.caret_to_end = true;
        }
        self.hide_autocomplete();
    }

    /// Draws the console over the top of the window while it is open.
    pub fn ui<C>(&mut self, ctx: &egui::Context, registry: &mut CommandRegistry<C>, game: &mut C) {
        if !self.open {
            return;
        }
        let height = ctx.content_rect().height() * self.size;
        let frame = egui::Frame::new()
            .fill(egui::Color32::from_rgba_unmultiplied(8, 6, 18, 235))
            .inner_margin(10.0);
        egui::Area::new(egui::Id::unique("debug_console"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(ctx.content_rect().width() - 20.0);
                    ui.set_height(height);
                    let input_h = 30.0;
                    egui::ScrollArea::vertical()
                        .max_height(height - input_h - 8.0)
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for (kind, text) in &self.lines {
                                let color = match kind {
                                    LogType::Command => egui::Color32::from_rgb(255, 210, 90),
                                    LogType::Error => egui::Color32::from_rgb(255, 110, 110),
                                    LogType::Warn => egui::Color32::from_rgb(255, 180, 80),
                                    LogType::System => egui::Color32::from_rgb(150, 200, 255),
                                    _ => egui::Color32::from_rgb(235, 228, 210),
                                };
                                let text = if *kind == LogType::Command {
                                    format!("] {text}")
                                } else {
                                    text.clone()
                                };
                                ui.label(egui::RichText::new(text).monospace().color(color));
                            }
                        });
                    self.input_row(ui, registry, game);
                });
            });
    }

    fn input_row<C>(&mut self, ui: &mut egui::Ui, registry: &mut CommandRegistry<C>, game: &mut C) {
        let id = egui::Id::unique("debug_console_input");
        let (up, down, tab, enter) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                i.key_pressed(egui::Key::Enter),
            )
        });
        if up {
            self.navigate_history(1);
        }
        if down {
            self.navigate_history(-1);
        }
        if tab {
            self.tab_complete(registry, Instant::now());
        }
        let before = self.input.clone();
        let mut output = egui::TextEdit::singleline(&mut self.input)
            .id(id)
            .hint_text("Type a command... (Tab for autocomplete)")
            .desired_width(f32::INFINITY)
            .font(egui::TextStyle::Monospace)
            .show(ui);
        output.response.request_focus();
        if self.caret_to_end {
            let end = egui::text::CCursor::new(self.input.chars().count());
            output
                .state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(end)));
            output.state.store(ui.ctx(), id);
            self.caret_to_end = false;
        }
        if self.input != before && !tab && !up && !down {
            self.update_autocomplete(registry);
        }
        if enter {
            self.submit(registry, game);
        }
        if !self.autocomplete_items.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (i, item) in self.autocomplete_items.clone().iter().enumerate().take(12) {
                    let selected = i as isize == self.autocomplete_index;
                    if ui.selectable_label(selected, item).clicked() {
                        self.select_autocomplete(i);
                    }
                }
            });
        }
    }
}

/// `con_size <n>`, which the console answers itself.
fn con_size(input: &str) -> Option<f32> {
    let mut parts = input.split_whitespace();
    (parts.next()? == "con_size").then_some(())?;
    parts.next()?.parse().ok()
}

/// `getLongestCommonPrefix`.
fn longest_common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut prefix: &str = first;
    for s in &items[1..] {
        let j = prefix
            .char_indices()
            .zip(s.chars())
            .take_while(|((_, a), b)| a == b)
            .last()
            .map_or(0, |((i, a), _)| i + a.len_utf8());
        prefix = &prefix[..j];
        if prefix.is_empty() {
            break;
        }
    }
    prefix.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::commands::register_engine_commands;
    use crate::core::cvar::{CvarOptions, init_cvars, with_cvars};

    fn setup() -> CommandRegistry<()> {
        init_cvars(None, false);
        with_cvars(|c| {
            c.register("cl_fov", 90, CvarOptions::default());
            c.register("cl_footsteps", 1, CvarOptions::default());
        });
        let mut r = CommandRegistry::new();
        register_engine_commands(&mut r, None, |_: &mut ()| {});
        r
    }

    #[test]
    fn tab_completes_the_common_prefix_then_lists() {
        let r = setup();
        let mut con = DebugConsole::new(None);
        con.input = "cl_f".into();
        let t0 = Instant::now();
        con.tab_complete(&r, t0);
        assert_eq!(con.input, "cl_fo", "the common prefix");
        assert_eq!(con.autocomplete_items, vec!["cl_footsteps", "cl_fov"]);
        con.tab_complete(&r, t0);
        assert_eq!(con.autocomplete_index, 0);
        con.tab_complete(&r, t0 + Duration::from_millis(100));
        assert!(con.lines.back().unwrap().1.starts_with("Matches:\n  cl_footsteps"));
        con.input = "cvarl".into();
        con.update_autocomplete(&r);
        con.tab_complete(&r, t0);
        assert_eq!(con.input, "cvarlist ");
    }

    #[test]
    fn history_walks_and_keeps_the_draft() {
        let mut r = setup();
        let mut con = DebugConsole::new(None);
        for cmd in ["echo one", "echo two"] {
            con.input = cmd.into();
            con.submit(&mut r, &mut ());
        }
        assert_eq!(con.lines.back().unwrap().1, "two");
        con.input = "draft".into();
        con.navigate_history(1);
        assert_eq!(con.input, "echo two");
        con.navigate_history(1);
        con.navigate_history(1);
        assert_eq!(con.input, "echo one");
        con.navigate_history(-1);
        con.navigate_history(-1);
        assert_eq!(con.input, "draft");
        con.input = "con_size 0.75".into();
        con.submit(&mut r, &mut ());
        assert_eq!(con.size, 0.75);
        con.input = "clear".into();
        con.submit(&mut r, &mut ());
        assert!(con.lines.is_empty());
    }
}
