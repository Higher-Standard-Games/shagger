//! Taking damage (SHAG `engine/ui/hud/DamageOverlay.ts`): a red vignette
//! closing in, brighter on the side the damage came from, and blood-red
//! triangles scattered along the screen's edges, weighted to that side.
//! More damage, more of it and longer.

use crate::ui::hud_2d::Hud2d;

const FLASH_DURATION_MIN: f64 = 0.35;
const FLASH_DURATION_MAX: f64 = 1.4;
const VIGNETTE_MAX_ALPHA: f32 = 0.7;
const TRIANGLE_COUNT_MIN: f64 = 28.0;
const TRIANGLE_COUNT_MAX: f64 = 60.0;
const TRIANGLE_SIZE_MIN: f32 = 12.0;
const TRIANGLE_SIZE_MAX: f32 = 50.0;
const BIG_TRIANGLE_SIZE: f32 = 70.0;
const BIG_TRIANGLE_CHANCE: f32 = 0.15;
const TRIANGLE_ALPHA_MIN: f32 = 0.35;
const TRIANGLE_ALPHA_MAX: f32 = 0.95;
const BORDER_DEPTH: f32 = 0.28;
const MAX_ACTIVE_FLASHES: usize = 6;
const HIGH_DAMAGE: f64 = 50.0;

#[derive(Debug, Clone, Copy)]
struct Triangle {
    /// 0 top, 1 right, 2 bottom, 3 left.
    edge: usize,
    edge_pos: f32,
    depth_pos: f32,
    rotation: f32,
    size: f32,
    alpha: f32,
    red: f32,
}

#[derive(Debug, Clone)]
struct DamageFlash {
    angle: f32,
    intensity: f32,
    time_left: f64,
    duration: f64,
    triangles: Vec<Triangle>,
}

/// `DamageOverlay`.
#[derive(Debug)]
pub struct DamageOverlay {
    flashes: Vec<DamageFlash>,
    /// `Math.random`'s stand-in: xorshift, nothing depends on its numbers.
    seed: u64,
}

