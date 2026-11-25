//! Flying controls and keys: keyboard, gamepad, joystick and mouse yoke into `Controls`, the key bindings, and egui's input.

use crate::*;

impl Game {
    pub(crate) fn controls(&mut self, dt: f64) -> Controls {
        self.mouse_holds();
        let k = |c: KeyCode| self.keys.contains(&c) || self.hot_keys.contains(&c);
        let ax = |pos: bool, neg: bool| (pos as i32 - neg as i32) as f64;
        let mut c = Controls {
            pitch: ax(k(KeyCode::KeyS) || k(KeyCode::ArrowDown), k(KeyCode::KeyW) || k(KeyCode::ArrowUp)),
            roll: ax(k(KeyCode::KeyD) || k(KeyCode::ArrowRight), k(KeyCode::KeyA) || k(KeyCode::ArrowLeft)),
            yaw: ax(k(KeyCode::KeyE), k(KeyCode::KeyQ)),
            throttle: ax(k(KeyCode::ShiftLeft) || k(KeyCode::ShiftRight), k(KeyCode::ControlLeft) || k(KeyCode::ControlRight)),
            brake: k(KeyCode::KeyB),
            brake_left: if self.hot_press.is_some_and(|(i, _, _)| self.cockpit_controls[i].0.hold == Some("brake_left")) { 1.0 } else { 0.0 },
            brake_right: if self.hot_press.is_some_and(|(i, _, _)| self.cockpit_controls[i].0.hold == Some("brake_right")) { 1.0 } else { 0.0 },
            // (The game's own boost is arcade mode's; real afterburners always work.)
            boost: k(KeyCode::Space) && (self.arcade || self.spec.real_boost()),
            reverse: k(KeyCode::KeyZ),
            throttle_set: None,
            starter: k(KeyCode::KeyO),
        };
        let mix = ax(k(KeyCode::PageUp), k(KeyCode::PageDown));
        if mix != 0.0 && !self.spec.is_jet() {
            self.plane.mixture = (self.plane.mixture + mix * 0.5 * dt).clamp(0.0, 1.0);
        }
        let obs = ax(k(KeyCode::BracketRight), k(KeyCode::BracketLeft));
        if obs != 0.0 {
            self.radios.obs = (self.radios.obs + obs * 20.0 * dt).rem_euclid(360.0);
        }
        let trim = ax(k(KeyCode::Numpad1), k(KeyCode::Numpad7));
        if trim != 0.0 && self.mode == Mode::Fly && !self.pause_open && !self.crashed {
            self.plane.trim = (self.plane.trim + trim * 0.035 * dt).clamp(-0.05, 0.24);
        }
        self.mouse_stick(&mut c);
        self.gamepad(dt, &mut c);
        self.joystick(&mut c);
        self.autopilot(dt, &mut c);
        c.brake |= self.plane.parking_brake;
        if let Some(t) = self.crash_at {
            if self.real > t && !self.crashed {
                c.pitch = -1.0;
                c.throttle = 1.0;
            }
        }
        if self.boost_at.is_some_and(|t| self.real > t) {
            c.throttle = 1.0;
            c.boost = true;
        }
        if !self.plane.hold && self.engines.is_some() {
            self.plane.lit = self.engines.take().unwrap_or(0);
        }
        if self.stage_at.is_some_and(|t| self.real > t) {
            self.stage_at = None;
            self.plane.stage_sep();
        }
        c
    }

