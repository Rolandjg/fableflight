//! Birds, weapons, explosions and wrecks: strikes, blasts, kills and what falls to the ground.

use crate::*;

impl Game {
    /// Particles live in a frame anchored near the action; move it if the action has moved on.
    /// Advances the birds (birds.rs) around the camera; returns their strikes.
    pub(crate) fn step_birds(&mut self, dt: f64) -> Vec<birds::Strike> {
        let level = self.birds_level.unwrap_or(self.gfx_eff.birds);
        let sun = self.sun_dir();
        let (utc, doy, rain, conv) = (self.utc, self.doy, self.wx.rain as f64, self.wx.convection());
        let world = &self.world;
        let wx = &self.wx;
        let ground = |p: DVec3| {
            let (la, lo, _) = geo::pos_to_lla(p);
            world.ground(la, lo).map(|g| g.h)
        };
        let place = |p: DVec3| {
            let (la, lo, _) = geo::pos_to_lla(p);
            world.ground(la, lo)?;
            // The share of water round about: the point and eight more 800 m out.
            let (e, n, _) = geo::enu(p);
            let mut wet = 0;
            let mut seen = 0;
            for k in 0..9 {
                let a = k as f64 * std::f64::consts::TAU / 8.0;
                let q = if k == 8 { p } else { p + (e * a.sin() + n * a.cos()) * 800.0 };
                let (qa, qo, _) = geo::pos_to_lla(q);
                if let Some(g) = world.ground(qa, qo) {
                    seen += 1;
                    wet += g.water as i32;
                }
            }
            Some(birds::Place {
                lat: la.to_degrees(),
                doy,
                hour: (utc + lo.to_degrees() / 15.0).rem_euclid(24.0),
                sun: p.normalize().dot(sun),
                water: if seen > 0 { wet as f64 / seen as f64 } else { 0.0 },
                convection: conv,
                rain,
            })
        };
        let t = self.time;
        let thermal = |p: DVec3, agl: f64| wx.thermal(p, agl, t);
        let wind = |p: DVec3, agl: f64| wx.steady(p, agl);
        let env = birds::Env { place: &place, ground: &ground, thermal: &thermal, wind: &wind };
        // (The aircraft's shape for the strike test, made once per type.)
        if self.bird_shape.0 != self.spec.keys[0] {
            self.bird_shape = (self.spec.keys[0], birds::Shape::of(self.spec));
        }
        let plane = birds::Aircraft {
            pos: self.plane.pos,
            vel: self.plane.vel,
            rot: self.plane.rot,
            shape: &self.bird_shape.1,
            flying: !self.crashed,
        };
        // (Last frame's camera: this frame's is placed after the physics.)
        self.birds.step(dt, self.prev_cam, level, &plane, &env)
    }

