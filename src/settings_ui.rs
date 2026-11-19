//! The settings screen: one full-window page shared by the start screen and
//! the pause menu. A sidebar of sections on the left (painted icons, no
//! glyphs the bundled font might not have), the chosen section's cards on
//! the right in a column of at most `MAX_W` points.
//!
//! Entry point: [`show`] (or `Game::settings_screen`). It draws the screen
//! over everything and returns true when it is closed with its Back button;
//! Esc and the gamepad's B are handled by the caller's key handling (they
//! close it the same way). The section shown is `Game::settings_sec`.
//!
//! Every setting lives here except the ones that only make sense in flight
//! (failures, in the pause menu). The values themselves are saved where
//! they always were: settings.json (settings.rs, compared each frame by
//! `keep_settings`), graphics.json (graphics.rs), bindings (bind.rs) and the
//! Google key (gmaps.rs).
use crate::menu::{self, ACCENT, DIM, TEXT};
use crate::padui::{self, Hint};
use crate::{bind, geo, gmaps, i18n, mp, mp_proto, padmap, t, tiles3d, Game};
use egui::{pos2, vec2, Align, Align2, Color32, Layout, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui};

/// The widest the content column gets, however wide the window.
pub const MAX_W: f32 = 780.0;

/// A section of the settings screen.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Section {
    General,
    Graphics,
    Sound,
    Camera,
    Controls,
    Map,
    Multiplayer,
}

impl Section {
    pub const ALL: [Section; 7] = [Section::General, Section::Graphics, Section::Sound, Section::Camera, Section::Controls, Section::Map, Section::Multiplayer];

    /// The name the command line goes by (`--settings graphics`, `--pause-menu settings:graphics`).
    pub fn name(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::Graphics => "graphics",
            Section::Sound => "sound",
            Section::Camera => "camera",
            Section::Controls => "controls",
            Section::Map => "map",
            Section::Multiplayer => "multiplayer",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::General => t!("General", "Général"),
            Section::Graphics => t!("Graphics", "Graphismes"),
            Section::Sound => t!("Sound", "Son"),
            Section::Camera => t!("Camera and view", "Caméra et vue"),
            Section::Controls => t!("Controls", "Commandes"),
            Section::Map => t!("Map and data", "Carte et données"),
            Section::Multiplayer => t!("Multiplayer", "Multijoueur"),
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Section::General => t!("Language, the HUD, the minimap, live traffic and arcade mode.", "Langue, HUD, mini-carte, trafic réel et mode arcade."),
            Section::Graphics => t!("How hard the renderer works. Changes apply at once.", "L'effort demandé au rendu. Les changements s'appliquent aussitôt."),
            Section::Sound => t!("Engine, wind, warnings and the world around you.", "Moteur, vent, alarmes et le monde autour de vous."),
            Section::Camera => t!("Where the camera starts and how wide the cockpit view is.", "Le point de vue de départ et l'ouverture de la vue cockpit."),
            Section::Controls => t!("Mouse, joysticks, pedals, gamepads and keys.", "Souris, joysticks, palonniers, manettes et touches."),
            Section::Map => t!("Map imagery, Google's 3D cities and real-world data.", "Imagerie, villes 3D de Google et données réelles."),
            Section::Multiplayer => t!("Fly with the other players, and see on the globe where they are.", "Volez avec les autres joueurs, et voyez sur le globe où ils sont."),
        }
    }

    pub fn parse(s: &str) -> Option<Section> {
        Some(match s.trim().to_lowercase().as_str() {
            "general" | "game" | "hud" | "language" => Section::General,
            "graphics" | "gfx" | "video" => Section::Graphics,
            "sound" | "audio" => Section::Sound,
            "camera" | "view" | "cam" => Section::Camera,
            "controls" | "bindings" | "input" | "keys" => Section::Controls,
            "map" | "data" | "imagery" | "google" => Section::Map,
            "multiplayer" | "mp" | "online" | "network" => Section::Multiplayer,
            _ => return None,
        })
    }

    /// The section before or after this one, round and round.
    pub fn cycle(self, next: bool) -> Section {
        let n = Section::ALL.len();
        let i = Section::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Section::ALL[(i + if next { 1 } else { n - 1 }) % n]
    }

    /// A small line icon for the sidebar, `s` points across, centred on `c`.
    pub(crate) fn icon(self, p: &Painter, c: Pos2, s: f32, col: Color32) {
        let st = Stroke::new(1.6, col);
        let u = s / 2.0;
        match self {
            // three sliders
            Section::General => {
                for (i, k) in [(-1.0, 0.3), (0.0, -0.4), (1.0, 0.15)] {
                    let y = c.y + i * u * 0.65;
                    p.line_segment([pos2(c.x - u, y), pos2(c.x + u, y)], st);
                    p.circle_filled(pos2(c.x + k * u, y), u * 0.22, col);
                }
            }
            // a monitor on its stand
            Section::Graphics => {
                let r = Rect::from_center_size(c - vec2(0.0, u * 0.15), vec2(s, s * 0.66));
                p.rect_stroke(r, 1.5, st, egui::StrokeKind::Middle);
                p.line_segment([pos2(c.x, r.bottom()), pos2(c.x, r.bottom() + u * 0.3)], st);
                p.line_segment([pos2(c.x - u * 0.45, r.bottom() + u * 0.32), pos2(c.x + u * 0.45, r.bottom() + u * 0.32)], st);
            }
            // a speaker and two waves
            Section::Sound => {
                let x0 = c.x - u;
                let body = vec![pos2(x0, c.y - u * 0.3), pos2(x0 + u * 0.4, c.y - u * 0.3), pos2(x0 + u * 0.95, c.y - u * 0.8), pos2(x0 + u * 0.95, c.y + u * 0.8), pos2(x0 + u * 0.4, c.y + u * 0.3), pos2(x0, c.y + u * 0.3)];
                p.add(egui::Shape::closed_line(body, st));
                for r in [0.45, 0.85] {
                    let pts: Vec<Pos2> = (0..=8).map(|i| {
                        let a = (i as f32 / 8.0 - 0.5) * 1.6;
                        pos2(c.x + u * 0.05 + a.cos() * r * u, c.y + a.sin() * r * u)
                    }).collect();
                    p.line(pts, st);
                }
            }
            // a camera body and its lens
            Section::Camera => {
                let r = Rect::from_center_size(c + vec2(0.0, u * 0.1), vec2(s, s * 0.68));
                p.rect_stroke(r, 2.0, st, egui::StrokeKind::Middle);
                p.line_segment([pos2(c.x - u * 0.35, r.top() - u * 0.2), pos2(c.x + u * 0.35, r.top() - u * 0.2)], st);
                p.circle_stroke(r.center(), u * 0.38, st);
            }
            // a gamepad: its outline, a cross and two buttons
            Section::Controls => {
                let r = Rect::from_center_size(c, vec2(s * 1.05, s * 0.62));
                p.rect_stroke(r, u * 0.3, st, egui::StrokeKind::Middle);
                let d = c - vec2(u * 0.48, 0.0);
                p.line_segment([d - vec2(u * 0.22, 0.0), d + vec2(u * 0.22, 0.0)], st);
                p.line_segment([d - vec2(0.0, u * 0.22), d + vec2(0.0, u * 0.22)], st);
                p.circle_filled(c + vec2(u * 0.38, u * 0.1), u * 0.11, col);
                p.circle_filled(c + vec2(u * 0.6, -u * 0.12), u * 0.11, col);
            }
            // a globe: its rim, a meridian and the equator
            Section::Map => {
                p.circle_stroke(c, u * 0.95, st);
                p.line_segment([c - vec2(u * 0.95, 0.0), c + vec2(u * 0.95, 0.0)], st);
                let pts: Vec<Pos2> = (0..=16).map(|i| {
                    let a = (i as f32 / 16.0) * std::f32::consts::TAU;
                    c + vec2(a.cos() * u * 0.4, a.sin() * u * 0.95)
                }).collect();
                p.line(pts, st);
            }
            // two people, one a step behind the other
            Section::Multiplayer => {
                for (dx, k) in [(u * 0.42, 0.82), (-u * 0.22, 1.0)] {
                    let h = c + vec2(dx, -u * 0.38 * k);
                    p.circle_stroke(h, u * 0.3 * k, st);
                    let pts: Vec<Pos2> = (0..=10).map(|i| {
                        let a = std::f32::consts::PI * (1.0 + i as f32 / 10.0);
                        pos2(c.x + dx + a.cos() * u * 0.62 * k, c.y + u * 0.88 + a.sin() * u * 0.7 * k)
                    }).collect();
                    p.line(pts, st);
                }
            }
        }
    }
}

