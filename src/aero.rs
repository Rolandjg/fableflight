//! The surface-element aerodynamics: the wing, the tailplane and the fin cut
//! into spanwise strips, each flying in its own local airflow, the forces and
//! moments summed about the centre of gravity.
//!
//! Each strip sees the aircraft's airspeed plus the velocity its own position
//! adds as the aircraft rotates (ω × r: what damps a roll, a pitch and a yaw),
//! the wing's downwash (at the tail, lagged by the time the air takes to get
//! there: the damping of a quick pitch), the propeller's slipstream and its
//! swirl (the tail's authority at low speed and full power, the left-turning
//! tendency), and ground effect. It works out its angle of attack, adds what
//! its control surface or flap does, and takes a lift and drag coefficient
//! from its airfoil: linear up to its own maximum, then a separated flat-plate
//! flow it leaves and rejoins with a lag. Stalls start where the local angle
//! of attack is highest (at the root of a washed-out wing, at the tip of a
//! swept one) and a wing drops because one side really is flying at a higher
//! angle than the other. Damage takes strips away: a shortened wing loses its
//! outboard strips, a missing elevator its control.
//!
//! What strips cannot give is added as coefficients (`Fuse`): the fuselage's
//! own unstable pitching and yawing moments and side force, and the roll due
//! to sideslip of the wing-fuselage junction (a high wing's extra stability).
//!
//! Everything comes from data: an aircraft opts in with a `Rig` on its `Spec`
//! and the airframe is built from that and the geometry the `Spec` already
//! has (the wing's sections, the aileron span). Nothing here knows which
//! aircraft it is flying.
//!
//! Frame: the body frame, +X right, +Y up, -Z forward, metres.

use crate::aircraft::Spec;
use glam::DVec3;

/// An airfoil section's characteristics (2-D, per radian).
#[derive(Clone, Copy, Debug)]
pub struct Foil2d {
    /// lift-curve slope (per rad; thin-airfoil theory gives 2π, real sections ~5.7-6.3)
    pub cl_a: f64,
    /// zero-lift angle (rad; negative for a cambered section)
    pub a0: f64,
    /// maximum lift coefficient, clean
    pub cl_max: f64,
    /// minimum drag coefficient
    pub cd0: f64,
    /// pitching moment about the quarter chord
    pub cm0: f64,
}

impl Foil2d {
    /// A symmetric tail section (NACA 0009-0012).
    pub const SYMMETRIC: Foil2d = Foil2d { cl_a: 6.0, a0: 0.0, cl_max: 1.2, cd0: 0.0075, cm0: 0.0 };
}

/// A straight-tapered surface, given by its quarter-chord line (the right or
/// upper half; a horizontal surface is mirrored to the left).
#[derive(Clone, Copy, Debug)]
pub struct Planform {
    /// quarter-chord point at the root and at the tip (body frame, m)
    pub root: DVec3,
    pub tip: DVec3,
    /// chord at the root and at the tip (m)
    pub chord: [f64; 2],
    /// incidence (rad, leading edge up) at the root and the tip
    pub incidence: [f64; 2],
}

/// A control surface or flap: where it runs along the span (m from the root
/// of its surface), its chord as a fraction of the surface's, and its travel
/// (rad) trailing edge up (or left) and down (or right), both positive.
#[derive(Clone, Copy, Debug)]
pub struct Ctrl {
    pub span: [f64; 2],
    pub chord: f64,
    pub travel: [f64; 2],
}

/// What strips cannot show: the fuselage and the wing-fuselage junction, as
/// stability-axis coefficients in the usual sign convention (per radian; on
/// the wing area, chord and span).
#[derive(Clone, Copy, Debug)]
pub struct Fuse {
    /// pitching moment from angle of attack (positive: nose-up, destabilising)
    pub cm_a: f64,
    /// yawing moment from sideslip (negative: destabilising)
    pub cn_b: f64,
    /// rolling moment from sideslip (negative: stabilising, more for a high wing)
    pub cl_b: f64,
    /// side force from sideslip (negative)
    pub cy_b: f64,
}

