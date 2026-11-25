//! The in-flight interface: the HUD and its panels, the airport panel, the loading screens.

use crate::*;

impl Game {
    /// the loading screen.
    pub(crate) fn loading_overlay(&mut self, ctx: &egui::Context) {
        use egui::{pos2, vec2, Align2, Color32, Rect, Stroke};
        use menu::{ACCENT, DIM, TEXT};
        let Some(l) = self.loading.clone() else { return };
        let prep = self.r.aircraft_prep();
        let a = match l.done {
            Some(d) => (1.0 - (self.real - d) / Loading::FADE).clamp(0.0, 1.0) as f32,
            None => 1.0,
        };
        let t = (self.real - l.t0) as f32;
        // ease in.
        let rise = |delay: f32| {
            let x = ((t - delay) / 0.5).clamp(0.0, 1.0);
            1.0 - (1.0 - x).powi(3)
        };
        let screen = ctx.content_rect();
        let spec = self.spec;
        let tex = self.still(ctx, spec);
        let fade = |c: Color32| c.gamma_multiply(a);
        let k = (screen.width() / 1600.0).clamp(0.7, 1.4);
        let margin = 56.0 * k;
        let (lat, lon, _) = geo::pos_to_lla(self.plane.pos);
        let (lat, lon) = (lat.to_degrees(), lon.to_degrees());
        let local = (self.utc + lon / 15.0).rem_euclid(24.0);
        let start = self.last_start.clone();
        egui::Area::new(egui::Id::unique("loading")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.set_min_size(screen.size());
            let p = ui.painter();
            // backdrop.
            mainmenu::gradient(p, screen, fade(Color32::from_rgb(14, 20, 31)), fade(Color32::from_rgb(5, 7, 11)));
            let stage = Rect::from_min_max(pos2(screen.left() + screen.width() * 0.40, screen.top()), screen.max);
            menu::glow(p, pos2(stage.center().x, stage.center().y + 70.0 * k), vec2(stage.width() * 0.55, stage.height() * 0.22), fade(Color32::from_white_alpha(16)));
            // drifting lines.
            let drift = (t * 18.0) % (90.0 * k);
            let mut y = screen.top() - 90.0 * k + drift;
            while y < screen.bottom() {
                p.hline(screen.x_range(), y, Stroke::new(1.0, fade(Color32::from_white_alpha(6))));
                y += 90.0 * k;
            }
            // aircraft.
            let w = (stage.width() * 0.92).min(1100.0 * k);
            let h = w * 392.0 / 680.0;
            let slide = (1.0 - rise(0.05)) * 60.0 * k;
            let bob = (t * 1.3).sin() * 4.0 * k;
            let pic = Rect::from_center_size(pos2(stage.center().x + slide, stage.center().y - 30.0 * k + bob), vec2(w, h));
            egui::Image::new(&tex).tint(Color32::WHITE.gamma_multiply(a * rise(0.05))).paint_at(ui, pic);
            // fade its left edge.
            let melt = Rect::from_min_max(pos2(stage.left() - 40.0, screen.top()), pos2(stage.left() + w * 0.18, screen.bottom()));
            mainmenu::hgradient(p, melt, fade(Color32::from_rgb(10, 14, 22)), Color32::TRANSPARENT);

            // wordmark and time.
            mainmenu::wordmark(p, pos2(screen.left() + margin, screen.top() + margin + 34.0 * k), screen.left() + margin + 520.0 * k, 0.62 * k);
            p.text(pos2(screen.right() - margin, screen.top() + margin), Align2::RIGHT_TOP, format!("{}  ·  {}", menu::hhmm(local), weather::date_label(self.doy)), menu::data(), fade(DIM));

            // flight info.
            let col_x = screen.left() + margin;
            let col_w = (screen.width() * 0.40 - margin).max(320.0);
            let mut y = screen.top() + screen.height() * 0.30 + (1.0 - rise(0.0)) * 20.0;
            let a1 = a * rise(0.0);
            mainmenu::spaced(p, pos2(col_x, y), Align2::LEFT_TOP, &i18n::caps(t!("game.preparing_your_flight")), menu::bold(16.0 * k), ACCENT.gamma_multiply(a1), 2.4 * k);
            y += 30.0 * k;
            let name = p.layout(spec.name.to_string(), menu::bold(58.0 * k), Color32::WHITE.gamma_multiply(a1), col_w);
            let nh = name.size().y;
            p.galley(pos2(col_x, y), name, Color32::WHITE);
            y += nh + 10.0 * k;
            let place = start.as_ref().map(|s| s.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| menu::latlon(lat, lon));
            let place = p.layout(place.chars().take(120).collect(), menu::item_title(), TEXT.gamma_multiply(a1), col_w);
            let ph = place.size().y;
            p.galley(pos2(col_x, y), place, TEXT);
            y += ph + 6.0;
            if !self.status.is_empty() {
                let st = p.layout(self.status.clone(), menu::body(), DIM.gamma_multiply(a1), col_w);
                let sh = st.size().y;
                p.galley(pos2(col_x, y), st, DIM);
                y += sh;
            }
            y += 28.0 * k;

            // route strip.
            let a2 = a * rise(0.12);
            let mut stops: Vec<String> = vec![start.as_ref().and_then(|s| s.airport.clone()).unwrap_or_else(|| t!("game.start_2").to_string())];
            let wps = &self.plan.wps;
            if wps.len() > 4 {
                stops.extend(wps[..2].iter().map(|w| w.ident.clone()));
                stops.push(format!("+{}", wps.len() - 3));
                stops.push(wps[wps.len() - 1].ident.clone());
            } else {
                stops.extend(wps.iter().map(|w| w.ident.clone()));
            }
            let strip = Rect::from_min_size(pos2(col_x, y), vec2(col_w.min(560.0 * k), 64.0 * k));
            if stops.len() > 1 {
                let n = stops.len() - 1;
                let xs: Vec<f32> = (0..=n).map(|i| strip.left() + 8.0 + (strip.width() - 16.0) * i as f32 / n as f32).collect();
                let ly = strip.top() + 14.0 * k;
                p.hline(xs[0]..=xs[n], ly, Stroke::new(2.0, ROUTE_COLOUR.gamma_multiply(a2 * 0.45)));
                let along = xs[0] + (xs[n] - xs[0]) * l.frac as f32;
                p.hline(xs[0]..=along, ly, Stroke::new(2.0, ROUTE_COLOUR.gamma_multiply(a2)));
                for (i, (x, id)) in xs.iter().zip(&stops).enumerate() {
                    let last = i == n;
                    p.circle_filled(pos2(*x, ly), 5.0 * k, fade(Color32::from_rgb(12, 16, 24)));
                    p.circle_stroke(pos2(*x, ly), 5.0 * k, Stroke::new(2.0, (if i == 0 || last { Color32::WHITE } else { ROUTE_COLOUR }).gamma_multiply(a2)));
                    let align = if i == 0 { Align2::LEFT_TOP } else if last { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
                    p.text(pos2(*x + if i == 0 { -8.0 } else if last { 8.0 } else { 0.0 }, ly + 12.0 * k), align, id, menu::data(), (if i == 0 || last { Color32::WHITE } else { TEXT }).gamma_multiply(a2));
                }
                // plane marker.
                let c = pos2(along, ly);
                let s = 7.0 * k;
                let plane = [c + vec2(s, 0.0), c + vec2(-s * 0.8, -s * 0.75), c + vec2(-s * 0.35, 0.0), c + vec2(-s * 0.8, s * 0.75)];
                p.add(egui::Shape::convex_polygon(vec![plane[0], plane[1], plane[2]], ACCENT.gamma_multiply(a2), Stroke::NONE));
                p.add(egui::Shape::convex_polygon(vec![plane[0], plane[2], plane[3]], ACCENT.gamma_multiply(a2), Stroke::NONE));
                let dist = self.plan.total(start.as_ref().map(|s| (s.lat, s.lon)));
                let cruise = spec.cruise.max(20.0);
                p.text(pos2(strip.left(), ly + 38.0 * k), Align2::LEFT_TOP, t!("game.waypoints_0_nm_about_cruise_0", wps.len(), dist / 1852.0, flightplan::hm(dist / cruise), self.plan.cruise_ft), menu::caption(), DIM.gamma_multiply(a2));
                y = strip.bottom() + 16.0 * k;
            } else {
                p.text(pos2(col_x, y), Align2::LEFT_TOP, t!("game.free_flight_no_flight_plan"), menu::caption(), DIM.gamma_multiply(a2));
                y += 30.0 * k;
            }

            // condition chips.
            let a3 = a * rise(0.2);
            let chips = [
                (t!("text.weather.wind"), format!("{:03.0}° / {:.0} kt", self.wx.wind_from.rem_euclid(360.0), self.wx.wind * 1.94384)),
                ("QNH", format!("{:.0} hPa", self.wx.qnh)),
                (t!("text.weather.sky"), if self.wx.live { t!("game.real_weather").to_string() } else { self.wx.summary() }),
                (t!("game.fuel_2"), format!("{:.0} kg", self.plane.fuel)),
            ];
            let mut x = col_x;
            for (tag, v) in chips {
                let tg = p.layout_no_wrap(tag.to_string(), menu::bold(11.5 * k.max(0.9)), DIM.gamma_multiply(a3));
                let vg = p.layout_no_wrap(v, menu::data(), Color32::WHITE.gamma_multiply(a3));
                let cw = tg.size().x.max(vg.size().x) + 24.0;
                if x + cw > col_x + col_w + 40.0 {
                    break;
                }
                let r = Rect::from_min_size(pos2(x, y), vec2(cw, 46.0));
                p.rect_filled(r, menu::R_WIDGET, Color32::from_rgba_premultiplied(18, 24, 34, 200).gamma_multiply(a3));
                p.rect_stroke(r, menu::R_WIDGET, Stroke::new(1.0, Color32::from_white_alpha(22).gamma_multiply(a3)), egui::StrokeKind::Inside);
                p.galley(pos2(r.left() + 12.0, r.top() + 7.0), tg, DIM);
                p.galley(pos2(r.left() + 12.0, r.top() + 23.0), vg, Color32::WHITE);
                x = r.right() + 8.0;
            }

            // progress bar.
            let bar = Rect::from_min_max(pos2(screen.left() + margin, screen.bottom() - margin - 70.0 * k), pos2(screen.right() - margin, screen.bottom() - margin - 64.0 * k));
            p.rect_filled(bar, 3.0, fade(Color32::from_white_alpha(18)));
            let mut done = bar;
            done.set_width(bar.width() * l.frac as f32);
            p.rect_filled(done, 3.0, fade(ACCENT));
            if l.done.is_none() && done.width() > 30.0 {
                let sx = done.left() + ((t * 0.6).fract()) * (done.width() + 120.0) - 60.0;
                let sheen = Rect::from_min_max(pos2((sx - 60.0).max(done.left()), done.top()), pos2((sx + 60.0).min(done.right()), done.bottom()));
                if sheen.width() > 0.0 {
                    mainmenu::hgradient(p, Rect::from_min_max(sheen.min, pos2(sheen.center().x, sheen.max.y)), Color32::TRANSPARENT, fade(Color32::from_white_alpha(90)));
                    mainmenu::hgradient(p, Rect::from_min_max(pos2(sheen.center().x, sheen.min.y), sheen.max), fade(Color32::from_white_alpha(90)), Color32::TRANSPARENT);
                }
            }
            let what = if l.done.is_some() {
                t!("game.ready").to_string()
            } else if l.left > 0 {
                t!("game.loading_the_scenery_tiles_to_go", l.left)
            } else {
                t!("game.loading_the_scenery_2").to_string()
            };
            p.text(pos2(bar.left(), bar.top() - 10.0), Align2::LEFT_BOTTOM, what, menu::label(), fade(TEXT));
            p.text(pos2(bar.right(), bar.top() - 10.0), Align2::RIGHT_BOTTOM, format!("{:.0}%", l.frac * 100.0), menu::bold(22.0 * k.max(0.9)), fade(Color32::WHITE));
            // The aircraft's shaders, if they are still being compiled.
            if let Some(prep) = prep {
                draw_prep(p, pos2(screen.center().x, bar.top() - 110.0 * k), (bar.width() * 0.45).clamp(160.0, 460.0), prep, self.real, a, true);
            }
            // rotating tip.
            let per = 8.0;
            let which = l.tip + (t / per) as usize;
            let phase = (t % per) / per;
            let tip_a = a * (phase * per / 0.4).min(1.0) * ((1.0 - phase) * per / 0.4).min(1.0);
            let ty = bar.bottom() + 22.0 * k;
            let tag = p.layout_no_wrap(t!("game.tip").to_string(), menu::bold(12.0), ACCENT.gamma_multiply(tip_a));
            let tag_w = tag.size().x;
            let tag_r = Rect::from_min_size(pos2(bar.left(), ty), vec2(tag_w + 14.0, 20.0));
            p.rect_stroke(tag_r, menu::R_WIDGET, Stroke::new(1.0, ACCENT.gamma_multiply(tip_a * 0.8)), egui::StrokeKind::Inside);
            p.galley(pos2(tag_r.left() + 7.0, tag_r.center().y - tag.size().y * 0.5), tag, ACCENT);
            let tip = p.layout(tr_en(LOADING_TIPS[which % LOADING_TIPS.len()]).to_string(), menu::body(), Color32::from_gray(205).gamma_multiply(tip_a), bar.width() * 0.7);
            p.galley(pos2(tag_r.right() + 12.0, tag_r.center().y - 10.0), tip, Color32::WHITE);
            let skip = if padui::active() { t!("game.a_skip") } else { t!("game.enter_space_or_click_skip") };
            p.text(pos2(bar.right(), tag_r.center().y), Align2::RIGHT_CENTER, skip, menu::caption(), fade(Color32::from_gray(120)));
        });
        ctx.request_repaint();
        // A click skips it too.
        if l.done.is_none() && ctx.input(|i| i.pointer.any_click()) {
            self.finish_loading();
        }
    }

