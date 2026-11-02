//! Audio (SHAG `src/engine/AudioManager.ts` and `src/engine/audio`, RUST.md
//! 9 and 22): decoding, the spatial model, the mixer and its buses, the
//! sound card, and `AudioV2Sfx` in front of them.

pub mod audio_v2_sfx;
pub mod decode;
pub mod device;
pub mod mixer;
pub mod spatial;
