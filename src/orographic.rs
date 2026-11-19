//! What the ground's shape does to the wind: ridge lift where the wind
//! blows up a slope and sink where it pours down the far side, lee waves
//! standing downwind of mountains in stable air, and the rotor turbulence
//! under them.
//!
//! The terrain is read along the wind's line through the aircraft (from
//! `UPWIND` upwind to `DOWNWIND` past it), every second or so. Then:
//!
//! - **Slope flow**: the wind follows the ground, w = U ∂h/∂s at the surface,
//!   fading with height above it over a scale set by the size of the slope.
//! - **Lee waves** (linear theory, trapped waves): every bit of upwind slope
//!   forces the air, and in stable air the response downwind is a train of
//!   waves of length λ = 2π U / N (U the wind at about the ridge tops, N the
//!   Brunt-Väisälä frequency, from the stability), decaying slowly
//!   downstream: w(0) = U Σ ∂h/∂s(s) G(-s) Δs with G(ξ) = C k sin(kξ) e^(-ξ/5λ).
//!   A ridge about a quarter wavelength wide excites them best; a broad
//!   plateau or a single crag hardly at all, as in the real atmosphere. They
//!   need wind across the ridge (from about 7 m/s), stable air, and fade out
//!   above the tropopause.
//! - **Rotor**: under strong waves, below the ridge tops downwind of them,
//!   violent turbulence.
//!
//! Nothing here knows about any particular place or aircraft: it is the
//! terrain the world has and the wind the weather has.
//!
//! References: Durran, "Mountain waves and downslope winds", in Atmospheric
//! Processes over Complex Terrain (AMS Meteor. Monogr. 23, 1990); Scorer,
//! "Theory of waves in the lee of mountains", QJRMS 75 (1949); FAA AC 00-6B
//! Aviation Weather ch. 16 (mountain waves and rotors).

use crate::geo;
use crate::weather::Weather;
use glam::DVec3;

/// How far upwind and downwind of the aircraft the ground is read (m), and how finely.
const UPWIND: f64 = 50_000.0;
const DOWNWIND: f64 = 1_000.0;
const STEP: f64 = 200.0;
/// Re-read the ground after this long (s) or this far (m).
const EVERY: f64 = 1.0;
const MOVED: f64 = 250.0;

/// What the ground does to the air at a point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Oro {
    /// vertical air motion (m/s, up positive): slope flow and lee waves
    pub w: f64,
    /// the slope flow and the lee waves separately (for the display and tests)
    pub slope: f64,
    pub wave: f64,
    /// extra RMS turbulence (m/s) in the rotor
    pub rotor: f64,
}

/// The ground along the wind line, and when and where it was read.
#[derive(Clone, Debug, Default)]
pub struct Orography {
    /// heights (m) at s = -UPWIND .. DOWNWIND (s along the wind, positive
    /// downwind, 0 at the aircraft); None where the world has no ground yet
    heights: Vec<Option<f64>>,
    at: DVec3,
    /// the wind's direction along the line (where it blows to, unit, planet frame)
    to: DVec3,
    read: Option<f64>,
}

impl Orography {
    /// Read the ground along the wind line through `pos` if it is time to.
    /// `ground(lat, lon)` (radians) gives the ground's height there, if known.
    pub fn update(&mut self, wx: &Weather, pos: DVec3, t: f64, ground: impl Fn(f64, f64) -> Option<f64>) {
        let fresh = self.read.is_some_and(|r| t - r < EVERY && t >= r) && self.at.distance(pos) < MOVED;
        if fresh {
            return;
        }
        self.read = Some(t);
        self.at = pos;
        // Along the wind a kilometre or so above the ground (what crosses the ridges).
        let w = wx.steady(pos, 1000.0);
        let up = pos.normalize();
        let w = w - up * w.dot(up);
        if w.length() < 0.5 {
            self.heights.clear();
            return;
        }
        self.to = w.normalize();
        let n = ((UPWIND + DOWNWIND) / STEP) as usize + 1;
        self.heights.clear();
        let r = pos.length();
        for i in 0..n {
            let s = -UPWIND + i as f64 * STEP;
            // Along the great circle through the aircraft in the wind's direction.
            let a = s / r;
            let p = up * a.cos() + self.to * a.sin();
            let (lat, lon, _) = geo::pos_to_lla(p * r);
            self.heights.push(ground(lat, lon));
        }
    }

