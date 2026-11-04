// SHAG src/engine/weapon/HitscanEffects.ts's wall marks (MIT): thin instances
// of one disc of three segments (a triangle) under a StandardMaterial lit by
// nothing, its emissive the first mark's colour times 0.3, alpha 0.7, the
// instance colour's alpha fading each mark out. Instances:
//   i0: center.xyz, size   i1: the disc's x axis   i2: its y axis
//   i3: emissive.rgb, alpha


struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) color: vec4f,
    // vAtmWorld, for world fog.
    @location(1) atm_world: vec3f,
};

@vertex
fn vs_main(in: QuadIn) -> VertexOut {
    let size = in.i0.w;
    let world_position = in.i0.xyz + (in.i1.xyz * in.position.x + in.i2.xyz * in.position.y) * size;
    var out: VertexOut;
    out.clip = globals.view_proj * vec4f(world_position, 1.0);
    out.color = in.i3;
    out.atm_world = world_position;
    return out;
}

// ALPHA_COMBINE written premultiplied, the colour fogged as a blended
// standard material is (mixed toward the fog).
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let color = atm_fog_apply(globals.fog, in.color.rgb, in.atm_world, globals.camera.xyz, 0.0, false);
    return vec4f(color * in.color.a, in.color.a);
}
