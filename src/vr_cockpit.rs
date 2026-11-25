//! VR hand controller interaction for flying and working the clickable cockpit:
//! - Grabbing yokes (1-handed or 2-handed push/pull + turn) and flight sticks
//!   (center-stick and side-stick tilt + displacement)
//! - Direct physical manipulation of all cockpit hotspots: push/pull levers
//!   (throttle + afterburner gate, mixture, prop, cowl flaps), notched levers
//!   (flaps), trim wheels, twistable rotary knobs (OBS, AP, radios, fuel),
//!   toggle switches, and momentary holds (starter, reversers, toe brakes)
//! - Three visual hand-orb modes: Idle (translucent yellow), Hover (glowing
//!   golden-yellow), and Active (vivid cyan interaction mode)
//! - Layered per-hand haptic feedback for tactile cockpit controls and flight
//!   physics (turbulence, stall buffet, high-G/overspeed, touchdown, ground roll,
//!   engine/afterburner vibration, and damage shocks).

use crate::aircraft::{Hotspot, Spec};
use crate::airframe::{Anim, Input, Piece};
use crate::vr::XrHandState;
use glam::{Mat4, Quat, Vec2, Vec3};

/// Visual state of a VR hand orb.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HandMode {
    /// Not holding anything and not near any grabbable control (translucent yellow).
    Idle,
    /// On/near a control that can be grabbed or pressed (glowing golden-yellow).
    Hover,
    /// Holding down and actively steering or manipulating a control (cyan active mode).
    Active,
}

impl HandMode {
    pub fn as_shader_mode(self) -> f32 {
        match self {
            HandMode::Idle => 0.0,
            HandMode::Hover => 1.0,
            HandMode::Active => 2.0,
        }
    }

    pub fn radius(self) -> f32 {
        match self {
            HandMode::Idle => 0.024,
            HandMode::Hover => 0.030,
            HandMode::Active => 0.022,
        }
    }
}

/// What a VR hand is currently hovering over or gripping in the cockpit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VrHandTarget {
    /// A clickable cockpit control (index into `cockpit_controls`).
    Hotspot(usize),
    /// A control yoke.
    Yoke,
    /// A center-stick or side-stick flight stick.
    Stick,
}

/// Per-hand orb visual info in aircraft body frame.
#[derive(Copy, Clone, Debug)]
pub struct VrHandOrbInfo {
    pub body_pos: Vec3,
    pub radius: f32,
    pub mode: HandMode,
    pub pulse: f32,
    pub grip: f32,
    /// Control currently hovered or gripped by this hand (for 3D cockpit labels).
    pub target: Option<VrHandTarget>,
    /// Aircraft body-frame position of the targeted control (hotspot center or grip point).
    pub target_pos: Option<Vec3>,
}

/// Discrete event emitted by VR hand interaction for `Game` to execute.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VrCockpitEvent {
    /// Fire a cockpit action id from `bind::ACTIONS` or `c172::cabin::MOUSE_ACTIONS`.
    TriggerAction(&'static str),
    /// Recenter the VR seated head origin.
    RecenterVr,
    /// Open/close the flight & radio radial menu.
    FlightMenu,
    /// Open/close the pause menu.
    PauseMenu,
}

/// One haptic vibration command for an OpenXR controller.
#[derive(Copy, Clone, Debug)]
pub struct HapticCmd {
    pub hand: usize,
    pub amplitude: f32,
    pub frequency: f32,
    pub duration_ns: i64,
}

/// Snapshot of current aircraft control states needed by `VrCockpit::step`.
#[derive(Copy, Clone, Debug, Default)]
pub struct AircraftSnapshot {
    pub pitch: f64,
    pub roll: f64,
    pub throttle: f64,
    pub mixture: f64,
    pub propeller: f64,
    pub cowl_flaps: f64,
    pub flap_set: u8,
    pub max_flap_notch: u8,
    pub real_boost: bool,
}

/// Outputs produced by one frame of VR cockpit hand interaction.
#[derive(Clone, Debug, Default)]
pub struct VrStepOut {
    pub pitch: Option<f64>,
    pub roll: Option<f64>,
    pub yaw: Option<f64>,
    pub throttle_set: Option<f64>,
    pub boost: bool,
    pub brake: bool,
    pub brake_left: f64,
    pub brake_right: f64,
    pub starter: bool,
    pub reverse: bool,
    pub mixture_set: Option<f64>,
    pub prop_set: Option<f64>,
    pub cowl_set: Option<f64>,
    pub trim_delta: f64,
    pub events: Vec<VrCockpitEvent>,
    pub orbs: [Option<VrHandOrbInfo>; 2],
    /// Optional hotspot index currently hovered or grabbed by each hand (for desktop HUD/debug).
    pub hovered_hotspot: [Option<usize>; 2],
}

