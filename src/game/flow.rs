//! A flight's course: starting at an airport or a place, loading, respawning, the start and end of a flight and its log, and going back to the globe.

use crate::*;

impl Game {
    pub(crate) fn respawn(&mut self, lat: f64, lon: f64, alt: f64, heading: f64) {
        self.respawn_at(lat, lon, alt, heading, self.spec.cruise);
    }

    /// The aircraft's missiles can be used: it has some, in arcade mode.
    pub(crate) fn missiles_ok(&self) -> bool {
        self.arcade && self.weapons.has_weapons()
    }

    pub(crate) fn missiles_refusal(&self) -> &'static str {
        if self.weapons.has_weapons() {
            t!("game.missiles_are_for_arcade_mode")
        } else {
            t!("game.no_missiles_on_this_aircraft")
        }
    }

    /// Says that `what` is arcade mode's.
    pub(crate) fn arcade_only(&mut self, what: &str) {
        self.status = t!("game.is_for_arcade_mode_settings", what);
        self.status_until = self.real + 3.0;
    }

    pub(crate) fn respawn_at(&mut self, lat: f64, lon: f64, alt: f64, heading: f64, speed: f64) {
        self.end_flight(false);
        self.cancel_cockpit_input();
        self.ap.disengage();
        self.hold_spot = None;
        self.plane.parking_brake = false;
        self.mcp_throttle = None;
        self.plane = Plane::spawn(self.spec, lat, lon, alt, heading, speed);
        // Rain set by hand comes along to a new place (its deck closes in there).
        self.wx.deck_moved(self.plane.pos);
        self.plane.fuel = self.spec.fuel_kg * self.menu.fuel;
        self.plane.payload = self.start_payload();
        // The spawn speed is through the air, not over the ground.
        let (la, lo, h) = geo::pos_to_lla(self.plane.pos);
        let agl = self.world.ground(la, lo).map_or(300.0, |g| h - g.h);
        self.plane.wind = self.wx.steady(self.plane.pos, agl.max(2.0));
        self.plane.vel += self.plane.wind;
        if let Some(spec) = &self.damage_spec {
            self.plane.dmg.apply_spec(spec);
        }
        self.restore_wreck_meshes();
        self.flash.clear();
        self.crashed = false;
        self.cam_rise = 0.0;
        self.cam_ang = None;
        self.acc = 0.0;
        self.cam_pos = self.plane.pos - self.plane.forward() * self.spec.cam_dist;
        self.cam_dir = -self.plane.forward();
        self.prev_cam = self.cam_pos;
        self.warn_t = [-10.0; 4];
        (self.g_slow, self.was_ground, self.prev_vs, self.prev_gear, self.prev_flaps) = (1.0, false, 0.0, self.plane.gear, self.plane.flaps);
        self.gauges = gauges::Gauges::new(&self.plane, self.frame ^ 0x9e37);
        self.weapons.reset(self.spec, &self.model.pieces);
        self.weapon_flash = None;
        self.kills.clear();
        self.contrails.clear();
    }

    /// On the ground, at rest, waiting for the terrain under it to load.
    pub(crate) fn respawn_ground(&mut self, lat: f64, lon: f64, h: f64, heading: f64, pitch: f64) {
        self.respawn_at(lat, lon, h + 10.0, heading, 0.0);
        self.plane = Plane::spawn_ground(self.spec, lat, lon, h, heading, pitch);
        self.plane.fuel = self.spec.fuel_kg * self.menu.fuel;
        self.plane.payload = self.start_payload();
        if self.menu.cold {
            self.plane.cold_and_dark();
        }
        if let Some(spec) = &self.damage_spec {
            self.plane.dmg.apply_spec(spec);
        }
        self.gauges = gauges::Gauges::new(&self.plane, self.frame ^ 0x51ed);
        self.cam_pos = self.plane.pos - self.plane.forward() * self.spec.cam_dist;
        self.cam_dir = -self.plane.forward();
        self.prev_cam = self.cam_pos;
        (self.prev_gear, self.prev_flaps, self.was_ground) = (1.0, 0.0, true);
        self.hold_spot = Some((lat, lon, self.real + 30.0));
    }

    /// Start at an airport: its layout is put together in the background
    /// (see `airport_rx`), then the aircraft appears lined up on the runway
    /// in use, or on a final approach to it.
    /// The loading screen: done once the ground under the aircraft is there and
    /// the tiles it needs have come in (or after `Loading::MAX` seconds).
    pub(crate) fn update_loading(&mut self) {
        let Some(l) = self.loading.as_mut() else { return };
        if let Some(done) = l.done {
            if self.real - done > Loading::FADE {
                self.loading = None;
            }
            return;
        }
        let photo = self.world.photo();
        let n = self.world.stats.1 + self.world.queued() + if photo.active { photo.stats.2 } else { 0 };
        l.most = l.most.max(n);
        l.left = n;
        let (lat, lon, _) = geo::pos_to_lla(self.plane.pos);
        let ground = self.world.ground(lat, lon).is_some();
        let t = self.real - l.t0;
        // How far along: the tiles come in, and time passes (never going back).
        let tiles = if l.most > 0 { 1.0 - n as f64 / l.most as f64 } else { 0.0 };
        l.frac = l.frac.max((0.92 * tiles + 0.08 * (t / Loading::MAX)).min(if ground { 1.0 } else { 0.9 }));
        // (A few tiles far off are always on their way: "nearly none" is enough.)
        let quiet = ground && n <= 3;
        l.quiet_since = if quiet { Some(l.quiet_since.unwrap_or(self.real)) } else { None };
        if (t > Loading::MIN && l.quiet_since.is_some_and(|q| self.real - q > 0.6)) || t > Loading::MAX {
            self.finish_loading();
        }
    }

    pub(crate) fn finish_loading(&mut self) {
        if let Some(l) = self.loading.as_mut() {
            l.frac = 1.0;
            l.done.get_or_insert(self.real);
        }
    }

    pub(crate) fn start_airport(&mut self, idx: u32, runway: Option<String>, approach: bool, keep_local: bool) {
        let a = &airports::Db::get().apts[idx as usize];
        self.status = t!("game.preparing", a.code(), a.name);
        self.status_until = self.real + 60.0;
        let (tx, rx) = std::sync::mpsc::channel();
        let src = self.src.clone();
        std::thread::spawn(move || {
            let g = airports::geo_for(&src, idx).map_err(|e| log::warn!("airport: {e}")).ok();
            let _ = tx.send((idx, g, runway, approach, keep_local));
        });
        self.airport_rx = Some(rx);
        self.menu.starting = true;
    }

    pub(crate) fn airport_ready(&mut self, idx: u32, g: Option<Arc<airports::Geo>>, runway: Option<String>, approach: bool, keep_local: bool) {
        let a = &airports::Db::get().apts[idx as usize];
        let local = if keep_local { (self.utc + geo::pos_to_lla(self.plane.pos).1.to_degrees() / 15.0).rem_euclid(24.0) } else { self.menu.hour };
        self.utc = (local - a.lon / 15.0).rem_euclid(24.0);
        if self.mode == Mode::Menu {
            self.loading = Some(Loading::new(self.real));
        }
        self.mode = Mode::Fly;
        self.menu.starting = false;
        self.status_until = self.real + 12.0;
        let Some(g) = g.filter(|g| !g.runways.is_empty()) else {
            // Nothing on the map to land on: overhead instead.
            self.respawn(a.lat.to_radians(), a.lon.to_radians(), a.elev as f64 + 450.0, self.heading);
            self.status = t!("game.no_runway_layout_on_the", a.code(), a.name);
            return;
        };
        let want = runway.as_deref().map(airports::norm_ident);
        let pick = want.as_ref().and_then(|w| g.runways.iter().enumerate().find_map(|(ri, r)| (0..2).find(|e| airports::norm_ident(&r.ident[*e]) == *w).map(|e| (ri, e))));
        let (ri, e) = pick.or_else(|| g.active(self.wx.wind_from as f32, (self.wx.wind * 1.94384) as f32)).unwrap_or((0, 0));
        let r = &g.runways[ri];
        let (thr, dir) = r.end(e);
        let heading = (dir.x as f64).atan2(dir.y as f64);
        if approach {
            let d = if r.len > 1500.0 { 7000.0 } else { 4500.0 };
            let p = thr - dir * d;
            let (lat, lon) = g.lla(p);
            let alt = g.h(thr) as f64 + (d + traffic::aim_dist(r.len)) as f64 * 3f64.to_radians().tan();
            // Approach speed (70 kt in the light single), approach flaps, gear
            // down, trimmed down a 3 degree glide.
            let (v, notch) = self.spec.approach;
            self.respawn_at(lat, lon, alt, heading, v);
            self.plane.configure(notch, true, v * 3f64.to_radians().sin());
            self.status = t!("game.final_approach_runway_nm", a.code(), a.name, r.ident[e], d / 1852.0);
            // NAV 1 on the runway's ILS, as a crew would have it set up.
            let (la, lo, _) = geo::pos_to_lla(self.plane.pos);
            if let Some((st, _)) = nav::near(la.to_degrees(), lo.to_degrees(), 20_000.0, nav::Kind::Ils).into_iter().find(|(s, _)| s.name == a.ident && s.ident == r.ident[e]) {
                self.radios.tune_nav(st, self.plane.pos);
            }
        } else {
            let start = if e == 0 { r.a } else { r.b } + dir * 25.0;
            let (lat, lon) = g.lla(start);
            // On a sloping runway it stands nose up (or down) with the slope.
            let pitch = ((g.h(start + dir * 3.0) - g.h(start - dir * 3.0)) / 6.0).atan() as f64;
            self.respawn_ground(lat, lon, g.h(start) as f64, heading, pitch);
            self.status = t!("game.runway_m_m", a.code(), a.name, r.ident[e], r.len, r.width);
        }
    }

    /// Esc or the gamepad's B on the start menu: a step back towards the
    /// main menu (which stays put: Quit is a button of its own).
    pub(crate) fn menu_back(&mut self) {
        use menu::Screen;
        match self.menu.screen {
            Screen::Main => return,
            Screen::Aircraft => self.aircraft_back(),
            Screen::Fly => self.menu.screen = Screen::Main,
            Screen::Settings => self.menu.screen = self.menu.set_back,
        }
        self.pad_home = true;
        self.egui_ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }

    /// Open or close the pause menu. The game stops while it is open.
    pub(crate) fn set_pause_menu(&mut self, open: bool) {
        if open == self.pause_open || self.mode != Mode::Fly {
            return;
        }
        self.cancel_cockpit_input();
        self.pause_open = open;
        self.radial.open = false;
        self.pause_page = menu::Page::Game;
        self.pad_home = open;
        self.drag = false;
        self.show_help = false;
        // A button or slider left focused would keep the keys from the aircraft.
        self.egui_ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }

    /// Back to the globe, looking at where we were.
    pub(crate) fn to_globe(&mut self) {
        self.end_flight(false);
        self.set_pause_menu(false);
        self.info_open = false;
        let (lat, lon, _) = geo::pos_to_lla(self.plane.pos);
        self.menu.look_at(lat.to_degrees(), lon.to_degrees(), 60_000.0);
        self.menu.pick = Some((lat.to_degrees(), lon.to_degrees()));
        self.menu.hour = (self.utc + lon.to_degrees() / 15.0).rem_euclid(24.0);
        self.menu.starting = false;
        self.menu.screen = menu::Screen::Fly;
        self.mode = Mode::Menu;
    }

    /// The aircraft as the flight log follows it.
    pub(crate) fn log_sample(&self) -> flightlog::Sample {
        let p = &self.plane;
        flightlog::Sample { pos: p.pos, vel: p.vel, forward: p.forward(), right: p.right(), on_ground: p.on_ground, g: p.g_load }
    }

    /// What the flight log saw: a graded touchdown shows on the status line.
    pub(crate) fn flight_event(&mut self, ev: flightlog::Event) {
        if let flightlog::Event::Landed(l) = ev {
            let place = match (&l.airport, &l.on_runway) {
                (Some(a), Some(r)) => format!("{a} {}", r.runway),
                (Some(a), None) => a.clone(),
                _ => String::new(),
            };
            self.status = if place.is_empty() { t!("log.landing", l.summary()) } else { t!("log.landing_at", place, l.summary()) };
            self.status_until = self.real + 10.0;
        }
    }

    /// The flight under way is over: into the logbook with it.
    pub(crate) fn end_flight(&mut self, crashed: bool) {
        if let Some(f) = self.flight.take().and_then(|t| t.finish(crashed)) {
            self.logbook.add(f);
            if self.logbook_saves {
                self.logbook.save();
            }
        }
    }

    /// A fresh aircraft in the air over where we are (or where we crashed), on the same heading.
    pub(crate) fn respawn_here(&mut self) {
        let (lat, lon, h) = geo::pos_to_lla(if self.crashed { self.crash.as_ref().map(|c| c.site).unwrap_or(self.plane.pos) } else { self.plane.pos });
        let g = self.world.ground(lat, lon).map(|g| g.h).unwrap_or(h);
        let hd = self.plane.forward().dot(geo::enu(self.plane.pos).0).atan2(self.plane.forward().dot(geo::enu(self.plane.pos).1));
        self.respawn(lat, lon, g + 420.0, hd);
    }

    /// Fly from a known spot: its ground height is looked up in the background,
    /// then the aircraft appears above it (see `goto_rx`).
    pub(crate) fn fly_to(&mut self, lat: f64, lon: f64, name: &str) {
        self.status = t!("game.flying_to", i18n::tr_en_owned(name));
        self.status_until = self.real + 30.0;
        let (tx, rx) = std::sync::mpsc::channel();
        let src = self.src.clone();
        let name = name.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(Some((lat, lon, ground_elev(&src, lat, lon), name)));
        });
        self.goto_rx = Some(rx);
    }

    /// Menu: choose a spot on the globe and look its name up in the background.
    pub(crate) fn pick(&mut self, lat: f64, lon: f64) {
        self.menu.pick = Some((lat, lon));
        self.menu.airport = None;
        self.menu.runway = None;
        self.menu.pick_name = match menu::Menu::near_city(lat, lon) {
            Some((name, d)) if d < 40_000.0 => tr_en(name).to_string(),
            Some((name, d)) if d < 400_000.0 => t!("game.km_from", d / 1000.0, if crate::i18n::locale() == "fr" { crate::i18n::de_place(tr_en(name)) } else { tr_en(name).to_string() }),
            _ => String::new(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let src = self.src.clone();
        std::thread::spawn(move || {
            let url = format!("https://nominatim.openstreetmap.org/reverse?format=json&zoom=12&accept-language={}&lat={lat:.5}&lon={lon:.5}", i18n::lang().code());
            let name = src.fetch.get(&url).ok().and_then(|d| serde_json::from_slice::<serde_json::Value>(&d).ok()).and_then(|j| j["display_name"].as_str().map(String::from));
            if let Some(n) = name {
                let _ = tx.send(n);
            }
        });
        self.name_rx = Some(rx);
    }

    /// Menu: choose an airport to start at.
    pub(crate) fn choose_airport(&mut self, i: u32) {
        let a = &airports::Db::get().apts[i as usize];
        self.menu.look_at(a.lat, a.lon, self.menu.alt.min(25_000.0));
        self.menu.pick = Some((a.lat, a.lon));
        self.menu.pick_name = format!("{} {} ({}, {})", a.code(), a.name, a.city, a.country);
        if self.menu.airport != Some(i) {
            self.menu.runway = None;
        }
        self.menu.airport = Some(i);
        self.name_rx = None;
    }

    /// Menu: take off at the chosen spot (once its ground height is known).
    pub(crate) fn begin(&mut self) {
        let Some((lat, lon)) = self.menu.pick else { return };
        if self.menu.starting {
            return;
        }
        self.set_aircraft(aircraft::nth(self.menu.aircraft));
        // remember the start for next time.
        let db = airports::Db::get();
        self.last_start = Some(settings::LastStart {
            lat,
            lon,
            name: self.menu.pick_name.clone(),
            airport: self.menu.airport.map(|i| db.apts[i as usize].ident.clone()),
            start: match self.menu.start {
                menu::Start::Runway => "runway",
                menu::Start::Approach => "approach",
                menu::Start::Overhead => "overhead",
            }
            .into(),
            runway: self.menu.runway.clone(),
            aircraft: aircraft::nth(self.menu.aircraft).keys.first().copied().unwrap_or("c172").into(),
        });
        self.keep_settings(true);
        self.plan.begin((lat, lon));
        if let Some(i) = self.menu.airport.filter(|_| self.menu.start != menu::Start::Overhead) {
            self.start_airport(i, self.menu.runway.clone(), self.menu.start == menu::Start::Approach, false);
            return;
        }
        self.menu.starting = true;
        let (tx, rx) = std::sync::mpsc::channel();
        let src = self.src.clone();
        std::thread::spawn(move || {
            let _ = tx.send((lat, lon, ground_elev(&src, lat, lon)));
        });
        self.start_rx = Some(rx);
    }
}