    /// A bird hit the aircraft: a thud, a burst of feathers, a note. No damage.
    pub(crate) fn bird_strike(&mut self, s: birds::Strike, femit: &mut Vec<(DVec3, render::Particle)>) {
        // (Where it met the aircraft's box, back in the planet frame.)
        let at = self.plane.pos + self.plane.rot * s.at.as_dvec3();
        log::info!("bird strike: a {} at {:.0} m/s, body frame {:.1?}", s.kind.spec().name, s.speed, s.at);
        self.anchor_fx(at);
        let sp = s.kind.spec();
        // Pale or dark feathers, as the bird was.
        let shade = match s.kind {
            birds::Kind::Gull => 0.85,
            birds::Kind::Goose => 0.45,
            birds::Kind::Buzzard => 0.35,
            _ => 0.06,
        };
        let n = (12.0 + 40.0 * sp.mass.sqrt()) as usize;
        let v = self.plane.vel.as_vec3();
        for _ in 0..n {
            let r = &mut self.rng;
            femit.push((at, render::Particle { vel: (v * r.range(0.2, 0.7) + r.dir() * r.range(1.0, 5.0)).into(), life: r.range(4.0, 9.0), size0: r.range(0.03, 0.08) * sp.span.sqrt() as f32, size1: r.range(0.04, 0.1) * sp.span.sqrt() as f32, kind: crash::FEATHER, drag: r.range(1.5, 3.0), seed: r.f(), temp: shade * r.range(0.8, 1.1), alpha: 1.0, ..Default::default() }));
        }
        // A puff of down, carried off in the slipstream: what the eye catches.
        for _ in 0..3 {
            let r = &mut self.rng;
            let k = sp.span.sqrt() as f32;
            femit.push((at, render::Particle { vel: (v * r.range(0.3, 0.6) + r.dir() * 2.0).into(), life: r.range(0.5, 0.9), size0: 0.4 * k, size1: r.range(1.5, 2.5) * k, kind: crash::MIST, drag: 2.5, seed: r.f(), alpha: 0.35, ..Default::default() }));
        }
        if let Some(a) = &self.audio {
            a.event(audio::Event::Bump(birds::thud(&s)));
        }
        self.flash.push((t!("game.bird_strike").to_string(), self.real));
    }

    pub(crate) fn anchor_fx(&mut self, at: DVec3) {
        let stale = self.crash.as_ref().map(|c| c.time > 90.0).unwrap_or(true);
        if stale && (self.fx_anchor - at).length() > 800.0 {
            // Live particles are carried over into the new frame.
            self.fx_shift += self.fx_anchor - at;
            self.crash = None;
            self.fx_anchor = at;
        }
    }

    /// A weapon-system event: status messages, the launch sound, and a
    /// missile's detonation in the particle system.
    pub(crate) fn weapon_event(&mut self, ev: weapons::Event, femit: &mut Vec<(DVec3, render::Particle)>) {
        use weapons::Refusal;
        match ev {
            weapons::Event::Locked => {
                self.status = t!("game.target_locked").into();
                self.status_until = self.real + 2.0;
            }
            weapons::Event::Lost => {}
            weapons::Event::Fired { station } => {
                if let Some(a) = &self.audio {
                    a.event(audio::Event::Missile(1.0));
                }
                let name = self.weapons.station(station).map_or("AIM-9", |s| s.spec.name);
                self.flash.push((t!("game.missile_away", name), self.real));
            }
            weapons::Event::Refused(why) => {
                let msg = match why {
                    Refusal::NotArmed => t!("game.master_arm_off_2"),
                    Refusal::NoPower => t!("game.weapons_no_power"),
                    Refusal::NoLock => t!("game.no_lock"),
                    Refusal::NoMissiles => t!("game.no_missiles"),
                    Refusal::NoWeapons => t!("game.no_weapons_on_this_aircraft"),
                    Refusal::NoTarget => t!("game.no_target"),
                };
                self.flash.push((msg.into(), self.real));
            }
            weapons::Event::Blast { pos, power, size, target } => {
                let near = 1.0 / (1.0 + ((self.cam_pos - pos).length() as f32 / 500.0).powi(2));
                if let Some(a) = &self.audio {
                    a.boom(power * (0.2 + 0.8 * near));
                }
                // The flash lights the aircraft for a moment and, if the hit
                // is close, gives the airframe a jolt.
                self.weapon_flash = Some((pos, power, self.real));
                if (self.cam_pos - pos).length() < 800.0 {
                    self.plane.dmg.vibe = self.plane.dmg.vibe.max(power * 1.5);
                }
                self.anchor_fx(pos);
                self.blast_fx(pos, power, size, femit);
                if let Some(key) = target {
                    let label = self.weapons.target_view.as_ref().map(|v| v.label.clone()).unwrap_or_default();
                    // Where it was and how it was moving, before it leaves the traffic.
                    let was = match &key {
                        weapons::TargetKey::Live(hex) => self.live.planes.iter().find(|p| &p.hex == hex).map(|p| (p.pos, p.vel)),
                        weapons::TargetKey::Airport { .. } => self.world.air_targets(pos).into_iter().find(|t| t.key == key).map(|t| (t.pos, t.vel)),
                    };
                    let hit = match &key {
                        weapons::TargetKey::Live(hex) => self.live.down(hex),
                        weapons::TargetKey::Airport { .. } => self.world.down_air_target(&key),
                    };
                    if hit {
                        let (at, vel) = was.unwrap_or((pos, DVec3::ZERO));
                        let span = airmodels::TYPES[(size as usize).min(4)].span;
                        self.kill_fx(at, vel, span, femit);
                        if let Some(a) = &self.audio {
                            a.boom((power * 1.5 * (span / 20.0).sqrt()).min(4.0) * (0.2 + 0.8 * near));
                        }
                        self.weapon_flash = Some((at, power * (span / 11.0).sqrt() * 2.0, self.real));
                        self.kills.push(Kill { key, pos: at, vel, span, t: 0.0, acc: 0.0 });
                        let msg = if label.is_empty() { t!("game.splash").to_string() } else { t!("game.splash_2", label) };
                        self.flash.push((msg, self.real));
                    }
                }
            }
        }
    }

