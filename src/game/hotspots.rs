//! The clickable cockpit: what the pointer or the gamepad's aim dot is on, clicks, holds and the wheel, and the mouse capture.

use crate::*;

impl Game {
    /// The cockpit view's ray through (x, y) window pixels, in the aircraft's
    /// body frame (from the eye, unit length), with the pose and the hotspots
    /// where they are now: in the cockpit view, flying, nothing open.
    pub(crate) fn cockpit_ray(&self, x: f64, y: f64) -> Option<(Vec3, Vec3, Vec<Mat4>, Vec<aircraft::Hotspot>)> {
        if self.mode != Mode::Fly || self.cam_mode != 1 || self.pause_open || self.crashed || self.cockpit_controls.is_empty() {
            return None;
        }
        let (vp, cam) = self.live_view?;
        let s = self.window.inner_size();
        let (w, h) = (s.width.max(1) as f64, s.height.max(1) as f64);
        let ndc = glam::Vec4::new((x / w * 2.0 - 1.0) as f32, (1.0 - y / h * 2.0) as f32, 0.5, 1.0);
        let p = vp.inverse() * ndc;
        let d = (p.truncate() / p.w).as_dvec3().normalize();
        let inv = self.plane.rot.inverse();
        let o = (inv * (cam - self.plane.model_pos())).as_vec3();
        let d = (inv * d).as_vec3();
        let pose = self.plane.pose(&self.model.pieces);
        let hs = self.cockpit_hotspots(&pose);
        Some((o, d, pose, hs))
    }

    /// The cockpit control under the mouse at (x, y) window pixels (an index
    /// into the loaded cockpit controls): in the cockpit view, flying, nothing open.
    pub(crate) fn hotspot_at(&self, x: f64, y: f64) -> Option<usize> {
        let (o, d, pose, hs) = self.cockpit_ray(x, y)?;
        let i = aircraft::pick(&hs, o, d)?;
        let depth = (Vec3::from(hs[i].at) - o).dot(d) - hs[i].r - 0.005;
        (!cockpit::occluded(&self.model.pieces, &pose, &self.pit_blocks, o, d, depth)).then_some(i)
    }

    /// Looking about the cockpit with the pad: the control the middle of the
    /// view is on, if any, and how near the nearest one in sight is (1 on
    /// it, falling to 0 at `AIM_NEAR` off it). None: not aiming with the pad.
    pub(crate) fn pad_aim_at(&self) -> Option<(Option<usize>, f32)> {
        /// how far off a control (radians, past its edge) the aim dot shows
        const AIM_NEAR: f32 = 0.14;
        if !self.pad_aim || self.info_open {
            return None;
        }
        let s = self.window.inner_size();
        let (o, d, pose, hs) = self.cockpit_ray(s.width as f64 * 0.5, s.height as f64 * 0.5)?;
        let on = aircraft::pick(&hs, o, d).filter(|&i| {
            let depth = (Vec3::from(hs[i].at) - o).dot(d) - hs[i].r - 0.005;
            !cockpit::occluded(&self.model.pieces, &pose, &self.pit_blocks, o, d, depth)
        });
        if on.is_some() {
            return Some((on, 1.0));
        }
        // The angle from the middle of the view to each control's edge: the
        // nearest few checked for being in sight (not behind the panel).
        let mut near: Vec<(f32, usize)> = hs.iter().enumerate().filter_map(|(i, h)| {
            let c = Vec3::from(h.at) - o;
            let l = c.length();
            (l > h.r && c.dot(d) > 0.0).then(|| (d.angle_between(c) - (h.r / l).asin(), i))
        }).filter(|&(a, _)| a < AIM_NEAR).collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let seen = near.iter().take(4).find(|&&(_, i)| {
            let c = Vec3::from(hs[i].at) - o;
            let l = c.length();
            !cockpit::occluded(&self.model.pieces, &pose, &self.pit_blocks, o, c / l, l - hs[i].r - 0.005)
        });
        Some((None, seen.map_or(0.0, |&(a, _)| 1.0 - (a / AIM_NEAR).clamp(0.0, 1.0))))
    }