/// An aircraft's aerodynamic rig: what the surface-element model needs that
/// the `Spec` does not already say.
#[derive(Clone, Copy, Debug)]
pub struct Rig {
    /// the wing's section; spanwise strips per wing half
    pub wing_foil: Foil2d,
    pub wing_strips: usize,
    /// span efficiency (Oswald), for the induced drag
    pub oswald: f64,
    /// flaps: span and chord, their full deflection taken from `Spec::flaps`;
    /// the extra maximum lift fully down
    pub flap: Ctrl,
    pub flap_dcl_max: f64,
    /// ailerons: chord and travel (span from `Spec::aileron`)
    pub aileron_chord: f64,
    pub aileron_travel: [f64; 2],
    /// a tailless aircraft (a delta): no horizontal tail; the wing's trailing
    /// edge surfaces (the flap and aileron spans) are elevons, worked by both
    /// the stick's pitch and its roll, with the elevator's travel
    pub elevons: bool,
    /// the horizontal tail, its section, the elevator (span over the whole
    /// half; chord 1 for an all-moving stabilator), and the trim's range as
    /// elevator deflection (nose down, nose up)
    pub htail: Planform,
    pub htail_foil: Foil2d,
    pub elevator: Ctrl,
    pub trim: [f64; 2],
    /// the fin, its section and the rudder (travel left, right)
    pub fin: Planform,
    pub fin_foil: Foil2d,
    pub rudder: Ctrl,
    /// the fuselage's share
    pub fuse: Fuse,
    /// how much of the wing's downwash reaches the tail (1: all of the
    /// lifting-line value), and the tail's dynamic pressure ratio outside the slipstream
    pub downwash: f64,
    pub eta_tail: f64,
    /// the propeller hub (body frame), for its slipstream; the slipstream's
    /// swirl as a fraction of its axial speed (sense from the propeller's turn:
    /// positive clockwise from the cockpit)
    pub prop_hub: DVec3,
    pub swirl: f64,
}