    /// An Xbox-style gamepad. Its flying layout can be moved about in
    /// Settings › Controls › Gamepad (padmap.rs); it starts as Microsoft
    /// Flight Simulator (2020) sets one up by default:
    ///
    /// | input | does |
    /// |---|---|
    /// | left stick | pitch and roll |
    /// | LT / RT | rudder left / right |
    /// | A / B (hold) | throttle up / down |
    /// | X (hold) | wheel brakes |
    /// | Y + B | parking brake on / off |
    /// | Y + d-pad up / down | elevator trim nose down / up |
    /// | LB / RB | flaps up / down a notch |
    /// | left stick click | landing gear |
    /// | right stick | look around |
    /// | right stick click | reset the view |
    /// | d-pad | quick views: ahead, panel, left, right |
    /// | View | cockpit / chase camera |
    /// | Menu | pause menu |
    /// | Y + View | flight panel |
    ///
    /// Y with another button reaches what the layout above leaves out:
    ///
    /// | input | does |
    /// |---|---|
    /// | Y + A (hold) | afterburner / boost |
    /// | Y + X (hold) | thrust reversers |
    /// | Y + LB | speedbrake |
    /// | Y + RB | autopilot on / off (a rocket: the ship lands itself) |
    /// | Y + left stick click | start / stop the engine |
    /// | Y + right stick click | hide / show the HUD |
    /// | Y + Menu | canopy open / shut |
    /// | Y + d-pad left / right | master arm, lock a target (a rocket: which engines burn, drop the booster) |
    /// | Y alone, let go | fire at a locked target |
    ///
    /// Those are the defaults; the menu buttons below are fixed, and so is
    /// Menu (pause) and A after a crash.
    ///
    /// In the menus the d-pad (and the left stick, off the globe) moves, A
    /// presses, B steps back and the right stick scrolls; the flight panel
    /// takes the d-pad, A, B, the bumpers and the right stick while it is
    /// open. A skips the loading screen, and after a crash starts again.
    pub(crate) fn gamepad(&mut self, dt: f64, c: &mut Controls) {
        use gilrs::{Axis, Button, EventType};
        padmap::publish(&self.binds.pad);
        let Some(g) = self.gilrs.as_mut() else { return };
        let (mut pressed, mut released, mut relayout) = (Vec::new(), Vec::new(), false);
        // A joystick capture (Settings › Controls › Buttons, Axes) takes every device but the pads.
        let stick_capture = self.binds.capture.as_ref().is_some_and(|c| !c.from_pad() && !matches!(c, bind::Capture::Key(_)));
        while let Some(ev) = g.next_event() {
            // Every device's axes and buttons go to the bindings (joysticks,
            // pedals, throttles); one with something bound is not the gamepad.
            // A pad gilrs knows the layout of is left out unless it has been
            // bound already, so binding a joystick button cannot take it by mistake.
            let (uuid, name, pad_like) = {
                let p = g.gamepad(ev.id);
                (p.uuid(), p.name().to_string(), p.mapping_source() == gilrs::MappingSource::SdlMappings)
            };
            let bound = self.binds.uses(&uuid);
            if bound || !pad_like {
                match ev.event {
                    EventType::AxisChanged(_, v, code) => self.binds.axis_event(uuid, &name, code.into_u32(), v),
                    EventType::ButtonPressed(_, code) => self.binds.button_event(uuid, &name, code.into_u32(), true),
                    EventType::ButtonReleased(_, code) => self.binds.button_event(uuid, &name, code.into_u32(), false),
                    _ => {}
                }
            }
            if bound || (stick_capture && !pad_like) {
                continue;
            }
            match ev.event {
                EventType::Connected => relayout = true,
                EventType::ButtonPressed(b, _) => {
                    self.pad_id = Some(ev.id);
                    pressed.push(b);
                }
                EventType::ButtonReleased(b, _) => released.push(b),
                EventType::AxisChanged(..) | EventType::ButtonChanged(..) => {
                    if self.pad_id.is_none() {
                        self.pad_id = Some(ev.id);
                    }
                }
                EventType::Disconnected if self.pad_id == Some(ev.id) => self.pad_id = None,
                _ => {}
            }
        }
        // A device a preset knows by name, seen for the first time: put it on.
        let seen: Vec<(bind::Uuid, String)> = g.gamepads().map(|(_, p)| (p.uuid(), p.name().to_string())).collect();
        for (uuid, name) in seen {
            if let Some(p) = self.binds.auto_preset(uuid, &name) {
                self.status = t!("game.preset_applied_see_bindings", name, p);
                self.status_until = self.real + 8.0;
            }
        }
        // A pad plugged in that gilrs has no layout for: open it again with one.
        let unfixed: Vec<[u8; 16]> = g.gamepads().filter(|(_, p)| needs_xbox_layout(p) && !self.pad_fixed.contains(&p.uuid())).map(|(_, p)| p.uuid()).collect();
        if relayout && !unfixed.is_empty() {
            self.pad_fixed.extend(unfixed);
            self.gilrs = open_gamepads();
            self.pad_id = None;
            return;
        }
        let binds = &self.binds;
        let free = |p: &gilrs::Gamepad| !binds.uses(&p.uuid());
        let id = self.pad_id.filter(|id| g.connected_gamepad(*id).is_some_and(|p| free(&p))).or_else(|| g.gamepads().find(|(_, p)| free(p)).map(|(id, _)| id));
        let Some(pad) = id.and_then(|id| g.connected_gamepad(id)) else {
            self.pad = Controls::default();
            self.pad_live = None;
            padui::set_active(false);
            self.pad_aim = false;
            self.pad_rep = None;
            (self.pad_y, self.pad_y_combo) = (false, false);
            self.pad_release_all();
            if self.binds.capture.as_ref().is_some_and(|c| c.from_pad()) {
                self.binds.capture = None;
            }
            return;
        };
        padui::set_active(true);
        self.pad_live = Some(pad_live(&pad));
        // Binding from the pad (Settings › Controls › Gamepad): its buttons
        // and sticks go to that alone.
        if let Some(cap) = self.binds.capture.clone().filter(|c| c.from_pad()) {
            use padmap::{Caught, PadBtn};
            let trig = |b: Button, z: Axis| match (pad.button_data(b), pad.axis_data(z)) {
                (Some(d), _) => d.value(),
                (None, Some(a)) => ((a.value() + 1.0) * 0.5).clamp(0.0, 1.0),
                _ => 0.0,
            };
            let sticks = [pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY), pad.value(Axis::RightStickX), pad.value(Axis::RightStickY), trig(Button::RightTrigger2, Axis::RightZ) - trig(Button::LeftTrigger2, Axis::LeftZ)];
            let y_held = pad.is_pressed(Button::North);
            let now = self.real;
            let c = &mut self.binds.pad_catch;
            let mut got = Caught::Wait;
            let evs = pressed.iter().map(|b| (*b, true)).chain(released.iter().map(|b| (*b, false)));
            for (b, down) in evs {
                if let Some(pb) = PadBtn::from_gilrs(b) {
                    match c.button(pb, down, y_held || (down && pb == PadBtn::Y), now) {
                        Caught::Wait => {}
                        r => got = r,
                    }
                }
            }
            if got == Caught::Wait {
                got = c.tick(now);
            }
            if let (Caught::Wait, bind::Capture::PadAxis(_)) = (&got, &cap) {
                got = c.sticks(sticks);
            }
            let lost = match (got, cap) {
                (Caught::Wait, _) => None,
                (Caught::Cancel, _) => Some(Vec::new()),
                (Caught::Refused(_), _) => {
                    self.pad_note = (t!("game.menu_pauses_it_cannot_be").into(), self.real + 6.0);
                    None
                }
                (Caught::Button(ch), bind::Capture::Pad(id)) => Some(self.binds.pad.set(id, ch)),
                (Caught::Axis(src), bind::Capture::PadAxis(a)) => Some(self.binds.pad.set_axis(a, Some(src))),
                // (a stick moved while waiting for a button, or the other way round)
                _ => None,
            };
            if let Some(lost) = lost {
                self.binds.capture = None;
                if !lost.is_empty() {
                    self.pad_note = (t!("game.no_longer_bound", lost.join(", ")), self.real + 8.0);
                }
            }
            self.pad = Controls::default();
            self.pad_rep = None;
            (self.pad_y, self.pad_y_combo) = (false, false);
            padui::set_scroll(0.0);
            return;
        }
        // Where the pad's buttons go: the globe, the pause menu, the flight
        // panel (which then has the d-pad, A, B and the bumpers), or the aircraft.
        let nav = if self.mode == Mode::Menu {
            Nav::Globe
        } else if self.pause_open {
            Nav::Pause
        } else if self.info_open || self.radial.open {
            Nav::Panel
        } else {
            Nav::Off
        };
        // Each thumbstick's dead zone (Settings › Controls › Gamepad), the rest
        // of the travel rescaled so there is no step out of it.
        let pm = &self.binds.pad;
        let (dl, dr, dtrig) = (pm.dead(padmap::Thumb::Left), pm.dead(padmap::Thumb::Right), pm.dead(padmap::Thumb::Triggers));
        let dzl = |v: f32| padmap::centred(v as f64, dl);
        let dzr = |v: f32| padmap::centred(v as f64, dr);
        let dz = |s: padmap::Stick, v: f32| if s.thumb() == padmap::Thumb::Left { dzl(v) } else { dzr(v) };
        // The analogue triggers; on a pad left without a layout they come through as
        // bare Z axes, -1 at rest to +1 squeezed.
        let trig = |b: Button, z: Axis| match (pad.button_data(b), pad.axis_data(z)) {
            (Some(d), _) => d.value() as f64,
            (None, Some(a)) => ((a.value() as f64 + 1.0) * 0.5).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let trig_dz = |b: Button, z: Axis| padmap::one_ended(trig(b, z), dtrig);
        let modifier = pad.is_pressed(Button::North);
        let flying = self.mode == Mode::Fly && !self.pause_open;
        // The globe itself is only on the start screen's Fly page; the main
        // menu, aircraft and settings pages cover it.
        let globe = nav == Nav::Globe && self.menu.screen == menu::Screen::Fly;
        // The flying axes, wherever Settings › Controls › Gamepad has put them.
        let map = &self.binds.pad;
        let stick = |s: padmap::Stick| match s {
            padmap::Stick::LX => dz(s, pad.value(Axis::LeftStickX)),
            padmap::Stick::LY => dz(s, pad.value(Axis::LeftStickY)),
            padmap::Stick::RX => dz(s, pad.value(Axis::RightStickX)),
            padmap::Stick::RY => dz(s, pad.value(Axis::RightStickY)),
            padmap::Stick::Trig => (trig_dz(Button::RightTrigger2, Axis::RightZ) - trig_dz(Button::LeftTrigger2, Axis::LeftZ)).clamp(-1.0, 1.0),
        };
        let axis = |a: padmap::PadAxis| map.axis(a).map(|s| stick(s.stick) * s.sign as f64).unwrap_or(0.0);
        self.pad = Controls { pitch: axis(padmap::PadAxis::Pitch), roll: axis(padmap::PadAxis::Roll), yaw: axis(padmap::PadAxis::Rudder), throttle: axis(padmap::PadAxis::Throttle), ..Controls::default() };
        let (look_x, look_y) = (axis(padmap::PadAxis::LookX), axis(padmap::PadAxis::LookY));
        // The buttons held now, for the flying actions held with them.
        let down: Vec<padmap::PadBtn> = padmap::PadBtn::ALL.into_iter().filter(|b| pad.is_pressed(b.gilrs())).collect();
        let ry = dzr(pad.value(Axis::RightStickY));
        let right = (pad.value(Axis::RightStickX), pad.value(Axis::RightStickY));

        // Moving about the menus: the d-pad (and the left stick too, except
        // where it turns the globe or flies), repeating while held.
        let dirs = [(Button::DPadUp, egui::Key::ArrowUp), (Button::DPadDown, egui::Key::ArrowDown), (Button::DPadLeft, egui::Key::ArrowLeft), (Button::DPadRight, egui::Key::ArrowRight)];
        let mut held = dirs.iter().find(|(b, _)| pad.is_pressed(*b)).map(|d| d.1);
        let (lx, ly) = (pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY));
        let stick_nav = nav == Nav::Pause || (nav == Nav::Globe && !globe);
        if held.is_none() && stick_nav && lx.abs().max(ly.abs()) > 0.6 {
            held = Some(if ly.abs() > lx.abs() { if ly > 0.0 { egui::Key::ArrowUp } else { egui::Key::ArrowDown } } else if lx > 0.0 { egui::Key::ArrowRight } else { egui::Key::ArrowLeft });
        }
        let tap = pressed.iter().find_map(|b| dirs.iter().find(|d| d.0 == *b).map(|d| d.1));
        let zoom = trig(Button::RightTrigger2, Axis::RightZ) - trig(Button::LeftTrigger2, Axis::LeftZ);
        let mut nav_keys = Vec::new();
        if nav == Nav::Off {
            self.pad_rep = None;
        } else {
            match (held, self.pad_rep) {
                (Some(k), Some((pk, t))) if pk == k => {
                    if self.real >= t {
                        nav_keys.push(k);
                        self.pad_rep = Some((k, (t + 0.085).max(self.real)));
                    }
                }
                (Some(k), _) => {
                    nav_keys.push(k);
                    self.pad_rep = Some((k, self.real + 0.4));
                }
                (None, _) => {
                    self.pad_rep = None;
                    nav_keys.extend(tap);
                }
            }
        }
        for k in nav_keys {
            if self.radial.open {
                // The flight menu is a ring: the d-pad goes round it.
                let n = flightui::RADIAL_N;
                let back = matches!(k, egui::Key::ArrowLeft | egui::Key::ArrowUp);
                self.radial.pad = Some(match self.radial.pad {
                    None => 0,
                    Some(i) if back => (i + n - 1) % n,
                    Some(i) => (i + 1) % n,
                });
            } else {
                self.pad_key(k);
            }
        }
        // The right stick points straight at a flight menu item.
        if self.radial.open {
            let (rx, ry) = right;
            if rx.hypot(ry) > 0.5 {
                let n = flightui::RADIAL_N;
                let step = std::f32::consts::TAU / n as f32;
                // Clockwise from the top, as the ring is laid out.
                let a = rx.atan2(ry).rem_euclid(std::f32::consts::TAU);
                self.radial.pad = Some((a / step).round() as usize % n);
            }
        }
        // The right stick scrolls the menus and the flight panel: squared,
        // for slow reading near the centre, fast at full tilt.
        padui::set_scroll(if nav == Nav::Off { 0.0 } else { (ry * ry.abs() * 1600.0 * dt) as f32 });
        if globe {
            // Left stick turns the globe (squared, for a fine touch), the triggers zoom.
            let h = self.window.inner_size().height.max(1) as f64;
            let sq = |v: f64| v * v.abs();
            let (sx, sy) = (sq(dzl(lx)), sq(dzl(ly)));
            if sx != 0.0 || sy != 0.0 {
                let k = 0.8 * h * dt;
                self.menu.drag(-sx * k, sy * k, h, 60f64.to_radians());
            }
            if zoom.abs() > 0.05 {
                self.menu.zoom(zoom * 5.0 * dt);
            }
        }

