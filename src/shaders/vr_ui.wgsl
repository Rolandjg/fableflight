// 3D stereoscopic VR UI panel, classic laser pointer beam, and target dot.
// Rendered in display-encoded LDR space (after tonemap, before OpenXR blit)
// with a dedicated short near-plane projection so UI and lasers stay crisp
// and never clip against the orbital globe's distant near plane.

struct VrOv {
  view_proj: mat4x4f,
  eye_pos: vec4f,
  // Floating UI quad in base-relative world coordinates:
  // quad_c = (center.xyz, half_w), quad_r = (right.xyz, half_h), quad_u = (up.xyz, time).
  quad_c: vec4f,
  quad_r: vec4f,
  quad_u: vec4f,
};

@group(0) @binding(0) var<uniform> OV: VrOv;
@group(0) @binding(1) var t_ui: texture_2d<f32>;
@group(0) @binding(2) var s_ui: sampler;

// ---- 1. Floating 3D UI panel quad ------------------------------------------

struct QuadOut {
  @builtin(position) clip: vec4f,
  @location(0) uv: vec2f,
};

@vertex fn vs_quad(@builtin(vertex_index) vi: u32) -> QuadOut {
  var corners = array<vec2f, 6>(
    vec2f(-1.0,  1.0),
    vec2f( 1.0,  1.0),
    vec2f( 1.0, -1.0),
    vec2f(-1.0,  1.0),
    vec2f( 1.0, -1.0),
    vec2f(-1.0, -1.0),
  );
  let c = corners[vi];
  let half_w = OV.quad_c.w;
  let half_h = OV.quad_r.w;
  let pos = OV.quad_c.xyz + OV.quad_r.xyz * (c.x * half_w) + OV.quad_u.xyz * (c.y * half_h);
  let uv = vec2f(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
  return QuadOut(OV.view_proj * vec4f(pos, 1.0), uv);
}

@fragment fn fs_quad(in: QuadOut) -> @location(0) vec4f {
  // t_ui is rendered by egui in premultiplied display-encoded RGBA.
  let c = textureSampleLevel(t_ui, s_ui, in.uv, 0.0);
  if (c.a <= 0.001) {
    discard;
  }
  return c;
}

// ---- 2. Classic VR laser pointer beam (camera-facing ribbon) ---------------

struct BeamIn {
  @builtin(vertex_index) vi: u32,
  @location(0) start_r: vec4f,
  @location(1) end_fade: vec4f,
  @location(2) color: vec4f,
};

struct BeamOut {
  @builtin(position) clip: vec4f,
  @location(0) uv: vec2f,
  @location(1) @interpolate(flat) color: vec4f,
  @location(2) @interpolate(flat) fade_far: f32,
};

@vertex fn vs_beam(v: BeamIn) -> BeamOut {
  // x: -1..1 across width, y: 0..1 from start to end
  var corners = array<vec2f, 6>(
    vec2f(-1.0, 0.0),
    vec2f( 1.0, 0.0),
    vec2f( 1.0, 1.0),
    vec2f(-1.0, 0.0),
    vec2f( 1.0, 1.0),
    vec2f(-1.0, 1.0),
  );
  let c = corners[v.vi];
  let p0 = v.start_r.xyz;
  let p1 = v.end_fade.xyz;
  let rad = v.start_r.w;
  let diff = p1 - p0;
  let len = max(length(diff), 1e-4);
  let axis = diff / len;
  let base_pos = mix(p0, p1, c.y);
  let to_eye = OV.eye_pos.xyz - base_pos;
  var side = cross(axis, to_eye);
  let sl = length(side);
  if (sl < 1e-4) {
    side = cross(axis, vec3f(0.0, 1.0, 0.0));
    if (length(side) < 1e-4) {
      side = cross(axis, vec3f(1.0, 0.0, 0.0));
    }
  }
  side = normalize(side);
  // Keep a crisp minimum angular width so beams projected onto the distant
  // 3D globe sphere stay smooth and visible all the way to the surface target.
  let eye_dist = length(to_eye);
  let width = max(rad * mix(1.0, 1.35, c.y), eye_dist * 0.0014);
  let pos = base_pos + side * (c.x * width);
  return BeamOut(OV.view_proj * vec4f(pos, 1.0), c, v.color, v.end_fade.w);
}

@fragment fn fs_beam(in: BeamOut) -> @location(0) vec4f {
  let d = abs(in.uv.x);
  let along = in.uv.y;
  let core = exp(-d * d * 16.0);
  let halo = pow(max(1.0 - d, 0.0), 2.2);
  let start_fade = smoothstep(0.0, 0.035, along);
  let end_fade = mix(1.0, smoothstep(1.0, 0.12, along), clamp(in.fade_far, 0.0, 1.0));
  let alpha = (core * 0.92 + halo * 0.48) * start_fade * end_fade * in.color.a;
  if (alpha <= 0.002) {
    discard;
  }
  let col = mix(in.color.rgb, vec3f(1.0, 1.0, 1.0), core * 0.78);
  return vec4f(col, clamp(alpha, 0.0, 0.96));
}

// ---- 3. Target reticle dot on UI + controller emitter orbs -----------------

struct DotIn {
  @builtin(vertex_index) vi: u32,
  @location(0) pos_r: vec4f,
  @location(1) norm_style: vec4f,
  @location(2) color: vec4f,
};

struct DotOut {
  @builtin(position) clip: vec4f,
  @location(0) local: vec2f,
  @location(1) @interpolate(flat) color: vec4f,
  @location(2) @interpolate(flat) style: f32,
};

@vertex fn vs_dot(v: DotIn) -> DotOut {
  var corners = array<vec2f, 6>(
    vec2f(-1.0,  1.0),
    vec2f( 1.0,  1.0),
    vec2f( 1.0, -1.0),
    vec2f(-1.0,  1.0),
    vec2f( 1.0, -1.0),
    vec2f(-1.0, -1.0),
  );
  let c = corners[v.vi];
  let center = v.pos_r.xyz;
  let rad = v.pos_r.w;
  let n_in = v.norm_style.xyz;
  let style = v.norm_style.w;

  var right: vec3f;
  var up: vec3f;
  var offset = vec3f(0.0);
  if (length(n_in) > 0.5) {
    let n = normalize(n_in);
    let ref_up = select(vec3f(0.0, 1.0, 0.0), vec3f(1.0, 0.0, 0.0), abs(n.y) > 0.95);
    right = normalize(cross(ref_up, n));
    up = normalize(cross(n, right));
    offset = n * 0.0025;
  } else {
    let to_eye = normalize(OV.eye_pos.xyz - center);
    let ref_up = select(vec3f(0.0, 1.0, 0.0), vec3f(1.0, 0.0, 0.0), abs(to_eye.y) > 0.95);
    right = normalize(cross(ref_up, to_eye));
    up = normalize(cross(to_eye, right));
  }
  let pos = center + offset + (right * c.x + up * c.y) * rad;
  return DotOut(OV.view_proj * vec4f(pos, 1.0), c, v.color, style);
}

@fragment fn fs_dot(in: DotOut) -> @location(0) vec4f {
  let r = length(in.local);
  if (r > 1.0) {
    discard;
  }
  let t = OV.quad_u.w;
  if (in.style < 0.5) {
    // Classic VR laser target dot: bright white core + dark contrast ring + colored glow halo.
    let pressed = clamp(in.color.a, 0.0, 1.0);
    let core_r = mix(0.28, 0.34, pressed);
    let ring_out = mix(0.46, 0.50, pressed);
    let pulse = 0.92 + 0.08 * sin(t * 6.0);
    let halo = pow(max(1.0 - r, 0.0), 1.9) * (0.65 + 0.35 * pressed) * pulse;
    let dark_ring = smoothstep(core_r - 0.04, core_r + 0.04, r) * (1.0 - smoothstep(ring_out - 0.05, ring_out + 0.05, r));
    let core = 1.0 - smoothstep(core_r - 0.06, core_r + 0.04, r);
    var col = in.color.rgb * (0.95 + 0.35 * pressed);
    col = mix(col, vec3f(0.05, 0.07, 0.10), dark_ring * 0.85);
    col = mix(col, vec3f(1.0, 1.0, 1.0), core);
    let alpha = clamp(max(max(core, dark_ring * 0.90), halo), 0.0, 0.98);
    return vec4f(col, alpha);
  } else if (in.style > 1.5) {
    // Surface-projected globe target ring: hugs the curved Earth sphere normal
    // with a crisp outer ring, dark contrast edge, and glowing center pip.
    let strength = clamp(in.color.a, 0.0, 1.0);
    let pulse = 0.94 + 0.06 * sin(t * 5.0);
    let ring_c = mix(0.68, 0.58, strength) * pulse;
    let ring_d = abs(r - ring_c);
    let ring = 1.0 - smoothstep(0.055, 0.115, ring_d);
    let ring_outline = (1.0 - smoothstep(0.11, 0.18, ring_d)) * (1.0 - ring);
    let pip = 1.0 - smoothstep(0.12, 0.20, r);
    let pip_outline = (1.0 - smoothstep(0.19, 0.27, r)) * (1.0 - pip);
    let glow = exp(-ring_d * ring_d * 26.0) * (0.45 + 0.35 * strength);
    var col = in.color.rgb * (1.0 + 0.25 * strength);
    let dark = max(ring_outline, pip_outline);
    col = mix(col, vec3f(0.04, 0.06, 0.10), dark * 0.88);
    col = mix(col, vec3f(1.0, 1.0, 1.0), max(ring * 0.55, pip * 0.92));
    let alpha = clamp(max(max(max(ring, pip), dark * 0.85), glow), 0.0, 0.98);
    return vec4f(col, alpha);
  } else {
    // Controller emitter orb (translucent sphere look).
    let z = sqrt(max(1.0 - r * r, 0.0));
    let fresnel = pow(1.0 - z, 1.9);
    let core = pow(z, 2.0);
    let col = mix(in.color.rgb, vec3f(1.0, 1.0, 1.0), fresnel * 0.55 + core * 0.35);
    let alpha = clamp(mix(0.32, 0.88, fresnel) + core * 0.25, 0.0, 0.92) * in.color.a;
    return vec4f(col, alpha);
  }
}
