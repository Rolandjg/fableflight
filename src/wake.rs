//! Wake turbulence: the pair of counter-rotating vortices a lifting wing
//! leaves behind it, which sink, drift with the wind, spread apart near the
//! ground and decay. Flying into one rolls an aircraft hard (the vortex's
//! upwash under one wing, its downwash under the other), and that roll comes
//! out of the aircraft's own geometry: the vortices' velocity is added to the
//! air wherever the airframe samples it (see `Plane::turbulence`).
//!
//! Model, for an aircraft of mass m, speed V and span b in air of density ρ:
//! - spacing b₀ = π/4 b (elliptic loading), circulation Γ₀ = m g / (ρ V b₀);
//! - each vortex's swirl is the Burnham-Hallock profile v = Γ/2π · r/(r² + r_c²),
//!   core radius r_c about 5% of b₀, growing as it ages;
//! - the pair sinks at Γ/(2π b₀) until near the ground, where it stops and
//!   the two spread apart instead;
//! - Γ holds for a while, then decays on the time scale T₀ = 2π b₀² / Γ₀,
//!   sooner in turbulent air (the two-phase decay of Holzäpfel, J. Aircraft
//!   40(2), 2003, simplified).
//!
//! The sources are whatever other aircraft the game has (airport traffic,
//! live traffic), as positions, velocities and size classes; nothing here is
//! specific to one aircraft.

use glam::DVec3;
use std::collections::VecDeque;

/// A trail's nodes are laid this often (s).
const EVERY: f64 = 0.5;
/// Longest a wake lasts (s), and how far from the player it is kept (m).
const LIFE: f64 = 240.0;
const KEEP: f64 = 25_000.0;

/// An aircraft shedding a wake this instant.
#[derive(Clone, Copy, Debug)]
pub struct Source {
    /// stable while the aircraft lives
    pub id: u64,
    pub pos: DVec3,
    pub vel: DVec3,
    pub span: f64,
    pub mass: f64,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    /// the middle of the pair (planet frame), and the direction to the right vortex
    mid: DVec3,
    right: DVec3,
    /// the vortices' spacing now (m), at the start, their circulation at the start (m²/s)
    sep: f64,
    b0: f64,
    gamma0: f64,
    born: f64,
}

impl Node {
    fn t0(&self) -> f64 {
        std::f64::consts::TAU * self.b0 * self.b0 / self.gamma0.max(1e-3)
    }
    /// Circulation at `t`, with the air's turbulence `sigma` (m/s) hastening the decay.
    fn gamma(&self, t: f64, sigma: f64) -> f64 {
        let age = t - self.born;
        let t0 = self.t0();
        let hold = t0 * 4.0 / (1.0 + 1.5 * sigma);
        self.gamma0 * (-(age - hold).max(0.0) / t0).exp()
    }
}

struct Trail {
    id: u64,
    /// oldest first
    nodes: VecDeque<Node>,
    last: f64,
}

/// A stretch of a wake near the player this frame: two vortex lines.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    /// the older and the newer end of the pair's middle, and their right-hand directions
    a: DVec3,
    b: DVec3,
    ra: DVec3,
    rb: DVec3,
    sep_a: f64,
    sep_b: f64,
    gamma: f64,
    core: f64,
}

#[derive(Default)]
pub struct Wakes {
    trails: Vec<Trail>,
    pub sigma: f64,
}

