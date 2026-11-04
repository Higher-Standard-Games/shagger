//! SHAG `src/engine/ui/TransferLoadingView.ts`: a map loading screen, built
//! as the lobby's twin.
//!
//! The lobby's rail (`components::lobby_twin`) keeps the handoff's steps and
//! logs the load's phases; the lobby's stage carries the map's name over its
//! levelshot, and the bar fills phase by phase. When the handoff screen is
//! still up as the map starts loading, this screen appears in its place
//! without a fade, so joining a match reads as one sequence from the ready
//! check to the arena.
//!
//! The browser mounts it in `MapLoadingOverlay`'s container from the loading
//! events; the native client feeds it the same events from its own loader and
//! paints it with [`paint_twin`](super::components::connecting_overlay::paint_twin).

use super::components::connecting_overlay::{BarFill, ConnectingOverlay, OverlayStage, StageState};
use super::components::lobby_twin::{TwinRail, TwinStepState};

/// The map loader's phases in the order it runs them, with what the status says.
pub const PHASES: [(&str, &str); 15] = [
    ("cleanup", "Clearing"),
    ("Loading", "Map data"),
    ("Dependencies", "Packages"),
    ("Shaders", "Shaders"),
    ("Geometry", "Geometry"),
    ("Textures", "Textures"),
    ("Skybox", "Sky"),
    ("Lighting", "Lightmaps"),
    ("Autosprites", "Sprites"),
    ("Collision", "Collision"),
    ("Assets", "Models"),
    ("Finalizing", "Warming up"),
    ("Recovery", "Recovering"),
    ("Entities", "Entities"),
    ("Complete", "Map loaded"),
];

/// How far the bar gets on the map's own phases; the rest is the game setting up.
pub const MAP_SHARE: f64 = 0.9;

/// A local load's steps, by the index of the first phase each covers.
const LOCAL_STEPS: [(&str, usize); 3] = [("Map data", 0), ("World", 4), ("Game", 10)];

/// `LoadingStartEvent` for a map.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadingStart {
    pub id: String,
    /// `mapTitle(id) ?? meta.displayName`.
    pub display_name: Option<String>,
}

/// `LoadingProgressEvent`.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadingProgress {
    pub phase: String,
    /// 0..1 or 0..100, as the phases report it.
    pub percent: Option<f64>,
    pub detail: Option<String>,
}

/// `LoadingCompleteEvent`.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadingComplete {
    pub success: bool,
    pub error: Option<String>,
    /// Milliseconds.
    pub duration: f64,
}

/// A name on two lines where it has two words or more, split where the lines
/// come out most even.
#[must_use]
pub fn title_lines(name: &str) -> Vec<String> {
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.len() < 2 {
        return words.into_iter().map(str::to_owned).collect();
    }
    let mut best = Vec::new();
    let mut longest = usize::MAX;
    for split in 1..words.len() {
        let lines = [words[..split].join(" "), words[split..].join(" ")];
        // JavaScript's `length`: UTF-16 code units.
        let length = lines[0].encode_utf16().count().max(lines[1].encode_utf16().count());
        if length < longest {
            longest = length;
            best = lines.to_vec();
        }
    }
    best
}

/// The share of the map's load a progress event stands for: its phase's
/// place in the order, plus how far into the phase the event says it is.
#[must_use]
pub fn phase_progress(event: &LoadingProgress) -> Option<f64> {
    let index = PHASES.iter().position(|(phase, _)| *phase == event.phase)?;
    // Some phases report 0..1, the event type says 0..100
    let within = match event.percent {
        None => 0.0,
        Some(p) if p > 1.0 => p / 100.0,
        Some(p) => p,
    };
    let next = (index as f64 + within.clamp(0.0, 1.0)).min((PHASES.len() - 1) as f64);
    Some(next / (PHASES.len() - 1) as f64 * MAP_SHARE)
}

