//! Multiplayer: sending our aircraft, drawing the others, the relays.

use crate::*;

impl Game {
    /// Multiplayer, once a frame: connected while it is switched on (to the
    /// relay and under the name in the settings). In flight our aircraft is
    /// sent and the others near us taken in; in the menus nothing of ours
    /// goes out and the whole network's players come in for the globe.
    pub(crate) fn update_mp(&mut self, in_menu: bool) {
        if self.mp_mirror {
            self.mp_players = if in_menu { Vec::new() } else { vec![self.mirror_player()] };
            return;
        }
        // "auto": the server that answers fastest, kept once chosen.
        let auto = self.mp_auto();
        let target = if !self.mp_on {
            None
        } else if auto {
            match &self.mp {
                Some(m) if m.auto => Some(m.server.clone()),
                _ => self.mp_directory().best().map(|s| s.address),
            }
        } else {
            Some(self.mp_server.trim().to_string()).filter(|s| !s.is_empty())
        };
        let stale = self.mp.as_ref().is_some_and(|m| Some(&m.server) != target.as_ref() || m.auto != auto || m.name != mp_proto::clean_name(&self.mp_name));
        if target.is_none() || stale {
            self.mp = None;
            self.mp_players.clear();
            self.mp_world.clear();
            (self.mp_world_total, self.mp_world_known) = (0, false);
        }
        let Some(target) = target else { return };
        if in_menu {
            // (the server list names the servers in the menus, and tells how many fly on each)
            self.mp_directory();
        }
        let name = self.mp_name.clone();
        let m = self.mp.get_or_insert_with(|| {
            let mut m = mp::Mp::start(&target, &name);
            m.auto = auto;
            m
        });
        m.set_watching(in_menu);
        if in_menu {
            self.mp_players.clear();
            (self.mp_world, self.mp_world_total, self.mp_world_known) = m.world();
            return;
        }
        self.mp_world.clear();
        let p = &self.plane;
        let q = p.rot.as_quat();
        let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        let axis = |x: f64| (x.clamp(-1.0, 1.0) * 127.0).round() as i8;
        let pw = p.sys.powered();
        let mut flags = 0;
        if pw {
            flags |= mp_proto::F_NAV | mp_proto::F_STROBE | mp_proto::F_BEACON;
        }
        // (The G650's landing lights follow their switches alone.)
        if pw && (if p.spec.id == 9 { p.landing_lights } else { p.gear_down }) {
            flags |= mp_proto::F_LANDING;
        }
        if p.on_ground {
            flags |= mp_proto::F_GROUND;
        }
        if self.crashed {
            flags |= mp_proto::F_CRASHED;
        }
        m.set_state(mp_proto::PlayerState {
            seq: 0,
            aircraft: self.spec.id as u8,
            pos: p.pos.to_array(),
            rot: [q.x, q.y, q.z, q.w],
            vel: p.vel.as_vec3().to_array(),
            flaps: byte(p.flaps),
            gear: byte(p.gear),
            throttle: byte(p.throttle),
            canopy: 0,
            pitch: axis(p.stick.x),
            roll: axis(p.stick.z),
            yaw: axis(p.stick.y),
            flags,
        });
        self.mp_players = m.players();
    }

    /// --mp-mirror: our own aircraft as another player, 25 m off the left wing.
    pub(crate) fn mirror_player(&self) -> mp::Remote {
        let p = &self.plane;
        let q = p.rot.as_quat();
        let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        let axis = |x: f64| (x.clamp(-1.0, 1.0) * 127.0).round() as i8;
        let pos = p.pos - p.right() * 25.0;
        let flags = mp_proto::F_NAV | mp_proto::F_STROBE | mp_proto::F_BEACON | if p.on_ground { mp_proto::F_GROUND } else { 0 };
        let state = mp_proto::PlayerState { seq: 0, aircraft: self.spec.id as u8, pos: pos.to_array(), rot: [q.x, q.y, q.z, q.w], vel: p.vel.as_vec3().to_array(), flaps: byte(p.flaps), gear: byte(p.gear), throttle: byte(p.throttle), canopy: 0, pitch: axis(p.stick.x), roll: axis(p.stick.z), yaw: axis(p.stick.y), flags };
        mp::Remote { id: 999, name: "Mirror".into(), aircraft: state.aircraft, pos, rot: p.rot, vel: p.vel, gear: p.gear as f32, flags, state }
    }

