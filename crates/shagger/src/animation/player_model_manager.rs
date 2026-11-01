//! The player model: one rigged Vanguard and its clips, posed per player
//! (SHAG `src/engine/animation/PlayerModelManager.ts`).
//!
//! SHAG loads `vanguard.glb` and every clip file once, retargets each clip
//! to the model's joints by name with its position tracks removed (root
//! motion), and gives every player an instance: a clone of the rig under
//! an anchor at the player's feet, the model scaled 36 (Mixamo metres to
//! Quake units) with the glTF loader's mirroring root replaced by that
//! plain scale. Here an instance is the rig's local transforms and the
//! clip it plays; [`PlayerModel::joint_matrices`] turns it into the skin
//! palette the renderer draws with, in glTF scene space (the caller puts
//! the anchor and the scale in front).
//!
//! Clip playback follows Babylon's `AnimationGroup`: a clip restarts at
//! its first frame when it is switched to, runs at 60 frames a second times
//! its speed ratio (negative plays it backwards) and loops ([`wrap_frame`]).
//! A switch leaves every joint the new clip does not animate where the old
//! one put it, as Babylon does. One-shot clips, the outline proxy and the
//! powerup glow are not ported yet.

use glam::{Mat4, Quat, Vec3};

use super::player_model_assets::AnimFileConfig;
use crate::loader::model::glb::GlbError;
use crate::loader::model::gltf_animation::{AnimationClip, ChannelPath, ClipChannel, GLTF_TARGET_FPS};
use crate::loader::model::gltf_model::{GltfModel, NodeTransform};

/// A looping clip's frame, wrapped into [from, to) in either direction
/// (`wrapFrame`).
#[must_use]
pub fn wrap_frame(frame: f64, from: f64, to: f64) -> f64 {
    let range = to - from;
    // NaN fails `range > 0` too, as in the TypeScript.
    if range.is_nan() || range <= 0.0 || !frame.is_finite() {
        return from;
    }
    // JavaScript's % keeps the dividend's sign, as Rust's does.
    from + (((frame - from) % range) + range) % range
}

/// How a player model is styled for visibility (`PlayerModelVisualStyle`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerModelVisualStyle {
    /// Replaces the albedo colour (values above 1 for HDR pop).
    pub tint_color: Option<Vec3>,
    pub fullbright_enabled: bool,
    pub fullbright_intensity: f32,
    pub outline_enabled: bool,
    pub outline_color: Vec3,
    /// In the model's own (unscaled) units.
    pub outline_width: f32,
}

/// A clip retargeted to the model (`StoredAnim`): channels by node index.
#[derive(Debug, Clone)]
pub struct StoredAnim {
    pub name: String,
    pub channels: Vec<(usize, ClipChannel)>,
    /// The group's frame range at 60 frames a second.
    pub from: f64,
    pub to: f64,
}

/// One player's pose state (`PlayerModelInstance`).
#[derive(Debug, Clone)]
pub struct PlayerModelInstance {
    /// Every node's local transform.
    pub locals: Vec<NodeTransform>,
    pub current_anim: String,
    /// The playing clip (`masterGroup`), by index into the model's anims.
    pub clip: Option<usize>,
    pub frame: f64,
    pub speed_ratio: f64,
    /// The Hips node and its bind position (`hipsBind`).
    pub hips_bind: Option<(usize, Vec3)>,
}

/// The loaded model and clips (the manager's source container).
#[derive(Debug, Clone)]
pub struct PlayerModel {
    pub model: GltfModel,
    pub anims: Vec<StoredAnim>,
    /// `RootNode`, `Armature` and `__root__` nodes (`findRootMotionNodes`).
    root_motion_nodes: Vec<usize>,
}