        c.pitch = (c.pitch + self.pad.pitch).clamp(-1.0, 1.0);
        c.roll = (c.roll + self.pad.roll).clamp(-1.0, 1.0);
        c.yaw = (c.yaw + self.pad.yaw).clamp(-1.0, 1.0);
        c.throttle = (c.throttle + self.pad.throttle).clamp(-1.0, 1.0);

        // (With the flight panel open the right stick scrolls it instead.)
        if flying && nav != Nav::Panel && self.cam_mode != settings::TAIL_VIEW {
            // Right stick: look around, as fast as a quick head turn at full deflection.
            // Pushed right the view swings right, in the cockpit and outside.
            let (sx, sy) = self.look_signs();
            let rate = 2.2 * self.look_sens * dt;
            let side = if self.cam_mode == 1 { 1.0 } else { -1.0 };
            self.cam_yaw += look_x * rate * side * sx;
            self.cam_pitch = (self.cam_pitch - look_y * rate * 0.8 * sy).clamp(-1.2, 1.4);
        }
        // Looking or flying with the pad: the middle of the view aims at the cockpit's controls.
        if look_x != 0.0 || look_y != 0.0 || self.pad.pitch != 0.0 || self.pad.roll != 0.0 || !pressed.is_empty() {
            self.pad_aim = true;
        }