impl Rig {
    /// A first rig for any aircraft, from what its `Spec` already says: the
    /// wing from its sections, the section's lift and stall from the whole
    /// aircraft's polar, the tail from its tips (`Spec::tail`) sized by typical
    /// tail volume coefficients (Raymer, Aircraft Design, table 6.4), and
    /// typical control travel. It flies; tuning it to the real aircraft (its
    /// tail areas, sections and travel) is what an aircraft's own `Rig` is for.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn derive(spec: &Spec) -> Rig {
        let jet = spec.is_jet();
        let span = spec.span as f64;
        let s = spec.wing_area;
        let ar = (2.0 * span).powi(2) / s;
        let a2d = 6.0;
        // The aircraft's lift slope and maximum lift, back to a section's.
        let cl_max_3d = spec.cl0 + spec.cl_a * spec.stall_alpha;
        let a0 = -spec.cl0 / spec.cl_a;
        let wing_foil = Foil2d { cl_a: a2d, a0, cl_max: (cl_max_3d * 1.08).max(0.8), cd0: 0.006, cm0: if jet { -0.02 } else { -0.045 } };
        let oswald = (1.0 / (std::f64::consts::PI * ar * spec.k_ind)).clamp(0.5, 0.95);
        let root = (spec.wing_sect)(0.0);
        let mac = s / (2.0 * span);
        let wing_ac = root.le.z as f64 + 0.25 * root.c as f64;
        let t = &spec.tail;
        // Horizontal tail: about 0.7 (light aircraft) to 0.9 (airliners, fighters) tail volume.
        let st = t.stab_tip;
        let lt = (st.z - wing_ac).max(0.3 * span);
        let sh = if jet { 0.85 } else { 0.7 } * s * mac / lt;
        let bh = (st.x * 2.0).max(0.5);
        let ch = sh / bh;
        let htail = Planform {
            root: DVec3::new(0.0, st.y, st.z - 0.3 * ch),
            tip: DVec3::new(st.x, st.y, st.z),
            chord: [1.25 * ch, 0.75 * ch],
            incidence: [-0.03, -0.03],
        };
        // Fin: 0.04 (light aircraft) to 0.08 (jets).
        let (fb, ft) = (t.fin_base, t.fin_tip);
        let lv = (fb.z - wing_ac).max(0.3 * span);
        let sv = if jet { 0.08 } else { 0.04 } * s * 2.0 * span / lv;
        let hv = (ft.y - fb.y).max(0.5);
        let cv = sv / hv;
        let fin = Planform { root: DVec3::new(0.0, fb.y, fb.z), tip: DVec3::new(0.0, ft.y, ft.z), chord: [1.3 * cv, 0.7 * cv], incidence: [0.0, 0.0] };
        let deg = |d: f64| d.to_radians();
        // No tailplane if the "stabiliser tip" lies on the wing itself (a delta's elevons).
        let ws = (spec.wing_sect)(st.x as f32);
        let elevons = (st.x as f32) < spec.span && st.z <= (ws.le.z + ws.c) as f64 + 0.5;
        // An agile jet's tailplane moves as a whole (a stabilator).
        let stabilator = jet && spec.g_limit >= 6.0;
        let nose = spec.contacts.iter().map(|c| c.0[2]).fold(0.0f64, f64::min);
        Rig {
            wing_foil,
            wing_strips: 10,
            oswald,
            flap: Ctrl { span: [0.12 * span, spec.aileron[0] as f64], chord: 0.28, travel: [0.0, spec.flaps.last().copied().unwrap_or(0.0).to_radians()] },
            flap_dcl_max: 2.0 * spec.flap_cl,
            aileron_chord: 0.25,
            aileron_travel: [deg(20.0), deg(15.0)],
            elevons,
            htail,
            htail_foil: Foil2d::SYMMETRIC,
            elevator: if stabilator { Ctrl { span: [0.0, st.x], chord: 1.0, travel: [deg(20.0), deg(10.0)] } } else { Ctrl { span: [0.0, st.x], chord: 0.35, travel: [deg(25.0), deg(20.0)] } },
            trim: [deg(10.0), deg(10.0)],
            fin,
            fin_foil: Foil2d::SYMMETRIC,
            rudder: Ctrl { span: [0.0, hv], chord: 0.3, travel: [deg(25.0), deg(25.0)] },
            fuse: Fuse { cm_a: 0.3, cn_b: -0.04, cl_b: -0.03, cy_b: -0.2 },
            downwash: 1.0,
            eta_tail: 0.9,
            prop_hub: DVec3::new(0.0, 0.0, nose),
            swirl: if jet { 0.0 } else { 0.25 },
        }
    }
}

/// Which surface a strip belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// side 0 left, 1 right
    Wing(usize),
    Tail(usize),
    Fin,
}

/// One spanwise strip.
#[derive(Clone, Debug)]
pub struct Strip {
    pub part: Part,
    /// span station (m from the centreline, or from the fin's root)
    pub station: f64,
    /// quarter-chord centre (body frame)
    pub at: DVec3,
    /// unit vectors: along the chord (leading to trailing edge), the
    /// surface's normal (the side lift is positive toward), along the span
    pub chord_dir: DVec3,
    pub normal: DVec3,
    pub span_dir: DVec3,
    pub chord: f64,
    pub area: f64,
    pub foil: Foil2d,
    /// 3-D lift slope and induced-drag factor of its surface
    pub cl_a: f64,
    pub k_ind: f64,
    /// effectiveness (rad of angle of attack per rad) of its flap and of its control surface
    pub flap_tau: f64,
    pub ctrl_tau: f64,
}