    /// The fireball and smoke of a missile going off (or a target being hit).
    pub(crate) fn blast_fx(&mut self, at: DVec3, power: f32, size: f32, femit: &mut Vec<(DVec3, render::Particle)>) {
        let up = at.normalize();
        let p = power.max(0.3);
        let k = (0.7 + 0.15 * size).min(1.5);
        let r = &mut self.rng;
        femit.push((at, render::Particle { life: 0.3, size0: 12.0 * p * k, size1: 52.0 * p * k, kind: crash::FLASH, alpha: 2.0 * p, ..Default::default() }));
        // The fireball: overlapping hot blobs, thrown on with the wreckage.
        for _ in 0..30 {
            let d = r.dir();
            femit.push((at, render::Particle {
                vel: (d * r.range(14.0, 50.0) * p.sqrt() + up.as_vec3() * r.range(2.0, 11.0)).into(),
                life: r.range(1.8, 4.0),
                size0: r.range(2.5, 5.5) * p * k,
                size1: r.range(10.0, 24.0) * p * k,
                temp: r.range(2.2, 3.2),
                kind: crash::GAS,
                drag: r.range(1.2, 2.0),
                buoy: r.range(6.0, 12.0),
                seed: r.f(),
                alpha: r.range(0.75, 1.0),
                ..Default::default()
            }));
        }
        // Burning fragments and sparks tearing out through the fireball.
        for _ in 0..40 {
            let d = (r.dir() + up.as_vec3() * 0.6).normalize_or_zero();
            femit.push((at, render::Particle {
                vel: (d * r.range(20.0, 85.0) * p.sqrt()).into(),
                life: r.range(0.6, 2.2),
                size0: r.range(0.06, 0.16),
                size1: 0.03,
                kind: crash::SPARK,
                drag: r.range(0.3, 0.9),
                seed: r.f(),
                alpha: r.range(0.6, 1.0),
                ..Default::default()
            }));
        }
        // The smoke column left behind, and the fire under it, lingering.
        for _ in 0..20 {
            let d = r.dir();
            femit.push((at, render::Particle {
                vel: (d * r.range(3.0, 14.0) * p.sqrt() + up.as_vec3() * r.range(1.0, 6.0)).into(),
                life: r.range(6.0, 12.0),
                size0: r.range(3.0, 6.0) * p * k,
                size1: r.range(14.0, 32.0) * p * k,
                kind: crash::GAS,
                drag: r.range(0.7, 1.3),
                buoy: r.range(2.0, 6.0),
                seed: r.f(),
                alpha: r.range(0.35, 0.6),
                ..Default::default()
            }));
        }
    }

