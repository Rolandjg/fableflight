//! Handling regression harness: the dynamic modes and control response of
//! each aircraft, measured by flying it in an empty world the way a flight
//! test card does (trim, a doublet or pulse, hands off, watch it settle).
//!
//! `performance_is_*` in plane.rs checks the steady numbers (speeds, climb,
//! glide); this checks how the aircraft moves between them, which is what
//! the flight model's rotational side decides:
//!
//! - short period: the quick pitch oscillation after an elevator pulse;
//! - phugoid: the slow exchange of speed and height, hands off;
//! - Dutch roll: the yaw-roll wobble after a rudder pulse;
//! - roll rate: the steady rate and time to 45° of bank at full aileron;
//! - spiral: what the bank does in 20 s hands off from 20°.
//!
//! Every run prints a table row (run with `--nocapture` to see it) with
//! reference figures beside it. The assertions are deliberately loose:
//! they catch a mode going unstable or vanishing, not small tuning moves,
//! so a change to the flight model can be judged by the numbers it prints.
//!
//! Reference figures (light aircraft from Roskam, Airplane Flight Dynamics,
//! part I, Appendix B, the Cessna 182 at cruise as the nearest tabulated
//! type; the 747 from Heffley & Jewell, NASA CR-2144, 1972, case at Mach 0.8
//! / 40,000 ft and the power-approach case) are typical values, not
//! measurements of these exact variants: they show where the modes ought to
//! fall, within perhaps ±30%.

use super::*;
use crate::aero;
use crate::aircraft::{b747, c172};

const DT: f64 = 1.0 / 180.0;

fn empty_world() -> World {
    let dir = std::env::temp_dir().join("fablegame-test-cache");
    let src = std::sync::Arc::new(crate::sources::Sources::new(crate::fetch::Fetcher::new(dir, true), String::new()));
    World::new(crate::sources::Loader::new(src, 0), 14)
}

/// An oscillation fitted from its peaks.
#[derive(Debug, Clone, Copy)]
pub struct Osc {
    /// s (0 when there was no oscillation to see)
    pub period: f64,
    /// damping ratio (1 when it never came back past the mean)
    pub zeta: f64,
}

/// Period and damping of `x` (sampled every `dt`) about its final value,
/// from the successive extremes (logarithmic decrement).
pub fn fit_osc(x: &[f64], dt: f64) -> Osc {
    let n = x.len();
    let tail = n / 5;
    let mean = x[n - tail..].iter().sum::<f64>() / tail.max(1) as f64;
    let y: Vec<f64> = x.iter().map(|v| v - mean).collect();
    let amp0 = y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    // Extremes between zero crossings, ignoring noise below 3% of the swing.
    let mut peaks: Vec<(usize, f64)> = Vec::new();
    let mut i = 0;
    while i < n {
        let s = y[i].signum();
        let mut best = (i, y[i]);
        let start = i;
        while i < n && y[i].signum() == s {
            if y[i].abs() > best.1.abs() {
                best = (i, y[i]);
            }
            i += 1;
        }
        if best.1.abs() > 0.03 * amp0 && i < n && i - start > 2 {
            peaks.push(best);
        }
    }
    if peaks.len() < 3 {
        return Osc { period: 0.0, zeta: 1.0 };
    }
    let k = peaks.len() - 1;
    let period = 2.0 * (peaks[k].0 - peaks[0].0) as f64 * dt / k as f64;
    // Half-cycle decrements, averaged.
    let mut d = 0.0;
    for w in peaks.windows(2) {
        d += (w[0].1.abs() / w[1].1.abs()).ln();
    }
    let delta = 2.0 * d / k as f64;
    Osc { period, zeta: delta / (4.0 * std::f64::consts::PI.powi(2) + delta * delta).sqrt() }
}

fn bank(p: &Plane) -> f64 {
    (-p.right().dot(p.pos.normalize())).asin()
}

fn level_wings(p: &Plane) -> f64 {
    (-(-p.right().dot(p.pos.normalize())) * 3.0 + p.omega.z * 0.5).clamp(-1.0, 1.0)
}