/// The airframe as the model flies it, built once per aircraft from its rig.
#[derive(Clone, Debug)]
pub struct Airframe {
    pub rig: Rig,
    pub strips: Vec<Strip>,
    /// wing: reference area, span, mean aerodynamic chord and its leading edge's z
    pub s_ref: f64,
    pub b_ref: f64,
    pub mac: f64,
    pub mac_le: DVec3,
    pub ar: f64,
    /// the drag area (m²) of everything but the surfaces' sections: what
    /// makes the total parasite drag the `Spec`'s cd0
    pub cda_body: f64,
    /// flaps' full deflection (rad)
    pub flap_max: f64,
    /// wing to tail distance (m), for the downwash's lag
    pub tail_arm: f64,
}

/// Lift-curve slope of a finite surface from its section's (Helmbold).
pub fn slope_3d(a2d: f64, ar: f64) -> f64 {
    let x = a2d / (std::f64::consts::PI * ar);
    a2d / ((1.0 + x * x).sqrt() + x)
}

/// A flap or control surface's effectiveness: the angle of attack its
/// deflection is worth, per radian (thin-airfoil theory, chord fraction `cf`).
pub fn flap_tau(cf: f64) -> f64 {
    if cf <= 0.0 {
        return 0.0;
    }
    let th = (2.0 * cf.min(1.0) - 1.0).clamp(-1.0, 1.0).acos();
    1.0 - (th - th.sin()) / std::f64::consts::PI
}