    /// The ground's effect at `pos` (the point it was read through, or near
    /// it), `agl` metres above the ground, in the weather `wx`.
    pub fn at(&self, wx: &Weather, pos: DVec3, agl: f64) -> Oro {
        let n = self.heights.len();
        if n < 8 {
            return Oro::default();
        }
        let (lat, _, alt) = geo::pos_to_lla(pos);
        // Where the aircraft is along the line now (it moves between reads).
        let s0 = (pos - self.at).dot(self.to);
        let i0 = (UPWIND + s0) / STEP;
        let h = |i: isize| -> Option<f64> { if i < 0 || i as usize >= n { None } else { self.heights[i as usize] } };
        let here = (i0.round() as isize).clamp(0, n as isize - 1);
        // The slope under the aircraft, over ±2 steps, and the size of the
        // hill it belongs to (the relief within ±1.5 km).
        let slope = match (h(here - 2), h(here + 2)) {
            (Some(a), Some(b)) => (b - a) / (4.0 * STEP),
            _ => 0.0,
        };
        let span = (1500.0 / STEP) as isize;
        let near: Vec<f64> = (here - span..=here + span).filter_map(h).collect();
        let relief = if near.is_empty() { 0.0 } else { near.iter().cloned().fold(f64::MIN, f64::max) - near.iter().cloned().fold(f64::MAX, f64::min) };
        let ground_here = h(here).unwrap_or(alt - agl);
        let (u_low, _) = wx.profile(lat, ground_here + 150.0, 150.0);
        let depth = (0.6 * relief).max(80.0);
        let w_slope = u_low * slope.clamp(-0.6, 0.6) * (-agl.max(0.0) / depth).exp();

        // Lee waves: the ridge tops upwind, the wind across them and the air's stability.
        let upwind: Vec<f64> = (0..=here).filter_map(h).collect();
        if upwind.len() < 4 {
            return Oro { w: w_slope, slope: w_slope, ..Default::default() };
        }
        let top = upwind.iter().cloned().fold(f64::MIN, f64::max);
        let (u_c, _) = wx.profile(lat, top + 1000.0, (top + 1000.0 - ground_here).max(10.0));
        let stab = wx.stability();
        // Brunt-Väisälä frequency: ~0.01/s in a standard atmosphere, more in
        // a stable one, less where convection mixes it.
        let nbv = 0.007 + 0.009 * stab;
        let lambda = (std::f64::consts::TAU * u_c / nbv).clamp(3000.0, 25000.0);
        let k = std::f64::consts::TAU / lambda;
        // Waves need the wind across the ridge, and are best in stable air.
        let drive = smooth(6.0, 12.0, u_c) * (0.35 + 0.65 * stab);
        let (mut ws, mut wc) = (0.0, 0.0);
        if drive > 0.0 {
            for i in 1..=here {
                let (Some(a), Some(b)) = (h(i - 1), h(i)) else { continue };
                let xi = (i0 - (i as f64 - 0.5)) * STEP;
                if xi <= 0.0 {
                    continue;
                }
                let decay = (-xi / (5.0 * lambda)).exp() * (1.0 - smooth(0.7 * UPWIND, UPWIND, xi));
                let f = (b - a) * decay;
                ws += f * (k * xi).sin();
                wc += f * (k * xi).cos();
            }
        }
        // C: an 800 m ridge about half a wavelength wide in a 10 m/s surface wind (some 18 m/s at the ridge tops) gives about 4-5 m/s.
        let c = 0.4;
        let amp_scale = u_c * c * k * drive;
        let (mut w_wave, mut amp) = (ws * amp_scale, (ws * ws + wc * wc).sqrt() * amp_scale);
        // (The linear response saturates: waves stronger than this break.)
        let cap = (0.6 * u_c).min(12.0);
        if amp > cap {
            w_wave *= cap / amp;
            amp = cap;
        }
        // Strongest from the ridge tops up through the middle troposphere,
        // weak in the valleys under them, gone above the tropopause.
        let above = alt - top;
        let env = smooth(-0.6 * relief.max(300.0), 600.0, above) * (1.0 - smooth(10000.0, 15000.0, alt));
        w_wave *= 0.3 + 0.7 * env;
        // Rotor: under strong waves, below about the ridge tops.
        let low = 1.0 - smooth(-200.0, 400.0, above);
        let rotor = 0.35 * smooth(1.5, 5.0, amp) * amp * low;
        Oro { w: w_slope + w_wave, slope: w_slope, wave: w_wave, rotor }
    }
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ridge across a westerly wind: a cosine bump `height` m high and
    /// `width` m from foot to foot, its crest `east` m east of (0, 45°N).
    fn ridge(height: f64, width: f64, east: f64) -> impl Fn(f64, f64) -> Option<f64> {
        move |lat: f64, lon: f64| {
            let x = (lon * lat.cos()) * 6.371e6 - east;
            let _ = lat;
            Some(if x.abs() < 0.5 * width { 0.5 * height * (1.0 + (std::f64::consts::TAU * x / width).cos()) } else { 0.0 })
        }
    }