    pub(crate) fn hotspot_under_mouse(&self) -> Option<usize> {
        if self.drag || self.lean_drag.is_some() {
            return None;
        }
        self.cursor.and_then(|(x, y)| self.hotspot_at(x, y))
    }

    /// The key an action stands for (its default: what `key` understands).
    pub(crate) fn action_key(id: &str) -> Option<KeyCode> {
        bind::ACTIONS.iter().find(|a| a.id == id).map(|a| a.key)
    }

    /// The left button going down (or up) on a cockpit hotspot: a click does
    /// what the action's key does; a hotspot with a `hold` action waits to
    /// see whether it is a click or held (see `mouse_holds`).
    pub(crate) fn hot_click(&mut self, down: bool) {
        if down {
            let Some(i) = self.hotspot_under_mouse() else { return };
            let h = self.cockpit_controls[i].0;
            if h.hold.is_none() {
                if let Some(k) = h.click.and_then(Self::action_key) {
                    self.key(k);
                } else if let Some(id) = h.click { self.cockpit_action(id); }
            }
            self.hot_press = Some((i, self.real, false));
        } else if let Some((i, _, held)) = self.hot_press.take() {
            let Some(h) = self.cockpit_controls.get(i).map(|e| e.0) else { return };
            if held {
                if let Some(k) = h.hold.and_then(Self::action_key) {
                    self.hot_keys.remove(&k);
                }
            } else if h.hold.is_some() {
                if let Some(k) = h.click.and_then(Self::action_key) {
                    self.key(k);
                }
            }
        }
    }

    /// The mouse wheel over a cockpit hotspot: each notch presses its "up"
    /// or "down" action once and holds it a moment. False: no hotspot there.
    pub(crate) fn hot_wheel(&mut self, d: f64) -> bool {
        let Some(i) = self.hotspot_under_mouse() else { return false };
        let Some((up, down)) = self.cockpit_controls[i].0.wheel else { return false };
        let steps = self.wheel_acc.steps(i, d);
        for _ in 0..steps.unsigned_abs() {
            let id = if steps > 0 { up } else { down };
            if let Some(k) = Self::action_key(id) {
                if let Some(opposite) = Self::action_key(if steps > 0 { down } else { up }) {
                    self.hot_keys.remove(&opposite);
                    self.wheel_hold.retain(|(q, _)| *q != opposite);
                }
                self.key(k);
                self.hot_keys.insert(k);
                let until = self.real + 0.12;
                match self.wheel_hold.iter_mut().find(|(q, _)| *q == k) {
                    Some(e) => e.1 = e.1.max(self.real) + 0.12,
                    None => self.wheel_hold.push((k, until)),
                }
            } else { self.cockpit_action(id); }
        }
        true
    }

    /// What the mouse holds: a hotspot's `hold` action once the button has
    /// been down a moment, and the keys the wheel holds until their time is up.
    pub(crate) fn mouse_holds(&mut self) {
        while let Some(&e) = self.script_clicks.first() {
            if self.real < e.t {
                break;
            }
            self.script_clicks.remove(0);
            if let Some(p) = e.at {
                (self.cursor, self.script_cursor) = (Some(p), Some(p));
            }
            if e.ev >= 50.0 {
                self.hot_wheel(e.ev - 100.0);
            } else {
                self.hot_click(e.ev > 0.5);
            }
        }
        if let Some((i, t0, false)) = self.hot_press {
            if let Some(k) = self.cockpit_controls.get(i).and_then(|e| e.0.hold).and_then(Self::action_key) {
                if self.real - t0 > 0.35 {
                    self.key(k);
                    self.hot_keys.insert(k);
                    self.hot_press = Some((i, t0, true));
                }
            }
        }
        let now = self.real;
        let mut done = Vec::new();
        self.wheel_hold.retain(|&(k, until)| {
            if until > now {
                return true;
            }
            done.push(k);
            false
        });
        for k in done {
            self.hot_keys.remove(&k);
        }
    }

