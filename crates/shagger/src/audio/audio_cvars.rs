//! The cvar half of SHAG `src/engine/AudioManager.ts` (`registerCvars`):
//! `s_volume` and `s_musicenabled` for the music, `s_sfxvolume` and
//! `s_sfxenabled` for the effects, and Warsow's channel shares
//! (`cg_volume_*`). The browser applies each in its `onChange`; here
//! [`apply_audio_cvars`] applies them all when the registry changed.

use crate::audio::audio_v2_sfx::AudioV2Sfx;
use crate::audio::mixer::SfxChannel;
use crate::audio::music::MusicPlayer;
use crate::core::cvar::{CvarOptions, CvarType, Cvars};

/// `CHANNEL_CVARS`.
pub const CHANNEL_CVARS: [(&str, SfxChannel, &str); 4] = [
    (
        "cg_volume_effects",
        SfxChannel::Effects,
        "Weapons, impacts and the world (0-1)",
    ),
    (
        "cg_volume_players",
        SfxChannel::Players,
        "Other players' voices and steps (0-1)",
    ),
    (
        "cg_volume_hitsound",
        SfxChannel::Hits,
        "Hit and kill confirmation (0-1)",
    ),
    ("cg_volume_announcer", SfxChannel::Announcer, "The announcer (0-1)"),
];
const DEFAULT_SFX_VOLUME: f64 = 1.0;
/// `musicVolume`'s starting value.
const DEFAULT_MUSIC_VOLUME: f64 = 0.5;

/// `registerCvars`.
pub fn register_audio_cvars(cvars: &mut Cvars) {
    let float = |d: &str| CvarOptions::describe(d).range(0.0, 1.0).kind(CvarType::Float);
    let boolean = |d: &str| CvarOptions::describe(d).kind(CvarType::Bool);
    cvars.register("s_volume", DEFAULT_MUSIC_VOLUME, float("Music volume (0-1)"));
    cvars.register("s_musicenabled", true, boolean("Music enabled (0/1)"));
    cvars.register("s_sfxvolume", DEFAULT_SFX_VOLUME, float("Sound effects volume (0-1)"));
    cvars.register("s_sfxenabled", true, boolean("Sound effects enabled (0/1)"));
    for (name, _, description) in CHANNEL_CVARS {
        cvars.register(name, 1.0, float(description));
    }
}

/// Every audio cvar onto the mix and the music.
pub fn apply_audio_cvars(
    cvars: &Cvars,
    sfx: &mut AudioV2Sfx,
    music: &mut MusicPlayer,
    fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) {
    sfx.set_volume(cvars.get_number("s_sfxvolume") as f32);
    // `sfxEnabled`: off, no effect plays (`playSfx*` return) and the loops
    // fall silent.
    let enabled = cvars.get_bool("s_sfxenabled");
    sfx.set_muted(!enabled);
    sfx.set_loops_muted(!enabled);
    for (name, channel, _) in CHANNEL_CVARS {
        sfx.set_channel_volume(channel, cvars.get_number(name) as f32);
    }
    music.set_music_volume(sfx, cvars.get_number("s_volume") as f32);
    music.set_music_enabled(sfx, fetch, cvars.get_bool("s_musicenabled"));
}