    /// An aircraft blowing up in the air: its fuel going up in one fireball
    /// sized to the airframe (`span`, m), with burning wreckage thrown out
    /// along its line of flight and a pall of black smoke left hanging.
    pub(crate) fn kill_fx(&mut self, at: DVec3, vel: DVec3, span: f32, femit: &mut Vec<(DVec3, render::Particle)>) {
        self.anchor_fx(at);
        let up = at.normalize().as_vec3();
        // Linear size from a light single (11 m span) up to a wide-body.
        let s = (span / 11.0).clamp(0.8, 6.0);
        let carry = vel.as_vec3() * 0.35;
        let r = &mut self.rng;
        femit.push((at, render::Particle { life: 0.45, size0: 18.0 * s, size1: 70.0 * s, kind: crash::FLASH, alpha: 2.5, ..Default::default() }));
        let n = (40.0 + 14.0 * s) as usize;
        for _ in 0..n {
            let d = r.dir();
            let off = d * r.range(0.0, 0.4) * span;
            femit.push((at + off.as_dvec3(), render::Particle {
                vel: (carry + d * r.range(10.0, 32.0) * s.sqrt() + up * r.range(2.0, 8.0)).into(),
                life: r.range(2.5, 5.0) * s.sqrt(),
                size0: r.range(3.0, 6.0) * s,
                size1: r.range(12.0, 26.0) * s,
                temp: r.range(2.4, 3.4),
                kind: crash::GAS,
                drag: r.range(1.0, 1.8),
                buoy: r.range(5.0, 10.0),
                seed: r.f(),
                alpha: r.range(0.8, 1.0),
                ..Default::default()
            }));
        }
        for _ in 0..(50.0 + 20.0 * s) as usize {
            let d = r.dir();
            femit.push((at, render::Particle {
                vel: (vel.as_vec3() * 0.6 + d * r.range(25.0, 110.0) * s.sqrt()).into(),
                life: r.range(1.5, 4.5),
                size0: r.range(0.1, 0.3) * s.sqrt(),
                size1: 0.05,
                kind: crash::SPARK,
                drag: r.range(0.1, 0.5),
                seed: r.f(),
                alpha: r.range(0.7, 1.0),
                ..Default::default()
            }));
        }
        for _ in 0..(20.0 + 8.0 * s) as usize {
            let d = r.dir();
            femit.push((at + (d * r.range(0.0, 0.5) * span).as_dvec3(), render::Particle {
                vel: (carry * 0.4 + d * r.range(2.0, 9.0) * s.sqrt() + up * r.range(0.5, 3.0)).into(),
                life: r.range(10.0, 20.0),
                size0: r.range(4.0, 8.0) * s,
                size1: r.range(20.0, 40.0) * s,
                temp: 0.05,
                kind: crash::GAS,
                drag: r.range(0.8, 1.4),
                buoy: r.range(1.0, 3.0),
                seed: r.f(),
                alpha: r.range(0.45, 0.7),
                ..Default::default()
            }));
        }
    }