/// The elevator's (or the elevons' pitch share of) deflection, rad,
/// trailing edge down positive: back on the stick raises it (less lift
/// behind the centre of gravity, nose up).
fn pitch_defl(inp: &Input, rig: &Rig) -> f64 {
    let e = inp.elevator;
    let stick = if e > 0.0 { -e * rig.elevator.travel[0] } else { -e * rig.elevator.travel[1] };
    (stick - inp.trim).clamp(-rig.elevator.travel[0], rig.elevator.travel[1])
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Airframe {
    pub fn build(spec: &Spec, rig: &Rig) -> Self {
        let mut strips = Vec::new();
        // The wing, from the `Spec`'s own sections: strips spaced closer
        // toward the tip (cosine spacing), where the load changes fastest.
        let span = spec.span as f64;
        let n = rig.wing_strips.max(4);
        let edge = |i: usize| span * (0.5 * std::f64::consts::PI * i as f64 / n as f64).sin();
        let (mut area, mut mac_num, mut le_num) = (0.0, 0.0, DVec3::ZERO);
        let mut wing = Vec::new();
        for i in 0..n {
            let (y0, y1) = (edge(i), edge(i + 1));
            let ym = 0.5 * (y0 + y1);
            let s = (spec.wing_sect)(ym as f32);
            let (c, le) = (s.c as f64, s.le.as_dvec3());
            let cdir = s.cdir.as_dvec3().normalize();
            let qc = le + cdir * (0.25 * c);
            let a = c * (y1 - y0);
            area += 2.0 * a;
            mac_num += 2.0 * a * c;
            le_num += le * (2.0 * a);
            wing.push((ym, qc, cdir, s.up.as_dvec3().normalize(), c, a));
        }
        let mac = mac_num / area;
        let mac_le = DVec3::new(0.0, le_num.y / area, le_num.z / area);
        let ar = (2.0 * span).powi(2) / area;
        let cl_a = slope_3d(rig.wing_foil.cl_a, ar);
        let k_ind = 1.0 / (std::f64::consts::PI * rig.oswald * ar);
        let ail = [spec.aileron[0] as f64, spec.aileron[1] as f64];
        for (ym, qc, cdir, up, c, a) in wing {
            for side in 0..2 {
                let m = if side == 0 { DVec3::new(-1.0, 1.0, 1.0) } else { DVec3::ONE };
                let span_dir = up.cross(cdir).normalize() * m;
                let in_flap = ym >= rig.flap.span[0] && ym <= rig.flap.span[1];
                let in_ail = (ym >= ail[0] && ym <= ail[1]) || (rig.elevons && in_flap);
                strips.push(Strip {
                    part: Part::Wing(side),
                    station: ym,
                    at: qc * m,
                    chord_dir: cdir,
                    normal: up * m,
                    span_dir,
                    chord: c,
                    area: a,
                    foil: rig.wing_foil,
                    cl_a,
                    k_ind,
                    flap_tau: if in_flap { flap_tau(rig.flap.chord) } else { 0.0 },
                    ctrl_tau: if in_ail { flap_tau(rig.aileron_chord) } else { 0.0 },
                });
            }
        }
        // The tail surfaces: straight-tapered, uniform strips.
        let mut surface = |pf: &Planform, foil: Foil2d, ctrl: &Ctrl, n: usize, fin: bool| {
            let len = (pf.tip - pf.root).length();
            let s_half = 0.5 * (pf.chord[0] + pf.chord[1]) * len;
            let ar = if fin { 2.0 * len * len / s_half } else { (2.0 * len).powi(2) / (2.0 * s_half) };
            let (cl_a, k) = (slope_3d(foil.cl_a, ar), 1.0 / (std::f64::consts::PI * 0.8 * ar));
            for i in 0..n {
                let u = (i as f64 + 0.5) / n as f64;
                let c = pf.chord[0] + (pf.chord[1] - pf.chord[0]) * u;
                let inc = pf.incidence[0] + (pf.incidence[1] - pf.incidence[0]) * u;
                let at = pf.root + (pf.tip - pf.root) * u;
                let station = len * u;
                let along = (pf.tip - pf.root).normalize();
                let cdir = DVec3::new(0.0, -inc.sin(), inc.cos());
                let sides: &[usize] = if fin { &[0] } else { &[0, 1] };
                for &side in sides {
                    let m = if fin || side == 1 { DVec3::ONE } else { DVec3::new(-1.0, 1.0, 1.0) };
                    let in_ctrl = station >= ctrl.span[0] && station <= ctrl.span[1];
                    strips.push(Strip {
                        part: if fin { Part::Fin } else { Part::Tail(side) },
                        station,
                        at: at * m,
                        chord_dir: cdir,
                        normal: if fin { DVec3::NEG_X } else { tail_normal(along * m, cdir) },
                        span_dir: along * m,
                        chord: c,
                        area: c * len / n as f64,
                        foil,
                        cl_a,
                        k_ind: k,
                        flap_tau: 0.0,
                        ctrl_tau: if in_ctrl { flap_tau(ctrl.chord) } else { 0.0 },
                    });
                }
            }
        };
        if !rig.elevons {
            surface(&rig.htail, rig.htail_foil, &rig.elevator, 6, false);
        }
        surface(&rig.fin, rig.fin_foil, &rig.rudder, 5, true);
        let s_ref = spec.wing_area;
        // The parasite drag left for the fuselage, gear fairings, struts and
        // the rest, once the sections' own is counted.
        let profile: f64 = strips.iter().map(|s| s.foil.cd0 * s.area).sum();
        let cda_body = (spec.cd0 * s_ref - profile).max(0.0);
        let flap_max = spec.flaps.last().copied().unwrap_or(0.0).to_radians();
        let tail_arm = (rig.htail.root.z - mac_le.z - 0.25 * mac).max(0.5);
        Airframe { rig: *rig, strips, s_ref, b_ref: 2.0 * span, mac, mac_le, ar, cda_body, flap_max, tail_arm }
    }

    /// The centre of gravity (body frame) for a CG at `frac` of the mean
    /// aerodynamic chord, on the body's centreline.
    pub fn cg_at(&self, frac: f64) -> DVec3 {
        DVec3::new(0.0, 0.0, self.mac_le.z + frac * self.mac)
    }
}

/// The normal of a horizontal surface's strip: perpendicular to its chord
/// and its span, on the upper side.
fn tail_normal(span_dir: DVec3, cdir: DVec3) -> DVec3 {
    let n = cdir.cross(span_dir).normalize();
    if n.y < 0.0 { -n } else { n }
}

/// What the pilot and the aircraft's state set this instant.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// airspeed (body frame, the aircraft's velocity through the air) and rotation (rad/s)
    pub vb: DVec3,
    pub omega: DVec3,
    pub rho: f64,
    /// the centre of gravity (body frame)
    pub cg: DVec3,
    /// elevator (+ nose up), aileron (+ roll right), rudder (+ nose right): -1..1
    pub elevator: f64,
    /// the trim, as elevator deflection (rad, + nose up)
    pub trim: f64,
    pub aileron: f64,
    pub rudder: f64,
    /// fixed rigging (aileron, rudder, as fractions of their travel): what
    /// the factory sets so it flies straight hands off in the cruise
    pub rigging: [f64; 2],
    /// flaps 0..1 of full
    pub flaps: f64,
    /// what is left: per side (left, right) of the ailerons, flaps,
    /// elevators and stabilisers; the fin and rudder; the wing's remaining span station
    pub ail_ok: [f64; 2],
    pub flap_ok: [f64; 2],
    pub elev_ok: [f64; 2],
    pub stab_ok: [f64; 2],
    pub fin_ok: f64,
    pub rud_ok: f64,
    pub wing_span: [f64; 2],
    /// ground effect (1 clear of the ground, less near it)
    pub ge: f64,
    /// the propeller's thrust (N) and disc radius (m), for its slipstream
    pub thrust: f64,
    pub prop_r: f64,
}

