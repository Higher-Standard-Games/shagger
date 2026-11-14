//! The player model: one rigged body and its clips, posed per player
//! (SHAG `src/engine/animation/PlayerModelManager.ts`).
//!
//! SHAG loads the body and every clip file once, retargets each clip to the
//! body's joints by name with its position tracks removed (root motion),
//! and gives every player an instance: a clone of the rig under an anchor
//! at the player's feet, the model scaled 36 (Mixamo metres to Quake units)
//! times its fit, with the glTF loader's mirroring root replaced by that
//! plain scale. Here an instance is the rig's local transforms and the
//! clips it plays; [`PlayerModel::joint_matrices`] turns it into the skin
//! palette the renderer draws with, in glTF scene space (the caller puts
//! the anchor, the fit's lift and the scale in front, [`PlayerModel::root_matrix`]).
//!
//! A packaged body (Police and its variants) shares its clips with the
//! others: they come from their own root, made on another rig, and are
//! retargeted with Babylon's `AnimatorAvatar` ([`retarget_animation_group`]).
//! Such a body is fitted to the hull's height, and its weapon hand, gun
//! support and blade stance are measured ([`super::player_model_fit`]).
//! Vanguard keeps its clips beside it and is none of this.
//!
//! Clip playback follows Babylon's `AnimationGroup`: a clip restarts at
//! its first frame when it is switched to, runs at 60 frames a second times
//! its speed ratio (negative plays it backwards) and loops ([`wrap_frame`]).
//! A switch leaves every joint the new clip does not animate where the old
//! one put it, as Babylon does. Over the locomotion clip an instance plays:
//!
//! - its upper body (the Spine and everything under it) at a share of the
//!   clip's speed ([`PlayerModel::set_upper_body_share`]);
//! - an upper-body stance, the blade's, which masks locomotion out of the
//!   upper body ([`PlayerModel::set_upper_body_stance`]);
//! - a one-shot ([`PlayerModel::play_once`]): `blade_slash` and the taunts
//!   on the upper body only, any other clip on the whole body with
//!   locomotion stopped;
//! - the aim pitch's spine bend ([`PlayerAimPose`]), after the clips.
//!
//! The outline proxy, the powerup glow and pose stepping (LOD) are not ported.

use glam::{Mat4, Quat, Vec3};

use super::player_aim_pose::{AIM_JOINTS, PlayerAimPose};
use super::player_model_assets::AnimFileConfig;
use super::player_model_fit::{
    BladeStance, GunSupport, HandBasis, PlayerModelFit, TemplatePose, measure_blade_stance, measure_gun_support,
    measure_player_model_fit,
};
use crate::babylon::animations::animator_avatar::retarget_animation_group;
use crate::babylon::maths::math_glam::decompose;
use crate::loader::model::glb::{Glb, GlbError};
use crate::loader::model::gltf_animation::{AnimationClip, ChannelPath, ClipChannel, GLTF_TARGET_FPS};
use crate::loader::model::gltf_model::{GltfModel, NodeTransform, Skeleton};

/// The gameplay hull's height (`PLAYER_HEIGHT`).
pub const PLAYER_HEIGHT: f32 = 64.0;

/// Taunts: played over the upper body after a kill, and among the victory
/// clips (`TAUNT_ANIMS`).
pub const TAUNT_ANIMS: [&str; 3] = ["taunt_1", "taunt_2", "taunt_3"];
/// The clips the post-game winner showcase picks one of at random: dances
/// and taunts (`VICTORY_ANIMS`).
pub const VICTORY_ANIMS: [&str; 6] = ["dance_1", "dance_2", "dance_3", "taunt_1", "taunt_2", "taunt_3"];
/// One-shots that play on the spine, arms and head while the legs keep
/// their locomotion (`UPPER_BODY_ONE_SHOTS`).
const UPPER_BODY_ONE_SHOTS: [&str; 4] = ["blade_slash", "taunt_1", "taunt_2", "taunt_3"];
/// The clip the blade's stance loops (`BLADE_STANCE`).
pub const BLADE_STANCE: &str = "blade_idle";

