//! SHAG `src/shared/entities/ProjectileDef.ts`: a projectile type, with the
//! `EntityDef` fields a projectile uses.

/// An `{ r, g, b }` colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

/// `ProjectileDef`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileDef {
    pub id: &'static str,
    pub name: &'static str,
    pub classname: Option<&'static str>,
    pub model_path: Option<&'static str>,
    pub texture_paths: &'static [&'static str],
    pub scale: Option<f64>,
    // Server-authoritative.
    /// Units per second.
    pub speed: f64,
    pub damage: f64,
    pub splash_damage: Option<f64>,
    pub splash_radius: Option<f64>,
    /// Seconds before despawning.
    pub lifetime: f64,
    /// Gravity multiplier.
    pub gravity: Option<f64>,
    pub bounces: Option<u32>,
    pub bounce_factor: Option<f64>,
    // Client visuals.
    pub light_color: Option<DefColor>,
    pub light_intensity: Option<f64>,
    pub light_range: Option<f64>,
    pub trail_effect: Option<&'static str>,
    /// World units; without it the trail spans its last 60 frames.
    pub trail_length: Option<f64>,
    pub impact_effect: Option<&'static str>,
    pub impact_model_path: Option<&'static str>,
    pub impact_texture_path: Option<&'static str>,
    pub impact_scale: Option<f64>,
    pub impact_duration: Option<f64>,
}

/// `DEFAULT_PROJECTILE_BOUNCE_FACTOR`.
pub const DEFAULT_PROJECTILE_BOUNCE_FACTOR: f64 = 0.45;

/// `PROJECTILE_DEFAULTS`, as a definition to fill in with `..`.
pub const PROJECTILE_DEFAULTS: ProjectileDef = ProjectileDef {
    id: "",
    name: "",
    classname: None,
    model_path: None,
    texture_paths: &[],
    scale: None,
    speed: 2000.0,
    damage: 15.0,
    splash_damage: None,
    splash_radius: None,
    lifetime: 5.0,
    gravity: Some(0.0),
    bounces: Some(0),
    bounce_factor: Some(DEFAULT_PROJECTILE_BOUNCE_FACTOR),
    light_color: None,
    light_intensity: Some(5.0),
    light_range: Some(20.0),
    trail_effect: None,
    trail_length: None,
    impact_effect: None,
    impact_model_path: None,
    impact_texture_path: None,
    impact_scale: None,
    impact_duration: None,
};
