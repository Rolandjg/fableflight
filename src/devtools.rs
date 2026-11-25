//! Developer commands run from the command line instead of the game: JSBSim comparisons and flight checks.

use crate::*;

/// `--jsbsim-compare FILE [--aircraft NAME]`: prints how a JSBSim aircraft's
/// aerodynamics compare with ours (the Cessna unless NAME picks another).
pub(crate) fn jsbsim_compare(file: Option<&String>, which: Option<&String>) -> i32 {
    let Some(file) = file else {
        eprintln!("--jsbsim-compare takes a JSBSim aircraft file (…/c172p/c172p.xml)");
        return 2;
    };
    let j = match jsbsim::Aircraft::load(std::path::Path::new(file)) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("{file}: {e}");
            return 1;
        }
    };
    let want = which.map(|w| w.to_lowercase()).unwrap_or_else(|| "172".into());
    let Some(spec) = aircraft::BUILTIN.iter().copied().find(|s| s.name.to_lowercase().contains(&want)) else {
        eprintln!("no aircraft of ours matches {want:?}: {}", aircraft::BUILTIN.iter().map(|s| s.name).collect::<Vec<_>>().join(", "));
        return 2;
    };
    println!("{}", jsbsim::compare(&j, spec, our_roll_rate(spec)));
    0
}

/// Ours flown: the highest roll rate full aileron reaches from 90 kt, level,
/// in the first four seconds (rad/s). No world is loaded (offline, empty).
pub(crate) fn our_roll_rate(spec: &'static aircraft::Spec) -> Option<f64> {
    let dir = std::env::temp_dir().join("fablegame-jsbsim-compare");
    let src = Arc::new(sources::Sources::new(fetch::Fetcher::new(dir, true), String::new()));
    let w = world::World::new(sources::Loader::new(src, 0), 14);
    let mut p = Plane::spawn(spec, 45f64.to_radians(), 5f64.to_radians(), 1500.0, 0.0, 90.0 / 1.943_84);
    p.throttle = 0.7;
    let c = Controls { roll: 1.0, ..Default::default() };
    let dt = 1.0 / 180.0;
    let mut best: f64 = 0.0;
    for _ in 0..(4.0 / dt) as usize {
        if p.step(dt, &c, &w).is_some() {
            break;
        }
        best = best.max(p.omega.z.abs());
    }
    Some(best)
}