// ---- building blocks for the sections (graphics.rs uses them too)

/// A titled group of settings: a faint card with a small accent title.
pub fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(menu::SURFACE)
        .stroke(Stroke::new(1.0, menu::LINE))
        .corner_radius(menu::R_PANEL)
        .inner_margin(egui::Margin { left: 22, right: 22, top: 16, bottom: 14 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(menu::section_text(title));
            ui.add_space(12.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            body(ui);
        });
    ui.add_space(16.0);
}

/// One `label | control` row: the label (and a short dim hint under it) in
/// a fixed left column, the control to its right.
pub fn row(ui: &mut Ui, label: &str, hint: &str, add: impl FnOnce(&mut Ui)) {
    let w = ui.available_width();
    let lw = (w * 0.34).clamp(150.0, 250.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(lw, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(lw - 12.0);
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add_space(3.0);
            ui.add(egui::Label::new(RichText::new(label).font(menu::label()).color(TEXT)).wrap());
            if !hint.is_empty() {
                ui.add(egui::Label::new(RichText::new(hint).font(menu::caption()).color(DIM)).wrap());
            }
        });
        ui.vertical(|ui| {
            ui.set_width((w - lw - 8.0).max(100.0));
            add(ui);
        });
    });
    ui.add_space(8.0);
}

/// The first cell of a grid row, as wide as a `row`'s label column so
/// grids line up with the rows around them.
fn grid_label(ui: &mut Ui, text: &str, card_w: f32) {
    let lw = (card_w * 0.34).clamp(150.0, 250.0) - 16.0;
    ui.horizontal(|ui| {
        ui.set_min_width(lw);
        ui.label(RichText::new(text).font(menu::label()).color(TEXT));
    });
}

/// A dim explanatory line across the card.
pub fn note(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(menu::caption()).color(DIM)).wrap());
}

/// Draws the settings screen for `game` (see the module docs); true when
/// its Back button was pressed. `in_flight`: over the paused game (dimmed)
/// rather than the start screen.
pub fn show(ctx: &egui::Context, game: &mut Game, in_flight: bool) -> bool {
    game.settings_screen(ctx, in_flight)
}

