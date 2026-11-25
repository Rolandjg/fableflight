//! VR menu laser pointer, 3D floating UI quad placement, two-handed pinch/pan
//! globe navigation, and optional compact on-screen VR keyboard (working
//! alongside the user's physical PC keyboard).

use crate::geo;
use crate::menu;
use crate::vr::{VrLaserBeam, VrLaserDot, VrOverlay, VrUiQuad, XrHandState, XrViewBase};
use crate::vr_cockpit::HapticCmd;
use glam::{DVec3, Vec3};

/// Snapshot of the 3D globe camera passed to [`VrMenu::step`] on [`MenuKind::Globe`],
/// enabling true 3D ray-sphere projection of the laser onto the curved Earth surface.
#[derive(Copy, Clone, Debug)]
pub struct GlobeView {
    /// Camera center latitude/longitude in degrees (`menu.lat`, `menu.lon`).
    pub lat: f64,
    pub lon: f64,
    /// Camera altitude above Earth surface in metres (`menu.alt`).
    pub alt: f64,
    /// Currently chosen take-off spot `(lat_deg, lon_deg)`, if any.
    pub picked: Option<(f64, f64)>,
    /// Recentered head position in seated VR space (metres).
    pub head_pos: Vec3,
}

/// Result of intersecting a VR laser ray with the 3D Earth sphere.
#[derive(Copy, Clone, Debug)]
pub struct GlobeHit {
    /// 3D hit point on the concentric virtual Earth sphere in seated VR space.
    pub pos: Vec3,
    /// Outward surface normal of the Earth sphere at `pos` (seated VR space, unit length).
    pub normal: Vec3,
    /// Exact geographic coordinates `(lat_deg, lon_deg)` on Earth under the hit point.
    pub lat: f64,
    pub lon: f64,
    /// Unit direction from `head_pos` to `pos` in seated VR space.
    pub eye_dir: Vec3,
    /// Distance from `head_pos` to `pos` in metres.
    pub eye_dist: f32,
}

/// Which kind of VR menu screen is currently active (determines panel distance & globe gestures).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// Start menu main screen, aircraft picker, or settings screen.
    FullScreen,
    /// Start menu globe (`Screen::Fly`): UI quad aligns 1:1 with the 60° globe view
    /// and open space between panels supports laser picking, grab-panning, and two-hand pinch zoom.
    Globe,
    /// In-flight floating UI (pause menu, flight info panel, go-to dialog, loading screen).
    InFlight,
}

/// Outputs produced by one frame of VR menu / globe interaction.
#[derive(Clone, Debug, Default)]
pub struct VrMenuOut {
    /// Synthetic egui pointer/wheel/key events to append to `RawInput::events`.
    pub egui_events: Vec<egui::Event>,
    /// Current hit position on the UI canvas in logical egui points (for mirror dot / hover).
    pub pointer_logical: Option<egui::Pos2>,
    /// Whether the active laser trigger is currently pressed.
    pub trigger_down: bool,
    /// Globe pan delta in physical pixels (`menu.drag(dx, dy, ...)`).
    pub globe_drag: Option<(f64, f64)>,
    /// Globe zoom delta (`menu.zoom(d)`): positive zooms in, negative zooms out.
    pub globe_zoom: f64,
    /// Single click on the open 3D globe in exact geographic `(lat_deg, lon_deg)`.
    pub globe_pick: Option<(f64, f64)>,
    /// Live sphere-projected hover point `(lat_deg, lon_deg)` when aiming at the open globe.
    pub globe_hover: Option<(f64, f64)>,
    /// Navigate back / close active overlay (Right B button).
    pub back: bool,
    /// Open or toggle the pause menu / step back (Left Menu button).
    pub menu: bool,
    /// Recenter seated VR view (thumbstick click or left Y button).
    pub recenter: bool,
    /// One-shot haptic feedback pulses for the controllers.
    pub haptics: Vec<HapticCmd>,
    /// 3D floating panel + laser beams + target dot in recentered seated space.
    /// Transform into camera-relative world space via [`VrMenuOut::to_world_overlay`].
    pub panel: Option<VrUiQuad>,
    pub beams: Vec<VrLaserBeam>,
    pub dots: Vec<VrLaserDot>,
    /// Whether two-handed globe pinch is currently active.
    pub pinching: bool,
}

impl VrMenuOut {
    /// Converts seated-space overlay primitives into camera-relative world coordinates
    /// matching `crate::vr::eye_matrices(base, ...)`.
    pub fn to_world_overlay(&self, base: XrViewBase, real_time: f32) -> VrOverlay {
        let to_w = |p: Vec3| base.cockpit_rot * (p - base.head_pos);
        let panel = self.panel.map(|q| VrUiQuad {
            center: to_w(q.center),
            right: (base.cockpit_rot * q.right).normalize_or_zero(),
            up: (base.cockpit_rot * q.up).normalize_or_zero(),
            half_w: q.half_w,
            half_h: q.half_h,
        });
        let beams = self
            .beams
            .iter()
            .map(|b| {
                let s = to_w(Vec3::new(b.start_r[0], b.start_r[1], b.start_r[2]));
                let e = to_w(Vec3::new(b.end_fade[0], b.end_fade[1], b.end_fade[2]));
                VrLaserBeam {
                    start_r: [s.x, s.y, s.z, b.start_r[3]],
                    end_fade: [e.x, e.y, e.z, b.end_fade[3]],
                    color: b.color,
                }
            })
            .collect();
        let dots = self
            .dots
            .iter()
            .map(|d| {
                let p = to_w(Vec3::new(d.pos_r[0], d.pos_r[1], d.pos_r[2]));
                let n_raw = Vec3::new(d.norm_style[0], d.norm_style[1], d.norm_style[2]);
                let n = if n_raw.length_squared() > 0.25 {
                    (base.cockpit_rot * n_raw).normalize_or_zero()
                } else {
                    Vec3::ZERO
                };
                VrLaserDot {
                    pos_r: [p.x, p.y, p.z, d.pos_r[3]],
                    norm_style: [n.x, n.y, n.z, d.norm_style[3]],
                    color: d.color,
                }
            })
            .collect();
        VrOverlay { panel, beams, dots, time: real_time }
    }
}

/// Trigger/grip hysteresis thresholds (crisp VR laser click feel).
const BTN_ON: f32 = 0.45;
const BTN_OFF: f32 = 0.25;

/// Logical-point deadzone around a UI trigger press so natural wrist tremor while
/// squeezing/releasing an analog VR trigger never cancels a button click, while
/// deliberate slider drags past this distance unlock immediately.
const CLICK_ANCHOR_PT: f32 = 24.0;
/// Maximum time (seconds) to keep the UI trigger-press stabilization anchor active.
const CLICK_LOCK_SECS: f64 = 0.45;

/// Sensitivity of two-handed pinch-to-zoom (`ln(d1 / d0) * PINCH_ZOOM_SENS`).
const PINCH_ZOOM_SENS: f64 = 10.5;
/// Sensitivity of direct 3D hand grab-panning on the globe (screen heights per metre of hand movement).
const HAND_PAN_SCALE: f64 = 1.65;
/// Net angular laser motion (radians) from press origin before a globe trigger hold becomes a drag.
const GLOBE_DRAG_RAD: f32 = 0.052; // ~3.0 degrees
/// Nearest surface distance (m) of the concentric scaled Earth sphere used for 3D VR globe projection.
/// Far enough for zero stereo disparity against the orbital planet and <0.4° hand-eye parallax,
/// while staying well within floating-point precision in `ov_vp`.
const GLOBE_DRAW_DIST: f64 = 50.0;

#[derive(Copy, Clone, Debug, Default)]
struct HandPrev {
    trig: bool,
    grip: bool,
    secondary: bool,
    menu: bool,
    stick_click: bool,
}

#[derive(Copy, Clone, Debug)]
struct PinchState {
    prev_dist: f32,
    prev_mid: Vec3,
    zoom_tick_acc: f32,
    pan_tick_acc: f32,
}