/// Physics telemetry fed into `VrCockpit::step_flight_haptics` after simulation substeps.
#[derive(Copy, Clone, Debug, Default)]
pub struct FlightFeedback {
    pub dt: f64,
    pub gust_speed: f32,
    pub gust_rot: f32,
    pub sigma_w: f32,
    pub sf: Vec3,
    pub stalled: bool,
    pub alpha_frac: f32,
    pub g_load: f32,
    pub overspeed: f32,
    pub ias: f32,
    pub on_ground: bool,
    pub ground_speed: f32,
    pub sink_rate: f32,
    pub burner: f32,
    pub running: bool,
    pub power: f32,
    pub braking: f32,
    pub damage_vibe: f32,
    pub impact_shock: f32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum FlightControlKind {
    Yoke,
    CenterStick,
    SideStick,
}

#[derive(Clone, Debug)]
struct FlightControlZone {
    piece_idx: usize,
    kind: FlightControlKind,
    /// Piece-local pivot (hub for yokes, gimbal base for sticks).
    pivot: Vec3,
    /// Piece-local AABB of the grabbable grip area.
    grip_lo: Vec3,
    grip_hi: Vec3,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Target {
    FlightControl(usize),
    Hotspot(usize),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HotspotKind {
    Throttle,
    Mixture,
    Propeller,
    CowlFlaps,
    Flaps,
    Trim,
    Rotary,
    Toggle,
    Hold,
    ClickOrHold,
}

#[derive(Copy, Clone, Debug)]
struct ActiveGrab {
    target: Target,
    grab_pos: Vec3,
    grab_rot: Quat,
    prev_pos: Vec3,
    prev_rot: Quat,
    start_time: f64,
    /// Initial pitch/roll/lever value when grabbed.
    init_a: f64,
    init_b: f64,
    /// Accumulated continuous roll angle (rad) for yoke steering.
    accum_angle: f32,
    /// Fractional accumulator for notched controls (flaps, rotary knobs, trim ticks).
    accum_steps: f32,
    /// Last quantized value for haptic friction ticks on smooth levers.
    last_tick_val: f32,
    /// Whether hold action has engaged (for ClickOrHold controls like ignition/starter).
    hold_engaged: bool,
    /// Whether afterburner detent was crossed on throttle.
    ab_detent: bool,
    /// Physical travel distance (m) for smooth levers.
    travel_m: f32,
}

#[derive(Copy, Clone, Debug, Default)]
struct HapticPulse {
    amplitude: f32,
    frequency: f32,
    remaining: f32,
}

#[derive(Clone, Debug)]
struct HandTracker {
    prev_grab_btn: bool,
    prev_secondary: bool,
    prev_menu: bool,
    prev_stick_click: bool,
    hovered: Option<Target>,
    grab: Option<ActiveGrab>,
    pulse: HapticPulse,
    was_vibrating: bool,
    activity_pulse: f32,
}

impl Default for HandTracker {
    fn default() -> Self {
        Self {
            prev_grab_btn: false,
            prev_secondary: false,
            prev_menu: false,
            prev_stick_click: false,
            hovered: None,
            grab: None,
            pulse: HapticPulse::default(),
            was_vibrating: false,
            activity_pulse: 0.0,
        }
    }
}

/// Manages VR hand-controller cockpit interaction, flight steering, and haptics.
pub struct VrCockpit {
    controls: Vec<FlightControlZone>,
    hands: [HandTracker; 2],
    /// Current VR-commanded pitch/roll when at least one hand holds a yoke/stick.
    flight_pitch: f64,
    flight_roll: f64,
    /// Two-handed yoke anchor state when both hands simultaneously grab the same yoke.
    two_hand_yoke: Option<TwoHandYoke>,
    /// Previous frame telemetry for detecting touchdown and turbulence jolts.
    prev_on_ground: bool,
    prev_sf: Vec3,
    prev_sf_valid: bool,
    time: f64,
    haptic_cooldown: [f32; 2],
}

#[derive(Copy, Clone, Debug)]
struct TwoHandYoke {
    zone_idx: usize,
    base_pitch: f64,
    base_roll: f64,
    base_mid_z: f32,
    prev_vec_xy: Vec2,
    accum_roll: f32,
}

/// Yoke fore/aft travel (m) for full ±1.0 pitch authority.
const YOKE_PITCH_TRAVEL: f32 = 0.11;
/// Yoke rotation (rad) for full ±1.0 roll authority (~49 deg).
const YOKE_ROLL_RANGE: f32 = 0.85;
/// Center-stick displacement (m) and tilt angle (rad) for full deflection.
const STICK_POS_TRAVEL: f32 = 0.11;
const STICK_TILT_RANGE: f32 = 0.38;
const SIDESTICK_TILT_RANGE: f32 = 0.30;

/// Extra reach margin (m) around cockpit hotspots and grips for forgiving VR grabbing.
const HOTSPOT_TOUCH_MARGIN: f32 = 0.036;
const YOKE_TOUCH_MARGIN: f32 = 0.062;
const STICK_TOUCH_MARGIN: f32 = 0.068;

/// Analog threshold for trigger/grip grab activation (with hysteresis).
const GRAB_ON: f32 = 0.55;
const GRAB_OFF: f32 = 0.35;

impl VrCockpit {
    /// Discover grabbable yokes and flight sticks from the aircraft's model pieces.
    pub fn new(_spec: &Spec, pieces: &[Piece]) -> Self {
        let mut controls = Vec::new();
        for (i, p) in pieces.iter().enumerate() {
            if p.name != "yoke" && p.name != "stick" {
                continue;
            }
            if p.mesh.v.is_empty() {
                continue;
            }
            let (full_lo, full_hi) = mesh_bounds(&p.mesh.v);
            if p.name == "yoke" {
                // Isolate the wheel/horns near the hub, excluding long shafts behind the panel.
                let z_cut = p.pivot.z - 0.065;
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                let mut any = false;
                for v in &p.mesh.v {
                    if v.pos[2] >= z_cut {
                        let pt = Vec3::from(v.pos);
                        lo = lo.min(pt);
                        hi = hi.max(pt);
                        any = true;
                    }
                }
                if !any {
                    (lo, hi) = (full_lo, full_hi);
                }
                controls.push(FlightControlZone {
                    piece_idx: i,
                    kind: FlightControlKind::Yoke,
                    pivot: p.pivot,
                    grip_lo: lo,
                    grip_hi: hi,
                });
            } else {
                // Flight stick: grab the upper 16 cm grip portion of the stick.
                let y_cut = full_hi.y - 0.16;
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                let mut any = false;
                for v in &p.mesh.v {
                    if v.pos[1] >= y_cut {
                        let pt = Vec3::from(v.pos);
                        lo = lo.min(pt);
                        hi = hi.max(pt);
                        any = true;
                    }
                }
                if !any {
                    (lo, hi) = (full_lo, full_hi);
                }
                let is_side = p.pivot.x.abs() > 0.22 && (full_hi.y - full_lo.y) < 0.28;
                controls.push(FlightControlZone {
                    piece_idx: i,
                    kind: if is_side { FlightControlKind::SideStick } else { FlightControlKind::CenterStick },
                    pivot: p.pivot,
                    grip_lo: lo,
                    grip_hi: hi,
                });
            }
        }
        Self {
            controls,
            hands: [HandTracker::default(), HandTracker::default()],
            flight_pitch: 0.0,
            flight_roll: 0.0,
            two_hand_yoke: None,
            prev_on_ground: true,
            prev_sf: Vec3::new(0.0, 9.81, 0.0),
            prev_sf_valid: false,
            time: 0.0,
            haptic_cooldown: [0.0; 2],
        }
    }

    /// Release all active grabs (e.g. when pausing, crashing, or switching aircraft).
    pub fn cancel_grabs(&mut self) {
        for h in &mut self.hands {
            h.grab = None;
            h.hovered = None;
        }
        self.two_hand_yoke = None;
        self.flight_pitch = 0.0;
        self.flight_roll = 0.0;
    }

    /// Keeps button edge detectors in sync while VR menu mode owns controller input
    /// so closing a menu with B / Menu / Stick Click never immediately re-opens it.
    pub fn sync_buttons(&mut self, raw_hands: [XrHandState; 2]) {
        for hi in 0..2 {
            let rh = raw_hands[hi];
            let tr = &mut self.hands[hi];
            tr.prev_secondary = rh.secondary_btn;
            tr.prev_menu = rh.menu_btn;
            tr.prev_stick_click = rh.thumbstick_click;
            tr.prev_grab_btn = rh.squeeze.max(rh.trigger) > GRAB_ON;
        }
    }

    /// Returns whether either hand is currently holding a yoke or stick.
    pub fn holding_flight_control(&self) -> bool {
        self.hands.iter().any(|h| matches!(h.grab, Some(ActiveGrab { target: Target::FlightControl(_), .. })))
    }

    /// Queue a one-shot tactile haptic impulse on a hand controller.
    pub fn trigger_pulse(&mut self, hand: usize, amplitude: f32, frequency: f32, duration_s: f32) {
        if hand >= 2 {
            return;
        }
        let p = &mut self.hands[hand].pulse;
        if amplitude >= p.amplitude || p.remaining <= 0.0 {
            p.amplitude = amplitude.clamp(0.0, 1.0);
            p.frequency = frequency;
            p.remaining = duration_s.max(0.012);
        }
        self.hands[hand].activity_pulse = self.hands[hand].activity_pulse.max(amplitude.clamp(0.0, 1.0));
    }

    /// Update VR hand tracking, cockpit control targeting/grabbing, and flight control inputs.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        dt: f64,
        seat_eye: Vec3,
        raw_hands: [XrHandState; 2],
        pieces: &[Piece],
        pose: &[Mat4],
        cockpit_controls: &[(Hotspot, Option<usize>)],
        animated_hs: &[Hotspot],
        snap: AircraftSnapshot,
        can_interact: bool,
    ) -> VrStepOut {
        let mut out = VrStepOut::default();
        self.time += dt;

        // Global VR controller utility buttons (flight/radio menu, recenter, pause, brakes, rudder).
        let mut rudder = 0.0f64;
        for hi in 0..2 {
            let rh = raw_hands[hi];
            let tr = &mut self.hands[hi];
            tr.activity_pulse = (tr.activity_pulse - dt as f32 * 3.5).max(0.0);

            let sec_edge = rh.secondary_btn && !tr.prev_secondary;
            let stick_edge = rh.thumbstick_click && !tr.prev_stick_click;
            if hi == 0 && (sec_edge || stick_edge) {
                // Left Y or Left Stick Click -> recenter VR view.
                out.events.push(VrCockpitEvent::RecenterVr);
                tr.pulse = HapticPulse { amplitude: 0.25, frequency: 180.0, remaining: 0.025 };
            } else if hi == 1 && (sec_edge || stick_edge) {
                // Right B or Right Stick Click -> open Flight / Radio radial menu.
                out.events.push(VrCockpitEvent::FlightMenu);
                tr.pulse = HapticPulse { amplitude: 0.30, frequency: 210.0, remaining: 0.022 };
            }
            tr.prev_secondary = rh.secondary_btn;
            tr.prev_stick_click = rh.thumbstick_click;

            if hi == 0 && rh.menu_btn && !tr.prev_menu {
                out.events.push(VrCockpitEvent::PauseMenu);
                tr.pulse = HapticPulse { amplitude: 0.28, frequency: 180.0, remaining: 0.020 };
            }
            tr.prev_menu = rh.menu_btn;

            if can_interact && rh.primary_btn {
                out.brake = true;
            }
        }

        if !can_interact {
            self.cancel_grabs();
            // Still emit idle hand orbs when tracked in VR so hands remain visible.
            for hi in 0..2 {
                if raw_hands[hi].tracked {
                    out.orbs[hi] = Some(VrHandOrbInfo {
                        body_pos: seat_eye + raw_hands[hi].pos,
                        radius: HandMode::Idle.radius(),
                        mode: HandMode::Idle,
                        pulse: 0.0,
                        grip: raw_hands[hi].squeeze.max(raw_hands[hi].trigger),
                        target: None,
                        target_pos: None,
                    });
                }
            }
            return out;
        }

        // Convert tracked hands into aircraft body frame.
        let body_pos = [seat_eye + raw_hands[0].pos, seat_eye + raw_hands[1].pos];
        let body_rot = [raw_hands[0].rot, raw_hands[1].rot];

        // 1. Update hover targets and grab start/end transitions per hand.
        for hi in 0..2 {
            let rh = raw_hands[hi];
            if !rh.tracked {
                self.hands[hi].grab = None;
                self.hands[hi].hovered = None;
                self.hands[hi].prev_grab_btn = false;
                continue;
            }

            let grab_val = rh.squeeze.max(rh.trigger);
            let grab_btn = if self.hands[hi].prev_grab_btn {
                grab_val > GRAB_OFF
            } else {
                grab_val > GRAB_ON
            };
            let just_pressed = grab_btn && !self.hands[hi].prev_grab_btn;
            let just_released = !grab_btn && self.hands[hi].prev_grab_btn;
            self.hands[hi].prev_grab_btn = grab_btn;

            // Only pick a new hover target when not already holding something.
            if self.hands[hi].grab.is_none() {
                let prefer_yoke = rh.squeeze > rh.trigger + 0.15;
                let target = self.pick_target(body_pos[hi], pose, cockpit_controls, animated_hs, prefer_yoke);
                if target != self.hands[hi].hovered && target.is_some() {
                    // Subtle hover-enter tactile tick.
                    self.trigger_pulse(hi, 0.14, 190.0, 0.016);
                }
                self.hands[hi].hovered = target;
            }

            if just_pressed {
                if let Some(target) = self.hands[hi].hovered {
                    match target {
                        Target::FlightControl(zi) => {
                            let (p0, r0) = if self.holding_flight_control() {
                                (self.flight_pitch, self.flight_roll)
                            } else {
                                (snap.pitch, snap.roll)
                            };
                            self.hands[hi].grab = Some(ActiveGrab {
                                target,
                                grab_pos: body_pos[hi],
                                grab_rot: body_rot[hi],
                                prev_pos: body_pos[hi],
                                prev_rot: body_rot[hi],
                                start_time: self.time,
                                init_a: p0,
                                init_b: r0,
                                accum_angle: 0.0,
                                accum_steps: 0.0,
                                last_tick_val: 0.0,
                                hold_engaged: false,
                                ab_detent: false,
                                travel_m: YOKE_PITCH_TRAVEL,
                            });
                            self.flight_pitch = p0;
                            self.flight_roll = r0;
                            self.trigger_pulse(hi, 0.36, 125.0, 0.030);
                            let _ = zi;
                        }
                        Target::Hotspot(hsi) => {
                            if let Some((hs, piece_opt)) = cockpit_controls.get(hsi).copied() {
                                let kind = classify_hotspot(&hs);
                                match kind {
                                    HotspotKind::Toggle => {
                                        if let Some(id) = hs.click {
                                            out.events.push(VrCockpitEvent::TriggerAction(id));
                                        }
                                        self.trigger_pulse(hi, 0.45, 220.0, 0.025);
                                        // Keep visual Active state while trigger/grip remains held.
                                        self.hands[hi].grab = Some(ActiveGrab {
                                            target,
                                            grab_pos: body_pos[hi],
                                            grab_rot: body_rot[hi],
                                            prev_pos: body_pos[hi],
                                            prev_rot: body_rot[hi],
                                            start_time: self.time,
                                            init_a: 0.0,
                                            init_b: 0.0,
                                            accum_angle: 0.0,
                                            accum_steps: 0.0,
                                            last_tick_val: 0.0,
                                            hold_engaged: false,
                                            ab_detent: false,
                                            travel_m: 0.10,
                                        });
                                    }
                                    HotspotKind::Hold => {
                                        self.trigger_pulse(hi, 0.30, 170.0, 0.022);
                                        self.hands[hi].grab = Some(ActiveGrab {
                                            target,
                                            grab_pos: body_pos[hi],
                                            grab_rot: body_rot[hi],
                                            prev_pos: body_pos[hi],
                                            prev_rot: body_rot[hi],
                                            start_time: self.time,
                                            init_a: 0.0,
                                            init_b: 0.0,
                                            accum_angle: 0.0,
                                            accum_steps: 0.0,
                                            last_tick_val: 0.0,
                                            hold_engaged: true,
                                            ab_detent: false,
                                            travel_m: 0.10,
                                        });
                                    }
                                    HotspotKind::ClickOrHold => {
                                        self.trigger_pulse(hi, 0.25, 190.0, 0.020);
                                        self.hands[hi].grab = Some(ActiveGrab {
                                            target,
                                            grab_pos: body_pos[hi],
                                            grab_rot: body_rot[hi],
                                            prev_pos: body_pos[hi],
                                            prev_rot: body_rot[hi],
                                            start_time: self.time,
                                            init_a: 0.0,
                                            init_b: 0.0,
                                            accum_angle: 0.0,
                                            accum_steps: 0.0,
                                            last_tick_val: 0.0,
                                            hold_engaged: false,
                                            ab_detent: false,
                                            travel_m: 0.10,
                                        });
                                    }
                                    HotspotKind::Throttle
                                    | HotspotKind::Mixture
                                    | HotspotKind::Propeller
                                    | HotspotKind::CowlFlaps
                                    | HotspotKind::Flaps
                                    | HotspotKind::Trim
                                    | HotspotKind::Rotary => {
                                        let init_a = match kind {
                                            HotspotKind::Throttle => snap.throttle,
                                            HotspotKind::Mixture => snap.mixture,
                                            HotspotKind::Propeller => snap.propeller,
                                            HotspotKind::CowlFlaps => snap.cowl_flaps,
                                            _ => 0.0,
                                        };
                                        let travel_m = if kind == HotspotKind::Flaps {
                                            flap_step_m(piece_opt.and_then(|pi| pieces.get(pi)), &hs, snap.max_flap_notch)
                                        } else {
                                            lever_travel_m(piece_opt.and_then(|pi| pieces.get(pi)), kind)
                                        };
                                        self.trigger_pulse(hi, 0.30, 150.0, 0.024);
                                        self.hands[hi].grab = Some(ActiveGrab {
                                            target,
                                            grab_pos: body_pos[hi],
                                            grab_rot: body_rot[hi],
                                            prev_pos: body_pos[hi],
                                            prev_rot: body_rot[hi],
                                            start_time: self.time,
                                            init_a,
                                            init_b: 0.0,
                                            accum_angle: 0.0,
                                            accum_steps: 0.0,
                                            last_tick_val: init_a as f32,
                                            hold_engaged: false,
                                            ab_detent: false,
                                            travel_m,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            } else if just_released {
                if let Some(grab) = self.hands[hi].grab.take() {
                    match grab.target {
                        Target::Hotspot(hsi) => {
                            if let Some((hs, _)) = cockpit_controls.get(hsi) {
                                if classify_hotspot(hs) == HotspotKind::ClickOrHold && !grab.hold_engaged {
                                    if let Some(id) = hs.click {
                                        out.events.push(VrCockpitEvent::TriggerAction(id));
                                        self.trigger_pulse(hi, 0.42, 220.0, 0.024);
                                    }
                                } else {
                                    self.trigger_pulse(hi, 0.12, 150.0, 0.015);
                                }
                            }
                        }
                        Target::FlightControl(_) => {
                            self.trigger_pulse(hi, 0.12, 140.0, 0.016);
                            // If the other hand is still holding the yoke, seamlessly re-anchor it
                            // so taking one hand off never jumps pitch or roll.
                            let other = 1 - hi;
                            if let Some(og) = self.hands[other].grab.as_mut() {
                                if matches!(og.target, Target::FlightControl(_)) {
                                    og.grab_pos = body_pos[other];
                                    og.grab_rot = body_rot[other];
                                    og.prev_pos = body_pos[other];
                                    og.prev_rot = body_rot[other];
                                    og.init_a = self.flight_pitch;
                                    og.init_b = self.flight_roll;
                                    og.accum_angle = 0.0;
                                }
                            }
                            self.two_hand_yoke = None;
                        }
                    }
                }
            }
        }

        // 2. Process active flight-control steering (yoke 1-hand / 2-hand, or stick).
        let fc_left = match self.hands[0].grab {
            Some(ActiveGrab { target: Target::FlightControl(z), .. }) => Some(z),
            _ => None,
        };
        let fc_right = match self.hands[1].grab {
            Some(ActiveGrab { target: Target::FlightControl(z), .. }) => Some(z),
            _ => None,
        };

        if let (Some(zl), Some(zr)) = (fc_left, fc_right) {
            if zl == zr && self.controls.get(zl).is_some_and(|c| c.kind == FlightControlKind::Yoke) {
                // Two-handed steering wheel grip!
                let cur_vec = Vec2::new(body_pos[1].x - body_pos[0].x, body_pos[1].y - body_pos[0].y);
                let cur_mid_z = 0.5 * (body_pos[0].z + body_pos[1].z);
                let th = self.two_hand_yoke.get_or_insert(TwoHandYoke {
                    zone_idx: zl,
                    base_pitch: self.flight_pitch,
                    base_roll: self.flight_roll,
                    base_mid_z: cur_mid_z,
                    prev_vec_xy: cur_vec,
                    accum_roll: 0.0,
                });
                if th.zone_idx != zl {
                    *th = TwoHandYoke {
                        zone_idx: zl,
                        base_pitch: self.flight_pitch,
                        base_roll: self.flight_roll,
                        base_mid_z: cur_mid_z,
                        prev_vec_xy: cur_vec,
                        accum_roll: 0.0,
                    };
                }
                if th.prev_vec_xy.length() > 0.04 && cur_vec.length() > 0.04 {
                    th.accum_roll += -signed_angle_xy(th.prev_vec_xy, cur_vec);
                }
                th.prev_vec_xy = cur_vec;
                let dz = cur_mid_z - th.base_mid_z;
                self.flight_pitch = (th.base_pitch + (dz / YOKE_PITCH_TRAVEL) as f64).clamp(-1.0, 1.0);
                self.flight_roll = (th.base_roll + (th.accum_roll / YOKE_ROLL_RANGE) as f64).clamp(-1.0, 1.0);
                for hi in 0..2 {
                    if let Some(g) = self.hands[hi].grab.as_mut() {
                        g.prev_pos = body_pos[hi];
                        g.prev_rot = body_rot[hi];
                    }
                }
                out.pitch = Some(self.flight_pitch);
                out.roll = Some(self.flight_roll);
            } else {
                // Two different flight controls grabbed (rare: e.g. left and right yokes) -> use right/latest.
                self.two_hand_yoke = None;
                let (p, r) = self.update_single_flight_control(1, zr, body_pos[1], body_rot[1], pose);
                self.flight_pitch = p;
                self.flight_roll = r;
                out.pitch = Some(p);
                out.roll = Some(r);
            }
        } else if let Some((hi, zi)) = fc_left.map(|z| (0, z)).or_else(|| fc_right.map(|z| (1, z))) {
            self.two_hand_yoke = None;
            let (p, r) = self.update_single_flight_control(hi, zi, body_pos[hi], body_rot[hi], pose);
            self.flight_pitch = p;
            self.flight_roll = r;
            out.pitch = Some(p);
            out.roll = Some(r);
        } else {
            self.two_hand_yoke = None;
        }

        // 3. Process active cockpit hotspot interactions per hand.
        for hi in 0..2 {
            let rh = raw_hands[hi];
            let Some(mut grab) = self.hands[hi].grab else {
                // Hand free: thumbstick X gives smooth rudder/yaw control.
                let sx = rh.thumbstick[0];
                if sx.abs() > 0.15 {
                    let v = ((sx.abs() - 0.15) / 0.85).powf(1.4) * sx.signum();
                    if v.abs() as f64 > rudder.abs() {
                        rudder = v as f64;
                    }
                }
                continue;
            };

            // Even while holding yoke/stick, allow thumbstick X to work the rudder!
            if matches!(grab.target, Target::FlightControl(_)) {
                let sx = rh.thumbstick[0];
                if sx.abs() > 0.15 {
                    let v = ((sx.abs() - 0.15) / 0.85).powf(1.4) * sx.signum();
                    if v.abs() as f64 > rudder.abs() {
                        rudder = v as f64;
                    }
                }
                // And thumbstick Y on yoke/stick hand trims elevator gently!
                let sy = rh.thumbstick[1];
                if sy.abs() > 0.25 {
                    let v = ((sy.abs() - 0.25) / 0.75) * sy.signum();
                    // Push stick forward (+Y) = trim nose down (-trim)
                    out.trim_delta += -(v as f64) * 0.035 * dt;
                }
                continue;
            }

            let Target::Hotspot(hsi) = grab.target else { continue };
            let Some((hs, piece_opt)) = cockpit_controls.get(hsi).copied() else {
                self.hands[hi].grab = None;
                continue;
            };
            let kind = classify_hotspot(&hs);
            let cur_pos = body_pos[hi];
            let cur_rot = body_rot[hi];
            let d_pos = cur_pos - grab.prev_pos;
            let d_twist = delta_roll_z(grab.prev_rot, cur_rot);
            grab.prev_pos = cur_pos;
            grab.prev_rot = cur_rot;

            match kind {
                HotspotKind::Throttle => {
                    // Pushing forward (-Z) increases throttle toward 1.0.
                    let dz_fwd = -(cur_pos.z - grab.grab_pos.z);
                    let raw = grab.init_a + (dz_fwd / grab.travel_m) as f64;
                    let clamped = raw.clamp(0.0, 1.0);
                    out.throttle_set = Some(clamped);

                    // Friction ticks every ~4% throttle movement.
                    let val_f = clamped as f32;
                    if (val_f - grab.last_tick_val).abs() >= 0.04 {
                        grab.last_tick_val = val_f;
                        self.trigger_pulse(hi, 0.18 + 0.10 * val_f, 155.0, 0.018);
                    }
                    // End-stop detents at idle (0%) and full power (100%).
                    if (clamped <= 0.001 && grab.last_tick_val > 0.005)
                        || (clamped >= 0.999 && grab.last_tick_val < 0.995)
                    {
                        grab.last_tick_val = val_f;
                        self.trigger_pulse(hi, 0.50, 115.0, 0.032);
                    }
                    // Afterburner gate on jets with real afterburners: push slightly past 100%.
                    if snap.real_boost && raw > 1.12 {
                        out.boost = true;
                        if !grab.ab_detent {
                            grab.ab_detent = true;
                            self.trigger_pulse(hi, 0.65, 100.0, 0.045);
                        }
                    } else if raw < 1.06 {
                        grab.ab_detent = false;
                    }
                }
                HotspotKind::Mixture => {
                    let dz_fwd = -(cur_pos.z - grab.grab_pos.z);
                    let val = (grab.init_a + (dz_fwd / grab.travel_m) as f64).clamp(0.0, 1.0);
                    out.mixture_set = Some(val);
                    if (val as f32 - grab.last_tick_val).abs() >= 0.05 {
                        grab.last_tick_val = val as f32;
                        self.trigger_pulse(hi, 0.18, 175.0, 0.016);
                    }
                }
                HotspotKind::Propeller => {
                    let dz_fwd = -(cur_pos.z - grab.grab_pos.z);
                    let val = (grab.init_a + (dz_fwd / grab.travel_m) as f64).clamp(0.0, 1.0);
                    out.prop_set = Some(val);
                    if (val as f32 - grab.last_tick_val).abs() >= 0.05 {
                        grab.last_tick_val = val as f32;
                        self.trigger_pulse(hi, 0.18, 175.0, 0.016);
                    }
                }
                HotspotKind::CowlFlaps => {
                    let delta = -(cur_pos.z - grab.grab_pos.z) + (cur_pos.y - grab.grab_pos.y) * 0.5;
                    let val = (grab.init_a + (delta / grab.travel_m) as f64).clamp(0.0, 1.0);
                    out.cowl_set = Some(val);
                    if (val as f32 - grab.last_tick_val).abs() >= 0.10 {
                        grab.last_tick_val = val as f32;
                        self.trigger_pulse(hi, 0.22, 165.0, 0.018);
                    }
                }
                HotspotKind::Flaps => {
                    if let Some((up, down)) = hs.wheel {
                        let piece = piece_opt.and_then(|pi| pieces.get(pi));
                        let piece_pose = piece_opt.and_then(|pi| pose.get(pi).copied());
                        let up_dir = flaps_up_dir(piece, piece_pose, &hs);
                        let step_m = grab.travel_m.max(0.015);
                        let thumb_step = if rh.thumbstick[1].abs() > 0.25 {
                            rh.thumbstick[1] * dt as f32 * 2.5
                        } else {
                            0.0
                        };
                        let step_delta = d_pos.dot(up_dir) / step_m + thumb_step;
                        grab.accum_steps += step_delta;
                        let bounded = snap.max_flap_notch > 0;
                        let mut notch = snap.flap_set;
                        if bounded && notch == 0 && grab.accum_steps > 0.0 {
                            grab.accum_steps = 0.0;
                        }
                        if bounded && notch >= snap.max_flap_notch && grab.accum_steps < 0.0 {
                            grab.accum_steps = 0.0;
                        }
                        while grab.accum_steps >= 1.0 {
                            if bounded && notch == 0 {
                                grab.accum_steps = 0.0;
                                break;
                            }
                            grab.accum_steps -= 1.0;
                            notch = notch.saturating_sub(1);
                            out.events.push(VrCockpitEvent::TriggerAction(up));
                            self.trigger_pulse(hi, 0.42, 185.0, 0.026);
                        }
                        while grab.accum_steps <= -1.0 {
                            if bounded && notch >= snap.max_flap_notch {
                                grab.accum_steps = 0.0;
                                break;
                            }
                            grab.accum_steps += 1.0;
                            notch = notch.saturating_add(1);
                            out.events.push(VrCockpitEvent::TriggerAction(down));
                            self.trigger_pulse(hi, 0.42, 185.0, 0.026);
                        }
                    }
                }
                HotspotKind::Trim => {
                    // Pulling back/up (+Z / +Y) trims nose up (+trim); pushing forward/down (-Z / -Y) trims nose down.
                    let delta = (d_pos.z * 0.7 + d_pos.y * 0.7) * 0.65 - (rh.thumbstick[1] * dt as f32 * 0.04);
                    out.trim_delta += delta as f64;
                    grab.accum_steps += delta.abs() / 0.008;
                    if grab.accum_steps >= 1.0 {
                        grab.accum_steps = 0.0;
                        self.trigger_pulse(hi, 0.18, 210.0, 0.015);
                    }
                }
                HotspotKind::Rotary => {
                    if let Some((up, down)) = hs.wheel {
                        // Twist wrist clockwise OR move hand up OR push thumbstick up/right.
                        let thumb = if rh.thumbstick[0].abs() > rh.thumbstick[1].abs() {
                            rh.thumbstick[0]
                        } else {
                            rh.thumbstick[1]
                        };
                        let thumb_step = if thumb.abs() > 0.25 { thumb * dt as f32 * 5.0 } else { 0.0 };
                        let step_delta = d_twist / 0.20 + d_pos.y / 0.022 + thumb_step;
                        grab.accum_steps += step_delta;
                        while grab.accum_steps >= 1.0 {
                            grab.accum_steps -= 1.0;
                            out.events.push(VrCockpitEvent::TriggerAction(up));
                            self.trigger_pulse(hi, 0.34, 230.0, 0.018);
                        }
                        while grab.accum_steps <= -1.0 {
                            grab.accum_steps += 1.0;
                            out.events.push(VrCockpitEvent::TriggerAction(down));
                            self.trigger_pulse(hi, 0.34, 230.0, 0.018);
                        }
                    }
                }
                HotspotKind::Hold => {
                    apply_hold_action(hs.hold, &mut out);
                    if hs.hold == Some("starter") || hs.hold == Some("reversers") {
                        self.trigger_pulse(hi, 0.26, 95.0, 0.030);
                    }
                }
                HotspotKind::ClickOrHold => {
                    grab.accum_angle += d_twist;
                    // Engage starter/hold if held past 0.35s OR twisted clockwise > 20 deg.
                    if !grab.hold_engaged && (self.time - grab.start_time >= 0.35 || grab.accum_angle > 0.35) {
                        grab.hold_engaged = true;
                        self.trigger_pulse(hi, 0.40, 140.0, 0.030);
                    }
                    if grab.hold_engaged {
                        apply_hold_action(hs.hold, &mut out);
                        self.trigger_pulse(hi, 0.28, 90.0, 0.030);
                    }
                }
                HotspotKind::Toggle => {}
            }

            self.hands[hi].grab = Some(grab);
        }

        if rudder.abs() > 0.0 {
            out.yaw = Some(rudder.clamp(-1.0, 1.0));
        }

        // 4. Emit the 3-mode hand orb visuals for each tracked hand.
        for hi in 0..2 {
            if !raw_hands[hi].tracked {
                continue;
            }
            let tr = &self.hands[hi];
            let mode = if tr.grab.is_some() {
                HandMode::Active
            } else if tr.hovered.is_some() {
                HandMode::Hover
            } else {
                HandMode::Idle
            };
            let raw_target = tr.grab.map(|g| g.target).or(tr.hovered);
            out.hovered_hotspot[hi] = match raw_target {
                Some(Target::Hotspot(i)) => Some(i),
                _ => None,
            };
            let (target, target_pos) = match raw_target {
                Some(Target::Hotspot(i)) => (
                    Some(VrHandTarget::Hotspot(i)),
                    animated_hs.get(i).map(|h| Vec3::from(h.at)),
                ),
                Some(Target::FlightControl(zi)) => {
                    let t = self.controls.get(zi).map(|c| match c.kind {
                        FlightControlKind::Yoke => VrHandTarget::Yoke,
                        FlightControlKind::CenterStick | FlightControlKind::SideStick => VrHandTarget::Stick,
                    });
                    (t, Some(body_pos[hi]))
                }
                None => (None, None),
            };
            out.orbs[hi] = Some(VrHandOrbInfo {
                body_pos: body_pos[hi],
                radius: mode.radius(),
                mode,
                pulse: tr.activity_pulse,
                grip: raw_hands[hi].squeeze.max(raw_hands[hi].trigger),
                target,
                target_pos,
            });
        }

        out
    }

    /// Compute and mix layered per-hand haptics from cockpit interaction pulses and
    /// live flight physics (turbulence, stall buffet, touchdown, ground roll, engine, damage).
    pub fn step_flight_haptics(&mut self, fb: FlightFeedback) -> [Option<HapticCmd>; 2] {
        let dt = fb.dt.max(1e-4) as f32;

        // Touchdown impact impulse when transitioning from airborne to on-ground.
        if fb.on_ground && !self.prev_on_ground {
            let amp = ((fb.sink_rate / 4.5).clamp(0.25, 1.0) * 0.90).clamp(0.25, 0.95);
            for hi in 0..2 {
                self.trigger_pulse(hi, amp, 55.0, 0.085);
            }
        }
        self.prev_on_ground = fb.on_ground;

        // Sudden structural shock / part break / weapon fire.
        if fb.impact_shock > 0.05 {
            let amp = fb.impact_shock.clamp(0.2, 1.0);
            for hi in 0..2 {
                self.trigger_pulse(hi, amp, 50.0, 0.095);
            }
        }

        // Turbulence intensity: sustained gusts + sharp accelerometer jolts.
        let sf_jolt = if self.prev_sf_valid {
            ((fb.sf - self.prev_sf).length() / dt.max(0.008) / 45.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.prev_sf = fb.sf;
        self.prev_sf_valid = true;

        let gust_amp = ((fb.gust_speed - 1.2) / 11.0).clamp(0.0, 1.0) * 0.50
            + ((fb.gust_rot - 0.04) / 0.35).clamp(0.0, 1.0) * 0.30
            + (fb.sigma_w / 6.0).clamp(0.0, 0.35);
        let turb_amp = (gust_amp + sf_jolt * 0.45).clamp(0.0, 0.85);

        // Stall buffet / stick shaker: ramps in from 82% of stall AoA and pulses sharply when stalled.
        let stall_wave = 0.65 + 0.35 * (self.time as f32 * 44.0).sin().abs();
        let stall_amp = if fb.stalled {
            0.78 * stall_wave
        } else if !fb.on_ground && fb.ias > 12.0 && fb.alpha_frac > 0.82 {
            ((fb.alpha_frac - 0.82) / 0.18).clamp(0.0, 1.0) * 0.55 * stall_wave
        } else {
            0.0
        };

        // High-G / overspeed aerodynamic buffet.
        let g_amp = ((fb.g_load.abs() - 2.0) / 5.0).clamp(0.0, 1.0) * 0.42;
        let over_amp = (fb.overspeed / 25.0).clamp(0.0, 1.0) * 0.65;

        // Ground roll & braking rumble.
        let ground_amp = if fb.on_ground && fb.ground_speed > 2.0 {
            let speed_k = ((fb.ground_speed - 2.0) / 45.0).clamp(0.0, 1.0);
            (speed_k * 0.18 + fb.braking * speed_k * 0.25).clamp(0.0, 0.45)
        } else {
            0.0
        };

        // Engine/afterburner/damage airframe vibration.
        let ab_amp = fb.burner * 0.36;
        let dmg_amp = (fb.damage_vibe * 0.45).clamp(0.0, 0.70);

        let mut cmds = [None, None];
        for hi in 0..2 {
            self.haptic_cooldown[hi] = (self.haptic_cooldown[hi] - dt).max(0.0);

            let holding_fc = matches!(self.hands[hi].grab, Some(ActiveGrab { target: Target::FlightControl(_), .. }));
            let holding_thr = matches!(self.hands[hi].grab, Some(ActiveGrab { target: Target::Hotspot(_), .. }));

            // Continuous ambient flight rumble for this hand.
            let mut cont_amp = 0.0f32;
            let mut cont_freq = 65.0f32;

            let mut add_cont = |a: f32, f: f32| {
                if a > cont_amp {
                    cont_amp = a;
                    cont_freq = f;
                }
            };

            // Turbulence is felt in both hands (stronger when gripping controls).
            let turb_gain = if holding_fc { 1.0 } else { 0.55 };
            if turb_amp * turb_gain > 0.06 {
                add_cont(turb_amp * turb_gain, 58.0);
            }

            // Ground roll and damage vibration travel through the whole airframe.
            let gnd_gain = if holding_fc { 1.0 } else { 0.55 };
            if ground_amp * gnd_gain > 0.04 {
                add_cont(ground_amp * gnd_gain, 52.0);
            }
            if dmg_amp > 0.06 {
                add_cont(dmg_amp * if holding_fc { 1.0 } else { 0.65 }, 68.0);
            }

            // Aerodynamic control loading & stall stick-shaker when gripping the yoke/stick.
            if holding_fc {
                if stall_amp > 0.05 {
                    add_cont(stall_amp, 78.0);
                }
                if g_amp > 0.05 {
                    add_cont(g_amp, 64.0);
                }
                if over_amp > 0.05 {
                    add_cont(over_amp, 95.0);
                }
                // Subtle control-force feel when deflecting hard at speed.
                let defl = (self.flight_pitch.hypot(self.flight_roll) as f32).clamp(0.0, 1.0);
                let speed_load = ((fb.ias - 25.0) / 110.0).clamp(0.0, 1.0);
                let load_amp = (defl - 0.35).max(0.0) * speed_load * 0.16;
                if load_amp > 0.03 {
                    add_cont(load_amp, 72.0);
                }
            } else if stall_amp > 0.25 {
                // Lighter airframe buffet even with hands off the yoke.
                add_cont(stall_amp * 0.35, 65.0);
            }

            // Afterburner & high-power engine vibration felt strongly on the throttle hand.
            if ab_amp > 0.04 {
                add_cont(ab_amp * if holding_thr { 1.0 } else { 0.45 }, 60.0);
            } else if holding_thr && fb.running && fb.power > 0.55 {
                add_cont(((fb.power - 0.55) / 0.45) * 0.09, 75.0);
            }

            // Mix one-shot interaction/impact pulse over continuous rumble.
            let p = &mut self.hands[hi].pulse;
            let (mut final_amp, mut final_freq, mut dur_ns) = (cont_amp, cont_freq, 35_000_000i64);
            if p.remaining > 0.0 {
                if p.amplitude >= final_amp {
                    final_amp = p.amplitude;
                    final_freq = p.frequency;
                    dur_ns = ((p.remaining * 1e9) as i64).clamp(12_000_000, 120_000_000);
                }
                p.remaining = (p.remaining - dt).max(0.0);
                if p.remaining <= 0.0 {
                    p.amplitude = 0.0;
                }
            }

            if final_amp > 0.025 {
                self.hands[hi].was_vibrating = true;
                self.hands[hi].activity_pulse = self.hands[hi].activity_pulse.max(final_amp);
                cmds[hi] = Some(HapticCmd {
                    hand: hi,
                    amplitude: final_amp.clamp(0.0, 1.0),
                    frequency: final_freq,
                    duration_ns: dur_ns,
                });
            } else if self.hands[hi].was_vibrating {
                self.hands[hi].was_vibrating = false;
                cmds[hi] = Some(HapticCmd {
                    hand: hi,
                    amplitude: 0.0,
                    frequency: 0.0,
                    duration_ns: 0,
                });
            }
        }
        cmds
    }

    fn pick_target(
        &self,
        hand_body: Vec3,
        pose: &[Mat4],
        cockpit_controls: &[(Hotspot, Option<usize>)],
        animated_hs: &[Hotspot],
        prefer_yoke: bool,
    ) -> Option<Target> {
        // 1. Closest cockpit hotspot within touch reach.
        let mut best_hs: Option<(usize, f32, bool)> = None;
        for (i, h) in animated_hs.iter().enumerate() {
            let d = (hand_body - Vec3::from(h.at)).length();
            let reach = h.r + HOTSPOT_TOUCH_MARGIN;
            if d <= reach {
                let score = d - h.r;
                let on_yoke = cockpit_controls
                    .get(i)
                    .and_then(|(_, pi)| *pi)
                    .is_some_and(|pi| self.controls.iter().any(|c| c.piece_idx == pi));
                if best_hs.is_none_or(|(_, bs, _)| score < bs) {
                    best_hs = Some((i, score, on_yoke));
                }
            }
        }

        // 2. Closest yoke or flight stick grip within touch reach.
        let mut best_fc: Option<(usize, f32)> = None;
        for (zi, fc) in self.controls.iter().enumerate() {
            let Some(&m) = pose.get(fc.piece_idx) else { continue };
            let local = m.inverse().transform_point3(hand_body);
            let d = dist_to_aabb(local, fc.grip_lo, fc.grip_hi);
            let margin = match fc.kind {
                FlightControlKind::Yoke => YOKE_TOUCH_MARGIN,
                _ => STICK_TOUCH_MARGIN,
            };
            if d <= margin && best_fc.is_none_or(|(_, bd)| d < bd) {
                best_fc = Some((zi, d));
            }
        }

        match (best_hs, best_fc) {
            (Some((_, _, true)), Some((zi, _))) if prefer_yoke => Some(Target::FlightControl(zi)),
            (Some((hsi, hs_score, _)), Some((zi, fc_dist))) => {
                // Non-yoke switches (throttle, gear, panel knobs) take priority when directly touched,
                // unless the user is much closer to the yoke grip.
                if hs_score <= fc_dist + 0.015 {
                    Some(Target::Hotspot(hsi))
                } else {
                    Some(Target::FlightControl(zi))
                }
            }
            (Some((hsi, _, _)), None) => Some(Target::Hotspot(hsi)),
            (None, Some((zi, _))) => Some(Target::FlightControl(zi)),
            (None, None) => None,
        }
    }

    fn update_single_flight_control(
        &mut self,
        hand_idx: usize,
        zone_idx: usize,
        cur_pos: Vec3,
        cur_rot: Quat,
        pose: &[Mat4],
    ) -> (f64, f64) {
        let Some(fc) = self.controls.get(zone_idx) else {
            return (self.flight_pitch, self.flight_roll);
        };
        let Some(grab) = self.hands[hand_idx].grab.as_mut() else {
            return (self.flight_pitch, self.flight_roll);
        };

        let (pitch, roll) = match fc.kind {
            FlightControlKind::Yoke => {
                // Pitch: push forward (-Z) = nose down, pull back (+Z) = nose up.
                let dz = cur_pos.z - grab.grab_pos.z;
                let pitch = (grab.init_a + (dz / YOKE_PITCH_TRAVEL) as f64).clamp(-1.0, 1.0);

                // Roll: blend wrist twist around body Z with orbital angle around the yoke hub.
                let d_wrist = delta_roll_z(grab.prev_rot, cur_rot);
                let hub_body = pose
                    .get(fc.piece_idx)
                    .map(|m| m.transform_point3(fc.pivot))
                    .unwrap_or(fc.pivot);
                let v_prev = Vec2::new(grab.prev_pos.x - hub_body.x, grab.prev_pos.y - hub_body.y);
                let v_cur = Vec2::new(cur_pos.x - hub_body.x, cur_pos.y - hub_body.y);
                let r = v_prev.length().min(v_cur.length());
                let d_roll = if r > 0.05 {
                    let d_orbit = -signed_angle_xy(v_prev, v_cur);
                    let w_orbit = ((r - 0.05) / 0.06).clamp(0.0, 1.0) * 0.45;
                    (1.0 - w_orbit) * d_wrist + w_orbit * d_orbit
                } else {
                    d_wrist
                };
                grab.accum_angle += d_roll;
                let roll = (grab.init_b + (grab.accum_angle / YOKE_ROLL_RANGE) as f64).clamp(-1.0, 1.0);
                (pitch, roll)
            }
            FlightControlKind::CenterStick | FlightControlKind::SideStick => {
                let (w_tilt, tilt_range) = match fc.kind {
                    FlightControlKind::SideStick => (0.75f32, SIDESTICK_TILT_RANGE),
                    _ => (0.55f32, STICK_TILT_RANGE),
                };
                let w_pos = 1.0 - w_tilt;

                // Tilt of the controller's up axis relative to the initial grip attitude.
                let dq = cur_rot * grab.grab_rot.conjugate();
                let up = dq * Vec3::Y;
                // Pulling stick back (+X rotation) moves +Y toward +Z -> +pitch (nose up).
                let tilt_pitch = up.z.atan2(up.y.max(0.05)) / tilt_range;
                // Tilting stick right (-Z rotation) moves +Y toward +X -> +roll (right).
                let tilt_roll = up.x.atan2(up.y.max(0.05)) / tilt_range;

                let pos_pitch = (cur_pos.z - grab.grab_pos.z) / STICK_POS_TRAVEL;
                let pos_roll = (cur_pos.x - grab.grab_pos.x) / STICK_POS_TRAVEL;

                let pitch = (grab.init_a + (w_tilt * tilt_pitch + w_pos * pos_pitch) as f64).clamp(-1.0, 1.0);
                let roll = (grab.init_b + (w_tilt * tilt_roll + w_pos * pos_roll) as f64).clamp(-1.0, 1.0);
                (pitch, roll)
            }
        };

        grab.prev_pos = cur_pos;
        grab.prev_rot = cur_rot;
        (pitch, roll)
    }
}

fn classify_hotspot(h: &Hotspot) -> HotspotKind {
    if let Some((up, _)) = h.wheel {
        return match up {
            "throttle_up" => HotspotKind::Throttle,
            "mixture_rich" => HotspotKind::Mixture,
            "prop_up" => HotspotKind::Propeller,
            "cowl_open" => HotspotKind::CowlFlaps,
            "flaps_up" => HotspotKind::Flaps,
            "trim_down" => HotspotKind::Trim,
            _ => HotspotKind::Rotary,
        };
    }
    match (h.click, h.hold) {
        (Some(_), Some(_)) => HotspotKind::ClickOrHold,
        (None, Some(_)) => HotspotKind::Hold,
        _ => HotspotKind::Toggle,
    }
}

fn lever_travel_m(piece: Option<&Piece>, kind: HotspotKind) -> f32 {
    if let Some(p) = piece {
        if let Anim::Move(m) = p.anim {
            if m.slide.gain.abs() > 0.01 {
                return (m.slide.gain.abs() * 1.45).clamp(0.08, 0.16);
            }
            if m.turn[0].gain.abs() > 0.05 {
                let arm = p.hotspot.map(|h| Vec3::from(h.at).length()).unwrap_or(0.22).max(0.08);
                return (arm * m.turn[0].gain.abs()).clamp(0.09, 0.17);
            }
        }
    }
    match kind {
        HotspotKind::Throttle => 0.12,
        HotspotKind::Mixture | HotspotKind::Propeller => 0.09,
        _ => 0.10,
    }
}

/// Unit body-frame direction that moves a flap lever toward `flaps_up` (retracted),
/// derived from the piece's actual `Input::FlapLever` hinge/slide animation and current
/// pose so floor handbrake levers (e.g. Robin DR400: push down/forward = flaps up,
/// pull up/back = flaps down), pedestal levers (B747/G650: forward = up, back = down),
/// and panel switches (C172/Bonanza/Concorde/F/A-18: up = up, down = down) all follow
/// their real physical motion in VR.
fn flaps_up_dir(piece: Option<&Piece>, piece_pose: Option<Mat4>, hs: &Hotspot) -> Vec3 {
    if let Some(p) = piece {
        if let Anim::Move(m) = p.anim {
            let mat = piece_pose.unwrap_or(Mat4::IDENTITY);
            if let Some(t) = m.turn.iter().find(|d| d.input == Input::FlapLever && d.gain.abs() > 1e-4) {
                let axis = mat.transform_vector3(p.axis).normalize_or_zero();
                let arm = mat.transform_vector3(Vec3::from(hs.at) - p.pivot);
                // Increasing `FlapLever` extends flaps (`flaps_down`), so `v_down = (axis * gain) × arm`.
                let v_up = -(axis * t.gain).cross(arm);
                if v_up.length_squared() > 1e-6 {
                    return v_up.normalize();
                }
            }
            if m.slide.input == Input::FlapLever && m.slide.gain.abs() > 1e-4 {
                let v_up = -mat.transform_vector3(m.dir * m.slide.gain);
                if v_up.length_squared() > 1e-6 {
                    return v_up.normalize();
                }
            }
        }
    }
    Vec3::new(0.0, 0.75, -0.65).normalize()
}

/// Hand travel per flap notch (m), matched to the lever's physical arc and notch count.
fn flap_step_m(piece: Option<&Piece>, hs: &Hotspot, max_notch: u8) -> f32 {
    let notches = max_notch.max(1) as f32;
    if let Some(p) = piece {
        if let Anim::Move(m) = p.anim {
            if let Some(t) = m.turn.iter().find(|d| d.input == Input::FlapLever && d.gain.abs() > 1e-4) {
                let arm = (Vec3::from(hs.at) - p.pivot).length();
                return (arm * t.gain.abs() / notches).clamp(0.028, 0.055);
            }
            if m.slide.input == Input::FlapLever && m.slide.gain.abs() > 1e-4 {
                return (m.slide.gain.abs() / notches).clamp(0.028, 0.055);
            }
        }
    }
    0.032
}

fn apply_hold_action(id: Option<&'static str>, out: &mut VrStepOut) {
    match id {
        Some("starter") => out.starter = true,
        Some("reversers") => out.reverse = true,
        Some("brake_left") => out.brake_left = 1.0,
        Some("brake_right") => out.brake_right = 1.0,
        _ => {}
    }
}

fn mesh_bounds(verts: &[crate::render::PartVertex]) -> (Vec3, Vec3) {
    verts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| {
        let p = Vec3::from(v.pos);
        (lo.min(p), hi.max(p))
    })
}

fn dist_to_aabb(p: Vec3, lo: Vec3, hi: Vec3) -> f32 {
    let clamped = p.clamp(lo, hi);
    (p - clamped).length()
}

/// Signed clockwise roll delta (radians, + = roll right) around body Z from `prev` to `cur`.
fn delta_roll_z(prev: Quat, cur: Quat) -> f32 {
    let mut dq = cur * prev.conjugate();
    if dq.w < 0.0 {
        dq = -dq;
    }
    // Positive right-hand rotation around +Z is counter-clockwise (roll left), so negate.
    -2.0 * dq.z.atan2(dq.w)
}

/// Signed counter-clockwise angle from 2D vector `a` to `b` in radians.
fn signed_angle_xy(a: Vec2, b: Vec2) -> f32 {
    let cross = a.x * b.y - a.y * b.x;
    let dot = a.dot(b);
    cross.atan2(dot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft;

    fn neutral_hand(pos: Vec3) -> XrHandState {
        XrHandState {
            tracked: true,
            pos,
            rot: Quat::IDENTITY,
            ..Default::default()
        }
    }

    #[test]
    fn discovers_yokes_and_sticks_across_crewed_aircraft() {
        for spec in aircraft::BUILTIN {
            if spec.rocket().is_some() {
                continue;
            }
            let model = (spec.build)();
            let vc = VrCockpit::new(spec, &model.pieces);
            assert!(
                !vc.controls.is_empty(),
                "aircraft {} should have at least one grabbable yoke or stick",
                spec.name
            );
        }
    }

    #[test]
    fn yoke_one_handed_push_pull_and_turn_steers_c172() {
        let spec = &aircraft::c172::SPEC;
        let model = (spec.build)();
        let mut vc = VrCockpit::new(spec, &model.pieces);
        let controls = aircraft::cockpit_controls(spec, &model.pieces);
        let pose = vec![Mat4::IDENTITY; model.pieces.len()];
        let hs = aircraft::animated_hotspots(&controls, &pose);

        // Pilot left yoke left horn in C172 is around (-0.40, -0.04, -1.05).
        let left_horn = Vec3::new(-0.40, -0.04, -1.05);
        let hand_rel = left_horn - spec.eye;

        // 1. Hand away from controls -> Idle mode.
        let far_hand = neutral_hand(Vec3::new(0.0, -0.2, 0.25));
        let out_idle = vc.step(1.0 / 90.0, spec.eye, [far_hand, XrHandState::default()], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert_eq!(out_idle.orbs[0].unwrap().mode, HandMode::Idle);

        // 2. Hand on yoke horn without squeezing -> Hover mode.
        let hover_hand = neutral_hand(hand_rel);
        let out_hover = vc.step(1.0 / 90.0, spec.eye, [hover_hand, XrHandState::default()], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert_eq!(out_hover.orbs[0].unwrap().mode, HandMode::Hover);

        // 3. Squeeze grip on yoke horn -> Active mode, starts at neutral.
        let mut grab_hand = hover_hand;
        grab_hand.squeeze = 0.9;
        let out_grab = vc.step(1.0 / 90.0, spec.eye, [grab_hand, XrHandState::default()], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert_eq!(out_grab.orbs[0].unwrap().mode, HandMode::Active);
        assert!(out_grab.pitch.unwrap().abs() < 1e-4);
        assert!(out_grab.roll.unwrap().abs() < 1e-4);

        // 4. Pull hand back (+Z = 5.5 cm -> +0.5 pitch nose up) and twist clockwise (-Z rot 0.425 rad -> +0.5 roll right).
        let mut steer_hand = grab_hand;
        steer_hand.pos.z += 0.055;
        steer_hand.rot = Quat::from_rotation_z(-0.425);
        let out_steer = vc.step(1.0 / 90.0, spec.eye, [steer_hand, XrHandState::default()], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!(out_steer.pitch.unwrap() > 0.45 && out_steer.pitch.unwrap() < 0.55, "pitch={:?}", out_steer.pitch);
        assert!(out_steer.roll.unwrap() > 0.20, "roll right expected positive, got {:?}", out_steer.roll);
    }

    #[test]
    fn yoke_two_handed_turning_and_seamless_handoff() {
        let spec = &aircraft::c172::SPEC;
        let model = (spec.build)();
        let mut vc = VrCockpit::new(spec, &model.pieces);
        let controls = aircraft::cockpit_controls(spec, &model.pieces);
        let pose = vec![Mat4::IDENTITY; model.pieces.len()];
        let hs = aircraft::animated_hotspots(&controls, &pose);

        let l_pos = Vec3::new(-0.40, -0.04, -1.05) - spec.eye;
        let r_pos = Vec3::new(-0.14, -0.04, -1.05) - spec.eye;

        let mut lh = neutral_hand(l_pos);
        let mut rh = neutral_hand(r_pos);
        // Hover first
        vc.step(1.0 / 90.0, spec.eye, [lh, rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        // Grab with both hands
        lh.squeeze = 0.9;
        rh.squeeze = 0.9;
        vc.step(1.0 / 90.0, spec.eye, [lh, rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!(vc.two_hand_yoke.is_some());

        // Turn wheel clockwise: left hand goes UP (+Y), right hand goes DOWN (-Y), push forward (-Z).
        lh.pos.y += 0.06;
        rh.pos.y -= 0.06;
        lh.pos.z -= 0.04;
        rh.pos.z -= 0.04;
        let out = vc.step(1.0 / 90.0, spec.eye, [lh, rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!(out.roll.unwrap() > 0.45, "two-hand clockwise roll should be positive, got {:?}", out.roll);
        assert!(out.pitch.unwrap() < -0.30, "pushing forward should pitch nose down, got {:?}", out.pitch);

        // Release right hand -> left hand seamlessly retains current pitch & roll without jumping!
        let saved_pitch = out.pitch.unwrap();
        let saved_roll = out.roll.unwrap();
        rh.squeeze = 0.0;
        let out2 = vc.step(1.0 / 90.0, spec.eye, [lh, rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!((out2.pitch.unwrap() - saved_pitch).abs() < 0.02);
        assert!((out2.roll.unwrap() - saved_roll).abs() < 0.02);
    }

    #[test]
    fn throttle_push_pull_and_switch_click_work_with_haptics() {
        let spec = &aircraft::c172::SPEC;
        let model = (spec.build)();
        let mut vc = VrCockpit::new(spec, &model.pieces);
        let controls = aircraft::cockpit_controls(spec, &model.pieces);
        let pose = vec![Mat4::IDENTITY; model.pieces.len()];
        let hs = aircraft::animated_hotspots(&controls, &pose);

        let thr_idx = controls.iter().position(|(h, _)| h.wheel == Some(("throttle_up", "throttle_down"))).unwrap();
        let thr_pos = Vec3::from(hs[thr_idx].at) - spec.eye;

        let mut rh = neutral_hand(thr_pos);
        // Hover over throttle
        let out_h = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot { throttle: 0.2, ..Default::default() }, true);
        assert_eq!(out_h.orbs[1].unwrap().mode, HandMode::Hover);
        assert_eq!(out_h.orbs[1].unwrap().target, Some(VrHandTarget::Hotspot(thr_idx)));
        assert!(out_h.orbs[1].unwrap().target_pos.is_some());

        // Grab throttle and push forward (-Z)
        rh.trigger = 0.9;
        vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot { throttle: 0.2, ..Default::default() }, true);
        rh.pos.z -= 0.05;
        let out_push = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot { throttle: 0.2, ..Default::default() }, true);
        assert!(out_push.throttle_set.unwrap() > 0.65, "pushing throttle forward should increase throttle: {:?}", out_push.throttle_set);

        // Haptics should fire on throttle push.
        let haps = vc.step_flight_haptics(FlightFeedback { dt: 1.0 / 90.0, ..Default::default() });
        assert!(haps[1].is_some_and(|c| c.amplitude > 0.1));

        // Release and click Master switch
        rh.trigger = 0.0;
        vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        let m_idx = controls.iter().position(|(h, _)| h.click == Some("master")).unwrap();
        rh.pos = Vec3::from(hs[m_idx].at) - spec.eye;
        vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        rh.trigger = 0.9;
        let out_click = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!(out_click.events.contains(&VrCockpitEvent::TriggerAction("master")));
    }

    #[test]
    fn turbulence_and_touchdown_produce_flight_haptics() {
        let spec = &aircraft::c172::SPEC;
        let model = (spec.build)();
        let mut vc = VrCockpit::new(spec, &model.pieces);

        // Airborne in strong turbulence -> haptic rumble on both hands.
        let h1 = vc.step_flight_haptics(FlightFeedback {
            dt: 1.0 / 90.0,
            gust_speed: 9.0,
            sigma_w: 3.5,
            on_ground: false,
            sf: Vec3::new(0.0, 12.0, 0.0),
            ..Default::default()
        });
        assert!(h1[0].is_some_and(|c| c.amplitude > 0.15));
        assert!(h1[1].is_some_and(|c| c.amplitude > 0.15));

        // Touchdown transition -> strong landing impulse.
        let h2 = vc.step_flight_haptics(FlightFeedback {
            dt: 1.0 / 90.0,
            on_ground: true,
            sink_rate: 3.0,
            sf: Vec3::new(0.0, 14.0, 0.0),
            ..Default::default()
        });
        assert!(h2[0].is_some_and(|c| c.amplitude > 0.5));
    }

    #[test]
    fn vr_utility_buttons_open_flight_menu_recenter_and_pause_without_bounce() {
        let spec = &aircraft::c172::SPEC;
        let model = (spec.build)();
        let mut vc = VrCockpit::new(spec, &model.pieces);
        let controls = aircraft::cockpit_controls(spec, &model.pieces);
        let pose = vec![Mat4::IDENTITY; model.pieces.len()];
        let hs = aircraft::animated_hotspots(&controls, &pose);

        // Right B opens the flight/radio radial menu.
        let mut rh = neutral_hand(Vec3::new(0.2, -0.2, -0.3));
        rh.secondary_btn = true;
        let out_b = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!(out_b.events.contains(&VrCockpitEvent::FlightMenu));
        assert!(!out_b.events.contains(&VrCockpitEvent::RecenterVr));

        // Left Y recenters VR view, Left Menu opens Pause menu.
        let mut lh = neutral_hand(Vec3::new(-0.2, -0.2, -0.3));
        lh.secondary_btn = true;
        lh.menu_btn = true;
        let out_l = vc.step(1.0 / 90.0, spec.eye, [lh, XrHandState::default()], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!(out_l.events.contains(&VrCockpitEvent::RecenterVr));
        assert!(out_l.events.contains(&VrCockpitEvent::PauseMenu));

        // Syncing buttons while menu is open prevents bounce when returning to cockpit.
        let mut rh_stick = neutral_hand(Vec3::new(0.2, -0.2, -0.3));
        rh_stick.thumbstick_click = true;
        vc.sync_buttons([XrHandState::default(), rh_stick]);
        let out_held = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh_stick], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!(!out_held.events.contains(&VrCockpitEvent::FlightMenu));

        // Releasing and clicking Right Stick Click opens FlightMenu cleanly.
        rh_stick.thumbstick_click = false;
        vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh_stick], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        rh_stick.thumbstick_click = true;
        let out_click = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh_stick], &model.pieces, &pose, &controls, &hs, AircraftSnapshot::default(), true);
        assert!(out_click.events.contains(&VrCockpitEvent::FlightMenu));
    }

    #[test]
    fn dr400_handbrake_flap_lever_follows_physical_up_and_down_motion() {
        let spec = &aircraft::dr400::SPEC;
        let model = (spec.build)();
        let mut vc = VrCockpit::new(spec, &model.pieces);
        let controls = aircraft::cockpit_controls(spec, &model.pieces);
        let flap_idx = controls.iter().position(|(h, _)| h.wheel == Some(("flaps_up", "flaps_down"))).unwrap();
        let piece_idx = controls[flap_idx].1.expect("DR400 flap lever piece");

        let make_pose = |notch: u8| {
            let mut pose = vec![Mat4::IDENTITY; model.pieces.len()];
            let p = &model.pieces[piece_idx];
            let frac = notch as f32 / spec.max_notch() as f32;
            let angle = frac * 0.6;
            pose[piece_idx] = Mat4::from_translation(p.pivot)
                * Mat4::from_axis_angle(p.axis, angle)
                * Mat4::from_translation(-p.pivot);
            pose
        };

        // Start at notch 0 (0°, lever flat on floor): hover and grab with right hand.
        let mut notch = 0u8;
        let mut pose = make_pose(notch);
        let mut hs = aircraft::animated_hotspots(&controls, &pose);
        let mut rh = neutral_hand(Vec3::from(hs[flap_idx].at) - spec.eye);
        let snap = |n| AircraftSnapshot { flap_set: n, max_flap_notch: spec.max_notch(), ..Default::default() };

        vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, snap(notch), true);
        rh.trigger = 0.9;
        vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, snap(notch), true);

        // Pulling the handbrake lever UP and BACK (+Y, +Z) must lower flaps (`flaps_down`).
        rh.pos += Vec3::new(0.0, 0.065, 0.025);
        let out_dn1 = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, snap(notch), true);
        assert!(out_dn1.events.contains(&VrCockpitEvent::TriggerAction("flaps_down")), "pulling DR400 lever up/back should extend flaps: {:?}", out_dn1.events);
        notch = 1;
        pose = make_pose(notch);
        hs = aircraft::animated_hotspots(&controls, &pose);

        rh.pos += Vec3::new(0.0, 0.060, 0.040);
        let out_dn2 = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, snap(notch), true);
        assert!(out_dn2.events.contains(&VrCockpitEvent::TriggerAction("flaps_down")), "continuing pull should reach full flaps: {:?}", out_dn2.events);
        notch = 2;
        pose = make_pose(notch);
        hs = aircraft::animated_hotspots(&controls, &pose);

        // Pulling past full flaps must NOT emit extra phantom events or resist pushing back down.
        rh.pos += Vec3::new(0.0, 0.08, 0.04);
        let out_stop = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, snap(notch), true);
        assert!(out_stop.events.is_empty(), "end stop should not emit phantom flap actions: {:?}", out_stop.events);

        // Pushing the handbrake lever DOWN and FORWARD (-Y, -Z) must raise flaps (`flaps_up`) reliably.
        rh.pos += Vec3::new(0.0, -0.055, -0.045);
        let out_up1 = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, snap(notch), true);
        assert!(out_up1.events.contains(&VrCockpitEvent::TriggerAction("flaps_up")), "pushing DR400 lever down/forward should retract flaps: {:?}", out_up1.events);
        notch = 1;
        pose = make_pose(notch);
        hs = aircraft::animated_hotspots(&controls, &pose);

        rh.pos += Vec3::new(0.0, -0.065, -0.030);
        let out_up2 = vc.step(1.0 / 90.0, spec.eye, [XrHandState::default(), rh], &model.pieces, &pose, &controls, &hs, snap(notch), true);
        assert!(out_up2.events.contains(&VrCockpitEvent::TriggerAction("flaps_up")), "continuing push down/forward should retract flaps to 0: {:?}", out_up2.events);
    }

    #[test]
    fn flap_lever_up_direction_matches_animated_piece_displacement_on_all_aircraft() {
        for spec in aircraft::BUILTIN {
            let model = (spec.build)();
            let controls = aircraft::cockpit_controls(spec, &model.pieces);
            for (hs, piece_opt) in &controls {
                if hs.wheel != Some(("flaps_up", "flaps_down")) {
                    continue;
                }
                let Some(pi) = *piece_opt else { continue };
                let p = &model.pieces[pi];
                let Anim::Move(m) = p.anim else { continue };
                let Some(t) = m.turn.iter().find(|d| d.input == Input::FlapLever && d.gain.abs() > 1e-4) else { continue };
                // Compare animated grip position at FlapLever=0 (up) vs FlapLever=1 (down).
                let at_frac = |f: f32| {
                    let ang = f * t.gain + t.offset;
                    let mat = Mat4::from_translation(p.pivot)
                        * Mat4::from_axis_angle(p.axis, ang)
                        * Mat4::from_translation(-p.pivot);
                    (mat, mat.transform_point3(Vec3::from(hs.at)))
                };
                let (mat0, pos0) = at_frac(0.0);
                let (mat1, pos1) = at_frac(1.0);
                let actual_up = (pos0 - pos1).normalize();
                let dir0 = flaps_up_dir(Some(p), Some(mat0), hs);
                let dir1 = flaps_up_dir(Some(p), Some(mat1), hs);
                assert!(
                    dir0.dot(actual_up) > 0.7 && dir1.dot(actual_up) > 0.7,
                    "{} flap lever up_dir ({:?}, {:?}) must agree with actual retracted direction {:?}",
                    spec.name, dir0, dir1, actual_up
                );
            }
        }
    }
}