impl Wakes {
    /// Lay, move and age the wakes. `wind(p)`: the steady wind at `p`;
    /// `ground`: the ground's sea-level height round the player (m);
    /// `player`: where the player is (wakes far from it are dropped).
    pub fn step(&mut self, dt: f64, t: f64, sources: &[Source], wind: impl Fn(DVec3) -> DVec3, ground: f64, player: DVec3) {
        for s in sources {
            let v = s.vel.length();
            if v < 20.0 || s.span <= 0.0 || s.mass <= 0.0 {
                continue;
            }
            let i = match self.trails.iter().position(|tr| tr.id == s.id) {
                Some(i) => i,
                None => {
                    self.trails.push(Trail { id: s.id, nodes: VecDeque::new(), last: f64::MIN });
                    self.trails.len() - 1
                }
            };
            let tr = &mut self.trails[i];
            if t - tr.last < EVERY && t >= tr.last {
                continue;
            }
            tr.last = t;
            let up = s.pos.normalize();
            let f = s.vel / v;
            let right = f.cross(up).normalize_or_zero();
            let alt = s.pos.length() - crate::geo::R;
            let rho = 1.225 * (-alt.max(0.0) / 8500.0).exp();
            let b0 = std::f64::consts::FRAC_PI_4 * s.span;
            let gamma0 = s.mass * 9.81 / (rho * v * b0);
            tr.nodes.push_back(Node { mid: s.pos, right, sep: b0, b0, gamma0, born: t });
        }
        let sigma = self.sigma;
        for tr in &mut self.trails {
            for n in &mut tr.nodes {
                let g = n.gamma(t, sigma);
                let up = n.mid.normalize();
                let h = n.mid.length() - crate::geo::R - ground;
                let w = g / (std::f64::consts::TAU * n.sep);
                // Near the ground the pair stops sinking and spreads apart.
                let gnd = 1.0 - smooth(0.5 * n.b0, 1.5 * n.b0, h);
                n.mid += (wind(n.mid) - up * (w * (1.0 - gnd))) * dt;
                n.sep += w * gnd * 2.0 * dt;
            }
            tr.nodes.retain(|n| t - n.born < LIFE && n.gamma(t, sigma) > 0.05 * n.gamma0 && n.mid.distance(player) < KEEP);
        }
        self.trails.retain(|tr| !tr.nodes.is_empty() || t - tr.last < 5.0);
    }

    /// The stretches of wake within `reach` metres of `pos`, at time `t`.
    pub fn near(&self, pos: DVec3, reach: f64, t: f64) -> Vec<Seg> {
        let mut out = Vec::new();
        for tr in &self.trails {
            for (a, b) in tr.nodes.iter().zip(tr.nodes.iter().skip(1)) {
                let span = a.mid.distance(b.mid);
                if span > 400.0 {
                    continue;
                }
                if a.mid.distance(pos).min(b.mid.distance(pos)) > reach + span {
                    continue;
                }
                let g = 0.5 * (a.gamma(t, self.sigma) + b.gamma(t, self.sigma));
                let age = t - 0.5 * (a.born + b.born);
                let core = (0.05 * a.b0).max(0.4) * (1.0 + 0.3 * age / a.t0());
                out.push(Seg { a: a.mid, b: b.mid, ra: a.right, rb: b.right, sep_a: a.sep, sep_b: b.sep, gamma: g, core });
            }
        }
        out
    }
}

impl Seg {
    /// Where the right-hand vortex is, at the older end (for tests).
    #[cfg(test)]
    pub fn right_vortex(&self) -> DVec3 {
        self.a + self.ra * (0.5 * self.sep_a)
    }
}