impl Default for DamageOverlay {
    fn default() -> Self {
        Self {
            flashes: Vec::new(),
            seed: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl DamageOverlay {
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 40) as f32 / (1u64 << 24) as f32
    }

    /// `show`: `angle` is where the damage came from on screen, radians,
    /// 0 to the right and a quarter turn down.
    pub fn show(&mut self, damage: f64, angle: f32) {
        let intensity = (damage / HIGH_DAMAGE).min(1.0);
        let duration = FLASH_DURATION_MIN + (FLASH_DURATION_MAX - FLASH_DURATION_MIN) * intensity;
        let count = (TRIANGLE_COUNT_MIN + (TRIANGLE_COUNT_MAX - TRIANGLE_COUNT_MIN) * intensity).round() as usize;
        let triangles = self.generate_triangles(count, angle, intensity as f32);
        if self.flashes.len() >= MAX_ACTIVE_FLASHES {
            self.flashes.remove(0);
        }
        self.flashes.push(DamageFlash {
            angle,
            intensity: intensity as f32,
            time_left: duration,
            duration,
            triangles,
        });
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.flashes.is_empty()
    }

    fn generate_triangles(&mut self, count: usize, angle: f32, intensity: f32) -> Vec<Triangle> {
        let weights = edge_weights(angle);
        (0..count)
            .map(|_| {
                let r = self.random();
                let mut cumulative = 0.0;
                let mut edge = 3;
                for (i, w) in weights.iter().enumerate() {
                    cumulative += w;
                    if r <= cumulative {
                        edge = i;
                        break;
                    }
                }
                let big = intensity > 0.5 && self.random() < BIG_TRIANGLE_CHANCE;
                let size = if big {
                    BIG_TRIANGLE_SIZE * (0.7 + self.random() * 0.3)
                } else {
                    TRIANGLE_SIZE_MIN + self.random() * (TRIANGLE_SIZE_MAX - TRIANGLE_SIZE_MIN)
                };
                Triangle {
                    edge,
                    edge_pos: self.random(),
                    depth_pos: self.random(),
                    rotation: self.random() * std::f32::consts::TAU,
                    size,
                    alpha: TRIANGLE_ALPHA_MIN + self.random() * (TRIANGLE_ALPHA_MAX - TRIANGLE_ALPHA_MIN) * intensity,
                    red: 140.0 + (self.random() * 80.0).floor(),
                }
            })
            .collect()
    }

    /// `_tick`: moves the flashes on by `dt` seconds and draws them over a
    /// `w` by `h` screen.
    pub fn draw(&mut self, hud: &mut Hud2d, dt: f64, w: f32, h: f32) {
        let dt = dt.min(0.1);
        for flash in &mut self.flashes {
            flash.time_left -= dt;
        }
        self.flashes.retain(|f| f.time_left > 0.0);
        for flash in &self.flashes {
            let progress = (1.0 - flash.time_left / flash.duration) as f32;
            let fade = 1.0 - progress * progress;
            draw_vignette(hud, w, h, flash, fade);
            draw_triangles(hud, w, h, flash, fade);
        }
    }
}

/// `getEdgeWeights`: top, right, bottom, left.
fn edge_weights(angle: f32) -> [f32; 4] {
    let (dy, dx) = angle.sin_cos();
    let base = 0.08;
    let right = base + dx.max(0.0) * 0.5;
    let left = base + (-dx).max(0.0) * 0.5;
    let bottom = base + dy.max(0.0) * 0.5;
    let top = base + (-dy).max(0.0) * 0.5;
    let total = top + right + bottom + left;
    [top / total, right / total, bottom / total, left / total]
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> [f32; 4] {
    [r / 255.0, g / 255.0, b / 255.0, a]
}

fn draw_vignette(hud: &mut Hud2d, w: f32, h: f32, flash: &DamageFlash, fade: f32) {
    let alpha = VIGNETTE_MAX_ALPHA * flash.intensity * fade;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let inner = w.min(h) * 0.3;
    let outer = w.max(h) * 0.75;
    hud.radial_gradient(
        cx,
        cy,
        &[
            (inner, rgba(180.0, 0.0, 0.0, 0.0)),
            (inner + (outer - inner) * 0.5, rgba(160.0, 0.0, 0.0, alpha * 0.3)),
            (outer, rgba(140.0, 0.0, 0.0, alpha)),
        ],
    );
    let (dy, dx) = flash.angle.sin_cos();
    let bloom = w.max(h) * 0.5;
    hud.radial_gradient(
        cx + dx * w * 0.4,
        cy + dy * h * 0.4,
        &[
            (0.0, rgba(200.0, 20.0, 20.0, alpha * 0.6)),
            (bloom * 0.4, rgba(180.0, 10.0, 10.0, alpha * 0.2)),
            (bloom, rgba(140.0, 0.0, 0.0, 0.0)),
        ],
    );
}

fn draw_triangles(hud: &mut Hud2d, w: f32, h: f32, flash: &DamageFlash, fade: f32) {
    let (bw, bh) = (w * BORDER_DEPTH, h * BORDER_DEPTH);
    for t in &flash.triangles {
        let (x, y) = match t.edge {
            0 => (t.edge_pos * w, t.depth_pos * bh),
            1 => (w - t.depth_pos * bw, t.edge_pos * h),
            2 => (t.edge_pos * w, h - t.depth_pos * bh),
            _ => (t.depth_pos * bw, t.edge_pos * h),
        };
        let s = t.size;
        let (sin, cos) = t.rotation.sin_cos();
        let place = |px: f32, py: f32| [x + px * cos - py * sin, y + px * sin + py * cos];
        let color = rgba(t.red, 15.0, 20.0, t.alpha * fade);
        hud.polygon(
            &[place(0.0, -s * 0.6), place(-s * 0.5, s * 0.4), place(s * 0.5, s * 0.4)],
            &[color; 3],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_lean_to_the_side_hit_and_flashes_expire() {
        let w = edge_weights(0.0);
        assert!(w[1] > w[3] && (w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        let mut overlay = DamageOverlay::default();
        overlay.show(100.0, 0.0);
        assert_eq!(overlay.flashes[0].triangles.len(), 60);
        let mut hud = Hud2d::new();
        hud.begin(640.0, 360.0, 1.0);
        overlay.draw(&mut hud, 0.1, 640.0, 360.0);
        assert!(overlay.is_active());
        for _ in 0..20 {
            overlay.draw(&mut hud, 0.1, 640.0, 360.0);
        }
        assert!(!overlay.is_active());
    }
}