/// The model's memory between steps: each strip's separation (0 attached, 1
/// separated) and the downwash on its way from the wing to the tail.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub sep: Vec<f64>,
    pub eps: f64,
    /// the gust at each strip (body frame, m/s): the air's own motion there
    /// beyond what `Input::vb` already has (the gust at the CG); empty: none
    pub gust: Vec<DVec3>,
}

/// What the strips add up to.
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    /// force (body frame, N) and moment about the centre of gravity (N m)
    pub force: DVec3,
    pub moment: DVec3,
    /// the wing's lift coefficient, its centreline angle of attack, its most
    /// separated strip (0..1), and the separation's left-right difference
    pub cl_wing: f64,
    pub alpha: f64,
    pub stall: f64,
    pub stall_lr: f64,
}

impl Airframe {
    /// Forces and moments for this instant; `dt` advances the separation and
    /// the downwash (0 leaves them as they are, for trimming).
    pub fn forces(&self, inp: &Input, st: &mut State, dt: f64) -> Output {
        let rig = &self.rig;
        let v = inp.vb.length();
        let mut out = Output::default();
        if v < 0.5 {
            return out;
        }
        if st.sep.len() != self.strips.len() {
            st.sep = vec![0.0; self.strips.len()];
        }
        let vh = inp.vb / v;
        out.alpha = (-inp.vb.y).atan2(-inp.vb.z);
        let beta = (inp.vb.x / v).clamp(-1.0, 1.0).asin();
        let q = 0.5 * inp.rho * v * v;
        // The slipstream (momentum theory): the induced speed at the disc,
        // twice that far behind it, contracted to a narrower tube.
        let v_ax = (-inp.vb.z).max(0.0);
        let disc = std::f64::consts::PI * inp.prop_r * inp.prop_r;
        let (dv, r_s) = if inp.thrust > 0.0 && disc > 0.0 {
            let vi = 0.5 * (-v_ax + (v_ax * v_ax + 2.0 * inp.thrust / (inp.rho * disc)).sqrt());
            (2.0 * vi, inp.prop_r * ((v_ax + vi) / (v_ax + 2.0 * vi).max(0.1)).sqrt())
        } else {
            (0.0, inp.prop_r)
        };
        // Perpendicular to the flow, "down" for the downwash.
        let down = (DVec3::NEG_Y - vh * vh.dot(DVec3::NEG_Y)).normalize_or_zero();
        let (mut f, mut m) = (DVec3::ZERO, DVec3::ZERO);
        let (mut l_wing, mut s_wing) = (0.0, 0.0);
        let mut sep_side = [0.0f64; 2];
        let mut n_side = [0.0f64; 2];
        for (i, s) in self.strips.iter().enumerate() {
            // What is left of it.
            let (scale, ctrl_ok, flap_ok) = match s.part {
                Part::Wing(side) => (if s.station + 0.5 * s.area / s.chord <= inp.wing_span[side] { 1.0 } else { 0.0 }, inp.ail_ok[side], inp.flap_ok[side]),
                Part::Tail(side) => (inp.stab_ok[side], inp.elev_ok[side], 0.0),
                Part::Fin => (inp.fin_ok, inp.rud_ok, 0.0),
            };
            if scale <= 0.0 {
                continue;
            }
            let r = s.at - inp.cg;
            // The strip's velocity through the air around it.
            let mut fl = inp.vb + inp.omega.cross(r);
            // The gust where this strip is (wind_strips: turbulence, wakes).
            if let Some(g) = st.gust.get(i) {
                fl -= *g;
            }
            let mut eta = 1.0;
            if !matches!(s.part, Part::Wing(_)) {
                eta = rig.eta_tail;
                fl -= down * (st.eps * v);
            }
            // In the slipstream: blown back, and swirled round the axis.
            if dv > 0.0 && s.at.z > rig.prop_hub.z {
                let rel = s.at - rig.prop_hub;
                let radial = DVec3::new(rel.x, rel.y, 0.0);
                let d = radial.length();
                let inside = 1.0 - smooth(0.8 * r_s, 1.2 * r_s, d);
                if inside > 0.0 {
                    let tang = DVec3::NEG_Z.cross(radial).normalize_or_zero() * (d / r_s).min(1.0);
                    fl -= (DVec3::Z * dv + tang * (rig.swirl * dv)) * inside;
                    eta = eta + (1.0 - eta) * inside;
                }
            }
            let fp = fl - s.span_dir * fl.dot(s.span_dir);
            let vl = fp.length();
            if vl < 0.1 {
                continue;
            }
            let alpha = (-fp.dot(s.normal)).atan2(-fp.dot(s.chord_dir));
            // The surface's deflection, as angle of attack.
            let (mut d_ctrl, mut flap) = (0.0, 0.0);
            match s.part {
                Part::Wing(side) => {
                    if s.ctrl_tau > 0.0 {
                        // Right stick: the right aileron up, the left down.
                        let a = (inp.aileron + inp.rigging[0]).clamp(-1.0, 1.0);
                        let x = if side == 1 { -a } else { a };
                        d_ctrl = x * if x > 0.0 { rig.aileron_travel[1] } else { rig.aileron_travel[0] };
                        if rig.elevons {
                            d_ctrl = (d_ctrl + pitch_defl(inp, rig)).clamp(-rig.elevator.travel[0], rig.elevator.travel[1]);
                        }
                    }
                    if s.flap_tau > 0.0 && !rig.elevons {
                        flap = inp.flaps * self.flap_max * flap_ok;
                    }
                }
                Part::Tail(_) => d_ctrl = pitch_defl(inp, rig),
                Part::Fin => {
                    let r = (inp.rudder + inp.rigging[1]).clamp(-1.0, 1.0);
                    d_ctrl = r * if r > 0.0 { rig.rudder.travel[1] } else { rig.rudder.travel[0] };
                }
            }
            // Big deflections lose effectiveness as the flow over them separates.
            let eff = |d: f64| d * (1.0 - 0.45 * smooth(0.17, 0.6, d.abs()));
            let da = s.ctrl_tau * eff(d_ctrl) * ctrl_ok + s.flap_tau * eff(flap);
            let ae = alpha + da - s.foil.a0;
            let cl_lin = s.cl_a * ae;
            let cl_max = s.foil.cl_max + if s.flap_tau > 0.0 { rig.flap_dcl_max * flap / self.flap_max.max(0.01) } else { 0.0 };
            // Separation: past the angle where the section reaches its maximum,
            // the flow breaks away over about 7°; it lets go quickly and
            // reattaches slowly (a few chord lengths of travel later).
            let a_stall = cl_max / s.cl_a;
            let a_neg = 0.8 * cl_max / s.cl_a;
            let target = if ae >= 0.0 { smooth(a_stall - 0.02, a_stall + 0.11, ae) } else { smooth(a_neg - 0.02, a_neg + 0.11, -ae) };
            let sep = &mut st.sep[i];
            if dt > 0.0 {
                let rate = vl / s.chord * if target > *sep { 0.5 } else { 0.08 };
                *sep += (target - *sep) * (dt * rate).min(1.0);
            }
            let sp = *sep;
            let alpha_g = alpha + 0.5 * da;
            let cl_fp = 2.0 * alpha_g.sin() * alpha_g.cos() * 0.95;
            let mut cl = cl_lin * (1.0 - sp) + cl_fp * sp;
            let mut kind = s.k_ind;
            if let Part::Wing(side) = s.part {
                cl *= 1.0 + 0.12 * (1.0 - inp.ge);
                kind *= inp.ge;
                l_wing += cl * s.area;
                s_wing += s.area;
                sep_side[side] += sp * s.area;
                n_side[side] += s.area;
                out.stall = out.stall.max(sp);
            }
            // (A deflected surface's own profile drag; the flaps' is in the `Spec`'s cd_flap.)
            let cd = s.foil.cd0 + kind * cl * cl + sp * 1.8 * alpha_g.sin().powi(2) + 0.25 * (s.ctrl_tau * eff(d_ctrl)).powi(2);
            // The camber a deflection adds pitches the section nose down; once
            // separated, the centre of pressure moves back toward mid-chord.
            let cm = (s.foil.cm0 - 0.15 * s.cl_a * da) * (1.0 - sp) - sp * 0.25 * cl_fp;
            let ql = 0.5 * inp.rho * vl * vl * eta * scale;
            let fh = fp / vl;
            let lift_dir = (s.normal - fh * s.normal.dot(fh)).normalize_or_zero();
            let force = lift_dir * (ql * s.area * cl) - fh * (ql * s.area * cd);
            f += force;
            m += r.cross(force) + s.normal.cross(s.chord_dir) * (ql * s.area * s.chord * cm);
        }
        out.cl_wing = if s_wing > 0.0 { l_wing / s_wing } else { 0.0 };
        out.stall_lr = sep_side[1] / n_side[1].max(1e-6) - sep_side[0] / n_side[0].max(1e-6);
        // The downwash follows the wing's lift after the air has crossed the tail arm.
        let eps_target = rig.downwash * 2.0 * out.cl_wing / (std::f64::consts::PI * self.ar);
        if dt > 0.0 {
            st.eps += (eps_target - st.eps) * (dt * v / self.tail_arm).min(1.0);
        }
        // The fuselage's share, and the drag of everything not a surface.
        let fz = &rig.fuse;
        let (sq, c, b) = (q * self.s_ref, self.mac, self.b_ref);
        let a_body = out.alpha.clamp(-0.5, 0.5);
        let b_body = beta.clamp(-0.5, 0.5);
        f += DVec3::X * (sq * fz.cy_b * b_body) - vh * (q * self.cda_body);
        m += DVec3::new(sq * c * fz.cm_a * a_body, -sq * b * fz.cn_b * b_body, -sq * b * fz.cl_b * b_body);
        out.force = f;
        out.moment = m;
        out
    }
}
