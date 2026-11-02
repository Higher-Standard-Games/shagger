//! SHAG `src/engine/weapon/PointLightPool.ts`: the eight effect lights the
//! scene's dynamic light cluster carries. A free light is parked at
//! intensity 0; when all are taken, the oldest is stolen and its old
//! holder's handle stops resolving.

use glam::Vec3;

use crate::rendering::dynamic_lights::DynamicLight;

/// Concurrent effect lights.
pub const POOL_SIZE: usize = 8;

/// A pooled light someone holds. It goes stale when the pool takes the
/// light back for another effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PooledLight {
    index: usize,
    generation: u64,
}

#[derive(Debug, Clone, Copy)]
struct PoolEntry {
    light: DynamicLight,
    intensity: f32,
    color: Vec3,
    in_use: bool,
    acquire_time: f64,
    generation: u64,
}

#[derive(Debug, Clone)]
pub struct PointLightPool {
    entries: Vec<PoolEntry>,
}

impl Default for PointLightPool {
    fn default() -> Self {
        Self::new()
    }
}

impl PointLightPool {
    /// `init`: every light parked (intensity 0, range 1).
    #[must_use]
    pub fn new() -> Self {
        let parked = PoolEntry {
            light: DynamicLight::pooled(Vec3::ZERO, Vec3::ZERO, 0.0, 1.0),
            intensity: 0.0,
            color: Vec3::ZERO,
            in_use: false,
            acquire_time: 0.0,
            generation: 0,
        };
        Self {
            entries: vec![parked; POOL_SIZE],
        }
    }

    /// `acquire` (with `forceRecycle`): a free light, or the oldest one
    /// taken back. `now` stands in for `performance.now()`.
    pub fn acquire(&mut self, now: f64, position: Vec3, color: Vec3, intensity: f32, range: f32) -> PooledLight {
        let index = match self.entries.iter().position(|e| !e.in_use) {
            Some(free) => free,
            None => {
                let mut oldest = 0;
                for (i, e) in self.entries.iter().enumerate() {
                    if e.acquire_time < self.entries[oldest].acquire_time {
                        oldest = i;
                    }
                }
                oldest
            }
        };
        let entry = &mut self.entries[index];
        entry.generation += 1;
        entry.in_use = true;
        entry.acquire_time = now;
        entry.color = color;
        entry.intensity = intensity;
        entry.light = DynamicLight::pooled(position, color, intensity, range);
        PooledLight {
            index,
            generation: entry.generation,
        }
    }

    fn entry_mut(&mut self, handle: PooledLight) -> Option<&mut PoolEntry> {
        self.entries
            .get_mut(handle.index)
            .filter(|e| e.in_use && e.generation == handle.generation)
    }

    /// Moves a held light; a stale handle does nothing.
    pub fn set_position(&mut self, handle: PooledLight, position: Vec3) {
        if let Some(e) = self.entry_mut(handle) {
            e.light.position = position;
        }
    }

    /// Sets a held light's intensity and range (an explosion's fading flash).
    pub fn set_intensity_range(&mut self, handle: PooledLight, intensity: f32, range: f32) {
        if let Some(e) = self.entry_mut(handle) {
            e.intensity = intensity;
            e.light = DynamicLight::pooled(e.light.position, e.color, intensity, range);
        }
    }

    /// `release`: parks the light again.
    pub fn release(&mut self, handle: PooledLight) {
        if let Some(e) = self.entry_mut(handle) {
            e.in_use = false;
            e.acquire_time = 0.0;
            e.intensity = 0.0;
            e.light = DynamicLight::pooled(e.light.position, e.color, 0.0, 1.0);
        }
    }

    /// The lights that shine (parked ones add nothing), for the cluster.
    pub fn lights(&self) -> impl Iterator<Item = DynamicLight> + '_ {
        self.entries.iter().filter(|e| e.intensity != 0.0).map(|e| e.light)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_light_is_stolen_when_full() {
        let mut pool = PointLightPool::new();
        let handles: Vec<_> = (0..POOL_SIZE)
            .map(|i| pool.acquire(i as f64, Vec3::ZERO, Vec3::ONE, 1.0, 10.0))
            .collect();
        let late = pool.acquire(100.0, Vec3::X, Vec3::ONE, 2.0, 10.0);
        assert_eq!(late.index, 0);
        pool.release(handles[0]);
        assert_eq!(pool.lights().count(), POOL_SIZE);
        pool.release(late);
        assert_eq!(pool.lights().count(), POOL_SIZE - 1);
    }
}
