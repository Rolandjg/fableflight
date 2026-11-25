//! What the renderer is handed about the aircraft and the sky: instruments, its own lights, the sun and moon, the cockpit's and the NAV receivers' uniforms.

use crate::*;

impl Game {
    /// Readings for the cockpit instruments and the state of the lights (see plane.wgsl).
    pub(crate) fn instruments(&self, agl: f64) -> [[f32; 4]; 6] {
        let p = &self.plane;
        let (_, lon, _) = geo::pos_to_lla(p.pos);
        let local = (self.utc + lon.to_degrees() / 15.0).rem_euclid(24.0) * 3600.0;
        let t = self.time;
        let strobe = { let s = t.rem_euclid(1.3); if s < 0.045 || (s > 0.16 && s < 0.2) { 1.0 } else { 0.0 } };
        let beacon = (1.0 - ((t * 1.1).rem_euclid(1.0) - 0.15).abs() * 7.0).max(0.0);
        let (mut g, card) = self.gauges.readings(p);
        // (The navigation lights, beacon and strobes run off the bus.)
        let (lit, pw) = if p.sys.powered() { (1000.0, 1.0) } else { (0.0, 0.0) };
        g[0][1] += self.radios.alt_error_ft(self.wx.qnh) as f32;
        if p.spec.id == 9 && p.sys.powered() {
            // The G650's attitude and heading come from its inertial reference
            // units, not the vacuum gyro and DG the gauges model: the aircraft's
            // own, as soon as the bus is powered (the IRS alignment is not simulated).
            let (e, n, u) = geo::enu(p.pos);
            let (f, r, up) = (p.forward(), p.right(), p.up());
            g[1][0] = f.dot(u).clamp(-1.0, 1.0).asin().to_degrees() as f32;
            g[1][1] = (-r.dot(u)).atan2(up.dot(u)).to_degrees() as f32;
            g[0][3] = f.dot(e).atan2(f.dot(n)).to_degrees().rem_euclid(360.0) as f32;
        }
        if self.spec.is_jet() {
            // The glass cockpit's engine and air data (b747/deck747.wgsl).
            let (_, _, h) = geo::pos_to_lla(p.pos);
            let mach = p.mach();
            let oat = 15.0 - 6.5 * h.clamp(0.0, 11000.0) / 1000.0;
            let tat = oat + (oat + 273.15) * 0.2 * mach * mach;
            let x = ((p.n1 - 0.2) / 0.82).clamp(0.0, 1.2);
            let running = if p.running { (p.n1 / 0.15).clamp(0.0, 1.0) } else { 0.0 };
            // (The Hornet's two F404s: hotter and thirstier for their size, 4.9 t of fuel inside.
            // The Phantom's J79 turbojets: about 250 C idling, 650 C at military power.
            // The Raptor's F119s are taken to run like the Hornet's.
            // Concorde's Olympus 593 turbojets: about 350 C idling, 750 C flat out.
            // The Blackbird's J58s: about 400 C idling, 795 C at military power and in afterburner.
            // The G650's BR725s show ITT: taken as about 440 C idling and 870 C at take-off
            // power, an estimate (no published figures found).)
            let (idle_t, rise) = match self.spec.id { 2 | 4 => (400.0, 380.0), 3 => (250.0, 330.0), 6 => (350.0, 400.0), 7 => (80.0, 740.0), 9 => (440.0, 430.0), _ => (440.0, 200.0) };
            let egt = oat + (idle_t - oat + rise * x.powf(1.4)) * running;
            // Fuel flow per engine (kg/h) and the fuel on board (kg), from the engines' burn.
            let n_eng = match &self.spec.engine { aircraft::Engine::Fan(f) => f.n as f64, _ => 1.0 };
            let ff = p.fuel_flow * 3600.0 / n_eng;
            let fuel = p.fuel;
            let _ = x;
            let u = p.pos.normalize();
            let gs = (p.vel - u * p.vel.dot(u)).length() * 1.94384;
            return [
                g[0],
                g[1],
                [p.n1 as f32, egt as f32, ff as f32, fuel as f32],
                [mach as f32, gs as f32, (agl * 3.28084) as f32, tat as f32],
                [p.throttle as f32, local as f32, p.gear as f32, p.flap_deg() as f32],
                [lit + card.rem_euclid(360.0), strobe * pw, beacon as f32 * pw, if (if matches!(self.spec.id, 0 | 9) { p.landing_lights } else { p.gear_down }) && p.sys.powered() { 1.0 } else { 0.0 }],
            ];
        }
        // Row 5's first slot carries the nav lights (on above 1000) and the compass card.
        [
            g[0],
            g[1],
            g[2],
            g[3],
            [g[4][0], local as f32, p.gear as f32, p.flap_deg() as f32],
            [lit + card.rem_euclid(360.0), strobe * pw, beacon as f32 * pw, if (if matches!(self.spec.id, 0 | 10) { p.landing_lights } else { p.gear_down }) && p.sys.powered() { 1.0 } else { 0.0 }],
        ]
    }