fn trimmed(spec: &'static Spec, alt: f64, speed: f64) -> (Plane, World) {
    let w = empty_world();
    let mut p = Plane::spawn(spec, 0.6, -2.1, alt, 0.0, speed);
    // Settle hands off (wings held level) so the run starts from steady flight.
    for _ in 0..(5.0 / DT) as usize {
        let c = Controls { roll: level_wings(&p), throttle_set: Some(p.throttle), ..Default::default() };
        assert!(p.step(DT, &c, &w).is_none());
    }
    (p, w)
}

/// Run `secs`, controls from `ctl(t, plane)`, recording `rec(plane)`.
fn record(p: &mut Plane, w: &World, secs: f64, mut ctl: impl FnMut(f64, &Plane) -> Controls, rec: impl Fn(&Plane) -> f64) -> Vec<f64> {
    let thr = p.throttle;
    let mut out = Vec::with_capacity((secs / DT) as usize);
    for i in 0..(secs / DT) as usize {
        let mut c = ctl(i as f64 * DT, p);
        c.throttle_set.get_or_insert(thr);
        assert!(p.step(DT, &c, w).is_none(), "crashed during the test");
        out.push(rec(p));
    }
    out
}

/// What the harness measured for one aircraft at one condition.
#[derive(Debug, Clone, Copy)]
pub struct Modes {
    pub short: Osc,
    pub phugoid: Osc,
    pub dutch: Osc,
    /// steady roll rate at full aileron, °/s, and seconds to 45° of bank
    pub roll_rate: f64,
    pub to_45: f64,
    /// bank after 20 s hands off from 20°, degrees (more than 20: spiral divergent)
    pub spiral: f64,
}

pub fn measure(spec: &'static Spec, alt: f64, speed: f64) -> Modes {
    // Short period: a half-second pull, then hands off; the angle of attack rings.
    let (mut p, w) = trimmed(spec, alt, speed);
    let a = record(&mut p, &w, 8.0, |t, p| Controls { pitch: if t < 0.5 { 0.4 } else { 0.0 }, roll: level_wings(p), ..Default::default() }, |p| p.alpha);
    let short = fit_osc(&a[(0.5 / DT) as usize..], DT);

    // Phugoid: two seconds' push, hands off; the airspeed swings slowly.
    let (mut p, w) = trimmed(spec, alt, speed);
    let v = record(&mut p, &w, 300.0, |t, p| Controls { pitch: if t < 2.0 { -0.25 } else { 0.0 }, roll: level_wings(p), ..Default::default() }, |p| p.airspeed().length());
    let phugoid = fit_osc(&v[(2.0 / DT) as usize..], DT);

    // Dutch roll: a one-second rudder pulse, hands and feet off.
    let (mut p, w) = trimmed(spec, alt, speed);
    let r = record(&mut p, &w, 30.0, |t, _| Controls { yaw: if t < 1.0 { 0.6 } else { 0.0 }, ..Default::default() }, |p| p.omega.y);
    let dutch = fit_osc(&r[(1.0 / DT) as usize..], DT);

    // Roll: full aileron from wings level.
    let (mut p, w) = trimmed(spec, alt, speed);
    let mut to_45 = f64::NAN;
    let mut peak: f64 = 0.0;
    let b = record(&mut p, &w, 4.0, |_, _| Controls { roll: 1.0, ..Default::default() }, bank);
    for (i, &x) in b.iter().enumerate() {
        if to_45.is_nan() && x > 45f64.to_radians() {
            to_45 = i as f64 * DT;
        }
        if i > 0 && x < 60f64.to_radians() {
            peak = peak.max((x - b[i - 1]) / DT);
        }
    }
    let roll_rate = peak.to_degrees();

    // Spiral: roll to 20°, let go, see where the bank goes in 20 s.
    let (mut p, w) = trimmed(spec, alt, speed);
    let target = 20f64.to_radians();
    record(&mut p, &w, 8.0, |_, p| Controls { roll: ((target - bank(p)) * 3.0 + p.omega.z * 0.5).clamp(-1.0, 1.0), ..Default::default() }, bank);
    let b = record(&mut p, &w, 20.0, |_, _| Controls::default(), bank);
    let spiral = b.last().unwrap().to_degrees();

    Modes { short, phugoid, dutch, roll_rate, to_45, spiral }
}