    /// Shot-down aircraft on their way down: a fire and smoke trail behind
    /// each, and a second explosion where it hits the ground (or the sea).
    pub(crate) fn step_kills(&mut self, dt: f64, femit: &mut Vec<(DVec3, render::Particle)>) {
        let mut kills = std::mem::take(&mut self.kills);
        kills.retain_mut(|k| {
            k.t += dt;
            match &k.key {
                weapons::TargetKey::Live(hex) => match self.live.planes.iter().find(|p| &p.hex == hex) {
                    // The live feed tumbles it down itself.
                    Some(p) => {
                        k.pos = p.pos;
                        k.vel = p.vel;
                    }
                    None => return false,
                },
                _ => {
                    let down = k.pos.normalize();
                    k.vel = (k.vel - down * 9.81 * dt) * (-dt * 0.15).exp();
                    k.pos += k.vel * dt;
                }
            }
            let (la, lo, h) = geo::pos_to_lla(k.pos);
            let ground = if h < 0.0 { Some(0.0) } else { self.world.ground(la, lo).map(|g| g.h).filter(|g| h < g + 2.0) };
            if let Some(g) = ground {
                let at = geo::lla_to_pos(la, lo, g.max(0.0) + 2.0);
                self.kill_fx(at, DVec3::ZERO, k.span * 0.8, femit);
                if let Some(a) = &self.audio {
                    let near = 1.0 / (1.0 + ((self.cam_pos - at).length() as f32 / 500.0).powi(2));
                    a.boom((1.5 * (k.span / 20.0).sqrt()).min(3.0) * (0.2 + 0.8 * near));
                }
                self.weapon_flash = Some((at, 2.0 * (k.span / 11.0).sqrt(), self.real));
                if let weapons::TargetKey::Live(hex) = &k.key {
                    self.live.remove(hex);
                }
                return false;
            }
            if k.t > 60.0 {
                return false;
            }
            // The trail: a burning fuel fire close behind, black smoke after.
            let s = (k.span / 11.0).clamp(0.8, 6.0);
            let burn = (1.0 - k.t as f32 / 50.0).max(0.3);
            k.acc += dt * 30.0;
            self.anchor_fx(k.pos);
            let r = &mut self.rng;
            while k.acc >= 1.0 {
                k.acc -= 1.0;
                let back = k.pos - k.vel * dt * r.f() as f64;
                femit.push((back, render::Particle {
                    vel: (k.vel.as_vec3() * 0.15 + r.dir() * 2.0 * s.sqrt()).into(),
                    life: r.range(0.6, 1.4),
                    size0: r.range(1.5, 3.0) * s * burn,
                    size1: r.range(4.0, 8.0) * s * burn,
                    temp: r.range(2.0, 3.0),
                    kind: crash::GAS,
                    drag: 2.0,
                    buoy: 4.0,
                    seed: r.f(),
                    alpha: 0.9,
                    ..Default::default()
                }));
                femit.push((back, render::Particle {
                    vel: (k.vel.as_vec3() * 0.05 + r.dir() * 1.5).into(),
                    life: r.range(8.0, 16.0),
                    size0: r.range(2.0, 4.0) * s,
                    size1: r.range(10.0, 22.0) * s,
                    temp: 0.05,
                    kind: crash::GAS,
                    drag: 1.5,
                    buoy: r.range(0.5, 1.5),
                    seed: r.f(),
                    alpha: r.range(0.4, 0.6),
                    ..Default::default()
                }));
            }
            true
        });
        // Anything that went down this step was added while we held the list.
        kills.append(&mut self.kills);
        self.kills = kills;
    }