    /// The flown aircraft's outside lights: a glow sprite on each lens
    /// (lights.wgsl, as the traffic has), and the landing lights' beams
    /// (Globals `spot`, common.wgsl `spot_light`). `p5` is the lights' state
    /// (instruments row 5: nav on, strobe, beacon, landing).
    pub(crate) fn own_lights(&self, base: &Mat4, pose: &[Mat4], p5: [f32; 4], g: &mut render::Globals, out: &mut Vec<airfield::Light>) {
        use airfield::{L_GREEN, L_RED, L_STROBE, L_WHITE};
        use airframe::lamp;
        let dm = &self.plane.dmg;
        let big = self.spec.span > 30.0;
        let nav = if p5[0] >= 999.0 { 1.0 } else { 0.0 };
        let mut beam: [(Vec3, f32); 2] = [(Vec3::ZERO, 0.0); 2];
        let mut beam_dir = Vec3::ZERO;
        let bend = self.plane.wing_bend();
        let mid = {
            let xs: Vec<f32> = self.lamps.iter().filter(|l| l.id == lamp::LANDING as u32 && l.at.x.abs() > 0.5).map(|l| l.at.x).collect();
            xs.iter().sum::<f32>() / xs.len().max(1) as f32
        };
        for l in &self.lamps {
            let p = &self.model.pieces[l.piece];
            if dm.hidden(p) || !dm.tears_for(p).iter().all(|t| t.keeps(l.at)) {
                continue;
            }
            let m = *base * pose[l.piece];
            // (a wingtip light rides on the bent wing, as plane.wgsl bends it)
            let mut b = pose[l.piece].transform_point3(l.at);
            b.y += b.x * b.x * match p.group {
                airframe::G_LWING => bend[0],
                airframe::G_RWING => bend[1],
                _ => 0.0,
            };
            let at = base.transform_point3(b);
            let fwd = m.transform_vector3(Vec3::NEG_Z).normalize();
            let right = m.transform_vector3(Vec3::X).normalize();
            // The navigation lights show only in their sectors: red ahead and
            // out to the left, green to the right, white behind; each fades
            // out a little past 90 degrees from the middle of its sector, so
            // all show from above and below (lights.wgsl).
            let (inten, col, dir, spread) = match l.id as f32 {
                lamp::NAV_RED => (40.0 * nav, L_RED, (fwd - right).normalize(), -0.35),
                lamp::NAV_GREEN => (40.0 * nav, L_GREEN, (fwd + right).normalize(), -0.35),
                lamp::TAIL => (25.0 * nav, L_WHITE, -fwd, -0.35),
                lamp::STROBE => (1600.0 * p5[1], L_STROBE, Vec3::ZERO, 0.0),
                lamp::BEACON => (160.0 * p5[2], L_RED, Vec3::ZERO, 0.0),
                _ => (if big { 6000.0 } else { 3000.0 } * p5[3], L_WHITE, fwd, 0.86),
            };
            if l.id == lamp::LANDING as u32 && p5[3] > 0.0 && l.at.x.abs() > 0.5 {
                let k = (l.at.x > mid) as usize;
                beam[k].0 += at;
                beam[k].1 += 1.0;
                beam_dir = fwd;
            }
            if inten > 0.0 && out.len() < render::MAX_DYN_LIGHTS {
                out.push(airfield::Light { pos: at.into(), inten, dir: dir.into(), spread, col: [col[0], col[1], col[2], 3], fx: [0, 0, 255, 1] });
            }
        }
        // The beams: aimed a little below the aircraft's axis, as they are
        // set to light the runway ahead in the flare; per side about 3e5 cd
        // on an airliner, 1e5 on a light aircraft (Globals units: cd / 1e4 /
        // pi), times 60: MOON_E is far brighter than the real moon, and the
        // beams graze the runway, so true-to-life ones would hardly show
        // against moonlit pavement.
        if beam_dir != Vec3::ZERO {
            let up = (*base * pose[0]).transform_vector3(Vec3::Y).normalize();
            let dir = (beam_dir - up * 0.11).normalize();
            let per_side = if big { 3.0e5 } else { 1.0e5 } / 1.0e4 / std::f32::consts::PI * 60.0;
            let n = beam.iter().filter(|b| b.1 > 0.0).count() as f32;
            for (i, b) in beam.iter().enumerate() {
                if b.1 > 0.0 {
                    let at = b.0 / b.1;
                    g.spot[i] = [at.x, at.y, at.z, per_side * 2.0 / n];
                }
            }
            g.spot[2] = [dir.x, dir.y, dir.z, 14f32.to_radians().cos()];
        }
    }

