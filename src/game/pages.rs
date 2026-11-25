//! The pause and flight menus' pages: flight, places, radio, autopilot, route and procedures, logbook, failures, credits.

use crate::*;

impl Game {
    /// The pause menu's logbook: the totals, the hours on each aircraft, and
    /// the flights, newest first.
    pub(crate) fn logbook_page(&mut self, ui: &mut egui::Ui) {
        use egui::RichText;
        use flightlog::{hours_text, Outcome};
        use menu::{ACCENT, DIM, TEXT};
        let book = &self.logbook;
        let t = book.totals();
        let stat = |ui: &mut egui::Ui, value: String, what: &str| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).font(menu::item_title()).color(TEXT));
                ui.label(RichText::new(what).font(menu::caption()).color(DIM));
            });
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 28.0;
            stat(ui, t.flights.to_string(), t!("log.flights"));
            stat(ui, hours_text(t.hours * 3600.0), t!("log.hours_in_the_air"));
            stat(ui, t.landings.to_string(), t!("log.landings"));
            stat(ui, t.best.map_or("–".into(), |b| format!("{b}/100")), t!("log.best_landing"));
        });
        ui.add_space(10.0);
        if t.flights == 0 {
            ui.label(RichText::new(t!("log.empty")).font(menu::body()).color(DIM));
        } else {
            menu::heading(ui, t!("log.by_aircraft"));
            for (name, h) in book.hours_by_aircraft().into_iter().take(6) {
                ui.label(RichText::new(format!("{}  {}", hours_text(h * 3600.0), i18n::tr_en_owned(&name))).font(menu::data()).color(TEXT));
            }
            ui.add_space(8.0);
            menu::heading(ui, t!("log.flights"));
            egui::Grid::new("logbook").striped(true).spacing([16.0, 6.0]).show(ui, |ui| {
                for h in [t!("log.date"), t!("log.aircraft"), t!("log.route"), t!("log.time"), t!("log.landings"), t!("log.best"), t!("log.outcome")] {
                    ui.label(RichText::new(h).font(menu::caption()).color(DIM));
                }
                ui.end_row();
                for f in book.flights.iter().rev().take(300) {
                    let place = |p: &Option<String>| p.clone().unwrap_or_else(|| "—".into());
                    ui.label(RichText::new(flightlog::date_text(f.date)).font(menu::data()).color(DIM));
                    ui.label(RichText::new(i18n::tr_en_owned(&f.aircraft)).color(TEXT));
                    ui.label(RichText::new(format!("{} → {}", place(&f.from), place(&f.to))).font(menu::data()).color(TEXT));
                    ui.label(RichText::new(hours_text(f.air_time)).font(menu::data()).color(TEXT));
                    ui.label(RichText::new(f.landings.len().to_string()).font(menu::data()).color(TEXT));
                    match f.best_landing() {
                        Some(l) => ui.label(RichText::new(format!("{} {}", l.grade.text(), l.score)).color(TEXT)).on_hover_text(l.summary()),
                        None => ui.label(RichText::new("—").color(DIM)),
                    };
                    let (text, color) = match f.outcome {
                        Outcome::Landed => (t!("log.landed"), TEXT),
                        Outcome::Crashed => (t!("log.crashed"), ACCENT),
                        Outcome::Unfinished => (t!("log.unfinished"), DIM),
                    };
                    ui.label(RichText::new(text).color(color));
                    ui.end_row();
                }
            });
        }
    }

    /// The pause menu's failures: each one by hand, or one now and then at random.
    pub(crate) fn failures_page(&mut self, ui: &mut egui::Ui) {
        use egui::RichText;
        use menu::{DIM, TEXT};
        menu::heading(ui, t!("game.failures"));
        ui.label(RichText::new(t!("game.they_act_as_the_real")).font(menu::caption()).color(DIM));
        for i in 0..systems::Failures::N {
            let mut on = self.plane.sys.fail.get(i);
            if ui.checkbox(&mut on, RichText::new(systems::Failures::label(i)).color(TEXT)).changed() {
                self.plane.sys.fail.set(i, on);
            }
        }
        if ui.button(t!("game.repair_everything")).clicked() {
            self.plane.sys.fail = systems::Failures::default();
        }
        ui.add_space(6.0);
        menu::heading(ui, t!("game.at_random"));
        if ui.checkbox(&mut self.fail_random, t!("game.a_failure_now_and_then")).changed() {
            self.fail_next = None;
        }
        ui.add_enabled_ui(self.fail_random, |ui| {
            ui.horizontal(|ui| {
                ui.label(t!("game.about_every"));
                ui.add(egui::Slider::new(&mut self.fail_mean, 5.0..=120.0).logarithmic(true).custom_formatter(|v, _| format!("{v:.0} min")));
            });
        });
    }

    /// Random failures: one at a time, at intervals drawn around the mean.
    pub(crate) fn random_failures(&mut self) {
        if !self.fail_random || self.mode != Mode::Fly || self.crashed || self.pause_open {
            if !self.fail_random {
                self.fail_next = None;
            }
            return;
        }
        let mean = self.fail_mean * 60.0;
        let next = *self.fail_next.get_or_insert_with(|| self.real + -(1.0 - self.rng.f() as f64).max(1e-6).ln() * mean);
        if self.real < next {
            return;
        }
        self.fail_next = None;
        let free: Vec<usize> = (0..systems::Failures::N).filter(|&i| !self.plane.sys.fail.get(i)).collect();
        if free.is_empty() {
            return;
        }
        let i = free[((self.rng.f() * free.len() as f32) as usize).min(free.len() - 1)];
        self.plane.sys.fail.set(i, true);
        self.flash.push((t!("game.failure", systems::Failures::label(i).to_uppercase()), self.real));
    }

    /// flight plan section on the start screen.
    pub(crate) fn plan_section(&mut self, ui: &mut egui::Ui) {
        use egui::{Color32, RichText};
        use menu::{DIM, TEXT};
        let dep = self.menu.pick;
        let summary = match self.plan.wps.last() {
            None => t!("game.direct_2").to_string(),
            Some(last) => format!("{}  ·  {:.0} nm", last.ident, self.plan.total(dep) / 1852.0),
        };
        menu::section(ui, "flightplan", t!("game.flight_plan"), &summary, true, |ui| {
            // typed waypoints.
            let mut add: Option<String> = None;
            ui.horizontal(|ui| {
                let resp = ui.add(egui::TextEdit::singleline(&mut self.plan_text).desired_width(ui.available_width() - 56.0).hint_text(t!("game.klax_a_vor_37_5_122")).margin(egui::vec2(8.0, 6.0)));
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.add(egui::Button::new(RichText::new(t!("game.add")).font(menu::body())).min_size(egui::vec2(48.0, 28.0))).clicked() || enter) && !self.plan_text.trim().is_empty() {
                    add = Some(self.plan_text.trim().to_string());
                }
            });
            // suggestions.
            let word = self.plan_text.trim().to_string();
            if word.len() >= 2 && !word.contains(char::is_whitespace) {
                let up = word.to_ascii_uppercase();
                let db = airports::Db::get();
                let near = self.plan.wps.last().map(|w| w.at()).or(dep);
                let mut stations: Vec<&nav::Station> = nav::db().iter().filter(|s| s.ident == up).collect();
                if let Some(r) = near {
                    stations.sort_by(|a, b| flightplan::dist(r, (a.lat, a.lon)).total_cmp(&flightplan::dist(r, (b.lat, b.lon))));
                }
                let apts = db.search(&word, 3);
                if !apts.is_empty() || !stations.is_empty() {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                        for i in apts {
                            let a = &db.apts[i as usize];
                            if ui.add(egui::Button::new(RichText::new(format!("{}  {}", a.ident, a.name.chars().take(24).collect::<String>())).font(menu::caption()))).clicked() {
                                self.plan.push(flightplan::Waypoint::airport(i));
                                self.plan_text.clear();
                            }
                        }
                        for st in stations.into_iter().take(3) {
                            let w = flightplan::Waypoint::station(st);
                            if ui.add(egui::Button::new(RichText::new(format!("{} {}  {}", w.kind.tag(), w.ident, w.name.chars().take(18).collect::<String>())).font(menu::caption()))).clicked() {
                                self.plan.push(w);
                                self.plan_text.clear();
                            }
                        }
                    });
                }
            }
            if let Some(text) = add {
                let bad = self.plan.add_route(&text, dep);
                self.plan_msg = if bad.is_empty() { String::new() } else { t!("game.not_found", bad.join(", ")) };
                self.plan_text = bad.join(" ");
            }
            if !self.plan_msg.is_empty() {
                ui.label(RichText::new(&self.plan_msg).font(menu::caption()).color(Color32::from_rgb(255, 130, 90)));
            }
            ui.add_space(4.0);
            menu::toggle(ui, &mut self.plan_pick, t!("game.add_waypoints_on_the_globe"));
            let how = if padui::active() {
                t!("game.while_on_x_adds_the_airport")
            } else {
                t!("game.while_on_a_click_adds_the")
            };
            ui.label(RichText::new(how).font(menu::caption()).color(DIM));
            if self.plan.is_empty() {
                return;
            }
            ui.add_space(8.0);
            // route list.
            let legs = self.plan.legs(dep);
            let start = match dep {
                Some(_) => self.menu.airport.map(|i| airports::Db::get().apts[i as usize].ident.clone()).unwrap_or_else(|| t!("game.start_point").to_string()),
                None => t!("game.pick_a_start_point").to_string(),
            };
            ui.label(RichText::new(format!(" ●  {start}")).font(menu::data()).color(DIM));
            let n = self.plan.wps.len();
            let mut act: Option<(usize, i8)> = None;
            for (i, wp) in self.plan.wps.iter().enumerate() {
                // no start, no first leg.
                let leg = if dep.is_some() { legs.get(i) } else { i.checked_sub(1).and_then(|k| legs.get(k)) };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(RichText::new(format!("{:>2}", i + 1)).font(menu::data()).color(ROUTE_COLOUR));
                    let tip = match &wp.phase {
                        Some((ph, name)) => format!("{} {name}  ·  {}", ph.tag(), wp.name),
                        None => format!("{}  ·  {}\n{}", wp.kind.tag(), wp.name, menu::latlon(wp.lat, wp.lon)),
                    };
                    let col = if wp.phase.is_some() { Color32::from_rgb(170, 220, 255) } else { Color32::WHITE };
                    ui.label(RichText::new(&wp.ident).font(menu::data()).color(col)).on_hover_text(tip);
                    match (wp.leg, leg) {
                        (flightplan::Leg::Fix { .. }, Some(l)) => {
                            ui.label(RichText::new(format!("{:03.0}°  {:.0} nm", l.3, l.2 / 1852.0)).font(menu::caption()).color(TEXT));
                        }
                        (flightplan::Leg::Climb { course, .. } | flightplan::Leg::Intercept { course } | flightplan::Leg::Vectors { course }, _) => {
                            ui.label(RichText::new(format!("HDG {course:03.0}°")).font(menu::caption()).color(TEXT));
                        }
                        _ => {}
                    }
                    if let Some(a) = wp.alt {
                        ui.label(RichText::new(a.text()).font(menu::caption()).color(menu::ACCENT));
                    }
                    if let Some(s) = wp.speed {
                        ui.label(RichText::new(format!("{s:.0} kt")).font(menu::caption()).color(DIM));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let small = |ui: &mut egui::Ui, t: &str, en: bool| ui.add_enabled(en, egui::Button::new(RichText::new(t).font(menu::caption())).min_size(egui::vec2(22.0, 20.0)));
                        if small(ui, "✕", true).on_hover_text(t!("game.remove")).clicked() {
                            act = Some((i, 0));
                        }
                        if small(ui, "▼", i + 1 < n).clicked() {
                            act = Some((i, 1));
                        }
                        if small(ui, "▲", i > 0).clicked() {
                            act = Some((i, -1));
                        }
                    });
                });
            }
            match act {
                Some((i, 0)) => self.plan.remove(i),
                Some((i, 1)) => self.plan.swap(i, i + 1),
                Some((i, _)) => self.plan.swap(i, i - 1),
                None => {}
            }
            self.proc_pickers(ui, dep);
            ui.add_space(6.0);
            let cruise = self.spec.cruise.max(20.0);
            let total = self.plan.total(dep);
            ui.label(RichText::new(t!("game.total_0_nm_about_at_0", total / 1852.0, flightplan::hm(total / cruise), cruise * 1.943_84)).font(menu::caption()).color(TEXT));
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("game.cruise")).font(menu::label()).color(TEXT));
                ui.add(egui::Slider::new(&mut self.plan.cruise_ft, 1000.0..=45000.0).step_by(500.0).suffix(" ft"));
            });
            ui.horizontal(|ui| {
                if ui.button(t!("game.reverse")).clicked() {
                    self.plan.reverse();
                }
                if ui.button(t!("game.clear")).clicked() {
                    self.plan.clear();
                    self.plan_msg.clear();
                }
                if ui.button(t!("game.copy_route")).on_hover_text(self.plan.route_text()).clicked() {
                    ui.ctx().copy_text(self.plan.route_text());
                }
            });
            ui.label(RichText::new(t!("game.in_flight_gps_on_the_autopilot")).font(menu::caption()).color(DIM));
        });
    }

    /// sid, star and approach pickers for us airports with faa procedures.
    pub(crate) fn proc_pickers(&mut self, ui: &mut egui::Ui, dep: Option<(f64, f64)>) {
        use egui::RichText;
        use flightplan::{Phase, Pick};
        use menu::{DIM, TEXT};
        let db = navdata::Db::get();
        let dep_apt = self.menu.airport.map(|i| airports::Db::get().apts[i as usize].ident.clone());
        let dest_apt = self.plan.destination().map(|w| w.ident.clone());
        // the star's runway follows the approach when there is one.
        let app_rwy = self.plan.app.as_ref().map(|p| navdata::approach_runway(&p.name));
        let rows = [(Phase::Sid, dep_apt.clone(), self.menu.runway.clone()), (Phase::Star, dest_apt.clone(), app_rwy), (Phase::App, dest_apt.clone(), None)];
        let mut change: Option<(Phase, Option<Pick>)> = None;
        let mut any = false;
        for (phase, apt, rwy_hint) in rows {
            let Some(apt) = apt else { continue };
            let kind = match phase {
                Phase::Sid => 'D',
                Phase::Star => 'E',
                Phase::App => 'F',
            };
            let names = db.names(&apt, kind);
            if names.is_empty() {
                continue;
            }
            if !any {
                ui.add_space(8.0);
                ui.label(RichText::new(t!("game.procedures")).font(menu::label()).color(TEXT));
                ui.label(RichText::new(t!("game.real_faa_data_at_us_airports")).font(menu::caption()).color(DIM));
                any = true;
            }
            let cur = match phase {
                Phase::Sid => self.plan.sid.clone(),
                Phase::Star => self.plan.star.clone(),
                Phase::App => self.plan.app.clone(),
            };
            let show = |n: &str| navdata::label(kind, n);
            let title = match phase {
                Phase::Sid => format!("SID {apt}"),
                Phase::Star => format!("STAR {apt}"),
                Phase::App => t!("game.approach_2", apt),
            };
            let none = t!("game.none");
            ui.horizontal(|ui| {
                ui.label(RichText::new(&title).font(menu::caption()).color(DIM));
                let sel = cur.as_ref().map(|p| show(&p.name)).unwrap_or_else(|| none.to_string());
                egui::ComboBox::from_id_salt(("proc", phase.tag())).width(ui.available_width()).selected_text(sel).show_ui(ui, |ui| {
                    if ui.selectable_label(cur.is_none(), none).clicked() {
                        change = Some((phase, None));
                    }
                    for n in &names {
                        if ui.selectable_label(cur.as_ref().is_some_and(|p| &p.name == n), show(n)).clicked() {
                            let rws = db.runway_trans(&apt, kind, n);
                            let rwy = rwy_hint.as_deref().and_then(|r| navdata::trans_for(&rws, r)).or_else(|| rws.first().cloned());
                            change = Some((phase, Some(Pick { apt: apt.clone(), name: n.clone(), rwy, trans: None })));
                        }
                    }
                });
            });
            let Some(p) = cur else { continue };
            let rws = db.runway_trans(&apt, kind, &p.name);
            let others = db.other_trans(&apt, kind, &p.name);
            ui.horizontal(|ui| {
                if !rws.is_empty() && phase != Phase::App {
                    egui::ComboBox::from_id_salt(("proc_rw", phase.tag())).width(90.0).selected_text(p.rwy.clone().unwrap_or_default()).show_ui(ui, |ui| {
                        for r in &rws {
                            if ui.selectable_label(p.rwy.as_ref() == Some(r), r).clicked() {
                                change = Some((phase, Some(Pick { rwy: Some(r.clone()), ..p.clone() })));
                            }
                        }
                    });
                }
                if !others.is_empty() {
                    let none = if phase == Phase::App { t!("game.vectors") } else { t!("game.no_transition") };
                    egui::ComboBox::from_id_salt(("proc_tr", phase.tag())).width(ui.available_width()).selected_text(p.trans.clone().unwrap_or_else(|| none.to_string())).show_ui(ui, |ui| {
                        if ui.selectable_label(p.trans.is_none(), none).clicked() {
                            change = Some((phase, Some(Pick { trans: None, ..p.clone() })));
                        }
                        for t in &others {
                            if ui.selectable_label(p.trans.as_ref() == Some(t), t).clicked() {
                                change = Some((phase, Some(Pick { trans: Some(t.clone()), ..p.clone() })));
                            }
                        }
                    });
                }
            });
        }
        if let Some((phase, pick)) = change {
            self.plan.set_proc(phase, pick, dep);
            // a new approach moves the star onto its runway.
            if phase == Phase::App {
                if let (Some(star), Some(app)) = (self.plan.star.clone(), self.plan.app.clone()) {
                    let rws = db.runway_trans(&star.apt, 'E', &star.name);
                    if let Some(r) = navdata::trans_for(&rws, &navdata::approach_runway(&app.name)) {
                        self.plan.set_proc(Phase::Star, Some(flightplan::Pick { rwy: Some(r), ..star }), dep);
                    }
                }
            }
        }
        if any && dest_apt.is_none() {
            ui.label(RichText::new(t!("game.add_a_destination_airport_last_for")).font(menu::caption()).color(DIM));
        }
        if any {
            ui.label(RichText::new(t!("game.heading_legs_fly_a_heading_after")).font(menu::caption()).color(DIM));
        }
    }

    /// the autopilot page: its modes as keys, then the four settings it flies to.
    pub(crate) fn autopilot_page(&mut self, ui: &mut egui::Ui) {
        use autopilot::{Lat, Vert};
        use egui::Color32;
        use flightui::{big, cells, key, note, tag, wide_tile, GAP, LIT};
        let s = autopilot::State::of(&self.plane);
        let (on, armed) = self.ap.annunciation();
        let mut lat = None;
        let mut vert = None;
        let mut toggle = false;
        let mut at = false;
        wide_tile(ui, |ui| {
            ui.horizontal(|ui| {
                tag(ui, t!("game.modes"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let line = if !self.ap.on {
                        t!("game.off_f5_turns_it_on").to_string()
                    } else if armed.is_empty() {
                        on.join("  ")
                    } else {
                        t!("game.armed_2", on.join("  "), armed.join("  "))
                    };
                    ui.label(egui::RichText::new(line).font(menu::caption()).color(if self.ap.on { LIT } else { flightui::SOFT }));
                });
            });
            ui.add_space(2.0);
            let n = 8.0;
            let w = (ui.available_width() - GAP * (n - 1.0)) / n;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                toggle = key(ui, "AP", self.ap.on, w, 40.0).on_hover_text("F5").clicked();
                let gps_tip = t!("game.gps_follows_the_flight_plan");
                for (m, name, k) in [(Lat::Hdg, "HDG", "7"), (Lat::Nav, "NAV", "8"), (Lat::Gps, "GPS", gps_tip), (Lat::Apr, "APR", "9")] {
                    if key(ui, name, self.ap.on && self.ap.lat == m, w, 40.0).on_hover_text(k).clicked() {
                        lat = Some(m);
                    }
                }
                for (m, name, k) in [(Vert::Alt, "ALT", "0"), (Vert::Vs, "VS", "")] {
                    let r = key(ui, name, self.ap.on && self.ap.vert == m, w, 40.0);
                    if (if k.is_empty() { r } else { r.on_hover_text(k) }).clicked() {
                        vert = Some(m);
                    }
                }
                at = key(ui, "AT", self.ap.at, w, 40.0).on_hover_text(t!("game.autothrottle")).clicked();
            });
        });
        if at {
            self.ap_at_toggle();
        }
        if toggle {
            self.ap_toggle();
        }
        if let Some(m) = lat {
            self.ap_lat(m);
        }
        if let Some(m) = vert {
            self.ap_vert(m);
        }
        // the settings: a big figure, then less, sync to the aircraft's own, more.
        let values = [
            ("HDG", format!("{:03.0}", self.ap.hdg.rem_euclid(360.0)), "°"),
            ("ALT", format!("{:.0}", self.ap.alt * 3.28084), "ft"),
            ("VS", format!("{:+.0}", self.ap.vs * 196.85), "fpm"),
            ("SPD", format!("{:.0}", self.ap.spd * 1.943_84), "kt"),
        ];
        let mut steps = [(false, false, false); 4];
        cells(ui, 4, 112.0, |ui, i| {
            let (name, v, unit) = &values[i];
            tag(ui, name);
            big(ui, v, unit, Color32::WHITE);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let side = 30.0;
                let mid = ui.available_width() - 2.0 * side - 8.0;
                steps[i].0 = hold_repeat(&key(ui, "−", false, side, 28.0));
                steps[i].2 = key(ui, t!("game.sync"), false, mid, 28.0).on_hover_text(t!("game.set_to_what_the_aircraft")).clicked();
                steps[i].1 = hold_repeat(&key(ui, "+", false, side, 28.0));
            });
        });
        let sign = |(m, p, _): (bool, bool, bool)| if p { 1.0 } else if m { -1.0 } else { 0.0 };
        let d = sign(steps[0]);
        if d != 0.0 {
            self.ap.hdg = (self.ap.hdg + 5.0 * d).rem_euclid(360.0);
        }
        if steps[0].2 {
            self.ap.hdg = s.hdg.round();
        }
        let d = sign(steps[1]);
        if d != 0.0 {
            self.ap.alt = (((self.ap.alt * 3.28084 / 100.0).round() * 100.0 + 100.0 * d).max(0.0)) / 3.28084;
        }
        if steps[1].2 {
            self.ap.alt = (s.alt * 3.28084 / 100.0).round() * 100.0 / 3.28084;
        }
        let d = sign(steps[2]);
        if d != 0.0 {
            self.ap_vs_step(d > 0.0);
        }
        if steps[2].2 {
            self.ap.vs = (s.vs * 196.85 / 100.0).round() * 100.0 / 196.85;
        }
        let d = sign(steps[3]);
        if d != 0.0 {
            self.ap_spd_step(d > 0.0);
        }
        if steps[3].2 {
            self.ap.spd = s.ias;
        }
        note(ui, t!("game.nav_and_apr_follow_nav"));
        self.route_tile(ui);
    }

    /// in flight route tile.
    pub(crate) fn route_tile(&mut self, ui: &mut egui::Ui) {
        use egui::{Color32, RichText};
        use flightui::{key, tag, wide_tile, GAP, LIT, SOFT};
        if self.plan.is_empty() {
            return;
        }
        let (lat, lon, _) = geo::pos_to_lla(self.plane.pos);
        let at = (lat.to_degrees(), lon.to_degrees());
        let up = self.plane.pos.normalize();
        let gs = (self.plane.vel - up * self.plane.vel.dot(up)).length();
        let mut direct = None;
        let mut restart = false;
        wide_tile(ui, |ui| {
            ui.horizontal(|ui| {
                tag(ui, t!("game.flight_plan"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let line = match (self.plan.steer(at), self.plan.ete(at, gs)) {
                        (Some(st), Some((_, all))) => t!("game.xtk_1_nm_to_the_end", st.xtk.abs() / 1852.0, if st.xtk >= 0.0 { "R" } else { "L" }, flightplan::hm(all)),
                        (Some(st), None) => format!("XTK {:.1} nm {}", st.xtk.abs() / 1852.0, if st.xtk >= 0.0 { "R" } else { "L" }),
                        _ => t!("game.all_waypoints_passed").to_string(),
                    };
                    ui.label(RichText::new(line).font(menu::caption()).color(SOFT));
                });
            });
            ui.add_space(2.0);
            for (i, wp) in self.plan.wps.iter().enumerate() {
                let active = i == self.plan.active;
                let passed = i < self.plan.active;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    let col = if active { ROUTE_COLOUR } else if passed { SOFT } else { Color32::WHITE };
                    ui.label(RichText::new(format!("{}{:>2}  {:<6}", if active { "▶" } else { " " }, i + 1, wp.ident)).font(menu::data()).color(col));
                    let d = flightplan::dist(at, wp.at()) / 1852.0;
                    let brg = flightplan::course(at, wp.at());
                    ui.label(RichText::new(format!("{:03.0}°  {:>6.1} nm", brg, d)).font(menu::data()).color(if passed { SOFT } else { LIT }));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if key(ui, t!("game.direct"), active, 70.0, 24.0).on_hover_text(t!("game.fly_straight_to_it_from_here")).clicked() {
                            direct = Some(i);
                        }
                    });
                });
            }
            if self.plan.done() {
                ui.add_space(4.0);
                restart = key(ui, t!("game.fly_it_again_from_the_first"), false, ui.available_width(), 28.0).clicked();
            }
        });
        if let Some(i) = direct {
            self.plan.direct(i, at);
            self.status = t!("game.direct_to", self.plan.wps[i].ident);
            self.status_until = self.real + 3.0;
        }
        if restart {
            self.plan.direct(0, at);
        }
    }

    /// the radio page: the two receivers, the stations near, and the altimeter.
    pub(crate) fn radio_page(&mut self, ui: &mut egui::Ui) {
        use egui::{Color32, RichText};
        use flightui::{big, cells, key, note, pick_row, tag, wide_tile, LIT, SOFT};
        use nav::Kind;
        let pos = self.plane.pos;
        let (la, lo, _) = geo::pos_to_lla(pos);
        let (lat, lon) = (la.to_degrees(), lo.to_degrees());
        let nav = self.radios.nav.clone();
        let adf = self.radios.adf.clone();
        let reading = nav.as_ref().map(|st| nav::nav_reading(st, self.radios.obs, pos));
        let hdg = { let (e, n, _) = geo::enu(pos); let f = self.plane.forward(); f.dot(e).atan2(f.dot(n)).to_degrees() };
        let adf_brg = adf.as_ref().and_then(|st| nav::adf_bearing(st, pos, hdg));
        let mut nav_off = false;
        let mut adf_off = false;
        let mut obs_step = 0.0;
        let mut direct = false;
        cells(ui, 2, 132.0, |ui, i| {
            let off = |ui: &mut egui::Ui| {
                let mut hit = false;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    hit = key(ui, t!("game.off"), false, 64.0, 22.0).clicked();
                });
                hit
            };
            if i == 0 {
                let mut hit = false;
                ui.horizontal(|ui| {
                    tag(ui, "NAV 1  ·  VOR / ILS");
                    if nav.is_some() {
                        hit = off(ui);
                    }
                });
                nav_off = hit;
                match (&nav, &reading) {
                    (Some(st), Some(r)) => {
                        // an ils without a frequency goes by its runway.
                        let f = st.freq_text();
                        let has_freq = f.starts_with(|c: char| c.is_ascii_digit());
                        if has_freq {
                            big(ui, &f, "MHz", LIT);
                        } else {
                            big(ui, &st.label(), "", LIT);
                        }
                        let mut state = if !r.live {
                            t!("game.no_signal").to_string()
                        } else {
                            match r.to { Some(true) => "TO".into(), Some(false) => "FROM".into(), None => "LOC".to_string() }
                        };
                        if r.live && r.gs.is_some() {
                            state += "  ·  G/S";
                        }
                        if let Some(d) = r.dme.filter(|_| r.live) {
                            state += &format!("  ·  DME {d:.1}");
                        }
                        note(ui, &if has_freq { format!("{}  ·  {state}", st.label()) } else { format!("{f}  ·  {state}") });
                        ui.add_space(2.0);
                        if st.kind == Kind::Ils {
                            note(ui, &t!("game.course_03_0_2", st.course));
                        } else {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (txt, d) in [("−10", -10.0), ("−1", -1.0)] {
                                    if key(ui, txt, false, 36.0, 24.0).clicked() {
                                        obs_step = d;
                                    }
                                }
                                ui.label(RichText::new(format!("OBS {:03.0}°", self.radios.obs)).font(menu::semibold(14.0)).color(Color32::WHITE));
                                for (txt, d) in [("+1", 1.0), ("+10", 10.0)] {
                                    if key(ui, txt, false, 36.0, 24.0).clicked() {
                                        obs_step = d;
                                    }
                                }
                                direct = key(ui, t!("game.direct"), false, ui.available_width(), 24.0).clicked();
                            });
                        }
                    }
                    _ => {
                        big(ui, "---.--", "MHz", SOFT);
                        note(ui, t!("game.pick_a_station_below"));
                    }
                }
            } else {
                let mut hit = false;
                ui.horizontal(|ui| {
                    tag(ui, "ADF  ·  NDB");
                    if adf.is_some() {
                        hit = off(ui);
                    }
                });
                adf_off = hit;
                match &adf {
                    Some(st) => {
                        big(ui, &st.freq_text(), "kHz", LIT);
                        note(ui, &st.label());
                        match adf_brg {
                            Some(b) => note(ui, &t!("game.needle_03_0_relative", b.rem_euclid(360.0))),
                            None => note(ui, t!("game.no_signal")),
                        }
                    }
                    None => {
                        big(ui, "---", "kHz", SOFT);
                        note(ui, t!("game.pick_a_beacon_below"));
                    }
                }
            }
        });
        if nav_off {
            self.radios.nav = None;
        }
        if adf_off {
            self.radios.adf = None;
        }
        if let Some(st) = &nav {
            if obs_step != 0.0 {
                self.radios.obs = (self.radios.obs.round() + obs_step).rem_euclid(360.0);
            }
            if direct {
                self.radios.obs = nav::bearing(pos, st.pos()).round();
            }
        }
        // the stations near, a row to tune each.
        let mut vors = nav::near(lat, lon, 200_000.0, Kind::Vor);
        vors.truncate(8);
        let mut ils = nav::near(lat, lon, 40_000.0, Kind::Ils);
        ils.truncate(8);
        vors.extend(ils);
        vors.sort_by(|a, b| a.1.total_cmp(&b.1));
        vors.truncate(7);
        let mut ndbs = nav::near(lat, lon, 150_000.0, Kind::Ndb);
        ndbs.truncate(7);
        let rows = vors.len().max(ndbs.len()).max(1) as f32;
        let mut tune_nav = None;
        let mut tune_adf = None;
        cells(ui, 2, 30.0 + rows * 32.0, |ui, i| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let (list, now, empty) = if i == 0 { (&vors, &self.radios.nav, t!("game.no_vor_or_ils_in")) } else { (&ndbs, &self.radios.adf, t!("game.no_beacon_in_range")) };
            tag(ui, if i == 0 { t!("game.stations_near") } else { t!("game.beacons_near") });
            if list.is_empty() {
                note(ui, empty);
            }
            for (st, d) in list {
                let on = now.as_ref() == Some(st);
                if pick_row(ui, &st.freq_text(), &st.label(), &format!("{:.0} km", d / 1000.0), on).clicked() && !on {
                    if i == 0 { tune_nav = Some(st.clone()) } else { tune_adf = Some(st.clone()) }
                }
            }
        });
        if let Some(st) = tune_nav {
            self.radios.tune_nav(st, pos);
        }
        if let Some(st) = tune_adf {
            self.radios.tune_adf(st);
        }
        // the altimeter: its setting large, the keys to change it at the right.
        let qnh = self.wx.qnh;
        let set = self.radios.setting(qnh);
        let src = if self.wx.live && !self.wx.live_station.is_empty() { self.wx.live_station.clone() } else { t!("game.standard").into() };
        let mut baro = self.radios.baro;
        wide_tile(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    tag(ui, t!("game.altimeter"));
                    big(ui, &format!("{set:.0}"), "hPa", Color32::WHITE);
                    note(ui, &format!("QNH {qnh:.0} hPa  ·  {src}"));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if key(ui, "STD", baro == Some(1013.25), 56.0, 32.0).on_hover_text("1013 hPa").clicked() {
                        baro = Some(1013.25);
                    }
                    if key(ui, "QNH", baro.is_none(), 56.0, 32.0).on_hover_text("X").clicked() {
                        baro = None;
                    }
                    if key(ui, "+1", false, 44.0, 32.0).clicked() {
                        baro = Some(set.round() + 1.0);
                    }
                    if key(ui, "−1", false, 44.0, 32.0).clicked() {
                        baro = Some(set.round() - 1.0);
                    }
                });
            });
        });
        self.radios.baro = baro;
    }

    /// The data credits for the corner of the screen (the Google Maps logo
    /// goes before them while Google's data is shown, see `credits`).
    pub(crate) fn attribution(&self) -> String {
        let g = &self.src.google;
        let p = self.world.photo();
        let photo = if p.zone_r > 0.0 && p.stats.0 > 0 { format!("3D: {}  ·  ", if p.credits.is_empty() { "Google" } else { &p.credits }) } else { String::new() };
        let mut live = if self.live.enabled() && !self.live.planes.is_empty() { format!("  ·  {}", t!("game.traffic_adsb_lol_odbl")) } else { String::new() };
        if self.wx.live && (!self.wx.live_cells.is_empty() || self.wx.deck_goal.is_some()) {
            live += t!("game.weather_radar_rainviewer");
        }
        if self.mode != Mode::Menu && self.snow == snow::Mode::Auto && self.snowmap.shown().is_some() {
            live += t!("game.snow_cover_ims_u_s");
        }
        // The hand-made Antwerp scenery is built on Flemish open data (Modellicentie
        // voor gratis hergebruik: GRB footprints, DHMV II heights, the 2025 orthophoto).
        if self.mode != Mode::Menu && self.n_scenery > 0 {
            live += "  ·  EBAW: Bron: GRB, DHMV II, Luchtopnamen Digitaal Vlaanderen";
        }
        if self.mode == Mode::Menu && self.gclouds.shown {
            live += t!("game.clouds_live_cloud_maps_matt");
        }
        photo + &if g.enabled() {
            let c = g.attribution.lock();
            let c = if c.is_empty() { t!("game.imagery_google").to_string() } else { c.clone() };
            format!("{c}  ·  {}", attribution_rest())
        } else {
            format!("{}  ·  {}", attribution_default(), attribution_rest())
        } + &live
    }

    /// The bottom-right credits: Google's logo (its official outlined
    /// artwork, unaltered, 22 px with its outline, clear space round it) while
    /// their imagery or 3D tiles are on screen, then the data credits.
    pub(crate) fn credits(&mut self, ctx: &egui::Context) {
        use egui::{Align2, Color32, RichText};
        // (Aircraft-picker stills show the aircraft alone.)
        if still_mode() { return; }
        let logo = self.google_shown().then(|| {
            self.gm_logo
                .get_or_insert_with(|| {
                    let img = image::load_from_memory(include_bytes!("../../assets/google_maps_logo.png")).expect("logo").to_rgba8();
                    let size = [img.width() as usize, img.height() as usize];
                    ctx.load_texture("google maps logo", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
                })
                .clone()
        });
        let text = self.attribution();
        // The logo on a line of its own at the right, above the credits (which
        // can run long), so nothing else on screen comes near it.
        egui::Area::new(egui::Id::unique("attr")).anchor(Align2::RIGHT_BOTTOM, [-10.0, -5.0]).show(ctx, |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                ui.label(RichText::new(text).size(10.0).color(Color32::from_rgba_unmultiplied(255, 255, 255, 110)));
                if let Some(t) = logo {
                    let s = t.size_vec2();
                    ui.add_space(4.0);
                    ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(s.x * 22.0 / s.y, 22.0)).alt_text("Google Maps"));
                }
            });
        });
    }

    /// Whether Google's data is on screen (its credit is shown, HUD or not).
    pub(crate) fn google_shown(&self) -> bool {
        self.src.google.enabled() || self.world.photo().stats.0 > 0
    }

    /// The pause menu (Esc): full screen over the dimmed, stopped game. A
    /// column of big buttons on the left; the key list and the failures open
    /// in a panel beside it; Settings opens the settings screen (settings_ui.rs).
    pub(crate) fn pause_menu(&mut self, ctx: &egui::Context, act: &mut Option<PauseAct>, lat: f64, lon: f64, local: f64) {
        use egui::{Align2, Color32, RichText};
        use menu::{Page, ACCENT, DIM, TEXT};
        if self.pause_page.is_info() {
            self.pause_page = Page::Game;
        }
        if self.pause_page == Page::Settings {
            if settings_ui::show(ctx, self, true) {
                self.pause_page = Page::Game;
                self.pad_home = true;
            }
            return;
        }
        // The backdrop dims the scene (darkest behind the buttons) and keeps the mouse from the camera behind it.
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("pause_dim")).order(egui::Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_response(screen.size(), egui::Sense::click());
            let p = ui.painter();
            p.rect_filled(screen, 0.0, Color32::from_black_alpha(150));
            let mut mesh = egui::Mesh::default();
            let (l, r) = (Color32::from_black_alpha(130), Color32::TRANSPARENT);
            let mid = screen.left() + screen.width() * 0.6;
            mesh.colored_vertex(screen.left_top(), l);
            mesh.colored_vertex(egui::pos2(mid, screen.top()), r);
            mesh.colored_vertex(egui::pos2(mid, screen.bottom()), r);
            mesh.colored_vertex(screen.left_bottom(), l);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            p.add(mesh);
        });
        let pad = padui::active();
        let margin = if screen.width() < 1100.0 { 40.0 } else { 72.0 };
        let col_w = 380.0f32.min(screen.width() * 0.4);
        egui::Area::new(egui::Id::unique("pause")).order(egui::Order::Foreground).anchor(Align2::LEFT_CENTER, [margin, 0.0]).show(ctx, |ui| {
            ui.set_width(col_w);
            menu::screen_heading(ui, t!("game.paused"));
            ui.add_space(22.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            let page = self.pause_page;
            let resume = menu::big_button(ui, t!("game.resume"), Hint::of("Esc", "B"), true, false);
            if resume.clicked() {
                *act = Some(PauseAct::Resume);
            }
            if page == Page::Game {
                self.pad_home_focus(&resume);
            }
            ui.add_space(8.0);
            for (p, text) in [(Page::Settings, t!("game.settings")), (Page::Controls, t!("game.controls_reference")), (Page::Failures, t!("game.failures_2")), (Page::Logbook, t!("log.logbook"))] {
                let b = menu::big_button(ui, text, Hint::None, false, page == p);
                if b.clicked() {
                    self.pause_page = if page == p && p != Page::Settings { Page::Game } else { p };
                    self.pad_home = p == Page::Settings;
                }
                if page == p {
                    self.pad_home_focus(&b);
                }
            }
            ui.add_space(14.0);
            if menu::big_button(ui, t!("game.respawn_in_the_air"), Hint::of("R", ""), false, false).clicked() {
                *act = Some(PauseAct::Respawn);
            }
            // Leaving: back to the globe for a new start (its Menu button goes on to the main menu).
            if menu::big_button(ui, t!("game.end_flight"), Hint::Text(t!("game.pick_a_new_start")), false, false).clicked() {
                *act = Some(PauseAct::Globe);
            }
            ui.add_space(14.0);
            // Quit, quieter: a plain line rather than a big button.
            let q = ui.add(egui::Button::new(RichText::new(t!("game.quit_to_desktop")).font(menu::label()).color(DIM)).frame(false));
            if q.clicked() {
                *act = Some(PauseAct::Quit);
            }
            ui.add_space(18.0);
            if pad {
                padui::prompts(ui, &[("Dpad", tr_en("move")), ("A", tr_en("press")), ("B", tr_en("back"))], 20.0, 12.0, DIM);
            } else {
                let t = t!("game.while_you_fly_tab_opens");
                ui.add(egui::Label::new(RichText::new(t).font(menu::caption()).color(DIM)).wrap());
            }
        });

        // Where and when, small in the top right corner.
        egui::Area::new(egui::Id::unique("pause_where")).order(egui::Order::Foreground).anchor(Align2::RIGHT_TOP, [-margin * 0.5, margin * 0.5]).show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(RichText::new(self.spec.name).font(menu::item_title()).color(TEXT));
                ui.label(RichText::new(menu::latlon(lat, lon)).font(menu::caption()).color(DIM));
                ui.label(RichText::new(t!("game.local_2", menu::hhmm(local))).font(menu::caption()).color(DIM));
            });
        });

        // The key list or the failures, beside the buttons.
        if matches!(self.pause_page, Page::Controls | Page::Failures | Page::Logbook) {
            let x = margin + col_w + 48.0;
            let w = (screen.width() - x - margin).clamp(300.0, 820.0);
            let h = screen.height() - 2.0 * margin.max(64.0);
            egui::Area::new(egui::Id::unique("pause_page")).order(egui::Order::Foreground).fixed_pos(egui::pos2(x, margin.max(64.0))).show(ctx, |ui| {
                menu::panel().inner_margin(24.0).show(ui, |ui| {
                    ui.set_width(w - 48.0);
                    ui.set_max_height(h - 48.0);
                    let title = match self.pause_page { Page::Controls => t!("game.controls_reference"), Page::Logbook => t!("log.logbook"), _ => t!("game.failures_2") };
                    ui.label(RichText::new(title).font(menu::page_title()).color(Color32::WHITE));
                    ui.add_space(10.0);
                    egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                        if self.pause_page == Page::Controls {
                            if !self.binds.keys.is_empty() {
                                ui.label(RichText::new(t!("game.some_keys_have_been_moved")).font(menu::caption()).color(ACCENT));
                            }
                            menu::controls_list(ui);
                        } else if self.pause_page == Page::Logbook {
                            self.logbook_page(ui);
                        } else {
                            self.failures_page(ui);
                        }
                        padui::follow_focus(ui);
                    });
                });
            });
        }
    }

    /// The flight panel (Tab): a see-through column on the right, over the
    /// game that keeps running: instruments, places to go and the weather.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn info_panel(&mut self, ctx: &egui::Context, act: &mut Option<PauseAct>, lon: f64) {
        use egui::Align2;
        use menu::Page;
        if !self.info_page.is_info() {
            self.info_page = Page::Flight;
        }
        // in the middle, under the heading strip, clear of the alerts at the foot.
        let y = if self.hud_on() { flightui::HEADING_TOP + flightui::HEADING_H + 34.0 } else { 24.0 };
        let max_h = (ctx.content_rect().height() - y - 84.0 - 36.0 - 48.0).max(160.0);
        egui::Area::new(egui::Id::unique("info_panel")).anchor(Align2::CENTER_TOP, [0.0, y]).show(ctx, |ui| {
            // an area only offers last frame's height; a longer page needs the room up front.
            ui.set_max_height(max_h + 120.0);
            flightui::panel(ui, flightui::glass().inner_margin(18.0), |ui| {
                ui.set_width(flightui::CARD_W);
                let (to, close, x) = flightui::card_header(ui, self.info_page);
                self.pad_home_focus(&x);
                if let Some(p) = to {
                    self.info_page = p;
                }
                if close {
                    self.info_open = false;
                }
                ui.add_space(10.0);
                egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                    ui.set_width(flightui::CARD_W);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    match self.info_page {
                        Page::Places => self.places_page(ui, act),
                        Page::Radio => self.radio_page(ui),
                        Page::Autopilot => self.autopilot_page(ui),
                        Page::Weather => self.wx.card(ui, &mut self.utc, &mut self.doy, lon),
                        _ => self.flight_page(ui),
                    }
                    padui::follow_focus(ui);
                });
            });
        });
    }

    /// the flight page: the engine, fuel and power, how the aircraft is set up, and what is wrong.
    pub(crate) fn flight_page(&mut self, ui: &mut egui::Ui) {
        use egui::{Color32, RichText};
        use flightui::{big, cells, key, meter, note, tag, wide_tile, AMBER, LIT, SOFT};
        let bad = menu::BAD;
        let mut engine_btn: Option<bool> = None;
        let p = &self.plane;
        let rpm = p.prop_rate * 60.0 / std::f64::consts::TAU;
        let starting = p.starter_on || (p.prop_rate > 5.0 && !p.spec.is_jet() && p.ignition);
        cells(ui, 3, 126.0, |ui, i| match i {
            0 => {
                tag(ui, t!("game.engine"));
                let (state, col) = if p.running { (t!("game.running_2"), Color32::WHITE) } else if starting { (t!("game.starting_2"), AMBER) } else { (t!("game.off_3"), SOFT) };
                big(ui, state, "", col);
                let detail = if let Some((_, lit, n)) = p.engines() {
                    t!("game.of_lit_0", lit, n, p.throttle * 100.0)
                } else if p.spec.is_jet() {
                    format!("N1 {:.0}%  ·  {} {:.0}%", p.n1 * 100.0, t!("game.thrust_2"), p.throttle * 100.0)
                } else {
                    format!("{rpm:.0} rpm  ·  {} {:.0}%", t!("game.throttle_2"), p.throttle * 100.0)
                };
                note(ui, &detail);
                ui.add_space(2.0);
                let on = p.running || p.starter_on;
                if key(ui, if on { t!("game.shut_down") } else { t!("game.start") }, false, ui.available_width(), 28.0).on_hover_text("F4").clicked() {
                    engine_btn = Some(!on);
                }
            }
            1 => {
                tag(ui, t!("game.fuel"));
                let low = p.fuel_frac() < 0.1;
                big(ui, &format!("{:.0}", p.fuel), "kg", if low { AMBER } else { Color32::WHITE });
                ui.add_space(4.0);
                meter(ui, p.fuel_frac() as f32, if low { AMBER } else { LIT });
                ui.add_space(2.0);
                let left = match p.endurance() {
                    Some(e) => t!("game.0_kg_h_h_02", p.fuel_flow * 3600.0, (e / 3600.0) as u32, (e / 60.0) as u32 % 60),
                    None => t!("game.0_full", p.fuel_frac() * 100.0),
                };
                note(ui, &left);
            }
            _ => {
                tag(ui, t!("game.electrics"));
                let el = &p.sys.elec;
                if !el.master {
                    big(ui, t!("game.off_4"), "", SOFT);
                    note(ui, t!("game.ins_turns_the_master_on"));
                } else {
                    big(ui, &format!("{:.1}", el.volts), "V", if p.sys.powered() { Color32::WHITE } else { AMBER });
                    ui.add_space(4.0);
                    meter(ui, el.charge as f32, if el.charge < 0.2 { AMBER } else { LIT });
                    ui.add_space(2.0);
                    note(ui, &t!("game.battery_0_0_a", el.charge * 100.0, el.amps));
                }
            }
        });
        // how it is set up, one small tile each.
        let mut set: Vec<(&str, String, Color32)> = Vec::new();
        let gear = if p.gear >= 1.0 { t!("game.down_3") } else if p.gear <= 0.0 { t!("game.up_2") } else { t!("game.moving_2") };
        set.push((t!("game.gear"), gear.into(), if p.gear > 0.0 && p.gear < 1.0 { AMBER } else { Color32::WHITE }));
        match &p.spec.lever {
            Some(l) => set.push((l.label(), l.notch(p.flap_set).to_string(), Color32::WHITE)),
            None => set.push((t!("game.flaps"), format!("{:.0}°", p.flap_notch_deg()), Color32::WHITE)),
        }
        if p.spec.spoiler_cd > 0.0 {
            let sb = if p.spoilers > 0.6 { t!("game.ground") } else if p.speedbrake > 0.0 { t!("game.out_2") } else { t!("game.in") };
            set.push((t!("game.speedbrake"), sb.into(), Color32::WHITE));
        }
        set.push((t!("game.brake"), if p.parking_brake { t!("game.set").into() } else { t!("game.off_5").into() }, if p.parking_brake { AMBER } else { Color32::WHITE }));
        if !p.spec.is_jet() {
            set.push((t!("game.mixture"), format!("{:.0}%", p.mixture * 100.0), Color32::WHITE));
        }
        if p.spec.chute.is_some() {
            let c = if p.spoilers > 0.0 { t!("game.out_3") } else if p.chute_spent { t!("game.gone_2") } else { t!("game.packed_2") };
            set.push((t!("game.drag_chute_2"), c.into(), Color32::WHITE));
        }
        set.push((t!("game.mass"), format!("{:.0} kg", p.mass()), if p.cg_excess() != 0.0 { bad } else { Color32::WHITE }));
        cells(ui, set.len(), 58.0, |ui, i| {
            let (k, v, col) = &set[i];
            tag(ui, k);
            ui.label(RichText::new(v).font(menu::semibold(17.0)).color(*col));
        });
        // what is wrong, only when something is.
        let mut wrong: Vec<(String, Color32)> = Vec::new();
        if self.crashed {
            wrong.push((t!("game.crashed_r_to_respawn").into(), bad));
        }
        if p.sys.fail.any() {
            for i in (0..systems::Failures::N).filter(|&i| p.sys.fail.get(i)) {
                wrong.push((systems::Failures::label(i).to_string(), bad));
            }
        }
        for l in p.dmg.report() {
            wrong.push((l, AMBER));
        }
        if p.cg_excess() != 0.0 {
            wrong.push((t!("game.out_of_balance").into(), bad));
        }
        if !wrong.is_empty() {
            wide_tile(ui, |ui| {
                tag(ui, t!("game.needs_attention"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for (text, col) in &wrong {
                        egui::Frame::new().fill(col.gamma_multiply(0.18)).corner_radius(flightui::R_IN).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                            ui.label(RichText::new(text).font(menu::medium(13.5)).color(*col));
                        });
                    }
                });
            });
        }
        // who else is flying.
        let me = p.pos;
        let live_n = self.live.planes.len();
        let nearest_live = self.live.planes.iter().min_by(|a, b| (a.pos - me).length().total_cmp(&(b.pos - me).length()));
        let mp_status = self.mp.as_ref().map(|m| m.status());
        let server = self.mp.as_ref().map(|m| self.mp_server_name(&m.server)).unwrap_or_default();
        cells(ui, 2, 92.0, |ui, i| {
            if i == 0 {
                tag(ui, t!("game.live_traffic"));
                if !self.live.enabled() {
                    big(ui, t!("game.off_6"), "", SOFT);
                    note(ui, t!("game.settings_general"));
                } else {
                    big(ui, &live_n.to_string(), t!("game.aircraft_3"), Color32::WHITE);
                    match nearest_live {
                        Some(lp) => note(ui, &t!("game.nearest_1_km", lp.call, (lp.pos - me).length() / 1000.0)),
                        None => note(ui, t!("game.waiting_for_data")),
                    }
                }
            } else {
                tag(ui, t!("game.multiplayer"));
                match &mp_status {
                    Some(mp::Status::Connected) => {
                        big(ui, &self.mp_players.len().to_string(), t!("game.pilots_nearby"), Color32::WHITE);
                        note(ui, &server);
                    }
                    Some(mp::Status::Connecting) => {
                        big(ui, t!("game.connecting"), "", AMBER);
                        note(ui, &server);
                    }
                    Some(mp::Status::Error(e)) => {
                        big(ui, t!("game.no_connection_2"), "", bad);
                        note(ui, e);
                    }
                    None if self.mp_on => {
                        big(ui, t!("game.searching"), "", AMBER);
                        note(ui, t!("game.for_the_fastest_server"));
                    }
                    None => {
                        big(ui, t!("game.off_6"), "", SOFT);
                        note(ui, t!("game.esc_settings_multiplayer"));
                    }
                }
            }
        });
        match engine_btn {
            Some(true) => self.plane.start_engine(),
            Some(false) => self.plane.stop_engine(),
            None => {}
        }
    }

    /// the go to page: start again here, find a place, the airports near and far.
    pub(crate) fn places_page(&mut self, ui: &mut egui::Ui, act: &mut Option<PauseAct>) {
        use flightui::{cells, chip, key, note, tag, GAP};
        let w = (ui.available_width() - GAP) * 0.5;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            if key(ui, t!("game.respawn_here"), false, w, 40.0).on_hover_text("R").clicked() {
                *act = Some(PauseAct::Respawn);
            }
            if key(ui, t!("game.search_a_place"), true, w, 40.0).on_hover_text("G").clicked() {
                *act = Some(PauseAct::GoTo);
            }
        });
        ui.add_space(10.0);
        tag(ui, t!("game.nearest_airports"));
        let db = airports::Db::get();
        let (la, lo, _) = geo::pos_to_lla(self.plane.pos);
        let near: Vec<(u32, f64)> = db.near(la.to_degrees(), lo.to_degrees(), 120_000.0).into_iter().filter(|(i, _)| { let a = &db.apts[*i as usize]; a.any_paved() || a.kind > 0 }).take(6).collect();
        if near.is_empty() {
            note(ui, t!("game.no_airfield_within_120_km"));
        }
        let here = geo::enu(self.plane.pos);
        for row in near.chunks(3) {
            cells(ui, 3, 96.0, |ui, k| {
                let Some(&(i, d)) = row.get(k) else { return };
                let a = &db.apts[i as usize];
                let to = geo::lla_to_pos(a.lat.to_radians(), a.lon.to_radians(), 0.0) - self.plane.pos;
                let brg = to.dot(here.0).atan2(to.dot(here.1)).to_degrees().rem_euclid(360.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&a.ident).font(menu::semibold(17.0)).color(egui::Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("{:.0} km  {brg:03.0}°", d / 1000.0)).font(menu::caption()).color(flightui::SOFT));
                    });
                });
                note(ui, &a.name);
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let bw = (ui.available_width() - 6.0) * 0.5;
                    if key(ui, t!("game.runway"), false, bw, 26.0).on_hover_text(t!("game.line_up_on_the_runway")).clicked() {
                        *act = Some(PauseAct::Airport(i, false));
                    }
                    if key(ui, t!("game.final"), false, bw, 26.0).on_hover_text(t!("game.start_on_a_final_approach")).clicked() {
                        *act = Some(PauseAct::Airport(i, true));
                    }
                });
            });
        }
        ui.add_space(8.0);
        tag(ui, t!("game.airports_around_the_world"));
        let hint = if padui::active() { "" } else { t!("game.shift_click_for_a_final") };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (code, name) in menu::AIRPORTS {
                if let Some(i) = db.find(code) {
                    if chip(ui, code, true, false).on_hover_text(format!("{}{hint}", tr_en(name))).clicked() {
                        *act = Some(PauseAct::Airport(i, ui.input(|inp| inp.modifiers.shift)));
                    }
                }
            }
        });
        ui.add_space(10.0);
        tag(ui, t!("game.fly_to"));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (name, la, lo) in menu::QUICK {
                if chip(ui, tr_en(name), false, false).clicked() {
                    *act = Some(PauseAct::FlyTo(*la, *lo, name));
                }
            }
        });
    }
}