fn osc_str(o: &Osc, digits: usize) -> String {
    if o.period == 0.0 { "no overshoot".into() } else { format!("{:.*} s ζ {:.3}", digits, o.period, o.zeta) }
}

fn report(name: &str, m: &Modes, refs: &str) {
    println!(
        "{name}: short period {} | phugoid {} | Dutch roll {} | roll {:.0}°/s, 45° in {:.1} s | spiral 20° → {:.0}° in 20 s",
        osc_str(&m.short, 1), osc_str(&m.phugoid, 0), osc_str(&m.dutch, 1), m.roll_rate, m.to_45, m.spiral
    );
    println!("    reference: {refs}");
}

#[test]
fn the_fit_reads_a_damped_oscillation() {
    let (wn, zeta) = (2.0f64, 0.15f64);
    let wd = wn * (1.0 - zeta * zeta).sqrt();
    let x: Vec<f64> = (0..20_000).map(|i| i as f64 * 0.001).map(|t| 3.0 + (-zeta * wn * t).exp() * (wd * t).cos()).collect();
    let o = fit_osc(&x, 0.001);
    assert!((o.period / (std::f64::consts::TAU / wd) - 1.0).abs() < 0.01, "{o:?}");
    assert!((o.zeta - zeta).abs() < 0.01, "{o:?}");
    let flat: Vec<f64> = (0..1000).map(|i| (-(i as f64) * 0.01).exp()).collect();
    assert_eq!(fit_osc(&flat, 0.01).period, 0.0);
}

#[test]
fn handling_of_a_cessna_172() {
    let m = measure(&c172::SPEC, 1500.0, 55.0);
    report("C172 at 107 kt, 1500 m", &m, "short period ~1.3 s ζ ~0.8 | phugoid ~30 s ζ ~0.1 | Dutch roll ~2.5 s ζ ~0.2 | roll ~45-70°/s | spiral near neutral");
    assert!(m.short.zeta > 0.3, "short period too lightly damped: {:?}", m.short);
    assert!(m.dutch.zeta > 0.03 || m.dutch.period == 0.0, "Dutch roll unstable: {:?}", m.dutch);
    assert!(m.phugoid.zeta > -0.05, "phugoid divergent: {:?}", m.phugoid);
    assert!(m.roll_rate > 20.0 && m.roll_rate < 150.0, "{}", m.roll_rate);
    assert!(m.spiral.abs() < 60.0, "{}", m.spiral);
}

#[test]
fn handling_of_a_robin_dr400() {
    let m = measure(&crate::aircraft::dr400::SPEC, 1500.0, 60.0);
    report("DR400 at 117 kt, 1500 m", &m, "light-aircraft values as the C172's; crisper in roll");
    assert!(m.short.zeta > 0.3, "{:?}", m.short);
    assert!(m.dutch.zeta > 0.03 || m.dutch.period == 0.0, "{:?}", m.dutch);
    assert!(m.phugoid.zeta > -0.05, "{:?}", m.phugoid);
    assert!(m.roll_rate > 20.0 && m.roll_rate < 180.0, "{}", m.roll_rate);
}

#[test]
fn handling_of_a_747() {
    let h = 10_668.0;
    let m = measure(&b747::SPEC, h, 0.84 * sound_speed(h));
    report("747 at M0.84, FL350", &m, "short period ~7 s ζ ~0.35 | phugoid ~90-100 s ζ ~0.05 | Dutch roll ~5-6 s ζ ~0.05-0.1 (no yaw damper) | roll ~10-20°/s");
    assert!(m.short.zeta > 0.2, "{:?}", m.short);
    assert!(m.phugoid.zeta > -0.05, "{:?}", m.phugoid);
    assert!(m.roll_rate > 3.0 && m.roll_rate < 60.0, "{}", m.roll_rate);
}

