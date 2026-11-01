//! glTF animations as Babylon's glTF loader turns them into `Animation`s
//! (`glTFLoader.ts` `_loadAnimationChannelAsync`): each channel's key times
//! become frames at the loader's `targetFps` (60), and a frame between two
//! keys interpolates them (quaternions by slerp, vectors linearly, cubic
//! splines by their Hermite tangents). Outside its keys a channel holds its
//! first or last value.

use super::glb::{Glb, GlbError};

/// Babylon's glTF loader default `targetFps`.
pub const GLTF_TARGET_FPS: f64 = 60.0;

/// What a channel animates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPath {
    Translation,
    Rotation,
    Scale,
}

/// One animated property of one node, targeted by the node's name (clips
/// are retargeted by bone name, `PlayerModelManager.extractAnimChannels`).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipChannel {
    pub target: String,
    pub path: ChannelPath,
    /// Key times in seconds.
    pub times: Vec<f32>,
    /// Per key: the value, or for a cubic spline in-tangent, value,
    /// out-tangent.
    pub values: Vec<f32>,
    pub cubic: bool,
}

impl ClipChannel {
    fn components(&self) -> usize {
        match self.path {
            ChannelPath::Rotation => 4,
            _ => 3,
        }
    }

    fn key(&self, index: usize, part: usize) -> [f32; 4] {
        let c = self.components();
        let stride = if self.cubic { 3 * c } else { c };
        let at = index * stride + if self.cubic { part * c } else { 0 };
        let mut out = [0.0; 4];
        if let Some(v) = self.values.get(at..at + c) {
            out[..c].copy_from_slice(v);
        }
        out
    }

    /// The channel's value at `time` seconds: x, y, z (and w for a
    /// rotation, as glTF stores quaternions).
    #[must_use]
    pub fn sample(&self, time: f32) -> [f32; 4] {
        let n = self.times.len();
        if n == 0 {
            return [0.0, 0.0, 0.0, 1.0];
        }
        if n == 1 || time <= self.times[0] {
            return self.key(0, 1);
        }
        if time >= self.times[n - 1] {
            return self.key(n - 1, 1);
        }
        let next = self.times.partition_point(|&t| t <= time).min(n - 1);
        let prev = next - 1;
        let span = self.times[next] - self.times[prev];
        let t = if span > 0.0 {
            (time - self.times[prev]) / span
        } else {
            0.0
        };
        let a = self.key(prev, 1);
        let b = self.key(next, 1);
        if self.cubic {
            // glTF's Hermite spline: tangents scaled by the key span.
            let out_tangent = self.key(prev, 2);
            let in_tangent = self.key(next, 0);
            let t2 = t * t;
            let t3 = t2 * t;
            let mut r = [0.0; 4];
            for i in 0..4 {
                r[i] = (2.0 * t3 - 3.0 * t2 + 1.0) * a[i]
                    + (t3 - 2.0 * t2 + t) * span * out_tangent[i]
                    + (-2.0 * t3 + 3.0 * t2) * b[i]
                    + (t3 - t2) * span * in_tangent[i];
            }
            if self.path == ChannelPath::Rotation {
                let q = glam::Quat::from_array(r).normalize();
                return q.to_array();
            }
            return r;
        }
        if self.path == ChannelPath::Rotation {
            let qa = glam::Quat::from_array(a);
            let qb = glam::Quat::from_array(b);
            return qa.slerp(qb, t).to_array();
        }
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            0.0,
        ]
    }
}

/// A glTF animation: Babylon's `AnimationGroup` for it.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationClip {
    pub name: String,
    pub channels: Vec<ClipChannel>,
}

impl AnimationClip {
    /// The animations of a `.glb`.
    pub fn from_glb(bytes: &[u8]) -> Result<Vec<Self>, GlbError> {
        Self::from_document(&Glb::parse(bytes)?)
    }

    /// The animations of a parsed `.glb`.
    pub fn from_document(glb: &Glb) -> Result<Vec<Self>, GlbError> {
        let doc = &glb.document;
        let mut clips = Vec::new();
        for (i, animation) in doc.animations.iter().enumerate() {
            let mut channels = Vec::new();
            for channel in &animation.channels {
                let Some(node) = channel.target.node.and_then(|n| doc.nodes.get(n).map(|node| (n, node))) else {
                    continue;
                };
                let path = match channel.target.path.as_str() {
                    "translation" => ChannelPath::Translation,
                    "rotation" => ChannelPath::Rotation,
                    "scale" => ChannelPath::Scale,
                    _ => continue,
                };
                let Some(sampler) = animation.samplers.get(channel.sampler) else {
                    continue;
                };
                channels.push(ClipChannel {
                    target: node.1.name.clone().unwrap_or_else(|| format!("node{}", node.0)),
                    path,
                    times: glb.read_floats(sampler.input)?.1,
                    values: glb.read_floats(sampler.output)?.1,
                    cubic: sampler.interpolation.as_deref() == Some("CUBICSPLINE"),
                });
            }
            clips.push(Self {
                name: animation.name.clone().unwrap_or_else(|| format!("animation{i}")),
                channels,
            });
        }
        Ok(clips)
    }

    /// The group's last frame at [`GLTF_TARGET_FPS`]; it starts at frame 0
    /// (the loader normalizes every group to begin there).
    #[must_use]
    pub fn last_frame(&self) -> f64 {
        self.channels
            .iter()
            .filter_map(|c| c.times.last())
            .fold(0.0, |m: f64, &t| m.max(f64::from(t) * GLTF_TARGET_FPS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(path: ChannelPath, times: Vec<f32>, values: Vec<f32>) -> ClipChannel {
        ClipChannel {
            target: "n".into(),
            path,
            times,
            values,
            cubic: false,
        }
    }

    #[test]
    fn interpolates_and_holds_the_ends() {
        let c = channel(
            ChannelPath::Translation,
            vec![1.0, 2.0],
            vec![0.0, 0.0, 0.0, 10.0, 20.0, 30.0],
        );
        assert_eq!(c.sample(0.0), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(c.sample(1.5), [5.0, 10.0, 15.0, 0.0]);
        assert_eq!(c.sample(9.0), [10.0, 20.0, 30.0, 0.0]);
    }

    #[test]
    fn slerps_rotations() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let c = channel(
            ChannelPath::Rotation,
            vec![0.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 0.0, h, 0.0, h],
        );
        let q = glam::Quat::from_array(c.sample(0.5));
        let expected = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        assert!(q.angle_between(expected) < 1e-5);
    }
}
