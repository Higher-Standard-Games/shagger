//! Port of SHAG `src/engine/effects/ColorCorrectionPostProcess.ts`:
//! colour-blindness accessibility.
//!
//! `r_colorcorrection` 1 to 3 "daltonizes" the final image for protanopia,
//! deuteranopia or tritanopia: it works out what that eye cannot tell apart
//! (the difference between a colour and how it is seen) and moves that
//! difference into channels it can. `r_colorcorrection_strength` scales the
//! shift. `r_colorsimulation` 1 to 3 shows the screen as that eye sees it,
//! after the correction.
//!
//! All of it is one 3×3 matrix, worked out here and applied per pixel to the
//! display-encoded frame (decoded with 2.2, mapped, encoded again). The
//! TypeScript runs it as its own pass last on the camera; here image
//! processing applies it as its last step
//! ([`ImageProcessingConfiguration::color_matrix`]), which is the same per
//! pixel.
//!
//! Simulation matrices: Machado, Oliveira & Fernandes (2009), severity 1.0,
//! linear RGB. Error shift: Fidaner, Lin & Ozguven (2005).
//!
//! [`ImageProcessingConfiguration::color_matrix`]:
//! crate::babylon::post_processes::image_processing_post_process::ImageProcessingConfiguration::color_matrix

/// `ColorVisionType`: 0 off, 1 protanopia (red-weak), 2 deuteranopia
/// (green-weak), 3 tritanopia (blue-yellow).
pub type ColorVisionType = u8;

/// Row-major 3×3.
pub type Mat3 = [f64; 9];

const IDENTITY: Mat3 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// `SIMULATION`: how each type sees linear RGB.
const SIMULATION: [Mat3; 3] = [
    [
        0.152286, 1.052583, -0.204868, 0.114503, 0.786281, 0.099216, -0.003882, -0.048116, 1.051998,
    ],
    [
        0.367322, 0.860646, -0.227968, 0.280085, 0.672501, 0.047413, -0.01182, 0.04294, 0.968881,
    ],
    [
        1.255528, -0.076749, -0.178779, -0.078411, 0.930809, 0.147602, 0.004733, 0.691367, 0.3039,
    ],
];

/// `SHIFT`: where the unseen difference goes: red-green into green and
/// blue, blue-yellow into red and green.
const SHIFT: [Mat3; 3] = [
    [0.0, 0.0, 0.0, 0.7, 1.0, 0.0, 0.7, 0.0, 1.0],
    [0.0, 0.0, 0.0, 0.7, 1.0, 0.0, 0.7, 0.0, 1.0],
    [1.0, 0.0, 0.7, 0.0, 1.0, 0.7, 0.0, 0.0, 0.0],
];

/// `multiply`.
#[must_use]
pub fn multiply(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [0.0; 9];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] = a[row * 3] * b[col] + a[row * 3 + 1] * b[3 + col] + a[row * 3 + 2] * b[6 + col];
        }
    }
    out
}

fn add(a: &Mat3, b: &Mat3, scale: f64) -> Mat3 {
    std::array::from_fn(|i| a[i] + b[i] * scale)
}

/// `simulationMatrix`.
#[must_use]
pub fn simulation_matrix(kind: ColorVisionType) -> Mat3 {
    match kind {
        1..=3 => SIMULATION[usize::from(kind - 1)],
        _ => IDENTITY,
    }
}

/// `correctionMatrix`: I + strength · shift · (I − simulation), the colour
/// plus its shifted unseen part.
#[must_use]
pub fn correction_matrix(kind: ColorVisionType, strength: f64) -> Mat3 {
    if !(1..=3).contains(&kind) || strength <= 0.0 {
        return IDENTITY;
    }
    let i = usize::from(kind - 1);
    let unseen = add(&IDENTITY, &SIMULATION[i], -1.0);
    add(&IDENTITY, &multiply(&SHIFT[i], &unseen), strength)
}

/// `colorMatrix`: the correction first, then the simulation of what is seen.
#[must_use]
pub fn color_matrix(correction: ColorVisionType, strength: f64, simulation: ColorVisionType) -> Mat3 {
    multiply(&simulation_matrix(simulation), &correction_matrix(correction, strength))
}

/// `asColorVisionType`: a cvar's value, rounded; anything else is 0.
#[must_use]
pub fn as_color_vision_type(value: f64) -> ColorVisionType {
    match value.round() {
        1.0 => 1,
        2.0 => 2,
        3.0 => 3,
        _ => 0,
    }
}

/// `set` then `isActive`: the matrix for these cvars, or none when the pass
/// would not run (no correction at a positive strength, no simulation).
#[must_use]
pub fn active_color_matrix(correction: f64, strength: f64, simulation: f64) -> Option<Mat3> {
    let correction = as_color_vision_type(correction);
    let strength = strength.clamp(0.0, 1.0);
    let simulation = as_color_vision_type(simulation);
    ((correction != 0 && strength > 0.0) || simulation != 0).then(|| color_matrix(correction, strength, simulation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_is_no_pass() {
        assert_eq!(active_color_matrix(0.0, 1.0, 0.0), None);
        assert_eq!(active_color_matrix(2.0, 0.0, 0.0), None);
        assert_eq!(correction_matrix(1, 0.0), IDENTITY);
        assert_eq!(as_color_vision_type(2.4), 2);
        assert_eq!(as_color_vision_type(7.0), 0);
    }

    #[test]
    fn correction_is_the_identity_plus_the_shifted_error() {
        // Deuteranopia: red's row is untouched, green gains 0.7 of the red
        // error plus the green error.
        let m = correction_matrix(2, 1.0);
        assert_eq!(&m[..3], &[1.0, 0.0, 0.0]);
        let unseen_r = [1.0 - 0.367322, -0.860646, 0.227968];
        let unseen_g = [-0.280085, 1.0 - 0.672501, -0.047413];
        for c in 0..3 {
            let expected = f64::from(u8::from(c == 1)) + 0.7 * unseen_r[c] + unseen_g[c];
            assert!((m[3 + c] - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn simulation_follows_correction() {
        let m = color_matrix(1, 0.5, 1);
        let expected = multiply(&SIMULATION[0], &correction_matrix(1, 0.5));
        assert_eq!(m, expected);
        assert_eq!(active_color_matrix(0.0, 1.0, 3.0), Some(SIMULATION[2]));
    }
}