    pub(crate) fn sun_dir(&self) -> DVec3 {
        let decl = (-23.44f64).to_radians() * (std::f64::consts::TAU * (self.doy + 10.0) / 365.25).cos();
        let lon = (-15.0 * (self.utc - 12.0)).to_radians();
        DVec3::new(decl.cos() * lon.cos(), decl.cos() * lon.sin(), decl.sin())
    }

    /// Whether snow lies on flat ground at `p` (for snow spray vs dirt dust).
    pub(crate) fn snowy_at(&self, p: DVec3) -> bool {
        match self.snow {
            snow::Mode::Off => false,
            snow::Mode::Always => true,
            snow::Mode::Auto => {
                let (la, _, alt) = geo::pos_to_lla(p);
                let Some(base) = snow::snow_line(self.snow, la.to_degrees(), self.doy, self.wx.freezing) else { return false };
                let line = self.snowmap.snow_line_at(p, base, self.doy);
                snow::cover(alt, 1.0, 0.0, line) > 0.35
            }
        }
    }

    /// Direction to the moon: the sun's ecliptic longitude advanced by the
    /// moon's elongation, carried into the same Earth-fixed frame as the sun.
    pub(crate) fn moon_dir(&self) -> (DVec3, f64) {
        use std::f64::consts::{FRAC_PI_2, TAU};
        let eps = 23.44f64.to_radians();
        let radec = |lam: f64| ((eps.cos() * lam.sin()).atan2(lam.cos()), (eps.sin() * lam.sin()).asin());
        let lam_s = TAU * (self.doy + 10.0) / 365.25 - FRAC_PI_2;
        let elong = TAU * self.wx.moon_age / 29.530588;
        let (ra_s, _) = radec(lam_s);
        let (ra_m, dec_m) = radec(lam_s + elong);
        let lon = (-15.0 * (self.utc - 12.0)).to_radians() + ra_m - ra_s;
        let lit = 0.5 * (1.0 - elong.cos());
        (DVec3::new(dec_m.cos() * lon.cos(), dec_m.cos() * lon.sin(), dec_m.sin()), lit)
    }