    fn west_wind(speed: f64, stable: bool) -> Weather {
        let mut wx = Weather::new(0.1);
        (wx.wind, wx.wind_from, wx.gusts) = (speed, 270.0, 0.0);
        wx.sun = if stable { -0.3 } else { 0.9 };
        wx
    }

    /// The vertical air motion along a line west to east at `alt` m.
    fn scan(wx: &Weather, g: &impl Fn(f64, f64) -> Option<f64>, alt: f64, from: f64, to: f64) -> Vec<(f64, Oro)> {
        let lat = 45f64.to_radians();
        let mut o = Orography::default();
        let mut out = Vec::new();
        let mut x = from;
        while x <= to {
            let lon = x / (6.371e6 * lat.cos());
            let gh = g(lat, lon).unwrap();
            let pos = geo::lla_to_pos(lat, lon, alt.max(gh + 5.0));
            o.update(wx, pos, x, g);
            out.push((x, o.at(wx, pos, alt.max(gh + 5.0) - gh)));
            x += 250.0;
        }
        out
    }

    #[test]
    fn the_wind_lifts_up_a_slope_and_sinks_down_the_far_side() {
        let g = ridge(400.0, 4000.0, 0.0);
        let wx = west_wind(10.0, false);
        let s = scan(&wx, &g, 0.0, -3000.0, 3000.0);
        let up = s.iter().filter(|(x, _)| (-1500.0..-500.0).contains(x)).map(|(_, o)| o.slope).fold(f64::MIN, f64::max);
        let down = s.iter().filter(|(x, _)| (500.0..1500.0).contains(x)).map(|(_, o)| o.slope).fold(f64::MAX, f64::min);
        println!("ridge lift {up:.2} m/s, lee sink {down:.2} m/s");
        // U ∂h/∂s: up to 10 · π · 400 / 4000 ≈ 3 m/s at the steepest, near the ground.
        assert!(up > 1.5 && up < 4.0 && down < -1.5 && down > -4.0, "{up} {down}");
    }

    #[test]
    fn stable_air_stands_lee_waves_downwind_of_a_ridge() {
        let g = ridge(800.0, 8000.0, 0.0);
        let stable = west_wind(10.0, true);
        let s = scan(&stable, &g, 4000.0, 2000.0, 45000.0);
        let lift = s.iter().map(|(_, o)| o.wave).fold(f64::MIN, f64::max);
        let sink = s.iter().map(|(_, o)| o.wave).fold(f64::MAX, f64::min);
        // Successive crests: the wavelength.
        let ups: Vec<f64> = s.windows(3).filter(|w| w[1].1.wave > w[0].1.wave && w[1].1.wave >= w[2].1.wave && w[1].1.wave > 0.5).map(|w| w[1].0).collect();
        println!("lee waves: lift {lift:.1}, sink {sink:.1} m/s, crests at {ups:?}");
        assert!(lift > 2.0 && lift < 12.0 && sink < -2.0, "{lift} {sink}");
        assert!(ups.len() >= 2, "{ups:?}");
        let lambda = ups[1] - ups[0];
        assert!((4000.0..20000.0).contains(&lambda), "{lambda}");
        // Unstable, light wind: next to nothing.
        let calm = west_wind(4.0, false);
        let c = scan(&calm, &g, 4000.0, 2000.0, 45000.0);
        let most = c.iter().map(|(_, o)| o.wave.abs()).fold(0.0, f64::max);
        assert!(most < 0.3, "{most}");
        // Upwind of the ridge: none.
        let before = scan(&stable, &g, 4000.0, -20000.0, -6000.0);
        assert!(before.iter().all(|(_, o)| o.wave.abs() < 0.05));
        // Rotor down low under strong waves, not high above them.
        let low = scan(&stable, &g, 300.0, 3000.0, 15000.0).iter().map(|(_, o)| o.rotor).fold(0.0, f64::max);
        let high = s.iter().map(|(_, o)| o.rotor).fold(0.0, f64::max);
        println!("rotor sigma: low {low:.1}, at 4 km {high:.1}");
        assert!(low > 1.0 && high < 0.2, "{low} {high}");
    }

    #[test]
    fn flat_ground_does_nothing() {
        let wx = west_wind(15.0, true);
        let flat = |_: f64, _: f64| Some(120.0);
        assert!(scan(&wx, &flat, 2000.0, 0.0, 10000.0).iter().all(|(_, o)| *o == Oro::default() || (o.w.abs() < 1e-9 && o.rotor < 1e-9)));
        // And no ground known yet: nothing either.
        let none = |_: f64, _: f64| None;
        let lat = 45f64.to_radians();
        let mut o = Orography::default();
        let p = geo::lla_to_pos(lat, 0.0, 2000.0);
        o.update(&wx, p, 0.0, none);
        assert_eq!(o.at(&wx, p, 2000.0), Oro::default());
    }
}