/// How far the held blade leans toward the body's front (`BLADE_STANCE_LEAN`).
const BLADE_STANCE_LEAN: f32 = 35.0 * std::f32::consts::PI / 180.0;

/// The aim bend's limit either way (`setAimPitch`).
pub const AIM_PITCH_LIMIT: f32 = 1.4;

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

/// Whether a clip track's joint is one a compact rig may lack: the fingers
/// and the toe ends.
fn may_lack(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("mixamorig:") else {
        return false;
    };
    let Some(rest) = rest.strip_prefix("Left").or_else(|| rest.strip_prefix("Right")) else {
        return false;
    };
    if rest == "Toe_End" {
        return true;
    }
    let Some(rest) = rest.strip_prefix("Hand") else {
        return false;
    };
    ["Thumb", "Index", "Middle", "Ring", "Pinky"].iter().any(|finger| {
        rest.strip_prefix(finger)
            .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4"))
    })
}

/// How a player model is styled for visibility (`PlayerModelVisualStyle`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerModelVisualStyle {
    /// Replaces the albedo colour (values above 1 for HDR pop); None keeps
    /// the body's own colours (`cl_player_tint 0`).
    pub tint_color: Option<Vec3>,
    pub fullbright_enabled: bool,
    pub fullbright_intensity: f32,
    pub outline_enabled: bool,
    pub outline_color: Vec3,
    /// In world units; the outline pass divides it by the body's fit.
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

/// A held upper-body stance (`stances`).
#[derive(Debug, Clone, PartialEq)]
pub struct StanceLayer {
    pub name: String,
    pub clip: usize,
    pub frame: f64,
    pub speed_ratio: f64,
    pub paused: bool,
    /// Not yet animated: a new animatable's first frame is its `from`.
    pub fresh: bool,
}

/// A playing one-shot (`activeOneShots`).
#[derive(Debug, Clone, PartialEq)]
pub struct OneShotLayer {
    pub clip: usize,
    pub frame: f64,
    pub speed_ratio: f64,
    pub base_speed_ratio: f64,
    /// The blade swing or a taunt: only the upper body, over running legs.
    pub upper_body_only: bool,
    /// On the frame the clip ends: the channel whose animatable ended
    /// first. It writes its last key and ends the clip
    /// (`onAnimationEndObservable` fires per animation), so the channels
    /// after it are stopped before they write.
    pub ending: Option<usize>,
    /// Not yet animated: a new animatable's first frame is its `from`.
    pub fresh: bool,
}

/// One player's pose state (`PlayerModelInstance`).
#[derive(Debug, Clone)]
pub struct PlayerModelInstance {
    /// Every node's local transform.
    pub locals: Vec<NodeTransform>,
    pub current_anim: String,
    /// The locomotion clip (`masterGroup`), by index into the model's anims.
    pub clip: Option<usize>,
    /// The locomotion clip's frame: the hips and legs'.
    pub frame: f64,
    /// The upper body's frame while it plays its own share of the clip.
    pub upper_frame: f64,
    pub speed_ratio: f64,
    /// `setUpperBodyShare`.
    pub upper_share: f64,
    /// Whether locomotion runs; a full-body one-shot stops it.
    pub playing: bool,
    /// Locomotion was (re)started and has not animated yet: Babylon
    /// evaluates a new animatable at its `from` frame on its first frame.
    pub fresh: bool,
    /// The same for the upper body's animatables, which a mask pauses
    /// before they first animate when a clip starts under a stance.
    pub upper_fresh: bool,
    /// Whether locomotion is masked out of the upper body (`upperBodyMask`).
    pub upper_masked: bool,
    pub stance: Option<StanceLayer>,
    pub one_shot: Option<OneShotLayer>,
    /// The aim bend's pitch, clamped (`setAimPitch`).
    pub aim_pitch: f32,
    /// The bent spine joints: their pose before the bend, and after.
    aim_applied: Vec<(usize, Quat, Quat)>,
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
    /// Whether each node is the Spine or under it (`getUpperBodyTargets`).
    upper_body: Vec<bool>,
    aim: PlayerAimPose,
    /// The standing fit of a packaged body (`modelFit`).
    pub fit: Option<PlayerModelFit>,
    /// The weapon hand under `mixamorig:RightHand` (`handBasis`).
    pub hand_basis: Option<HandBasis>,
    pub gun_support: Option<GunSupport>,
    pub blade_stance: Option<BladeStance>,
}

