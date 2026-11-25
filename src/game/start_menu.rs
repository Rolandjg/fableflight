//! The start screen's interface (the globe, the hangar, the menus).

use crate::*;

impl Game {
    /// The start screen drawn over the globe: labels, the chosen spot, and the panel.
    pub(crate) fn menu_ui(&mut self, vp: Mat4, cam: DVec3, w: f32, h: f32) -> (Vec<egui::ClippedPrimitive>, egui::TexturesDelta, f32) {
        let raw = self.egui_input();
        let mut go: Option<String> = None;
        let mut start = false;
        let mut jump: Option<(f64, f64, &'static str)> = None;
        let mut airport: Option<u32> = None;
        let alt = self.menu.alt;
        self.src.google.update_attribution(self.menu.lat, self.menu.lon, alt);
        let out = self.egui_ctx.clone().run_ui(raw, |root| {
            let ctx = &root.ctx().clone();
            self.pad_pass(ctx);
            // The main menu, the aircraft and the settings screens each take
            // the whole window; the rest of this draws the globe's.
            match self.menu.screen {
                menu::Screen::Main => {
                    self.main_menu(ctx);
                    padui::focus_ring(ctx, menu::ACCENT);
                    return;
                }
                menu::Screen::Settings => {
                    if settings_ui::show(ctx, self, false) {
                        self.menu.screen = self.menu.set_back;
                        self.pad_home = true;
                    }
                    padui::focus_ring(ctx, menu::ACCENT);
                    return;
                }
                menu::Screen::Aircraft => {
                    self.aircraft_screen(ctx);
                    padui::focus_ring(ctx, menu::ACCENT);
                    return;
                }
                menu::Screen::Fly => {}
            }
            use egui::{Align2, Color32, FontId, RichText};
            let ppp = ctx.pixels_per_point();
            // With a gamepad, X starts at the crosshair in the middle of the screen.
            let aim = padui::active().then(|| egui::pos2(w * 0.5 / ppp, h * 0.5 / ppp));
            // Screen position of a place, if it is on the near side of the planet.
            let project = |lat: f64, lon: f64| -> Option<egui::Pos2> {
                let p = geo::lla_to_pos(lat.to_radians(), lon.to_radians(), 0.0);
                if p.normalize().dot((cam - p).normalize()) < 0.05 {
                    return None;
                }
                let c = vp * (p - cam).as_vec3().extend(1.0);
                if c.w <= 0.0 {
                    return None;
                }
                let (x, y) = ((c.x / c.w * 0.5 + 0.5) * w, (0.5 - c.y / c.w * 0.5) * h);
                (x > -50.0 && y > -50.0 && x < w + 50.0 && y < h + 50.0).then(|| egui::pos2(x / ppp, y / ppp))
            };
            let painter = ctx.layer_painter(egui::LayerId::background());
            let label = |p: egui::Pos2, text: &str, size: f32, col: Color32| {
                painter.text(p + egui::vec2(1.0, 1.0), Align2::LEFT_CENTER, text, menu::medium(size), Color32::from_black_alpha(200));
                painter.text(p, Align2::LEFT_CENTER, text, menu::medium(size), col);
            };
            // (where the other players are on screen: city names give way to theirs)
            let mp_at: Vec<egui::Pos2> = self.mp_world.iter().filter_map(|sp| project(sp.lat_deg(), sp.lon_deg())).collect();
            if alt > 25_000.0 {
                // cities next to the start marker give way to its label.
                let pick_at = self.menu.pick.and_then(|(la, lo)| project(la, lo));
                for (name, lat, lon, tier) in menu::CITIES {
                    if *tier > 1 && alt > 7.0e6 {
                        continue;
                    }
                    let clear = |p: &egui::Pos2| pick_at.is_none_or(|k| (*p - k).length() > 70.0) && mp_at.iter().all(|m| (m.y - p.y).abs() > 16.0 || (m.x - p.x).abs() > 90.0);
                    if let Some(p) = project(*lat, *lon).filter(clear) {
                        painter.circle_filled(p, 2.5, Color32::from_rgba_unmultiplied(255, 255, 255, 220));
                        label(p + egui::vec2(7.0, 0.0), tr_en(name), if *tier == 1 { 14.0 } else { 12.5 }, Color32::from_rgba_unmultiplied(255, 255, 255, 225));
                    }
                }
            }
            // Airports, once zoomed in far enough to tell them apart. Each one can be clicked.
            self.menu.markers.clear();
            if alt < 450_000.0 {
                let db = airports::Db::get();
                let mut n = 0;
                for (i, _) in db.near(self.menu.lat, self.menu.lon, (alt * 1.6).min(600_000.0)) {
                    let a = &db.apts[i as usize];
                    if a.kind < 1 && alt > 60_000.0 || a.kind < 2 && alt > 200_000.0 || !a.any_paved() || n >= 80 {
                        continue;
                    }
                    if let Some(p) = project(a.lat, a.lon) {
                        n += 1;
                        self.menu.markers.push((i, p.x * ppp, p.y * ppp));
                        let chosen = self.menu.airport == Some(i);
                        let hover = ctx.pointer_hover_pos().is_some_and(|h| (h - p).length() < 14.0 / ppp) || aim.is_some_and(|c| (c - p).length() < 28.0);
                        let col = if chosen || hover { menu::ACCENT } else if a.kind == 2 { Color32::from_rgb(120, 200, 255) } else { Color32::from_rgb(170, 200, 220) };
                        if hover {
                            painter.circle_stroke(p, 8.0, egui::Stroke::new(1.5, col));
                            label(p + egui::vec2(if chosen { 18.0 } else { 6.0 }, 16.0), &a.name.chars().take(40).collect::<String>(), 13.0, col);
                        }
                        painter.rect_filled(egui::Rect::from_center_size(p, egui::vec2(5.0, 5.0)), 1.0, col);
                        // the chosen airport is named by the start marker.
                        if !chosen {
                            label(p + egui::vec2(6.0, 0.0), &a.ident, if a.kind == 2 { 13.0 } else { 12.0 }, col);
                        }
                    }
                }
            }
            // The other players (multiplayer, connected): an arrow each, the
            // way they fly, with their name; more about them under the mouse.
            if !self.mp_world.is_empty() {
                let col = MP_COLOUR;
                let shade = Color32::from_black_alpha(170);
                let d = (alt / 111_000.0 * 0.03).clamp(0.002, 0.6);
                let crowd = self.mp_world.len() > 24 && alt > 3.0e6;
                // (a name that would overlap one already written is left to the mouse)
                let mut named: Vec<egui::Rect> = Vec::new();
                for sp in &self.mp_world {
                    let (la, lo) = (sp.lat_deg(), sp.lon_deg());
                    let Some(p) = project(la, lo) else { continue };
                    let h = sp.heading_deg().to_radians();
                    let ahead = project(la + d * h.cos(), lo + d * h.sin() / la.to_radians().cos().max(0.05));
                    let dir = ahead.map(|a| a - p).filter(|v| v.length() > 0.05).map(|v| v.normalized()).unwrap_or(egui::vec2(0.0, -1.0));
                    let side = egui::vec2(-dir.y, dir.x);
                    let arrow = |k: f32| vec![p + dir * 8.0 * k, p - dir * 5.0 * k + side * 5.5 * k, p - dir * 2.0 * k, p - dir * 5.0 * k - side * 5.5 * k];
                    let hover = ctx.pointer_hover_pos().is_some_and(|m| (m - p).length() < 12.0) || aim.is_some_and(|c| (c - p).length() < 20.0);
                    let k = if hover { 1.35 } else { 1.0 };
                    painter.add(egui::Shape::closed_line(arrow(k), egui::Stroke::new(3.0, shade)));
                    // (two triangles: the arrowhead's notch makes it concave)
                    let a = arrow(k);
                    painter.add(egui::Shape::convex_polygon(vec![a[0], a[1], a[2]], col, egui::Stroke::NONE));
                    painter.add(egui::Shape::convex_polygon(vec![a[0], a[2], a[3]], col, egui::Stroke::NONE));
                    let at = p + egui::vec2(11.0, 0.0);
                    let room = egui::Rect::from_min_size(at - egui::vec2(0.0, 9.0), egui::vec2(9.0 * sp.name.chars().count() as f32 + 8.0, 18.0));
                    if hover || (!crowd && named.iter().all(|r| !r.intersects(room))) {
                        label(at, &sp.name, 13.0, col);
                        named.push(room);
                    }
                    if hover {
                        let ac = mp_proto::AIRCRAFT.get(sp.aircraft as usize).copied().unwrap_or("?");
                        let how = if sp.flags & mp_proto::F_GROUND != 0 { t!("game.on_the_ground").to_string() } else { format!("{:.0} ft  ·  {} kt  ·  {:03.0}°", sp.alt_m() * 3.28084, sp.speed, sp.heading_deg()) };
                        label(p + egui::vec2(11.0, 15.0), &format!("{ac}  ·  {how}"), 11.5, Color32::from_rgba_unmultiplied(255, 255, 255, 220));
                    }
                }
            }
            // flight plan route.
            if !self.plan.is_empty() {
                let shade = egui::Stroke::new(4.5, Color32::from_black_alpha(120));
                let line = egui::Stroke::new(2.5, ROUTE_COLOUR);
                for (a, b, d, _) in self.plan.legs(self.menu.pick) {
                    let n = (d / 40_000.0).clamp(2.0, 96.0) as usize;
                    let pts: Vec<Option<egui::Pos2>> = (0..=n)
                        .map(|k| {
                            let (la, lo) = flightplan::between(a, b, k as f64 / n as f64);
                            project(la, lo)
                        })
                        .collect();
                    for st in [shade, line] {
                        for w in pts.windows(2) {
                            if let (Some(p0), Some(p1)) = (w[0], w[1]) {
                                painter.line_segment([p0, p1], st);
                            }
                        }
                    }
                }
                for (i, wp) in self.plan.wps.iter().enumerate() {
                    if let Some(p) = project(wp.lat, wp.lon) {
                        painter.circle_filled(p, 9.0, Color32::from_rgb(20, 14, 30));
                        painter.circle_stroke(p, 9.0, egui::Stroke::new(2.0, ROUTE_COLOUR));
                        painter.text(p, Align2::CENTER_CENTER, format!("{}", i + 1), menu::bold(12.0), Color32::WHITE);
                        label(p + egui::vec2(13.0, -1.0), &wp.ident, 13.5, ROUTE_COLOUR);
                    }
                }
            }
            if let Some((lat, lon)) = self.menu.pick {
                if let Some(p) = project(lat, lon) {
                    let accent = menu::ACCENT;
                    let pulse = 11.0 + 3.0 * (self.real * 3.0).sin() as f32;
                    painter.circle_stroke(p, pulse, egui::Stroke::new(2.0, accent));
                    painter.circle_filled(p, 3.5, accent);
                    let name = self.menu.airport.map(|i| airports::Db::get().apts[i as usize].ident.clone());
                    label(p + egui::vec2(18.0, 0.0), &name.unwrap_or_else(|| t!("game.start_here").to_string()), 16.0, accent);
                }
            }
            if let Some(c) = aim {
                let st = egui::Stroke::new(1.5, Color32::from_white_alpha(200));
                let sh = egui::Stroke::new(3.0, Color32::from_black_alpha(110));
                for s in [sh, st] {
                    painter.circle_stroke(c, 9.0, s);
                    for d in [egui::vec2(1.0, 0.0), egui::vec2(-1.0, 0.0), egui::vec2(0.0, 1.0), egui::vec2(0.0, -1.0)] {
                        painter.line_segment([c + d * 13.0, c + d * 20.0], s);
                    }
                }
            }
            use menu::{ACCENT, DIM, TEXT};
            const W: f32 = 340.0;
            const M: f32 = 22.0;
            let screen = ctx.content_rect();
            // Top left: back to the main menu, then the aircraft card (hangar_ui.rs).
            let mut back = false;
            egui::Area::new(egui::Id::unique("menu_back")).fixed_pos(egui::pos2(M, M)).show(ctx, |ui| {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(132.0, 34.0), egui::Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, t!("game.menu")));
                let lit = menu::ease(ctx, resp.id, resp.hovered() || resp.has_focus());
                let p = ui.painter();
                p.rect_filled(r, menu::R_WIDGET, menu::mix(menu::PANEL, Color32::from_rgba_premultiplied(40, 47, 58, 238), lit));
                p.rect_stroke(r, menu::R_WIDGET, egui::Stroke::new(1.0, menu::mix(menu::LINE_HI, Color32::from_white_alpha(70), lit)), egui::StrokeKind::Inside);
                menu::chevron(p, r.left_center() + egui::vec2(16.0 - 3.0 * lit, 0.0), 5.5, 2.0, menu::mix(TEXT, ACCENT, lit));
                p.text(r.left_center() + egui::vec2(30.0, 0.0), Align2::LEFT_CENTER, t!("game.menu"), menu::label(), menu::mix(TEXT, Color32::WHITE, lit));
                match Hint::of("Esc", "B") {
                    Hint::Text(h) => {
                        p.text(r.right_center() - egui::vec2(12.0, 0.0), Align2::RIGHT_CENTER, h, FontId::monospace(11.0), DIM);
                    }
                    Hint::Pad(spec) => {
                        let x = r.right() - 10.0 - padui::spec_width(p, spec, 18.0);
                        padui::paint_spec(p, x, r.center().y, spec, 18.0, DIM);
                    }
                    Hint::None => {}
                }
                if resp.clicked() {
                    back = true;
                }
                if self.menu.pick.is_none() {
                    self.pad_home_focus(&resp);
                }
            });
            if back {
                self.menu_back();
            }
            // Top middle, while multiplayer is on: how many fly (they are on
            // the globe), or how the connection stands; it opens its settings.
            if self.mp_on {
                let (col, line) = self.mp_line();
                let (title, sub) = match self.mp_flying() {
                    Some(n) => (n, line),
                    None => (line, String::new()),
                };
                let mut open = false;
                egui::Area::new(egui::Id::unique("menu_mp")).anchor(Align2::CENTER_TOP, [0.0, M]).show(ctx, |ui| {
                    let tg = ui.painter().layout_no_wrap(title.clone(), menu::label(), TEXT);
                    let sg = ui.painter().layout_no_wrap(sub.clone(), menu::caption(), DIM);
                    let w = (tg.size().x.max(sg.size().x) + 52.0).clamp(160.0, (screen.width() - 2.0 * (W + 2.0 * M)).max(160.0));
                    let h = if sub.is_empty() { 34.0 } else { 46.0 };
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, t!("game.multiplayer")));
                    let lit = menu::ease(ctx, resp.id, resp.hovered() || resp.has_focus());
                    let p = ui.painter().with_clip_rect(r);
                    p.rect_filled(r, menu::R_WIDGET, menu::mix(menu::PANEL, Color32::from_rgba_premultiplied(40, 47, 58, 238), lit));
                    p.rect_stroke(r, menu::R_WIDGET, egui::Stroke::new(1.0, menu::mix(menu::LINE_HI, Color32::from_white_alpha(70), lit)), egui::StrokeKind::Inside);
                    p.circle_filled(egui::pos2(r.left() + 18.0, r.center().y), 4.0, col);
                    if sub.is_empty() {
                        p.galley(egui::pos2(r.left() + 32.0, r.center().y - tg.size().y * 0.5), tg, menu::mix(TEXT, Color32::WHITE, lit));
                    } else {
                        p.galley(egui::pos2(r.left() + 32.0, r.top() + 6.0), tg, menu::mix(TEXT, Color32::WHITE, lit));
                        p.galley(egui::pos2(r.left() + 32.0, r.bottom() - 6.0 - sg.size().y), sg, DIM);
                    }
                    if resp.clicked() {
                        open = true;
                    }
                });
                if open {
                    self.open_settings(settings_ui::Section::Multiplayer, menu::Screen::Fly);
                }
            }
            let r = self.aircraft_window(ctx, W, egui::pos2(M, M + 34.0 + 10.0));

