// Translucent 3D VR hand indicator orbs:
//   mode 0 = idle (semi-transparent warm yellow orb)
//   mode 1 = hover / grabbable (brightly glowing pulsing golden-yellow orb)
//   mode 2 = grabbing / interacting (vivid locked-in cyan-blue orb)

struct VIn {
  @location(0) pos: vec3f,
  @location(1) inst_pos_r: vec4f,
  @location(2) inst_state: vec4f,
};

struct VOut {
  @builtin(position) clip: vec4f,
  @location(0) norm: vec3f,
  @location(1) world_pos: vec3f,
  @location(2) @interpolate(flat) state: vec4f,
};

@vertex fn vs(v: VIn) -> VOut {
  let center = v.inst_pos_r.xyz;
  let radius = v.inst_pos_r.w;
  let world_pos = center + v.pos * radius;
  let clip = G.view_proj * vec4f(world_pos, 1.0);
  return VOut(clip, v.pos, world_pos, v.inst_state);
}

@fragment fn fs(in: VOut) -> @location(0) vec4f {
  let N = normalize(in.norm);
  let V = normalize(-in.world_pos);
  let ndv = clamp(dot(N, V), 0.0, 1.0);
  let fresnel = pow(1.0 - ndv, 2.2);
  let core = pow(ndv, 1.8);

  let mode = in.state.x;
  let pulse = in.state.y;
  let grip = in.state.z;
  let t = G.time.x;

  // Compensate scene exposure so orbs stay legible by day and comfortable at night.
  let exp_scale = 0.72 / max(G.time.z, 0.22);

  var base_col: vec3f;
  var rim_col: vec3f;
  var emissive: f32;
  var alpha: f32;

  if (mode < 0.5) {
    // Mode 0: Idle — calm semi-transparent yellow orb.
    base_col = vec3f(0.95, 0.76, 0.14);
    rim_col = vec3f(1.0, 0.90, 0.35);
    emissive = 0.65 + 0.15 * grip;
    alpha = mix(0.20, 0.58, fresnel);
  } else if (mode < 1.5) {
    // Mode 1: Hover / Can Grab — glowing pulsing golden-yellow orb.
    let breathe = 0.5 + 0.5 * sin(t * 7.0);
    base_col = vec3f(1.0, 0.85, 0.18);
    rim_col = vec3f(1.0, 0.97, 0.62);
    emissive = (1.55 + 0.45 * breathe) * (0.9 + 0.2 * pulse);
    alpha = mix(0.34, 0.86, fresnel) + 0.08 * core;
  } else {
    // Mode 2: Holding / Interacting — distinct vivid cyan-blue active mode.
    let band = smoothstep(0.82, 0.98, 1.0 - abs(N.y));
    base_col = vec3f(0.10, 0.78, 1.0);
    rim_col = vec3f(0.68, 0.96, 1.0);
    emissive = 1.35 + 0.35 * band + 0.30 * pulse;
    alpha = mix(0.32, 0.84, fresnel) + 0.10 * band;
  }

  let col = mix(base_col, rim_col, fresnel * 0.75 + core * 0.20) * emissive * exp_scale;
  return vec4f(col, clamp(alpha, 0.0, 0.92));
}
