//! SHAG `src/engine/loader/shader/Q3StageEvaluation.ts`: shader waves and
//! the texture-coordinate state `tcMod`s step each frame.
//!
//! The arithmetic is the TypeScript's, in its order, with V8's `Math.sin`
//! and `Math.cos` ([`js_sin`], [`js_cos`]) so a run matches SHAG's bit for
//! bit. `tcMod transform`'s `Math.atan2` uses the platform `atan2`; no
//! shipped shader uses it.

use std::f64::consts::PI;

use super::q3_shader_types::{TcMod, WaveForm, WaveFunc};
use crate::shared::js::{js_cos, js_sin};

/// `TcState`: offsets and scales in texture units, rotation in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TcState {
    pub u_offset: f64,
    pub v_offset: f64,
    pub rotation: f64,
    pub u_scale: f64,
    pub v_scale: f64,
}

/// `createTcState`: identity, with any `tcMod scale` applied (the last wins).
#[must_use]
pub fn create_tc_state(tc_mods: &[TcMod]) -> TcState {
    let mut state = TcState {
        u_offset: 0.0,
        v_offset: 0.0,
        rotation: 0.0,
        u_scale: 1.0,
        v_scale: 1.0,
    };
    for tc_mod in tc_mods {
        if let TcMod::Scale { s_scale, t_scale } = *tc_mod {
            state.u_scale = s_scale;
            state.v_scale = t_scale;
        }
    }
    state
}

/// `evaluateWaveFunc`: the wave at `time` seconds. `rng` is `Math.random`
/// for `noise`.
pub fn evaluate_wave_func(wave: &WaveFunc, time: f64, rng: &mut impl FnMut() -> f64) -> f64 {
    let phase = wave.phase + time * wave.frequency;
    let value = match wave.func {
        WaveForm::Sin => js_sin(phase * 2.0 * PI),
        WaveForm::Triangle => ((((phase % 1.0) + 1.0) % 1.0) * 4.0 - 2.0).abs() - 1.0,
        WaveForm::Square => {
            if phase % 1.0 < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        WaveForm::Sawtooth => (((phase % 1.0) + 1.0) % 1.0) * 2.0 - 1.0,
        WaveForm::InverseSawtooth => 1.0 - (((phase % 1.0) + 1.0) % 1.0) * 2.0,
        WaveForm::Noise => rng() * 2.0 - 1.0,
    };
    wave.base + value * wave.amplitude
}

/// `stepTcModsInto`: advance the state by `delta_seconds`, in `tcMod` order.
pub fn step_tc_mods_into(
    state: &mut TcState,
    tc_mods: &[TcMod],
    delta_seconds: f64,
    elapsed_seconds: f64,
    rng: &mut impl FnMut() -> f64,
) {
    for tc_mod in tc_mods {
        match *tc_mod {
            TcMod::Scroll { s_speed, t_speed } => {
                state.u_offset += s_speed * delta_seconds;
                state.v_offset += t_speed * delta_seconds;
                // Wrapped so long sessions keep their precision.
                state.u_offset %= 1000.0;
                state.v_offset %= 1000.0;
            }
            // Applied once, by create_tc_state.
            TcMod::Scale { .. } => {}
            TcMod::Rotate { degrees_per_second } => {
                let rad_per_sec = degrees_per_second * (PI / 180.0);
                state.rotation += rad_per_sec * delta_seconds;
                state.rotation %= 2.0 * PI;
            }
            TcMod::Stretch { func } => {
                let value = evaluate_wave_func(&func, elapsed_seconds, rng);
                state.u_scale = value;
                state.v_scale = value;
            }
            TcMod::Turb {
                amplitude,
                phase,
                frequency,
                ..
            } => {
                let turb_phase = elapsed_seconds * frequency + phase;
                let turb_u = js_sin(turb_phase * 2.0 * PI) * amplitude;
                let turb_v = js_cos(turb_phase * 2.0 * PI + 0.7) * amplitude;
                state.u_offset += turb_u * delta_seconds * 10.0;
                state.v_offset += turb_v * delta_seconds * 10.0;
            }
            TcMod::Transform {
                m00,
                m01,
                m10,
                m11,
                t0,
                t1,
            } => {
                state.u_scale = (m00 * m00 + m01 * m01).sqrt();
                state.v_scale = (m10 * m10 + m11 * m11).sqrt();
                state.rotation = m01.atan2(m00);
                state.u_offset = t0;
                state.v_offset = t1;
            }
        }
    }
}