    /// A panel at the top while the aircraft's shaders are still compiling
    /// (it is not drawn until they are): how far, and that it takes a while.
    pub(crate) fn aircraft_prep_overlay(&self, ctx: &egui::Context) {
        let Some(prep) = self.r.aircraft_prep() else { return };
        let screen = ctx.content_rect();
        let w = (screen.width() - 80.0).clamp(160.0, 460.0);
        egui::Area::new(egui::Id::unique("aircraft prep")).order(egui::Order::Foreground).fixed_pos(screen.min).interactable(false).show(ctx, |ui| {
            draw_prep(ui.painter(), egui::pos2(screen.center().x, screen.top() + 40.0), w, prep, self.real, 1.0, true);
        });
    }

    /// What the airport panel says: the airfield, the runway in use and the
    /// wind, and on the way in a guide down the approach (distance, which side
    /// of the centreline, above or below the 3° glide path); on the ground,
    /// where on the airfield the aircraft is. (text, colour: 0 plain, 1 good, 2 caution, 3 warning)
    pub(crate) fn airport_hud(&self) -> Option<Vec<(String, u8)>> {
        if self.crashed {
            return None;
        }
        let f = self.world.airfield_near(self.plane.pos, 18_000.0)?;
        let g = &f.geo;
        let a = g.apt();
        let mut out = vec![(format!("{}  {}", a.code(), a.name), 0u8)];
        let wind_kt = self.wx.wind * 1.94384;
        let wind = if wind_kt < 1.0 { t!("game.calm").to_string() } else { format!("{:03.0}°/{:.0} kt", self.wx.wind_from, wind_kt) };
        let moving = f.traffic.movers.len();
        let active = f.traffic.active.map(|(ri, e)| t!("game.runway_in_use", g.runways[ri].ident[e])).unwrap_or_else(|| t!("game.no_runway_on_the_map").into());
        out.push((t!("game.wind_elevation_ft_aircraft_moving", g.elev * 3.28084, moving, active = active, wind = wind), 0));
        let p = g.local(self.plane.pos);
        let alt = geo::pos_to_lla(self.plane.pos).2 as f32;
        let fw = self.plane.forward();
        let hd = glam::Vec2::new(fw.dot(g.e) as f32, fw.dot(g.n) as f32).normalize_or_zero();
        if self.plane.on_ground || alt - g.h(p) < 3.0 {
            if let Some(ri) = g.on_runway(p, 0.0) {
                let r = &g.runways[ri];
                let e = if hd.dot(r.dir) >= 0.0 { 0 } else { 1 };
                let (s, _) = r.local(p);
                let left = if e == 0 { r.len - s } else { s };
                out.push((t!("game.on_runway_m_of_runway", r.ident[e], left), 1));
            } else if let Some(h) = g.holds.iter().find(|h| (h.p - p).length() < 25.0) {
                let r = &g.runways[h.rwy as usize];
                out.push((t!("game.holding_point_runway", r.name()), 2));
            }
            return Some(out);
        }
        // The runway end ahead that we are lined up with best.
        let mut best: Option<(f32, usize, usize)> = None;
        for (ri, r) in g.runways.iter().enumerate() {
            for e in 0..2 {
                let (thr, dir) = r.end(e);
                let along = (thr - p).dot(dir);
                let x = (p - thr).dot(airports::right_of(dir));
                if along > -200.0 && along < 20_000.0 && hd.dot(dir) > 0.75 && x.abs() < along.max(0.0) * 0.25 + 300.0 && best.map(|b| along < b.0).unwrap_or(true) {
                    best = Some((along, ri, e));
                }
            }
        }
        if let Some((along, ri, e)) = best {
            let r = &g.runways[ri];
            let (thr, dir) = r.end(e);
            let x = (p - thr).dot(airports::right_of(dir));
            let h = alt - g.h(thr);
            let aim = (along + traffic::aim_dist(r.len)).max(1.0);
            let gs = h.atan2(aim).to_degrees();
            let dev = gs - 3.0;
            let side = if x.abs() < 0.01 * along.max(300.0) + 5.0 { t!("game.on_the_centreline").to_string() } else { t!("game.m_of_the_centreline", x.abs(), if x > 0.0 { t!("game.right") } else { t!("game.left") }) };
            let glide = if dev.abs() < 0.3 { t!("game.on_glide_path").to_string() } else if dev > 0.0 { t!("game.high", dev) } else { t!("game.low", -dev) };
            let col = if dev < -0.7 || x.abs() > 0.06 * along + 30.0 { 3 } else if dev.abs() > 0.3 { 2 } else { 1 };
            out.push((t!("game.runway_nm_ft_above_it", r.ident[e], along.max(0.0) / 1852.0, side, glide, h * 3.28084), col));
        }
        Some(out)
    }

