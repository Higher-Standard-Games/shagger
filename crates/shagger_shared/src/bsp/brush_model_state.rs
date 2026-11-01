//! Targetable inline geometry (SHAG `src/shared/bsp/BrushModelState.ts`).

use super::map_entities::{EntityLike, ModelLike, inline_model_index, origin_of};
use crate::js::{js_string_to_number, to_int32};
use crate::server::simulation::types::Vec3;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct BrushModelDefinition {
    pub model_index: usize,
    pub targetname: String,
    pub delay: f64,
    pub initially_enabled: bool,
    /// Game space (y up).
    pub origin: Vec3,
    pub min: Vec3,
    pub max: Vec3,
    pub debris: Vec<String>,
}

/// `extractBrushModels`: every `func_explosive`; coordinates cross from Q3 to
/// game space here.
pub fn extract_brush_models<E: EntityLike, M: ModelLike>(entities: &[E], models: &[M]) -> Vec<BrushModelDefinition> {
    let mut definitions = Vec::new();
    for entity in entities {
        if entity.classname() != "func_explosive" {
            continue;
        }
        let Some(model_index) = inline_model_index(entity.property("model"), true) else {
            continue;
        };
        let Some(model) = models.get(model_index) else { continue };
        let Some(origin) = origin_of(entity.property("origin")) else {
            continue;
        };
        let delay = entity.property("delay").map_or(0.0, js_string_to_number);
        let spawnflags = entity.property("spawnflags").map_or(0.0, js_string_to_number);
        let (mins, maxs) = (model.mins(), model.maxs());
        definitions.push(BrushModelDefinition {
            model_index,
            targetname: entity.property("targetname").unwrap_or("").to_owned(),
            delay: if delay.is_finite() {
                crate::js::js_max(0.0, delay)
            } else {
                0.0
            },
            initially_enabled: to_int32(spawnflags) & 1 == 0,
            origin: Vec3::new(origin[0], origin[2], origin[1]),
            min: Vec3::new(mins[0] + origin[0], mins[2] + origin[2], mins[1] + origin[1]),
            max: Vec3::new(maxs[0] + origin[0], maxs[2] + origin[2], maxs[1] + origin[1]),
            debris: ["debris1", "debris2"]
                .iter()
                .filter_map(|k| entity.property(k))
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect(),
        });
    }
    definitions
}

/// `BrushModelState`: which targetable models are present. Shared by
/// rendering and collision; activation belongs to the game.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushModelState {
    pub definitions: Vec<BrushModelDefinition>,
    disabled: BTreeSet<usize>,
    known: BTreeSet<usize>,
}

impl BrushModelState {
    #[must_use]
    pub fn new(definitions: Vec<BrushModelDefinition>) -> Self {
        let known = definitions.iter().map(|d| d.model_index).collect();
        let mut state = Self {
            definitions,
            disabled: BTreeSet::new(),
            known,
        };
        state.reset();
        state
    }

    pub fn reset(&mut self) {
        self.disabled = self
            .definitions
            .iter()
            .filter(|d| !d.initially_enabled)
            .map(|d| d.model_index)
            .collect();
    }

    #[must_use]
    pub fn is_enabled(&self, model_index: usize) -> bool {
        self.known.contains(&model_index) && !self.disabled.contains(&model_index)
    }

    pub fn set_enabled(&mut self, model_index: usize, enabled: bool) {
        if !self.known.contains(&model_index) {
            return;
        }
        if enabled {
            self.disabled.remove(&model_index);
        } else {
            self.disabled.insert(model_index);
        }
    }

    /// The disabled models, ascending.
    #[must_use]
    pub fn snapshot(&self) -> Vec<usize> {
        self.disabled.iter().copied().collect()
    }

    pub fn restore(&mut self, disabled: &[usize]) {
        self.reset();
        for &m in disabled {
            self.set_enabled(m, false);
        }
    }
}