#[derive(Copy, Clone, Debug)]
struct GripDragState {
    hand: usize,
    prev_pos: Vec3,
    tick_acc: f32,
}

#[derive(Copy, Clone, Debug)]
struct LaserGlobePress {
    hand: usize,
    press_lat_lon: (f64, f64),
    start_dir: Vec3,
    last_dir: Vec3,
    dragging: bool,
}

/// Manages VR laser menu navigation, globe pinch/drag gestures, and the optional VR keyboard.
pub struct VrMenu {
    active_hand: usize,
    prev: [HandPrev; 2],
    /// Smoothed UV coordinates `(0..1, 0..1)` on the UI quad for the active laser.
    smooth_uv: Option<(f32, f32)>,
    /// Smoothed laser ray direction when projecting onto the 3D Earth sphere.
    smooth_globe_dir: Option<Vec3>,
    /// Trigger-down stabilization anchor so pulling the trigger does not jitter off small buttons.
    click_anchor_uv: Option<(f32, f32)>,
    click_lock_until: f64,
    /// Whether the active trigger press started on an egui UI widget/panel (vs open globe).
    ui_press_active: bool,
    /// Active two-handed pinch zoom/pan gesture on the globe.
    pinch: Option<PinchState>,
    /// Active single-handed grip drag gesture on the globe.
    grip_drag: Option<GripDragState>,
    /// Active single-handed laser trigger press/drag on the open globe.
    globe_press: Option<LaserGlobePress>,
    /// Internal clock (seconds).
    time: f64,
    /// Optional VR on-screen keyboard state.
    pub kb_hidden_for: Option<egui::Id>,
    pub kb_target: Option<egui::Id>,
    pub kb_shift: bool,
    pub kb_pending: Vec<egui::Event>,
    /// Re-focus this text edit ID on the next UI pass after a virtual key click.
    pub kb_refocus: Option<egui::Id>,
}

impl Default for VrMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl VrMenu {
    pub fn new() -> Self {
        Self {
            active_hand: 1,
            prev: [HandPrev::default(); 2],
            smooth_uv: None,
            smooth_globe_dir: None,
            click_anchor_uv: None,
            click_lock_until: 0.0,
            ui_press_active: false,
            pinch: None,
            grip_drag: None,
            globe_press: None,
            time: 0.0,
            kb_hidden_for: None,
            kb_target: None,
            kb_shift: false,
            kb_pending: Vec::new(),
            kb_refocus: None,
        }
    }

    /// Resets transient drag/pinch states when leaving menu mode and syncs button
    /// edge state so holding B / Menu / Stick Click across the transition never bounces.
    pub fn reset(&mut self, hands: [XrHandState; 2]) {
        self.smooth_uv = None;
        self.smooth_globe_dir = None;
        self.click_anchor_uv = None;
        self.ui_press_active = false;
        self.pinch = None;
        self.grip_drag = None;
        self.globe_press = None;
        for hi in 0..2 {
            let h = hands[hi];
            let p = &mut self.prev[hi];
            p.trig = h.trigger > BTN_ON || h.primary_btn;
            p.grip = h.squeeze > BTN_ON;
            p.secondary = h.secondary_btn;
            p.menu = h.menu_btn;
            p.stick_click = h.thumbstick_click;
        }
    }

    /// Computes the floating 3D UI quad in the recentered seated frame for `kind`.
    ///
    /// On [`MenuKind::Globe`], the floating menu panels sit on the `z = -1.80 m` quad
    /// while globe text labels, markers, and laser picks project directly in 3D onto the Earth.
    pub fn panel_quad(kind: MenuKind, win_w: f32, win_h: f32) -> VrUiQuad {
        let aspect = (win_w.max(1.0) / win_h.max(1.0)).clamp(0.5, 3.5);
        match kind {
            MenuKind::Globe => {
                let dist = 1.80f32;
                let half_h = dist * (30.0f32.to_radians()).tan();
                let half_w = half_h * aspect;
                VrUiQuad {
                    center: Vec3::new(0.0, 0.0, -dist),
                    right: Vec3::X,
                    up: Vec3::Y,
                    half_w,
                    half_h,
                }
            }
            MenuKind::FullScreen => {
                let dist = 1.75f32;
                let base_h = 0.62f32;
                let half_w = (base_h * aspect).min(1.22);
                let half_h = half_w / aspect;
                VrUiQuad {
                    center: Vec3::new(0.0, -0.02, -dist),
                    right: Vec3::X,
                    up: Vec3::Y,
                    half_w,
                    half_h,
                }
            }
            MenuKind::InFlight => {
                let dist = 1.15f32;
                let base_h = 0.44f32;
                let half_w = (base_h * aspect).min(0.84);
                let half_h = half_w / aspect;
                VrUiQuad {
                    center: Vec3::new(0.0, -0.04, -dist),
                    right: Vec3::X,
                    up: Vec3::Y,
                    half_w,
                    half_h,
                }
            }
        }
    }