            // Below it: where to start. Search, places and airports fold away;
            // the chosen spot and take-off stay in view.
            let top = r.bottom() + 12.0;
            // room for the pinned start point and take off, as last frame.
            let foot_h = ctx.data(|d| d.get_temp::<f32>(egui::Id::unique("globe_pinned_h"))).unwrap_or(150.0);
            // the list gives up height first so the panel never overlaps the aircraft card.
            let body_h = (screen.bottom() - M - top - 40.0 - 28.0 - foot_h).max(48.0);
            egui::Area::new(egui::Id::unique("menu")).anchor(Align2::LEFT_TOP, [M, top]).show(ctx, |ui| {
                ui.set_max_height(screen.bottom() - M - top);
                menu::panel().inner_margin(egui::Margin::symmetric(16, 14)).show(ui, |ui| {
                    ui.set_width(W - 32.0);
                    menu::window_title(ui, t!("game.start_location"));
                    egui::ScrollArea::vertical().max_height(body_h).auto_shrink([false, true]).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let summary = if self.menu.search.trim().is_empty() { String::new() } else { self.menu.search.chars().take(18).collect() };
                        menu::section(ui, "search", t!("game.search"), &summary, true, |ui| {
                            ui.horizontal(|ui| {
                                let resp = ui.add(egui::TextEdit::singleline(&mut self.menu.search).desired_width(ui.available_width() - 56.0).hint_text(t!("game.city_address_or_airport_sfo")).margin(egui::vec2(8.0, 6.0)));
                                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                if (ui.add(egui::Button::new(RichText::new(t!("game.go")).font(menu::body())).min_size(egui::vec2(48.0, 28.0))).clicked() || enter) && !self.menu.search.trim().is_empty() {
                                    go = Some(self.menu.search.trim().to_string());
                                }
                            });
                            // Airports matching what is typed.
                            let hits = airports::Db::get().search(&self.menu.search, 5);
                            if !hits.is_empty() {
                                ui.add_space(2.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                                    for i in hits {
                                        let a = &airports::Db::get().apts[i as usize];
                                        let label = format!("{}  {}", a.ident, a.name.chars().take(28).collect::<String>());
                                        if ui.add(egui::Button::new(RichText::new(label).font(menu::caption())).selected(self.menu.airport == Some(i))).clicked() {
                                            airport = Some(i);
                                        }
                                    }
                                });
                            }
                            ui.add_space(2.0);
                            let tip = if padui::active() { t!("game.or_turn_the_globe_left") } else { t!("game.or_drag_the_globe_scroll") };
                            ui.label(RichText::new(tip).font(menu::caption()).color(DIM));
                        });
                        let place = menu::QUICK.iter().find(|q| self.menu.pick == Some((q.1, q.2))).map(|q| tr_en(q.0)).unwrap_or("");
                        menu::section(ui, "places", t!("game.places"), place, false, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                                for (name, lat, lon) in menu::QUICK {
                                    let sel = self.menu.pick == Some((*lat, *lon));
                                    if ui.add(egui::Button::new(RichText::new(tr_en(name)).font(menu::caption())).selected(sel)).clicked() {
                                        jump = Some((*lat, *lon, name));
                                    }
                                }
                            });
                        });
                        let db = airports::Db::get();
                        let apt = self.menu.airport.map(|i| db.apts[i as usize].ident.clone()).unwrap_or_default();
                        menu::section(ui, "airports", t!("game.airports"), &apt, false, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                                for (code, name) in menu::AIRPORTS {
                                    if let Some(i) = db.find(code) {
                                        if ui.add(egui::Button::new(RichText::new(*code).font(menu::data())).selected(self.menu.airport == Some(i))).on_hover_text(tr_en(name)).clicked() {
                                            airport = Some(i);
                                        }
                                    }
                                }
                            });
                            ui.label(RichText::new(if padui::active() { t!("game.or_put_an_airport_square") } else { t!("game.or_click_an_airport_square") }).font(menu::caption()).color(DIM));
                        });
                        padui::follow_focus(ui);
                    });
                    // pinned under the list: the start point, a summary and take off.
                    let pin_top = ui.cursor().top();
                    let db = airports::Db::get();
                    ui.add_space(12.0);
                    menu::heading(ui, t!("game.start_point"));
                    ui.add_space(2.0);
                    match self.menu.pick {
                        Some((lat, lon)) => {
                            let name = if self.menu.pick_name.is_empty() { t!("game.unnamed_spot").to_string() } else { self.menu.pick_name.chars().take(90).collect() };
                            ui.add(egui::Label::new(RichText::new(name).font(menu::item_title()).color(Color32::WHITE)).wrap());
                            ui.label(RichText::new(t!("game.local", menu::latlon(lat, lon), menu::hhmm(self.menu.hour))).font(menu::caption()).color(DIM));
                            ui.add_space(6.0);
                            match self.menu.airport {
                                Some(i) => {
                                    let a = &db.apts[i as usize];
                                    menu::segments(ui, |ui| {
                                        menu::seg(ui, &mut self.menu.start, menu::Start::Runway, t!("game.runway"));
                                        menu::seg(ui, &mut self.menu.start, menu::Start::Approach, t!("game.final"));
                                        menu::seg(ui, &mut self.menu.start, menu::Start::Overhead, t!("game.overhead"));
                                    });
                                    if self.menu.start != menu::Start::Overhead && !a.rwys.is_empty() {
                                        ui.add_space(4.0);
                                        let into_wind = t!("game.into_the_wind");
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(t!("game.runway")).font(menu::label()).color(TEXT));
                                            egui::ComboBox::from_id_salt("start_runway").width(ui.available_width()).selected_text(self.menu.runway.clone().unwrap_or_else(|| into_wind.to_string())).show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.menu.runway, None, into_wind);
                                                for r in &a.rwys {
                                                    for id in r.ident.iter().filter(|id| !id.is_empty()) {
                                                        let what = format!("{id}   {:.0} m{}", r.len, if r.paved { "" } else { t!("game.unpaved") });
                                                        ui.selectable_value(&mut self.menu.runway, Some(id.clone()), what);
                                                    }
                                                }
                                            });
                                        });
                                    }
                                }
                                None => {
                                    // offer the nearest airfield.
                                    let near = db.near(lat, lon, 60_000.0).into_iter().find(|(i, _)| db.apts[*i as usize].any_paved());
                                    if let Some((i, d)) = near {
                                        let a = &db.apts[i as usize];
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(t!("game.nearest_airport_km", a.ident, d / 1000.0)).font(menu::caption()).color(DIM));
                                            if ui.button(t!("game.start_there")).clicked() {
                                                airport = Some(i);
                                            }
                                        });
                                    }
                                }
                            }
                        }
                        None => {
                            ui.label(RichText::new(t!("game.pick_a_place_or_click")).font(menu::body()).color(DIM));
                        }
                    }
                    ui.add_space(14.0);
                    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top() - 7.0, egui::Stroke::new(1.0, menu::LINE));
                    // the whole flight in one line.
                    if self.menu.pick.is_some() {
                        let ground = self.menu.airport.is_some() && self.menu.start != menu::Start::Overhead;
                        // the aircraft is named on its card.
                        let how = if ground {
                            let first = if self.menu.start == menu::Start::Runway { t!("game.runway") } else { t!("game.final") };
                            match &self.menu.runway {
                                Some(rw) => format!("{first} {rw}"),
                                None => t!("game.into_the_wind_2", first),
                            }
                        } else {
                            format!("{:.0} ft · {:03.0}°", (self.menu.start_agl * 3.28084 / 50.0).round() * 50.0, self.heading.to_degrees().rem_euclid(360.0))
                        };
                        let wx = if self.wx.live { t!("game.real_weather").to_string() } else { self.wx.summary().to_lowercase() };
                        let line = format!("{how}  ·  {}  ·  {wx}", menu::hhmm(self.menu.hour));
                        ui.add(egui::Label::new(RichText::new(line).font(menu::caption()).color(TEXT)).wrap());
                        ui.add_space(8.0);
                    }
                    let text = if self.menu.starting { t!("game.preparing_2") } else { t!("game.take_off") };
                    let ready = self.menu.pick.is_some() && !self.menu.starting;
                    let take_off = menu::wide_button(ui, text, if ready { Hint::of("Enter", "Menu") } else { Hint::None }, true, ready);
                    if take_off.clicked() {
                        start = true;
                    }
                    if ready {
                        self.pad_home_focus(&take_off);
                    }
                    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("globe_pinned_h"), ui.cursor().top() - pin_top));
                });
            });

            // On the right: how the flight starts, the weather, the time.
            let right_h = (screen.height() - M - 64.0 - 28.0).max(200.0);
            egui::Area::new(egui::Id::unique("menu_setup")).anchor(Align2::RIGHT_TOP, [-M, M]).show(ctx, |ui| {
                // (an area lays out in last frame's size: let it grow as sections open)
                ui.set_max_height(right_h);
                menu::panel().inner_margin(egui::Margin::symmetric(16, 14)).show(ui, |ui| {
                    ui.set_width(320.0);
                    menu::window_title(ui, t!("game.conditions"));
                    egui::ScrollArea::vertical().max_height(right_h - 40.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().slider_width = 116.0;
                        let ground = self.menu.airport.is_some() && self.menu.start != menu::Start::Overhead;
                        let flight = if ground {
                            (if self.menu.start == menu::Start::Runway { t!("game.on_the_runway") } else { t!("game.final_approach") }).to_string()
                        } else {
                            format!("{:.0} ft · {:03.0}°", (self.menu.start_agl * 3.28084 / 50.0).round() * 50.0, self.heading.to_degrees().rem_euclid(360.0))
                        };
                        self.plan_section(ui);
                        menu::section(ui, "flight", t!("game.flight"), &flight, false, |ui| {
                            if ground {
                                ui.label(RichText::new(if self.menu.start == menu::Start::Runway { t!("game.starting_on_the_runway_ready") } else { t!("game.starting_on_a_3_final") }).font(menu::caption()).color(DIM));
                            } else {
                                ui.label(RichText::new(t!("game.starting_in_the_air")).font(menu::caption()).color(DIM));
                            }
                            ui.horizontal(|ui| {
                                ui.label(t!("game.fuel"));
                                let kg = self.spec.fuel_kg;
                                ui.add(egui::Slider::new(&mut self.menu.fuel, 0.05..=1.0).custom_formatter(move |v, _| format!("{:.0}%  ·  {:.0} kg", v * 100.0, v * kg)));
                            });
                            let [usual, most, _] = self.spec.payload;
                            if most > usual {
                                ui.horizontal(|ui| {
                                    ui.label(t!("game.payload"));
                                    let mut f = self.menu.payload.unwrap_or(usual / most);
                                    if ui.add(egui::Slider::new(&mut f, 0.15..=1.0).custom_formatter(move |v, _| format!("{:.0} kg", v * most))).changed() {
                                        self.menu.payload = Some(f);
                                    }
                                });
                            }
                            {
                                // Mass and balance as it will be loaded.
                                let mut probe = Plane::spawn(self.spec, 0.0, 0.0, 1000.0, 0.0, 50.0);
                                probe.fuel = self.spec.fuel_kg * self.menu.fuel;
                                probe.payload = self.start_payload();
                                let s = self.spec;
                                let out = probe.cg_excess() != 0.0;
                                let text = t!("game.kg_cg_mac_limits", probe.mass(), probe.cg() * 100.0, s.cg[1] * 100.0, s.cg[2] * 100.0, if out { t!("game.out_of_limits") } else { "" });
                                ui.label(RichText::new(text).font(menu::caption()).color(if out { Color32::from_rgb(255, 120, 70) } else { DIM }));
                            }
                            if ground {
                                ui.checkbox(&mut self.menu.cold, t!("game.engine_off_cold_start_f4"));
                            }
                            ui.add_enabled_ui(!ground, |ui| {
                                egui::Grid::new("start_air").num_columns(2).min_col_width(92.0).spacing([10.0, 5.0]).show(ui, |ui| {
                                    ui.label(t!("game.height"));
                                    let mut ft = self.menu.start_agl * 3.28084;
                                    if ui.add(egui::Slider::new(&mut ft, 100.0..=15000.0).logarithmic(true).step_by(50.0).suffix(" ft").custom_formatter(|v, _| format!("{:.0}", (v / 50.0).round() * 50.0))).changed() {
                                        self.menu.start_agl = ft / 3.28084;
                                    }
                                    ui.end_row();
                                    ui.label("");
                                    ui.label(RichText::new(t!("game.above_the_ground")).font(menu::caption()).weak());
                                    ui.end_row();
                                    ui.label(t!("game.heading"));
                                    let mut hd = self.heading.to_degrees().rem_euclid(360.0);
                                    if ui.add(egui::Slider::new(&mut hd, 0.0..=359.0).step_by(1.0).custom_formatter(|v, _| format!("{v:03.0}°"))).changed() {
                                        self.heading = hd.to_radians();
                                    }
                                    ui.end_row();
                                    if !ground {
                                        ui.label("");
                                        menu::heading_compass(ui, &mut self.heading);
                                        ui.end_row();
                                    }
                                });
                            });
                        });
                        let wx = self.wx.summary();
                        menu::section(ui, "weather", t!("game.weather"), &wx, false, |ui| {
                            self.wx.start_weather(ui);
                        });
                        let when = format!("{} · {}", menu::hhmm(self.menu.hour), weather::date_label(self.doy));
                        menu::section(ui, "time", t!("game.time"), &when, false, |ui| {
                            weather::Weather::start_time(ui, &mut self.menu.hour, &mut self.doy, &mut self.wx.real_time);
                        });
                        padui::follow_focus(ui);
                    });
                });
            });
            self.credits(ctx);
            padui::focus_ring(ctx, ACCENT);
        });
        self.egui_state.handle_platform_output(&self.window, out.platform_output);
        if self.mouse_captured { self.window.set_cursor_visible(false); }
        if let Some((lat, lon, name)) = jump {
            self.menu.look_at(lat, lon, 30_000.0);
            self.menu.pick = Some((lat, lon));
            self.menu.pick_name = tr_en(name).to_string();
            self.menu.airport = None;
            self.menu.runway = None;
        }
        if let Some(i) = airport {
            self.choose_airport(i);
        }
        let want = aircraft::nth(self.menu.aircraft);
        if !std::ptr::eq(want, self.spec) {
            self.want_aircraft(want);
        }
        if let Some(i) = go.as_deref().and_then(|q| airports::Db::get().find(q)) {
            self.choose_airport(i);
            go = None;
        }
        if let Some(q) = go {
            self.menu.pick_name = t!("game.searching_for", q);
            let (tx, rx) = std::sync::mpsc::channel();
            let src = self.src.clone();
            std::thread::spawn(move || {
                let _ = tx.send(geocode(&src.fetch, &q).map(|(la, lo, name)| (la, lo, 0.0, name)));
            });
            self.goto_rx = Some(rx);
        }
        if start {
            self.begin();
        }
        let jobs = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
        (jobs, out.textures_delta, out.pixels_per_point)
    }
}