        // Y held before this frame, or pressed in it: what the buttons pressed now mean.
        let y_now = self.pad_y || pressed.contains(&Button::North);
        for &b in &pressed {
            self.pad_button(nav, b, modifier);
        }
        self.pad_fly_buttons(nav, &down, &pressed, y_now);
        // Y on its own, let go without having been used with another button.
        if pressed.contains(&Button::North) {
            self.pad_y = true;
            self.pad_y_combo = false;
        }
        if self.pad_y && pressed.iter().any(|b| *b != Button::North) {
            self.pad_y_combo = true;
        }
        if released.contains(&Button::North) {
            if self.pad_y && !self.pad_y_combo && nav == Nav::Off && self.mode == Mode::Fly && !self.pause_open {
                if let Some(id) = self.binds.pad.action_at(padmap::Y_ALONE) {
                    // A tap of Y fires only at a locked target: no complaint otherwise.
                    let quiet = id == "fire_missile" && (self.crashed || !self.missiles_ok() || self.weapons.lock < 1.0);
                    if !quiet {
                        self.pad_do(id, false);
                    }
                }
            }
            self.pad_y = false;
            self.pad_y_combo = false;
        }
    }

    /// The pad's flying buttons, as Settings › Controls › Gamepad has them:
    /// each action pressed with its button and held while it is (its key
    /// held for the controls that read keys, like the throttle and brakes).
    /// `down`: the buttons held now; `pressed`: those pressed this frame
    /// (a tap quicker than a frame still counts); `y`: Y held for them.
    pub(crate) fn pad_fly_buttons(&mut self, nav: Nav, down: &[padmap::PadBtn], pressed: &[gilrs::Button], y: bool) {
        use padmap::{Chord, PadBtn};
        let mut now: Vec<&'static str> = Vec::new();
        let mut taps: Vec<&'static str> = Vec::new();
        if matches!(nav, Nav::Off | Nav::Panel) && self.mode == Mode::Fly && !self.pause_open {
            let map = &self.binds.pad;
            // The flight panel keeps A, B, the d-pad and the bumpers while it is open.
            let panel = |c: Chord| nav == Nav::Panel && !c.y && matches!(c.btn, PadBtn::A | PadBtn::B | PadBtn::LB | PadBtn::RB | PadBtn::Up | PadBtn::Down | PadBtn::Left | PadBtn::Right);
            // While Y is held a button with a Y binding does that, not its own.
            let live = |c: Chord, held: bool| held && c != padmap::Y_ALONE && !panel(c) && if c.y { y } else { !(y && map.has_y(c.btn)) };
            for (id, c) in map.bound() {
                if live(c, down.contains(&c.btn)) {
                    now.push(id);
                } else if live(c, pressed.contains(&c.btn.gilrs())) {
                    taps.push(id);
                }
            }
            // A after a crash flies again (pad_button), and skips the loading screen.
            let skip_a = self.crashed || self.loading.as_ref().is_some_and(|l| l.done.is_none());
            if skip_a {
                let a = map.action_at(padmap::Chord { btn: PadBtn::A, y: false });
                now.retain(|id| Some(*id) != a);
                taps.retain(|id| Some(*id) != a);
            }
        }
        let gone: Vec<_> = self.pad_held.iter().filter(|(id, _)| !now.contains(id)).copied().collect();
        for (_, k) in gone {
            if let Some(k) = k {
                self.keys.remove(&k);
            }
        }
        self.pad_held.retain(|(id, _)| now.contains(id));
        for id in now {
            match self.pad_held.iter().find(|(h, _)| *h == id) {
                // (put back if the window losing focus let go of the keys)
                Some((_, Some(k))) => {
                    self.keys.insert(*k);
                }
                Some(_) => {}
                None => {
                    let k = self.pad_do(id, true);
                    self.pad_held.push((id, k));
                }
            }
        }
        for id in taps {
            self.pad_do(id, false);
        }
    }

    /// A pad action pressed: what its key does (returned, to be let go of
    /// later when `held`), or one of the pad's views.
    pub(crate) fn pad_do(&mut self, id: &'static str, held: bool) -> Option<KeyCode> {
        use padmap::{Does, View};
        match padmap::does(id, self.spec.rocket().is_some())? {
            Does::View(v) => {
                if self.cam_mode == settings::TAIL_VIEW {
                    return None;
                }
                // Quick views. Outside, left and right look at that side of the aircraft.
                let (home_y, home_p) = self.cam_home;
                let side = if self.cam_mode == 1 { 1.0 } else { -1.0 };
                (self.cam_yaw, self.cam_pitch) = match v {
                    View::Ahead => (home_y, home_p),
                    View::Down if self.cam_mode == 1 => (home_y, 0.55),
                    View::Down => (home_y + std::f64::consts::PI, home_p),
                    View::Left => (home_y - side * std::f64::consts::FRAC_PI_2, home_p.min(0.15)),
                    View::Right => (home_y + side * std::f64::consts::FRAC_PI_2, home_p.min(0.15)),
                    View::Reset => {
                        self.head.zoom = 1.0;
                        (home_y, home_p)
                    }
                };
                None
            }
            Does::Key(KeyCode::Tab) => {
                self.flight_menu_key();
                None
            }
            Does::Key(k) => {
                self.key(k);
                if held {
                    self.keys.insert(k);
                    return Some(k);
                }
                None
            }
        }
    }

    /// Lets go of every key the pad's buttons were holding.
    pub(crate) fn pad_release_all(&mut self) {
        for (_, k) in std::mem::take(&mut self.pad_held) {
            if let Some(k) = k {
                self.keys.remove(&k);
            }
        }
    }

    /// Bound joystick, pedal and throttle axes, and bound buttons (which do
    /// what their key does).
    pub(crate) fn joystick(&mut self, c: &mut Controls) {
        use bind::AxisAct as A;
        let b = &self.binds;
        if let Some(v) = b.axis(A::Pitch) {
            c.pitch = (c.pitch + v).clamp(-1.0, 1.0);
        }
        if let Some(v) = b.axis(A::Roll) {
            c.roll = (c.roll + v).clamp(-1.0, 1.0);
        }
        if let Some(v) = b.axis(A::Yaw) {
            c.yaw = (c.yaw + v).clamp(-1.0, 1.0);
        }
        // A throttle lever sets the power, unless the keys are moving it.
        if let Some(t) = b.axis(A::Throttle) {
            // The copilot's setting holds until the pilot moves the throttle;
            // any key, gamepad or lever input takes the power back.
            if c.throttle != 0.0 || self.mcp_throttle.is_some_and(|_| self.throttle_axis.is_some_and(|last| (t - last).abs() > 0.02)) {
                self.mcp_throttle = None;
            }
            self.throttle_axis = Some(t);
            if let Some(t) = self.mcp_throttle {
                c.throttle_set = Some(t);
            } else if c.throttle == 0.0 {
                c.throttle_set = Some(t);
            }
        } else {
            self.throttle_axis = None;
        }
        c.brake_left = c.brake_left.max(b.axis(A::BrakeLeft).unwrap_or(0.0).clamp(0.0, 1.0));
        c.brake_right = c.brake_right.max(b.axis(A::BrakeRight).unwrap_or(0.0).clamp(0.0, 1.0));
        for (id, down) in self.binds.take_edges() {
            let Some(a) = bind::ACTIONS.iter().find(|a| a.id == id) else { continue };
            if down {
                // (Esc on the globe would quit the game.)
                if !(a.key == KeyCode::Escape && self.mode != Mode::Fly) {
                    self.key(a.key);
                }
                self.keys.insert(a.key);
            } else {
                self.keys.remove(&a.key);
            }
        }
    }

    /// mouse yoke, pointer offset from centre is the stick.
    pub(crate) fn mouse_stick(&mut self, c: &mut Controls) {
        if self.yoke_thr != 0.0 && self.mode == Mode::Fly && !self.crashed {
            c.throttle_set = Some((self.plane.throttle + self.yoke_thr).clamp(0.0, 1.0));
        }
        self.yoke_thr = 0.0;
        if !self.mouse_yoke || self.mode != Mode::Fly || self.pause_open || self.crashed {
            return;
        }
        if self.ap.on {
            self.yoke = (0.0, 0.0);
            return;
        }
        let busy = self.drag || self.lean_drag.is_some() || self.egui_ctx.is_pointer_over_egui() || self.egui_ctx.egui_wants_pointer_input();
        if !busy {
            if let Some((x, y)) = self.cursor {
                let size = self.window.inner_size();
                let (w, h) = (size.width.max(1) as f64, size.height.max(1) as f64);
                let r = w.min(h) * 0.4;
                let shape = |v: f64| {
                    let v = v.clamp(-1.0, 1.0);
                    let a = ((v.abs() - 0.03) / 0.97).max(0.0);
                    v.signum() * a.powf(1.5)
                };
                self.yoke = (shape((h * 0.5 - y) / r), shape((x - w * 0.5) / r));
            }
        }
        c.pitch = (c.pitch + self.yoke.0).clamp(-1.0, 1.0);
        c.roll = (c.roll + self.yoke.1).clamp(-1.0, 1.0);
    }

    /// toggles the mouse yoke.
    pub(crate) fn toggle_mouse_yoke(&mut self) {
        self.mouse_yoke = !self.mouse_yoke;
        self.yoke = (0.0, 0.0);
        self.status = if self.mouse_yoke {
            t!("game.mouse_yoke_on_the_pointer_flies").into()
        } else {
            t!("game.mouse_yoke_off").into()
        };
        self.status_until = self.real + 4.0;
    }

    /// mouse yoke cursor, a dot in place of the pointer.
    pub(crate) fn mouse_yoke_hud(&self, ctx: &egui::Context) {
        use egui::Color32;
        if !self.mouse_yoke || self.pause_open || self.crashed || ctx.is_pointer_over_egui() {
            return;
        }
        let Some((x, y)) = self.cursor else { return };
        let ppp = ctx.pixels_per_point();
        let at = egui::pos2(x as f32 / ppp, y as f32 / ppp);
        ctx.set_cursor_icon(egui::CursorIcon::None);
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("mouse_yoke")));
        p.circle_filled(at, 4.5, Color32::from_black_alpha(140));
        p.circle_filled(at, 3.0, if self.ap.on { Color32::from_white_alpha(120) } else { Color32::WHITE });
    }

    /// A gamepad button pressed, for whatever it works at the moment.
    pub(crate) fn pad_button(&mut self, nav: Nav, b: gilrs::Button, modifier: bool) {
        use gilrs::Button;
        match (nav, b) {
            // The start menu. Everywhere A presses what is picked out and B
            // steps back towards the main menu. On the globe Menu takes off
            // and X starts at the crosshair; on the settings and aircraft
            // screens the bumpers turn the sections or the aircraft.
            (Nav::Globe, Button::South) => self.pad_key(egui::Key::Enter),
            (Nav::Globe, Button::East) => self.menu_back(),
            (Nav::Globe, Button::Start) if self.menu.screen == menu::Screen::Fly => self.begin(),
            (Nav::Globe, Button::West) if self.menu.screen == menu::Screen::Fly => self.pad_pick_centre(),
            (Nav::Globe, Button::LeftTrigger | Button::RightTrigger) if self.menu.screen == menu::Screen::Settings => {
                self.settings_sec = self.settings_sec.cycle(b == Button::RightTrigger);
                self.pad_home = true;
            }
            (Nav::Globe, Button::LeftTrigger | Button::RightTrigger) if self.menu.screen == menu::Screen::Aircraft => {
                let n = aircraft::list().len();
                self.menu.air_view = (self.menu.air_view + if b == Button::RightTrigger { 1 } else { n - 1 }) % n;
                self.menu.air_tab = hangar_ui::Tab::Mine;
                self.pad_home = true;
            }
            (Nav::Globe, _) => {}
            // The pause menu: A presses, B steps back out of a page (then resumes), Menu resumes,
            // the bumpers turn the settings screen's sections.
            (Nav::Pause, Button::East) if self.pause_page != menu::Page::Game => {
                self.pause_page = menu::Page::Game;
                self.pad_home = true;
            }
            (Nav::Pause, Button::Start | Button::East) => self.set_pause_menu(false),
            (Nav::Pause, Button::South) => self.pad_key(egui::Key::Enter),
            (Nav::Pause, Button::LeftTrigger | Button::RightTrigger) if self.pause_page == menu::Page::Settings => {
                self.settings_sec = self.settings_sec.cycle(b == Button::RightTrigger);
                self.pad_home = true;
            }
            (Nav::Pause, _) => {}
            // The flight panel, open beside the flying: the same, and B closes it.
            // (Its other buttons, and Y with any, still fly: pad_fly_buttons.)
            (Nav::Panel, Button::South) if !modifier && self.radial.open => self.radial.pick = true,
            (Nav::Panel, Button::South) if !modifier => self.pad_key(egui::Key::Enter),
            (Nav::Panel, Button::East) if !modifier && self.radial.open => self.radial.open = false,
            (Nav::Panel, Button::East) if !modifier => self.info_open = false,
            (Nav::Panel, Button::LeftTrigger | Button::RightTrigger) if !modifier => {
                self.info_page = menu::Page::cycle(&menu::Page::INFO, self.info_page, b == Button::RightTrigger);
                self.pad_home = true;
            }
            // A skips the loading screen (the scenery goes on loading), as Enter does.
            (_, Button::South) if self.loading.as_ref().is_some_and(|l| l.done.is_none()) => self.key(KeyCode::Enter),
            (_, Button::South) if self.crashed => self.respawn_here(),
            // Menu pauses, unless Y + Menu is given something to do.
            (_, Button::Start) if !(modifier && self.binds.pad.has_y(padmap::PadBtn::Menu)) => self.set_pause_menu(true),
            // Everything else that flies is in Settings › Controls › Gamepad (pad_fly_buttons).
            _ => {}
        }
    }

    /// A key for the menus from the gamepad (d-pad arrows, A as Enter). With
    /// nothing picked out yet it picks out the screen's first choice instead;
    /// in a text box the arrows move on rather than along the text.
    pub(crate) fn pad_key(&mut self, key: egui::Key) {
        if self.egui_ctx.memory(|m| m.focused()).is_none() {
            self.pad_home = true;
            return;
        }
        let dir = match key {
            egui::Key::ArrowUp => Some(egui::FocusDirection::Up),
            egui::Key::ArrowDown => Some(egui::FocusDirection::Down),
            egui::Key::ArrowLeft => Some(egui::FocusDirection::Left),
            egui::Key::ArrowRight => Some(egui::FocusDirection::Right),
            _ => None,
        };
        if let Some(d) = dir {
            self.pad_skip = Some((d, 0, 0));
            if self.egui_ctx.text_edit_focused() {
                self.pad_move = Some(d);
                return;
            }
        }
        for pressed in [true, false] {
            self.pad_events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
        }
    }

    /// adds a waypoint at a screen position.
    pub(crate) fn plan_add_at(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        if let Some(i) = self.menu.marker_near(x, y, r) {
            self.plan.push(flightplan::Waypoint::airport(i));
        } else if let Some((lat, lon)) = self.menu.pick_at(x, y, w, h, 60f64.to_radians()) {
            self.plan.push_point(lat, lon);
        }
        self.plan_msg.clear();
    }

    /// The globe: start at the airport or the spot under the crosshair.
    pub(crate) fn pad_pick_centre(&mut self) {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1) as f64, size.height.max(1) as f64);
        if self.plan_pick {
            self.plan_add_at(w * 0.5, h * 0.5, w, h, 28.0 * self.egui_ctx.pixels_per_point() as f64);
            return;
        }
        let near = self.menu.marker_near(w * 0.5, h * 0.5, 28.0 * self.egui_ctx.pixels_per_point() as f64);
        if let Some(i) = near {
            self.choose_airport(i);
        } else if let Some((lat, lon)) = self.menu.pick_at(w * 0.5, h * 0.5, w, h, 60f64.to_radians()) {
            self.pick(lat, lon);
        }
    }

    pub(crate) fn key(&mut self, code: KeyCode) {
        let alt = self.egui_ctx.input(|i| i.modifiers.alt);
        if code == KeyCode::F11 || (alt && matches!(code, KeyCode::Enter | KeyCode::NumpadEnter)) {
            self.toggle_fullscreen();
            return;
        }
        if self.mode == Mode::Menu {
            use menu::Screen;
            let enter = matches!(code, KeyCode::Enter | KeyCode::NumpadEnter);
            match self.menu.screen {
                _ if code == KeyCode::Escape => self.menu_back(),
                Screen::Main if enter => {
                    self.menu.screen = Screen::Fly;
                    self.pad_home = true;
                }
                Screen::Fly if enter => self.begin(),
                Screen::Aircraft if enter && self.menu.air_tab == hangar_ui::Tab::Mine => {
                    self.menu.aircraft = self.menu.air_view;
                    self.menu.screen = self.menu.air_back;
                    self.pad_home = true;
                }
                _ if code == KeyCode::F12 => self.shots.insert(0, (0.0, PathBuf::from(format!("screenshot-{}.png", self.frame)), None)),
                _ => {}
            }
            return;
        }
        // Enter, Space or Esc skip the loading screen (the scenery goes on loading).
        if self.loading.as_ref().is_some_and(|l| l.done.is_none()) {
            if matches!(code, KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space | KeyCode::Escape) {
                self.finish_loading();
            }
            return;
        }
        if self.pause_open {
            // The pause menu has the keyboard (and the game is stopped) until it closes.
            match code {
                // Esc steps back: out of a page (the settings, the key list, the failures), then out of the menu.
                KeyCode::Escape if self.pause_page != menu::Page::Game => {
                    self.pause_page = menu::Page::Game;
                    self.pad_home = true;
                }
                KeyCode::Escape => self.set_pause_menu(false),
                KeyCode::F12 => self.shots.insert(0, (0.0, PathBuf::from(format!("screenshot-{}.png", self.frame)), None)),
                _ => {}
            }
            return;
        }
        match code {
            KeyCode::Tab => self.flight_menu_key(),
            KeyCode::Escape if self.radial.open => self.radial.open = false,
            // Esc closes a pop-up window first; otherwise it pauses.
            KeyCode::Escape if self.show_help => self.show_help = false,
            KeyCode::Escape if self.minimap.full => self.minimap.full = false,
            KeyCode::Escape => self.set_pause_menu(true),
            KeyCode::KeyR if self.arcade || self.crashed => self.respawn_here(),
            KeyCode::KeyR => self.arcade_only(t!("game.respawning_in_flight")),
            KeyCode::KeyC => self.next_view(),
            KeyCode::F8 => self.toggle_mouse_yoke(),
            KeyCode::F6 if !self.crashed => self.toggle_tail_view(),
            KeyCode::KeyF if !self.crashed => self.plane.set_flaps(1),
            KeyCode::KeyV if !self.crashed => self.plane.set_flaps(-1),
            KeyCode::KeyL if !self.crashed => self.plane.toggle_gear(),
            KeyCode::Slash if !self.crashed => self.plane.toggle_speedbrake(),
            KeyCode::Digit6 if !self.crashed && self.model.pieces.iter().any(|p| p.anim.uses(airframe::Input::Canopy)) => {
                self.plane.toggle_canopy();
                self.status = if self.plane.canopy_open { t!("game.canopy_open") } else { t!("game.canopy_shut") }.into();
                self.status_until = self.real + 3.0;
            }
            KeyCode::Digit1 if !self.crashed => {
                self.plane.next_engines();
                if let Some((name, lit, _)) = self.plane.engines() {
                    self.status = t!("game.engines_burning", name, lit);
                    self.status_until = self.real + 3.0;
                }
            }
            KeyCode::Backspace if !self.crashed => self.plane.stage_sep(),
            KeyCode::Digit2 if !self.crashed => self.plane.toggle_autoland(),
            KeyCode::Digit3 if !self.crashed => {
                if !self.missiles_ok() {
                    self.status = self.missiles_refusal().into();
                } else {
                    let on = self.weapons.toggle_arm();
                    self.status = if on { t!("game.master_arm_on") } else { t!("game.master_arm_off") }.into();
                }
                self.status_until = self.real + 3.0;
            }
            KeyCode::Digit4 if !self.crashed => {
                if !self.missiles_ok() {
                    self.status = self.missiles_refusal().into();
                } else {
                    self.weapons.request_cycle();
                }
                self.status_until = self.real + 3.0;
            }
            KeyCode::Digit5 if !self.crashed => {
                if !self.missiles_ok() {
                    self.status = self.missiles_refusal().into();
                } else {
                    self.weapons.request_fire();
                }
                self.status_until = self.real + 3.0;
            }
            KeyCode::KeyG => {
                self.goto_open = true;
                self.goto_text.clear();
            }
            KeyCode::KeyT | KeyCode::KeyY | KeyCode::KeyN | KeyCode::KeyM if !self.arcade => self.arcade_only(t!("game.the_weather_and_time_keys")),
            KeyCode::KeyT | KeyCode::KeyY => {
                // Setting the time by hand leaves real time.
                self.wx.real_time = false;
                self.utc = (self.utc + if code == KeyCode::KeyT { 0.5 } else { -0.5 }).rem_euclid(24.0);
            }
            KeyCode::KeyN => self.wx.clouds = (self.wx.clouds + 0.1).min(1.0),
            KeyCode::KeyM => self.wx.clouds = (self.wx.clouds - 0.1).max(0.0),
            KeyCode::F3 => self.toolbar_act(flightui::Bar::Page(menu::Page::Weather)),
            KeyCode::KeyP => self.paused = !self.paused,
            // h toggles the hud of the current view.
            KeyCode::KeyH if self.cam_mode == 1 => self.hud_cockpit = !self.hud_cockpit,
            KeyCode::KeyH => self.show_hud = !self.show_hud,
            KeyCode::Backquote => self.minimap.visible = !self.minimap.visible,
            KeyCode::KeyJ => self.minimap.toggle_full(),
            KeyCode::Insert => {
                self.plane.pair_off[1] = [false; 2];
                let m = &mut self.plane.sys.elec.master;
                *m = !*m;
                self.status = if *m { t!("game.battery_master_on") } else { t!("game.battery_master_off") }.into();
                self.status_until = self.real + 3.0;
            }
            KeyCode::KeyI => {
                self.plane.pair_off[0] = [false; 2];
                self.plane.ignition = !self.plane.ignition;
                let on = self.plane.ignition;
                self.status = if self.spec.is_jet() {
                    if on { t!("game.fuel_control_switches_run") } else { t!("game.fuel_control_switches_cutoff") }
                } else if on {
                    t!("game.magnetos_on")
                } else {
                    t!("game.magnetos_off")
                }
                .into();
                self.status_until = self.real + 3.0;
            }
            KeyCode::KeyO if self.spec.is_jet() => {
                self.plane.starter_on = true;
                self.status = t!("game.starters_engaged").into();
                self.status_until = self.real + 3.0;
            }
            KeyCode::F4 => {
                let on = self.plane.running || self.plane.starter_on;
                if on { self.plane.stop_engine() } else { self.plane.start_engine() }
                self.status = if on { t!("game.engine_shut_down") } else { t!("game.starting_the_engine") }.into();
                self.status_until = self.real + 3.0;
            }
            KeyCode::KeyX => {
                self.radios.baro = None;
                self.status = t!("game.altimeter_set_to_the_qnh", self.wx.qnh);
                self.status_until = self.real + 3.0;
            }
            KeyCode::Period => {
                self.plane.parking_brake = !self.plane.parking_brake;
                self.status = if self.plane.parking_brake { t!("game.parking_brake_set") } else { t!("game.parking_brake_released") }.into();
                self.status_until = self.real + 2.0;
            }
            KeyCode::F5 if !self.crashed => self.ap_toggle(),
            KeyCode::Digit7 if !self.crashed => self.ap_lat(autopilot::Lat::Hdg),
            KeyCode::Digit8 if !self.crashed => self.ap_lat(autopilot::Lat::Nav),
            KeyCode::Digit9 if !self.crashed => self.ap_lat(autopilot::Lat::Apr),
            KeyCode::Digit0 if !self.crashed => self.ap_vert(autopilot::Vert::Alt),
            KeyCode::Minus | KeyCode::Equal => {
                let d = if code == KeyCode::Minus { -5.0 } else { 5.0 };
                self.ap.hdg = (self.ap.hdg + d).rem_euclid(360.0);
                self.status = t!("game.heading_bug", self.ap.hdg);
                self.status_until = self.real + 2.0;
            }
            KeyCode::Semicolon | KeyCode::Quote => {
                let d = if code == KeyCode::Semicolon { -100.0 } else { 100.0 };
                self.ap.alt = (((self.ap.alt * 3.28084 / 100.0).round() * 100.0 + d).max(0.0)) / 3.28084;
                self.status = t!("game.altitude_preselect_ft", self.ap.alt * 3.28084);
                self.status_until = self.real + 2.0;
            }
            KeyCode::KeyK if !self.crashed => {
                self.gauges.sync_dg(&self.plane);
                self.status = t!("game.heading_indicator_set_to_the").into();
                self.status_until = self.real + 2.0;
            }
            KeyCode::KeyU => {
                self.sound_on = !self.sound_on;
                self.status = if self.sound_on { t!("game.sound_on") } else { t!("game.sound_off") }.into();
                self.status_until = self.real + 2.0;
            }
            KeyCode::F1 => self.show_help = !self.show_help,
            KeyCode::F2 => {
                    self.gfx.taa = !self.gfx.taa;
                    self.gfx.save();
                }
            KeyCode::F12 => {
                let p = PathBuf::from(format!("screenshot-{}.png", self.frame));
                self.shots.insert(0, (0.0, p, None));
            }
            _ => {}
        }
    }

    /// Input for the UI, minus Tab: that opens and closes the menu here, not
    /// egui's keyboard focus (which would then keep keys from the aircraft).
    pub(crate) fn egui_input(&mut self) -> egui::RawInput {
        // shrink the ui to fit short windows instead of letting panels overlap.
        let native = self.window.scale_factor() as f32;
        let h = self.window.inner_size().height as f32;
        let ppp = (h / UI_MIN_H).clamp(0.75, native.max(0.75));
        let zoom = ppp / native;
        if (self.egui_ctx.zoom_factor() - zoom).abs() > 1e-3 {
            self.egui_ctx.set_zoom_factor(zoom);
        }
        let mut raw = self.egui_state.take_egui_input(&self.window);
        if !self.egui_ctx.text_edit_focused() {
            raw.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Tab, .. }));
        }
        raw.events.append(&mut self.pad_events);
        // --cursor points the UI too, for scripted screenshots of it.
        if let Some((x, y)) = self.script_cursor {
            raw.events.push(egui::Event::PointerMoved(egui::pos2(x as f32 / ppp, y as f32 / ppp)));
        }
        // The pad works the game whether or not the window has the keyboard,
        // and egui only lets a focused slider take the arrows in a focused window.
        if padui::active() {
            raw.focused = true;
        }
        raw
    }

    /// At the start of a UI pass: a gamepad move out of a text box (the box
    /// itself would keep the arrow keys).
    /// Text boxes are passed over: a pad cannot type, and a slider's number
    /// box would otherwise keep left and right from the slider. (A widget
    /// only shows as a text box the pass after it takes the focus.)
    pub(crate) fn pad_pass(&mut self, ctx: &egui::Context) {
        padui::begin_pass();
        if let Some(d) = self.pad_move.take() {
            ctx.memory_mut(|m| m.move_focus(d));
        }
        if let Some((d, stage, hops)) = self.pad_skip {
            self.pad_skip = if stage < 2 {
                Some((d, stage + 1, hops))
            } else if ctx.text_edit_focused() && hops < 6 {
                ctx.memory_mut(|m| m.move_focus(d));
                Some((d, 1, hops + 1))
            } else {
                None
            };
        }
    }

    /// Whether this is the widget to pick out for the gamepad (once asked for).
    pub(crate) fn pad_home_focus(&mut self, resp: &egui::Response) {
        if self.pad_home && padui::active() {
            resp.request_focus();
            self.pad_home = false;
        }
    }
}