impl PlayerModel {
    /// Loads the model and its clips (`loadAssets`); `read` returns a file
    /// by its path. The clips come from `animation_root`: the model's own
    /// root (Vanguard) plays them as they are, another root's are
    /// retargeted. A clip that fails to load is skipped with a warning, as
    /// SHAG's loader does; a missing optional clip quietly.
    pub fn load(
        mut read: impl FnMut(&str) -> Option<Vec<u8>>,
        root: &str,
        model_file: &str,
        anim_files: &[AnimFileConfig],
        animation_root: &str,
    ) -> Result<Self, GlbError> {
        let bytes =
            read(&format!("{root}{model_file}")).ok_or_else(|| GlbError(format!("{root}{model_file} is missing")))?;
        let model = GltfModel::from_glb_rigged(&bytes)?;
        if model.skeleton.skins.is_empty() {
            return Err(GlbError("Player model has no skeleton".into()));
        }
        let skeleton = &model.skeleton;
        let spine = skeleton.find("mixamorig:Spine");
        let upper_body = (0..skeleton.nodes.len())
            .map(|i| {
                let mut at = Some(i);
                let mut depth = 0;
                while let Some(n) = at {
                    if Some(n) == spine {
                        return true;
                    }
                    depth += 1;
                    if depth > skeleton.nodes.len() {
                        break;
                    }
                    at = skeleton.nodes[n].parent;
                }
                false
            })
            .collect();
        let mut out = Self {
            root_motion_nodes: skeleton
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    let name = n.name.to_lowercase();
                    name == "rootnode" || name == "armature" || name == "__root__"
                })
                .map(|(i, _)| i)
                .collect(),
            upper_body,
            aim: PlayerAimPose::new(skeleton),
            model,
            anims: Vec::new(),
            fit: None,
            hand_basis: None,
            gun_support: None,
            blade_stance: None,
        };
        let embedded = AnimationClip::from_glb(&bytes)?;
        if let Some(first) = embedded.first() {
            out.store_animation(first.clone(), "embedded")?;
        }
        if let Some(running) = embedded.iter().find(|c| c.name == "Running") {
            out.store_animation(running.clone(), "run_relaxed")?;
        }
        let retarget = animation_root != root;
        for anim in anim_files {
            let path = format!("{animation_root}{}", anim.file);
            let Some(bytes) = read(&path) else {
                if !anim.optional {
                    eprintln!("[PlayerModelManager] Failed to load {}: {path} is missing", anim.name);
                }
                continue;
            };
            let result = if retarget {
                out.store_retargeted(&bytes, anim)
            } else {
                AnimationClip::from_glb(&bytes).and_then(|clips| {
                    let clip = clips
                        .into_iter()
                        .next()
                        .ok_or_else(|| GlbError(format!("No animation group in {}", anim.file)))?;
                    out.store_animation(clip, anim.name)
                })
            };
            if let Err(e) = result {
                eprintln!("[PlayerModelManager] Failed to load {}: {e}", anim.name);
            }
        }
        if retarget {
            // Slight visual oversize keeps animated silhouettes readable
            // around the fixed player hull.
            out.fit = out
                .template_pose("idle")
                .and_then(|pose| measure_player_model_fit(&pose, PLAYER_HEIGHT * 1.1 / 36.0));
            if let Some(basis) = out.hand_basis {
                out.gun_support = out
                    .template_pose("idle")
                    .and_then(|pose| measure_gun_support(&pose, &basis));
                out.blade_stance = out
                    .template_pose(BLADE_STANCE)
                    .and_then(|pose| measure_blade_stance(&pose, &basis, BLADE_STANCE_LEAN));
            }
        }
        Ok(out)
    }

    /// The template posed at the first frame of `clip` (`withClipPose`).
    fn template_pose(&self, clip: &str) -> Option<TemplatePose<'_>> {
        let anim = self.anims.iter().find(|a| a.name == clip)?;
        TemplatePose::new(&self.model, |locals| write_channels(anim, locals, anim.from, |_| true))
    }

    /// One shared clip file, retargeted onto this body: the weapon hand is
    /// measured from the first clip's rig, and every track must find its
    /// joint unless the body is a compact rig without fingers or toe ends.
    fn store_retargeted(&mut self, bytes: &[u8], anim: &AnimFileConfig) -> Result<(), GlbError> {
        let glb = Glb::parse(bytes)?;
        let source = AnimationClip::from_document(&glb)?
            .into_iter()
            .next()
            .ok_or_else(|| GlbError(format!("No animation group in {}", anim.file)))?;
        let rig = Skeleton::nodes_of(&glb)?;
        let skeleton = &self.model.skeleton;
        if self.hand_basis.is_none() {
            let reference = source
                .channels
                .iter()
                .any(|c| c.target == "mixamorig:RightHand")
                .then(|| rig.find("mixamorig:RightHand"))
                .flatten();
            if let (Some(reference), Some(hand)) = (reference, skeleton.find("mixamorig:RightHand")) {
                let reference_world = rig.global_matrices(&rig.rest())[reference];
                let hand_world = skeleton.global_matrices(&skeleton.rest())[hand];
                // Handedness reflections as well as export scale.
                let (scale, rotation, _) = decompose(hand_world.inverse() * reference_world);
                self.hand_basis = Some(HandBasis { rotation, scale });
            }
        }
        for channel in &source.channels {
            if skeleton.find(&channel.target).is_none() && !may_lack(&channel.target) {
                return Err(GlbError(format!(
                    "Unmapped source animation target: {}",
                    channel.target
                )));
            }
        }
        let retargeted = retarget_animation_group(&source, &rig, skeleton, |name| skeleton.find(name));
        self.store_animation(retargeted, anim.name)
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

    /// Whether a clip is loaded.
    #[must_use]
    pub fn has_animation(&self, name: &str) -> bool {
        self.anims.iter().any(|a| a.name == name)
    }

    /// A clip's index in [`Self::anims`].
    #[must_use]
    pub fn anim_index(&self, name: &str) -> Option<usize> {
        self.anims.iter().position(|a| a.name == name)
    }

    /// The skeleton's placement under the anchor (the instance mesh's
    /// `position` and `scaling`), for a body `scale` world units to a model
    /// unit (36).
    #[must_use]
    pub fn root_matrix(&self, scale: f32) -> Mat4 {
        let fit = self.fit.unwrap_or(PlayerModelFit {
            scale: 1.0,
            offset_y: 0.0,
        });
        Mat4::from_translation(Vec3::new(0.0, scale * fit.offset_y, 0.0))
            * Mat4::from_scale(Vec3::splat(scale * fit.scale))
    }

    /// `modelScaleFactor`: the fit's share of the body's scale.
    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        self.fit.map_or(1.0, |f| f.scale)
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
            upper_frame: 0.0,
            speed_ratio: 1.0,
            upper_share: 1.0,
            playing: false,
            fresh: false,
            upper_fresh: false,
            upper_masked: false,
            stance: None,
            one_shot: None,
            aim_pitch: 0.0,
            aim_applied: Vec::new(),
            hips_bind: hips.map(|h| (h, skeleton.nodes[h].rest.translation)),
        }
    }

    /// Switches to a clip from its first frame at speed 1; a clip already
    /// playing, or one not loaded, changes nothing (`playAnimation`). A
    /// full-body one-shot keeps the new clip stopped until it ends.
    pub fn play_animation(&self, instance: &mut PlayerModelInstance, name: &str) -> bool {
        if instance.current_anim == name {
            return false;
        }
        let Some(index) = self.anim_index(name) else {
            return false;
        };
        instance.clip = Some(index);
        instance.frame = self.anims[index].from;
        instance.upper_frame = instance.frame;
        instance.speed_ratio = 1.0;
        instance.upper_share = 1.0;
        instance.playing = !self.is_full_body_one_shot_playing(instance);
        instance.fresh = true;
        instance.upper_fresh = true;
        instance.current_anim = name.to_owned();
        self.apply(instance);
        true
    }

    /// `setUpperBodyShare`: the upper body plays the locomotion clip at
    /// `share` of its speed. Call after every change of the speed ratio.
    pub fn set_upper_body_share(&self, instance: &mut PlayerModelInstance, share: f64) {
        instance.upper_share = share;
    }

    /// `setAimPitch`: the spine bend toward `pitch`, clamped to
    /// [`AIM_PITCH_LIMIT`]; applied with the next pose.
    pub fn set_aim_pitch(&self, instance: &mut PlayerModelInstance, pitch: f32) {
        instance.aim_pitch = if pitch.is_finite() {
            pitch.clamp(-AIM_PITCH_LIMIT, AIM_PITCH_LIMIT)
        } else {
            0.0
        };
    }

    /// `setUpperBodyStance`: loop `name` on the upper body over the
    /// locomotion clip, which keeps the hips and legs; None returns the upper
    /// body to locomotion. `time_scale` carries the demo clock.
    pub fn set_upper_body_stance(&self, instance: &mut PlayerModelInstance, name: Option<&str>, time_scale: f64) {
        if let Some(current) = instance.stance.as_mut()
            && Some(current.name.as_str()) == name
        {
            current.speed_ratio = time_scale;
            return;
        }
        if instance.stance.is_none() && name.is_none() {
            return;
        }
        instance.stance = None;
        let swinging = instance.one_shot.as_ref().is_some_and(|o| o.upper_body_only);
        let has_upper = self.upper_body.iter().any(|&u| u);
        let clip = name.and_then(|n| self.anim_index(n));
        let usable = clip.filter(|&c| has_upper && self.anims[c].channels.iter().any(|(n, _)| self.upper_body[*n]));
        let (Some(name), Some(clip)) = (name, usable) else {
            if !swinging {
                self.set_upper_body_mask(instance, false);
            }
            return;
        };
        let paused = swinging || self.is_full_body_one_shot_playing(instance);
        instance.stance = Some(StanceLayer {
            name: name.to_owned(),
            clip,
            frame: self.anims[clip].from,
            speed_ratio: time_scale,
            paused,
            fresh: true,
        });
        self.set_upper_body_mask(instance, true);
    }

    /// `setUpperBodyMask`: the mask pauses the upper joints' animatables;
    /// removing it resumes them at the legs' phase (`goToFrame`), without
    /// restarting the stride.
    fn set_upper_body_mask(&self, instance: &mut PlayerModelInstance, masked: bool) {
        if instance.upper_masked
            && !masked
            && let Some(anim) = instance.clip.and_then(|c| self.anims.get(c))
        {
            let legs = instance.frame;
            // Babylon's jump is a clock offset of (frame - paused frame) /
            // speed, which a reversed clip applies the other way: playing
            // backwards, the upper body lands mirrored about its paused frame.
            let speed = instance.speed_ratio * instance.upper_share;
            instance.upper_frame = if speed < 0.0 {
                wrap_frame(2.0 * instance.upper_frame - legs, anim.from, anim.to)
            } else {
                legs
            };
            // goToFrame writes the legs' frame at once; the jump shows from
            // the next frame.
            write_channels(anim, &mut instance.locals, legs, |n| self.upper_body[n]);
        }
        instance.upper_masked = masked;
    }

    /// `playOnce`: an exclusive one-shot, or for the blade swing and the
    /// taunts a spine-and-arms overlay over the running legs.
    pub fn play_once(&self, instance: &mut PlayerModelInstance, name: &str, speed_ratio: f64) -> bool {
        let Some(clip) = self.anim_index(name) else {
            return false;
        };
        self.stop_one_shot(instance, false);
        let upper_body_only = UPPER_BODY_ONE_SHOTS.contains(&name) && self.upper_body.iter().any(|&u| u);
        let anim = &self.anims[clip];
        if !anim
            .channels
            .iter()
            .any(|(n, _)| !upper_body_only || self.upper_body[*n])
        {
            return false;
        }
        instance.one_shot = Some(OneShotLayer {
            clip,
            frame: anim.from,
            speed_ratio,
            base_speed_ratio: speed_ratio,
            upper_body_only,
            ending: None,
            fresh: true,
        });
        if let Some(stance) = instance.stance.as_mut() {
            stance.paused = true;
        }
        if upper_body_only {
            self.set_upper_body_mask(instance, true);
        } else {
            instance.playing = false;
        }
        true
    }

    /// `isFullBodyOneShotPlaying`.
    #[must_use]
    pub fn is_full_body_one_shot_playing(&self, instance: &PlayerModelInstance) -> bool {
        instance.one_shot.as_ref().is_some_and(|o| !o.upper_body_only)
    }

    /// `activeOneShot`: the name of the one-shot playing, if any.
    #[must_use]
    pub fn active_one_shot<'a>(&'a self, instance: &PlayerModelInstance) -> Option<&'a str> {
        instance.one_shot.as_ref().map(|o| self.anims[o.clip].name.as_str())
    }

    /// `isOneShotPlaying`.
    #[must_use]
    pub fn is_one_shot_playing(&self, instance: &PlayerModelInstance) -> bool {
        instance.one_shot.is_some()
    }

    /// `getOneShotDurationMs`: the clip's last key frame read at 30 frames a
    /// second, as SHAG reads it.
    #[must_use]
    pub fn one_shot_duration_ms(&self, name: &str, speed_ratio: f64) -> f64 {
        let Some(anim) = self.anims.iter().find(|a| a.name == name) else {
            return 0.0;
        };
        if speed_ratio <= 0.0 {
            return 0.0;
        }
        let max_frames = anim
            .channels
            .iter()
            .filter_map(|(_, c)| c.times.last())
            .fold(0.0, |m: f64, &t| m.max(f64::from(t) * GLTF_TARGET_FPS));
        max_frames / 30.0 * 1000.0 / speed_ratio
    }

    /// `setOneShotTimeScale`.
    pub fn set_one_shot_time_scale(&self, instance: &mut PlayerModelInstance, time_scale: f64) {
        if let Some(one_shot) = instance.one_shot.as_mut() {
            one_shot.speed_ratio = one_shot.base_speed_ratio * time_scale;
        }
    }

    /// `stopOneShot`: a held stance takes the upper body back, otherwise
    /// locomotion does; after a full-body clip locomotion restarts.
    pub fn stop_one_shot(&self, instance: &mut PlayerModelInstance, resume_locomotion: bool) {
        let active = instance.one_shot.take();
        let has_stance = instance.stance.is_some();
        self.set_upper_body_mask(instance, has_stance && self.upper_body.iter().any(|&u| u));
        if let Some(stance) = instance.stance.as_mut() {
            stance.paused = false;
        }
        if resume_locomotion
            && !active.is_some_and(|a| a.upper_body_only)
            && !instance.playing
            && let Some(clip) = instance.clip
        {
            // A stopped group plays again from its first frame.
            instance.playing = true;
            instance.fresh = true;
            instance.upper_fresh = true;
            instance.frame = self.anims[clip].from;
            instance.upper_frame = instance.frame;
        }
    }

    /// Advances every playing layer by `dt_seconds` at its speed ratio and
    /// poses the rig (Babylon's clock).
    pub fn advance(&self, instance: &mut PlayerModelInstance, dt_seconds: f64) {
        let step = dt_seconds * GLTF_TARGET_FPS;
        if instance.playing
            && let Some(anim) = instance.clip.and_then(|c| self.anims.get(c))
        {
            // A new animatable shows its first frame; a masked one is paused.
            let advance = step * instance.speed_ratio;
            if !std::mem::take(&mut instance.fresh) {
                instance.frame = wrap_frame(instance.frame + advance, anim.from, anim.to);
            }
            if !instance.upper_masked && !std::mem::take(&mut instance.upper_fresh) {
                instance.upper_frame = wrap_frame(
                    instance.upper_frame + advance * instance.upper_share,
                    anim.from,
                    anim.to,
                );
            }
        }
        if let Some(stance) = instance.stance.as_mut()
            && !stance.paused
            && !std::mem::take(&mut stance.fresh)
        {
            let anim = &self.anims[stance.clip];
            stance.frame = wrap_frame(stance.frame + step * stance.speed_ratio, anim.from, anim.to);
        }
        let mut ended = false;
        if let Some(one_shot) = instance.one_shot.as_mut()
            && !std::mem::take(&mut one_shot.fresh)
        {
            let anim = &self.anims[one_shot.clip];
            one_shot.frame += step * one_shot.speed_ratio;
            // Each channel's animatable plays the group's range clamped to
            // its own keys and ends past its own last key.
            let elapsed = one_shot.frame - anim.from;
            let only = one_shot.upper_body_only;
            one_shot.ending = anim.channels.iter().position(|(n, c)| {
                let (Some(&first), Some(&last)) = (c.times.first(), c.times.last()) else {
                    return false;
                };
                let first = (f64::from(first) * GLTF_TARGET_FPS).max(anim.from);
                (!only || self.upper_body[*n]) && elapsed >= f64::from(last) * GLTF_TARGET_FPS - first
            });
            ended = one_shot.ending.is_some();
        }
        self.write_layers(instance);
        if ended {
            // The clip writes its last frame and `onAnimationEnd` hands the
            // joints back within the same animation pass: a held stance
            // resumes from the next frame, while locomotion restarted after
            // a full-body clip joins this pass at its first frame. The aim
            // bend follows (`onAfterAnimationsObservable`).
            self.stop_one_shot(instance, true);
            if instance.playing && std::mem::take(&mut instance.fresh) {
                if !instance.upper_masked {
                    instance.upper_fresh = false;
                }
                let masked = instance.upper_masked;
                let upper = &self.upper_body;
                self.write_locomotion(instance, |n| !(masked && upper[n]));
                self.reset_root_motion(instance);
            }
        }
        self.apply_aim(instance);
    }

    /// Jumps the locomotion clip to a frame (`goToFrame`), the whole body
    /// with it.
    pub fn go_to_frame(&self, instance: &mut PlayerModelInstance, frame: f64) {
        instance.frame = frame;
        instance.upper_frame = frame;
        self.write_locomotion(instance, |_| true);
        self.apply(instance);
    }

    /// Poses one clip at `frame` over the body as it stands, root motion
    /// cancelled and the aim bend applied (`goToFrame` on a death throw's
    /// group, then `cancelRootMotion`).
    pub fn pose_clip(&self, instance: &mut PlayerModelInstance, clip: usize, frame: f64) {
        self.restore_aim(instance);
        if let Some(anim) = self.anims.get(clip) {
            write_channels(anim, &mut instance.locals, frame, |_| true);
        }
        self.reset_root_motion(instance);
        self.apply_aim(instance);
    }

    /// Lets go of every layer: physics owns the bones now.
    pub fn release(&self, instance: &mut PlayerModelInstance) {
        instance.clip = None;
        instance.playing = false;
        instance.stance = None;
        instance.one_shot = None;
        instance.upper_masked = false;
    }

    fn write_locomotion(&self, instance: &mut PlayerModelInstance, keep: impl Fn(usize) -> bool) {
        let Some(anim) = instance.clip.and_then(|c| self.anims.get(c)) else {
            return;
        };
        let (frame, upper_frame) = (instance.frame, instance.upper_frame);
        let upper = &self.upper_body;
        write_channels(anim, &mut instance.locals, frame, |n| keep(n) && !upper[n]);
        write_channels(anim, &mut instance.locals, upper_frame, |n| keep(n) && upper[n]);
    }

    /// Poses every layer: locomotion (without the upper body while it is
    /// masked), the stance, the one-shot, root motion cancelled, then the
    /// aim bend.
    fn apply(&self, instance: &mut PlayerModelInstance) {
        self.write_layers(instance);
        self.apply_aim(instance);
    }

    /// Every layer's channels, the aim bend taken off first and root motion
    /// cancelled after.
    fn write_layers(&self, instance: &mut PlayerModelInstance) {
        self.restore_aim(instance);
        if instance.playing {
            let masked = instance.upper_masked;
            let upper = &self.upper_body;
            self.write_locomotion(instance, |n| !(masked && upper[n]));
        }
        if let Some(stance) = instance.stance.as_ref().filter(|s| !s.paused) {
            let anim = &self.anims[stance.clip];
            write_channels(anim, &mut instance.locals, stance.frame, |n| self.upper_body[n]);
        }
        if let Some(one_shot) = instance.one_shot.as_ref() {
            let anim = &self.anims[one_shot.clip];
            let only = one_shot.upper_body_only;
            let keep = |n: usize| !only || self.upper_body[n];
            match one_shot.ending {
                None => write_channels(anim, &mut instance.locals, one_shot.frame, keep),
                Some(end) => {
                    for (i, (node, channel)) in anim.channels.iter().enumerate().take(end + 1) {
                        if !keep(*node) {
                            continue;
                        }
                        let time = if i == end {
                            channel.times.last().copied().unwrap_or(0.0)
                        } else {
                            (one_shot.frame / GLTF_TARGET_FPS) as f32
                        };
                        write_channel(&mut instance.locals[*node], channel, time);
                    }
                }
            }
        }
        self.reset_root_motion(instance);
    }

    /// `PlayerAimPose.restore`: a pose written since may have replaced it.
    fn restore_aim(&self, instance: &mut PlayerModelInstance) {
        for (joint, base, applied) in instance.aim_applied.drain(..) {
            if let Some(local) = instance.locals.get_mut(joint)
                && local.rotation == applied
            {
                local.rotation = base;
            }
        }
    }

    /// `PlayerAimPose.apply`.
    fn apply_aim(&self, instance: &mut PlayerModelInstance) {
        if instance.aim_pitch != 0.0 {
            let before: Vec<(usize, Quat)> = AIM_JOINTS
                .iter()
                .filter_map(|n| self.model.skeleton.find(n))
                .map(|j| (j, instance.locals[j].rotation))
                .collect();
            self.aim.apply(
                &self.model.skeleton,
                &mut instance.locals,
                Mat4::IDENTITY,
                instance.aim_pitch,
            );
            instance.aim_applied = before
                .into_iter()
                .map(|(j, base)| (j, base, instance.locals[j].rotation))
                .collect();
        }
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

/// Writes a clip's channels for the nodes `keep` accepts at `frame`.
fn write_channels(anim: &StoredAnim, locals: &mut [NodeTransform], frame: f64, keep: impl Fn(usize) -> bool) {
    let time = (frame / GLTF_TARGET_FPS) as f32;
    for (node, channel) in &anim.channels {
        if !keep(*node) {
            continue;
        }
        let Some(local) = locals.get_mut(*node) else {
            continue;
        };
        write_channel(local, channel, time);
    }
}

/// Writes one channel's value at `time` seconds.
fn write_channel(local: &mut NodeTransform, channel: &ClipChannel, time: f32) {
    let v = channel.sample(time);
    match channel.path {
        ChannelPath::Rotation => local.rotation = Quat::from_array(v),
        ChannelPath::Scale => local.scale = Vec3::new(v[0], v[1], v[2]),
        ChannelPath::Translation => local.translation = Vec3::new(v[0], v[1], v[2]),
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
