//! The winit application: the window, its events, and the fonts egui draws with.

use crate::*;

pub(crate) struct App {
    pub(crate) args: Args,
    pub(crate) game: Option<Game>,
    pub(crate) frame_pacer: frame_pacer::FramePacer,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.game.is_some() {
            return;
        }
        progress::step("window");
        let title = if self.args.bench.is_some() { "Fable Flight: graphics test" } else { "Fable Flight" };
        // full screen unless windowed or scripted.
        let scripted = self.args.headless || self.args.bench.is_some() || std::env::args().any(|a| a == "--shot" || a == "--record");
        let windowed = self.args.windowed.unwrap_or(scripted || !settings::Settings::load().fullscreen);
        let mut attrs = Window::default_attributes().with_title(title).with_inner_size(winit::dpi::PhysicalSize::new(self.args.size.0, self.args.size.1));
        if !windowed {
            attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        }
        let window = Arc::new(el.create_window(attrs).expect("window"));
        match Game::new(window, &self.args) {
            Ok(g) => self.game = Some(g),
            Err(e) => {
                eprintln!("failed to start: {e:#}");
                el.exit();
            }
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _device_id: winit::event::DeviceId, event: DeviceEvent) {
        let Some(g) = self.game.as_mut() else { return };
        if !g.mouse_captured {
            return;
        }
        if let DeviceEvent::MouseMotion { delta } = event {
            let (sx, sy) = g.look_signs();
            let k = g.look_sens * g.head.zoom;
            g.cam_yaw += delta.0 * 0.005 * k * sx;
            g.cam_pitch = (g.cam_pitch + delta.1 * 0.004 * k * sy).clamp(-1.2, 1.4);
            g.pad_aim = false;
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(g) = self.game.as_mut() else { return };
        let consumed = g.egui_state.on_window_event(&g.window, &event).consumed;
        // A look drag ends on any release, even one the UI took (the flight menu
        // or a panel under the pointer), and when the window loses focus;
        // otherwise the view keeps turning with the mouse after the button is up.
        if matches!(event, WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left | MouseButton::Right, .. } | WindowEvent::Focused(false)) {
            g.drag = false;
        }
        match event {
            WindowEvent::CloseRequested => {
                g.end_flight(false);
                g.keep_settings(true);
                if !std::env::args().any(|a| a == "--headless" || a == "--shot" || a == "--record" || a == "--bench") { g.src.google.save(); }
                // Dropped here, while the event loop (and with it the Wayland
                // connection) is still alive: egui's clipboard thread tears
                // down its Wayland objects on drop, and if the game outlived
                // `run_app` that would be on an already freed display.
                self.game = None;
                std::process::exit(0);
            }
            WindowEvent::Resized(s) => g.r.resize(s.width, s.height),
            WindowEvent::RedrawRequested => {
                // The OS can also request redraws before our frame deadline.
                let now = Instant::now();
                if self.frame_pacer.control_flow(now, g.gfx.fps_cap) != ControlFlow::Poll {
                    return;
                }
                self.frame_pacer.frame_started(now);
                g.frame();
                if g.frame == 1 {
                    progress::step("ready");
                }
                if g.quit {
                    g.end_flight(false);
                    g.keep_settings(true);
                    if !std::env::args().any(|a| a == "--headless" || a == "--shot" || a == "--record" || a == "--bench") { g.src.google.save(); }
                    // Worker threads may be mid-download, so the process just
                    // exits; but the GPU device is torn down first: exiting
                    // under the driver's shader compiler threads can crash it.
                    self.game = None;
                    std::process::exit(0);
                }
            }
            WindowEvent::Focused(false) => {
                g.release_mouse_capture();
                g.cancel_cockpit_input();
                g.keys.clear();
                (g.pad_y, g.pad_y_combo) = (false, false);
                if g.mode == Mode::Fly && !g.crashed && !g.r.headless && g.record.is_none() && g.shots.is_empty() && g.bench.is_none() {
                    g.set_pause_menu(true);
                }
            }
            WindowEvent::KeyboardInput { event: k, .. } => {
                if let PhysicalKey::Code(raw) = k.physical_key {
                    // Waiting for a key (or Esc) to bind, or Esc calling off a joystick capture.
                    let pressed = k.state == ElementState::Pressed;
                    let capturing = match g.binds.capture.clone() {
                        Some(bind::Capture::Key(id)) if pressed => {
                            if raw == KeyCode::Escape {
                                g.binds.capture = None;
                            } else if bind::bindable(raw) {
                                g.binds.capture = None;
                                g.binds.set_key(id, raw);
                            }
                            g.keys.clear();
                            true
                        }
                        Some(_) if pressed && raw == KeyCode::Escape => {
                            g.binds.capture = None;
                            true
                        }
                        _ => false,
                    };
                    let mapped = if capturing { None } else { g.binds.map_key(raw) };
                    if let Some(code) = mapped {
                    if k.state == ElementState::Pressed {
                        // egui always claims Tab (and, while one of its buttons has
                        // focus, every key), so the menu keys are let through here.
                        let typing = g.goto_open || g.egui_ctx.text_edit_focused();
                        let menu_key = code == KeyCode::Tab || code == KeyCode::Escape || code == KeyCode::F11;
                        // A key while the gamepad has a button of the flight panel picked
                        // out: the keyboard flies again (egui would keep the keys).
                        let mut consumed = consumed;
                        if consumed && g.mode == Mode::Fly && !g.pause_open && !typing {
                            g.egui_ctx.memory_mut(|m| {
                                if let Some(id) = m.focused() {
                                    m.surrender_focus(id);
                                }
                            });
                            consumed = false;
                        }
                        if menu_key && !typing {
                            if !k.repeat {
                                g.key(code);
                            }
                        } else if !(consumed || g.goto_open) {
                            if !k.repeat {
                                g.key(code);
                            }
                            g.keys.insert(code);
                        }
                    } else {
                        g.keys.remove(&code);
                        // tab held, pointed and let go picks from the flight menu.
                        if code == KeyCode::Tab {
                            if let Some(b) = g.flight_menu_release() {
                                g.toolbar_act(b);
                            }
                        }
                    }
                    }
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } if g.mode == Mode::Menu => {
                if g.menu.screen != menu::Screen::Fly {
                    g.menu.press = None;
                    return;
                }
                let size = g.window.inner_size();
                let (w, h) = (size.width.max(1) as f64, size.height.max(1) as f64);
                if state == ElementState::Pressed {
                    if !consumed {
                        g.menu.press = g.last_cursor;
                        g.menu.dragged = 0.0;
                    }
                } else if let Some((x, y)) = g.menu.press.take() {
                    // A click (not a drag) chooses the spot; a second click on it starts.
                    // shift or add mode adds a waypoint.
                    let shift = g.egui_ctx.input(|i| i.modifiers.shift);
                    if g.menu.dragged < 6.0 && (g.plan_pick || shift) {
                        g.plan_add_at(x, y, w, h, 14.0);
                    } else if g.menu.dragged < 6.0 {
                        if let Some(i) = g.menu.marker_at(x, y) {
                            let double = g.real - g.last_click < 0.45;
                            g.last_click = g.real;
                            if double && g.menu.airport == Some(i) {
                                g.begin();
                            } else {
                                g.choose_airport(i);
                            }
                        } else if let Some((lat, lon)) = g.menu.pick_at(x, y, w, h, 60f64.to_radians()) {
                            let double = g.real - g.last_click < 0.45;
                            g.last_click = g.real;
                            if double && g.menu.pick.is_some() {
                                g.begin();
                            } else {
                                g.pick(lat, lon);
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Middle, .. }
                if g.lean_drag.is_some() || (!consumed && g.mode == Mode::Fly && g.cam_mode == 1 && !g.pause_open && !g.crashed) => {
                if state == ElementState::Pressed {
                    g.lean_drag = g.last_cursor;
                } else if let (Some((sx, sy)), Some((x, y))) = (g.lean_drag.take(), g.last_cursor) {
                    if (sx - x).hypot(sy - y) < 3.0 { g.head.center_seat(); }
                }
            }
            // In the cockpit, a click on a control works it (see `hot_click`).
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } if if state == ElementState::Pressed { !consumed && g.hotspot_under_mouse().is_some() } else { g.hot_press.is_some() } => {
                g.hot_click(state == ElementState::Pressed);
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left | MouseButton::Right, .. } if !consumed => {
                g.drag = state == ElementState::Pressed;
                g.last_cursor = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if g.mouse_captured {
                    let size = g.window.inner_size();
                    let centre = (size.width as f64 * 0.5, size.height as f64 * 0.5);
                    g.cursor = Some(centre);
                    g.last_cursor = Some(centre);
                    let _ = g.window.set_cursor_position(PhysicalPosition::new(centre.0, centre.1));
                } else if g.mode == Mode::Menu {
                    if let (Some(_), Some((x, y))) = (g.menu.press, g.last_cursor) {
                        let h = g.window.inner_size().height.max(1) as f64;
                        g.menu.drag(position.x - x, position.y - y, h, 60f64.to_radians());
                    }
                } else if g.lean_drag.is_some() && !g.pause_open {
                    if let Some((x, y)) = g.last_cursor {
                        let scale = 0.85 / g.window.inner_size().height.max(1) as f64;
                        g.head.lean_by((position.x - x) * scale, (y - position.y) * scale);
                    }
                } else if g.drag && !g.pause_open && g.cam_mode != settings::TAIL_VIEW {
                    if let Some((x, y)) = g.last_cursor {
                        // Zoomed in, the view turns slower under the mouse.
                        let (sx, sy) = g.look_signs();
                        let k = g.look_sens * if g.cam_mode == 1 { g.head.zoom } else { 1.0 };
                        g.cam_yaw += (position.x - x) * 0.005 * k * sx;
                        g.cam_pitch = (g.cam_pitch + (position.y - y) * 0.004 * k * sy).clamp(-1.2, 1.4);
                    }
                }
                // The mouse moved (not just a twitch): it points at the controls again, not the pad.
                if g.cursor.is_none_or(|(x, y)| (position.x - x).abs() + (position.y - y).abs() > 3.0) {
                    g.pad_aim = false;
                }
                g.last_cursor = Some((position.x, position.y));
                // (--cursor pins it, for scripted screenshots.)
                if g.script_cursor.is_none() {
                    g.cursor = Some((position.x, position.y));
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !consumed => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                if g.mode == Mode::Menu {
                    if g.menu.screen == menu::Screen::Fly {
                        g.menu.zoom(d);
                    }
                } else if g.cam_mode == 1 && !g.crashed && g.hot_wheel(d) {
                    // (A cockpit control under the mouse took it.)
                } else if g.mouse_yoke && g.mode == Mode::Fly && !g.crashed {
                    g.yoke_thr += d * 0.05;
                } else if g.cam_mode == 1 && !g.crashed {
                    g.head.zoom_by(d);
                } else {
                    g.cam_dist = (g.cam_dist * (1.0 - d * 0.1)).clamp(6.0, 400.0);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(g) = &mut self.game {
            // Answer tool calls even when redraws are delayed; screenshots wait for a frame.
            g.pump_mcp();
            let flow = self.frame_pacer.control_flow(Instant::now(), g.gfx.fps_cap);
            if flow == ControlFlow::Poll {
                g.window.request_redraw();
            }
            event_loop.set_control_flow(flow);
        }
    }
}

/// The UI library ships no fonts of its own here, so borrow the system's.
pub(crate) fn load_fonts(ctx: &egui::Context) {
    // Each candidate is "path" or "path#face", the face picking one font out of a
    // .ttc collection. fontconfig's index carries the named instance of a variable
    // font in its upper 16 bits; only the low 16 are the face.
    let find = |kind: &str, fallbacks: &[&str]| -> Option<(Vec<u8>, u32)> {
        let fc = std::process::Command::new("fc-match").args(["-f", "%{file}#%{index}", kind]).output().ok().and_then(|o| String::from_utf8(o.stdout).ok());
        fc.iter().map(|s| s.trim().to_string()).chain(fallbacks.iter().map(|s| s.to_string())).find_map(|c| {
            let (path, face) = match c.rsplit_once('#') {
                Some((p, i)) => (p.to_string(), i.parse::<u32>().ok()? & 0xFFFF),
                None => (c.clone(), 0),
            };
            let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
                return None;
            }
            std::fs::read(&path).ok().map(|d| (d, face))
        })
    };
    let mut fonts = egui::FontDefinitions::empty();
    // fontconfig's answer where there is one (Linux), then each system's own fonts.
    let mono: &[&str] = &[
        "/usr/share/fonts/google-noto/NotoSansMono-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
        "/System/Library/Fonts/SFNSMono.ttf",
        "/System/Library/Fonts/Monaco.ttf",
        "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
        "/System/Library/Fonts/Supplemental/Courier New.ttf",
        "C:/Windows/Fonts/consola.ttf",
        "C:/Windows/Fonts/cour.ttf",
    ];
    let sans: &[&str] = &[
        "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Geneva.ttf",
        "/Library/Fonts/Arial Unicode.ttf",
        "C:/Windows/Fonts/segoeui.ttf",
        "C:/Windows/Fonts/arial.ttf",
    ];
    // A Korean (Hangul) face, for the Korean interface. The bundled Barlow and
    // the Latin system fonts carry no Hangul, so without this Korean text would
    // render as empty boxes. Noto Sans CJK collections hold JP, KR, SC, TC, HK
    // in that order, so "#1" picks the Korean face.
    let korean: &[&str] = &[
        "/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJK-VF.ttc#1",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc#1",
        "/usr/share/fonts/opentype/noto-cjk/NotoSansCJK-Regular.ttc#1",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc#1",
        "/usr/share/fonts/google-noto/NotoSansKR-Regular.otf",
        "/usr/share/fonts/opentype/noto/NotoSansKR-Regular.otf",
        "/usr/share/fonts/truetype/noto/NotoSansKR-Regular.ttf",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        "/Library/Fonts/AppleGothic.ttf",
        "C:/Windows/Fonts/malgun.ttf",
    ];
    // our bundled faces first, then the system's for missing glyphs.
    let own: [(&str, &'static [u8]); 5] = [("barlow", menu::FONT_REGULAR), ("barlow-medium", menu::FONT_MEDIUM), ("barlow-semibold", menu::FONT_SEMIBOLD), ("barlow-condensed", menu::FONT_DISPLAY), ("plex-mono", menu::FONT_MONO)];
    for (name, data) in own {
        fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_static(data)));
    }
    let families = [
        (egui::FontFamily::Proportional, "barlow"),
        (egui::FontFamily::Monospace, "plex-mono"),
        (egui::FontFamily::Name(menu::MEDIUM.into()), "barlow-medium"),
        (egui::FontFamily::Name(menu::SEMIBOLD.into()), "barlow-semibold"),
        (egui::FontFamily::Name(menu::BOLD.into()), "barlow-condensed"),
    ];
    for (family, name) in &families {
        fonts.families.entry(family.clone()).or_default().push((*name).into());
    }
    for (name, kind, fb) in [("mono", "monospace", mono), ("sans", "sans", sans), ("korean", "sans:lang=ko", korean)] {
        if let Some((data, face)) = find(kind, fb) {
            let mut fd = egui::FontData::from_blob(Arc::new(data), face);
            // A variable font starts at its default instance, which for Noto Sans
            // CJK VF is Thin (wght 100); ask for Regular instead.
            if fd.variation_axes().iter().any(|a| a.tag.to_be_bytes() == *b"wght") {
                fd.tweak.coords.push(b"wght", 400.0);
            }
            fonts.font_data.insert(name.into(), Arc::new(fd));
        }
    }
    for (family, _) in families {
        // Korean is the last fallback everywhere so Latin keeps the bundled face
        // and only the Hangul glyphs come from the Korean font.
        let fallback = if family == egui::FontFamily::Monospace { ["mono", "sans", "korean"] } else { ["sans", "mono", "korean"] };
        for f in fallback {
            if fonts.font_data.contains_key(f) {
                fonts.families.entry(family.clone()).or_default().push(f.into());
            }
        }
    }
    ctx.set_fonts(fonts);
}
