//! SHAG `src/engine/weapon/explosion/ExplosionBatchMaterial.ts`: the layers,
//! their timing constants and their shader (`explosion_batch.wgsl`).

/// The three explosion layers, each one instanced mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplosionLayer {
    Fireball,
    Ray,
    Spark,
}

/// Explosion time over which the fireball and rays play out.
pub const EXPLOSION_FIRE_TIME: f64 = 0.6;
/// Babylon particle age units per second (0.01 per frame at 60 fps).
pub const SPARK_AGE_PER_SECOND: f64 = 0.6;
pub const SPARK_GRAVITY: f64 = -30.0;

/// The layers' vertex and fragment shaders.
pub const EXPLOSION_BATCH_WGSL: &str = include_str!("explosion_batch.wgsl");

impl ExplosionLayer {
    /// `createExplosionLayerMaterial`: the layer's vertex entry point.
    #[must_use]
    pub fn vertex_entry(self) -> &'static str {
        match self {
            Self::Fireball => "vs_fireball",
            Self::Ray => "vs_ray",
            Self::Spark => "vs_spark",
        }
    }

    /// Sparks add (`ALPHA_ADD`); the fireball and rays blend over
    /// (`ALPHA_COMBINE`).
    #[must_use]
    pub fn additive(self) -> bool {
        self == Self::Spark
    }

    /// Rays keep their cone's back faces culled; the fireball and sparks
    /// draw both sides.
    #[must_use]
    pub fn back_face_culling(self) -> bool {
        self == Self::Ray
    }
}