/// The air's velocity (planet frame, m/s) the wakes `segs` induce at `p`.
pub fn induced(segs: &[Seg], p: DVec3) -> DVec3 {
    let mut v = DVec3::ZERO;
    for s in segs {
        // The right vortex points aft (from the newer node to the older),
        // the left forward: downwash between them, upwash outside.
        for (side, sign) in [(1.0, 1.0), (-1.0, -1.0)] {
            let la = s.a + s.ra * (side * 0.5 * s.sep_a);
            let lb = s.b + s.rb * (side * 0.5 * s.sep_b);
            let ax = lb - la;
            let l2 = ax.length_squared();
            if l2 < 1e-6 {
                continue;
            }
            let u = (p - la).dot(ax) / l2;
            // Each point belongs to one stretch (the next takes over at its end).
            if !(0.0..1.0).contains(&u) {
                continue;
            }
            let d = -ax / l2.sqrt() * sign;
            let r = p - (la + ax * u);
            let r2 = r.length_squared();
            if r2 > 250_000.0 {
                continue;
            }
            v += d.cross(r) * (s.gamma / std::f64::consts::TAU / (r2 + s.core * s.core));
        }
    }
    v
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: f64 = crate::geo::R;

    /// A heavy jet flying north at `alt`, 75 m/s, for 30 s, no wind.
    fn heavy_wake(alt: f64, ground: f64) -> (Wakes, DVec3, f64) {
        let mut w = Wakes::default();
        let mut t = 0.0;
        let dt = 0.05;
        let up = DVec3::Z;
        let north = DVec3::Y;
        let start = up * (R + alt);
        let mut pos = start;
        while t < 30.0 {
            let src = Source { id: 1, pos, vel: north * 75.0, span: 64.4, mass: 250_000.0 };
            w.step(dt, t, &[src], |_| DVec3::ZERO, ground, start);
            pos = (pos + north * 75.0 * dt).normalize() * (R + alt);
            t += dt;
        }
        (w, start, t)
    }

    #[test]
    fn a_heavy_jets_wake_swirls_sinks_and_decays() {
        let (w, start, t) = heavy_wake(1000.0, 0.0);
        // 30 s on, a kilometre behind the jet's start point.
        let at = (start + DVec3::Y * 1000.0).normalize() * (R + 1000.0);
        let segs = w.near(at, 300.0, t);
        assert!(!segs.is_empty());
        let b0 = std::f64::consts::FRAC_PI_4 * 64.4;
        // Γ₀ = m g / (ρ V b₀): about 650 m²/s; the pair sinks about 2 m/s.
        let sink = segs[0].gamma / (std::f64::consts::TAU * b0);
        println!("gamma {:.0} m²/s, sinking {sink:.2} m/s", segs[0].gamma);
        assert!((500.0..800.0).contains(&segs[0].gamma) && (1.5..2.6).contains(&sink));
        // Find the pair's middle by its downwash, below where it was laid.
        let x = DVec3::X;
        let mut best = (0.0, 0.0);
        for dz in 0..120 {
            let p = at - DVec3::Z * dz as f64;
            let v = induced(&segs, p).z;
            if v < best.1 {
                best = (dz as f64, v);
            }
        }
        println!("strongest downwash {:.1} m/s, {:.0} m below", best.1, best.0);
        assert!(best.0 > 20.0 && best.0 < 80.0 && best.1 < -5.0, "{best:?}");
        let mid = at - DVec3::Z * best.0;
        // Upwash outside the vortices, downwash between them; right and left mirror.
        let out_r = induced(&segs, mid + x * (0.5 * b0 + 8.0)).z;
        let out_l = induced(&segs, mid - x * (0.5 * b0 + 8.0)).z;
        assert!(out_r > 3.0 && out_l > 3.0 && (out_r - out_l).abs() < 0.2 * out_r, "{out_r} {out_l}");
        // Far off: nothing.
        assert!(induced(&segs, mid + x * 600.0).length() < 1e-9);
        // Later it decays away and is dropped.
        let mut w2 = w;
        let mut t2 = t;
        while t2 < t + 250.0 {
            w2.step(0.5, t2, &[], |_| DVec3::ZERO, 0.0, start);
            t2 += 0.5;
        }
        assert!(w2.near(at, 5000.0, t2).is_empty());
    }

    #[test]
    fn near_the_ground_the_vortices_stop_sinking_and_spread() {
        // Laid 40 m up over ground at 0.
        let (w, start, t) = heavy_wake(40.0, 0.0);
        let at = (start + DVec3::Y * 500.0).normalize() * (R + 40.0);
        let segs = w.near(at, 300.0, t);
        let s = segs.iter().min_by(|a, b| a.a.distance(at).total_cmp(&b.a.distance(at))).unwrap();
        let h = s.a.length() - R;
        let b0 = std::f64::consts::FRAC_PI_4 * 64.4;
        println!("in ground effect: height {h:.1} m, spacing {:.0} m (b0 {b0:.0})", s.sep_a);
        assert!(h > 15.0 && s.sep_a > 1.3 * b0);
    }

    #[test]
    fn the_wake_drifts_with_the_wind() {
        let mut w = Wakes::default();
        let start = DVec3::Z * (R + 800.0);
        let src = Source { id: 7, pos: start, vel: DVec3::Y * 70.0, span: 35.0, mass: 60_000.0 };
        w.step(0.05, 0.0, &[src], |_| DVec3::X * 5.0, 0.0, start);
        for i in 1..200 {
            w.step(0.05, i as f64 * 0.05, &[], |_| DVec3::X * 5.0, 0.0, start);
        }
        let n = &w.trails[0].nodes[0];
        assert!(((n.mid - start).x - 50.0).abs() < 1.0, "{}", (n.mid - start).x);
    }
}