/// `--fly-check KEY`: flies an aircraft in an empty world and prints how it
/// does: a take-off run at full power, level flight at full power (its top
/// speed) and a power-off slow-down to the stall. For checking a fit (hangar.rs).
pub(crate) fn fly_check(key: Option<&String>) -> i32 {
    hangar::load_installed();
    let Some(spec) = key.and_then(|k| aircraft::by_key(k)) else {
        eprintln!("--fly-check KEY: one of {}", aircraft::list().iter().map(|s| s.keys[0]).collect::<Vec<_>>().join(", "));
        return 1;
    };
    let dir = std::env::temp_dir().join("fablegame-jsbsim-compare");
    let src = Arc::new(sources::Sources::new(fetch::Fetcher::new(dir, true), String::new()));
    let mut w = world::World::new(sources::Loader::new(src, 0), 14);
    w.flat = Some(0.0);
    let dt = 1.0 / 120.0;
    let kt = |v: f64| v * 1.943_84;
    println!("{}: mass {:.0} kg, cruise {:.0} kt, approach {:.0} kt, vne {:.0} kt", spec.name, spec.mass, kt(spec.cruise), kt(spec.approach.0), kt(spec.vne));
    let (lat, lon) = (37.6f64, -122.37f64);
    // wings held level; pitch to an attitude (degrees), gently
    let level = |p: &Plane| (p.right().dot(p.pos.normalize()) * 3.0 + p.omega.z * 0.5).clamp(-1.0, 1.0);
    let attitude = |p: &Plane, want: f64| ((want - p.forward().dot(p.pos.normalize()).asin().to_degrees()) * 0.1 - p.omega.x * 0.3).clamp(-1.0, 1.0);
    // Take-off: first flap notch, full power, rotate to 10° at 1.1 times the clean stall.
    let mut p = Plane::spawn_ground(spec, lat.to_radians(), lon.to_radians(), 0.0, 0.5, 0.0);
    p.hold = false;
    p.t = 2.0;
    if spec.max_notch() > 0 {
        p.flap_set = 1;
        p.flaps = spec.flap_frac(1);
    }
    let vr = spec.approach.0 / 1.3 * 1.15;
    let start = p.pos;
    let (mut off, mut t, mut crashed) = (None, 0.0, false);
    while t < 120.0 {
        let c = Controls { throttle: 1.0, pitch: if p.vel.length() > vr { attitude(&p, 10.0) } else { 0.0 }, roll: level(&p), ..Default::default() };
        if p.step(dt, &c, &w).is_some() {
            crashed = true;
            break;
        }
        if off.is_none() && !p.on_ground && t > 5.0 {
            off = Some((t, (p.pos - start).length(), kt(p.vel.length())));
        }
        if off.is_some_and(|o| t > o.0 + 20.0) {
            break;
        }
        t += dt;
    }
    let h = geo::pos_to_lla(p.pos).2;
    match off {
        Some((t, d, v)) => println!("  take-off: lift-off after {t:.0} s, {d:.0} m, {v:.0} kt; 20 s later {h:.0} m{}", if crashed { ", CRASHED" } else { "" }),
        None => println!("  take-off: never left the ground ({:.0} kt){}", kt(p.vel.length()), if crashed { ", CRASHED" } else { "" }),
    }
    println!("    notes {:?}, gear {:?}", p.notes, p.dmg.gear);
    // Level at full power: hold the height, see where the speed settles.
    let alt0 = 1500.0;
    let hold = |p: &Plane| {
        let h = geo::pos_to_lla(p.pos).2;
        ((alt0 - h) * 0.002 - p.vel.dot(p.pos.normalize()) * 0.02 - p.omega.x * 0.3).clamp(-1.0, 1.0)
    };
    let mut p = Plane::spawn(spec, lat.to_radians(), lon.to_radians(), alt0, 0.0, spec.cruise);
    p.throttle = 1.0;
    let mut line = String::new();
    for k in 0..(180.0 / dt) as usize {
        let c = Controls { throttle: 1.0, pitch: hold(&p), roll: level(&p), ..Default::default() };
        if p.step(dt, &c, &w).is_some() {
            line += " CRASHED";
            break;
        }
        if k % (30.0 / dt) as usize == 0 {
            line += &format!("  {:.0}kt/{:.0}m", kt(p.ias()), geo::pos_to_lla(p.pos).2);
        }
    }
    println!("  level, full power:{line}  -> {:.0} kt, alpha {:.1}°, wings {:?}, notes {:?}", kt(p.ias()), p.alpha.to_degrees(), p.dmg.wing, p.notes);
    // Power off, hold the height until it stalls.
    let mut p = Plane::spawn(spec, lat.to_radians(), lon.to_radians(), alt0, 0.0, spec.cruise * 0.7);
    p.throttle = 0.0;
    let mut stall = None;
    for k in 0..(240.0 / dt) as usize {
        let c = Controls { throttle: 0.0, pitch: hold(&p), roll: level(&p), ..Default::default() };
        if p.step(dt, &c, &w).is_some() {
            break;
        }
        if stall.is_none() && k as f64 * dt > 5.0 && (p.alpha > spec.stall_alpha * 0.98 || alt0 - geo::pos_to_lla(p.pos).2 > 60.0) {
            stall = Some((kt(p.ias()), p.alpha.to_degrees()));
        }
    }
    match stall {
        Some((v, a)) => println!("  slow-down, power off: stalls at {v:.0} kt, {a:.1}°"),
        None => println!("  slow-down: no stall in 240 s"),
    }
    0
}
