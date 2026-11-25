//! The autopilot's controls: engaging it, its modes and steps, and following the flight plan.

use crate::*;

impl Game {
    /// The flight panel's radios: NAV 1 (VOR or ILS) and the ADF tuned from
    /// the stations around, the OBS, and the altimeter setting.
    /// The autopilot flies, if engaged: its stick and throttle replace the
    /// pilot's (whose small inputs add to them; a firm one disconnects it).
    pub(crate) fn autopilot(&mut self, dt: f64, c: &mut Controls) {
        // (Scripted presses: --ap.)
        while let Some(k) = self.ap_script.iter().position(|(t, _)| self.real >= *t) {
            let (_, m) = self.ap_script.remove(k);
            match m.as_str() {
                "ap" => self.ap_toggle(),
                "hdg" => self.ap_lat(autopilot::Lat::Hdg),
                "nav" => self.ap_lat(autopilot::Lat::Nav),
                "gps" => self.ap_lat(autopilot::Lat::Gps),
                "apr" => self.ap_lat(autopilot::Lat::Apr),
                "alt" => self.ap_vert(autopilot::Vert::Alt),
                "vs" => self.ap_vert(autopilot::Vert::Vs),
                "at" => self.ap.at = !self.ap.at,
                _ => {}
            }
        }
        if self.mode == Mode::Fly && dt > 0.0 {
            self.follow_plan();
        }
        if !self.ap.on || dt <= 0.0 || self.mode != Mode::Fly || self.pause_open {
            return;
        }
        if self.crashed {
            self.ap_disconnect(None);
            return;
        }
        let s = autopilot::State::of(&self.plane);
        let r = {
            let nav = self.radios.nav.as_ref().map(|st| autopilot::Nav { st, reading: nav::nav_reading(st, self.radios.obs, self.plane.pos), obs: self.radios.obs });
            self.ap.step(&s, &self.plane, nav.as_ref(), (c.pitch, c.roll), dt)
        };
        match r {
            Ok(o) => {
                c.pitch = o.pitch;
                c.roll = o.roll;
                if let Some(t) = o.throttle_set {
                    // The throttle keys take the power back from the autothrottle.
                    if c.throttle != 0.0 {
                        self.ap.at = false;
                    } else {
                        c.throttle_set = Some(t);
                    }
                }
            }
            Err(why) => self.ap_disconnect(Some(why)),
        }
    }

    /// sequences the plan and feeds gps.
    pub(crate) fn follow_plan(&mut self) {
        let (lat, lon, _) = geo::pos_to_lla(self.plane.pos);
        let at = (lat.to_degrees(), lon.to_degrees());
        let up = self.plane.pos.normalize();
        let gs = (self.plane.vel - up * self.plane.vel.dot(up)).length();
        if !self.plane.on_ground {
            let alt_ft = geo::pos_to_lla(self.plane.pos).2 * 3.28084;
            if let Some(i) = self.plan.sequence(at, gs, alt_ft) {
                let w = &self.plan.wps[i];
                self.status = match self.plan.target() {
                    Some(n) => match n.leg {
                        flightplan::Leg::Vectors { course } => t!("game.vectors_heading_03_0_go_direct", course),
                        _ => {
                            let cons = n.alt.map(|a| format!("  ·  {}", a.text())).unwrap_or_default();
                            t!("game.passed_next", w.ident, n.ident, cons)
                        }
                    },
                    None => t!("game.reached_end_of_the_flight_plan", w.ident),
                };
                self.status_until = self.real + 5.0;
            }
        }
        self.ap.gps_trk = self.plan.steer(at).map(|s| s.track);
    }

    pub(crate) fn ap_disconnect(&mut self, why: Option<autopilot::Off>) {
        use autopilot::Off;
        self.ap.disengage();
        self.ap_off_t = self.real;
        log::info!("autopilot off: {why:?}");
        self.status = match why {
            Some(Off::Pilot) | None => t!("game.autopilot_off").into(),
            Some(Off::Ground) => t!("game.autopilot_off_on_the_ground").into(),
            Some(Off::Stall) => t!("game.autopilot_off_stall").into(),
            Some(Off::Attitude) => t!("game.autopilot_off_unusual_attitude").into(),
        };
        self.status_until = self.real + 4.0;
    }