    pub(crate) fn ui(&mut self, agl: f64) -> (Vec<egui::ClippedPrimitive>, egui::TexturesDelta, f32) {
        let raw = self.egui_input();
        let (lat, lon, alt) = geo::pos_to_lla(self.plane.pos);
        let (e, n, u) = geo::enu(self.plane.pos);
        let f = self.plane.forward();
        let hdg = f.dot(e).atan2(f.dot(n)).to_degrees().rem_euclid(360.0);
        // Indicated airspeed: what the wing feels, and what the overspeed warnings are about.
        let speed = self.plane.ias();
        let vs = self.plane.vel.dot(u);
        // (A rocket's height is its engines' above the ground.)
        let base_up = self.world.ground(lat, lon).map_or(alt, |g| alt - g.h) - self.plane.wheel_height();
        let local = (self.utc + lon.to_degrees() / 15.0).rem_euclid(24.0);
        let mut go: Option<String> = None;
        let mut close_goto = false;
        let mut act = None;
        let mut bar: Option<flightui::Bar> = None;
        let airport_hud = self.airport_hud();
        let landing_guide = if self.minimap.landing_guidance && !self.crashed {
            self.world.airfield_near(self.plane.pos, 18_000.0).and_then(|field| {
                let g = &field.geo;
                field.traffic.active.or_else(|| g.active(self.wx.wind_from as f32, (self.wx.wind * 1.94384) as f32))
                    .map(|(runway, end)| approach::Guide::new(g, runway, end))
            })
        } else { None };
        self.src.google.update_attribution(lat.to_degrees(), lon.to_degrees(), agl);
        self.flash.retain(|(_, t)| self.real - t < FLASH_HOLD + FLASH_FADE);
        let out = self.egui_ctx.clone().run_ui(raw, |root| {
            let ctx = &root.ctx().clone();
            self.pad_pass(ctx);
            use egui::{Align2, Color32, FontId, RichText};
            let pad = padui::active();
            let white = Color32::from_rgba_unmultiplied(255, 255, 255, 235);
            // keys to press next, as key caps.
            let keys_line = |ui: &mut egui::Ui, keys: &[(&str, &str)]| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (k, (key, what)) in keys.iter().enumerate() {
                        if k > 0 {
                            ui.add_space(12.0);
                        }
                        menu::key_cap(ui, key);
                        ui.label(RichText::new(*what).font(menu::label()).color(menu::TEXT));
                    }
                });
            };
            if self.pause_open {
                self.pause_menu(ctx, &mut act, lat.to_degrees(), lon.to_degrees(), local);
                padui::focus_ring(ctx, menu::ACCENT);
                return;
            }
            if self.info_open {
                self.info_panel(ctx, &mut act, lon.to_degrees());
            }
            // the flight menu, over everything but the pause menu.
            if self.mode == Mode::Fly {
                self.radial_menu(ctx, &mut bar);
            }
            // the hud.
            if self.hud_on() {
                self.hud(ctx, &flightui::HudIn { hdg, ias: speed, alt, agl, vs, local });
                // rocket figures or weapons, for the aircraft that have them.
                self.systems_card(ctx, vs, base_up);
                // the card takes the top centre while it is open.
                if !self.info_open {
                    self.notices(ctx, airport_hud.as_deref());
                }
            }
            self.hotspot_overlay(ctx);
            // a ring on each aircraft in range, tags on the nearest that do not overlap.
            if let (true, Some((vp, cam))) = (self.hud_on() && self.live.enabled(), self.live_view) {
                let screen = ctx.content_rect();
                let painter = ctx.layer_painter(egui::LayerId::background());
                let mut seen: Vec<(f64, egui::Pos2, &live::Plane)> = Vec::new();
                for p in &self.live.planes {
                    if p.downed > 0.0 {
                        continue;
                    }
                    let rel = p.pos - cam;
                    let d = rel.length();
                    if d > 40_000.0 || d < 30.0 {
                        continue;
                    }
                    let c = vp * rel.as_vec3().extend(1.0);
                    if c.w <= 0.0 {
                        continue;
                    }
                    let at = egui::pos2(screen.left() + (c.x / c.w * 0.5 + 0.5) * screen.width(), screen.top() + (0.5 - c.y / c.w * 0.5) * screen.height());
                    if screen.expand(40.0).contains(at) {
                        seen.push((d, at, p));
                    }
                }
                seen.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut taken: Vec<egui::Rect> = Vec::new();
                for (d, at, p) in seen {
                    // fainter with distance.
                    let a = (1.0 - d / 40_000.0).clamp(0.45, 1.0) as f32;
                    painter.circle_stroke(at, 5.0, egui::Stroke::new(1.5, flightui::LIT.gamma_multiply(a)));
                    let line2 = format!("{}  ·  {:.0} ft  ·  {:.1} km", if p.ty.is_empty() { "?" } else { p.ty.as_str() }, p.alt_ft, d / 1000.0);
                    // bare text, a dark shadow under it to read against cloud and sky.
                    let lines = [(p.call.as_str(), menu::semibold(11.5), Color32::WHITE.gamma_multiply(a)), (line2.as_str(), menu::medium(10.0), flightui::SOFT.gamma_multiply(a))];
                    let w = lines.iter().map(|(s, f, _)| painter.layout_no_wrap(s.to_string(), f.clone(), Color32::WHITE).size().x).fold(0.0, f32::max);
                    let r = egui::Rect::from_min_size(at + egui::vec2(9.0, -12.0), egui::vec2(w, 26.0));
                    if taken.len() >= 8 || taken.iter().any(|t| t.expand(4.0).intersects(r)) {
                        continue;
                    }
                    taken.push(r);
                    let shade = Color32::from_black_alpha((200.0 * a) as u8);
                    for (k, (s, f, col)) in lines.into_iter().enumerate() {
                        let pos = r.min + egui::vec2(0.0, 13.0 * k as f32);
                        painter.text(pos + egui::vec2(1.0, 1.0), Align2::LEFT_TOP, s, f.clone(), shade);
                        painter.text(pos, Align2::LEFT_TOP, s, f, col);
                    }
                }
            }
            // The other players: a tag beside each.
            if let (true, Some((vp, cam))) = (self.show_hud && !self.mp_players.is_empty(), self.live_view) {
                let screen = ctx.content_rect();
                let painter = ctx.layer_painter(egui::LayerId::background());
                let col = Color32::from_rgb(255, 205, 90);
                for p in &self.mp_players {
                    let rel = p.pos - cam;
                    let d = rel.length();
                    if d > 60_000.0 || d < 15.0 {
                        continue;
                    }
                    let c = vp * rel.as_vec3().extend(1.0);
                    if c.w <= 0.0 {
                        continue;
                    }
                    let at = egui::pos2(screen.left() + (c.x / c.w * 0.5 + 0.5) * screen.width(), screen.top() + (0.5 - c.y / c.w * 0.5) * screen.height());
                    if !screen.expand(40.0).contains(at) {
                        continue;
                    }
                    painter.circle_stroke(at, 6.0, egui::Stroke::new(1.5, col));
                    let line2 = format!("{}  {:.0} ft  {:.0} kt  {:.1} km", mp_proto::AIRCRAFT.get(p.aircraft as usize).copied().unwrap_or("?"), geo::pos_to_lla(p.pos).2 * 3.28084, p.vel.length() * 1.94384, d / 1000.0);
                    for (k, (s, size)) in [(p.name.as_str(), 13.0), (line2.as_str(), 11.0)].into_iter().enumerate() {
                        let pos = at + egui::vec2(10.0, -10.0 + 14.0 * k as f32);
                        painter.text(pos + egui::vec2(1.0, 1.0), Align2::LEFT_TOP, s, FontId::proportional(size), Color32::from_black_alpha(160));
                        painter.text(pos, Align2::LEFT_TOP, s, FontId::proportional(size), col);
                    }
                }
            }
            // The map destination: a diamond and tag in the HUD, like live traffic.
            if let (true, Some((vp, cam)), Some(dest)) = (self.hud_on(), self.live_view, self.minimap.destination()) {
                let screen = ctx.content_rect();
                let dest_pos = geo::lla_to_pos(dest.lat, dest.lon, 0.0);
                let rel = dest_pos - cam;
                let d = rel.length();
                let c = vp * rel.as_vec3().extend(1.0);
                if c.w > 0.0 {
                    let at = egui::pos2(screen.left() + (c.x / c.w * 0.5 + 0.5) * screen.width(), screen.top() + (0.5 - c.y / c.w * 0.5) * screen.height());
                    if screen.expand(40.0).contains(at) {
                        let painter = ctx.layer_painter(egui::LayerId::background());
                        let col = minimap::DEST_COLOR;
                        let r = 6.0;
                        let diamond = vec![at + egui::vec2(0.0, -r), at + egui::vec2(r, 0.0), at + egui::vec2(0.0, r), at + egui::vec2(-r, 0.0)];
                        painter.add(egui::Shape::closed_line(diamond, egui::Stroke::new(1.8, col)));
                        painter.circle_filled(at, 2.0, col);
                        let title = dest.name.as_deref().unwrap_or(t!("minimap.destination"));
                        let brg = rel.dot(e).atan2(rel.dot(n)).to_degrees().rem_euclid(360.0);
                        let line2 = format!("{}  ·  {:03.0}°", minimap::distance_label(d, i18n::lang()), brg);
                        let shade = Color32::from_black_alpha(200);
                        for (k, (s, f, col)) in [(title, menu::semibold(11.5), Color32::WHITE), (line2.as_str(), menu::medium(10.0), flightui::SOFT)].into_iter().enumerate() {
                            let pos = at + egui::vec2(10.0, -12.0 + 13.0 * k as f32);
                            painter.text(pos + egui::vec2(1.0, 1.0), Align2::LEFT_TOP, s, f.clone(), shade);
                            painter.text(pos, Align2::LEFT_TOP, s, f, col);
                        }
                    }
                }
            }
            // The seeker's target: a diamond on the aircraft, and its range.
            if let (true, Some((vp, cam))) = (self.hud_on(), self.live_view) {
                if let Some(t) = &self.weapons.target_view {
                    let screen = ctx.content_rect();
                    let c = vp * (t.pos - cam).as_vec3().extend(1.0);
                    if c.w > 0.0 {
                        let at = egui::pos2(screen.left() + (c.x / c.w * 0.5 + 0.5) * screen.width(), screen.top() + (0.5 - c.y / c.w * 0.5) * screen.height());
                        if screen.expand(100.0).contains(at) {
                            let painter = ctx.layer_painter(egui::LayerId::background());
                            let locked = self.weapons.lock >= 1.0;
                            let col = if locked { Color32::from_rgb(120, 230, 140) } else { Color32::from_rgb(255, 180, 50) };
                            let st = egui::Stroke::new(1.6, col);
                            let r = if locked { 16.0 } else { 12.0 };
                            painter.circle_stroke(at, r + 5.0, egui::Stroke::new(1.0, col));
                            for (a, b) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                                painter.line_segment([at + egui::vec2(a * r, b * r), at + egui::vec2(a * (r + 8.0), b * (r + 8.0))], st);
                            }
                            let line = format!("{}  {}  {:.1} km  {:+.0} m/s", if locked { "LOCK" } else { "TRACK" }, t.label, t.range / 1000.0, t.closure);
                            let pos = at + egui::vec2(0.0, r + 22.0);
                            painter.text(pos + egui::vec2(1.0, 1.0), Align2::CENTER_TOP, &line, FontId::monospace(12.0), Color32::from_black_alpha(180));
                            painter.text(pos, Align2::CENTER_TOP, &line, FontId::monospace(12.0), col);
                        }
                    }
                }
            }
            if self.hud_on() && !self.pause_open && !self.plane.on_ground {
                if let (Some(guide), Some((vp, camera))) = (&landing_guide, self.live_view) {
                    guide.draw_world(ctx, vp, camera, self.plane.pos);
                }
            }
            // The loading screen, over everything else.
            self.loading_overlay(ctx);
            // After it, the aircraft still waiting for its shaders.
            if self.loading.is_none() {
                self.aircraft_prep_overlay(ctx);
            }
            // Google's imagery has to carry its credit, HUD or not.
            if self.hud_on() || self.google_shown() {
                self.credits(ctx);
            }
            // Damage: warnings while the airframe is being overstressed, what has
            // already broken, and a flash for each new loss.
            let p = &self.plane;
            let mut warn: Vec<(&str, Color32)> = Vec::new();
            let red = menu::BAD;
            let amber = Color32::from_rgb(255, 196, 80);
            if !self.crashed && self.show_hud {
                if p.stalled {
                    warn.push(("STALL", red));
                }
                if p.gear < 1.0 && agl < 150.0 && p.throttle < 0.35 && !p.on_ground {
                    warn.push((if pad { "GEAR UP  (L3)" } else { "GEAR UP  (L)" }, red));
                }
                let w = &self.warn;
                if w.pull_up {
                    warn.push(("PULL UP", red));
                } else if w.dive > 0.0 {
                    warn.push(("NOSE DIVE", red));
                } else if w.too_low {
                    warn.push(("TOO LOW  TERRAIN", amber));
                } else if w.sink {
                    warn.push(("SINK RATE", amber));
                }
                let over = p.overspeed();
                if over > 13.0 {
                    warn.push(("OVERSPEED  AIRFRAME BREAKING UP", red));
                } else if over > 0.0 {
                    warn.push(("OVERSPEED", amber));
                }
                if p.flaps > 0.25 && speed > p.vfe() {
                    warn.push((if p.spec.lever.is_some() { "NOSE OVERSPEED" } else { "FLAPS OVERSPEED" }, if p.dmg.flap_stress > 0.5 { red } else { amber }));
                }
                if p.g_load.abs() > 5.0 {
                    warn.push(("OVER G", if p.dmg.fatigue > 1.2 { red } else { amber }));
                }
                if p.fuel <= 0.0 {
                    warn.push(("NO FUEL", red));
                } else if p.fuel_frac() < 0.1 {
                    warn.push(("LOW FUEL", amber));
                }
                if !p.running && !p.on_ground {
                    warn.push(("ENGINE OFF", red));
                }
                let el = &p.sys.elec;
                if el.master && (el.volts < 24.0 || (p.sys.fail.alternator && p.running)) {
                    warn.push(("LOW VOLTS", amber));
                }
                if p.sys.fail.vacuum && !p.spec.is_jet() {
                    warn.push(("VACUUM", amber));
                }
                if p.cg_excess() != 0.0 {
                    warn.push(("CG LIMIT", amber));
                }
            }
            let report = if self.crashed { Vec::new() } else { p.dmg.report() };
            // warnings, damage and the approach line.
            if self.show_hud {
                let guide = if self.hud_on() { airport_hud.as_ref().and_then(|l| l.get(2)) } else { None };
                self.alerts(ctx, &warn, &report, guide);
            }
            if !self.flash.is_empty() && !self.crashed {
                // Each new loss: a small red plate down the left edge (under the systems card,
                // if any), newest on top, fading quickly. A crash sheds many pieces at once, so
                // the stack stops at half the screen height rather than covering the view.
                let top = if self.hud_on() { self.minimap.top } else { 0.0 } + 14.0;
                let floor = ctx.content_rect().height() * 0.5;
                egui::Area::new(egui::Id::unique("flash")).anchor(Align2::LEFT_TOP, [14.0, top]).interactable(false).show(ctx, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        for (s, t0) in self.flash.iter().rev() {
                            // (a plate is about 24 px tall: start one only if it ends above the floor)
                            if ui.cursor().top() + 24.0 > floor {
                                break;
                            }
                            let a = (1.0 - ((self.real - t0 - FLASH_HOLD) / FLASH_FADE).max(0.0)).clamp(0.0, 1.0) as f32;
                            ui.scope(|ui| {
                                ui.multiply_opacity(a);
                                egui::Frame::new().fill(menu::BAD.gamma_multiply(0.2)).stroke(egui::Stroke::new(1.0, menu::BAD)).corner_radius(menu::R_WIDGET).inner_margin(egui::Margin::symmetric(9, 3)).show(ui, |ui| {
                                    ui.label(RichText::new(i18n::caps(s)).font(menu::bold(14.0)).color(menu::BAD).extra_letter_spacing(0.6));
                                });
                            });
                        }
                    });
                });
            }
            // Come to rest in pieces without burning: say so, and how to go on.
            if !self.crashed && self.plane.wrecked() && self.plane.vel.length() < 2.0 && (self.plane.on_hull || self.plane.on_ground) {
                self.wreck_t += self.real - self.wreck_seen;
            } else {
                self.wreck_t = 0.0;
            }
            self.wreck_seen = self.real;
            if !self.crashed && self.wreck_t > 2.0 && self.mode == Mode::Fly {
                egui::Area::new(egui::Id::unique("wreck")).anchor(Align2::CENTER_CENTER, [0.0, 80.0]).show(ctx, |ui| {
                    flightui::panel(ui, flightui::glass().inner_margin(egui::Margin::symmetric(24, 16)), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(t!("game.aircraft_wrecked")).font(menu::bold(34.0)).color(Color32::from_rgb(255, 160, 80)).extra_letter_spacing(1.5));
                            ui.add_space(6.0);
                            if pad {
                                let panel = padmap::spec_of("flight_panel");
                                let mut items = vec![(panel.as_str(), tr_en("go somewhere else")), ("Menu", tr_en("menu: respawn, globe"))];
                                items.retain(|i| !i.0.is_empty());
                                padui::prompts(ui, &items, 24.0, 16.0, white);
                            } else {
                                keys_line(ui, &[("R", t!("game.respawn")), ("G", t!("game.go_somewhere_else")), ("Esc", t!("game.menu_2"))]);
                            }
                        });
                    });
                });
            }
            if self.crashed && self.real - self.crash_real > 3.5 {
                egui::Area::new(egui::Id::unique("crash")).anchor(Align2::CENTER_BOTTOM, [0.0, -70.0]).show(ctx, |ui| {
                    flightui::panel(ui, flightui::glass().inner_margin(egui::Margin::symmetric(20, 12)), |ui| {
                        if pad {
                            let panel = padmap::spec_of("flight_panel");
                            let mut items = vec![("A", tr_en("respawn")), (panel.as_str(), tr_en("go somewhere else")), ("Menu", tr_en("menu"))];
                            items.retain(|i| !i.0.is_empty());
                            padui::prompts(ui, &items, 24.0, 16.0, white);
                        } else {
                            keys_line(ui, &[("R", t!("game.respawn")), ("G", t!("game.go_somewhere_else")), ("Esc", t!("game.menu_2"))]);
                        }
                    });
                });
            }
            if self.mode == Mode::Fly && !self.show_help && !self.goto_open {
                self.mouse_yoke_hud(ctx);
            }
            self.minimap.trail.push(lat.to_degrees(), lon.to_degrees());
            self.minimap.tuned = minimap_tuned(&self.radios);
            if !self.pause_open && !self.show_help && !self.goto_open && (self.minimap.full || (self.hud_on() && self.minimap.visible)) {
                {
                    let horizontal = f.dot(e).hypot(f.dot(n));
                    let up = self.plane.pos.normalize();
                    let gs = (self.plane.vel - up * self.plane.vel.dot(up)).length();
                    let route = (!self.plan.done()).then_some((&self.plan, gs));
                    self.minimap.show(ctx, &self.src, lat, lon, (horizontal > 1e-5).then_some(hdg.to_radians()), self.info_open, landing_guide.as_ref(), &self.world, route);
                }
            }
            if self.show_help {
                egui::Area::new(egui::Id::unique("help")).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    menu::panel().show(ui, |ui| {
                        ui.set_width((ctx.content_rect().width() - 80.0).clamp(320.0, 820.0));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t!("game.controls")).font(menu::page_title()).color(Color32::WHITE));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(t!("game.f1_esc_close")).font(menu::caption()).color(menu::DIM));
                            });
                        });
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 160.0).max(200.0)).auto_shrink([false, true]).show(ui, |ui| {
                            menu::controls_list(ui);
                        });
                    });
                });
            }
            if self.goto_open {
                egui::Window::new(t!("game.go_to")).id(egui::Id::unique("goto_window")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, -80.0]).show(ctx, |ui| {
                    ui.label(t!("game.city_landmark_street_address_or"));
                    let resp = ui.add(egui::TextEdit::singleline(&mut self.goto_text).desired_width(360.0));
                    resp.request_focus();
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.goto_text.trim().is_empty() {
                        go = Some(self.goto_text.trim().to_string());
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close_goto = true;
                    }
                });
            }
            padui::focus_ring(ctx, menu::ACCENT);
        });
        self.egui_state.handle_platform_output(&self.window, out.platform_output);
        if self.mouse_captured { self.window.set_cursor_visible(false); }
        // A button or slider left focused on the flight panel would keep the
        // keys from the aircraft, which goes on flying while the panel is open.
        // (Except where the gamepad works the panel.)
        if !self.pause_open && !(self.info_open && padui::active()) && !self.egui_ctx.text_edit_focused() {
            self.egui_ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        if let Some(i) = go.as_deref().and_then(|q| airports::Db::get().find(q)) {
            self.goto_open = false;
            go = None;
            self.start_airport(i, None, false, true);
        }
        if let Some(q) = go {
            self.goto_open = false;
            self.status = t!("game.searching_for", q);
            self.status_until = self.real + 30.0;
            let (tx, rx) = std::sync::mpsc::channel();
            let src = self.src.clone();
            std::thread::spawn(move || {
                let _ = tx.send(geocode(&src.fetch, &q).map(|(la, lo, name)| (la, lo, ground_elev(&src, la, lo), name)));
            });
            self.goto_rx = Some(rx);
        }
        if close_goto {
            self.goto_open = false;
        }
        if let Some(b) = bar {
            self.toolbar_act(b);
        }
        match act {
            Some(PauseAct::Resume) => self.set_pause_menu(false),
            Some(PauseAct::Respawn) => {
                self.set_pause_menu(false);
                self.info_open = false;
                self.respawn_here();
            }
            Some(PauseAct::GoTo) => {
                self.set_pause_menu(false);
                self.goto_open = true;
                self.goto_text.clear();
            }
            Some(PauseAct::FlyTo(lat, lon, name)) => {
                self.set_pause_menu(false);
                self.info_open = false;
                self.fly_to(lat, lon, name);
            }
            Some(PauseAct::Airport(i, appr)) => {
                self.set_pause_menu(false);
                self.info_open = false;
                self.start_airport(i, None, appr, true);
            }
            Some(PauseAct::Globe) => self.to_globe(),
            Some(PauseAct::Quit) => self.quit = true,
            None => {}
        }
        let jobs = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
        (jobs, out.textures_delta, out.pixels_per_point)
    }
}