    /// Intersects a ray `(origin, dir)` with `quad`. Returns `Some((t, hit_pos, u, v))`
    /// when the ray hits the front face inside `u, v in [0, 1]`.
    pub fn intersect_quad(origin: Vec3, dir: Vec3, quad: VrUiQuad) -> Option<(f32, Vec3, f32, f32)> {
        let dir = dir.normalize_or_zero();
        if dir.length_squared() < 0.5 {
            return None;
        }
        let normal = quad.right.cross(quad.up).normalize_or_zero();
        let denom = dir.dot(normal);
        if denom >= -1e-4 {
            return None;
        }
        let t = (quad.center - origin).dot(normal) / denom;
        if !(0.02..=12.0).contains(&t) {
            return None;
        }
        let hit = origin + dir * t;
        let rel = hit - quad.center;
        let u = 0.5 + rel.dot(quad.right) / (2.0 * quad.half_w.max(1e-4));
        let v = 0.5 - rel.dot(quad.up) / (2.0 * quad.half_h.max(1e-4));
        if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
            Some((t, hit, u, v))
        } else {
            None
        }
    }

    /// Intersects a VR laser ray `(origin, dir)` in seated space with the 3D Earth sphere
    /// matching the rendered globe silhouette from `globe.head_pos`.
    ///
    /// All sphere math runs in `f64` using a cancellation-free quadratic so accuracy holds
    /// from orbital view (`26,000 km`) all the way down to individual runways (`900 m`).
    pub fn intersect_globe(origin: Vec3, dir: Vec3, globe: GlobeView) -> Option<GlobeHit> {
        let dir_d = dir.as_dvec3().normalize_or_zero();
        if dir_d.length_squared() < 0.5 {
            return None;
        }
        let alt = globe.alt.max(100.0);
        let k = GLOBE_DRAW_DIST / alt;
        let r_s = geo::R * k;
        let head_d = globe.head_pos.as_dvec3();
        let rel = origin.as_dvec3() - head_d;
        let center_z = -(GLOBE_DRAW_DIST + r_s);
        let oc = DVec3::new(rel.x, rel.y, rel.z - center_z);
        let b = oc.dot(dir_d);
        // `oc.z - r_s == rel.z + GLOBE_DRAW_DIST`, avoiding catastrophic cancellation at low alt.
        let c = oc.x * oc.x + oc.y * oc.y + (rel.z + GLOBE_DRAW_DIST) * (oc.z + r_s);
        let disc = b * b - c;
        if disc < 0.0 {
            return None;
        }
        let t = -b - disc.sqrt();
        if t < 0.05 {
            return None;
        }
        let hit_rel = rel + dir_d * t;
        let hit_pos = head_d + hit_rel;
        let normal_d = (DVec3::new(hit_rel.x, hit_rel.y, hit_rel.z - center_z) / r_s).normalize();
        let cam = geo::lla_to_pos(globe.lat.to_radians(), globe.lon.to_radians(), alt);
        let (e, n, u) = geo::enu(cam);
        let v_world = hit_rel / k;
        let p_ecef = cam + e * v_world.x + n * v_world.y + u * v_world.z;
        let (lat_r, lon_r, _) = geo::pos_to_lla(p_ecef);
        let eye_dist = hit_rel.length();
        let eye_dir = (hit_rel / eye_dist.max(1e-6)).as_vec3();
        Some(GlobeHit {
            pos: hit_pos.as_vec3(),
            normal: normal_d.as_vec3(),
            lat: lat_r.to_degrees(),
            lon: lon_r.to_degrees(),
            eye_dir,
            eye_dist: eye_dist as f32,
        })
    }

    /// Projects a geographic point `(lat_deg, lon_deg)` onto the concentric 3D Earth sphere
    /// in seated VR space when it lies on the visible front hemisphere.
    ///
    /// Returns `(pos, outward_normal, eye_dir, eye_dist)`.
    pub fn globe_point_on_sphere(lat: f64, lon: f64, globe: GlobeView) -> Option<(Vec3, Vec3, Vec3, f32)> {
        let alt = globe.alt.max(100.0);
        let k = GLOBE_DRAW_DIST / alt;
        let cam = geo::lla_to_pos(globe.lat.to_radians(), globe.lon.to_radians(), alt);
        let (e, n, u) = geo::enu(cam);
        let p_ecef = geo::lla_to_pos(lat.to_radians(), lon.to_radians(), 0.0);
        let n_world = p_ecef.normalize();
        if n_world.dot((cam - p_ecef).normalize()) < 0.03 {
            return None;
        }
        let v_ecef = p_ecef - cam;
        let hit_rel = DVec3::new(v_ecef.dot(e), v_ecef.dot(n), v_ecef.dot(u)) * k;
        let eye_dist = hit_rel.length();
        if eye_dist < 1e-4 {
            return None;
        }
        let pos = globe.head_pos + hit_rel.as_vec3();
        let normal = Vec3::new(
            n_world.dot(e) as f32,
            n_world.dot(n) as f32,
            n_world.dot(u) as f32,
        )
        .normalize_or_zero();
        let eye_dir = (hit_rel / eye_dist).as_vec3();
        Some((pos, normal, eye_dir, eye_dist as f32))
    }

    /// Projects an eye-ray direction (from the viewer) onto the 2D `egui` UI quad in logical points.
    fn project_dir_to_quad(eye_dir: Vec3, quad: VrUiQuad, win_w: f32, win_h: f32, ppp: f32) -> Option<egui::Pos2> {
        let (_, _, u, v) = Self::intersect_quad(Vec3::ZERO, eye_dir, quad)?;
        Some(egui::pos2((u * win_w) / ppp, (v * win_h) / ppp))
    }

    /// Updates VR menu laser pointing, clicking, scrolling, 3D globe sphere projection,
    /// and globe pinch/pan gestures.
    ///
    /// `is_over_ui` tests whether a logical `egui` point lands on an interactive UI panel/widget
    /// (evaluated directly for the current frame's ray hit on [`MenuKind::Globe`], eliminating
    /// one-frame hover lag). On [`MenuKind::FullScreen`] and [`MenuKind::InFlight`], UI takes all clicks.
    #[allow(clippy::too_many_arguments)]
    pub fn step<F: Fn(egui::Pos2) -> bool>(
        &mut self,
        dt: f64,
        kind: MenuKind,
        hands: [XrHandState; 2],
        win_w: f32,
        win_h: f32,
        ppp: f32,
        globe: Option<GlobeView>,
        is_over_ui: F,
    ) -> VrMenuOut {
        let mut out = VrMenuOut::default();
        let dt = dt.clamp(0.0, 0.1);
        self.time += dt;
        let quad = Self::panel_quad(kind, win_w, win_h);
        out.panel = Some(quad);

        // Drain any queued virtual keyboard events first.
        out.egui_events.append(&mut self.kb_pending);

        // Compute per-hand button edges with hysteresis.
        let mut trig = [false; 2];
        let mut grip = [false; 2];
        let mut trig_pressed = [false; 2];
        let mut trig_released = [false; 2];
        let mut grip_pressed = [false; 2];

        for hi in 0..2 {
            let h = hands[hi];
            let p = &mut self.prev[hi];

            // Trigger or primary button (A/X) acts as click.
            let t_now = if p.trig {
                h.trigger > BTN_OFF || h.primary_btn
            } else {
                h.trigger > BTN_ON || h.primary_btn
            };
            let g_now = if p.grip { h.squeeze > BTN_OFF } else { h.squeeze > BTN_ON };

            trig[hi] = t_now;
            grip[hi] = g_now;
            trig_pressed[hi] = t_now && !p.trig;
            trig_released[hi] = !t_now && p.trig;
            grip_pressed[hi] = g_now && !p.grip;

            // Right B (secondary_btn on hand 1) or Right Stick Click -> Back / close menu.
            if hi == 1 && ((h.secondary_btn && !p.secondary) || (h.thumbstick_click && !p.stick_click)) {
                out.back = true;
                out.haptics.push(HapticCmd { hand: hi, amplitude: 0.28, frequency: 180.0, duration_ns: 20_000_000 });
            }
            // Left Menu button -> Pause menu / step back.
            if hi == 0 && h.menu_btn && !p.menu {
                out.menu = true;
                out.haptics.push(HapticCmd { hand: hi, amplitude: 0.28, frequency: 180.0, duration_ns: 20_000_000 });
            }
            // Left Y (secondary_btn on hand 0) or Left Stick Click -> Recenter VR.
            if hi == 0 && ((h.secondary_btn && !p.secondary) || (h.thumbstick_click && !p.stick_click)) {
                out.recenter = true;
                out.haptics.push(HapticCmd { hand: hi, amplitude: 0.25, frequency: 180.0, duration_ns: 25_000_000 });
            }

            p.trig = t_now;
            p.grip = g_now;
            p.secondary = h.secondary_btn;
            p.menu = h.menu_btn;
            p.stick_click = h.thumbstick_click;
        }

        // Compute aim rays, UI panel hits, and 3D globe sphere hits for both controllers.
        let is_globe = kind == MenuKind::Globe && globe.is_some();
        let mut hits: [Option<(f32, Vec3, f32, f32)>; 2] = [None; 2];
        let mut globe_hits: [Option<GlobeHit>; 2] = [None; 2];
        let mut origins = [Vec3::ZERO; 2];
        let mut dirs = [Vec3::NEG_Z; 2];
        for hi in 0..2 {
            if hands[hi].aim_tracked {
                origins[hi] = hands[hi].aim_pos;
                dirs[hi] = (hands[hi].aim_rot * Vec3::NEG_Z).normalize_or_zero();
                hits[hi] = Self::intersect_quad(origins[hi], dirs[hi], quad);
                if let Some(g) = globe.filter(|_| is_globe) {
                    globe_hits[hi] = Self::intersect_globe(origins[hi], dirs[hi], g);
                }
            }
        }

        // Active hand switching: prefer whichever hand just pulled trigger, or fall back to tracked hand.
        if !self.ui_press_active && self.globe_press.is_none() {
            for hi in 0..2 {
                if trig_pressed[hi] && hands[hi].aim_tracked && self.active_hand != hi {
                    self.active_hand = hi;
                    self.smooth_uv = hits[hi].map(|(_, _, u, v)| (u, v));
                    self.smooth_globe_dir = None;
                }
            }
            let ah_has_target = hits[self.active_hand].is_some() || globe_hits[self.active_hand].is_some();
            let other = 1 - self.active_hand;
            let other_has_target = hits[other].is_some() || globe_hits[other].is_some();
            if !hands[self.active_hand].aim_tracked && hands[other].aim_tracked {
                self.active_hand = other;
                self.smooth_uv = hits[other].map(|(_, _, u, v)| (u, v));
                self.smooth_globe_dir = None;
            } else if !ah_has_target && !trig[self.active_hand] && other_has_target {
                self.active_hand = other;
                self.smooth_uv = hits[other].map(|(_, _, u, v)| (u, v));
                self.smooth_globe_dir = None;
            }
        }

        // Determine immediately (with zero-frame lag) whether the active ray hits an egui panel.
        let ah = self.active_hand;
        let cand_ui_pos = hits[ah].map(|(_, _, u, v)| egui::pos2((u * win_w) / ppp, (v * win_h) / ppp));
        let effective_over_ui = if !is_globe {
            true
        } else if self.ui_press_active {
            true
        } else if self.globe_press.is_some() {
            false
        } else {
            cand_ui_pos.is_some_and(&is_over_ui)
        };

        // ---- 1. Two-handed pinch zoom & pan on the Globe ----
        let both_tracked = hands[0].tracked && hands[1].tracked;
        let two_hand_grip = both_tracked && grip[0] && grip[1];
        let two_hand_globe_trig = both_tracked
            && !self.ui_press_active
            && !effective_over_ui
            && trig[0]
            && trig[1];
        let two_hand_mix = both_tracked
            && !self.ui_press_active
            && ((grip[0] && trig[1] && !effective_over_ui) || (grip[1] && trig[0] && !effective_over_ui));

        if is_globe && (two_hand_grip || two_hand_globe_trig || two_hand_mix) {
            // Cancel any single-hand drags or clicks cleanly.
            self.grip_drag = None;
            self.globe_press = None;
            self.click_anchor_uv = None;
            out.pinching = true;

            let p0 = hands[0].pos;
            let p1 = hands[1].pos;
            let dist = (p1 - p0).length().max(0.06);
            let mid = (p0 + p1) * 0.5;

            if let Some(ps) = &mut self.pinch {
                let ratio = (dist / ps.prev_dist.max(0.06)).clamp(0.5, 2.0);
                let ln_r = ratio.ln();
                out.globe_zoom += (ln_r as f64) * PINCH_ZOOM_SENS;

                let d_mid = mid - ps.prev_mid;
                let dx = d_mid.x as f64 * HAND_PAN_SCALE * win_h as f64;
                let dy = -d_mid.y as f64 * HAND_PAN_SCALE * win_h as f64;
                if dx.abs() + dy.abs() > 0.01 {
                    out.globe_drag = Some((dx, dy));
                }

                ps.zoom_tick_acc += ln_r.abs();
                ps.pan_tick_acc += d_mid.length();
                if ps.zoom_tick_acc >= 0.11 {
                    ps.zoom_tick_acc = 0.0;
                    for hi in 0..2 {
                        out.haptics.push(HapticCmd {
                            hand: hi,
                            amplitude: 0.22,
                            frequency: if ln_r > 0.0 { 185.0 } else { 135.0 },
                            duration_ns: 16_000_000,
                        });
                    }
                } else if ps.pan_tick_acc >= 0.08 {
                    ps.pan_tick_acc = 0.0;
                    for hi in 0..2 {
                        out.haptics.push(HapticCmd {
                            hand: hi,
                            amplitude: 0.14,
                            frequency: 150.0,
                            duration_ns: 14_000_000,
                        });
                    }
                }
                ps.prev_dist = dist;
                ps.prev_mid = mid;
            } else {
                self.pinch = Some(PinchState {
                    prev_dist: dist,
                    prev_mid: mid,
                    zoom_tick_acc: 0.0,
                    pan_tick_acc: 0.0,
                });
                for hi in 0..2 {
                    out.haptics.push(HapticCmd {
                        hand: hi,
                        amplitude: 0.34,
                        frequency: 160.0,
                        duration_ns: 24_000_000,
                    });
                }
            }

            // Visuals: glowing cyan orbs on both controllers + connecting pinch beam!
            let cyan = [0.18, 0.86, 1.0, 0.92];
            out.beams.push(VrLaserBeam {
                start_r: [p0.x, p0.y, p0.z, 0.0045],
                end_fade: [p1.x, p1.y, p1.z, 0.0],
                color: cyan,
            });
            for p in [p0, p1] {
                out.dots.push(VrLaserDot {
                    pos_r: [p.x, p.y, p.z, 0.022],
                    norm_style: [0.0, 0.0, 0.0, 1.0],
                    color: cyan,
                });
            }
            return out;
        } else {
            self.pinch = None;
        }

        // ---- 2. One-handed Grip grab-and-spin/circle the Globe ----
        if is_globe {
            let grabbing_hand = if let Some(gd) = self.grip_drag {
                grip[gd.hand].then_some(gd.hand)
            } else if grip_pressed[1] && hands[1].tracked {
                Some(1)
            } else if grip_pressed[0] && hands[0].tracked {
                Some(0)
            } else if grip[1] && hands[1].tracked {
                Some(1)
            } else if grip[0] && hands[0].tracked {
                Some(0)
            } else {
                None
            };

            if let Some(gh) = grabbing_hand {
                let pos = hands[gh].pos;
                if let Some(gd) = &mut self.grip_drag {
                    if gd.hand == gh {
                        let d_pos = pos - gd.prev_pos;
                        let dx = d_pos.x as f64 * HAND_PAN_SCALE * win_h as f64;
                        let dy = -d_pos.y as f64 * HAND_PAN_SCALE * win_h as f64;
                        if dx.abs() + dy.abs() > 0.01 {
                            out.globe_drag = Some((dx, dy));
                        }
                        gd.tick_acc += d_pos.length();
                        if gd.tick_acc >= 0.075 {
                            gd.tick_acc = 0.0;
                            out.haptics.push(HapticCmd {
                                hand: gh,
                                amplitude: 0.14,
                                frequency: 145.0,
                                duration_ns: 14_000_000,
                            });
                        }
                        gd.prev_pos = pos;
                    } else {
                        *gd = GripDragState { hand: gh, prev_pos: pos, tick_acc: 0.0 };
                    }
                } else {
                    self.grip_drag = Some(GripDragState { hand: gh, prev_pos: pos, tick_acc: 0.0 });
                    out.haptics.push(HapticCmd {
                        hand: gh,
                        amplitude: 0.28,
                        frequency: 155.0,
                        duration_ns: 20_000_000,
                    });
                }
                self.globe_press = None;
            } else {
                self.grip_drag = None;
            }
        } else {
            self.grip_drag = None;
        }

        // ---- 3. VR Laser Pointer: UI Panel Mode vs 3D Globe Sphere Projection ----
        let normal = quad.right.cross(quad.up).normalize_or_zero();
        let active_trig = trig[ah];
        let active_pressed = trig_pressed[ah];
        let active_released = trig_released[ah];
        out.trigger_down = active_trig;

        if hands[ah].aim_tracked {
            let origin = origins[ah];
            let dir = dirs[ah];

            if effective_over_ui {
                // ---- 3a. Pointing at an egui UI panel/widget ----
                self.smooth_globe_dir = None;
                self.globe_press = None;

                if let Some((_, _raw_hit, raw_u, raw_v)) = hits[ah] {
                    // Velocity-adaptive smoothing: steady on small tremors, snappy on deliberate moves.
                    let (su, sv) = match self.smooth_uv {
                        Some((pu, pv)) => {
                            let dist_uv = (raw_u - pu).hypot(raw_v - pv);
                            let alpha = ((dist_uv - 0.002) / 0.028).clamp(0.0, 1.0);
                            let k = 0.28 + 0.62 * alpha;
                            (pu + (raw_u - pu) * k, pv + (raw_v - pv) * k)
                        }
                        None => (raw_u, raw_v),
                    };
                    self.smooth_uv = Some((su, sv));

                    // Trigger-down stabilization: hold the press point through BOTH the hold
                    // AND the release frame while within the click deadzone, so analog trigger
                    // squeeze/release wobble never trips egui's click-distance cutoff, while
                    // deliberate slider drags past `CLICK_ANCHOR_PT` unlock immediately.
                    if active_pressed {
                        self.click_anchor_uv = Some((su, sv));
                        self.click_lock_until = self.time + CLICK_LOCK_SECS;
                    }
                    let (eu, ev) = if let Some((au, av)) = self.click_anchor_uv {
                        let dist_pt = ((su - au) * win_w / ppp).hypot((sv - av) * win_h / ppp);
                        let limit = if active_released { CLICK_ANCHOR_PT * 1.25 } else { CLICK_ANCHOR_PT };
                        let keep = (active_trig || active_released)
                            && self.time < self.click_lock_until
                            && dist_pt < limit;
                        if keep {
                            (au, av)
                        } else {
                            self.click_anchor_uv = None;
                            (su, sv)
                        }
                    } else {
                        (su, sv)
                    };

                    let px = (eu * win_w) as f64;
                    let py = (ev * win_h) as f64;
                    let egui_pos = egui::pos2((px / ppp as f64) as f32, (py / ppp as f64) as f32);
                    out.pointer_logical = Some(egui_pos);

                    // Always emit PointerMoved so hover rings, tooltips, and highlights stay live.
                    out.egui_events.push(egui::Event::PointerMoved(egui_pos));

                    if active_pressed {
                        self.ui_press_active = true;
                        out.egui_events.push(egui::Event::PointerButton {
                            pos: egui_pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        });
                        out.haptics.push(HapticCmd {
                            hand: ah,
                            amplitude: 0.32,
                            frequency: 210.0,
                            duration_ns: 18_000_000,
                        });
                    }

                    if active_released {
                        self.click_anchor_uv = None;
                        if self.ui_press_active {
                            self.ui_press_active = false;
                            out.egui_events.push(egui::Event::PointerButton {
                                pos: egui_pos,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            });
                            out.haptics.push(HapticCmd {
                                hand: ah,
                                amplitude: 0.14,
                                frequency: 165.0,
                                duration_ns: 12_000_000,
                            });
                        }
                    }

                    // Recompute 3D hit point on quad from stabilized UV so visual dot matches clicks!
                    let hit_pos = quad.center
                        + quad.right * ((eu - 0.5) * 2.0 * quad.half_w)
                        + quad.up * ((0.5 - ev) * 2.0 * quad.half_h);

                    let (beam_col, dot_col) = if active_trig {
                        ([0.16, 0.85, 1.0, 0.95], [0.16, 0.88, 1.0, 1.0])
                    } else {
                        ([0.45, 0.90, 1.0, 0.88], [0.98, 0.82, 0.18, 0.35])
                    };

                    let beam_rad = if active_trig { 0.0024 } else { 0.0019 };
                    let dot_rad = match kind {
                        MenuKind::InFlight => if active_trig { 0.011 } else { 0.014 },
                        _ => if active_trig { 0.016 } else { 0.020 },
                    };

                    out.beams.push(VrLaserBeam {
                        start_r: [origin.x, origin.y, origin.z, beam_rad],
                        end_fade: [hit_pos.x, hit_pos.y, hit_pos.z, 0.0],
                        color: beam_col,
                    });
                    out.dots.push(VrLaserDot {
                        pos_r: [hit_pos.x, hit_pos.y, hit_pos.z, dot_rad],
                        norm_style: [normal.x, normal.y, normal.z, 0.0],
                        color: dot_col,
                    });
                } else {
                    // Pointing away from the panel: release any active UI press cleanly.
                    self.smooth_uv = None;
                    if active_released && self.ui_press_active {
                        self.ui_press_active = false;
                        let release_pos = if let Some((au, av)) = self.click_anchor_uv.take() {
                            if self.time < self.click_lock_until {
                                egui::pos2((au * win_w) / ppp, (av * win_h) / ppp)
                            } else {
                                egui::pos2(-100.0, -100.0)
                            }
                        } else {
                            egui::pos2(-100.0, -100.0)
                        };
                        out.egui_events.push(egui::Event::PointerButton {
                            pos: release_pos,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        });
                    } else if !self.ui_press_active {
                        self.click_anchor_uv = None;
                        out.egui_events.push(egui::Event::PointerGone);
                    }
                    let far = origin + dir * 2.2;
                    out.beams.push(VrLaserBeam {
                        start_r: [origin.x, origin.y, origin.z, 0.0018],
                        end_fade: [far.x, far.y, far.z, 1.0],
                        color: [0.55, 0.82, 1.0, 0.45],
                    });
                }
            } else if let Some(g) = globe {
                // ---- 3b. Open 3D Globe: project laser directly onto the curved Earth sphere! ----
                self.smooth_uv = None;
                self.click_anchor_uv = None;

                // Velocity-adaptive angular smoothing of the laser direction onto the sphere.
                let s_dir = match self.smooth_globe_dir {
                    Some(prev_d) => {
                        let ang = prev_d.angle_between(dir);
                        let alpha = ((ang - 0.0015) / 0.025).clamp(0.0, 1.0);
                        let k = 0.30 + 0.62 * alpha;
                        prev_d.lerp(dir, k).normalize_or_zero()
                    }
                    None => dir,
                };

                if let Some(gh) = Self::intersect_globe(origin, s_dir, g) {
                    self.smooth_globe_dir = Some(s_dir);

                    if active_pressed && self.grip_drag.is_none() {
                        self.globe_press = Some(LaserGlobePress {
                            hand: ah,
                            press_lat_lon: (gh.lat, gh.lon),
                            start_dir: gh.eye_dir,
                            last_dir: gh.eye_dir,
                            dragging: false,
                        });
                        out.haptics.push(HapticCmd {
                            hand: ah,
                            amplitude: 0.22,
                            frequency: 175.0,
                            duration_ns: 15_000_000,
                        });
                    }

                    if active_trig && !active_pressed {
                        if let Some(gp) = &mut self.globe_press {
                            if gp.hand == ah {
                                if !gp.dragging && gp.start_dir.angle_between(gh.eye_dir) > GLOBE_DRAG_RAD {
                                    gp.dragging = true;
                                }
                                if gp.dragging {
                                    // Convert angular eye-ray change into equivalent 60° screen pixels.
                                    let scale = (win_h as f64) / (2.0 * 30.0f64.to_radians().tan());
                                    let z0 = (-gp.last_dir.z).max(0.25) as f64;
                                    let z1 = (-gh.eye_dir.z).max(0.25) as f64;
                                    let dx = ((gh.eye_dir.x as f64 / z1) - (gp.last_dir.x as f64 / z0)) * scale;
                                    let dy = -((gh.eye_dir.y as f64 / z1) - (gp.last_dir.y as f64 / z0)) * scale;
                                    if dx.abs() + dy.abs() > 0.05 {
                                        out.globe_drag = Some((dx, dy));
                                    }
                                }
                                gp.last_dir = gh.eye_dir;
                            }
                        }
                    }

                    if active_released {
                        if let Some(gp) = self.globe_press.take() {
                            if !gp.dragging {
                                out.globe_pick = Some(gp.press_lat_lon);
                                out.haptics.push(HapticCmd {
                                    hand: ah,
                                    amplitude: 0.36,
                                    frequency: 220.0,
                                    duration_ns: 20_000_000,
                                });
                            }
                        }
                    }

                    // While holding trigger before dragging, lock the target ring visually onto the
                    // pressed `(lat, lon)` on the sphere so pulling the trigger feels rock-solid.
                    let (t_pos, t_norm, _t_dir, t_dist, t_lat, t_lon) = match self.globe_press {
                        Some(gp) if !gp.dragging => {
                            if let Some((p, n, d, dist)) = Self::globe_point_on_sphere(gp.press_lat_lon.0, gp.press_lat_lon.1, g) {
                                (p, n, d, dist, gp.press_lat_lon.0, gp.press_lat_lon.1)
                            } else {
                                (gh.pos, gh.normal, gh.eye_dir, gh.eye_dist, gh.lat, gh.lon)
                            }
                        }
                        _ => (gh.pos, gh.normal, gh.eye_dir, gh.eye_dist, gh.lat, gh.lon),
                    };

                    out.globe_hover = Some((t_lat, t_lon));
                    // Open-globe hover/labels project directly in 3D onto the Earth sphere,
                    // so clear 2D panel hover and avoid drawing a flat 1.8 m dot over the globe.
                    out.egui_events.push(egui::Event::PointerGone);

                    let (beam_col, ring_col) = if active_trig {
                        ([0.16, 0.88, 1.0, 0.95], [0.16, 0.90, 1.0, 1.0])
                    } else {
                        ([0.45, 0.92, 1.0, 0.82], [0.22, 0.92, 1.0, 0.35])
                    };
                    let beam_rad = if active_trig { 0.0024 } else { 0.0019 };
                    let ring_rad = t_dist * if active_trig { 0.0135 } else { 0.0175 };

                    out.beams.push(VrLaserBeam {
                        start_r: [origin.x, origin.y, origin.z, beam_rad],
                        end_fade: [t_pos.x, t_pos.y, t_pos.z, 0.0],
                        color: beam_col,
                    });
                    // Style 2.0: surface-projected target ring aligned to the Earth sphere normal!
                    out.dots.push(VrLaserDot {
                        pos_r: [t_pos.x, t_pos.y, t_pos.z, ring_rad],
                        norm_style: [t_norm.x, t_norm.y, t_norm.z, 2.0],
                        color: ring_col,
                    });
                } else {
                    // Pointing into deep space past the Earth's limb.
                    self.smooth_globe_dir = None;
                    if active_released {
                        if let Some(gp) = self.globe_press.take() {
                            if !gp.dragging {
                                out.globe_pick = Some(gp.press_lat_lon);
                            }
                        }
                    }
                    out.egui_events.push(egui::Event::PointerGone);
                    let far = origin + dir * 8.0;
                    out.beams.push(VrLaserBeam {
                        start_r: [origin.x, origin.y, origin.z, 0.0018],
                        end_fade: [far.x, far.y, far.z, 1.0],
                        color: [0.55, 0.82, 1.0, 0.40],
                    });
                }
            }
        } else {
            self.smooth_uv = None;
            self.smooth_globe_dir = None;
            self.click_anchor_uv = None;
            if self.ui_press_active {
                self.ui_press_active = false;
            }
            self.globe_press = None;
        }

        // Draw a pulsing 3D surface-projected start pin right on the Earth sphere when a spot is chosen.
        if let Some(g) = globe.filter(|_| is_globe) {
            if let Some((plat, plon)) = g.picked {
                if let Some((p_pos, p_norm, p_dir, p_dist)) = Self::globe_point_on_sphere(plat, plon, g) {
                    let hidden_by_ui = Self::project_dir_to_quad(p_dir, quad, win_w, win_h, ppp)
                        .is_some_and(&is_over_ui);
                    if !hidden_by_ui {
                        out.dots.push(VrLaserDot {
                            pos_r: [p_pos.x, p_pos.y, p_pos.z, p_dist * 0.021],
                            norm_style: [p_norm.x, p_norm.y, p_norm.z, 2.0],
                            color: [0.98, 0.80, 0.14, 0.85],
                        });
                    }
                }
            }
        }

        // Controller emitter orbs so both hands remain subtly visible in VR menu space.
        for hi in 0..2 {
            if hands[hi].aim_tracked {
                let p = if self.grip_drag.is_some_and(|g| g.hand == hi) {
                    hands[hi].pos
                } else {
                    origins[hi]
                };
                let grabbing = self.grip_drag.is_some_and(|g| g.hand == hi);
                let col = if grabbing {
                    [0.18, 0.86, 1.0, 0.92]
                } else if hi == ah {
                    if active_trig { [0.18, 0.88, 1.0, 0.90] } else { [0.98, 0.84, 0.25, 0.80] }
                } else {
                    [0.95, 0.78, 0.20, 0.42]
                };
                out.dots.push(VrLaserDot {
                    pos_r: [p.x, p.y, p.z, if grabbing { 0.020 } else if hi == ah { 0.013 } else { 0.010 }],
                    norm_style: [0.0, 0.0, 0.0, 1.0],
                    color: col,
                });
            }
        }

        // ---- 4. Thumbstick Scrolling (UI) & Globe Pan/Zoom ----
        let sy_active = hands[ah].thumbstick[1];
        if effective_over_ui && sy_active.abs() > 0.18 {
            let v = ((sy_active.abs() - 0.18) / 0.82) * sy_active.signum();
            out.egui_events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, v * (dt as f32) * 420.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
                source: egui::MouseWheelSource::Wheel,
            });
        } else if is_globe && !effective_over_ui {
            // Left stick circles/pans the globe; right stick Y zooms in/out.
            let lx = hands[0].thumbstick[0];
            let ly = hands[0].thumbstick[1];
            if lx.abs().max(ly.abs()) > 0.18 {
                let dead = |v: f32| {
                    if v.abs() > 0.18 {
                        ((v.abs() - 0.18) / 0.82) * v.signum()
                    } else {
                        0.0
                    }
                };
                let sx = dead(lx) as f64;
                let sy = dead(ly) as f64;
                let rate = win_h as f64 * 0.55 * dt;
                let (ax, ay) = out.globe_drag.unwrap_or((0.0, 0.0));
                out.globe_drag = Some((ax - sx * rate, ay + sy * rate));
            }
            let ry = hands[1].thumbstick[1];
            if ry.abs() > 0.18 {
                let v = ((ry.abs() - 0.18) / 0.82) * ry.signum();
                out.globe_zoom += (v as f64) * 4.5 * dt;
            }
        }

        out
    }

    /// Draws the subtle desktop-mirror reticle dot at the VR laser position so the
    /// mirror shows where the VR pointer is aiming too.
    pub fn draw_mirror_dot(ctx: &egui::Context, pos: Option<egui::Pos2>, pressed: bool) {
        let Some(p) = pos else { return };
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("vr_laser_dot")));
        let col = if pressed {
            egui::Color32::from_rgb(40, 220, 255)
        } else {
            menu::ACCENT
        };
        let r = if pressed { 6.5 } else { 8.0 };
        painter.circle_stroke(p, r, egui::Stroke::new(2.0, col));
        painter.circle_stroke(p, 4.0, egui::Stroke::new(1.5, egui::Color32::from_black_alpha(200)));
        painter.circle_filled(p, 3.0, egui::Color32::WHITE);
    }

    /// Optional compact on-screen VR keyboard that appears neatly at the bottom-center
    /// whenever a text field has focus in VR. Users can type on their physical PC keyboard
    /// at any time OR point & click keys with the VR laser (and can hide/re-show the virtual
    /// keyboard with one click).
    pub fn draw_vr_keyboard(&mut self, ctx: &egui::Context) {
        // Re-apply focus after a virtual key press so the text field stays active.
        if let Some(id) = self.kb_refocus.take() {
            ctx.memory_mut(|m| m.request_focus(id));
        }

        let focused_text = if ctx.text_edit_focused() {
            ctx.memory(|m| m.focused())
        } else {
            None
        };

        if let Some(id) = focused_text {
            self.kb_target = Some(id);
        } else if self.kb_refocus.is_none() {
            // Only drop target when no text box is focused and no virtual key click is in flight.
            if self.kb_pending.is_empty() {
                self.kb_target = None;
                self.kb_hidden_for = None;
            }
        }

        let Some(target_id) = self.kb_target else { return };
        let screen = ctx.content_rect();

        // If user hid the virtual keyboard for this field, show a tiny re-open button.
        if self.kb_hidden_for == Some(target_id) {
            egui::Area::new(egui::Id::unique("vr_kb_pill"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -14.0])
                .show(ctx, |ui| {
                    menu::panel().inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Type on keyboard or").font(menu::caption()).color(menu::DIM));
                            if ui.button(egui::RichText::new("Show VR Keyboard").font(menu::caption())).clicked() {
                                self.kb_hidden_for = None;
                                self.kb_refocus = Some(target_id);
                            }
                        });
                    });
                });
            return;
        }

        const ROWS: [&[&str]; 4] = [
            &["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
            &["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
            &["a", "s", "d", "f", "g", "h", "j", "k", "l"],
            &["z", "x", "c", "v", "b", "n", "m", "-", "."],
        ];

        let mut emit_text: Option<String> = None;
        let mut emit_key: Option<egui::Key> = None;
        let mut clear_all = false;
        let mut toggle_shift = false;
        let mut hide_kb = false;

        let key_w = ((screen.width() * 0.48) / 11.5).clamp(32.0, 44.0);
        let key_h = (key_w * 0.82).clamp(28.0, 36.0);

        egui::Area::new(egui::Id::unique("vr_keyboard"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -14.0])
            .show(ctx, |ui| {
                menu::panel().inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("VR KEYBOARD")
                                .font(menu::section_title())
                                .color(menu::ACCENT),
                        );
                        ui.label(
                            egui::RichText::new("· physical keyboard also works")
                                .font(menu::caption())
                                .color(menu::DIM),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(egui::RichText::new("Hide").font(menu::caption())).min_size(egui::vec2(48.0, 22.0))).clicked() {
                                hide_kb = true;
                            }
                        });
                    });
                    ui.add_space(4.0);

                    for (ri, row) in ROWS.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ri == 2 {
                                ui.add_space(key_w * 0.45);
                            }
                            if ri == 3 {
                                let shift_btn = egui::Button::new(
                                    egui::RichText::new(if self.kb_shift { "SHIFT" } else { "Shift" })
                                        .font(menu::caption())
                                        .color(if self.kb_shift { menu::ON_ACCENT } else { menu::TEXT }),
                                )
                                .fill(if self.kb_shift { menu::ACCENT } else { menu::RAISED })
                                .min_size(egui::vec2(key_w * 1.45, key_h));
                                if ui.add(shift_btn).clicked() {
                                    toggle_shift = true;
                                }
                            }
                            for &k in *row {
                                let ch = if self.kb_shift { k.to_ascii_uppercase() } else { k.to_string() };
                                let btn = egui::Button::new(
                                    egui::RichText::new(&ch).font(menu::label()).color(menu::TEXT),
                                )
                                .min_size(egui::vec2(key_w, key_h));
                                if ui.add(btn).clicked() {
                                    emit_text = Some(ch);
                                }
                            }
                            if ri == 0 {
                                let bk = egui::Button::new(
                                    egui::RichText::new("⌫").font(menu::label()).color(menu::TEXT),
                                )
                                .min_size(egui::vec2(key_w * 1.35, key_h));
                                if ui.add(bk).clicked() {
                                    emit_key = Some(egui::Key::Backspace);
                                }
                            }
                        });
                    }

                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(egui::RichText::new("Clear").font(menu::caption())).min_size(egui::vec2(key_w * 1.8, key_h)))
                            .clicked()
                        {
                            clear_all = true;
                        }
                        if ui
                            .add(egui::Button::new(egui::RichText::new("Space").font(menu::label())).min_size(egui::vec2(key_w * 6.0, key_h)))
                            .clicked()
                        {
                            emit_text = Some(" ".to_string());
                        }
                        let go_btn = egui::Button::new(
                            egui::RichText::new("GO / ENTER")
                                .font(menu::semibold(13.5))
                                .color(menu::ON_ACCENT),
                        )
                        .fill(menu::ACCENT)
                        .min_size(egui::vec2(key_w * 3.2, key_h));
                        if ui.add(go_btn).clicked() {
                            emit_key = Some(egui::Key::Enter);
                        }
                    });
                });
            });

        if toggle_shift {
            self.kb_shift = !self.kb_shift;
            self.kb_refocus = Some(target_id);
        }
        if hide_kb {
            self.kb_hidden_for = Some(target_id);
            self.kb_refocus = Some(target_id);
        }
        if clear_all {
            self.kb_refocus = Some(target_id);
            self.kb_pending.push(egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            });
            self.kb_pending.push(egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            });
            self.kb_pending.push(egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            self.kb_pending.push(egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        if let Some(text) = emit_text {
            self.kb_refocus = Some(target_id);
            self.kb_pending.push(egui::Event::Text(text));
        }
        if let Some(key) = emit_key {
            self.kb_refocus = Some(target_id);
            self.kb_pending.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            self.kb_pending.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            if key == egui::Key::Enter {
                // Enter submits the search box; let focus release afterwards.
                self.kb_target = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn hand(pos: Vec3, rot: Quat, trigger: f32, squeeze: f32) -> XrHandState {
        XrHandState {
            tracked: true,
            pos,
            rot,
            aim_tracked: true,
            aim_pos: pos,
            aim_rot: rot,
            trigger,
            squeeze,
            ..Default::default()
        }
    }

    #[test]
    fn ray_quad_center_and_corners() {
        let q = VrMenu::panel_quad(MenuKind::Globe, 1920.0, 1080.0);
        // Straight ahead from origin -> exact center (0.5, 0.5)
        let hit = VrMenu::intersect_quad(Vec3::ZERO, Vec3::NEG_Z, q).expect("center hit");
        assert!((hit.2 - 0.5).abs() < 1e-5);
        assert!((hit.3 - 0.5).abs() < 1e-5);
        assert!((hit.0 - 1.80).abs() < 1e-4);

        // Pointing backward -> miss
        assert!(VrMenu::intersect_quad(Vec3::ZERO, Vec3::Z, q).is_none());

        // Pointing at top-left corner -> (0, 0)
        let tl = q.center - q.right * q.half_w + q.up * q.half_h;
        let hit_tl = VrMenu::intersect_quad(Vec3::ZERO, tl.normalize(), q).expect("top-left hit");
        assert!(hit_tl.2.abs() < 1e-4);
        assert!(hit_tl.3.abs() < 1e-4);
    }

    #[test]
    fn globe_quad_aligns_with_60_deg_fov() {
        let q = VrMenu::panel_quad(MenuKind::Globe, 1600.0, 900.0);
        // Ray at +30 deg up from horizontal should hit the exact top edge (v = 0).
        let dir = Vec3::new(0.0, 30.0f32.to_radians().sin(), -30.0f32.to_radians().cos());
        let hit = VrMenu::intersect_quad(Vec3::ZERO, dir, q).expect("top edge hit");
        assert!((hit.2 - 0.5).abs() < 1e-4);
        assert!(hit.3.abs() < 1e-4);
    }

    fn sample_globe() -> GlobeView {
        GlobeView {
            lat: 37.7749,
            lon: -122.4194,
            alt: 1.1e7,
            picked: None,
            head_pos: Vec3::ZERO,
        }
    }

    #[test]
    fn laser_click_emits_egui_press_and_release_despite_vr_hand_wobble() {
        let mut m = VrMenu::new();
        let origin = Vec3::new(0.2, -0.2, -0.3);
        let to_center = (Vec3::new(0.0, -0.02, -1.75) - origin).normalize();
        let aim_rot = Quat::from_rotation_arc(Vec3::NEG_Z, to_center);

        let o1 = m.step(0.016, MenuKind::FullScreen, [XrHandState::default(), hand(origin, aim_rot, 0.0, 0.0)], 1280.0, 720.0, 1.0, None, |_| true);
        assert!(o1.pointer_logical.is_some());
        assert_eq!(o1.beams.len(), 1);
        assert!(o1.dots.iter().any(|d| d.norm_style[3] < 0.5), "expected laser target dot on UI");

        let o2 = m.step(0.016, MenuKind::FullScreen, [XrHandState::default(), hand(origin, aim_rot, 0.9, 0.0)], 1280.0, 720.0, 1.0, None, |_| true);
        let press_pos = o2.egui_events.iter().find_map(|e| match e {
            egui::Event::PointerButton { pos, pressed: true, .. } => Some(*pos),
            _ => None,
        }).expect("expected press event");

        // Simulate realistic 1.0 deg wrist pitch wobble over ~200ms while pulling/releasing trigger (~15-20 logical pts, > egui's desktop 6pt limit).
        let wobble_rot = Quat::from_rotation_x(0.014) * aim_rot;
        for _ in 0..12 {
            let _ = m.step(0.016, MenuKind::FullScreen, [XrHandState::default(), hand(origin, wobble_rot, 0.9, 0.0)], 1280.0, 720.0, 1.0, None, |_| true);
        }
        let o3 = m.step(0.016, MenuKind::FullScreen, [XrHandState::default(), hand(origin, wobble_rot, 0.0, 0.0)], 1280.0, 720.0, 1.0, None, |_| true);
        let release_pos = o3.egui_events.iter().find_map(|e| match e {
            egui::Event::PointerButton { pos, pressed: false, .. } => Some(*pos),
            _ => None,
        }).expect("expected release event");
        assert!(press_pos.distance(release_pos) < 1e-3, "release should stay anchored to press_pos during normal VR trigger wobble: {press_pos:?} vs {release_pos:?}");
    }

    #[test]
    fn two_hand_pinch_zooms_in_and_out_and_pans() {
        let mut m = VrMenu::new();
        let g = Some(sample_globe());
        let l0 = hand(Vec3::new(-0.15, 0.0, -0.4), Quat::IDENTITY, 0.0, 0.9);
        let r0 = hand(Vec3::new(0.15, 0.0, -0.4), Quat::IDENTITY, 0.0, 0.9);
        let o0 = m.step(0.016, MenuKind::Globe, [l0, r0], 1280.0, 720.0, 1.0, g, |_| false);
        assert!(o0.pinching);
        assert_eq!(o0.globe_zoom, 0.0);

        // Spread controllers apart -> positive zoom (zoom IN)
        let l1 = hand(Vec3::new(-0.30, 0.05, -0.4), Quat::IDENTITY, 0.0, 0.9);
        let r1 = hand(Vec3::new(0.30, 0.05, -0.4), Quat::IDENTITY, 0.0, 0.9);
        let o1 = m.step(0.016, MenuKind::Globe, [l1, r1], 1280.0, 720.0, 1.0, g, |_| false);
        assert!(o1.pinching);
        assert!(o1.globe_zoom > 2.0, "spreading hands should zoom in, got {}", o1.globe_zoom);
        assert!(o1.globe_drag.is_some(), "moving midpoint should pan the globe simultaneously");

        // Pinch controllers back together -> negative zoom (zoom OUT)
        let o2 = m.step(0.016, MenuKind::Globe, [l0, r0], 1280.0, 720.0, 1.0, g, |_| false);
        assert!(o2.globe_zoom < -2.0, "pinching hands together should zoom out, got {}", o2.globe_zoom);
    }

    #[test]
    fn globe_sphere_projection_conforms_to_curvature_and_sets_start_point() {
        let mut m = VrMenu::new();
        let g = sample_globe();
        // Aim straight at globe center: normal should point straight back (+Z) and lat/lon should match camera center.
        let center_hit = VrMenu::intersect_globe(Vec3::ZERO, Vec3::NEG_Z, g).expect("center hit");
        assert!((center_hit.lat - g.lat).abs() < 1e-3);
        assert!((center_hit.lon - g.lon).abs() < 1e-3);
        assert!((center_hit.normal - Vec3::Z).length() < 1e-3);

        // Aim 15 deg east toward curved limb: surface normal must tilt east (+X) along the sphere!
        let dir_east = Vec3::new(15.0f32.to_radians().sin(), 0.0, -15.0f32.to_radians().cos());
        let side_hit = VrMenu::intersect_globe(Vec3::ZERO, dir_east, g).expect("side hit");
        assert!(side_hit.normal.x > 0.35, "expected sphere normal to tilt east, got {:?}", side_hit.normal);
        assert!(side_hit.lon > g.lon + 10.0, "expected east longitude, got {}", side_hit.lon);

        // Zoomed in close (1000 m altitude): f64 precision must hold cleanly.
        let low_g = GlobeView { alt: 1000.0, ..g };
        let low_hit = VrMenu::intersect_globe(Vec3::new(0.2, -0.2, -0.3), Vec3::NEG_Z, low_g).expect("low-alt hit");
        assert!((low_hit.lat - g.lat).abs() < 0.01);
        assert!((low_hit.lon - g.lon).abs() < 0.01);

        // Aiming past the limb into space -> miss.
        let dir_space = Vec3::new(45.0f32.to_radians().sin(), 0.0, -45.0f32.to_radians().cos());
        assert!(VrMenu::intersect_globe(Vec3::ZERO, dir_space, g).is_none());

        // Trigger tap with slight realistic hand tremor (< 3 deg) MUST pick the globe spot!
        let origin = Vec3::new(0.25, -0.25, -0.35);
        let rot0 = Quat::from_rotation_arc(Vec3::NEG_Z, dir_east);
        let rot_tremor = Quat::from_rotation_y(0.018) * rot0; // ~1 deg tremor
        let o_hover = m.step(0.016, MenuKind::Globe, [XrHandState::default(), hand(origin, rot0, 0.0, 0.0)], 1280.0, 720.0, 1.0, Some(g), |_| false);
        assert!(o_hover.globe_hover.is_some());
        assert!(o_hover.dots.iter().any(|d| d.norm_style[3] > 1.5 && d.norm_style[0] > 0.3), "expected sphere-normal-aligned globe target ring");

        let _ = m.step(0.016, MenuKind::Globe, [XrHandState::default(), hand(origin, rot0, 0.9, 0.0)], 1280.0, 720.0, 1.0, Some(g), |_| false);
        for _ in 0..8 {
            let _ = m.step(0.016, MenuKind::Globe, [XrHandState::default(), hand(origin, rot_tremor, 0.9, 0.0)], 1280.0, 720.0, 1.0, Some(g), |_| false);
        }
        let o_rel = m.step(0.016, MenuKind::Globe, [XrHandState::default(), hand(origin, rot_tremor, 0.0, 0.0)], 1280.0, 720.0, 1.0, Some(g), |_| false);
        let (plat, plon) = o_rel.globe_pick.expect("expected trigger tap on globe to set start point");
        assert!((plat - side_hit.lat).abs() < 1.5 && (plon - side_hit.lon).abs() < 1.5, "picked ({plat}, {plon}) vs expected ({}, {})", side_hit.lat, side_hit.lon);
    }

    #[test]
    fn menu_back_recenter_and_reset_prevent_bounce() {
        let mut m = VrMenu::new();
        let mut lh = XrHandState::default();
        let mut rh = XrHandState::default();
        rh.secondary_btn = true;
        lh.menu_btn = true;

        // Opening a menu with B or Menu held down and syncing via reset must not bounce immediately.
        m.reset([lh, rh]);
        let o0 = m.step(0.016, MenuKind::InFlight, [lh, rh], 1280.0, 720.0, 1.0, None, |_| true);
        assert!(!o0.back && !o0.menu, "held button across transition must not fire back/menu");

        // Release then press each button distinctly.
        let idle = [XrHandState::default(), XrHandState::default()];
        let _ = m.step(0.016, MenuKind::InFlight, idle, 1280.0, 720.0, 1.0, None, |_| true);

        let mut rh_b = XrHandState::default();
        rh_b.secondary_btn = true;
        let o_back = m.step(0.016, MenuKind::InFlight, [XrHandState::default(), rh_b], 1280.0, 720.0, 1.0, None, |_| true);
        assert!(o_back.back && !o_back.menu && !o_back.recenter);

        let mut lh_m = XrHandState::default();
        lh_m.menu_btn = true;
        let o_menu = m.step(0.016, MenuKind::InFlight, [lh_m, XrHandState::default()], 1280.0, 720.0, 1.0, None, |_| true);
        assert!(o_menu.menu && !o_menu.back && !o_menu.recenter);

        let mut lh_y = XrHandState::default();
        lh_y.secondary_btn = true;
        let o_rec = m.step(0.016, MenuKind::InFlight, [lh_y, XrHandState::default()], 1280.0, 720.0, 1.0, None, |_| true);
        assert!(o_rec.recenter && !o_rec.back && !o_rec.menu);
    }
}