/// Slow down at idle a knot a second, wings level, until the wing lets go;
/// the lowest indicated airspeed reached (kt), and whether the nose dropped.
pub fn stall_speed(spec: &'static Spec, notch: u8) -> (f64, bool) {
    let (mut p, w) = trimmed(spec, 1500.0, spec.approach.0 * 1.3);
    p.flap_set = notch;
    let mut target = p.ias();
    let (mut min_ias, mut dropped) = (f64::MAX, false);
    for i in 0..(90.0 / DT) as usize {
        target -= 0.514 * DT;
        let err = p.ias() - target;
        let c = Controls { pitch: (err * 0.08 + p.omega.x * 0.0).clamp(-1.0, 1.0), roll: level_wings(&p), throttle_set: Some(0.0), ..Default::default() };
        assert!(p.step(DT, &c, &w).is_none());
        if i as f64 * DT > 10.0 {
            min_ias = min_ias.min(p.ias());
            if p.stalled && p.omega.x < -0.05 {
                dropped = true;
                break;
            }
        }
    }
    (min_ias * 1.943_84, dropped)
}

#[test]
fn a_cessna_172_stalls_where_the_book_says() {
    // Cessna 172RG POH, at 2650 lb, power off, most forward CG: 50 KIAS
    // clean, 42 KIAS flaps 30° (ours is 2425 lb: a few knots less).
    let (clean, d0) = stall_speed(&c172::SPEC, 0);
    let (full, d3) = stall_speed(&c172::SPEC, 3);
    println!("C172 stall: {clean:.0} kt clean (nose drop {d0}), {full:.0} kt flaps 30° (nose drop {d3}); POH 50 / 42 KIAS at 2650 lb");
    assert!((44.0..54.0).contains(&clean), "{clean}");
    assert!((36.0..47.0).contains(&full), "{full}");
    assert!(full < clean - 3.0);
}

#[test]
fn every_aircraft_can_be_rigged_from_its_spec() {
    // The generic rig: built from what each `Spec` already says, every
    // aircraft's strips carry its weight at its cruise speed and fly a
    // minute hands off without coming apart. (Tuning each one to its real
    // handling is its own `Rig`'s job; this is the starting point.)
    let (mut flying, mut total) = (0, 0);
    for spec in crate::aircraft::list() {
        if spec.rocket().is_some() {
            continue;
        }
        let rig: &'static aero::Rig = Box::leak(Box::new(spec.rig.copied().unwrap_or_else(|| aero::Rig::derive(spec))));
        let s: &'static Spec = Box::leak(Box::new(Spec { rig: Some(rig), ..spec.clone() }));
        let w = empty_world();
        let h = if spec.is_jet() { 6_000.0 } else { 1_500.0 };
        let mut p = Plane::spawn(s, 0.6, -2.1, h, 0.0, spec.cruise);
        let h0 = geo::pos_to_lla(p.pos).2;
        for _ in 0..(60.0 / DT) as usize {
            let c = Controls { roll: level_wings(&p), throttle_set: Some(p.throttle), ..Default::default() };
            assert!(p.step(DT, &c, &w).is_none(), "{} crashed", spec.name);
        }
        let dh = geo::pos_to_lla(p.pos).2 - h0;
        let v = p.airspeed().length();
        println!("{}: trim {:+.2}, alpha {:.1}°, after 60 s {:+.0} m, {:.0} → {:.0} m/s", spec.name, p.pitch_trim, p.alpha.to_degrees(), dh, spec.cruise, v);
        assert!(p.omega.is_finite() && v.is_finite());
        // A delta's elevons and the derived Phantom's tail do not trim yet
        // without a rig of their own: reported, not required.
        let settled = dh.abs() < 0.25 * h && (v - spec.cruise).abs() < 0.5 * spec.cruise;
        if !settled {
            println!("    {} needs its own rig to trim", spec.name);
        }
        flying += settled as usize;
        total += 1;
    }
    println!("{flying} of {total} fly trimmed on their derived rig");
    assert!(flying * 3 >= total * 2, "the derived rig should fly most aircraft");
}