    /// The state a cockpit control is in, for the label beside the mouse.
    pub(crate) fn hot_state(&self, id: &str) -> Option<String> {
        let p = &self.plane;
        let on_off = |b: bool| if b { "ON" } else { "OFF" };
        Some(match id {
            "master" => on_off(p.sys.elec.master).into(),
            "ignition" if self.spec.is_jet() => format!("{}{}", if p.ignition { "RUN" } else { "CUTOFF" }, if p.starter_on || p.cranking { " · START" } else { "" }),
            "ignition" => format!("{}{}", on_off(p.ignition), if p.starter_on || p.cranking { " · START" } else { "" }),
            "fuel_control_l" | "fuel_control_r" | "battery_l" | "battery_r" => {
                let fuel = id.starts_with("fuel");
                let on = p.pair_on(!fuel as usize, id.ends_with("_r") as usize);
                if fuel {
                    format!("{}{}", if on { "RUN" } else { "CUTOFF" }, if p.ignition && (p.starter_on || p.cranking) { " · START" } else { "" })
                } else { on_off(on).into() }
            }
            "starter" => if p.starter_on || p.cranking { "START" } else if p.running { "RUNNING" } else { "OFF" }.into(),
            "speedbrake" if self.spec.chute.is_none() => if p.speedbrake > 0.0 { t!("game.extended") } else { t!("game.retracted") }.into(),
            "ap" => on_off(self.ap.on).into(),
            "ap_hdg" | "ap_hdg_right" => format!("HDG {:03.0}°", self.ap.hdg.rem_euclid(360.0)),
            "ap_alt" | "ap_alt_up" => format!("ALT {:.0} ft", self.ap.alt * 3.28084),
            "ap_nav" | "ap_apr" => {
                let m = if id == "ap_nav" { autopilot::Lat::Nav } else { autopilot::Lat::Apr };
                if !self.ap.on || self.ap.lat != m { "OFF".into() } else if self.ap.nav_cap { "ON".into() } else { t!("game.armed").into() }
            }
            "ap_vs" | "ap_vs_up" => format!("VS {:+.0} ft/min{}", self.ap.vs * 196.85, if self.ap.on && self.ap.vert == autopilot::Vert::Vs { "" } else { " (off)" }),
            "ap_at" | "ap_spd_up" => format!("A/T {} · {:.0} kt", on_off(self.ap.at), self.ap.spd * 1.943_84),
            "parking_brake" => on_off(p.parking_brake).into(),
            "mixture_rich" => format!("{:.0} %", p.mixture * 100.0),
            "throttle_up" => format!("{:.0} %", p.throttle * 100.0),
            "flaps_up" if self.spec.lever.is_some() => self.spec.lever.as_ref()?.notch(p.flap_set).into(),
            "speedbrake" if self.spec.chute.is_some() => if p.chute_spent { "JETTISONED" } else if p.speedbrake > 0.0 { "DEPLOYED" } else { "PACKED" }.into(),
            "flaps_up" => format!("{:.0}°", self.spec.flaps.get(p.flap_set as usize).copied().unwrap_or(0.0)),
            "trim_down" => format!("{:+.1}°", p.trim.to_degrees()),
            "obs_right" => format!("OBS {:03.0}°", self.radios.obs),
            "gear" => if p.gear_down { t!("game.down") } else { t!("game.up") }.into(),
            "prop_up" => format!("{:.0} RPM", 1800.0 + 900.0 * p.propeller),
            "cowl_open" => format!("{:.0} %", p.cowl_flaps * 100.0),
            "landing_lights" => on_off(p.landing_lights).into(),
            "fuel_valve" | "fuel_next" => match p.fuel_selector {
                plane::FuelSelector::Center if std::ptr::eq(self.spec, &aircraft::dr400::SPEC) => "MAIN",
                plane::FuelSelector::Center => "BOTH",
                plane::FuelSelector::Right => "RIGHT",
                plane::FuelSelector::Left => "LEFT",
                plane::FuelSelector::Off => "OFF",
            }.into(),
            "nav_next" => self.radios.nav.as_ref().map(|s| format!("{} {}", s.freq_text(), s.label())).unwrap_or_else(|| "OFF".into()),
            "adf_next" => self.radios.adf.as_ref().map(|s| format!("{} {}", s.freq_text(), s.label())).unwrap_or_else(|| "OFF".into()),
            "baro_qnh" => format!("{:.0} hPa", self.radios.setting(self.wx.qnh)),
            _ => return None,
        })
    }