    /// Whether `spec`'s model is ready to draw (it is flown, or waits in the
    /// hangar); if not, it is built in the background, as for the menu.
    pub(crate) fn fleet_ready(&mut self, spec: &'static aircraft::Spec) -> bool {
        if std::ptr::eq(spec, self.spec) || self.hangar.iter().any(|l| std::ptr::eq(l.spec, spec)) {
            return true;
        }
        match self.building.iter().position(|(s, _)| std::ptr::eq(*s, spec)) {
            Some(i) => match self.building[i].1.try_recv() {
                Ok(m) => {
                    self.building.swap_remove(i);
                    let l = Loaded::upload(spec, m, &mut self.r);
                    self.hangar.push(l);
                    true
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => false,
                Err(_) => {
                    self.building.swap_remove(i);
                    false
                }
            },
            None => {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(Loaded::make(spec));
                });
                self.building.push((spec, rx));
                false
            }
        }
    }

    /// The other players near enough to be worth it, drawn with their
    /// aircraft's own model: a stand-in `Plane` takes their controls, flaps,
    /// gear and throttle, and poses the pieces as our own aircraft's are.
    /// Returns which players were drawn so (the rest stay traffic models).
    pub(crate) fn draw_mp_planes(&mut self, cam: DVec3, dt: f64, objs: &mut Vec<DrawObj>) -> Vec<bool> {
        const FULL_DIST: f64 = 4000.0;
        let players = std::mem::take(&mut self.mp_players);
        let mut full = vec![false; players.len()];
        self.mp_puppets.retain(|id, _| players.iter().any(|p| p.id == *id));
        for (k, rp) in players.iter().enumerate() {
            if (rp.pos - cam).length() > FULL_DIST || rp.flags & mp_proto::F_CRASHED != 0 {
                continue;
            }
            let Some(spec) = aircraft::BUILTIN.iter().copied().find(|s| s.id == rp.aircraft as u32) else { continue };
            if spec.rocket().is_some() || !self.fleet_ready(spec) {
                continue;
            }
            let pup = self.mp_puppets.entry(rp.id).or_insert_with(|| Plane::spawn(spec, 0.0, 0.0, 1000.0, 0.0, 50.0));
            if !std::ptr::eq(pup.spec, spec) {
                *pup = Plane::spawn(spec, 0.0, 0.0, 1000.0, 0.0, 50.0);
            }
            let st = &rp.state;
            (pup.pos, pup.rot, pup.vel) = (rp.pos, rp.rot, rp.vel);
            pup.flaps = st.flaps as f64 / 255.0;
            pup.gear = st.gear as f64 / 255.0;
            pup.gear_down = st.gear > 127;
            pup.throttle = st.throttle as f64 / 255.0;
            pup.stick = DVec3::new(st.pitch as f64, st.yaw as f64, st.roll as f64) / 127.0;
            pup.on_ground = st.flags & mp_proto::F_GROUND != 0;
            // The propeller: idling on the ground, faster with the throttle.
            pup.prop_rate = 75.0 + 200.0 * pup.throttle;
            pup.prop = (pup.prop + pup.prop_rate * dt).rem_euclid(std::f64::consts::TAU * 1000.0);
            pup.t += dt;
            let (model, mesh, cut) = if std::ptr::eq(spec, self.spec) {
                (&self.model, &self.piece_mesh, &self.piece_cut)
            } else {
                let l = self.hangar.iter().find(|l| std::ptr::eq(l.spec, spec)).unwrap();
                (&l.model, &l.piece_mesh, &l.piece_cut)
            };
            // (Room for all of it, or it stays a traffic model: never half an aircraft.)
            if objs.len() + model.pieces.len() + 48 > render::MAX_OBJS as usize {
                continue;
            }
            let pose = pup.pose(&model.pieces);
            let base = Mat4::from_rotation_translation(rp.rot.as_quat(), (rp.pos - cam).as_vec3());
            let blur = ((pup.prop_rate - 30.0) / 25.0).clamp(0.0, 1.0) as f32;
            // Its lights (plane.wgsl reads them from the instruments' last row).
            let t = self.time + rp.id as f64 * 0.37;
            let strobe = { let s = t.rem_euclid(1.3); if s < 0.045 || (s > 0.16 && s < 0.2) { 1.0 } else { 0.0 } };
            let beacon = (1.0 - ((t * 1.1).rem_euclid(1.0) - 0.15).abs() * 7.0).max(0.0) as f32;
            let on = |f: u8| (st.flags & f != 0) as u8 as f32;
            let mut params = [[0.0f32; 4]; 6];
            params[5] = [1000.0 * on(mp_proto::F_NAV), strobe * on(mp_proto::F_STROBE), beacon * on(mp_proto::F_BEACON), on(mp_proto::F_LANDING)];
            for (i, p) in model.pieces.iter().enumerate() {
                let hide = match p.vis {
                    airframe::Vis::Always => false,
                    airframe::Vis::SlowProp => blur >= 1.0,
                    airframe::Vis::FastProp => blur <= 0.0,
                    airframe::Vis::Chute => true,
                };
                if hide {
                    continue;
                }
                objs.push(DrawObj { mesh: mesh[i], model: base * pose[i], tint: [blur, 0.0, 0.0, 0.0], glow: [0.0, spec.id as f32, 0.0, 0.0], params, cast: 0b11, glass: p.glass, cut: cut[i], ..DrawObj::DEFAULT });
            }
            full[k] = true;
        }
        self.mp_players = players;
        full
    }

    pub(crate) fn mp_auto(&self) -> bool {
        self.mp_server.trim().is_empty() || self.mp_server.trim().eq_ignore_ascii_case("auto")
    }

    /// The network's server list (read in the background from the first call on).
    pub(crate) fn mp_directory(&mut self) -> &mp::Directory {
        let (src, url) = (self.src.clone(), self.mp_list.clone());
        let cache = dirs::config_dir().map(|d| d.join("fablegame")).inspect(|d| {
            let _ = std::fs::create_dir_all(d);
        }).map(|d| d.join("mp_servers.json"));
        self.mp_dir.get_or_insert_with(|| mp::Directory::start(src, url, cache))
    }

    /// A server's name in the list, by its address (else the address itself).
    pub(crate) fn mp_server_name(&self, address: &str) -> String {
        let a = mp::with_port(address);
        self.mp_dir.as_ref().and_then(|d| d.servers().into_iter().find(|s| s.address == a).map(|s| s.name)).unwrap_or_else(|| address.to_string())
    }
}