    /// Knock a building down, with any parts standing on it.
    /// `hit`: the aircraft itself went in (the wall shows its outline).
    /// `energy`: kinetic energy (J) of what hit it. A building only comes down
    /// as a whole when that is enough for its size (see `destruct::resistance`);
    /// less breaks out the part round the impact, and a mere fraction of it
    /// (a blast from the next lot, a lump of rubble) leaves a building alone.
    pub(crate) fn bring_down(&mut self, tile: geo::TileId, idx: usize, at: DVec3, vel: DVec3, energy: f64, hit: Option<destruct::Hitter>, femit: &mut Vec<(DVec3, render::Particle)>) {
        if hit.is_none() {
            if let Some((base, top, area)) = self.world.footprint(tile, idx) {
                let h = (top - base).max(3.0);
                if energy < 0.03 * destruct::resistance(h as f32, (area.max(25.0) as f64 * h) as f32) {
                    return;
                }
            }
        }
        let above = self.world.structure_of(tile, idx);
        let mut parts = Vec::new();
        for (t, i) in std::iter::once((tile, idx)).chain(above) {
            if let Some(w) = self.world.destroy_building(&self.r.queue, t, i) {
                parts.push(w);
            }
        }
        if parts.is_empty() {
            return;
        }
        // Settings › Always topple buildings: the building the aircraft itself
        // went into comes down as a whole, whatever it would take. (Four times
        // its resistance: past three a tower also gives way after the shortest delay.)
        let energy = if hit.is_some() && self.topple_buildings {
            let base = parts.iter().map(|w| w.base).fold(f64::MAX, f64::min);
            let top = parts.iter().map(|w| w.top).fold(f64::MIN, f64::max);
            let h = (top - base).max(1.0);
            let vol: f64 = parts.iter().map(|w| destruct::ring_area_m2(&w.ring) * (w.top - w.base).max(1.0)).sum();
            energy.max(4.0 * destruct::resistance(h as f32, vol as f32))
        } else {
            energy
        };
        // Under the 3D tiles the building is the mesh's: it is cut out of the
        // mesh, and its own walls and roof come down on the pieces, which
        // stand as high as it did.
        let rings = self.world.cut_rings(&parts, 2.0);
        let base = parts.iter().map(|w| w.base).fold(f64::MAX, f64::min);
        let photo = self.world.photo_mut().take_building(&rings, base);
        if let Some(src) = &photo {
            for (w, top) in parts.iter_mut().zip(&src.tops) {
                if let Some(t) = *top {
                    w.top = t.max(w.base + 3.0);
                    let top = w.top as f32;
                    w.tiers.retain(|z| z.1 < top - 1.0);
                }
            }
            log::info!("3D tiles: building cut out, {} tiles, {} triangles", src.tiles.len(), src.tiles.iter().map(|t| t.tris.len()).sum::<usize>());
        }
        self.destr.collapse(&parts, at, vel, energy, hit, photo.as_ref(), femit);
    }

    /// The wreck is made of what was left of the aircraft: every group's mesh
    /// is rebuilt without the pieces that had already gone, and cut along its
    /// breaks. Returns the break each group keeps showing.
    pub(crate) fn wreck_meshes(&mut self) -> Vec<[Tear; 2]> {
        let dm = self.plane.dmg;
        let mut out = vec![[Tear::NONE; 2]; airframe::N_GROUPS];
        for g in 0..airframe::N_GROUPS {
            let pieces: Vec<&airframe::Piece> = self.model.pieces.iter().filter(|p| p.group == g && !p.glass).collect();
            let changed = pieces.iter().any(|p| dm.hidden(p) || dm.tears_for(p).iter().any(|t| t.is_some()));
            if !changed {
                continue;
            }
            self.wreck_dirty = true;
            let mut m = airframe::Mesh::default();
            for p in &pieces {
                if dm.hidden(p) {
                    continue;
                }
                let tears = dm.tears_for(p);
                let primary = matches!(p.name, "left wing" | "right wing" | "stabiliser" | "blades");
                if primary {
                    out[g] = tears;
                }
                let base = m.v.len() as u32;
                m.v.extend_from_slice(&p.mesh.v);
                for t in p.mesh.i.chunks(3) {
                    let gone = t.iter().all(|&i| { let q = Vec3::from(p.mesh.v[i as usize].pos); tears.iter().any(|tr| !tr.keeps(q)) });
                    if !gone {
                        m.i.extend(t.iter().map(|i| i + base));
                    }
                }
            }
            if m.i.is_empty() {
                m = self.model.groups[g].clone();
            }
            self.r.replace_mesh(self.group_mesh[g], &m.v, &m.i);
        }
        out
    }

    pub(crate) fn restore_wreck_meshes(&mut self) {
        if self.wreck_dirty {
            for g in 0..airframe::N_GROUPS {
                let m = &self.model.groups[g];
                self.r.replace_mesh(self.group_mesh[g], &m.v, &m.i);
            }
            self.wreck_dirty = false;
        }
    }
}