impl Game {
    pub(crate) fn settings_screen(&mut self, ctx: &egui::Context, in_flight: bool) -> bool {
        let screen = ctx.content_rect();
        let mut closed = false;
        egui::Area::new(egui::Id::unique("settings_screen")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            // The backdrop takes every click; the game (or globe) behind is all but hidden.
            ui.interact(screen, ui.scope_id().with("backdrop"), Sense::click());
            let p = ui.painter().clone();
            p.rect_filled(screen, 0.0, if in_flight { Color32::from_rgba_premultiplied(4, 6, 9, 226) } else { menu::BG });
            let side_w = if screen.width() < 1000.0 { 220.0 } else { 264.0 };
            let side = Rect::from_min_size(screen.min, vec2(side_w, screen.height()));
            p.rect_filled(side, 0.0, Color32::from_black_alpha(110));
            p.vline(side.right(), side.y_range(), Stroke::new(1.0, menu::LINE_HI));

            // The sidebar: title, the sections, Back at the foot.
            let mut s = ui.new_child(egui::UiBuilder::new().max_rect(side.shrink2(vec2(16.0, 28.0))).layout(Layout::top_down(Align::Min)));
            s.add_space(10.0);
            menu::screen_heading(&mut s, t!("Settings", "Réglages"));
            s.add_space(24.0);
            s.spacing_mut().item_spacing.y = 2.0;
            // the chosen tab's marker slides to it.
            let marker = s.painter().add(egui::Shape::Noop);
            let mut on_rect = None;
            for sec in Section::ALL {
                let r = self.settings_tab(&mut s, sec);
                if self.settings_sec == sec {
                    on_rect = Some(r.rect);
                }
                if r.clicked() && self.settings_sec != sec {
                    self.settings_sec = sec;
                }
            }
            if let Some(r) = on_rect {
                let y = ctx.animate_value_with_time(s.scope_id().with("tab_marker"), r.top(), 0.2);
                let m = Rect::from_min_size(pos2(r.left(), y), r.size());
                s.painter().set(marker, egui::Shape::rect_filled(m, menu::R_WIDGET, Color32::from_white_alpha(14)));
                let bar = Rect::from_center_size(pos2(m.left() + 1.5, m.center().y), vec2(3.0, m.height() * 0.5));
                s.painter().add(egui::Shape::rect_filled(bar, 1.0, ACCENT));
            }
            let foot = Rect::from_min_max(pos2(side.left() + 16.0, side.bottom() - 100.0), pos2(side.right() - 16.0, side.bottom() - 24.0));
            let mut f = ui.new_child(egui::UiBuilder::new().max_rect(foot).layout(Layout::bottom_up(Align::Min)));
            if padui::active() {
                padui::prompts(&mut f, &[("LB / RB", crate::tr("section")), ("B", crate::tr("back"))], 18.0, 12.0, DIM);
            } else {
                f.label(RichText::new(t!("Changes are saved as you make them.", "Les changements sont enregistrés aussitôt.")).font(menu::caption()).color(DIM));
            }
            f.add_space(6.0);
            let back = if in_flight { t!("Back to the pause menu", "Retour au menu pause") } else { t!("Back", "Retour") };
            if menu::wide_button(&mut f, back, Hint::of("Esc", "B"), false, true).clicked() {
                closed = true;
            }

            // The content: the section's title and its cards, in a centred column.
            let area = Rect::from_min_max(pos2(side.right() + 1.0, screen.top()), screen.max);
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(Layout::top_down(Align::Min)));
            let col_w = (area.width() - 96.0).clamp(320.0, MAX_W);
            let left = ((area.width() - col_w) * 0.5).max(24.0);
            egui::ScrollArea::vertical().id_salt(("settings_scroll", self.settings_sec.name())).auto_shrink([false, false]).show(&mut c, |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(left);
                    ui.vertical(|ui| {
                        ui.set_width(col_w);
                        ui.add_space(44.0);
                        let sec = self.settings_sec;
                        menu::page_heading(ui, sec.label(), sec.blurb());
                        ui.add_space(24.0);
                        ui.spacing_mut().slider_width = (col_w * 0.36).clamp(160.0, 300.0);
                        match sec {
                            Section::General => self.set_general(ui),
                            Section::Graphics => self.set_graphics(ui),
                            Section::Sound => self.set_sound(ui),
                            Section::Camera => self.set_camera(ui, in_flight),
                            Section::Controls => self.set_controls(ui),
                            Section::Map => self.set_map(ui),
                            Section::Multiplayer => self.set_multiplayer(ui, in_flight),
                        }
                        ui.add_space(40.0);
                    });
                });
                padui::follow_focus(ui);
            });
            ui.expand_to_include_rect(screen);
        });
        closed
    }

    /// One sidebar tab: its icon and name, the chosen one marked in the accent colour.
    fn settings_tab(&mut self, ui: &mut Ui, sec: Section) -> egui::Response {
        let on = self.settings_sec == sec;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, sec.label()));
        if on {
            self.pad_home_focus(&resp);
        }
        let lit = menu::ease(ui.ctx(), resp.id, (resp.hovered() || resp.has_focus()) && !on);
        let pick = ui.ctx().animate_bool_with_time(resp.id.with("on"), on, 0.2);
        let p = ui.painter();
        if lit > 0.0 {
            p.rect_filled(rect, menu::R_WIDGET, Color32::from_white_alpha((7.0 * lit) as u8));
        }
        let col = menu::mix(menu::mix(DIM, TEXT, lit), Color32::WHITE, pick);
        sec.icon(p, pos2(rect.left() + 26.0, rect.center().y), 18.0, menu::mix(col, ACCENT, pick));
        p.text(pos2(rect.left() + 50.0, rect.center().y), Align2::LEFT_CENTER, sec.label(), menu::label(), col);
        resp
    }

    fn set_general(&mut self, ui: &mut Ui) {
        card(ui, t!("Language", "Langue"), |ui| {
            row(ui, t!("Interface language", "Langue de l'interface"), "", |ui| self.language_picker(ui));
        });
        card(ui, t!("HUD and minimap", "HUD et mini-carte"), |ui| {
            row(ui, "HUD", t!("key H in flight", "touche H en vol"), |ui| {
                menu::switch(ui, &mut self.show_hud);
            });
            row(ui, t!("HUD in the cockpit", "HUD dans le cockpit"), t!("off: the cockpit's own instruments only; key H in the cockpit", "désactivé : seuls les instruments du cockpit ; touche H dans le cockpit"), |ui| {
                ui.add_enabled_ui(self.show_hud, |ui| menu::switch(ui, &mut self.hud_cockpit));
            });
            row(ui, t!("Minimap", "Mini-carte"), t!("key ` (left of 1) in flight", "touche ² (à gauche de 1) en vol"), |ui| {
                menu::switch(ui, &mut self.minimap.visible);
            });
            row(ui, t!("Minimap size", "Taille de la mini-carte"), t!("it fits the window and keeps this size", "elle s'adapte à la fenêtre et garde cette taille"), |ui| {
                if ui.add(egui::Slider::new(&mut self.minimap.size.x, 240.0..=640.0).step_by(10.0).suffix(" px")).changed() {
                    self.minimap.size.y = self.minimap.size.x;
                }
            });
            row(ui, t!("Minimap style", "Style de la mini-carte"), "", |ui| {
                menu::segments(ui, |ui| {
                    menu::seg(ui, &mut self.minimap.plan, false, t!("Satellite", "Satellite"));
                    menu::seg(ui, &mut self.minimap.plan, true, t!("Plan", "Plan"));
                });
            });
            row(ui, t!("Minimap orientation", "Orientation de la mini-carte"), "", |ui| {
                menu::segments(ui, |ui| {
                    menu::seg(ui, &mut self.minimap.heading_up, false, t!("North up", "Nord en haut"));
                    menu::seg(ui, &mut self.minimap.heading_up, true, t!("Heading up", "Cap en haut"));
                });
            });
            row(ui, t!("Landing guidance", "Aide à l’atterrissage"), t!("approach arrows and tunnel", "flèches et tunnel d’approche"), |ui| {
                menu::switch(ui, &mut self.minimap.landing_guidance);
            });
        });
        card(ui, t!("Mode", "Mode"), |ui| {
            row(ui, t!("Arcade mode", "Mode arcade"), t!("missiles, boost on any aircraft, respawn at will, weather and time keys", "missiles, boost sur tous les avions, réapparition à volonté, touches météo et heure"), |ui| {
                menu::switch(ui, &mut self.arcade);
            });
            row(ui, t!("Auto-trim", "Trim automatique"), t!("winds the trim for you while you fly the stick", "règle le trim pour vous pendant que vous pilotez"), |ui| {
                menu::switch(ui, &mut self.auto_trim);
            });
        });
        card(ui, t!("Air traffic", "Trafic aérien"), |ui| {
            row(ui, t!("Live air traffic", "Trafic aérien réel"), t!("real aircraft around you, from adsb.lol", "les vrais avions autour de vous, via adsb.lol"), |ui| {
                let mut on = self.live.enabled();
                if menu::switch(ui, &mut on).changed() {
                    self.live.set_enabled(on);
                }
            });
        });
        let t = if padui::active() { t!("In flight, View switches the camera; Y + View opens the flight panel.", "En vol, View change de caméra ; Y + View ouvre le panneau de vol.") } else { t!("Shortcuts in flight: U sound, C camera, H HUD, ` minimap, F2 anti-aliasing, Tab the flight panel.", "Raccourcis en vol : U son, C caméra, H HUD, ` mini-carte, F2 anticrénelage, Tab le panneau de vol.") };
        note(ui, t);
    }

    fn set_graphics(&mut self, ui: &mut Ui) {
        let before = self.gfx;
        let size = self.window.inner_size();
        card(ui, t!("Preset", "Préréglage"), |ui| self.gfx.preset_ui(ui));
        card(ui, t!("Quality", "Qualité"), |ui| self.gfx.quality_ui(ui));
        card(ui, t!("Display", "Affichage"), |ui| self.gfx.display_ui(ui, (size.width, size.height), self.fps));
        card(ui, t!("Scenery", "Paysage"), |ui| {
            use crate::snow::Mode;
            row(ui, t!("Snow on the ground", "Neige au sol"), t!("Automatic: from the date, the latitude and the real weather's temperature", "Automatique : selon la date, la latitude et la température de la météo réelle"), |ui| {
                menu::segments(ui, |ui| {
                    menu::seg(ui, &mut self.snow, Mode::Auto, t!("Automatic", "Automatique"));
                    menu::seg(ui, &mut self.snow, Mode::Off, t!("Off", "Désactivée"));
                    menu::seg(ui, &mut self.snow, Mode::Always, t!("Always", "Toujours"));
                });
            });
        });
        let on = if self.src.google.photoreal() { t!("on", "activées") } else { t!("off", "désactivées") };
        note(ui, &t!("Google's photorealistic 3D cities ({}) have their own switch and detail level under Map and data.", "Les villes 3D photoréalistes de Google ({}) ont leur propre réglage dans Carte et données.", on));
        if self.gfx != before {
            self.gfx.save();
        }
    }

    fn set_sound(&mut self, ui: &mut Ui) {
        card(ui, t!("Sound", "Son"), |ui| {
            row(ui, t!("Sound", "Son"), t!("key U in flight", "touche U en vol"), |ui| {
                menu::switch(ui, &mut self.sound_on);
            });
            row(ui, t!("Volume", "Volume"), "", |ui| {
                ui.add_enabled(self.sound_on, egui::Slider::new(&mut self.volume, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            });
        });
    }

    fn set_camera(&mut self, ui: &mut Ui, in_flight: bool) {
        card(ui, t!("Camera", "Caméra"), |ui| {
            let hint = if in_flight { t!("C switches it in flight", "C la change en vol") } else { t!("the view you start in; C switches it", "la vue au départ ; C la change") };
            row(ui, t!("Camera", "Caméra"), hint, |ui| {
                menu::segments(ui, |ui| {
                    menu::seg(ui, &mut self.cam_mode, 0, t!("Chase", "Extérieure"));
                    menu::seg(ui, &mut self.cam_mode, 1, "Cockpit");
                });
            });
            row(ui, t!("Cockpit field of view", "Champ de vision cockpit"), t!("the mouse wheel zooms for a moment", "la molette zoome un instant"), |ui| {
                ui.add(egui::Slider::new(&mut self.cockpit_fov, 35.0..=85.0).custom_formatter(|v, _| format!("{v:.0}° vertical")));
            });
        });
    }

    fn set_controls(&mut self, ui: &mut Ui) {
        self.binds_capture_banner(ui);
        card(ui, t!("Mouse", "Souris"), |ui| {
            row(ui, t!("Mouse look speed", "Vitesse du regard (souris)"), t!("dragging to look around", "glisser pour regarder autour"), |ui| {
                ui.add(egui::Slider::new(&mut self.look_sens, 0.25..=3.0).logarithmic(true).custom_formatter(|v, _| format!("{v:.2}×")));
            });
        });
        card(ui, t!("Connected devices", "Périphériques connectés"), |ui| self.binds_devices(ui));
        card(ui, t!("Gamepad", "Manette"), |ui| self.binds_pad(ui));
        card(ui, t!("Joystick axes", "Axes du joystick"), |ui| self.binds_axes(ui));
        card(ui, t!("Buttons (joystick, throttle, wheel)", "Boutons (joystick, manette des gaz, volant)"), |ui| self.binds_buttons(ui));
        card(ui, t!("Keyboard", "Clavier"), |ui| self.binds_keys(ui));
        if ui.add_enabled(self.binds.capture.is_none(), egui::Button::new(t!("Reset all bindings", "Tout réinitialiser"))).clicked() {
            self.binds = bind::Bindings::default();
            self.keys.clear();
        }
        ui.add_space(14.0);
        card(ui, t!("Key reference", "Aide-mémoire des touches"), |ui| {
            let t = if padui::active() { t!("In flight, Y + View shows the flight panel (instruments, places, weather) and Menu pauses.", "En vol, Y + View affiche le panneau de vol (instruments, destinations, météo) et Menu met en pause.") } else { t!("In flight, Tab shows the flight panel (instruments, places, weather) and Esc pauses.", "En vol, Tab affiche le panneau de vol (instruments, destinations, météo) et Échap met en pause.") };
            note(ui, t);
            ui.add_space(6.0);
            menu::controls_list(ui);
        });
    }

    fn set_map(&mut self, ui: &mut Ui) {
        let g = self.src.google.clone();
        let has_key = !self.gkey.trim().is_empty();
        let need_key = t!("Enter a Google Maps Platform API key first", "Saisissez d'abord une clé API Google Maps Platform");
        card(ui, t!("Map imagery", "Imagerie cartographique"), |ui| {
            row(ui, t!("Google API key", "Clé API Google"), t!("Map Tiles API, billed to you", "Map Tiles API, facturée à vous"), |ui| {
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.gkey).password(!self.gkey_show).hint_text(t!("Google Maps Platform key", "Clé Google Maps Platform")).desired_width((ui.available_width() - 60.0).min(380.0)));
                    if r.lost_focus() || (r.changed() && self.gkey.trim().is_empty()) {
                        g.set_key(&self.gkey);
                    }
                    if ui.button(if self.gkey_show { t!("Hide", "Masquer") } else { t!("Show", "Afficher") }).clicked() {
                        self.gkey_show = !self.gkey_show;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.hyperlink_to(RichText::new(t!("How to get a key", "Obtenir une clé")).font(menu::caption()), "https://developers.google.com/maps/documentation/tile/get-api-key");
                    ui.label(RichText::new("·").font(menu::caption()).color(DIM));
                    ui.hyperlink_to(RichText::new(t!("Cloud console: credentials", "Console Cloud : identifiants")).font(menu::caption()), "https://console.cloud.google.com/google/maps-apis/credentials");
                });
            });
            row(ui, t!("Satellite imagery", "Imagerie satellite"), if has_key { t!("Google Maps", "Google Maps") } else { need_key }, |ui| {
                let mut on = g.enabled();
                if ui.add_enabled_ui(has_key, |ui| menu::switch(ui, &mut on)).inner.changed() {
                    g.set_key(&self.gkey);
                    g.set_enabled(on);
                }
                note(ui, if on { t!("Terrain and roofs take Google's tiles (Map Tiles API); what has loaded already is redone. Tiles not yet on disk are billed to your key.", "Le terrain et les toits utilisent les tuiles de Google (Map Tiles API) ; ce qui est déjà chargé est refait. Les tuiles absentes du disque sont facturées sur votre clé.") } else { t!("Off: the free default imagery. On: Google's Map Tiles API with your own key, billed per tile request.", "Désactivé : l'imagerie gratuite par défaut. Activé : la Map Tiles API de Google avec votre propre clé, facturée à la tuile.") });
            });
            let status = g.status.lock().clone();
            if !status.is_empty() {
                ui.label(RichText::new(status.chars().take(160).collect::<String>()).font(menu::caption()).color(Color32::from_rgb(255, 150, 70)));
            }
        });
        card(ui, t!("Photorealistic 3D cities", "Villes 3D photoréalistes"), |ui| {
            let mut photo = g.photoreal();
            row(ui, t!("Google 3D tiles", "Tuiles 3D Google"), if has_key { t!("beta", "bêta") } else { need_key }, |ui| {
                if ui.add_enabled_ui(has_key, |ui| menu::switch(ui, &mut photo)).inner.changed() {
                    g.set_key(&self.gkey);
                    g.set_photoreal(photo);
                }
            });
            let detail = g.detail();
            let zone_km = tiles3d::Params::of(detail).zone_r / 1000.0;
            row(ui, t!("3D detail", "Détail 3D"), "", |ui| {
                ui.add_enabled_ui(photo, |ui| {
                    let mut d = detail;
                    menu::segments(ui, |ui| {
                        for c in gmaps::Detail::ALL {
                            let name = match c {
                                gmaps::Detail::Low => t!("Low", "Faible"),
                                gmaps::Detail::Medium => t!("Medium", "Moyen"),
                                gmaps::Detail::High => t!("High", "Élevé"),
                                gmaps::Detail::Ultra => "Ultra",
                            };
                            menu::seg(ui, &mut d, c, name);
                        }
                    });
                    if d != detail {
                        g.set_detail(d);
                    }
                    note(ui, &t!("{} One billed session per flight at every level; the first minute over a city takes up to a third more.", "{} Une session facturée par vol à chaque niveau ; la première minute au-dessus d'une ville consomme jusqu'à un tiers de plus.", crate::detail_hint(detail)));
                });
            });
            note(ui, &if photo { t!("Google's photogrammetry meshes replace the ground, buildings and trees within about {zone_km:.0} km of the aircraft; at night they light up with our street lamps and windows. Each flight opens one billed 3D session (a root request, good for about 3 hours); the tiles under it are kept on disk only for the 4 hours Google allows.", "Les maillages photogrammétriques de Google remplacent le sol, les bâtiments et les arbres dans un rayon d'environ {zone_km:.0} km autour de l'avion ; la nuit, nos lampadaires et fenêtres les éclairent. Chaque vol ouvre une session 3D facturée (une requête racine, valable environ 3 heures) ; les tuiles sont gardées sur le disque seulement pendant les 4 heures autorisées par Google.", zone_km = zone_km) } else { t!("Off: our own 3D world everywhere. On: Google's Photorealistic 3D Tiles near the aircraft, one billed session per flight.", "Désactivé : notre propre monde 3D partout. Activé : les Photorealistic 3D Tiles de Google près de l'avion, une session facturée par vol.").to_string() });
        });
        card(ui, t!("Google usage", "Utilisation de Google"), |ui| {
            let u = g.usage.lock().clone();
            let req = |n: u64| i18n::plural(n, "request", "requests", "requête", "requêtes");
            let line = |ui: &mut Ui, k: &str, v: String| row(ui, k, "", |ui| {
                ui.label(RichText::new(v).font(menu::caption()).color(TEXT));
            });
            line(ui, t!("This session", "Cette session"), format!("{} · {}", req(u.session_requests as u64), crate::bytes(u.session_bytes)));
            line(ui, t!("   imagery", "   imagerie"), t!("{} tiles downloaded · {} from disk", "{} tuiles téléchargées · {} depuis le disque", u.session_tiles, u.session_cached));
            if u.session_roots > 0 || u.total_roots > 0 {
                let ses = i18n::plural(u.session_roots as u64, "session", "sessions", "session", "sessions");
                line(ui, "   3D", t!("{} (billed) · {} tiles · {} from disk", "{} (facturées) · {} tuiles · {} depuis le disque", ses, u.session_3d, u.session_3d_cached));
            }
            if u.session_errors > 0 {
                line(ui, t!("   failed", "   échecs"), format!("{}", u.session_errors));
            }
            line(ui, &t!("Month {}", "Mois {}", u.month), format!("{} · {}", req(u.month_requests as u64), crate::bytes(u.month_bytes)));
            line(ui, t!("All time", "Depuis le début"), format!("{} · {}", req(u.total_requests as u64), crate::bytes(u.total_bytes)));
            if u.total_roots > 0 {
                line(ui, t!("3D sessions", "Sessions 3D"), t!("{} this month · {} all time", "{} ce mois-ci · {} au total", u.month_roots, u.total_roots));
            }
            note(ui, t!("Counts only calls that reach Google; tiles already on disk are free. Check the Cloud console for billing.", "Ne compte que les appels qui atteignent Google ; les tuiles déjà sur le disque sont gratuites. Consultez la console Cloud pour la facturation."));
        });
        card(ui, t!("Real-world data", "Données réelles"), |ui| {
            row(ui, t!("Real weather", "Météo réelle"), t!("METAR, else Open-Meteo; radar", "METAR, sinon Open-Meteo ; radar"), |ui| {
                menu::switch(ui, &mut self.wx.live);
            });
            row(ui, t!("Real time", "Heure réelle"), t!("the sun where you are, now; off once a time is set by hand", "le soleil là où vous êtes, maintenant ; coupée dès qu'on règle l'heure"), |ui| {
                menu::switch(ui, &mut self.wx.real_time);
            });
            row(ui, t!("Live air traffic", "Trafic aérien réel"), t!("real aircraft around you, from adsb.lol", "les vrais avions autour de vous, via adsb.lol"), |ui| {
                let mut on = self.live.enabled();
                if menu::switch(ui, &mut on).changed() {
                    self.live.set_enabled(on);
                }
            });
        });
    }

    // ---- multiplayer (mp.rs)

    /// Opens the settings screen at `sec`; its Back button (and Esc) return to `back`.
    pub(crate) fn open_settings(&mut self, sec: Section, back: crate::menu::Screen) {
        self.settings_sec = sec;
        self.menu.set_back = back;
        self.menu.screen = crate::menu::Screen::Settings;
        self.pad_home = true;
    }

    /// Where multiplayer stands in a few words, and its colour (the
    /// settings, the main menu's Multiplayer bar and the globe use it).
    pub(crate) fn mp_line(&self) -> (Color32, String) {
        if !self.mp_on {
            return (DIM, t!("Not connected", "Non connecté").to_string());
        }
        let server = self.mp.as_ref().map(|m| self.mp_server_name(&m.server)).unwrap_or_default();
        match self.mp.as_ref().map(|m| m.status()) {
            None => (ACCENT, t!("Looking for the fastest server…", "Recherche du serveur le plus rapide…").to_string()),
            Some(mp::Status::Connecting) => (ACCENT, t!("Connecting to {}…", "Connexion à {}…", server)),
            Some(mp::Status::Error(e)) => (menu::BAD, t!("No connection: {}", "Pas de connexion : {}", e.chars().take(60).collect::<String>())),
            Some(mp::Status::Connected) => (menu::OK, t!("Connected to {}", "Connecté à {}", server)),
        }
    }

    /// The same in a word or two, for the main menu's Multiplayer bar.
    pub(crate) fn mp_short(&self) -> (Color32, String) {
        let (col, _) = self.mp_line();
        let text = if let Some(n) = self.mp_flying() {
            n
        } else {
            match (self.mp_on, self.mp.as_ref().map(|m| m.status())) {
                (false, _) => t!("Off", "Désactivé").to_string(),
                (true, Some(mp::Status::Connected)) => t!("Connected", "Connecté").to_string(),
                (true, Some(mp::Status::Error(_))) => t!("No connection", "Pas de connexion").to_string(),
                (true, _) => t!("Connecting…", "Connexion…").to_string(),
            }
        };
        (col, text)
    }

    /// How many fly, as the menus show it ("12 pilots flying"), once the relay has said.
    pub(crate) fn mp_flying(&self) -> Option<String> {
        let connected = self.mp.as_ref().is_some_and(|m| m.status() == mp::Status::Connected);
        (connected && self.mp_world_known).then(|| i18n::plural(self.mp_world_total as u64, "pilot flying", "pilots flying", "pilote en vol", "pilotes en vol"))
    }

    fn set_multiplayer(&mut self, ui: &mut Ui, in_flight: bool) {
        card(ui, t!("Connection", "Connexion"), |ui| {
            // Where it stands, and the one button that changes it.
            let (col, line) = self.mp_line();
            let sub = if !self.mp_on {
                t!("Connect to see the other players, on the globe and in the air, and be seen by them.", "Connectez-vous pour voir les autres joueurs, sur le globe et en vol, et être vu d'eux.").to_string()
            } else if in_flight {
                i18n::plural(self.mp_players.len() as u64, "other pilot nearby", "other pilots nearby", "autre pilote à proximité", "autres pilotes à proximité")
            } else if let Some(n) = self.mp_flying() {
                t!("{} on the network: they are on the globe.", "{} sur le réseau : ils sont sur le globe.", n)
            } else if self.mp.as_ref().is_some_and(|m| m.status() == mp::Status::Connected) {
                t!("This server does not send the world map (an older relay): you will see the others once in the air.", "Ce serveur n'envoie pas la carte du monde (un ancien relais) : vous verrez les autres une fois en vol.").to_string()
            } else {
                String::new()
            };
            ui.horizontal(|ui| {
                let bw = 190.0;
                ui.allocate_ui_with_layout(vec2((ui.available_width() - bw - 16.0).max(120.0), 46.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.5, col);
                        ui.add(egui::Label::new(RichText::new(line).font(menu::label()).color(TEXT)).truncate());
                    });
                    if !sub.is_empty() {
                        ui.add(egui::Label::new(RichText::new(sub).font(menu::caption()).color(DIM)).wrap());
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.allocate_ui(vec2(bw, 46.0), |ui| {
                        let (text, primary) = if self.mp_on { (t!("Disconnect", "Se déconnecter"), false) } else { (t!("Connect", "Se connecter"), true) };
                        if menu::wide_button(ui, text, Hint::None, primary, true).clicked() {
                            self.mp_on = !self.mp_on;
                        }
                    });
                });
            });
            ui.add_space(10.0);
            row(ui, t!("Your name", "Votre nom"), t!("shown to the others; left empty, the server gives one", "montré aux autres ; laissé vide, le serveur en donne un"), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.mp_name).desired_width(240.0).char_limit(mp_proto::NAME_MAX).hint_text(t!("Your name", "Votre nom")));
            });
            row(ui, t!("Server", "Serveur"), t!("the servers form one network: everyone sees everyone", "les serveurs forment un seul réseau : tout le monde voit tout le monde"), |ui| self.mp_server_picker(ui));
        });
        if !in_flight && self.mp_on && !self.mp_world.is_empty() {
            card(ui, t!("Flying now", "En vol"), |ui| self.mp_pilots(ui));
        }
        if in_flight && !self.mp_players.is_empty() {
            card(ui, t!("Nearby", "À proximité"), |ui| {
                let me = self.plane.pos;
                for p in self.mp_players.iter().take(12) {
                    let ac = mp_proto::AIRCRAFT.get(p.aircraft as usize).copied().unwrap_or("?");
                    let alt = geo::pos_to_lla(p.pos).2 * 3.28084;
                    row(ui, &p.name, ac, |ui| {
                        ui.label(RichText::new(format!("{:.1} km  ·  {:.0} ft  ·  {:.0} kt", (p.pos - me).length() / 1000.0, alt, p.vel.length() * 1.94384)).font(menu::data()).color(TEXT));
                    });
                }
            });
        }
        note(ui, t!("Only while you are connected: in flight your aircraft (where it is, how it flies) and your name go to the server, which passes them on to the players within 150 km. On the start screen nothing of your aircraft is sent; it shows where the flying players are, to within about a kilometre.", "Seulement quand vous êtes connecté : en vol, votre avion (sa position, son vol) et votre nom vont au serveur, qui les transmet aux joueurs à moins de 150 km. Sur l'écran de départ, rien de votre avion n'est envoyé ; il montre où sont les joueurs en vol, à un kilomètre près environ."));
    }

    /// The pilots flying on the network (start screen): name, aircraft,
    /// where and how high; Show turns the globe to them.
    fn mp_pilots(&mut self, ui: &mut Ui) {
        let mut look: Option<(f64, f64)> = None;
        let shown = 16;
        for s in self.mp_world.iter().take(shown) {
            let ac = mp_proto::AIRCRAFT.get(s.aircraft as usize).copied().unwrap_or("?");
            let (lat, lon) = (s.lat_deg(), s.lon_deg());
            let place = match crate::menu::Menu::near_city(lat, lon) {
                Some((c, d)) if d < 120_000.0 => t!("near {}", "près de {}", crate::tr(c)),
                // (to a tenth of a degree: the world map is no finer than that)
                _ => format!("{:.1}°{}  {:.1}°{}", lat.abs(), if lat >= 0.0 { "N" } else { "S" }, lon.abs(), if lon >= 0.0 { "E" } else { "W" }),
            };
            let how = if s.flags & mp_proto::F_GROUND != 0 { t!("on the ground", "au sol").to_string() } else { format!("{:.0} ft  ·  {} kt", s.alt_m() * 3.28084, s.speed) };
            ui.horizontal(|ui| {
                let w = ui.available_width();
                ui.allocate_ui_with_layout(vec2((w - 96.0).max(100.0), 40.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(egui::Label::new(RichText::new(format!("{}  ·  {ac}", s.name)).font(menu::label()).color(TEXT)).truncate());
                    ui.add(egui::Label::new(RichText::new(format!("{place}  ·  {how}")).font(menu::caption()).color(DIM)).truncate());
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(t!("Show", "Voir")).on_hover_text(t!("Turn the globe to them", "Tourner le globe vers eux")).clicked() {
                        look = Some((lat, lon));
                    }
                });
            });
            ui.add_space(4.0);
        }
        let more = (self.mp_world_total as usize).saturating_sub(self.mp_world.len().min(shown));
        if more > 0 {
            note(ui, &i18n::plural(more as u64, "more", "more", "autre", "autres"));
        }
        if let Some((lat, lon)) = look {
            self.menu.look_at(lat, lon, 400_000.0);
            self.menu.screen = crate::menu::Screen::Fly;
            self.pad_home = true;
        }
    }

    /// The server: the network's servers with their pilots and ping,
    /// "automatic" (the fastest), or an address typed in.
    fn mp_server_picker(&mut self, ui: &mut Ui) {
        let dir = self.mp_directory();
        let (servers, err, stale) = (dir.servers(), dir.error(), dir.stale());
        let best = dir.best();
        let auto = self.mp_auto();
        let current = mp::with_port(&self.mp_server);
        let listed = servers.iter().any(|s| s.address == current);
        let label = |s: &mp::ServerEntry| {
            let state = match (s.up, s.ping_ms) {
                (false, _) => t!("offline", "hors ligne").to_string(),
                (true, Some(ms)) => format!("{ms} ms"),
                (true, None) => t!("no answer", "pas de réponse").to_string(),
            };
            format!("{}  ·  {}  ·  {}", s.name, i18n::plural(s.flying as u64, "pilot", "pilots", "pilote", "pilotes"), state)
        };
        let selected = if auto {
            match &best {
                Some(b) => t!("Automatic ({})", "Automatique ({})", b.name),
                None => t!("Automatic: the fastest", "Automatique : le plus rapide").to_string(),
            }
        } else if let Some(s) = servers.iter().find(|s| s.address == current) {
            label(s)
        } else {
            t!("Other: {}", "Autre : {}", self.mp_server.trim())
        };
        let mut pick: Option<String> = None;
        egui::ComboBox::from_id_salt("mp_server").selected_text(selected).width(ui.available_width().min(340.0)).show_ui(ui, |ui| {
            if ui.selectable_label(auto, t!("Automatic: the fastest", "Automatique : le plus rapide")).clicked() {
                pick = Some("auto".into());
            }
            for s in &servers {
                ui.add_enabled_ui(s.up, |ui| {
                    if ui.selectable_label(!auto && s.address == current, label(s)).clicked() {
                        pick = Some(s.address.clone());
                    }
                });
            }
            if ui.selectable_label(!auto && !listed, t!("Other address…", "Autre adresse…")).clicked() {
                pick = Some(String::new());
            }
        });
        match pick.as_deref() {
            Some("") => {
                self.mp_custom = true;
                if auto || listed {
                    self.mp_server.clear();
                }
            }
            Some(a) => {
                self.mp_custom = false;
                self.mp_server = a.to_string();
            }
            None => {}
        }
        if self.mp_custom || (!auto && !listed) {
            ui.add(egui::TextEdit::singleline(&mut self.mp_server).desired_width(ui.available_width().min(340.0)).char_limit(100).hint_text("host[:port]"));
        }
        ui.horizontal(|ui| {
            let text = match (&err, servers.is_empty()) {
                (Some(_), false) if stale => t!("Last known list (the list server is not answering)", "Dernière liste connue (le serveur de liste ne répond pas)").to_string(),
                (Some(e), _) => t!("Server list unavailable: {}", "Liste des serveurs indisponible : {}", e.chars().take(50).collect::<String>()),
                (None, true) => t!("Reading the server list…", "Lecture de la liste des serveurs…").to_string(),
                (None, false) => i18n::plural(servers.iter().filter(|s| s.up).count() as u64, "server online", "servers online", "serveur en ligne", "serveurs en ligne"),
            };
            ui.label(RichText::new(text).font(menu::caption()).color(DIM));
            if ui.small_button(t!("Refresh", "Actualiser")).clicked() {
                self.mp_directory().refresh();
            }
        });
    }

    // ---- joysticks, pedals and keys (bind.rs)

    /// While waiting for an axis, button or key to bind: what to do, and Cancel.
    fn binds_capture_banner(&mut self, ui: &mut Ui) {
        use bind::{Capture, ACTIONS};
        let Some(c) = self.binds.capture.clone() else { return };
        let label = |id: &str| ACTIONS.iter().find(|a| a.id == id).map(|a| a.label()).unwrap_or("?");
        let what = match &c {
            Capture::Axis(a) => a.how().to_string(),
            Capture::Button(id) => t!("Press the button for: {}", "Appuyez sur le bouton pour : {}", label(id)),
            Capture::Key(id) => t!("Press the key for: {}  (Esc: cancel)", "Appuyez sur la touche pour : {}  (Échap : annuler)", label(id)),
            Capture::Pad(id) => t!("Press a button for: {}. Or hold Y and press one.", "Appuyez sur un bouton pour : {}. Ou maintenez Y et appuyez sur un bouton.", padmap::label(id)),
            Capture::PadAxis(a) => format!("{}: {}", a.label(), a.how()),
        };
        // From the pad: B held calls it off (a tap of B binds B), and how far through that is.
        let from_pad = c.from_pad();
        let cancelling = self.binds.pad_catch.cancelling(self.real);
        egui::Frame::new().fill(ACCENT.gamma_multiply(0.22)).corner_radius(menu::R_PANEL).inner_margin(10.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(what).font(menu::body()).color(Color32::WHITE));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(t!("Cancel", "Annuler")).clicked() {
                        self.binds.capture = None;
                    }
                    if from_pad {
                        ui.label(RichText::new(t!("hold to cancel", "maintenir pour annuler")).font(menu::caption()).color(Color32::WHITE));
                        padui::glyphs(ui, "B", 20.0);
                    }
                });
            });
            if cancelling > 0.0 {
                ui.add(egui::ProgressBar::new(cancelling).desired_height(4.0));
            }
        });
        ui.add_space(12.0);
    }

    fn binds_devices(&mut self, ui: &mut Ui) {
        let devices: Vec<(bind::Uuid, String, bool)> = self.gilrs.as_ref().map(|g| g.gamepads().map(|(_, p)| (p.uuid(), p.name().to_string(), self.binds.uses(&p.uuid()))).collect()).unwrap_or_default();
        if devices.is_empty() {
            note(ui, t!("No joystick, pedals or gamepad found.", "Aucun joystick, palonnier ni manette détecté."));
        }
        for (k, (uuid, name, bound)) in devices.into_iter().enumerate() {
            let role = if bound { t!("bound", "assigné") } else { t!("gamepad layout", "disposition manette") };
            row(ui, &name, role, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("preset", k)).selected_text(t!("Apply a preset…", "Appliquer un préréglage…")).width(260.0).show_ui(ui, |ui| {
                        for p in bind::PRESETS {
                            if ui.selectable_label(false, p.label()).clicked() {
                                self.binds.apply_preset(p, uuid, &name, true);
                            }
                        }
                    });
                    if bound && ui.button(t!("Remove its bindings", "Retirer ses assignations")).clicked() {
                        self.binds.clear_device(&uuid);
                    }
                });
            });
        }
        note(ui, t!("A gamepad has its own layout (Gamepad, below). A joystick, pedals or a throttle take the axes and buttons bound to them; a pad given any of those no longer works as a gamepad.", "Une manette a sa propre disposition (Manette, ci-dessous). Joystick, palonnier ou manette des gaz prennent les axes et boutons qu'on leur assigne ; une manette à qui l'on en assigne ne sert plus de manette de jeu."));
    }

    /// The gamepad's flying layout (padmap.rs): the sticks and triggers,
    /// then every action, each bound from the pad itself.
    fn binds_pad(&mut self, ui: &mut Ui) {
        use bind::Capture;
        use padmap::{AxisSrc, PadAxis};
        let busy = self.binds.capture.is_some();
        note(ui, t!("In the menus the pad stays as it is: the d-pad moves, A presses, B goes back, LB / RB turn the page, the right stick scrolls. Menu pauses. The rest is yours: pick Change, then press a button, or hold Y and press one.", "Dans les menus, la manette ne change pas : la croix déplace, A appuie, B revient, LB / RB tournent la page, le stick droit fait défiler. Menu met en pause. Le reste est à vous : choisissez Changer, puis appuyez sur un bouton, ou maintenez Y et appuyez sur un bouton."));
        if self.pad_note.1 > self.real {
            ui.add_space(4.0);
            ui.label(RichText::new(&self.pad_note.0).font(menu::caption()).color(ACCENT));
        }
        ui.add_space(8.0);
        let cw = ui.available_width();
        let mut lost = Vec::new();
        bind_grid(ui, "pad_binds", |ui| {
            for a in PadAxis::ALL {
                let src = self.binds.pad.axis(a);
                grid_label(ui, a.label(), cw);
                pad_cell(ui, src.map(|s| s.stick.spec()), self.binds.pad.axis_moved(a));
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new(t!("Change", "Changer"))).clicked() {
                        self.binds.start_capture(Capture::PadAxis(a));
                    }
                    if src.is_some() && ui.add_enabled(!busy, egui::Button::new(t!("Clear", "Effacer"))).clicked() {
                        lost = self.binds.pad.set_axis(a, None);
                    }
                    if self.binds.pad.axis_moved(a) && ui.add_enabled(!busy, egui::Button::new(t!("Default", "Par défaut"))).clicked() {
                        lost = self.binds.pad.reset_axis(a);
                    }
                    if let Some(s) = src {
                        let mut inv = s.inverted(a);
                        if menu::toggle(ui, &mut inv, t!("Invert", "Inverser")).changed() {
                            self.binds.pad.set_axis(a, Some(AxisSrc { sign: -s.sign, ..s }));
                        }
                    }
                });
                ui.end_row();
            }
            // (one grid for both, so the columns line up)
            for _ in 0..3 {
                ui.add_space(4.0);
            }
            ui.end_row();
            for &(id, _) in padmap::actions() {
                let moved = self.binds.pad.is_moved(id);
                let chord = self.binds.pad.chord(id);
                grid_label(ui, padmap::label(id), cw);
                pad_cell(ui, chord.map(|c| if c == padmap::Y_ALONE { t!("Y (let go)", "Y (relâché)").to_string() } else { c.spec() }), moved);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new(t!("Change", "Changer"))).clicked() {
                        self.binds.start_capture(Capture::Pad(id));
                    }
                    if chord.is_some() && ui.add_enabled(!busy, egui::Button::new(t!("Clear", "Effacer"))).clicked() {
                        self.binds.pad.clear(id);
                    }
                    if moved && ui.add_enabled(!busy, egui::Button::new(t!("Default", "Par défaut"))).clicked() {
                        lost = self.binds.pad.reset(id);
                    }
                });
                ui.end_row();
            }
        });
        if !lost.is_empty() {
            self.pad_note = (t!("No longer bound: {}", "Plus assigné : {}", lost.join(", ")), self.real + 8.0);
        }
        ui.add_space(8.0);
        if ui.add_enabled(!busy && !self.binds.pad.is_default(), egui::Button::new(t!("Back to the Flight Simulator layout", "Revenir à la disposition de Flight Simulator"))).clicked() {
            self.binds.pad = padmap::PadMap::default();
            self.pad_note = (String::new(), 0.0);
        }
    }

    fn binds_axes(&mut self, ui: &mut Ui) {
        use bind::{AxisAct, Capture};
        let busy = self.binds.capture.is_some();
        note(ui, t!("Binding · then its live value and dead zone (travel that does nothing)", "Assignation · puis sa valeur en direct et sa zone morte (course sans effet)"));
        ui.add_space(6.0);
        let cw = ui.available_width();
        bind_grid(ui, "bind_axes", |ui| {
            for a in AxisAct::ALL {
                grid_label(ui, a.label(), cw);
                binding_cell(ui, self.binds.describe_axis(a), false);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new(t!("Assign", "Assigner"))).clicked() {
                        self.binds.start_capture(Capture::Axis(a));
                    }
                    if self.binds.axes.contains_key(&a) && ui.button(t!("Clear", "Effacer")).clicked() {
                        self.binds.axes.remove(&a);
                    }
                });
                ui.horizontal(|ui| {
                    // 0 until the axis moves.
                    if let Some(v) = self.binds.axis(a).or_else(|| self.binds.axes.contains_key(&a).then_some(0.0)) {
                        let f = if a.centred() { (v as f32 + 1.0) * 0.5 } else { v as f32 };
                        ui.add(egui::ProgressBar::new(f).desired_width(64.0).desired_height(6.0));
                    }
                    // the dead zone.
                    if let Some(b) = self.binds.axes.get_mut(&a) {
                        ui.spacing_mut().slider_width = 70.0;
                        ui.add(egui::Slider::new(&mut b.dead, 0.0..=0.3).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)))
                            .on_hover_text(t!("Dead zone: how much travel does nothing", "Zone morte : la course qui ne fait rien"));
                        // The sign is learnt when binding; this flips it.
                        let mut inv = b.sign < 0.0;
                        if menu::toggle(ui, &mut inv, t!("Invert", "Inverser")).changed() {
                            b.sign = -b.sign;
                        }
                    }
                });
                ui.end_row();
            }
        });
    }

    fn binds_buttons(&mut self, ui: &mut Ui) {
        use bind::{Capture, ACTIONS};
        let busy = self.binds.capture.is_some();
        let cw = ui.available_width();
        bind_grid(ui, "bind_buttons", |ui| {
            for a in ACTIONS {
                grid_label(ui, a.label(), cw);
                binding_cell(ui, self.binds.describe_button(a.id), false);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new(t!("Assign", "Assigner"))).clicked() {
                        self.binds.start_capture(Capture::Button(a.id));
                    }
                    if self.binds.buttons.contains_key(a.id) && ui.button(t!("Clear", "Effacer")).clicked() {
                        self.binds.buttons.remove(a.id);
                    }
                });
                ui.label("");
                ui.end_row();
            }
        });
    }

    fn binds_keys(&mut self, ui: &mut Ui) {
        use bind::{Capture, ACTIONS};
        let busy = self.binds.capture.is_some();
        let cw = ui.available_width();
        bind_grid(ui, "bind_keys", |ui| {
            for a in ACTIONS.iter().filter(|a| a.keyboard) {
                grid_label(ui, a.label(), cw);
                let moved = self.binds.keys.contains_key(a.id);
                binding_cell(ui, self.binds.key_of(a).map(bind::key_name), moved);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new(t!("Change", "Changer"))).clicked() {
                        self.binds.start_capture(Capture::Key(a.id));
                        self.keys.clear();
                    }
                    if moved && ui.button(t!("Default", "Par défaut")).clicked() {
                        self.binds.reset_key(a.id);
                        self.keys.clear();
                    }
                });
                ui.label("");
                ui.end_row();
            }
        });
    }
}