/// The map loading screen's state, to paint with
/// [`paint_twin`](super::components::connecting_overlay::paint_twin).
#[derive(Debug, Clone, PartialEq)]
pub struct TransferLoadingView {
    pub rail: TwinRail,
    pub stage: OverlayStage,
    networked: bool,
    progress_share: f64,
    phase_index: usize,
    failed: bool,
    /// Took over from the handoff (`connecting-overlay--continued`).
    pub continued: bool,
}

impl TransferLoadingView {
    /// `mount`: a match's map when `networked`, else a local one. Given the
    /// handoff still up, it takes its place without a fade.
    #[must_use]
    pub fn mount(map: &LoadingStart, networked: bool, handoff: Option<ConnectingOverlay>) -> Self {
        let continued = handoff.is_some();
        let rail = if networked {
            let mut rail = TwinRail::new("Map load", &["Server", "Handshake", "Map load"], "Match handoff");
            // What the handoff said stays on the rail.
            if let Some(h) = &handoff {
                rail.facts.clone_from(&h.rail.facts);
                rail.lines.clone_from(&h.rail.lines);
            }
            rail
        } else {
            let labels: Vec<&str> = LOCAL_STEPS.iter().map(|(label, _)| *label).collect();
            TwinRail::new("Map load", &labels, "Local map")
        };
        let mut stage = OverlayStage::new("NOW LOADING", Vec::new(), "", "LOAD STATUS");
        if let Some(h) = &handoff {
            stage.entered_at = h.stage.entered_at;
            stage.shot = h.stage.shot;
        }
        let mut view = Self {
            rail,
            stage,
            networked,
            progress_share: 0.0,
            phase_index: 0,
            failed: false,
            continued,
        };
        view.restart(map);
        view
    }

    pub fn restart(&mut self, map: &LoadingStart) {
        let name = map.display_name.clone().unwrap_or_else(|| map.id.clone());
        self.stage.set_title(title_lines(&name.to_uppercase()));
        self.rail.set_fact("map", "Map", &map.id);
        "NOW LOADING".clone_into(&mut self.stage.kicker);
        self.stage.lead = if self.networked {
            "Connected to the match server. Loading the arena it is running.".into()
        } else {
            "Loading the arena from its files.".into()
        };
        self.stage.error = None;
        self.failed = false;
        self.progress_share = 0.0;
        self.phase_index = 0;
        self.set_bar(0.0);
        self.set_status(StageState::Loading, "PREPARING");
        self.stage.footer.clear();
        self.refresh_steps(TwinStepState::Active);
    }

    pub fn progress(&mut self, event: &LoadingProgress) {
        let index = PHASES.iter().position(|(phase, _)| *phase == event.phase);
        if let Some(index) = index {
            self.phase_index = self.phase_index.max(index);
        }
        let label = index.map_or(event.phase.as_str(), |i| PHASES[i].1);
        self.set_status(StageState::Loading, &label.to_uppercase());
        self.stage.footer = event.detail.clone().unwrap_or_default();
        match &event.detail {
            Some(detail) if !detail.is_empty() => self.rail.log(&format!("{label}: {detail}")),
            _ => self.rail.log(label),
        }
        if let Some(share) = phase_progress(event) {
            self.set_bar(share);
        }
        self.refresh_steps(TwinStepState::Active);
    }

    pub fn complete(&mut self, event: &LoadingComplete, waiting_for: &[&str]) {
        if !event.success {
            self.failed = true;
            "MAP ERROR".clone_into(&mut self.stage.kicker);
            "The arena could not be loaded.".clone_into(&mut self.stage.lead);
            self.set_status(StageState::Failed, "LOAD FAILED");
            self.stage.footer.clear();
            self.stage.error = Some(
                event
                    .error
                    .clone()
                    .filter(|e| !e.is_empty())
                    .unwrap_or_else(|| "The map could not be loaded.".into()),
            );
            self.refresh_steps(TwinStepState::Error);
            return;
        }
        self.phase_index = PHASES.len() - 1;
        self.set_bar(MAP_SHARE);
        let loaded = format!("Map loaded in {:.1} s", event.duration / 1000.0);
        self.stage.footer.clone_from(&loaded);
        self.rail.log(&loaded);
        if !waiting_for.is_empty() {
            self.set_status(StageState::SettingUp, "SETTING UP");
        }
        self.refresh_steps(TwinStepState::Active);
    }