    pub(crate) fn cockpit_globals(&self, agl: f64) -> [[f32; 4]; 12] {
        let p = &self.plane;
        let (lat, lon, h) = geo::pos_to_lla(p.pos);
        let (east, north, up) = geo::enu(p.pos);
        let gs = (p.vel - up * p.vel.dot(up)).length() * 1.94384;
        let hdg = p.forward().dot(east).atan2(p.forward().dot(north));
        let mut data = [[0.0; 4]; 12];
        data[0] = [(self.radios.setting(self.wx.qnh) * 0.029529983 * 100.0) as f32,
            ((h * 3.28084 + (1013.25 - self.wx.qnh) * 27.3) / 100.0) as f32,
            gs as f32, (p.spec.fuel_kg / 0.72 / 3.785 / 2.0) as f32];
        let adf = self.radios.adf.as_ref().and_then(|s| nav::adf_bearing(s, p.pos, hdg.to_degrees())).filter(|_| p.sys.powered());
        data[1] = [self.radios.nav.as_ref().map(|s| s.freq_khz as f32 / 1000.0).unwrap_or(0.0),
            self.radios.adf.as_ref().map(|s| s.freq_khz as f32).unwrap_or(0.0), adf.unwrap_or(0.0) as f32, if adf.is_some() { 1.0 } else { 0.0 }];
        data[2] = [self.gauges.cylinder_head_f(), ((p.trim + 0.05) / 0.29) as f32, if p.sys.powered() { 1.0 } else { 0.0 }, p.engine_hours as f32];
        let plot = |pos: DVec3| { let v = pos - p.pos; [v.dot(east) as f32 / 18520.0, v.dot(north) as f32 / 18520.0, 1.0, 0.0] };
        if let Some(st) = &self.radios.nav { data[3] = plot(st.pos()); }
        // (row 3's last slot: the fuel flow in US gal/h, for the Bonanza's gauge)
        data[3][3] = (p.fuel_flow * 3600.0 / 0.72 / 3.785) as f32;
        for (dst, (st, _)) in data[4..8].iter_mut().zip(nav::near(lat.to_degrees(), lon.to_degrees(), 18520.0, nav::Kind::Vor).iter().take(4)) { *dst = plot(st.pos()); }
        // Rows 8-11: what the glass cockpits show of the autopilot and the
        // systems (common.wgsl `Globals::cockpit`, deck650.wgsl).
        {
            use autopilot::{Lat, Vert};
            let ap = &self.ap;
            let bit = |b: bool, v: u32| if b { v } else { 0 };
            let bits = bit(ap.on, 1) | bit(ap.at, 2)
                | match ap.lat { Lat::Roll => 0, Lat::Hdg => 4, Lat::Nav | Lat::Gps => 8, Lat::Apr => 16 }
                | match ap.vert { Vert::Pitch => 0, Vert::Alt => 32, Vert::Vs => 64, Vert::Gs => 128 }
                | bit(ap.nav_cap, 256) | bit(ap.gs_cap, 512) | bit(ap.alt_cap, 1024) | bit(ap.slow, 2048) | bit(ap.fast, 4096);
            data[8] = [bits as f32, ap.hdg.rem_euclid(360.0) as f32, (ap.alt * 3.28084) as f32, (ap.spd * 1.943_84) as f32];
            data[9] = [(ap.vs * 196.85) as f32, p.trim.to_degrees() as f32, p.speedbrake as f32, p.spoilers as f32];
            let flags = bit(p.running, 1) | bit(p.starter_on, 2) | bit(p.ignition, 4) | bit(p.sys.powered(), 8) | bit(p.on_ground, 16) | bit(p.parking_brake, 32);
            data[10] = [p.reverser as f32, flags as f32, p.toe_brake[0].max(p.toe_brake[1]) as f32, (p.alpha / p.stall_alpha().max(0.01)) as f32];
            // The cabin: held near the field's height on the ground and low,
            // scheduled to about 4,850 ft at 51,000 ft (an estimate, not the
            // real pressurisation schedule). The speed trend: the along-path
            // acceleration (the accelerometers' specific force less gravity)
            // carried 6 s ahead.
            let alt_ft = h * 3.28084;
            let agl_ft = agl * 3.28084;
            let k = ((agl_ft - 500.0) / 3000.0).clamp(0.0, 1.0);
            let cabin = alt_ft + (alt_ft * 0.095 - alt_ft).min(0.0) * k;
            let air = p.vel - p.wind;
            let acc = p.rot * p.sf - up * 9.81;
            let trend = if air.length() > 1.0 { acc.dot(air.normalize()) * 6.0 * 1.943_84 } else { 0.0 };
            data[11] = [p.sys.elec.volts as f32, p.sys.elec.amps as f32, cabin as f32, trend as f32];
        }
        data
    }

    /// (-1, -1) where Settings › Camera turns looking around the other way in this view.
    pub(crate) fn look_signs(&self) -> (f64, f64) {
        let (ix, iy) = if self.cam_mode == 1 {
            (self.invert_look_cockpit, self.invert_look_cockpit)
        } else {
            (self.invert_look_outside_x, self.invert_look_outside_y)
        };
        (if ix { -1.0 } else { 1.0 }, if iy { -1.0 } else { 1.0 })
    }

    /// NAV 1 as the cockpit's course deviation indicators show it (Globals::nav).
    pub(crate) fn nav_globals(&self) -> [f32; 4] {
        let obs = self.radios.obs as f32;
        let Some(st) = self.radios.nav.as_ref().filter(|_| self.plane.sys.powered()) else {
            return [0.0, 0.0, obs, 0.0];
        };
        let r = nav::nav_reading(st, self.radios.obs, self.plane.pos);
        if !r.live {
            return [0.0, 0.0, obs, 0.0];
        }
        let mut flags = 1.0;
        match r.to {
            Some(true) => flags += 2.0,
            Some(false) => flags += 4.0,
            None => {}
        }
        if r.gs.is_some() {
            flags += 8.0;
        }
        [r.cdi as f32, r.gs.unwrap_or(0.0) as f32, obs, flags]
    }
}