/// the grid every bindings table shares, so their columns line up.
fn bind_grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id).num_columns(4).spacing([16.0, 6.0]).min_row_height(30.0).show(ui, body);
}

/// what an action is bound to, as a key cap.
fn binding_cell(ui: &mut Ui, what: Option<String>, moved: bool) {
    ui.allocate_ui_with_layout(vec2(180.0, 26.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(180.0);
        match what {
            Some(w) => {
                let r = menu::key_cap(ui, &w);
                if moved {
                    ui.painter().rect_stroke(r.rect, menu::R_WIDGET, Stroke::new(1.0, ACCENT), egui::StrokeKind::Outside);
                }
            }
            None => {
                ui.label(RichText::new(t!("Not bound", "Non assigné")).font(menu::caption()).color(menu::FAINT));
            }
        }
    });
}

/// What a pad action is bound to, drawn as the pad's buttons, in the same
/// column as `binding_cell`. `moved`: changed from the default.
fn pad_cell(ui: &mut Ui, spec: Option<String>, moved: bool) {
    ui.allocate_ui_with_layout(vec2(180.0, 26.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(180.0);
        match spec {
            Some(s) => {
                let r = padui::glyphs(ui, &s, 22.0);
                if moved {
                    ui.painter().rect_stroke(r.rect.expand(3.0), menu::R_WIDGET, Stroke::new(1.0, ACCENT), egui::StrokeKind::Outside);
                }
            }
            None => {
                ui.label(RichText::new(t!("Not bound", "Non assigné")).font(menu::caption()).color(menu::FAINT));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_parse_and_cycle() {
        for s in Section::ALL {
            assert_eq!(Section::parse(s.name()), Some(s));
            assert_eq!(s.cycle(true).cycle(false), s);
        }
        assert_eq!(Section::parse("bindings"), Some(Section::Controls));
        assert_eq!(Section::parse("Graphics"), Some(Section::Graphics));
        assert_eq!(Section::Map.cycle(true), Section::Multiplayer);
        assert_eq!(Section::Multiplayer.cycle(true), Section::General);
        assert_eq!(Section::parse("mp"), Some(Section::Multiplayer));
        assert_eq!(Section::parse("nope"), None);
        // `--pause-menu controls` is the key list, not the settings section.
        assert_eq!(crate::menu::Page::parse("controls"), Some(crate::menu::Page::Controls));
        assert_eq!(crate::menu::Page::parse("graphics"), None);
    }
}