    pub fn hold_progress(&mut self, label: &str, fraction: f64) {
        if self.failed {
            return;
        }
        self.set_status(
            StageState::SettingUp,
            &format!("{} {}%", label.to_uppercase(), (fraction * 100.0).floor()),
        );
        self.set_bar(MAP_SHARE + (1.0 - MAP_SHARE) * fraction);
    }

    pub fn ready(&mut self) {
        if self.failed {
            return;
        }
        "ARENA READY".clone_into(&mut self.stage.kicker);
        "The arena is loaded. Dropping you in.".clone_into(&mut self.stage.lead);
        self.set_bar(1.0);
        self.set_status(StageState::Ready, "READY");
        self.refresh_steps(TwinStepState::Done);
    }

    /// The bar's share, 0..1.
    #[must_use]
    pub fn share(&self) -> f64 {
        self.progress_share
    }

    fn set_status(&mut self, state: StageState, text: &str) {
        self.stage.state = state;
        text.clone_into(&mut self.stage.status);
    }

    /// The bar only ever moves forward, whatever order the phases report in.
    fn set_bar(&mut self, share: f64) {
        self.progress_share = self.progress_share.max(share);
        self.stage.bar = BarFill::Fill(self.progress_share as f32);
    }

    /// `current` is the state of the step the load is on; the ones before it are done.
    fn refresh_steps(&mut self, current: TwinStepState) {
        let count = self.rail.steps.len();
        let mut at = count.saturating_sub(1);
        if !self.networked {
            at = LOCAL_STEPS.iter().enumerate().fold(
                0,
                |step, (index, (_, first))| if self.phase_index >= *first { index } else { step },
            );
            if current == TwinStepState::Done {
                at = count - 1;
            }
        }
        let states: Vec<TwinStepState> = (0..count)
            .map(|index| match index.cmp(&at) {
                std::cmp::Ordering::Less => TwinStepState::Done,
                std::cmp::Ordering::Equal => current,
                std::cmp::Ordering::Greater => TwinStepState::Pending,
            })
            .collect();
        self.rail.set_steps(&states);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(id: &str) -> LoadingStart {
        LoadingStart {
            id: id.into(),
            display_name: None,
        }
    }

    fn progress(phase: &str, percent: Option<f64>) -> LoadingProgress {
        LoadingProgress {
            phase: phase.into(),
            percent,
            detail: None,
        }
    }

    fn states(view: &TransferLoadingView) -> Vec<TwinStepState> {
        view.rail.steps.iter().map(|(_, s)| *s).collect()
    }

    // TransferLoadingView.test.ts: titleLines.
    #[test]
    fn a_name_splits_where_its_lines_come_out_most_even() {
        assert_eq!(title_lines("WASHOI_DM2"), ["WASHOI_DM2"]);
        assert_eq!(title_lines("PROVING GROUNDS"), ["PROVING", "GROUNDS"]);
        assert_eq!(title_lines("THE LONG DARK HALL"), ["THE LONG", "DARK HALL"]);
        assert_eq!(title_lines("A VERY LONGNAME"), ["A VERY", "LONGNAME"]);
        assert!(title_lines("   ").is_empty());
    }

    // TransferLoadingView.test.ts: phaseProgress.
    #[test]
    fn a_phase_stands_for_its_place_in_the_order() {
        assert_eq!(phase_progress(&progress("cleanup", None)), Some(0.0));
        assert_eq!(phase_progress(&progress("Complete", None)), Some(MAP_SHARE));
        let half = phase_progress(&progress("Loading", Some(50.0))).unwrap();
        assert!((half - 1.5 / 14.0 * MAP_SHARE).abs() < 1e-12);
        // 0..1 and 0..100 are read alike.
        assert_eq!(
            phase_progress(&progress("Loading", Some(0.5))),
            phase_progress(&progress("Loading", Some(50.0)))
        );
        assert_eq!(phase_progress(&progress("Complete", Some(1.0))), Some(MAP_SHARE));
        assert_eq!(phase_progress(&progress("nonsense", None)), None);
    }

    #[test]
    fn the_bar_only_moves_forward() {
        let mut view = TransferLoadingView::mount(&start("washoi_dm2"), true, None);
        view.progress(&progress("Collision", None));
        let at = view.share();
        view.progress(&progress("Loading", None));
        assert_eq!(view.share(), at);
        assert_eq!(view.stage.status, "MAP DATA");
    }

    #[test]
    fn a_match_load_is_on_its_last_step_from_the_start() {
        let mut view = TransferLoadingView::mount(&start("washoi_dm2"), true, None);
        assert_eq!(
            states(&view),
            [TwinStepState::Done, TwinStepState::Done, TwinStepState::Active]
        );
        assert_eq!(view.stage.title, ["WASHOI_DM2"]);
        view.complete(
            &LoadingComplete {
                success: true,
                error: None,
                duration: 1234.0,
            },
            &["fps:postload"],
        );
        assert_eq!(view.stage.state, StageState::SettingUp);
        assert_eq!(view.stage.footer, "Map loaded in 1.2 s");
        view.hold_progress("Compiling shaders", 0.5);
        assert_eq!(view.stage.status, "COMPILING SHADERS 50%");
        assert!((view.share() - 0.95).abs() < 1e-12);
        view.ready();
        assert_eq!(view.stage.kicker, "ARENA READY");
        assert_eq!(view.share(), 1.0);
        assert_eq!(states(&view), [TwinStepState::Done; 3]);
    }

    #[test]
    fn a_local_load_walks_its_steps_by_phase() {
        let mut view = TransferLoadingView::mount(&start("washoi_dm2"), false, None);
        assert_eq!(view.rail.context, "Local map");
        assert_eq!(states(&view)[0], TwinStepState::Active);
        view.progress(&progress("Geometry", None));
        assert_eq!(
            states(&view),
            [TwinStepState::Done, TwinStepState::Active, TwinStepState::Pending]
        );
        view.progress(&progress("Assets", None));
        assert_eq!(states(&view)[2], TwinStepState::Active);
    }

    #[test]
    fn a_failed_load_says_why_and_stops_moving() {
        let mut view = TransferLoadingView::mount(&start("washoi_dm2"), true, None);
        view.complete(
            &LoadingComplete {
                success: false,
                error: Some("maps/washoi_dm2.bsp is in none of the packages".into()),
                duration: 0.0,
            },
            &[],
        );
        assert_eq!(view.stage.status, "LOAD FAILED");
        assert_eq!(view.stage.kicker, "MAP ERROR");
        assert_eq!(states(&view)[2], TwinStepState::Error);
        view.hold_progress("Compiling shaders", 1.0);
        view.ready();
        assert_eq!(view.stage.status, "LOAD FAILED");
    }

    #[test]
    fn taking_over_from_the_handoff_keeps_its_rail_and_skips_the_fade() {
        let mut handoff = ConnectingOverlay::new("wss://eu.washoi.net/game/1", false);
        handoff.set_status("CONNECTING");
        handoff.stage.entered_at = Some(3.0);
        let view = TransferLoadingView::mount(
            &LoadingStart {
                id: "washoi_dm2".into(),
                display_name: Some("Proving Grounds".into()),
            },
            true,
            Some(handoff),
        );
        assert!(view.continued);
        assert_eq!(view.stage.entered_at, Some(3.0));
        assert_eq!(view.stage.title, ["PROVING", "GROUNDS"]);
        assert_eq!(view.rail.facts.len(), 2);
        assert!(view.rail.lines.iter().any(|l| l == "Connecting"));
    }
}
