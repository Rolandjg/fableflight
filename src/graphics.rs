//! Graphics settings: how hard the renderer works, as presets (Low, Medium,
//! High, Ultra) or tuned one by one. High is what the game always drew before
//! these settings existed; Ultra goes past it (supersampling, a finer shadow
//! map, finer cloud steps, terrain and scenery further out). Saved in
//! `<config>/fablegame/graphics.json`.
//!
//! Google's photorealistic 3D tiles are not part of this: they are switched on
//! and given their own detail level in the map imagery settings (gmaps.rs).
use crate::menu::{self, DIM};
use crate::settings_ui::{note, row};
use egui::RichText;
use std::path::PathBuf;

/// A quality step. `Off` only applies to the shadows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Off,
    Low,
    Medium,
    High,
    Ultra,
}

impl Level {
    pub const PRESETS: [Level; 4] = [Level::Low, Level::Medium, Level::High, Level::Ultra];

    pub fn name(self) -> &'static str {
        match self {
            Level::Off => "Off",
            Level::Low => "Low",
            Level::Medium => "Medium",
            Level::High => "High",
            Level::Ultra => "Ultra",
        }
    }

    pub fn parse(s: &str) -> Option<Level> {
        [Level::Off, Level::Low, Level::Medium, Level::High, Level::Ultra].into_iter().find(|l| l.name().eq_ignore_ascii_case(s.trim()))
    }

    fn pick(self, low: f64, medium: f64, high: f64, ultra: f64) -> f64 {
        match self {
            Level::Off | Level::Low => low,
            Level::Medium => medium,
            Level::High => high,
            Level::Ultra => ultra,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Graphics {
    /// render resolution as a share of the window's (each side): below 1 is
    /// upscaled, above 1 supersampled
    pub scale: f32,
    pub shadows: Level,
    pub clouds: Level,
    /// storm cell rendering detail (separate from ordinary clouds so heavy
    /// thunderstorm, supercell and squall-line skies can be tuned on their own)
    pub storms: Level,
    pub terrain: Level,
    /// how far out buildings and trees are loaded
    pub scenery: Level,
    /// how far out buildings keep their full geometry, dressing and window depth
    pub city: Level,
    /// how far out trees are drawn, and how near they turn into impostors
    pub trees: Level,
    /// moving and parked cars (Off, or Low to High; Ultra is High)
    pub cars: Level,
    /// birds in the air (birds.rs; Off, or Low to High; Ultra is High)
    pub birds: Level,
    /// ambient occlusion (GTAO)
    pub ao: bool,
    // Not part of the presets: preferences more than costs.
    /// temporal anti-aliasing (F2)
    pub taa: bool,
    pub vsync: bool,
    /// Optional frame limit (0 means no software limit).
    pub fps_cap: u16,
    /// lower the render resolution while the gpu cant hold 60 fps (main.rs dyn_res)
    pub dynamic_res: bool,
}

/// lowest share of the render resolution dynamic resolution goes to.
pub const DYN_MIN: f32 = 0.6;

impl Default for Graphics {
    fn default() -> Self {
        Self::preset(Level::High)
    }
}

pub const SCALE_MIN: f32 = 0.5;
pub const SCALE_MAX: f32 = 2.0;

impl Graphics {
    /// One of the presets (taa and vsync on).
    pub fn preset(l: Level) -> Self {
        let mut g = Graphics { scale: 1.0, shadows: Level::High, clouds: Level::High, storms: Level::High, terrain: Level::High, scenery: Level::High, city: Level::High, trees: Level::High, cars: Level::High, birds: Level::High, ao: true, taa: true, vsync: true, fps_cap: 0, dynamic_res: false };
        g.apply(l);
        g
    }

    /// Sets everything a preset covers (not taa or vsync).
    pub fn apply(&mut self, l: Level) {
        let l = if l == Level::Off { Level::Low } else { l };
        // each preset down roughly halves gpu work, resolution first, then shadows.
        self.scale = l.pick(0.6, 0.75, 1.0, 1.5) as f32;
        self.shadows = match l {
            Level::Off | Level::Low => Level::Off,
            Level::Medium => Level::Low,
            h => h,
        };
        self.clouds = l;
        self.storms = l;
        self.terrain = l;
        self.scenery = l;
        self.city = l;
        self.trees = l;
        self.cars = if l == Level::Ultra { Level::High } else { l };
        self.birds = if l == Level::Ultra { Level::High } else { l };
        self.ao = matches!(l, Level::High | Level::Ultra);
    }

    /// The preset these settings are, if any.
    pub fn matches(&self) -> Option<Level> {
        Level::PRESETS.into_iter().find(|l| {
            let mut p = *self;
            p.apply(*l);
            p == *self
        })
    }

    /// msaa samples, applied on next start. 4 on ultra, 2 otherwise since taa does most of the work.
    pub fn samples(&self) -> u32 {
        if self.matches() == Some(Level::Ultra) { 4 } else { 2 }
    }

    /// Side of the sun's shadow map (texels; Off keeps a small unused one).
    pub fn shadow_res(&self) -> u32 {
        match self.shadows {
            Level::Off => 256,
            Level::Low => 1024,
            Level::Medium => 2048,
            Level::High => 4096,
            Level::Ultra => 6144,
        }
    }

    /// Length of the cloud raymarch's steps relative to High (clouds.wgsl).
    pub fn cloud_step(&self) -> f32 {
        self.clouds.pick(1.8, 1.35, 1.0, 0.7) as f32
    }

    /// Storm cell quality tier passed in Globals.detail.w (1 Low .. 4 Ultra;
    /// 0 in WGSL falls back to High).
    pub fn storm_tier(&self) -> f32 {
        match self.storms {
            Level::Off | Level::Low => 1.0,
            Level::Medium => 2.0,
            Level::High => 3.0,
            Level::Ultra => 4.0,
        }
    }

    /// Maximum nearby storm cells uploaded for rendering (nearest first; flight
    /// physics and weather radar still use all cells in `Weather::cells`).
    pub fn storm_max_cells(&self) -> usize {
        match self.storms {
            Level::Off | Level::Low => 8,
            Level::Medium => 12,
            Level::High | Level::Ultra => crate::weather::MAX_STORMS,
        }
    }

    /// Terrain tile split threshold (tile width / distance, world.rs `lod_k`):
    /// lower splits sooner, so finer tiles reach further.
    pub fn lod_k(&self) -> f64 {
        self.terrain.pick(0.95, 0.78, 0.62, 0.5)
    }

    /// Multiplies how far out building (and tree) tiles are loaded.
    pub fn reach(&self) -> f64 {
        self.scenery.pick(0.6, 0.8, 1.0, 1.3)
    }

    /// Multiplies the distances buildings change level of detail at (their
    /// dissolve into far stand-ins, the dressing, the window depth close up).
    pub fn city_k(&self) -> f32 {
        self.city.pick(0.45, 0.7, 1.0, 1.4) as f32
    }

    /// How far out trees are drawn (m); the last fifth dissolves away.
    pub fn tree_far(&self) -> f32 {
        self.trees.pick(1400.0, 2200.0, 3200.0, 4500.0) as f32
    }

    /// Multiplies the distance trees turn from models into impostors at.
    /// Never above 1: the leaf cards and the impostor bake are made for it.
    pub fn tree_imp_k(&self) -> f32 {
        self.trees.pick(0.5, 0.75, 1.0, 1.0) as f32
    }

    /// Share of the cars on the roads (0..1, by each car's own rank) and how
    /// far off moving and parked cars are drawn (m).
    pub fn car_density(&self) -> (f32, f64, f64) {
        match self.cars {
            Level::Off => (0.0, 0.0, 0.0),
            Level::Low => (0.35, 900.0, 450.0),
            Level::Medium => (0.65, 1500.0, 750.0),
            Level::High | Level::Ultra => (1.0, 2200.0, 1100.0),
        }
    }

    /// Whether the settings have ever been saved (else the defaults may
    /// adapt to the machine, e.g. a lower resolution on an integrated GPU).
    pub fn saved() -> bool {
        Self::path().is_some_and(|p| p.exists())
    }

    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("fablegame").join("graphics.json"))
    }

    /// The saved settings, or High.
    pub fn load() -> Self {
        let j: serde_json::Value = Self::path().and_then(|p| std::fs::read(p).ok()).and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
        let mut g = Self::default();
        g.fps_cap = j["fps_cap"].as_u64().map(|v| v as u16).filter(|v| [0, 30, 60, 90, 120, 144, 165, 240].contains(v)).unwrap_or(0);
        g.dynamic_res = j["dynamic_res"].as_bool().unwrap_or(g.dynamic_res);
        // a saved preset follows the code, so retuned presets reach old saves.
        if let Some(l) = j["preset"].as_str().and_then(Level::parse) {
            g.apply(l);
            g.taa = j["taa"].as_bool().unwrap_or(g.taa);
            g.vsync = j["vsync"].as_bool().unwrap_or(g.vsync);
            return g;
        }
        let lvl = |k: &str, d: Level| j[k].as_str().and_then(Level::parse).filter(|l| *l != Level::Off).unwrap_or(d);
        if let Some(s) = j["scale"].as_f64() {
            g.scale = (s as f32).clamp(SCALE_MIN, SCALE_MAX);
        }
        g.shadows = j["shadows"].as_str().and_then(Level::parse).unwrap_or(g.shadows);
        g.clouds = lvl("clouds", g.clouds);
        g.storms = lvl("storms", g.clouds);
        g.terrain = lvl("terrain", g.terrain);
        g.scenery = lvl("scenery", g.scenery);
        g.city = lvl("city", g.city);
        g.trees = lvl("trees", g.trees);
        g.cars = j["cars"].as_str().and_then(Level::parse).map(|l| if l == Level::Ultra { Level::High } else { l }).unwrap_or(g.cars);
        g.birds = j["birds"].as_str().and_then(Level::parse).map(|l| if l == Level::Ultra { Level::High } else { l }).unwrap_or(g.birds);
        g.ao = j["ao"].as_bool().unwrap_or(g.ao);
        g.taa = j["taa"].as_bool().unwrap_or(g.taa);
        g.vsync = j["vsync"].as_bool().unwrap_or(g.vsync);
        g
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        let j = serde_json::json!({
            "preset": self.matches().map(Level::name).unwrap_or("custom"),
            "scale": self.scale,
            "shadows": self.shadows.name(),
            "clouds": self.clouds.name(),
            "storms": self.storms.name(),
            "terrain": self.terrain.name(),
            "scenery": self.scenery.name(),
            "city": self.city.name(),
            "trees": self.trees.name(),
            "cars": self.cars.name(),
            "birds": self.birds.name(),
            "ao": self.ao,
            "taa": self.taa,
            "vsync": self.vsync,
            "fps_cap": self.fps_cap,
            "dynamic_res": self.dynamic_res,
        });
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&j).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// The preset row and what the chosen preset does (the settings
    /// screen's Graphics section, settings_ui.rs).
    pub fn preset_ui(&mut self, ui: &mut egui::Ui) {
        row(ui, "Preset", "sets everything below at once", |ui| {
            ui.horizontal(|ui| {
                let cur = self.matches();
                let mut pick = cur;
                menu::segments(ui, |ui| {
                    for l in Level::PRESETS {
                        menu::seg(ui, &mut pick, Some(l), l.name());
                    }
                });
                if let (Some(l), true) = (pick, pick != cur) {
                    self.apply(l);
                }
                if cur.is_none() {
                    ui.label(RichText::new("Custom").font(menu::label()).color(DIM));
                }
            });
        });
        note(ui, match self.matches() {
            Some(Level::Low) => "Low: 60% resolution, no shadows, quicker clouds, nearer scenery, simpler cities and trees, light traffic, no ambient occlusion.",
            Some(Level::Medium) => "Medium: 75% resolution, a coarse shadow map, no ambient occlusion and a lighter version of everything else.",
            Some(Level::High) => "High: the full look at the window's own resolution (what the game has always drawn).",
            Some(Level::Ultra) => "Ultra: past High. Supersampled at 150%, a 6K shadow map, finer clouds, terrain and scenery further out. Wants a strong GPU.",
            _ => "Custom: tuned one by one below.",
        });
    }

    /// Shadows, clouds, terrain, scenery, cities, trees, traffic, AO: what the presets set.
    pub fn quality_ui(&mut self, ui: &mut egui::Ui) {
        let levels = |ui: &mut egui::Ui, v: &mut Level, off: bool| {
            menu::segments(ui, |ui| {
                if off {
                    menu::seg(ui, v, Level::Off, "Off");
                }
                for l in Level::PRESETS {
                    menu::seg(ui, v, l, l.name());
                }
            });
        };
        let dim = |ui: &mut egui::Ui, s: String| {
            ui.label(RichText::new(s).font(menu::caption()).color(DIM));
        };
        row(ui, "Shadows", "", |ui| {
            levels(ui, &mut self.shadows, true);
            if self.shadows != Level::Off {
                let r = self.shadow_res();
                dim(ui, format!("{r}×{r} map"));
            }
        });
        row(ui, "Clouds", "", |ui| levels(ui, &mut self.clouds, false));
        row(ui, "Storm cells", "thunderstorms, supercells, squall lines", |ui| {
            levels(ui, &mut self.storms, false);
            dim(ui, match self.storms {
                Level::Off | Level::Low => "lighter lighting and detail, up to 8 nearest cells".into(),
                Level::Medium => "faster march and lighting, up to 12 nearest cells".into(),
                Level::High => "full billows, lighting and up to 16 cells".into(),
                Level::Ultra => "finer steps and full storm detail".into(),
            });
        });
        row(ui, "Terrain detail", "", |ui| levels(ui, &mut self.terrain, false));
        row(ui, "Scenery distance", "", |ui| {
            levels(ui, &mut self.scenery, false);
            dim(ui, format!("buildings and trees out to {:.1} km", 7.5 * self.reach()));
        });
        row(ui, "City detail", "", |ui| {
            levels(ui, &mut self.city, false);
            let k = self.city_k();
            dim(ui, format!("full buildings out to {:.1} km (towers further), lit window depth to {:.0} m", 1.3 * k, 550.0 * k));
        });
        row(ui, "Tree detail", "", |ui| {
            levels(ui, &mut self.trees, false);
            dim(ui, format!("trees out to {:.1} km, full models to {:.0} m", self.tree_far() / 1000.0, 120.0 * self.tree_imp_k()));
        });
        row(ui, "Traffic", "cars on the roads", |ui| {
            menu::segments(ui, |ui| {
                for l in [Level::Off, Level::Low, Level::Medium, Level::High] {
                    menu::seg(ui, &mut self.cars, l, l.name());
                }
            });
            let (share, far, _) = self.car_density();
            dim(ui, if share > 0.0 { format!("{:.0}% of the cars, out to {:.1} km", share * 100.0, far / 1000.0) } else { "no cars on the roads".into() });
        });
        row(ui, crate::i18n::t!("Birds", "Oiseaux"), crate::i18n::t!("gulls, crows, starlings, buzzards and geese, by place, season and hour", "goélands, corneilles, étourneaux, buses et oies, selon le lieu, la saison et l'heure"), |ui| {
            menu::segments(ui, |ui| {
                for l in [Level::Off, Level::Low, Level::Medium, Level::High] {
                    menu::seg(ui, &mut self.birds, l, l.name());
                }
            });
            let (flocks, birds) = crate::birds::caps(self.birds);
            dim(ui, if flocks > 0 { crate::i18n::t!("up to {} flocks, {} birds", "jusqu'à {} volées, {} oiseaux", flocks, birds) } else { crate::i18n::t!("no birds", "aucun oiseau").to_string() });
        });
        row(ui, "Ambient occlusion", "contact shadows (GTAO)", |ui| {
            menu::switch(ui, &mut self.ao);
        });
    }

    /// Render resolution, anti-aliasing, vsync and the frame limit. `window`
    /// is the window's size in pixels (for the resolution readout), `fps`
    /// the frame rate now.
    pub fn display_ui(&mut self, ui: &mut egui::Ui, window: (u32, u32), fps: f64) {
        row(ui, "Render resolution", "below 100% upscales, above supersamples", |ui| {
            let (w, h) = window;
            let (rw, rh) = ((w as f32 * self.scale).round() as u32, (h as f32 * self.scale).round() as u32);
            ui.add(egui::Slider::new(&mut self.scale, SCALE_MIN..=SCALE_MAX).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            let what = if self.scale > 1.001 { "supersampled" } else if self.scale < 0.999 { "upscaled" } else { "native" };
            ui.label(RichText::new(format!("{rw}×{rh}, {what}")).font(menu::caption()).color(DIM));
        });
        row(ui, "Dynamic resolution", "holds 60 fps by lowering the resolution a little when the GPU falls behind", |ui| {
            menu::switch(ui, &mut self.dynamic_res);
        });
        row(ui, "Anti-aliasing", "temporal (TAA) · F2 in flight", |ui| {
            menu::switch(ui, &mut self.taa);
        });
        row(ui, "VSync", if self.vsync { "no tearing" } else { "uncapped, may tear" }, |ui| {
            menu::switch(ui, &mut self.vsync);
        });
        row(ui, "Frame limit", "frames per second", |ui| {
            menu::segments(ui, |ui| {
                for cap in [0u16, 30, 60, 90, 120, 144, 165, 240] {
                    menu::seg(ui, &mut self.fps_cap, cap, &if cap == 0 { "Unlimited".to_owned() } else { cap.to_string() });
                }
            });
        });
        note(ui, &format!("Now {fps:.0} fps. Changes apply at once; scenery and terrain fill in over a few seconds."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip() {
        for l in Level::PRESETS {
            assert_eq!(Graphics::preset(l).matches(), Some(l));
        }
        let mut g = Graphics::preset(Level::High);
        g.taa = false;
        g.vsync = false;
        assert_eq!(g.matches(), Some(Level::High), "taa and vsync are not part of a preset");
        g.scale = 1.2;
        assert_eq!(g.matches(), None);
    }

    #[test]
    fn high_is_the_old_look() {
        let g = Graphics::default();
        assert_eq!((g.scale, g.shadow_res(), g.cloud_step(), g.lod_k(), g.reach(), g.ao), (1.0, 4096, 1.0, 0.62, 1.0, true));
        assert_eq!((g.storms, g.storm_tier(), g.storm_max_cells()), (Level::High, 3.0, crate::weather::MAX_STORMS));
        assert_eq!((g.city_k(), g.tree_far(), g.tree_imp_k(), g.car_density()), (1.0, 3200.0, 1.0, (1.0, 2200.0, 1100.0)));
    }

    #[test]
    fn storm_quality_independent_of_clouds() {
        let mut g = Graphics::preset(Level::High);
        g.storms = Level::Medium;
        assert_eq!(g.matches(), None, "changing storm quality independently makes a custom profile");
        assert_eq!((g.clouds, g.storms, g.storm_tier(), g.storm_max_cells()), (Level::High, Level::Medium, 2.0, 12));
        g.storms = Level::Low;
        assert_eq!((g.storm_tier(), g.storm_max_cells()), (1.0, 8));
        g.storms = Level::Ultra;
        assert_eq!((g.storm_tier(), g.storm_max_cells()), (4.0, crate::weather::MAX_STORMS));
    }
}