    /// The cockpit control under the mouse: a ring round it, what it is, how
    /// it is set, and the pointing hand.
    ///
    /// Looking about with the pad it is the middle of the view that points:
    /// a faint dot there once a control is near it, filling in on one, and
    /// beside it the pad buttons that work the control.
    pub(crate) fn hotspot_overlay(&self, ctx: &egui::Context) {
        use egui::{Align2, Color32, FontId};
        let screen = ctx.content_rect();
        if self.mouse_captured {
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("cockpit aim dot")));
            let c = screen.center();
            p.circle_filled(c, 2.5, Color32::from_white_alpha(220));
            p.circle_stroke(c, 6.0, egui::Stroke::new(1.0, Color32::from_black_alpha(180)));
        }
        let aim = self.pad_aim_at();
        if let Some((on, near)) = aim {
            if near > 0.0 {
                let painter = ctx.layer_painter(egui::LayerId::background());
                let c = screen.center();
                // (Faint at the edge of the range, firmer as it closes in.)
                let a = 0.35 + 0.45 * near;
                if on.is_some() {
                    painter.circle_filled(c, 5.0, Color32::from_rgba_unmultiplied(255, 210, 90, 220));
                    painter.circle_stroke(c, 5.0, egui::Stroke::new(1.0, Color32::from_black_alpha(140)));
                } else {
                    painter.circle_filled(c, 4.5, Color32::from_white_alpha((a * 255.0) as u8));
                    painter.circle_stroke(c, 4.5, egui::Stroke::new(1.0, Color32::from_black_alpha((a * 160.0) as u8)));
                }
            }
        }
        let Some(i) = (match aim { Some((on, _)) => on, None => self.hotspot_under_mouse() }) else { return };
        let Some((vp, cam)) = self.live_view else { return };
        if aim.is_none() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let h = self.cockpit_hotspots(&self.plane.pose(&self.model.pieces))[i];
        let to_screen = |b: Vec3| -> Option<egui::Pos2> {
            let w = self.plane.model_pos() + self.plane.rot * b.as_dvec3();
            let c = vp * (w - cam).as_vec3().extend(1.0);
            (c.w > 0.0).then(|| egui::pos2(screen.left() + (c.x / c.w * 0.5 + 0.5) * screen.width(), screen.top() + (0.5 - c.y / c.w * 0.5) * screen.height()))
        };
        let at = Vec3::from(h.at);
        let (Some(c), Some(e)) = (to_screen(at), to_screen(at + Vec3::Y * h.r)) else { return };
        let painter = ctx.layer_painter(egui::LayerId::background());
        let ring = Color32::from_rgba_unmultiplied(255, 210, 90, 200);
        painter.circle_stroke(c, (e - c).length().max(6.0), egui::Stroke::new(1.5, ring));
        let label = |id: &'static str| match id {
            "flaps_up" if self.spec.lever.is_some() => t!("game.raise_nose_visor"),
            "flaps_down" if self.spec.lever.is_some() => t!("game.lower_nose_visor"),
            "speedbrake" if self.spec.chute.is_some() => t!("game.drag_chute"),
            _ => bind::ACTIONS.iter().find(|a| a.id == id).map(|a| a.label()).unwrap_or_else(|| aircraft::c172::cabin::action_label(id)),
        };
        if aim.is_some() {
            // The pad: each action with the buttons that do it (none: not on the pad).
            let mut rows: Vec<(String, String)> = Vec::new();
            let mut add = |id: &'static str, extra: &str| rows.push((padmap::spec_of(id), format!("{}{extra}", label(id))));
            if let Some(id) = h.click { add(id, ""); }
            if let Some(id) = h.hold { add(id, &if h.click.is_none() { format!(" ({})", t!("game.hold")) } else { String::new() }); }
            if let Some((up, down)) = h.wheel { add(up, ""); add(down, ""); }
            rows.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
            if let Some(st) = h.click.or(h.wheel.map(|w| w.0)).and_then(|id| self.hot_state(id)) {
                rows.push((String::new(), st));
            }
            let p = ctx.layer_painter(egui::LayerId::background());
            let x0 = screen.center().x + 18.0;
            let y0 = screen.center().y + 16.0;
            let gw = rows.iter().map(|r| if r.0.is_empty() { 0.0 } else { padui::spec_width(&p, &r.0, 18.0) + 6.0 }).fold(0.0, f32::max);
            for (k, (spec, text)) in rows.iter().enumerate() {
                let y = y0 + 22.0 * k as f32;
                if !spec.is_empty() {
                    padui::paint_spec(&p, x0, y, spec, 18.0, Color32::WHITE);
                }
                let pos = egui::pos2(x0 + gw, y);
                p.text(pos + egui::vec2(1.0, 1.0), Align2::LEFT_CENTER, text, FontId::proportional(13.5), Color32::from_black_alpha(200));
                p.text(pos, Align2::LEFT_CENTER, text, FontId::proportional(13.5), Color32::WHITE);
            }
            return;
        }
        let mut lines: Vec<String> = Vec::new();
        if let Some(id) = h.click {
            let hold = h.hold.map(|k| t!("game.hold_2", label(k))).unwrap_or_default();
            lines.push(format!("{}{hold}", label(id)));
        }
        if h.click.is_none() {
            if let Some(id) = h.hold { lines.push(format!("{} ({})", label(id), t!("game.hold"))); }
        }
        if let Some((up, down)) = h.wheel {
            lines.push(format!("{} {} / {}", t!("game.wheel"), label(up), label(down)));
        }
        if let Some(s) = h.click.or(h.wheel.map(|w| w.0)).and_then(|id| self.hot_state(id)) {
            lines.push(s);
        }
        let mouse = self.cursor.map(|(x, y)| egui::pos2(x as f32 / ctx.pixels_per_point(), y as f32 / ctx.pixels_per_point())).unwrap_or(c);
        // (On the right of the screen the label goes left of the mouse, so it stays on it.)
        let left = mouse.x > screen.center().x;
        let (align, dx) = if left { (Align2::RIGHT_TOP, -16.0) } else { (Align2::LEFT_TOP, 16.0) };
        for (k, s) in lines.iter().enumerate() {
            let pos = mouse + egui::vec2(dx, 6.0 + 16.0 * k as f32);
            painter.text(pos + egui::vec2(1.0, 1.0), align, s, FontId::proportional(13.5), Color32::from_black_alpha(200));
            painter.text(pos, align, s, FontId::proportional(13.5), Color32::WHITE);
        }
    }

    pub(crate) fn cancel_cockpit_input(&mut self) {
        self.hot_press = None;
        self.lean_drag = None;
        self.hot_keys.clear();
        self.wheel_hold.clear();
        self.wheel_acc = cockpit::WheelSteps::default();
    }

    /// Keep the pointer at screen centre while mouse-look is active. The ray
    /// through that point also aims clickable cockpit controls.
    pub(crate) fn sync_mouse_capture(&mut self) {
        let want = self.window.has_focus()
            && self.mode == Mode::Fly
            && self.cam_mode == 1
            && !self.crashed
            && !self.pause_open
            && !self.info_open
            && !self.goto_open
            && !self.show_help
            && !self.minimap.full
            && self.binds.capture.is_none()
            && !self.egui_ctx.text_edit_focused();
        if want && !self.mouse_captured {
            if self.window.set_cursor_grab(CursorGrabMode::Locked).is_ok()
                || self.window.set_cursor_grab(CursorGrabMode::Confined).is_ok()
            {
                self.mouse_captured = true;
                self.window.set_cursor_visible(false);
            }
        } else if !want && self.mouse_captured {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
            self.mouse_captured = false;
            self.cursor = None;
            self.last_cursor = None;
        }
        if self.mouse_captured {
            let size = self.window.inner_size();
            let centre = (size.width as f64 * 0.5, size.height as f64 * 0.5);
            self.cursor = Some(centre);
            self.last_cursor = Some(centre);
            let _ = self.window.set_cursor_position(PhysicalPosition::new(centre.0, centre.1));
        }
    }

    pub(crate) fn release_mouse_capture(&mut self) {
        if self.mouse_captured {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
            self.mouse_captured = false;
            self.cursor = None;
            self.last_cursor = None;
        }
    }

    pub(crate) fn cockpit_hotspots(&self, pose: &[Mat4]) -> Vec<aircraft::Hotspot> {
        aircraft::animated_hotspots(&self.cockpit_controls, pose)
    }

    pub(crate) fn cockpit_action(&mut self, id: &str) {
        match id {
            "prop_up" => self.plane.propeller = (self.plane.propeller + 0.05).min(1.0),
            "prop_down" => self.plane.propeller = (self.plane.propeller - 0.05).max(0.0),
            "cowl_open" => self.plane.cowl_flaps = (self.plane.cowl_flaps + 0.1).min(1.0),
            "cowl_close" => self.plane.cowl_flaps = (self.plane.cowl_flaps - 0.1).max(0.0),
            "landing_lights" => self.plane.landing_lights = !self.plane.landing_lights,
            "ap_vs" if !self.crashed => self.ap_vert(autopilot::Vert::Vs),
            "ap_at" => self.ap_at_toggle(),
            "ap_vs_up" | "ap_vs_down" => self.ap_vs_step(id == "ap_vs_up"),
            "ap_spd_up" | "ap_spd_down" => self.ap_spd_step(id == "ap_spd_up"),
            "fuel_control_l" | "fuel_control_r" | "battery_l" | "battery_r" => self.pair_switch(id),
            "fuel_valve" | "fuel_next" | "fuel_prev" => {
                self.plane.fuel_selector = self.plane.fuel_selector.turn(id != "fuel_prev");
                // (The Bonanza's selector has LEFT, RIGHT and OFF: no BOTH.)
                if std::ptr::eq(self.spec, &aircraft::bonanza::SPEC) && self.plane.fuel_selector == plane::FuelSelector::Center {
                    self.plane.fuel_selector = self.plane.fuel_selector.turn(id != "fuel_prev");
                }
            }
            "nav_next" | "nav_prev" | "adf_next" | "adf_prev" => {
                let adf = id.starts_with("adf");
                let (lat, lon, _) = geo::pos_to_lla(self.plane.pos);
                let mut list = nav::near(lat.to_degrees(), lon.to_degrees(), if adf { 150_000.0 } else { 200_000.0 }, if adf { nav::Kind::Ndb } else { nav::Kind::Vor });
                if !adf { list.extend(nav::near(lat.to_degrees(), lon.to_degrees(), 40_000.0, nav::Kind::Ils)); }
                list.sort_by(|a,b| a.1.total_cmp(&b.1));
                let current = if adf { &self.radios.adf } else { &self.radios.nav };
                let slot = current.as_ref().and_then(|s| list.iter().position(|(q, _)| q == s)).map(|i| i + 1).unwrap_or(0);
                let next = (slot as isize + if id.ends_with("next") { 1 } else { -1 }).rem_euclid(list.len() as isize + 1) as usize;
                let station = next.checked_sub(1).map(|i| list[i].0.clone());
                if adf { self.radios.adf = station; } else if let Some(st) = station { self.radios.tune_nav(st, self.plane.pos); } else { self.radios.nav = None; }
            }
            _ => {}
        }
    }

    /// One of a twin jet's L/R fuel control or battery switches clicked: it
    /// goes to the other position. The shared state (`Plane::ignition`,
    /// `sys.elec.master`) is on while either switch of the pair is, and is
    /// switched with the action's key (`ignition`, `master`), so the mouse
    /// does what the key does.
    pub(crate) fn pair_switch(&mut self, id: &str) {
        let fuel = id.starts_with("fuel");
        let (k, w) = (id.ends_with("_r") as usize, !fuel as usize);
        let shared = |p: &plane::Plane| if fuel { p.ignition } else { p.sys.elec.master };
        let on = shared(&self.plane);
        let want = [!self.plane.pair_on(w, k), self.plane.pair_on(w, 1 - k)];
        if want[0] != on && !(on && want[1]) {
            // Off to on, or the last switch of the pair off: the key.
            if let Some(key) = Self::action_key(if fuel { "ignition" } else { "master" }) {
                self.key(key);
            }
            if shared(&self.plane) == on { return; }
        }
        // Which switch is off while its partner holds the state on (both on, or both off: neither).
        let mut pair = [false; 2];
        if want[0] != want[1] {
            pair[k] = !want[0];
            pair[1 - k] = !want[1];
        }
        self.plane.pair_off[w] = pair;
    }
}