impl PlayerModel {
    /// Loads the model and its clips (`loadAssets`); `read` returns a file
    /// under `root`. A clip that fails to load is skipped with a warning,
    /// as SHAG's loader does; a missing optional clip quietly.
    pub fn load(
        mut read: impl FnMut(&str) -> Option<Vec<u8>>,
        root: &str,
        model_file: &str,
        anim_files: &[AnimFileConfig],
    ) -> Result<Self, GlbError> {
        let bytes =
            read(&format!("{root}{model_file}")).ok_or_else(|| GlbError(format!("{root}{model_file} is missing")))?;
        let model = GltfModel::from_glb_rigged(&bytes)?;
        if model.skeleton.skins.is_empty() {
            return Err(GlbError("Player model has no skeleton".into()));
        }
        let mut out = Self {
            root_motion_nodes: model
                .skeleton
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    let name = n.name.to_lowercase();
                    name == "rootnode" || name == "armature" || name == "__root__"
                })
                .map(|(i, _)| i)
                .collect(),
            model,
            anims: Vec::new(),
        };
        let embedded = AnimationClip::from_glb(&bytes)?;
        if let Some(first) = embedded.into_iter().next() {
            out.store_animation(first, "embedded")?;
        }
        for anim in anim_files {
            let Some(bytes) = read(&format!("{root}{}", anim.file)) else {
                if !anim.optional {
                    eprintln!(
                        "[PlayerModelManager] Failed to load {}: {root}{} is missing",
                        anim.name, anim.file
                    );
                }
                continue;
            };
            let result = AnimationClip::from_glb(&bytes).and_then(|clips| {
                let clip = clips
                    .into_iter()
                    .next()
                    .ok_or_else(|| GlbError(format!("No animation group in {}", anim.file)))?;
                out.store_animation(clip, anim.name)
            });
            if let Err(e) = result {
                eprintln!("[PlayerModelManager] Failed to load {}: {e}", anim.name);
            }
        }
        Ok(out)
    }

    /// `storeAnimation` with `extractAnimChannels`: position tracks are
    /// dropped and every other channel must name one of the model's bones.
    fn store_animation(&mut self, clip: AnimationClip, name: &str) -> Result<(), GlbError> {
        let to = clip.last_frame();
        let mut channels = Vec::new();
        for channel in clip.channels {
            if channel.path == ChannelPath::Translation {
                continue;
            }
            let node = self
                .model
                .skeleton
                .find(&channel.target)
                .ok_or_else(|| GlbError(format!("Unmapped source animation target: {}", channel.target)))?;
            channels.push((node, channel));
        }
        if channels.is_empty() {
            return Err(GlbError(format!("Player animation \"{name}\" has no usable channels")));
        }
        self.anims.push(StoredAnim {
            name: name.to_owned(),
            channels,
            from: 0.0,
            to,
        });
        Ok(())
    }

    /// A fresh player at rest with no clip (`createInstance`).
    #[must_use]
    pub fn create_instance(&self) -> PlayerModelInstance {
        let skeleton = &self.model.skeleton;
        let hips = skeleton.skins[0]
            .joints
            .iter()
            .copied()
            .find(|&j| skeleton.nodes[j].name.to_lowercase().contains("hips"));
        PlayerModelInstance {
            locals: skeleton.rest(),
            current_anim: String::new(),
            clip: None,
            frame: 0.0,
            speed_ratio: 1.0,
            hips_bind: hips.map(|h| (h, skeleton.nodes[h].rest.translation)),
        }
    }

    /// Switches to a clip from its first frame at speed 1; a clip already
    /// playing, or one not loaded, changes nothing (`playAnimationOn`).
    pub fn play_animation(&self, instance: &mut PlayerModelInstance, name: &str) -> bool {
        if instance.current_anim == name {
            return false;
        }
        let Some(index) = self.anims.iter().position(|a| a.name == name) else {
            return false;
        };
        instance.clip = Some(index);
        instance.frame = self.anims[index].from;
        instance.speed_ratio = 1.0;
        instance.current_anim = name.to_owned();
        self.apply(instance);
        true
    }

    /// Advances the playing clip by `dt_seconds` at its speed ratio and
    /// poses the rig (`stepAnimation`'s clock, which is Babylon's).
    pub fn advance(&self, instance: &mut PlayerModelInstance, dt_seconds: f64) {
        let Some(anim) = instance.clip.and_then(|c| self.anims.get(c)) else {
            return;
        };
        instance.frame = wrap_frame(
            instance.frame + dt_seconds * GLTF_TARGET_FPS * instance.speed_ratio,
            anim.from,
            anim.to,
        );
        self.apply(instance);
    }

    /// Jumps the playing clip to a frame (`goToFrame`).
    pub fn go_to_frame(&self, instance: &mut PlayerModelInstance, frame: f64) {
        instance.frame = frame;
        self.apply(instance);
    }

    fn apply(&self, instance: &mut PlayerModelInstance) {
        let Some(anim) = instance.clip.and_then(|c| self.anims.get(c)) else {
            return;
        };
        let time = (instance.frame / GLTF_TARGET_FPS) as f32;
        for (node, channel) in &anim.channels {
            let Some(local) = instance.locals.get_mut(*node) else {
                continue;
            };
            let v = channel.sample(time);
            match channel.path {
                ChannelPath::Rotation => local.rotation = Quat::from_array(v),
                ChannelPath::Scale => local.scale = Vec3::new(v[0], v[1], v[2]),
                ChannelPath::Translation => local.translation = Vec3::new(v[0], v[1], v[2]),
            }
        }
        self.reset_root_motion(instance);
    }

    /// Cancels root motion so the body stays on its anchor
    /// (`resetRootMotion`): the root nodes go to the origin and the Hips
    /// back to their bind position.
    pub fn reset_root_motion(&self, instance: &mut PlayerModelInstance) {
        for &node in &self.root_motion_nodes {
            if let Some(local) = instance.locals.get_mut(node) {
                local.translation = Vec3::ZERO;
            }
        }
        if let Some((node, bind)) = instance.hips_bind
            && let Some(local) = instance.locals.get_mut(node)
        {
            local.translation = bind;
        }
    }

    /// The skin palette for `skin`, in glTF scene space: joint global
    /// times inverse bind.
    #[must_use]
    pub fn joint_matrices(&self, instance: &PlayerModelInstance, skin: usize) -> Vec<Mat4> {
        let skeleton = &self.model.skeleton;
        skeleton.joint_matrices(skin, &skeleton.global_matrices(&instance.locals))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_frames_both_ways() {
        assert_eq!(wrap_frame(5.0, 0.0, 10.0), 5.0);
        assert_eq!(wrap_frame(12.5, 0.0, 10.0), 2.5);
        assert_eq!(wrap_frame(-2.5, 0.0, 10.0), 7.5);
        assert_eq!(wrap_frame(10.0, 0.0, 10.0), 0.0);
        assert_eq!(wrap_frame(3.0, 2.0, 2.0), 2.0);
        assert_eq!(wrap_frame(f64::NAN, 1.0, 10.0), 1.0);
    }
}