    /// F5: the autopilot on (holding the heading and altitude of the
    /// moment, unless modes were picked beforehand) or off.
    pub(crate) fn ap_toggle(&mut self) {
        if self.ap.on {
            self.ap_disconnect(None);
            return;
        }
        if self.plane.on_ground || self.crashed || self.spec.rocket().is_some() {
            self.status = t!("game.the_autopilot_works_in_flight").into();
            self.status_until = self.real + 3.0;
            return;
        }
        let s = autopilot::State::of(&self.plane);
        if self.ap.alt == 0.0 {
            self.ap.sync(&s);
        }
        if self.ap.lat == autopilot::Lat::Roll {
            self.ap.set_lat(autopilot::Lat::Hdg, &s);
            self.ap.hdg = s.hdg.round();
        }
        if self.ap.vert == autopilot::Vert::Pitch {
            self.ap.set_vert(autopilot::Vert::Alt, &s);
        }
        self.ap.engage(&s);
        log::info!("autopilot on: {:?} {:?}", self.ap.lat, self.ap.vert);
        self.status = t!("game.autopilot_on").into();
        self.status_until = self.real + 2.0;
    }

    /// A lateral mode button: picks it (and engages the autopilot), or, pressed again, back to HDG.
    pub(crate) fn ap_lat(&mut self, m: autopilot::Lat) {
        let s = autopilot::State::of(&self.plane);
        let m = if self.ap.on && self.ap.lat == m && m != autopilot::Lat::Hdg { autopilot::Lat::Hdg } else { m };
        // (Off, nothing set yet: the heading and altitude of the moment.)
        if !self.ap.on && self.ap.alt == 0.0 {
            self.ap.sync(&s);
            // gps takes the cruise altitude.
            if m == autopilot::Lat::Gps && !self.plan.done() {
                self.ap.alt = (self.plan.cruise_ft / 100.0).round() * 100.0 / 3.28084;
            }
        }
        // NAV and APR stay armed, flying the heading bug, until NAV 1 has the course.
        if matches!(m, autopilot::Lat::Nav | autopilot::Lat::Apr) && self.radios.nav.is_none() {
            self.status = t!("game.armed_tune_nav_1_radio").into();
            self.status_until = self.real + 3.0;
        }
        // gps needs a plan.
        if m == autopilot::Lat::Gps && self.plan.done() {
            self.status = t!("game.armed_no_flight_plan_make_one").into();
            self.status_until = self.real + 3.0;
        }
        self.ap.set_lat(m, &s);
        if !self.ap.on {
            self.ap_toggle();
        }
    }

    pub(crate) fn ap_vert(&mut self, m: autopilot::Vert) {
        let s = autopilot::State::of(&self.plane);
        if !self.ap.on && self.ap.alt == 0.0 {
            self.ap.sync(&s);
        }
        if m == autopilot::Vert::Vs && self.ap.vert == autopilot::Vert::Vs {
            self.ap.set_vert(autopilot::Vert::Alt, &s);
        } else {
            self.ap.set_vert(m, &s);
        }
        if !self.ap.on {
            self.ap_toggle();
        }
    }

    /// The autothrottle on or off (holding the airspeed of the moment if none is set).
    pub(crate) fn ap_at_toggle(&mut self) {
        self.ap.at = !self.ap.at;
        if self.ap.at && self.ap.spd <= 0.0 {
            self.ap.spd = autopilot::State::of(&self.plane).ias;
        }
    }

    /// The vertical speed the autopilot flies in VS, up or down 100 ft/min.
    pub(crate) fn ap_vs_step(&mut self, up: bool) {
        self.ap.vs = ((self.ap.vs * 196.85 / 100.0).round() * 100.0 + if up { 100.0 } else { -100.0 }).clamp(-4000.0, 4000.0) / 196.85;
    }

    /// The autothrottle's speed, up or down 5 kt.
    pub(crate) fn ap_spd_step(&mut self, up: bool) {
        self.ap.spd = ((self.ap.spd * 1.943_84).round() + if up { 5.0 } else { -5.0 }).max(40.0) / 1.943_84;
    }
}
