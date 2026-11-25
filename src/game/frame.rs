//! One frame: the simulation stepped, the world updated, the scene drawn.

use crate::*;

impl Game {
    pub(crate) fn frame(&mut self) {
        self.sync_mouse_capture();
        self.keep_settings(false);
        self.random_failures();
        if let Some(err) = self.r.take_gpu_error() {
            eprintln!("GPU error: {err}");
            self.status = format!("GPU error: {err}");
            self.status_until = self.real + 15.0;
        }
        if self.r.is_device_lost() {
            eprintln!("GPU device lost; exiting cleanly.");
            self.quit = true;
            return;
        }
        let now = Instant::now();
        crate::gputime::cpu_begin();
        let raw_dt = (now - self.last).as_secs_f64();
        let mut dt_real = raw_dt.min(0.25);
        if self.fps_log.0 {
            let l = &mut self.fps_log;
            l.2 += 1;
            l.3 = l.3.max(raw_dt);
            let el = (now - l.1).as_secs_f64();
            if el >= 1.0 {
                let n = l.2 as f64;
                let st = std::mem::take(&mut self.r.stats);
                log::info!("{:.1} fps avg ({:.1} ms), slowest frame {:.1} ms | game {:.1}  wait-gpu {:.1}  encode {:.1}  submit {:.1} ms", n / el, el / n * 1000.0, l.3 * 1000.0, st[0] / n * 1000.0, st[1] / n * 1000.0, st[2] / n * 1000.0, st[3] / n * 1000.0);
                (l.1, l.2, l.3) = (now, 0, 0.0);
                // GPU memory in use (cargo build --features mem-stats).
                #[cfg(feature = "mem-stats")]
                {
                    let c = self.r.device.get_internal_counters().hal;
                    let mb = |v: isize| v as f64 / (1 << 20) as f64;
                    let ((nt, bt), (nb, bb)) = self.world.gpu_usage();
                    log::info!("gpu memory: buffers {:.0} MB ({}), textures {:.0} MB ({}) | terrain {nt} tiles {:.0} MB, buildings {nb} tiles {:.0} MB", mb(c.buffer_memory.read()), c.buffers.read(), mb(c.texture_memory.read()), c.textures.read(), mb(bt as isize), mb(bb as isize));
                }
            }
        }
        if self.record.is_some() {
            // Frozen for a few wall-clock seconds so the city streams in first.
            dt_real = if self.rec_t0.elapsed().as_secs_f64() < 8.0 { 0.0 } else { 1.0 / REC_FPS };
        }
        self.last = now;
        self.real += dt_real;
        self.frame += 1;
        if self.frame > 1 && let Some(b) = &mut self.bench {
            let size = (self.r.config.width, self.r.config.height);
            if b.frame(raw_dt, &self.r.gpu.name, size, &self.gfx) {
                self.quit = true;
            }
        }
        self.pump_mcp();
        if self.record.is_none() {
            self.fps += (1.0 / raw_dt.max(1e-4) - self.fps) * 0.05;
        }
        // dynamic quality: judged on flying frames only (not menus, pauses or recordings)
        if self.gfx.dynamic && self.mode == Mode::Fly && !self.paused && self.record.is_none() && (self.bench.is_none() || self.dyn_cli) {
            let hz = self.window.current_monitor().and_then(|m| m.refresh_rate_millihertz()).map(|m| m as f32 / 1000.0);
            let target = dynamic::target_ms(&self.gfx, if self.gfx.vsync || self.gfx.dyn_fps == 0 { hz } else { None });
            // (vsync or a frame limit at the target holds frames back, hiding any room)
            let paced = self.gfx.vsync || (self.gfx.fps_cap > 0 && (1000.0 / self.gfx.fps_cap as f32 - target).abs() < 0.01);
            self.dynq.update(raw_dt as f32, self.r.frame_gpu_ms(), target, paced);
        }

        // Results of background lookups.
        if let Some(rx) = &self.goto_rx {
            if let Ok(res) = rx.try_recv() {
                self.goto_rx = None;
                match res {
                    Some((lat, lon, elev, name)) if self.mode == Mode::Menu => {
                        self.menu.look_at(lat, lon, 45_000.0);
                        self.menu.pick = Some((lat, lon));
                        self.menu.pick_name = name;
                        let _ = elev;
                    }
                    Some((lat, lon, elev, name)) => {
                        let local = (self.utc + geo::pos_to_lla(self.plane.pos).1.to_degrees() / 15.0).rem_euclid(24.0);
                        self.utc = (local - lon / 15.0).rem_euclid(24.0);
                        self.respawn(lat.to_radians(), lon.to_radians(), elev + 450.0, 0.0);
                        self.status = name;
                    }
                    None => {
                        self.status = t!("game.place_not_found").into();
                        self.menu.pick_name = t!("game.place_not_found").into();
                    }
                }
                self.status_until = self.real + 8.0;
            }
        }
        if let Some(rx) = &self.name_rx {
            if let Ok(name) = rx.try_recv() {
                self.menu.pick_name = name;
                self.name_rx = None;
            }
        }
        if let Some(rx) = &self.start_rx {
            if let Ok((lat, lon, elev)) = rx.try_recv() {
                self.start_rx = None;
                self.utc = (self.menu.hour - lon / 15.0).rem_euclid(24.0);
                self.respawn(lat.to_radians(), lon.to_radians(), elev + self.menu.start_agl, self.heading);
                self.status = self.menu.pick_name.clone();
                self.status_until = self.real + 10.0;
                self.loading = Some(Loading::new(self.real));
                self.mode = Mode::Fly;
                self.menu.starting = false;
            }
        }
        if let Some(rx) = &self.airport_rx {
            if let Ok((idx, g, rw, appr, keep)) = rx.try_recv() {
                self.airport_rx = None;
                self.airport_ready(idx, g, rw, appr, keep);
            }
        }
        if let Some((idx, rw, appr)) = self.pending_airport.take() {
            self.start_airport(idx, rw, appr, false);
        }
        // Released once the ground under the wheels is there in full detail.
        if let Some((lat, lon, until)) = self.hold_spot {
            let fine = self.world.ground_zoom(lat, lon).is_some_and(|z| z >= self.world.max_zoom.min(16));
            if (fine && self.real > until - 28.0) || self.real > until {
                if let Some(gr) = self.world.ground(lat, lon) {
                    let up = self.plane.pos.normalize();
                    self.plane.pos = up * (geo::R + gr.h + self.plane.stand_height());
                }
                self.plane.hold = false;
                self.plane.t = 2.0;
                self.hold_spot = None;
            }
        }
        if let Some(t) = self.menu_go {
            if self.real > t && self.mode == Mode::Menu && !self.menu.starting {
                self.pick(self.menu.lat, self.menu.lon);
                self.begin();
                self.menu_go = None;
            }
        }
        let in_menu = self.mode == Mode::Menu;

        if self.radial_at.is_some_and(|t| self.real >= t && self.mode == Mode::Fly) {
            self.flight_menu_key();
            self.radial_at = None;
        }
        if let Some((page, sec, t)) = self.pause_at {
            if self.real >= t && self.mode == Mode::Fly {
                if page.is_info() {
                    (self.info_open, self.info_page) = (true, page);
                } else {
                    self.set_pause_menu(true);
                    self.pause_page = page;
                    if let Some(s) = sec {
                        self.settings_sec = s;
                    }
                }
                self.pause_at = None;
            }
        }
        if !in_menu {
            self.update_loading();
        }
        let loading = self.loading.as_ref().is_some_and(|l| l.done.is_none());
        let scale = if self.paused || in_menu || self.pause_open || self.real < self.hold || loading { 0.0 } else { 1.0 };
        let dt = dt_real * scale;
        self.time += dt;
        let mut glass_rain = 0.0f32;
        if !in_menu {
            // The clock runs at the panel's rate; past midnight the date turns over.
            let t = self.utc + dt * self.wx.time_rate / 3600.0;
            self.doy = (self.doy + (t / 24.0).floor()).rem_euclid(365.0);
            self.utc = t.rem_euclid(24.0);
            if self.wx.real_time {
                (self.utc, self.doy) = weather::real_clock();
                self.wx.time_rate = 1.0;
            }
            // The air the aircraft flies through, gusts and all.
            let (la, lo, h) = geo::pos_to_lla(self.plane.pos);
            let g = self.world.ground(la, lo);
            let gh = g.as_ref().map(|g| g.h);
            let agl = h - gh.unwrap_or(0.0);
            let water = g.map(|g| g.water).unwrap_or(false);
            self.wx.sun = self.sun_dir().dot(self.plane.pos.normalize());
            // Turbulence, thermals and storm drafts ride on top of the gusting wind.
            let mut air = self.wx.air(self.plane.pos, agl, self.time, water);
            // The ground's shape: ridge lift, lee waves, and the rotor under them.
            let world = &self.world;
            self.oro.update(&self.wx, self.plane.pos, self.time, |la, lo| world.ground(la, lo).map(|g| g.h));
            let oro = self.oro.at(&self.wx, self.plane.pos, agl);
            if oro.rotor > 0.0 {
                air.sigma_w = air.sigma_w.hypot(oro.rotor);
                air.sigma_h = air.sigma_h.hypot(oro.rotor);
                // (A rotor's eddies are a few hundred metres across.)
                let k = (oro.rotor / 2.0).min(1.0);
                air.len_w += (air.len_w.min(250.0) - air.len_w) * k;
                air.len_h += (air.len_h.min(250.0) - air.len_h) * k;
            }
            air.draft += oro.w;
            self.plane.air = air;
            self.plane.wind = self.wx.at(self.plane.pos, agl, self.time) + self.plane.pos.normalize() * air.draft + air.gust;
            // The rain on the windows: only under (or in) a cloud that rains, the
            // rain deck the slider sets or a storm cell.
            self.wx.step_deck(dt, self.plane.pos, gh);
            self.wx.rain = self.wx.rain_at(self.plane.pos, agl, self.time) as f32;
            self.wx.advance(dt, self.plane.pos);
            // The wakes of the other aircraft about: airport traffic, live traffic and other players.
            if dt > 0.0 {
                let mut targets = self.live.targets();
                targets.extend(self.world.air_targets(self.plane.pos));
                let sources: Vec<wake::Source> = targets
                    .iter()
                    .filter(|t| t.pos.distance(self.plane.pos) < 20_000.0)
                    .map(|t| {
                        use std::hash::{Hash, Hasher};
                        let mut hsh = std::collections::hash_map::DefaultHasher::new();
                        t.key.hash(&mut hsh);
                        let ty = &airmodels::TYPES[(t.size as usize).min(airmodels::TYPES.len() - 1)];
                        wake::Source { id: hsh.finish(), pos: t.pos, vel: t.vel, span: ty.span as f64, mass: ty.mass as f64 }
                    })
                    .chain(self.mp_players.iter().filter(|r| r.pos.distance(self.plane.pos) < 20_000.0).map(|r| {
                        // Other players: their own aircraft's span and mass.
                        let spec = aircraft::nth(r.aircraft as usize);
                        wake::Source { id: 0x6d70_0000_0000_0000 ^ r.id as u64, pos: r.pos, vel: r.vel, span: spec.b, mass: spec.mass }
                    }))
                    .collect();
                let (wx, world) = (&self.wx, &self.world);
                self.wakes.sigma = air.sigma_w;
                // (Each node reads the ground under it once, as it is laid.)
                let ground_at = |p: DVec3| {
                    let (la, lo, _) = geo::pos_to_lla(p);
                    world.ground(la, lo).map(|g| g.h)
                };
                self.wakes.step(dt, self.time, &sources, |p, agl| wx.steady(p, agl), ground_at, h - agl, self.plane.pos);
                self.plane.wake = self.wakes.near(self.plane.pos, 150.0 + self.plane.spec.b, self.time);
            }
            // (Snow does not wet the glass or streak across it: only the liquid rain share does.)
            let fz = match (self.snow, self.wx.freezing) {
                (snow::Mode::Always, _) | (_, None) => snow::freezing_guess(self.snow, la.to_degrees(), self.doy),
                (_, Some(f)) => f,
            };
            glass_rain = self.wx.rain * (1.0 - snow::snow_share(h, fz) as f32);
            self.glass.step(dt, glass_rain, self.plane.ias() * 1.94384);
        }

        crate::gputime::cpu("sim.weather+controls");
        // Physics at a fixed step.
        let c = self.controls(dt_real);
        let mut emit = Vec::new();
        let mut femit: Vec<(DVec3, render::Particle)> = Vec::new();
        let (mut scrape_n, mut steps) = (0usize, 0usize);
        if !self.crashed && !in_menu {
            // A flight starts with the first step of a new aircraft, and ends parked after landing.
            if self.flight.as_ref().is_some_and(|f| f.parked()) {
                self.end_flight(false);
            }
            if self.flight.is_none() {
                self.flight = Some(flightlog::Tracker::new(self.spec.name));
            }
            let weapon_targets = if !self.missiles_ok() {
                Vec::new()
            } else {
                let mut t = self.live.targets();
                t.extend(self.world.air_targets(self.plane.pos));
                t
            };
            let h = 1.0 / 180.0;
            const MAX_SUBSTEPS: usize = 45;
            self.acc = (self.acc + dt).min(h * MAX_SUBSTEPS as f64);
            while self.acc >= h && steps < MAX_SUBSTEPS {
                self.acc -= h;
                let imp = self.plane.step(h, &c, &self.world);
                if self.flight.is_some() {
                    let s = self.log_sample();
                    let events = self.flight.as_mut().map(|f| f.step(s, h)).unwrap_or_default();
                    for ev in events {
                        self.flight_event(ev);
                    }
                }
                let powered = self.plane.sys.powered();
                for ev in self.weapons.step(h, &self.plane, &weapon_targets, &self.world, powered) {
                    self.weapon_event(ev, &mut femit);
                }
                self.gauges.update(h, &self.plane, self.time);
                steps += 1;
                scrape_n += self.plane.sparks.len();
                // Anything the aircraft ploughed through goes over in its direction of travel.
                if !self.plane.tree_hits.is_empty() {
                    let hits: Vec<_> = self.plane.tree_hits.drain(..).collect();
                    self.anchor_fx(hits[0].pos);
                    for t in hits {
                        self.destr.knock(&mut self.world, &self.r.queue, &t, self.plane.vel, 1.3, 0.0, 0.0);
                    }
                    if let Some(a) = &self.audio {
                        a.event(audio::Event::Crunch((self.plane.vel.length() / 50.0).clamp(0.4, 1.6) as f32));
                    }
                }
                // Parts that came off: let go of them where the aircraft is now.
                if !self.plane.shed.is_empty() {
                    self.anchor_fx(self.plane.pos);
                    let pose = self.plane.pose(&self.model.pieces);
                    let reqs: Vec<_> = self.plane.shed.drain(..).collect();
                    for req in &reqs {
                        self.sheds.spawn(req, &self.plane.dmg_prev, &self.model.pieces, &self.piece_mesh, &self.piece_cut, &pose, self.plane.model_pos(), self.plane.rot, self.plane.vel, &mut self.rng, &mut femit);
                    }
                }
                for n in self.plane.notes.drain(..) {
                    self.flash.push((n, self.real));
                    if let Some(a) = &self.audio {
                        a.event(audio::Event::Snap(1.0));
                    }
                }
                if !self.plane.splashes.is_empty() {
                    self.anchor_fx(self.plane.pos);
                    for (p, v) in self.plane.splashes.drain(..) {
                        if self.rng.f() < 0.25 {
                            let r = &mut self.rng;
                            femit.push((p, render::Particle { vel: (v.as_vec3() + r.dir() * 2.0).into(), life: r.range(1.0, 2.5), size0: r.range(0.15, 0.35), size1: r.range(0.5, 1.2), kind: crash::DROPS, drag: r.range(0.2, 0.8), seed: r.f(), alpha: r.range(0.3, 0.55), ..Default::default() }));
                        }
                    }
                }
                if !self.plane.snow_marks.is_empty() {
                    if self.snow != snow::Mode::Off {
                        for m in self.plane.snow_marks.drain(..) {
                            self.snowtrack.stamp(m);
                        }
                    } else {
                        self.plane.snow_marks.clear();
                    }
                }
                if !self.plane.dirt.is_empty() {
                    self.anchor_fx(self.plane.pos);
                    let up = self.plane.pos.normalize().as_vec3();
                    let field = self.world.airfield_near(self.plane.pos, 3_000.0);
                    let snowy = self.snowy_at(self.plane.pos);
                    for (p, v, amt) in self.plane.dirt.drain(..) {
                        // Paved runways throw up nothing; dirt, grass and fields do.
                        if field.is_some_and(|f| f.geo.on_runway(f.geo.local(p), 2.0).is_some_and(|ri| f.geo.runways[ri].paved)) {
                            continue;
                        }
                        let speed = v.length() as f32;
                        let n = amt * (speed / 8.0).clamp(0.3, 1.5) * 0.2;
                        let r = &mut self.rng;
                        let mut k = n;
                        while k > 0.0 && r.f() < k {
                            k -= 1.0;
                            let big = amt > 1.5;
                            let vel = v.as_vec3() * r.range(0.6, 1.2) + r.dir() * (1.0 + speed * 0.15) + up * r.range(0.5, 2.5);
                            let kind = if snowy && (!big || r.f() < 0.7) { crash::MIST } else { crash::DUST };
                            femit.push((p, render::Particle { vel: vel.into(), life: r.range(1.5, 4.0) * if big { 1.5 } else { 1.0 }, size0: r.range(0.3, 0.7), size1: r.range(2.0, 4.0) * if big { 1.6 } else { 1.0 }, kind, drag: 1.4, buoy: 0.15, seed: r.f(), alpha: r.range(0.45, 0.75), ..Default::default() }));
                        }
                    }
                }
                if !self.plane.sparks.is_empty() {
                    self.anchor_fx(self.plane.pos);
                    let up = self.plane.pos.normalize().as_vec3();
                    for (p, v) in self.plane.sparks.drain(..) {
                        if self.rng.f() < 0.3 {
                            let r = &mut self.rng;
                            femit.push((p, render::Particle { vel: (v.as_vec3() + r.dir() * 3.0 + up * 1.5).into(), life: r.range(0.3, 0.9), size0: 0.05, size1: 0.02, kind: crash::SPARK, drag: 0.6, seed: r.f(), alpha: 0.9, ..Default::default() }));
                        }
                    }
                }
                if let Some(mut imp) = imp {
                    self.r.clear_particles();
                    let tears = self.wreck_meshes();
                    // A house is no match for an airliner: it goes through
                    // it and on, slowed about as much as the house weighs.
                    let size = (self.spec.span / 5.5).max(1.0);
                    let heavy = size > 2.0;
                    // What it brings to a building: its kinetic energy.
                    let e_hit = 0.5 * self.spec.mass * imp.vel.length_squared();
                    let mut through = 0.0f32;
                    if let Some((tile, idx)) = imp.building.filter(|_| heavy) {
                        if let Some((base, top, area)) = self.world.footprint(tile, idx) {
                            let mb = area.max(25.0) as f64 * (top - base).max(3.0) * 120.0 * 0.3;
                            let keep = self.spec.mass / (self.spec.mass + mb);
                            if keep > 0.55 {
                                through = keep as f32;
                                // (the blast stays where the aircraft went up, up the
                                // side of the house; only a low hit is at street level)
                                let (la, lo, alt) = geo::pos_to_lla(imp.pos);
                                let g = self.world.ground(la, lo).map(|g| g.h).unwrap_or(base);
                                if alt - g < 6.0 {
                                    imp.ground_h = g;
                                }
                            }
                        }
                    }
                    let mut crash = Crash::new(&imp, &self.plane, &self.group_mesh, &self.model.groups, &self.group_pivots, &tears, through, self.rng.0 ^ self.frame, self.wx.steady(imp.pos, 40.0));
                    self.r.fluid.latest = None;
                    let (site, power) = (crash.site, crash.power);
                    self.fx_anchor = site;
                    let mut gone = 0.0;
                    // How far the blast reaches beyond a light aircraft's.
                    let reach = if heavy { size.powf(0.6) } else { 1.0 };
                    if let Some((tile, idx)) = imp.building {
                        // The building that was hit comes down, and any shed or small house right beside it.
                        let (la, lo, alt) = geo::pos_to_lla(site);
                        if let Some(g) = self.world.ground(la, lo) {
                            crash.set_lift(alt - g.h);
                        }
                        let d = &self.plane.dmg;
                        let hit = destruct::Hitter { right: self.plane.right(), up: self.plane.up(), span: d.wing, shape: &self.spec.hole, full: self.spec.span, stab: d.stab, fin: d.fin };
                        self.bring_down(tile, idx, imp.pos, imp.vel, e_hit, Some(hit), &mut femit);
                        // (next door only gets the blast and the flying pieces: a hundredth of it)
                        for (t, i) in self.world.buildings_near(site, 9.0 * (power * reach) as f64, if heavy { 700.0 } else { 160.0 }) {
                            self.bring_down(t, i, imp.pos, imp.vel * 0.3, e_hit * 0.01, None, &mut femit);
                        }
                    } else if !imp.water {
                        // Open ground: a bite is taken out of it (a big one
                        // where an airliner came down steeply).
                        let steep = (-imp.vel.dot(site.normalize()) / imp.vel.length().max(1.0)).clamp(0.0, 1.0) as f32;
                        let r = (3.0 + 5.0 * power) * (1.0 + (size.sqrt() - 1.0) * steep);
                        self.world.add_crater(site, r);
                        if self.snow != snow::Mode::Off {
                            let upw = site.normalize();
                            let tang = imp.vel - upw * imp.vel.dot(upw);
                            let along = tang.normalize_or_zero();
                            let skid_len = (r as f64 * (1.8 - 1.1 * steep as f64)).max(0.0);
                            self.snowtrack.stamp(snowtrack::Mark {
                                a: site - along * (r as f64 * 0.5),
                                b: site + along * skid_len,
                                radius: r * 1.35,
                                clear: 1.0,
                                berm: 1.0,
                                rough: 1.0,
                            });
                        }
                        self.destr.eject(site, r, imp.vel, &mut femit);
                        gone = r as f64;
                        for (t, i) in self.world.buildings_near(site, r as f64 + 4.0 * reach as f64, if heavy { 900.0 } else { 400.0 }) {
                            self.bring_down(t, i, imp.pos, imp.vel * 0.3, e_hit * 0.01, None, &mut femit);
                        }
                    }
                    if !imp.water {
                        // A fireball flattens the trees around; without one only those
                        // the wreck itself ploughs into go down.
                        let blast = if crash.ignited { 26.0 + 30.0 * power as f64 } else { 6.0 + 4.0 * power as f64 };
                        self.destr.blast_trees(&mut self.world, &self.r.queue, site, blast * reach as f64, gone, &mut femit);
                    }
                    self.crash = Some(crash);
                    self.crashed = true;
                    self.end_flight(true);
                    self.crash_real = self.real;
                    self.orbit = 0.0;
                    // Watch from back along the way it came in: that air is
                    // clear (it just flew through it), so the camera never
                    // starts inside the building or behind it.
                    {
                        let (e, n, u) = geo::enu(site);
                        let v = imp.vel - u * imp.vel.dot(u);
                        let back = if v.length() > 1.0 { -v.normalize() } else { -self.plane.forward() };
                        let a = back.dot(n).atan2(back.dot(e));
                        self.cam_ang = Some(a);
                        let dist = (70.0 + 34.0 * power as f64) * (self.crash.as_ref().map_or(1.0, |c| c.size) as f64).sqrt() + self.cam_dist;
                        let horiz = e * a.cos() + n * a.sin();
                        self.cam_pos = site + horiz * dist * 0.6 + u * (12.0 + dist * 0.15);
                        self.cam_look = site;
                    }
                    self.cam_clear_t = 0.0;
                    self.cam_rise = 0.0;
                    self.acc = 0.0;
                    break;
                }
            }
        }
        crate::gputime::cpu("sim.flight_physics+weapons");
        if dt > 0.0 && !self.kills.is_empty() {
            self.step_kills(dt, &mut femit);
        }
        if dt > 0.0 {
            self.destr.update(dt as f32, &self.world, &mut femit);
            // Buildings next door that heavy falling pieces went through.
            let hits: Vec<_> = self.destr.crush.drain(..).collect();
            for (t, i, p, v, e) in hits {
                self.bring_down(t, i, p, v * 0.4, e, None, &mut femit);
                self.destr.booms.push(0.5);
            }
            self.sheds.update(dt as f32, &self.world);
            if !self.sheds.snow_marks.is_empty() {
                if self.snow != snow::Mode::Off {
                    for m in self.sheds.snow_marks.drain(..) {
                        self.snowtrack.stamp(m);
                    }
                } else {
                    self.sheds.snow_marks.clear();
                }
            }
            // Birds (birds.rs): fly, dive out of the way, now and then strike.
            if !in_menu {
                for s in self.step_birds(dt) {
                    self.bird_strike(s, &mut femit);
                }
            }
        }
        crate::gputime::cpu("sim.destruction+birds");
        let booms: Vec<f32> = self.destr.booms.drain(..).collect();
        if !booms.is_empty() {
            let near = self.crash.as_ref().map(|cr| 1.0 / (1.0 + ((self.cam_pos - cr.site).length() as f32 / 400.0).powi(2))).unwrap_or(0.5);
            for b in booms {
                if let Some(a) = &self.audio {
                    a.boom(b * (0.4 + 0.6 * near));
                }
            }
        }
        // Smoke from a damaged engine: oil-white while it still runs, black once it has seized.
        let sm = self.plane.dmg.smoke;
        if sm > 0.0 && dt > 0.0 && !self.crashed && !in_menu {
            self.anchor_fx(self.plane.pos);
            self.smoke_acc += sm * 22.0 * dt as f32;
            let exhaust = self.plane.pos + self.plane.rot * DVec3::new(0.335, -0.75, -2.10);
            let up = self.plane.pos.normalize().as_vec3();
            let fwd = self.plane.forward().as_vec3();
            let pv = self.plane.vel.as_vec3();
            let black = self.plane.dmg.smoke_black;
            while self.smoke_acc >= 1.0 {
                self.smoke_acc -= 1.0;
                let r = &mut self.rng;
                femit.push((exhaust, render::Particle { vel: (pv * 0.3 - fwd * 2.0 + r.dir() * 1.5 + up * 0.5).into(), life: r.range(2.0, 4.5), size0: r.range(0.25, 0.5), size1: r.range(2.5, 5.0), temp: 0.05, kind: if black { crash::GAS } else { crash::MIST }, drag: 1.2, buoy: 0.8, seed: r.f(), alpha: r.range(0.25, 0.45) * sm.clamp(0.4, 1.0), ..Default::default() }));
            }
        }
        let mut fire = 0.0f32;
        let crash_snowy = self.crash.as_ref().is_some_and(|cr| !cr.water && self.snowy_at(cr.site));
        if let Some(cr) = self.crash.as_mut() {
            if dt > 0.0 {
                // Smoke leaving the top of the fire simulation carries on as particles.
                cr.top_out = match (&cr.domain, self.r.fluid.latest) {
                    (Some(dm), Some(st)) if st.out > 1e-3 => {
                        // Just outside the face it leaves through.
                        let f = st.out_c;
                        let edge = |v: f32| if v < 0.08 { -0.01 } else if v > 0.92 { 1.01 } else { v };
                        let fr = Vec3::new(edge(f.x), edge(f.y), if f.z > 0.92 { 1.01 } else { f.z });
                        let at = dm.at_frac(fr) - cr.site;
                        Some((at.as_vec3(), st.out * 1.8 / 110.0, 5.0))
                    }
                    _ => None,
                };
                cr.update(dt, &self.world, crash_snowy, &mut emit);
            }
            if !cr.snow_marks.is_empty() {
                if self.snow != snow::Mode::Off {
                    for m in cr.snow_marks.drain(..) {
                        self.snowtrack.stamp(m);
                    }
                } else {
                    cr.snow_marks.clear();
                }
            }
        }
        // What the wreckage ploughed through on its way comes down.
        let crushed: Vec<_> = self.crash.as_mut().map(|cr| cr.crush.drain(..).collect()).unwrap_or_default();
        for (t, i, p, v, e) in crushed {
            self.bring_down(t, i, p, v * 0.5, e, None, &mut femit);
            self.destr.booms.push(0.6);
            if let Some(a) = &self.audio {
                a.event(audio::Event::Crunch(1.4));
            }
        }
        if let Some(cr) = self.crash.as_mut() {
            if let Some(a) = &self.audio {
                let near = 1.0 / (1.0 + ((self.cam_pos - cr.site).length() as f32 / 250.0).powi(2));
                for p in &cr.booms {
                    a.boom(p * (0.35 + 0.65 * near));
                }
                for p in &cr.splashes {
                    a.event(audio::Event::Splash(p * (0.35 + 0.65 * near)));
                }
                let f = if cr.water { 0.0 } else { (cr.light().min(12.0) / 12.0) * near };
                fire = (f + self.destr.rumble.min(1.0) * near) * scale as f32;
            }
            cr.booms.clear();
            cr.splashes.clear();
        }
        for (pos, mut p) in femit {
            p.pos = (pos - self.fx_anchor).as_vec3().into();
            emit.push(p);
        }
        // Smoke and dust that would start inside the fire simulation's grid go
        // into the grid instead, where they can mix and flow with the rest.
        if let Some(cr) = self.crash.as_mut().filter(|_| dt > 0.0) {
            if let Some(dm) = &cr.domain {
                let off = (self.fx_anchor - cr.site).as_vec3();
                let mut conv = Vec::new();
                emit.retain(|p| {
                    let k = p.kind as i32;
                    let pos = Vec3::from(p.pos) + off;
                    if matches!(k, 0 | 2 | 7) && dm.contains(pos, 2.0) {
                        conv.push((pos, *p));
                        false
                    } else {
                        true
                    }
                });
                let keep = 48usize;
                let scale = (conv.len() as f32 / keep as f32).max(1.0);
                let step = scale.ceil() as usize;
                for (pos, p) in conv.iter().step_by(step.max(1)) {
                    let r = (p.size1 * 0.55).clamp(1.5, 7.0);
                    let amt = p.alpha * 1.2 / r * scale / dt as f32;
                    let k = p.kind as i32;
                    cr.fluid.push(fluid::FluidSrc {
                        pos: *pos,
                        radius: r,
                        vel: Vec3::from(p.vel),
                        radial: 1.0,
                        soot: if k == 0 { amt * 0.5 } else { 0.0 },
                        dust: if k == 0 { 0.0 } else { amt },
                        temp: if k == 0 { p.temp.min(1.2) } else { 0.0 },
                        vel_k: 2.5,
                        temp_k: 4.0,
                        ..Default::default()
                    });
                }
            }
        }
        if !emit.is_empty() {
            self.r.emit(&emit);
        }
        // Missiles leave a trail of smoke (and a little flame on the boost).
        if !in_menu && !self.weapons.missiles.is_empty() {
            let mut trail = Vec::new();
            for m in &self.weapons.missiles {
                let pos = (m.pos - self.fx_anchor).as_vec3();
                let back = (m.rot * DVec3::Z).as_vec3();
                let n = if m.burn > 0.0 { 2 } else { 1 };
                for _ in 0..n {
                    let r = &mut self.rng;
                    let drift = r.dir() * r.range(0.4, 1.6);
                    trail.push(render::Particle {
                        pos: pos.into(),
                        vel: (back * r.range(3.0, 8.0) + drift).into(),
                        life: r.range(1.2, 3.0),
                        size0: if m.burn > 0.0 { 0.18 } else { 0.28 },
                        size1: r.range(1.2, 3.2),
                        temp: if m.burn > 0.0 { 0.15 } else { 0.0 },
                        kind: crash::MIST,
                        drag: 1.0,
                        buoy: 0.4,
                        seed: r.f(),
                        alpha: 0.4,
                        ..Default::default()
                    });
                }
            }
            self.r.emit(&trail);
        }

        // ---- camera
        self.apply_graphics();
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1) as f32, size.height.max(1) as f32);
        // the size the scene is drawn at (graphics settings: render resolution)
        let (rw, rh) = { let (a, b) = self.r.render_size(); (a as f32, b as f32) };
        let mut fov = 60f32.to_radians();
        let (cam, dir, upv);
        if !(self.cam_mode == 1 && !self.crashed && !in_menu) {
            self.head_live = false;
        }
        if in_menu {
            (cam, dir, upv) = if self.menu.screen == menu::Screen::Fly {
                self.menu.update(dt_real);
                self.menu.camera()
            } else {
                // Off the globe (the main menu, the aircraft and settings
                // screens) the camera turns from the planet to space: level,
                // square to the sun, so neither the planet nor the sun is in
                // view and little but the stars is drawn.
                let (cam, down, _) = self.menu.camera();
                let up = -down;
                let side = up.cross(self.sun_dir());
                let dir = if side.length() > 0.2 { side.normalize() } else { geo::enu(cam).1 };
                (cam, dir, up)
            };
            // The globe's sun is where it is now (the terminator, the night
            // side's lights), unless the start time has been set by hand: then
            // that time at the chosen spot (or under the camera).
            let lon = self.menu.pick.map(|p| p.1).unwrap_or(self.menu.lon);
            if self.wx.real_time {
                // Real time: the start time is the time now at the chosen spot.
                let (utc, doy) = weather::real_clock();
                self.menu.hour = (utc + lon / 15.0).rem_euclid(24.0);
                (self.utc, self.doy) = (utc, doy);
            } else {
                if self.menu_time_seen.0.is_finite() && (self.menu.hour, self.doy) != self.menu_time_seen {
                    self.menu_time_set = true;
                }
                if self.menu_time_set {
                    self.utc = (self.menu.hour - lon / 15.0).rem_euclid(24.0);
                } else {
                    (self.utc, self.doy) = weather::real_clock();
                }
            }
            self.menu_time_seen = (self.menu.hour, self.doy);
            self.cam_pos = cam;
        } else if self.crashed {
            let cr = self.crash.as_ref().unwrap();
            // After an impact up the side of a building, the wreck ends up in the street below.
            // While the fire burns up at the hole (6 s, see `Crash::set_lift`),
            // watch it there; then follow the wreck down to the street.
            let down = { let x = ((cr.time - 5.0) / 4.0).clamp(0.0, 1.0); x * x * (3.0 - 2.0 * x) };
            let street = cr.site - cr.up * (cr.lift * down as f64);
            let mut target = cr.debris.first().filter(|d| !d.sunk).map(|d| d.pos).unwrap_or(street) * 0.5 + street * 0.5 + cr.up * 6.0;
            // (an airliner's fireball is a few hundred metres across: further back)
            let mut dist = (70.0 + 34.0 * cr.power as f64) * (cr.size as f64).sqrt() + self.cam_dist;
            // (into the water there is no fireball to take in: once the
            // spray has come down, closer, to see the wreck afloat)
            if cr.water {
                let settle = ((cr.time - 4.0) / 8.0).clamp(0.0, 1.0);
                let k = settle * settle * (3.0 - 2.0 * settle);
                dist = (38.0 + 14.0 * cr.power as f64) * (cr.size as f64).sqrt() * (1.0 - 0.7 * k) + self.cam_dist * (1.0 - 0.5 * k);
            }
            let (mut centre, mut rise) = (street, 18.0 + dist * 0.24);
            // An airliner's wreck slides a long way: watch the middle of the
            // trail it leaves, and the fireball over it.
            if let Some(w) = cr.wreck_centre() {
                let w = w - cr.up * (w - street).dot(cr.up);
                if cr.water {
                    // Afloat: watch the wreck from low over the water as it
                    // drifts off from the splash.
                    target = w * 0.85 + street * 0.15 + cr.up * (1.5 * (cr.size as f64).sqrt());
                    centre = w * 0.8 + street * 0.2;
                    rise = (6.0 + dist * 0.18) * (cr.size as f64).sqrt().min(2.0);
                } else {
                    target = w * 0.6 + street * 0.4 + cr.up * (6.0 * (cr.size as f64).sqrt());
                    centre = w * 0.5 + street * 0.5;
                }
            }
            // A tower coming down: back off and watch all of it from the side.
            if let Some((mid, hgt)) = self.destr.focus().filter(|f| (f.0 - cr.site).length() < 800.0) {
                target = target * 0.25 + mid * 0.75;
                dist = dist.max(hgt * 1.2 + 60.0 + self.cam_dist);
                centre = mid;
                rise = dist * 0.1;
            }
            self.cam_look += (target - self.cam_look) * (dt_real * 3.0).min(1.0);
            self.orbit += dt_real * 0.1;
            let (e, n, u) = geo::enu(cr.site);
            let back = (self.cam_pos - centre).normalize_or_zero();
            let mut a = self.cam_ang.unwrap_or_else(|| back.dot(n).atan2(back.dot(e))) + dt_real * 0.12;
            let horiz = |a: f64| e * a.cos() + n * a.sin();
            // Keep a clear view: swing round, or rise a little, to see past
            // whatever building stands in the way.
            if self.real - self.cam_clear_t > 0.25 {
                self.cam_clear_t = self.real;
                let look = centre + u * 6.0;
                let clear = |from: DVec3| {
                    !(0..18).any(|k| {
                        let p = from + (look - from) * (k as f64 / 20.0);
                        let (la, lo, h) = geo::pos_to_lla(p);
                        self.world.building(la, lo).is_some_and(|b| h < b.top + 3.0 && h > b.base - 2.0)
                            // (under the 3D tiles the buildings are the mesh, the ground's top)
                            || (self.world.photo().zone_r > 0.0 && self.world.ground(la, lo).is_some_and(|g| h < g.h + 2.0))
                    })
                };
                // Only move when the view is blocked, and then stay a while.
                if !clear(centre + horiz(a) * dist + u * (rise + dist * self.cam_rise)) && self.real > self.cam_hold {
                    let mut best = (0.0, 0.4);
                    'search: for off in [0.0, 0.35, -0.35, 0.7, -0.7, 1.05, -1.05, 1.4, -1.4, 1.75, -1.75, 2.1, -2.1] {
                        for k in [0.0, 0.15, 0.3] {
                            if clear(centre + horiz(a + off) * dist + u * (rise + dist * k)) {
                                best = (off, k);
                                break 'search;
                            }
                        }
                    }
                    a += best.0;
                    self.cam_rise = best.1;
                    self.cam_hold = self.real + 1.5;
                }
            }
            self.cam_ang = Some(a);
            let want = centre + horiz(a) * dist + u * (rise + dist * self.cam_rise);
            self.cam_pos += (want - self.cam_pos) * (dt_real * 1.2).min(1.0);
            cam = self.cam_pos;
            dir = (self.cam_look - cam).normalize();
            upv = u;
        } else if self.cam_mode == 1 {
            if !self.head_live {
                self.head.reset(self.cam_yaw, self.cam_pitch);
                self.head_live = true;
            }
            let up = self.plane.pos.normalize();
            let day = ((up.dot(self.sun_dir()) as f32 + 0.2) / 0.4).clamp(0.0, 1.0);
            let view = self.head.update(self.spec, dt_real, (self.cam_yaw, self.cam_pitch), self.cockpit_fov, self.plane.head, self.plane.rot, up, w / h, day);
            fov = view.fov.to_radians();
            // A rough engine or damage shakes the whole cabin.
            let vibe = self.plane.dmg.vibe.min(2.0) as f64;
            let mut jit = || self.rng.range(-1.0, 1.0) as f64 * vibe;
            let shake = DVec3::new(jit() * 0.006, jit() * 0.006, 0.0);
            let wob = DQuat::from_rotation_y(jit() * 0.004) * DQuat::from_rotation_x(jit() * 0.004);
            let r = self.plane.rot * wob * view.rot;
            cam = self.plane.model_pos() + self.plane.rot * (view.eye + shake);
            dir = r * DVec3::NEG_Z;
            upv = r * DVec3::Y;
            self.cam_pos = cam;
        } else if self.cam_mode == settings::TAIL_VIEW {
            let at = self.plane.centre();
            let dist = self.cam_dist * if self.plane.stage > 0 { 0.5 } else { 1.0 };
            upv = self.plane.up();
            cam = at - self.plane.forward() * dist + upv * (0.25 * dist);
            dir = (at + self.plane.forward() * (0.25 * dist) - cam).normalize();
            self.cam_pos = cam;
            self.cam_dir = -self.plane.forward();
            self.cam_look = at;
        } else {
            let u = self.plane.pos.normalize();
            // (A rocket points at the sky: its chase camera stays level with it, on the side its back is to, and closes in on the ship once that flies alone.)
            let rocket = self.spec.rocket().is_some();
            let back = if rocket { self.plane.right().cross(u).normalize_or_zero() } else { -self.plane.forward() };
            self.cam_dir = (self.cam_dir + (back - self.cam_dir) * (dt_real * 3.5).min(1.0)).normalize();
            let flat = (self.cam_dir - u * self.cam_dir.dot(u)).normalize_or_zero();
            let tilt = self.cam_dir.dot(u).asin() * 0.6 + self.cam_pitch;
            let d = DQuat::from_axis_angle(u, self.cam_yaw) * (flat * tilt.cos() + u * tilt.sin());
            let vibe = self.plane.dmg.vibe.min(2.0) as f64;
            // (Offsets scale with the aircraft: the camera sits as far back for its size.)
            let sz = self.spec.cam_dist / 16.0;
            let at = self.plane.centre();
            let dist = self.cam_dist * if self.plane.stage > 0 { 0.5 } else { 1.0 };
            if still_mode() {
                // Aircraft-picker stills: fixed to the airframe, aimed at its
                // middle (yaw 0 behind it; pitch in radians, positive above it).
                let (sy, cy) = self.cam_yaw.sin_cos();
                let (sp, cp) = self.cam_pitch.sin_cos();
                cam = at + self.plane.rot * DVec3::new(sy * cp, sp, cy * cp) * self.cam_dist * sz;
                dir = (at - cam).normalize();
                upv = self.plane.up();
            } else {
                cam = at + d * dist + u * (1.5 * sz) + (self.plane.right() * self.rng.range(-1.0, 1.0) as f64 + u * self.rng.range(-1.0, 1.0) as f64) * (vibe * 0.03);
                dir = if rocket { (at - cam).normalize() } else { (self.plane.pos + u * (1.2 * sz) + self.plane.forward() * (6.0 * sz) - cam).normalize() };
                upv = if rocket { u } else { (u + self.plane.up() * 0.25).normalize() };
            }
            self.cam_pos = cam;
            self.cam_look = self.plane.pos;
        }
        crate::gputime::cpu("audio+camera+misc");
        // Never let the camera sink into the ground.
        let (clat, clon, calt) = geo::pos_to_lla(cam);
        let ground = self.world.ground(clat, clon).map(|g| g.h);
        let cam = match ground {
            Some(g) if calt < g + 1.5 => cam + cam.normalize() * (g + 1.5 - calt),
            _ => cam,
        };
        let calt = geo::pos_to_lla(cam).2;
        let agl = calt - ground.unwrap_or(0.0);
        // Storms around the camera: the rain falling there, the lightning.
        if in_menu {
            self.cam_rain = 0.0;
            self.cam_hail = 0.0;
            (self.cam_snow, self.cam_blow) = (0.0, 0.0);
        } else {
            self.cam_rain = self.wx.rain_at(cam, agl, self.time) as f32;
            self.cam_hail = if self.cam_rain > 0.0 { storm::local(&self.wx, cam, agl, self.time).hail as f32 } else { 0.0 };
            let world = &self.world;
            let gnd = |p: DVec3| {
                let (la, lo, _) = geo::pos_to_lla(p);
                world.ground(la, lo).map(|g| g.h).unwrap_or(0.0)
            };
            self.lightning.step(dt, self.time, &self.wx, self.plane.pos, cam, &gnd);
            self.rain_field.step(dt, cam, self.wx.at(cam, agl, self.time));
            self.snow_field.step_fall(dt, cam, self.wx.at(cam, agl, self.time), storm::FLAKE_FALL);
            // Snow (snow.rs): what falls is snow above the melting layer under
            // the freezing level (from the real weather, else the season's);
            // where it reaches the ground fresh snow settles; the wind lifts
            // lying snow into a blizzard.
            let (lat_d, gh) = (clat.to_degrees(), ground.unwrap_or(0.0));
            let fz = match (self.snow, self.wx.freezing) {
                (snow::Mode::Always, _) | (_, None) => snow::freezing_guess(self.snow, lat_d, self.doy),
                (_, Some(f)) => f,
            };
            self.cam_snow = snow::snow_share(calt, fz) as f32;
            let at_ground = self.wx.rain_at(geo::lla_to_pos(clat, clon, gh), 0.0, self.time) * snow::snow_share(gh, fz);
            if self.snow == snow::Mode::Off {
                self.fresh = snow::Fresh::default();
            } else {
                self.fresh.step(dt, at_ground);
            }
            self.fresh_line = snow::Fresh::line(gh) as f32;
            let white = snow::snow_line(self.snow, lat_d, self.doy, self.wx.freezing).map_or(0.0, |l| {
                let l = l + (l.min(snow::Fresh::line(gh)) - l) * self.fresh.k;
                snow::cover(gh, 1.0, 0.0, l)
            });
            // (Blown snow fills the lowest hundred metres or so.)
            let low = 1.0 - snow::smoothstep(60.0, 400.0, agl);
            self.cam_blow = (snow::blowing(white, at_ground.min(1.0), self.wx.wind, self.wx.blowing as f64) * low) as f32;
        }
        let scrape = if steps > 0 { (scrape_n as f32 / steps as f32 * 0.5).min(1.0) } else { 0.0 };
        self.update_audio(cam, fire, in_menu, &c, dt, scrape);

        let near = if in_menu { (calt * 0.02).clamp(0.4, 20_000.0) as f32 } else if self.cam_mode == 1 && !self.crashed { 0.03f32 } else { 0.25f32 };
        let proj = perspective_rev(fov, w / h, near);
        let view = look_to(Vec3::ZERO, dir.as_vec3(), upv.as_vec3());
        let vp = proj * view;
        // Temporal anti-aliasing: each frame is drawn a different fraction of
        // a pixel off centre (a Halton sequence), and the renderer blends the
        // frames back together. Everything else works from the centred `vp`.
        // (FSR 3.1/4.1 jitter whether or not the AA is on, through as many
        // phases as AMD asks for the upscale ratio: upscale.rs.)
        let jitter = if self.r.up.active().temporal() {
            let [jx, jy] = fsr3::jitter_offset(self.frame, fsr3::jitter_phases(rw as u32, w as u32));
            // AMD's offsets are in pixels, y down
            Vec2::new(jx * 2.0 / rw, -jy * 2.0 / rh)
        } else if self.gfx.taa {
            let halton = |mut i: u64, b: u64| { let (mut f, mut r) = (1.0f32, 0.0f32); while i > 0 { f /= b as f32; r += f * (i % b) as f32; i /= b; } r };
            let k = self.frame % 8 + 1;
            Vec2::new(halton(k, 2) - 0.5, halton(k, 3) - 0.5) * 2.0 / Vec2::new(rw, rh)
        } else {
            Vec2::ZERO
        };
        let vp_jit = Mat4::from_translation(jitter.extend(0.0)) * vp;
        let frustum = world::Frustum::new(DMat4::from_cols_array(&vp.to_cols_array().map(|v| v as f64)));
        crate::gputime::cpu("weather.lightning+rain");
        self.world.update(&self.r, cam, &frustum);
        crate::gputime::cpu("world.stream_terrain+buildings");
        let focus_abs = if in_menu { cam } else if self.crashed { self.crash.as_ref().unwrap().site } else { self.plane.pos };
        {
            // Photorealistic 3D tiles in flight, day and night (after dusk the
            // mesh takes our street lamps and lit windows, photo.wgsl).
            let day = if in_menu { 0.0 } else { 1.0 };
            let px = h as f64 / (2.0 * (fov as f64 * 0.5).tan());
            self.world.update_photo(&self.r, cam, focus_abs, &frustum, px, day, dt_real);
        }
        crate::gputime::cpu("world.photo3d_stream");
        if dt > 0.0 {
            self.world.update_traffic(dt as f32, focus_abs);
            let local = (self.utc + geo::pos_to_lla(focus_abs).1.to_degrees() / 15.0).rem_euclid(24.0);
            let player = (!self.crashed && !in_menu).then_some(self.plane.pos);
            self.world.update_airports(dt as f32, self.wx.wind_from as f32, (self.wx.wind * 1.94384) as f32, (cars::activity(local as f32) * 1.2).clamp(0.25, 1.0), player, focus_abs);
        }

        crate::gputime::cpu("world.traffic+airports_sim");
        // ---- frame constants
        let (east, north, up) = geo::enu(cam);
        let delta = cam - self.prev_cam;
        self.prev_cam = cam;
        if delta.length() < 5000.0 {
            self.wave_off += DVec2::new(delta.dot(east), delta.dot(north));
            self.wave_off = DVec2::new(self.wave_off.x.rem_euclid(65536.0), self.wave_off.y.rem_euclid(65536.0));
            self.ocean_off += DVec2::new(delta.dot(east), delta.dot(north));
            self.ocean_off = DVec2::new(self.ocean_off.x.rem_euclid(ocean::WRAP), self.ocean_off.y.rem_euclid(ocean::WRAP));
        }
        // The sea follows the weather's wind (and the clock).
        let sinlat = cam.normalize().z;
        self.world.ocean.update(dt, self.time, self.wx.wind, self.wx.wind_from, sinlat, cam, east, north, self.ocean_off);
        let sun = self.sun_dir();
        let (moon, moon_lit) = self.moon_dir();
        // City light in the air: the lights come on as the sun sets.
        let glow_px = if in_menu { None } else { self.skyglow.update(cam) };
        // Today's real snow cover, when the snow follows the real world.
        let snow_px = self.snowmap.update(&self.src, cam, !in_menu && self.snow == snow::Mode::Auto);
        let snow_map = if in_menu || self.snow != snow::Mode::Auto { [[0.0; 4]; 3] } else { self.snowmap.globals(cam, self.doy) };
        let snow_tr_uploads = if in_menu || self.snow == snow::Mode::Off { Vec::new() } else { self.snowtrack.take_uploads() };
        let snow_tr = if in_menu || self.snow == snow::Mode::Off { [[0.0; 4]; 3] } else { self.snowtrack.globals(cam) };
        let glow = {
            let (la, lo, _) = geo::pos_to_lla(cam);
            let gh = self.world.ground(la, lo).map(|g| g.h).unwrap_or(0.0);
            let on = 1.0 - ((up.dot(sun) as f32 + 0.2) / 0.25).clamp(0.0, 1.0);
            if in_menu { [[0.0; 4]; 3] } else { self.skyglow.globals(cam, gh, on * on * (3.0 - 2.0 * on)) }
        };
        let mu = up.dot(sun) as f32;
        let day = ((mu + 0.2) / 0.4).clamp(0.0, 1.0);
        let day = day * day * (3.0 - 2.0 * day);
        // From the cockpit the eye adapts a little to the cabin.
        // Exposure moves in stops through twilight (a straight blend opens up
        // far too early, washing the city out while the sky still glows).
        // Night stays a night photograph: dark walls, bright windows.
        // Under a storm (in daylight) the eye opens up a stop or so to the gloom.
        let shade = if in_menu { 0.0 } else { storm::shade(&self.wx, cam).max(self.wx.deck_shade(cam)) as f32 };
        self.storm_adapt += (shade * day - self.storm_adapt) * (dt_real as f32 * 0.4).min(1.0);
        // The start screen's globe from orbit is exposed for its day side
        // (common.wgsl `globe_k`), wherever the night falls on it.
        let globe = if in_menu { let k = ((calt as f32 - 150e3) / 650e3).clamp(0.0, 1.0); k * k * (3.0 - 2.0 * k) } else { 0.0 };
        // Over a city at night the eye (or a camera) sets itself by the lights,
        // not the moonlit dark: the walls, the haze and the sky above drop
        // away while the windows and streets stay bright (the facades give
        // their lights back most of it, facade.wgsl). Over open country it
        // stays opened up to the moon and stars.
        if in_menu {
            self.city_probe = None;
        } else if self.city_probe.is_none_or(|(p, _)| p.distance(cam) > 400.0) {
            let (la, lo, alt) = geo::pos_to_lla(cam);
            let gh = self.world.ground(la, lo).map(|g| g.h).unwrap_or(0.0);
            self.city_probe = Some((cam, city_light(la.to_degrees(), lo.to_degrees(), (alt - gh).max(0.0))));
        }
        let lit = self.city_probe.map_or(0.0, |(_, v)| v);
        let city_target = if in_menu { 0.0 } else { lit.sqrt() * (1.0 - day) };
        self.city_adapt += (city_target - self.city_adapt) * (dt_real as f32 * 0.3).min(1.0);
        let exposure = (NIGHT_EXPOSURE.ln() + (GLOBE_EXPOSURE.ln() - NIGHT_EXPOSURE.ln()) * globe + (0.5f32.ln() - NIGHT_EXPOSURE.ln()) * day * (1.0 - globe)).exp() * if self.cam_mode == 1 && !self.crashed && !in_menu { 2f32.powf(self.head.adapt) } else { 1.0 } * 2f32.powf(1.3 * self.storm_adapt - CITY_STOPS * self.city_adapt);
        let v4 = |v: DVec3, w: f64| [v.x as f32, v.y as f32, v.z as f32, w as f32];
        let cam_right = dir.cross(upv).normalize();
        let cam_up = cam_right.cross(dir);
        // Thunder from the lightning just set off, panned to where it struck.
        for th in self.lightning.thunder.drain(..) {
            if let Some(a) = &self.audio {
                a.event(audio::Event::Thunder { delay: th.delay as f32, near: th.near as f32, far: th.far as f32, power: th.power as f32, pan: th.from.dot(cam_right) as f32 });
            }
        }
        // Shadow cascades: three nested boxes around the aircraft, each snapped
        // to its own texel grid so shadows do not shimmer as it moves.
        let shadow_on = mu > 0.04 && !in_menu && self.gfx_eff.shadows != graphics::Level::Off;
        let sres = self.r.shadow_res() as f32;
        // (the aircraft's layer may use only part of it: Renderer::plane_shadow_res)
        let pres = self.r.plane_shadow_res() as f32;
        const CASC: [f32; 3] = [260.0, 1500.0, 8500.0];
        let mut casc_vp = [[[0f32; 4]; 4]; render::CASCADES + 4];
        {
            let right = { let r = DVec3::Z.cross(sun); if r.length_squared() < 1e-6 { DVec3::X } else { r.normalize() } };
            let upl = sun.cross(right);
            for (i, r) in CASC.iter().enumerate() {
                let texel = (2.0 * r / sres) as f64;
                let snap = |v: f64| (v / texel).floor() * texel - v;
                let centre = focus_abs - cam + right * snap(focus_abs.dot(right)) + upl * snap(focus_abs.dot(upl));
                let d = r + 3500.0;
                let lview = look_to(centre.as_vec3() + sun.as_vec3() * d, -sun.as_vec3(), upl.as_vec3());
                casc_vp[i] = (ortho(-r, *r, -r, *r, 0.0, 2.0 * d) * lview).to_cols_array_2d();
            }
            // The aircraft's own shadow map: a few metres across, deep enough
            // to reach the ground below it.
            let centre = (self.plane.pos - cam).as_vec3();
            let (pr, pb) = (self.plane.shadow_r(), PSHADOW_BACK * self.plane.shadow_r() / PSHADOW_R);
            let lview = look_to(centre + sun.as_vec3() * pb, -sun.as_vec3(), upl.as_vec3());
            casc_vp[3] = (ortho(-pr, pr, -pr, pr, 0.0, PSHADOW_DEPTH) * lview).to_cols_array_2d();
        }
        // The two outer cascades take turns: the one skipped keeps last
        // frame's map, its matrix moved by how far the camera has moved since.
        let mut casc_skip = 0u8;
        let sres_now = self.r.shadow_res();
        if !shadow_on || sres_now != self.casc_res {
            self.casc_ok = 0;
            self.casc_res = sres_now;
        }
        let turn = 1 + (self.frame % 2) as usize;
        for i in 1..3 {
            let (vp, at) = self.casc_keep[i - 1];
            if i != turn && self.casc_ok & (1 << i) != 0 && (cam - at).length() < 300.0 {
                casc_vp[i] = (Mat4::from_cols_array_2d(&vp) * Mat4::from_translation((cam - at).as_vec3())).to_cols_array_2d();
                casc_skip |= 1 << i;
            } else if shadow_on {
                self.casc_keep[i - 1] = (casc_vp[i], cam);
                self.casc_ok |= 1 << i;
            }
        }
        let mut shadow_vp = [[[0f32; 4]; 4]; 3];
        shadow_vp.copy_from_slice(&casc_vp[..3]);
        let mut g = Globals {
            view_proj: vp_jit.to_cols_array_2d(),
            inv_view_proj: vp_jit.inverse().to_cols_array_2d(),
            shadow_vp,
            planet: v4(-cam, geo::R),
            sun_dir: v4(sun, SUN_E as f64),
            east: v4(east, self.wave_off.x),
            north: v4(north, self.wave_off.y),
            up: v4(up, calt),
            screen: [rw, rh, 1.0 / rw, 1.0 / rh],
            time: [self.time as f32, dt as f32, exposure, self.frame as f32],
            cloud: [if in_menu { 0.0 } else { self.wx.clouds }, self.wx.base(), self.wx.depth(), self.wx.cloud_ceiling()],
            light_pos: [0.0; 4],
            light_col: [0.0; 4],
            vol_o: [0.0; 4],
            vol_u: [0.0; 4],
            // screen space reflections on water and glass, off on low.
            misc: [near, CITY_STOPS * self.city_adapt, if self.gfx_eff.ssr() { 1.0 } else { 0.0 }, 2.0 * (fov * 0.5).tan() / rh],
            csm: [CASC[0] * 2.0 / sres, CASC[1] * 2.0 / sres, CASC[2] * 2.0 / sres, if shadow_on { 1.0 } else { 0.0 }],
            scorch: [[0.0; 4]; 8],
            crater: [[0.0; 4]; 8],
            prev_vp: self.prev_vp,
            // A jump (teleport, camera switch) invalidates the cloud history.
            cam_delta: v4(delta, if delta.length() < 500.0 { 1.0 } else { 0.0 }),
            moon: v4(moon, (MOON_E as f64) * moon_lit.powf(1.5)),
            vol_e: [0.0; 4],
            vol_n: [0.0; 4],
            pshadow_vp: casc_vp[3],
            pshadow: [self.plane.shadow_r() * 2.0 / pres, 0.0012 / PSHADOW_DEPTH, pres / sres, if shadow_on && !self.crashed { 1.0 } else { 0.0 }],
            taa: [jitter.x * 0.5, -jitter.y * 0.5, if self.gfx.taa { 1.0 } else { 0.0 }, 0.0],
            plane_inv: [[0.0; 4]; 4],
            plane_prev: [[0.0; 4]; 4],
            plane_box: self.plane_box,
            weather: { let o = self.wx.cloud_off; [o.x as f32, o.y as f32, o.z as f32, if in_menu { 1.0 } else { self.wx.haze }] },
            storm: { let w = self.wx.cloud_drift(cam); let d = if w.length() > 0.1 { w.normalize() } else { east }; v4(d, 0.0) },
            storm_cells: [[0.0; 4]; 2 * weather::MAX_STORMS],
            det: [[0.0; 4]; 16],
            photo_vp: [[0.0; 4]; 4],
            photo: [0.0; 4],
            photo_c: [0.0; 4],
            photo_sun: [0.0, 0.0, 0.0, -1.0],
            cut: [[0.0; 4]; 3 * tiles3d::MAX_CUTS],
            cut_v: [[0.0; 4]; tiles3d::MAX_CUT_V / 2],
            cs: [[0.0; 4]; 2 * render::CS_MAP],
            alto: [if in_menu { 0.0 } else { self.wx.alto }, 4800f32.max(self.wx.base() + 0.4 * self.wx.depth() + 800.0), 0.0, 0.0],
            ocean: self.world.ocean.globals(),
            shore_vp: [[0.0; 4]; 4],
            shore: [0.0; 4],
            foam: [[0.0; 4]; 16],
            // (yzw: the liquid rain, how wet the windows are, and the airspeed over them in kt; plane.wgsl rain_water)
            start: [if in_menu { 1.0 } else { 0.0 }, glass_rain, self.glass.wet, (self.plane.ias() * 1.94384) as f32],
            stormfx: [0.0; 4],
            flash: [[0.0; 4]; 2 * storm::MAX_FLASHES],
            rain: [[0.0; 4]; 2],
            quality: [0.0; 4],
            // (w: storm tier, negated for aircraft-picker stills, `still_mode`)
            detail: [self.gfx_eff.city_k(), self.gfx_eff.tree_imp_k(), self.gfx_eff.tree_far(), if still_mode() { -self.gfx_eff.storm_tier() } else { self.gfx_eff.storm_tier() }],
            nimbo: if in_menu { [[0.0; 4]; 4] } else { self.wx.deck_globals() },
            glow_c: glow[0],
            glow_e: glow[1],
            glow_n: glow[2],
            spot: [[0.0; 4]; 3],
            nav: self.nav_globals(),
            cockpit: self.cockpit_globals(agl),
            snow: if in_menu { [0.0; 4] } else { snow::globals(self.snow, clat.to_degrees(), self.doy, self.wx.freezing) },
            snow_c: snow_map[0],
            snow_e: snow_map[1],
            snow_n: snow_map[2],
            flakes: [0.0; 4],
            flakes_v: [0.0; 4],
            fresh: [0.0; 4],
            snow_tr_c: snow_tr[0],
            snow_tr_e: snow_tr[1],
            snow_tr_n: snow_tr[2],
            storm_bounds: [[0.0; 4]; weather::MAX_STORMS],
            storm_turrets: [[0.0; 4]; 8 * weather::MAX_STORMS],
            lite: [self.gfx_eff.water_detail(), if self.gfx_eff.clouds_on() { 1.0 } else { 0.0 }, 0.0, 0.0],
            det_mean: [[1.0 / 0.35, 1.0 / 0.35, 1.0 / 0.35, 1.0]; 16],
            det_hmean: [[0.5; 4]; 4],
            flood: [[0.0; 4]; 16],
        };
        if !in_menu {
            let cells = self.wx.storm_cells(cam);
            let storm_w = DVec3::new(g.storm[0] as f64, g.storm[1] as f64, g.storm[2] as f64);
            let storm_base = self.wx.base();
            let render_n = cells.len().min(self.gfx_eff.storm_max_cells());
            g.storm[3] = render_n as f32;
            self.storms_seen = render_n > 0 && self.gfx_eff.clouds_on();
            for (i, c) in cells.iter().take(render_n).enumerate() {
                g.storm_cells[2 * i] = c[0];
                g.storm_cells[2 * i + 1] = c[1];
                g.storm_bounds[i] = storm::bound(c, self.wx.cloud_off, storm_w, storm_base);
                let tur = storm::turrets(c, storm_base);
                g.storm_turrets[8 * i..8 * i + 8].copy_from_slice(&tur);
            }
            // The flight model flies through the same cells.
            self.wx.cells = cells;
            // The storms close to the camera (they darken the ground under them),
            // the lightning lighting the scene, and the rain falling around the camera.
            g.stormfx = [storm::near_mask(&self.wx, cam) as f32, self.cam_rain, self.lightning.globals(cam, dt, &mut g.flash) as f32, shade];
            let o = self.rain_field.off;
            let v = self.rain_field.rel;
            // Drops closer than this are inside the cockpit (or the camera's lens).
            let near_cut = if self.cam_mode == 1 && !self.crashed { 2.5 } else { 0.3 };
            // (Hail rides along in the intensity's whole part, in sixteenths: precip.wgsl.)
            let rain_w = self.cam_rain.min(0.999) + (self.cam_hail.clamp(0.0, 1.0) * 15.0).round();
            g.rain = [[o.x as f32, o.y as f32, o.z as f32, rain_w], [v.x as f32, v.y as f32, v.z as f32, near_cut as f32]];
            let (o, v) = (self.snow_field.off, self.snow_field.rel);
            g.flakes = [o.x as f32, o.y as f32, o.z as f32, self.cam_snow];
            g.flakes_v = [v.x as f32, v.y as f32, v.z as f32, self.cam_blow];
            g.fresh = [self.fresh_line, self.fresh.k as f32, 0.0, 0.0];
        }
        if !self.crashed && !in_menu {
            // The aircraft keeps pace with the camera, so its pixels are found
            // in last frame's image through where it was then, not through
            // where the ground under them was.
            let base = Mat4::from_rotation_translation(self.plane.rot.as_quat(), (self.plane.model_pos() - cam).as_vec3());
            g.taa[3] = 1.0;
            g.plane_inv = base.inverse().to_cols_array_2d();
            g.plane_prev = (Mat4::from_cols_array_2d(&self.prev_vp) * self.prev_plane).to_cols_array_2d();
            self.prev_plane = base;
        }
        self.prev_vp = vp.to_cols_array_2d();
        for (i, (p, r)) in self.world.craters.iter().take(8).enumerate() {
            g.crater[i] = v4(*p - cam, *r as f64);
        }
        let mut psim = PSim { anchor: v4(self.fx_anchor - cam, 0.0), upv: v4(self.fx_anchor.normalize(), dt), cam_r: v4(cam_right, self.time), cam_u: v4(cam_up, 0.0), wind: v4(self.wx.steady(self.fx_anchor, 10.0), 0.0), shift: v4(self.fx_shift, 0.0), ..Default::default() };
        if dt > 0.0 {
            // The simulation runs this frame and applies the shift.
            self.fx_shift = DVec3::ZERO;
        }
        let mut objs = Vec::new();
        if let Some(cr) = &self.crash {
            // The ground under an impact up the side of a building is further down.
            psim.anchor = v4(cr.site - cam, -cr.lift);
            psim.upv = v4(cr.up, dt);
            g.light_pos = v4(cr.site + cr.up * 8.0 * cr.size as f64 - cam, (12.0 + 8.0 * cr.power as f64) * cr.size as f64);
            let l = cr.light();
            g.light_col = [1.0 * l, 0.5 * l, 0.2 * l, 0.0];
            let grow = (cr.time as f32 / 1.2).min(1.0);
            for (i, (p, rad)) in cr.scorch.iter().take(8).enumerate() {
                g.scorch[i] = v4(cr.site + p.as_dvec3() - cam, (*rad * grow) as f64);
            }
            for (i, (p, rad, k, age, seed)) in cr.foam().into_iter().take(8).enumerate() {
                g.foam[2 * i] = v4(cr.site + p.as_dvec3() - cam, rad as f64);
                g.foam[2 * i + 1] = [k, age, seed, 0.0];
            }
            if self.crashed || cr.time < 240.0 {
                // (the airframe's dent stays on the pieces it breaks into: their meshes are in the body frame)
                let dent = (self.plane.dmg.crumple, self.plane.dmg.crumple_dir);
                for (i, d) in cr.debris.iter().enumerate() {
                    if d.sunk {
                        continue;
                    }
                    let m = Mat4::from_scale_rotation_translation(Vec3::splat(d.scale), d.rot.as_quat(), (d.pos - cam).as_vec3()) * Mat4::from_translation(-d.origin);
                    objs.push(DrawObj { mesh: d.mesh, model: m, tint: [1.0, 1.0, 1.0, d.char], glow: [if d.burn > 0.0 { if d.rigid { 0.35 } else { 1.0 } } else if cr.water { 0.0 } else { 0.25 }, self.spec.id as f32, 0.0, i as f32 * 0.37], cast: 0b11, cut: true, tear: [d.tear[0].gpu(), d.tear[1].gpu()], dmg: [d.tear[0].mode, d.tear[1].mode, 0.6, d.jag], crumple: if d.group.is_some() { dent.0 } else { [0.0; 4] }, crumple_dir: if d.group.is_some() { dent.1 } else { [0.0; 4] }, ..DrawObj::DEFAULT });
                }
            }
        }
        // The fire simulation: this frame's sources, and the light it gives off.
        let mut fluid = None;
        if let Some(cr) = self.crash.as_mut() {
            let wind = self.wx.steady(cr.site, 10.0);
            // Once the fire is out and its smoke has gone, the grid rests.
            let spent = cr.fluid.is_empty() && cr.time > 8.0 && self.r.fluid.latest.is_some_and(|st| st.soot < 2.0 && st.emit < 1.0);
            if let Some(dm) = cr.domain.as_mut().filter(|_| !spent) {
                fluid = Some(dm.frame(dt, cam, wind, sun, &cr.fluid, &self.world));
                if let Some(st) = self.r.fluid.latest {
                    // A ball about as big as the burning gas, as bright as flame.
                    let (t, p, _) = cr.last_blast;
                    let flash = p * 6.0 * (-(t as f32) / 0.25).exp();
                    let rad = (0.75 * st.emit.max(0.0).cbrt()).clamp(3.0, 25.0 * cr.size);
                    let l = flash + 2.5 * (st.emit / 300.0).min(1.0);
                    g.light_pos = v4(dm.at_frac(st.emit_c) - cam, rad as f64);
                    g.light_col = [1.0 * l, 0.48 * l, 0.17 * l, 0.0];
                }
            }
        }
        let params = self.instruments(agl);
        let mut own_lights = Vec::new();
        if !self.crashed && !in_menu {
            let base = Mat4::from_rotation_translation(self.plane.rot.as_quat(), (self.plane.model_pos() - cam).as_vec3());
            let pose = self.plane.pose(&self.model.pieces);
            // Blades while they can be followed by eye, a blur once they cannot.
            let blur = ((self.plane.prop_rate - 30.0) / 25.0).clamp(0.0, 1.0) as f32;
            let dm = self.plane.dmg;
            let kind = self.spec.id as f32;
            let rain_ph = self.glass.phases();
            let bend = self.plane.wing_bend();
            // What the exhaust shows (plane.wgsl, the plume and the nozzles): how far the
            // afterburners are lit, and how hard the engines are running dry (0 idle .. 1).
            let (burner, dry, nozzle_z) = match &self.spec.engine {
                aircraft::Engine::Fan(fe) => (self.plane.burner as f32, (((self.plane.n1 - fe.idle) / (1.0 - fe.idle)).clamp(0.0, 1.0) * if dm.engine > 0.0 { 1.0 } else { 0.0 }) as f32, fe.nozzle_z),
                // (A rocket: how hard the lit engines burn, and which stage's they are with how many of them, see plane.wgsl `rocket_plume`.)
                aircraft::Engine::Rocket(rk) => (self.plane.burner as f32, self.plane.plume_code(), rk.stages[self.plane.stage].z[1]),
                _ => (0.0, 0.0, 0.0),
            };
            // (A rocket's plume also wants to know how thin the air is: 0 at the sea, a half by 20 km, 1 in space; and its tiles how hot.)
            let rocket = self.spec.rocket().is_some();
            let thin = if rocket { 1.0 - (plane::density(geo::pos_to_lla(self.plane.pos).2) / plane::RHO0).min(1.0).powf(0.25) as f32 } else { 0.0 };
            let heat = if rocket { self.plane.heat() } else { 0.0 };
            if burner > 0.01 && self.crash.is_none() {
                // The flames light the tail from behind (turned down at night, as they are, for the eye that has opened up).
                let k = burner * (0.12 + 0.88 * day) * 2.2;
                g.light_pos = v4(self.plane.model_pos() + self.plane.rot * DVec3::new(0.0, 0.0, nozzle_z + 2.0) - cam, 1.3);
                g.light_col = [k, 0.5 * k, 0.2 * k, 0.0];
            }
            if let Some((at, power, t0)) = self.weapon_flash {
                let age = self.real - t0;
                if age < 0.45 {
                    let k = (1.0 - age / 0.45) as f32 * power * (0.25 + 0.75 * day);
                    g.light_pos = v4(at - cam, (6.0 * power as f64).max(2.0));
                    g.light_col = [2.8 * k, 1.1 * k, 0.35 * k, 0.0];
                } else {
                    self.weapon_flash = None;
                }
            }
            // from far outside the cabin interior cannot be seen, skip its vertices.
            let far_out = self.cam_mode != 1 && (cam - self.plane.model_pos()).length() > 40.0;
            for (i, p) in self.model.pieces.iter().enumerate() {
                if far_out && self.piece_inner[i] {
                    continue;
                }
                let hide = match p.vis {
                    airframe::Vis::Always => false,
                    airframe::Vis::SlowProp => blur >= 1.0,
                    airframe::Vis::FastProp => blur <= 0.0,
                    airframe::Vis::Chute => self.plane.spoilers < 0.02,
                };
                if hide || dm.hidden(p) || self.weapons.hides_piece(p.name) {
                    continue;
                }
                let tears = dm.tears_for(p);
                let torn = tears[0].is_some() || tears[1].is_some();
                objs.push(DrawObj {
                    mesh: self.piece_mesh[i],
                    model: base * pose[i],
                    // (Windows: the phases of the water on them, rain.rs Glass::phases; a
                    // rocket's parts: how thin the air is and how hot its tiles. A rocket
                    // has no windows: its "glass" is the exhaust plumes, which spread by
                    // that thinness; given the rain's ever-growing flow phase instead,
                    // they swelled into a white sheet across the whole view.)
                    tint: [blur, burner, dry, if p.glass && !rocket { rain_ph[2] } else { thin }],
                    glow: if p.glass && !rocket { [0.0, kind, rain_ph[0], rain_ph[1]] } else { [0.0, kind, heat, 0.0] },
                    params,
                    cast: 1 << render::PLANE_LAYER,
                    glass: p.glass,
                    cut: self.piece_cut[i] || torn,
                    tear: [tears[0].gpu(), tears[1].gpu()],
                    dmg: [tears[0].mode, tears[1].mode, dm.scuff[p.group], dm.jag_for(p)],
                    dmg2: [dm.oil, dm.crack[0], dm.crack[1], dm.crack[2]],
                    crumple: dm.crumple,
                    crumple_dir: dm.crumple_dir,
                    pose: pose_rows(&pose[i]),
                    flex: match p.group {
                        airframe::G_LWING => bend[0],
                        airframe::G_RWING => bend[1],
                        _ => 0.0,
                    },
                    ..DrawObj::DEFAULT
                });
            }
            // A buckled wing: the same pieces again past the break, swung about
            // the torn skin they hang from.
            for (side, f) in dm.fold.iter().enumerate() {
                if !f.on {
                    continue;
                }
                let g = if side == 0 { airframe::G_LWING } else { airframe::G_RWING };
                let swing = base * f.mat();
                for (i, p) in self.model.pieces.iter().enumerate() {
                    let structure = matches!(p.role, airframe::Role::WingStructure(_) | airframe::Role::TailStructure);
                    if p.group != g || p.glass || (dm.hidden(p) && !structure) {
                        continue;
                    }
                    let stick = if structure { 0.12 * self.spec.span / 5.5 } else { 0.0 };
                    let tears = [f.tear.flip().grow(stick), f.outer.grow(stick)];
                    objs.push(DrawObj {
                        mesh: self.piece_mesh[i],
                        model: swing * pose[i],
                        glow: [0.0, kind, 0.0, 0.0],
                        params,
                        cast: 1 << render::PLANE_LAYER,
                        cut: true,
                        tear: [tears[0].gpu(), tears[1].gpu()],
                        dmg: [tears[0].mode, tears[1].mode, dm.scuff[p.group].max(0.5), f.tear.jag.max(0.03)],
                        ..DrawObj::DEFAULT
                    });
                }
            }
            // Missiles in flight: the same wingtip piece, now on its own.
            for m in &self.weapons.missiles {
                let Some(st) = self.weapons.station(m.station) else { continue };
                let Some(pi) = st.piece else { continue };
                let local = self.model.pieces[pi].pivot.as_dvec3();
                let model = Mat4::from_rotation_translation(m.rot.as_quat(), (m.pos - cam).as_vec3()) * Mat4::from_translation((-local).as_vec3());
                objs.push(DrawObj { mesh: self.piece_mesh[pi], model, tint: [0.0; 4], glow: [0.0, kind, 0.0, 0.0], params, cast: 0, ..DrawObj::DEFAULT });
            }
            self.own_lights(&base, &pose, params[5], &mut g, &mut own_lights);
        }
        if !in_menu {
            self.sheds.draw(cam, params, &mut objs);
            // A booster just let go: its centre engines' flame dying away behind it.
            let ix = |n: &str| self.model.pieces.iter().position(|p| p.name == n);
            if let (true, Some(body), Some(plume)) = (self.plane.stage > 0, ix("booster body"), ix("booster plume")) {
                if let Some((model, age)) = self.sheds.find(self.piece_mesh[body], cam).filter(|s| s.1 < 2.5) {
                    let thin = 1.0 - (plane::density(geo::pos_to_lla(self.plane.pos).2) / plane::RHO0).min(1.0).powf(0.25) as f32;
                    objs.push(DrawObj { mesh: self.piece_mesh[plume], model, tint: [0.0, 0.7 * (1.0 - age / 2.5), 0.9 * 3.0 / 33.0, thin], glow: [0.0, self.spec.id as f32, 0.0, 0.0], params, glass: true, ..DrawObj::DEFAULT });
                }
            }
        }
        crate::gputime::cpu("frame_constants+aircraft_objects");
        let mut insts = Vec::new();
        let mut scenery = Vec::new();
        let mut fall = Vec::new();
        let mut air_lights = own_lights;
        if !in_menu {
            let local = (self.utc + clon.to_degrees() / 15.0).rem_euclid(24.0);
            let sun_up = geo::enu(cam).2.dot(self.sun_dir());
            self.world.car_instances(cam, cars::activity(local as f32), sun_up, &mut insts, &mut air_lights);
            self.n_cars = insts.len();
            self.destr.instances(cam, &mut insts, &mut fall);
            self.destr.skin_frame(cam);
            self.destr.upload_skins(&self.r);
            self.world.airport_instances(cam, (self.wx.wind_from as f32, (self.wx.wind * 1.94384) as f32), self.time, &mut insts, &mut scenery, &mut air_lights);
            let world = &self.world;
            let before = insts.len();
            self.birds.instances(cam, 2.0 * (fov as f64 * 0.5).tan() / h as f64, &|rel, r| world.in_view(rel, r), &mut insts);
            self.bird_insts = insts.len() - before;
            self.n_scenery = scenery.len();
            // The apron floodlights are on from dusk to dawn.
            let on = 1.0 - ((sun_up as f32 + 0.08) / 0.18).clamp(0.0, 1.0);
            if on > 0.0 {
                self.world.flood_globals(cam, on, &mut g.flood);
            }
            self.world.photo_beacons(cam, &mut air_lights);
        }
        crate::gputime::cpu("instances(cars,airport,birds)");
        self.live.step(&self.src, (!in_menu).then_some(self.plane.pos));
        if !in_menu {
            // A downed aircraft that has reached the ground is gone (see
            // `step_kills`); one that outlived its kill entry goes here.
            let mut gone = Vec::new();
            for p in self.live.planes.iter().filter(|p| p.downed > 0.0) {
                let (la, lo, h) = geo::pos_to_lla(p.pos);
                let tracked = self.kills.iter().any(|k| matches!(&k.key, weapons::TargetKey::Live(x) if *x == p.hex));
                if !tracked && (h < 0.0 || self.world.ground(la, lo).is_some_and(|g| h < g.h + 2.0)) {
                    gone.push(p.hex.clone());
                }
            }
            for hex in gone {
                self.live.remove(&hex);
            }
        }
        let wx_at = if in_menu {
            self.menu.pick
        } else {
            let (la, lo, _) = geo::pos_to_lla(self.plane.pos);
            Some((la.to_degrees(), lo.to_degrees()))
        };
        self.livewx.step(&self.src, &mut self.wx, wx_at, if in_menu { 0.0 } else { dt_real });
        if let Some(px) = self.gclouds.step(&self.src, in_menu) {
            self.r.upload_globe_clouds(&px);
        }
        if !in_menu {
            self.live.instances(cam, &mut insts, &mut air_lights);
        }
        self.update_mp(in_menu);
        if !in_menu {
            let full = self.draw_mp_planes(cam, dt_real, &mut objs);
            mp::instances(&self.mp_players, &full, cam, &mut insts, &mut air_lights);
        }
        self.live_view = Some((vp, cam));

        // The globe's labels are placed in the window's pixels, not the (maybe smaller) 3D scene's.
        let (ww, wh) = (self.r.config.width as f32, self.r.config.height as f32);
        crate::gputime::cpu("live_traffic+weather_fetch");
        let ui = if in_menu { self.menu_ui(vp, cam, ww, wh) } else { self.ui(agl) };
        crate::gputime::cpu("ui.build(egui)");
        // The smoke volume: a grid around the effects anchor, on its local east/north/up axes.
        {
            let site = cam + DVec3::new(psim.anchor[0] as f64, psim.anchor[1] as f64, psim.anchor[2] as f64);
            let (ve, vn, vu) = geo::enu(site);
            // (coarser round a big fire, as its grid is: see fluid.rs `coarse`)
            let vs = self.crash.as_ref().and_then(|c| c.domain.as_ref()).map(|d| d.sc as f64).unwrap_or(1.0);
            let (cs, zmin) = (render::VOL_CELL as f64 * vs, render::VOL_ZMIN as f64 * vs);
            let lift = self.crash.as_ref().map(|c| c.lift).unwrap_or(0.0);
            let corner = site - ve * (cs * render::VOL_N[0] as f64 * 0.5) - vn * (cs * render::VOL_N[1] as f64 * 0.5) + vu * (zmin - lift);
            g.vol_o = v4(corner - cam, cs);
            g.vol_u = v4(vu, 0.0);
            g.vol_e = v4(ve, 0.0);
            g.vol_n = v4(vn, 0.0);
            psim.ve = v4(ve, cs);
            psim.vn = v4(vn, zmin - lift);
        }
        let pf = self.world.photo_frame(cam);
        let photo = self.world.photo_draws(cam);
        g.photo_vp = pf.mask_vp.to_cols_array_2d();
        g.photo = pf.zone;
        g.photo_c = pf.centre;
        g.photo_sun = pf.sun;
        g.cut = pf.cut;
        g.cut_v = pf.cut_v;
        casc_vp[render::MASK_SLOT] = g.photo_vp;
        crate::gputime::cpu("photo3d.draw_lists");
        let (tiles, tess, details, near) = self.world.draw_lists(cam, focus_abs, CASC, &shadow_vp);
        crate::gputime::cpu("world.draw_lists(culling)");
        let cover = self.world.light_cover(cam);
        // The shore distance field: a step of its rebuild this frame.
        let sf = self.shore.frame(cam);
        casc_vp[render::SHORE_SLOT] = sf.build_vp;
        casc_vp[render::SHORE_WIDE_SLOT] = sf.build_vp_wide;
        g.shore_vp = sf.vp;
        g.shore = [if sf.on { 1.0 } else { 0.0 }, shore::TEXEL as f32, shore::MAX_D, 2.0 * shore::HALF as f32];
        let shore_tiles = if matches!(sf.step, Some(shore::Step::Raster)) { self.world.shore_tiles(cam, sf.build_c) } else { Vec::new() };
        let shore_c = sf.build_c.as_vec3().to_array();
        let fields = if in_menu { Vec::new() } else { self.world.field_draws(cam) };
        let mut shot = match self.shots.first() {
            Some((t, _, _)) if self.real >= *t => {
                // Logged with each scripted shot, for comparing builds.
                log::info!("fps {:.1}  rain {:.2} (glass {:.2}, wet {:.2}, hail {:.2})  sigma {:.2} draft {:.1}  birds {} in {} flocks ({} drawn, nearest {:.0} m)  scenery {}", self.fps, self.cam_rain, self.wx.rain, self.glass.wet, self.cam_hail, self.plane.air.sigma_w, self.plane.air.draft, self.birds.count(), self.birds.flocks.len(), self.bird_insts, self.birds.nearest(self.prev_cam), scenery.len());
                Some(self.shots.remove(0).1)
            }
            _ => None,
        };
        let mut rec_done = false;
        if let Some((dir, start, secs)) = &self.record {
            // The synth runs every frame so it is settled when recording starts.
            let n = (audio::Audio::RATE as f64 / REC_FPS).round() as usize;
            let samples = self.audio.as_ref().map(|a| a.take(n)).unwrap_or_default();
            if self.real >= *start {
                if shot.is_none() {
                    let _ = std::fs::create_dir_all(dir);
                    shot = Some(dir.join(format!("f{:05}.png", self.rec_n)));
                    self.rec_n += 1;
                }
                self.rec_audio.extend(samples);
                if self.real >= start + secs {
                    write_wav(&dir.join("audio.wav"), &self.rec_audio, audio::Audio::RATE);
                    rec_done = true;
                }
            }
        }
        // Scripted shots can pre-position the chase camera for the next capture.
        if let Some((_, _, Some((y, p, d)))) = self.shots.first() {
            (self.cam_yaw, self.cam_pitch, self.cam_dist) = (y.to_radians(), *p, *d);
        }
        let had_shots = shot.is_some() && self.shots.is_empty();
        let (skin_slots, skins) = if in_menu { (Vec::new(), Vec::new()) } else { self.destr.skin_draws() };
        self.r.stats[0] += now.elapsed().as_secs_f64();
        let mut bolts = Vec::new();
        if !in_menu {
            self.lightning.segments(cam, dt, &mut bolts);
        }
        if !in_menu && self.gfx.contrails {
            let apt_air: Vec<(u32, u32, u8, DVec3, DVec3)> = self
                .world
                .air_targets(cam)
                .into_iter()
                .filter_map(|t| match t.key {
                    weapons::TargetKey::Airport { apt, mover } => Some((apt, mover, t.size, t.pos, t.vel)),
                    _ => None,
                })
                .collect();
            self.contrails.step(dt, cam, &self.wx, self.doy, Some((&self.plane, !self.crashed)), &self.live.planes, &apt_air, &self.mp_players);
            self.contrails.build_draw(cam, vp, &mut self.contrail_draw);
        } else {
            if !self.gfx.contrails && self.contrails.trail_count() > 0 {
                self.contrails.clear();
            }
            self.contrail_draw.nodes.clear();
            self.contrail_draw.segs.clear();
        }
        crate::gputime::cpu("sim.contrails");
        let rain_drops = if in_menu || (self.cam_rain < 0.002 && self.cam_blow < 0.01) { 0 } else { render::RAIN_DROPS };
        crate::gputime::cpu("shore+fields+misc");
        self.r.render(Frame { rain_drops, bolts, contrails: &self.contrail_draw, g, tiles, tess, details, photo, skins, skin_slots, fields, lights: air_lights, objs, insts, scenery, fall, near, casc_vp, casc_skip, shore: sf.step, shore_c, shore_tiles, psim, sim_steps: dt > 0.0, fluid, ocean: self.world.water_in_view().then(|| self.world.ocean.uniform()), egui: Some(ui), shot, cover, glow: glow_px, snow: snow_px, snow_tracks: snow_tr_uploads });
        self.finish_mcp_shots();
        if (had_shots && std::env::args().any(|a| a == "--shot") && self.record.is_none()) || rec_done {
            self.quit = true;
        }
    }
}
