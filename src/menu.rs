//! Main menu: the planet itself is the map. Spin it, zoom in, click where
//! you want to start.
use crate::{geo, t};
use glam::DVec3;

/// (name, latitude, longitude, tier). Tier 1 is always labelled, tier 2 once zoomed in a little.
pub const CITIES: &[(&str, f64, f64, u8)] = &[
    ("San Francisco", 37.7749, -122.4194, 1),
    ("Los Angeles", 34.0522, -118.2437, 1),
    ("New York", 40.7128, -74.0060, 1),
    ("Chicago", 41.8781, -87.6298, 2),
    ("Seattle", 47.6062, -122.3321, 2),
    ("Vancouver", 49.2827, -123.1207, 2),
    ("Denver", 39.7392, -104.9903, 2),
    ("Miami", 25.7617, -80.1918, 2),
    ("Toronto", 43.6532, -79.3832, 2),
    ("Mexico City", 19.4326, -99.1332, 1),
    ("Havana", 23.1136, -82.3666, 2),
    ("Bogota", 4.7110, -74.0721, 2),
    ("Lima", -12.0464, -77.0428, 2),
    ("Rio de Janeiro", -22.9068, -43.1729, 1),
    ("Buenos Aires", -34.6037, -58.3816, 1),
    ("Santiago", -33.4489, -70.6693, 2),
    ("Honolulu", 21.3069, -157.8583, 1),
    ("Anchorage", 61.2181, -149.9003, 2),
    ("Reykjavik", 64.1466, -21.9426, 2),
    ("London", 51.5074, -0.1278, 1),
    ("Paris", 48.8566, 2.3522, 1),
    ("Madrid", 40.4168, -3.7038, 2),
    ("Lisbon", 38.7223, -9.1393, 2),
    ("Rome", 41.9028, 12.4964, 1),
    ("Berlin", 52.5200, 13.4050, 2),
    ("Amsterdam", 52.3676, 4.9041, 2),
    ("Zermatt", 46.0207, 7.7491, 2),
    ("Venice", 45.4408, 12.3155, 2),
    ("Athens", 37.9838, 23.7275, 2),
    ("Istanbul", 41.0082, 28.9784, 1),
    ("Stockholm", 59.3293, 18.0686, 2),
    ("Oslo", 59.9139, 10.7522, 2),
    ("Moscow", 55.7558, 37.6173, 1),
    ("Cairo", 30.0444, 31.2357, 1),
    ("Marrakesh", 31.6295, -7.9811, 2),
    ("Lagos", 6.5244, 3.3792, 2),
    ("Nairobi", -1.2921, 36.8219, 2),
    ("Cape Town", -33.9249, 18.4241, 1),
    ("Dubai", 25.2048, 55.2708, 1),
    ("Tehran", 35.6892, 51.3890, 2),
    ("Mumbai", 19.0760, 72.8777, 1),
    ("Delhi", 28.6139, 77.2090, 2),
    ("Kathmandu", 27.7172, 85.3240, 2),
    ("Bangkok", 13.7563, 100.5018, 2),
    ("Singapore", 1.3521, 103.8198, 1),
    ("Hong Kong", 22.3193, 114.1694, 1),
    ("Shanghai", 31.2304, 121.4737, 2),
    ("Beijing", 39.9042, 116.4074, 1),
    ("Seoul", 37.5665, 126.9780, 2),
    ("Tokyo", 35.6762, 139.6503, 1),
    ("Manila", 14.5995, 120.9842, 2),
    ("Jakarta", -6.2088, 106.8456, 2),
    ("Perth", -31.9523, 115.8613, 2),
    ("Sydney", -33.8688, 151.2093, 1),
    ("Melbourne", -37.8136, 144.9631, 2),
    ("Auckland", -36.8509, 174.7645, 2),
    ("Queenstown", -45.0312, 168.6626, 2),
];

/// Places offered as one-click starts.
pub const QUICK: &[(&str, f64, f64)] = &[
    ("Golden Gate", 37.8085, -122.4650),
    ("Manhattan", 40.7580, -73.9855),
    ("Yosemite Valley", 37.7340, -119.6000),
    ("Matterhorn", 45.9950, 7.7000),
    ("Paris", 48.8584, 2.2945),
    ("Rio de Janeiro", -22.9519, -43.2105),
    ("Hong Kong", 22.2930, 114.1694),
    ("Sydney Harbour", -33.8523, 151.2108),
    ("Dubai", 25.1972, 55.2744),
    ("Black Forest", 48.0500, 8.1500),
];

/// Airports offered as one-click starts: big and small, flat and mountain, beach and city.
pub const AIRPORTS: &[(&str, &str)] = &[
    ("KSFO", "San Francisco"),
    ("KJFK", "New York JFK"),
    ("KLAX", "Los Angeles"),
    ("EGLL", "London Heathrow"),
    ("LFPG", "Paris CDG"),
    ("LSZH", "Zürich"),
    ("OMDB", "Dubai"),
    ("VHHH", "Hong Kong"),
    ("RJTT", "Tokyo Haneda"),
    ("YSSY", "Sydney"),
    ("NZQN", "Queenstown"),
    ("TNCM", "St Maarten"),
    ("LXGB", "Gibraltar"),
    ("LFLJ", "Courchevel"),
    ("KHAF", "Half Moon Bay"),
    ("KOSH", "Oshkosh"),
];

pub struct Menu {
    /// what the camera is looking straight down at, degrees
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
    target_alt: f64,
    pub pick: Option<(f64, f64)>,
    pub pick_name: String,
    pub search: String,
    pub hour: f64,
    pub press: Option<(f64, f64)>,
    pub dragged: f64,
    pub starting: bool,
    /// which start screen is up: the main menu, the globe, the aircraft or the settings
    pub screen: Screen,
    /// where the aircraft screen goes back to (the main menu or the globe)
    pub air_back: Screen,
    /// the aircraft looked at on the aircraft screen (index into `aircraft::list()`)
    pub air_view: usize,
    spin: f64,
    /// the chosen spot is this airport, and how to start there
    pub airport: Option<u32>,
    pub start: Start,
    /// runway end to start on (its ident); None takes the one into the wind
    pub runway: Option<String>,
    /// height above the ground for a start in the air (m)
    pub start_agl: f64,
    /// airport markers drawn on the globe last frame: (index, x, y in physical pixels)
    pub markers: Vec<(u32, f32, f32)>,
    /// which aircraft to fly (index into `aircraft::list()`)
    pub aircraft: usize,
    /// the aircraft screen's page: the aircraft you have, or more to download
    pub air_tab: crate::hangar_ui::Tab,
    /// the collection entry picked out on its download page, what its search has, and whether it lists the unflyable ones too
    pub hangar_sel: Option<String>,
    pub hangar_find: String,
    pub hangar_all: bool,
    /// fuel to start with (fraction of full tanks), and an engine left off for a start on the ground
    pub fuel: f64,
    pub cold: bool,
    /// payload, as a fraction of the most the aircraft takes (None: as it comes)
    pub payload: Option<f64>,
}

/// The screens of the start menu. The game boots into `Main`; `Fly` is the
/// globe (where to start, the conditions, take-off).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Screen {
    Main,
    Fly,
    Aircraft,
    Settings,
}

impl Screen {
    /// `--menu-screen main|fly|aircraft|settings`
    pub fn parse(s: &str) -> Option<Screen> {
        Some(match s.trim().to_lowercase().as_str() {
            "main" | "menu" | "home" => Screen::Main,
            "fly" | "globe" | "start" => Screen::Fly,
            "aircraft" | "hangar" | "planes" => Screen::Aircraft,
            "settings" | "options" => Screen::Settings,
            _ => return None,
        })
    }
}

/// A longitude for the globe's first view that has the sun on it: the
/// subsolar longitude at `utc` (hours), pulled up to 35° towards the nearest
/// of a few land-rich views, so it opens on lit continents rather than the
/// night side or open ocean.
pub fn day_side_lon(utc: f64) -> f64 {
    let wrap = |x: f64| (x + 180.0).rem_euclid(360.0) - 180.0;
    let sub = wrap(-15.0 * (utc - 12.0));
    // the Americas, Europe and Africa, India, East Asia
    let land = [-95.0, -60.0, 15.0, 78.0, 115.0];
    let d = land.iter().map(|l| wrap(l - sub)).min_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap_or(0.0);
    // (a little to the west of it: the afternoon side, the terminator on
    // the eastern limb and the slow spin still on the day side for a while)
    wrap(sub + d.clamp(-35.0, 35.0) - 15.0)
}

/// How to start at an airport chosen on the globe.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Start {
    /// at rest on the runway, lined up for take-off
    Runway,
    /// on a 3° final approach to it
    Approach,
    /// in the air above it, at the chosen height
    Overhead,
}

impl Menu {
    pub fn new(lat: f64, lon: f64, hour: f64) -> Self {
        Self { lat, lon, alt: 1.5e7, target_alt: 1.1e7, pick: None, pick_name: String::new(), search: String::new(), hour, press: None, dragged: 0.0, starting: false, screen: Screen::Main, air_back: Screen::Main, air_view: 0, spin: 1.0, airport: None, start: Start::Runway, runway: None, start_agl: 450.0, markers: Vec::new(), aircraft: 0, air_tab: crate::hangar_ui::Tab::Mine, hangar_sel: None, hangar_find: String::new(), hangar_all: false, fuel: 0.5, cold: false, payload: None }
    }

    pub fn look_at(&mut self, lat: f64, lon: f64, alt: f64) {
        (self.lat, self.lon, self.target_alt) = (lat, lon, alt);
        self.alt = self.alt.max(alt);
        self.spin = 0.0;
    }

    /// Eye position and the view basis (direction, up).
    pub fn camera(&self) -> (DVec3, DVec3, DVec3) {
        let cam = geo::lla_to_pos(self.lat.to_radians(), self.lon.to_radians(), self.alt);
        let (_, n, u) = geo::enu(cam);
        (cam, -u, n)
    }

    pub fn update(&mut self, dt: f64) {
        self.alt += (self.target_alt - self.alt) * (dt * 6.0).min(1.0);
        // The globe drifts slowly (0.3°/s, so the opening view stays on the
        // day side for minutes) until you drag, zoom or pick on it.
        if self.spin > 0.0 && self.press.is_none() {
            self.lon = (self.lon + dt * 0.3 * self.spin + 180.0).rem_euclid(360.0) - 180.0;
        }
    }

    pub fn zoom(&mut self, d: f64) {
        self.target_alt = (self.target_alt * 0.82f64.powf(d)).clamp(900.0, 2.6e7);
        self.spin = 0.0;
    }

    /// Metres on the ground per screen pixel, at the centre of the view.
    fn metres_per_pixel(&self, h: f64, fov: f64) -> f64 {
        self.alt.min(geo::R * 1.1) * 2.0 * (fov * 0.5).tan() / h
    }

    pub fn drag(&mut self, dx: f64, dy: f64, h: f64, fov: f64) {
        let mpp = self.metres_per_pixel(h, fov);
        self.lat = (self.lat + (dy * mpp / geo::R).to_degrees()).clamp(-84.0, 84.0);
        self.lon -= (dx * mpp / (geo::R * self.lat.to_radians().cos().max(0.15))).to_degrees();
        self.lon = (self.lon + 180.0).rem_euclid(360.0) - 180.0;
        self.dragged += dx.abs() + dy.abs();
        self.spin = 0.0;
    }

    /// The point on the globe under a screen position, if any.
    /// (`cx`: where the middle of the view is across the window, 0..1.)
    pub fn pick_at(&self, x: f64, y: f64, w: f64, h: f64, fov: f64, cx: f64) -> Option<(f64, f64)> {
        let (cam, dir, up) = self.camera();
        let right = dir.cross(up);
        let t = (fov * 0.5).tan();
        let d = (dir + right * ((x / w - cx) * 2.0 * t * w / h) + up * ((1.0 - y / h * 2.0) * t)).normalize();
        let b = cam.dot(d);
        let c = cam.length_squared() - geo::R * geo::R;
        let disc = b * b - c;
        if disc < 0.0 {
            return None;
        }
        let hit = cam + d * (-b - disc.sqrt());
        let (lat, lon, _) = geo::pos_to_lla(hit);
        Some((lat.to_degrees(), lon.to_degrees()))
    }

    /// The airport marker under a screen position (physical pixels), if any.
    pub fn marker_at(&self, x: f64, y: f64) -> Option<u32> {
        self.marker_near(x, y, 14.0)
    }

    /// The airport marker nearest a screen position, within `r` pixels.
    pub fn marker_near(&self, x: f64, y: f64, r: f64) -> Option<u32> {
        self.markers.iter().map(|&(i, mx, my)| (i, (mx as f64 - x).hypot(my as f64 - y))).filter(|m| m.1 < r).min_by(|a, b| a.1.total_cmp(&b.1)).map(|m| m.0)
    }

    /// Nearest listed city, for naming a picked spot before the lookup returns.
    pub fn near_city(lat: f64, lon: f64) -> Option<(&'static str, f64)> {
        let p = geo::lla_to_pos(lat.to_radians(), lon.to_radians(), 0.0);
        CITIES.iter().map(|c| (c.0, (geo::lla_to_pos(c.1.to_radians(), c.2.to_radians(), 0.0) - p).length())).min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

// ---- shared look and widgets for the start screen and the in-flight menu

use crate::i18n::tr;
use crate::padui::{self, Hint};
use egui::{Color32, RichText, Stroke};

// ---- type
//
// The interface's own faces, bundled (assets/fonts, SIL OFL) so every
// machine sees the same thing: Barlow for reading, Barlow Condensed for the
// display lines, IBM Plex Mono for numbers, coordinates, keys and readouts.
// The system's sans stays behind them for any glyph they lack. See
// `load_fonts` in main.rs.
//
// Every screen uses these roles and nothing else, so text of the same job
// looks the same everywhere:
//
//   screen_title  display caps 44   SETTINGS, AIRCRAFT, PAUSED
//   section_title display caps 15   card and panel headings, tracked
//   page_title    SemiBold 28       a page's own heading (General, Cessna 172RG)
//   item_title    SemiBold 17       the name of a thing (a start point, an airport)
//   label         Medium 15         row labels, buttons, tabs, list names
//   body          Regular 15        sentences
//   caption       Regular 13        hints, notes, second lines (DIM)
//   data          Plex Mono 13      numbers, coordinates, key names
//
// Writing: sentence case everywhere ("Camera and view"), except the two
// display roles, which are set in capitals.

/// The display family's name: Barlow Condensed Bold.
pub const BOLD: &str = "bold";
/// Barlow Medium: labels, buttons, list names.
pub const MEDIUM: &str = "medium";
/// Barlow SemiBold: headings and anything that must read as bold.
pub const SEMIBOLD: &str = "semibold";

pub const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/Barlow-Regular.ttf");
pub const FONT_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Barlow-Medium.ttf");
pub const FONT_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Barlow-SemiBold.ttf");
pub const FONT_DISPLAY: &[u8] = include_bytes!("../assets/fonts/BarlowCondensed-Bold.ttf");
pub const FONT_MONO: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf");

/// The display face (condensed bold) at a size: for the few one-off display
/// sizes (FLY, the tile captions). Prefer the roles below.
pub fn bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(BOLD.into()))
}

/// Medium weight at a size.
pub fn medium(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(MEDIUM.into()))
}

/// SemiBold at a size.
pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(SEMIBOLD.into()))
}

pub fn screen_title() -> egui::FontId {
    bold(44.0)
}
pub fn section_title() -> egui::FontId {
    bold(15.0)
}
pub fn page_title() -> egui::FontId {
    semibold(28.0)
}
pub fn item_title() -> egui::FontId {
    semibold(17.0)
}
pub fn label() -> egui::FontId {
    medium(15.0)
}
pub fn body() -> egui::FontId {
    egui::FontId::proportional(15.0)
}
pub fn caption() -> egui::FontId {
    egui::FontId::proportional(13.0)
}
pub fn data() -> egui::FontId {
    egui::FontId::monospace(13.0)
}

/// A screen's title, in capitals (the sidebars, the pause menu).
pub fn screen_heading(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s.to_uppercase()).font(screen_title()).color(Color32::WHITE).extra_letter_spacing(1.5));
}

/// A page's own heading and the line under it.
pub fn page_heading(ui: &mut egui::Ui, title: &str, blurb: &str) {
    ui.label(RichText::new(title).font(page_title()).color(Color32::WHITE));
    if !blurb.is_empty() {
        ui.label(RichText::new(blurb).font(body()).color(DIM));
    }
}

/// A card's or panel's heading: display capitals, tracked.
pub fn section_text(s: &str) -> RichText {
    RichText::new(s.to_uppercase()).font(section_title()).color(Color32::WHITE).extra_letter_spacing(1.0)
}

// ---- colour
//
// One cool, night-sky dark, stepped in surfaces; one warm accent that means
// "this is the thing to press, or the thing you are on". Everything else is
// neutral so the yellow keeps its meaning.

/// the UI accent: a warm aviation yellow
pub const ACCENT: Color32 = Color32::from_rgb(245, 197, 24);
/// the accent a step brighter, for a hovered primary button
pub const ACCENT_HI: Color32 = Color32::from_rgb(255, 214, 72);
/// dark text on the accent colour
pub const ON_ACCENT: Color32 = Color32::from_rgb(26, 20, 2);
/// Corner radius of panels, windows and frames: very sharp, not quite square.
pub const R_PANEL: u8 = 3;
/// Corner radius of buttons, chips, text fields, sliders, checkboxes, scroll bars.
pub const R_WIDGET: u8 = 3;
pub const TEXT: Color32 = Color32::from_rgb(232, 236, 242);
pub const DIM: Color32 = Color32::from_rgb(146, 156, 171);
/// quieter than DIM: credits, unchosen captions (still 4.5:1 on BG)
pub const FAINT: Color32 = Color32::from_rgb(118, 128, 143);
/// the full-screen backdrop
pub const BG: Color32 = Color32::from_rgb(8, 11, 16);
/// a card or group on the backdrop
pub const SURFACE: Color32 = Color32::from_rgb(15, 19, 26);
/// a control's resting fill (buttons, tracks)
pub const RAISED: Color32 = Color32::from_rgb(28, 34, 44);
/// a control under the pointer
pub const RAISED_HI: Color32 = Color32::from_rgb(42, 50, 63);
/// the chosen segment of a segmented control
pub const CHOSEN: Color32 = Color32::from_rgb(66, 78, 98);
/// hairlines: card edges, dividers
pub const LINE: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
/// a firmer edge: control outlines, the rail's divider
pub const LINE_HI: Color32 = Color32::from_rgba_premultiplied(44, 44, 44, 44);
/// links
pub const LINK: Color32 = Color32::from_rgb(132, 184, 255);
pub const OK: Color32 = Color32::from_rgb(112, 214, 138);
pub const BAD: Color32 = Color32::from_rgb(255, 122, 92);
pub const PANEL: Color32 = Color32::from_rgba_premultiplied(9, 12, 17, 232);
/// the see-through glass of the flight panel, over the running game
pub const GLASS: Color32 = Color32::from_rgba_premultiplied(5, 8, 12, 150);

/// Eases a hover (or any on/off state) in and out instead of snapping: 0..1.
pub fn ease(ctx: &egui::Context, id: egui::Id, on: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(id.with("ease"), on, 0.14, egui::emath::easing::cubic_out)
}

/// `a` towards `b` by `t`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

/// A soft round glow: `col` at the centre fading smoothly to nothing at
/// radii `r` (an ellipse when the two differ). No banding: one mesh.
pub fn glow(p: &egui::Painter, c: egui::Pos2, r: egui::Vec2, col: Color32) {
    let n = 48;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, col);
    // eased fall-off: a few rings, each fainter
    let rings = [(0.35, 0.72), (0.65, 0.32), (1.0, 0.0)];
    for (k, a) in rings {
        for i in 0..n {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(c + egui::vec2(t.cos() * r.x * k, t.sin() * r.y * k), col.gamma_multiply(a));
        }
    }
    for i in 0..n as u32 {
        let j = (i + 1) % n as u32;
        mesh.add_triangle(0, 1 + i, 1 + j);
        for ring in 0..2u32 {
            let (a0, b0) = (1 + ring * n as u32, 1 + (ring + 1) * n as u32);
            mesh.add_triangle(a0 + i, b0 + i, b0 + j);
            mesh.add_triangle(a0 + i, b0 + j, a0 + j);
        }
    }
    p.add(mesh);
}

/// A segmented control: a recessed track holding `seg`s side by side, the
/// chosen one raised, its highlight sliding across when the choice changes.
pub fn segments<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(Color32::from_rgba_premultiplied(0, 0, 0, 70))
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(R_WIDGET + 1)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            // A slot under the labels for the highlight, filled once the chosen segment is known.
            let id = ui.next_auto_id().with("seg_slide");
            let slot = ui.painter().add(egui::Shape::Noop);
            ui.ctx().data_mut(|d| d.insert_temp::<Option<egui::Rect>>(seg_key(), None));
            let r = ui.horizontal(|ui| add(ui)).inner;
            if let Some(target) = ui.ctx().data(|d| d.get_temp::<Option<egui::Rect>>(seg_key())).flatten() {
                // eased from where it was: it slides when the choice changes
                let ctx = ui.ctx().clone();
                let x0 = ctx.animate_value_with_time(id.with("x0"), target.left(), 0.18);
                let x1 = ctx.animate_value_with_time(id.with("x1"), target.right(), 0.18);
                let hr = egui::Rect::from_x_y_ranges(x0..=x1, target.y_range());
                let fill = if ui.is_enabled() { CHOSEN } else { RAISED };
                ui.painter().set(slot, egui::Shape::rect_filled(hr, R_WIDGET, fill));
            }
            r
        })
        .inner
}

/// Where a `seg` tells its `segments` track that it is the chosen one.
fn seg_key() -> egui::Id {
    egui::Id::unique("menu::segments chosen")
}

/// One choice of a `segments` control, like `selectable_value`.
pub fn seg<T: PartialEq>(ui: &mut egui::Ui, current: &mut T, value: T, text: &str) -> egui::Response {
    let on = *current == value;
    let g = ui.painter().layout_no_wrap(text.to_string(), label(), TEXT);
    let size = egui::vec2(g.size().x + 26.0, 28.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), on, text));
    if on {
        ui.ctx().data_mut(|d| d.insert_temp::<Option<egui::Rect>>(seg_key(), Some(rect)));
    }
    let lit = ease(ui.ctx(), resp.id, (resp.hovered() || resp.has_focus()) && !on);
    let p = ui.painter();
    if lit > 0.0 {
        p.rect_filled(rect, R_WIDGET, Color32::from_white_alpha((10.0 * lit) as u8));
    }
    let col = if !ui.is_enabled() { FAINT } else if on { Color32::WHITE } else { mix(DIM, TEXT, lit) };
    p.galley(rect.center() - g.size() * 0.5, g, col);
    if resp.clicked() && !on {
        *current = value;
        resp.mark_changed();
    }
    resp
}

/// An on/off switch reading only "On" or "Off": the row it sits in says what it is.
pub fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let t = if *on { crate::t!("On", "Activé") } else { crate::t!("Off", "Désactivé") };
    toggle(ui, on, t)
}

/// An on/off switch with its label after it, in place of a checkbox.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let text: egui::WidgetText = text.into();
    let galley = text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::FontSelection::FontId(body()));
    let (w, h) = (34.0, 18.0);
    let size = egui::vec2(w + if galley.size().x > 0.0 { 10.0 + galley.size().x } else { 0.0 }, 24.0f32.max(galley.size().y));
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *on, galley.text()));
    let t = ui.ctx().animate_bool_with_time_and_easing(resp.id.with("knob"), *on, 0.16, egui::emath::easing::cubic_out);
    let lit = ease(ui.ctx(), resp.id, resp.hovered() || resp.has_focus());
    let en = ui.is_enabled();
    let track = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - h * 0.5), egui::vec2(w, h));
    let p = ui.painter();
    let off_fill = mix(RAISED, RAISED_HI, lit);
    let on_fill = if lit > 0.0 { mix(ACCENT, ACCENT_HI, lit) } else { ACCENT };
    let fill = mix(off_fill, on_fill, t);
    p.rect_filled(track, h * 0.5, if en { fill } else { fill.gamma_multiply(0.45) });
    p.rect_stroke(track, h * 0.5, Stroke::new(1.0, mix(LINE_HI, Color32::TRANSPARENT, t)), egui::StrokeKind::Inside);
    let r = h * 0.5 - 3.0;
    let cx = egui::lerp((track.left() + 3.0 + r)..=(track.right() - 3.0 - r), t);
    let knob = mix(Color32::from_rgb(196, 204, 216), ON_ACCENT, t);
    p.circle_filled(egui::pos2(cx, track.center().y), r, if en { knob } else { knob.gamma_multiply(0.5) });
    if galley.size().x > 0.0 {
        let col = if en { mix(DIM.lerp_to_gamma(TEXT, 0.55), TEXT, lit.max(t * 0.6)) } else { FAINT };
        p.galley(egui::pos2(track.right() + 10.0, rect.center().y - galley.size().y * 0.5), galley, col);
    }
    resp
}

/// Pages of the in-flight panels: the flight panel (Tab) runs beside the
/// game; the pause menu (Esc) stops it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Page {
    Flight,
    Places,
    Radio,
    Weather,
    Game,
    /// the settings screen (settings_ui.rs), over the paused game
    Settings,
    /// the keyboard and gamepad reference
    Controls,
    Failures,
}

impl Page {
    pub const INFO: [Page; 4] = [Page::Flight, Page::Places, Page::Radio, Page::Weather];
    pub const PAUSE: [Page; 4] = [Page::Game, Page::Settings, Page::Controls, Page::Failures];

    pub fn name(self) -> &'static str {
        match self {
            Page::Flight => "Flight",
            Page::Places => "Go to",
            Page::Radio => "Radio",
            Page::Weather => "Weather",
            Page::Game => "Game",
            Page::Settings => "Settings",
            Page::Controls => "Controls",
            Page::Failures => "Failures",
        }
    }

    /// The name shown on its tab, in the interface language (`name` stays
    /// English: the command line's `--pause-menu` goes by it).
    pub fn label(self) -> &'static str {
        crate::i18n::tr(self.name())
    }

    /// on the flight panel (Tab), not the pause menu
    pub fn is_info(self) -> bool {
        Page::INFO.contains(&self)
    }

    /// The page before or after this one in a set, round and round.
    pub fn cycle(set: &[Page], p: Page, next: bool) -> Page {
        let i = set.iter().position(|q| *q == p).unwrap_or(0);
        set[(i + if next { 1 } else { set.len() - 1 }) % set.len()]
    }

    pub fn parse(s: &str) -> Option<Page> {
        let s = s.to_lowercase();
        match s.as_str() {
            "pause" | "menu" => return Some(Page::Game),
            "airports" | "places" => return Some(Page::Places),
            _ => {}
        }
        Page::INFO.into_iter().chain(Page::PAUSE).find(|p| p.name().to_lowercase().starts_with(&s))
    }
}

/// Keyboard and mouse: (keys, what they do). Shown on F1 and the Controls page.
pub const CONTROLS: &[(&str, &[(&str, &str)])] = &[
    ("FLYING", &[
        ("W / S", "pitch down / up"),
        ("A / D", "roll left / right"),
        ("Q / E", "rudder left / right"),
        ("Shift / Ctrl", "throttle up / down"),
        ("Space", "boost"),
        ("B", "wheel brakes"),
        ("F / V", "flaps down / up a notch"),
        ("L", "landing gear down / up"),
        ("/", "speedbrake (airliners)"),
        ("6", "slide the canopy open / shut (Robin)"),
        ("Z", "thrust reversers, on the ground (hold)"),
        ("K", "set the heading indicator to the compass"),
        ("Num 7 / Num 1", "trim nose down / up"),
        ("F4", "start / stop the engine (automatic)"),
        ("Ins", "battery master switch"),
        ("I", "magnetos on / off (jets: fuel controls)"),
        ("O", "starter (hold; jets: engage)"),
        ("Page Up / Down", "mixture richer / leaner"),
        ("J", "navigation instruments (VOR, ILS, ADF)"),
        ("[ / ]", "OBS course left / right"),
        ("X", "altimeter to the QNH"),
        (".", "parking brake"),
    ]),
    ("VIEW", &[
        ("C", "chase / cockpit camera"),
        ("Mouse drag", "look around"),
        ("Mouse wheel", "camera distance / cockpit zoom"),
        ("H", "hide / show the HUD"),
        ("`", "hide / show the minimap"),
        ("F12", "screenshot"),
    ]),
    ("WORLD", &[
        ("T / Y", "time of day forward / back"),
        ("N / M", "more / fewer clouds"),
        ("F3", "weather & time window"),
    ]),
    ("GAME", &[
        ("Tab", "flight panel: instruments, places, weather"),
        ("Esc", "pause menu: settings, globe, quit"),
        ("P", "pause without the menu"),
        ("G", "go to a place or address"),
        ("R", "respawn in the air"),
        ("U", "sound on / off"),
        ("F2", "anti-aliasing (TAA) on / off"),
        ("F1", "quick key reference"),
    ]),
    ("GAMEPAD (MSFS 2020 layout)", &[
        ("Left stick", "pitch and roll"),
        ("LT / RT", "rudder left / right"),
        ("A / B", "throttle up / down (hold)"),
        ("X", "wheel brakes (hold)"),
        ("Y + B", "parking brake on / off"),
        ("Y + D-pad up / dn", "trim nose down / up"),
        ("LB / RB", "flaps up / down a notch"),
        ("Left stick click", "landing gear"),
        ("Right stick", "look around"),
        ("Right stick click", "reset the view"),
        ("D-pad", "quick views"),
        ("View", "chase / cockpit camera"),
        ("Menu", "pause menu (B closes it)"),
        ("Y + View", "flight panel"),
        ("A after a crash", "fly again"),
]),
];

/// The one look shared by every panel: dark glass, yellow accent, roomy widgets.
pub fn apply_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|s| {
        use egui::{FontFamily, FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.5, FontFamily::Name(MEDIUM.into()))),
            (TextStyle::Heading, FontId::new(21.0, FontFamily::Name(SEMIBOLD.into()))),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(12.0, 5.0);
        s.spacing.slider_width = 200.0;
        s.spacing.slider_rail_height = 4.0;
        s.spacing.interact_size.y = 26.0;
        s.spacing.icon_width = 16.0;
        s.spacing.icon_width_inner = 9.0;
        s.spacing.icon_spacing = 8.0;
        s.spacing.scroll = egui::style::ScrollStyle { bar_width: 6.0, floating_allocated_width: 4.0, ..egui::style::ScrollStyle::floating() };
        s.animation_time = 0.14;
        let v = &mut s.visuals;
        v.dark_mode = true;
        v.window_fill = PANEL;
        v.panel_fill = PANEL;
        v.window_corner_radius = R_PANEL.into();
        v.menu_corner_radius = R_PANEL.into();
        v.window_stroke = Stroke::new(1.0, LINE_HI);
        v.extreme_bg_color = Color32::from_rgb(11, 14, 20);
        v.text_edit_bg_color = Some(Color32::from_rgb(11, 14, 20));
        v.faint_bg_color = Color32::from_white_alpha(6);
        v.slider_trailing_fill = true;
        // (selected chips, the slider's trail and selected text)
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.5, ON_ACCENT);
        // links in a calm blue: yellow is for the thing to press
        v.hyperlink_color = LINK;
        v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
        v.interact_cursor = Some(egui::CursorIcon::PointingHand);
        v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.6 };
        let w = &mut v.widgets;
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        for (st, bg, fg) in [
            (&mut w.inactive, RAISED, Color32::from_rgb(222, 227, 234)),
            (&mut w.hovered, RAISED_HI, Color32::WHITE),
            (&mut w.active, CHOSEN, Color32::WHITE),
            (&mut w.open, RAISED_HI, Color32::WHITE),
        ] {
            st.bg_fill = bg;
            st.weak_bg_fill = bg;
            st.fg_stroke = Stroke::new(1.5, fg);
            st.corner_radius = R_WIDGET.into();
            st.expansion = 0.0;
        }
        // a hairline edge so controls (text fields, boxes, buttons) read on any card
        w.inactive.bg_stroke = Stroke::new(1.0, LINE_HI);
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(70));
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.open.bg_stroke = Stroke::new(1.0, LINE_HI);
    });
}

/// A panel frame in the shared style.
pub fn panel() -> egui::Frame {
    egui::Frame::new().fill(PANEL).corner_radius(R_PANEL).inner_margin(20.0).stroke(Stroke::new(1.0, LINE_HI)).shadow(egui::Shadow { offset: [0, 10], blur: 32, spread: 0, color: Color32::from_black_alpha(120) })
}

/// A key or the gamepad's buttons, right-aligned at `right` on a row.
fn paint_hint(p: &egui::Painter, hint: Hint, right: f32, y: f32, pad_h: f32, col: Color32) {
    match hint {
        Hint::Text(h) => {
            // a key's name in the instrument face; a few words in the reading one
            let font = if h.contains(' ') { egui::FontId::proportional(13.0) } else { egui::FontId::monospace(11.0) };
            p.text(egui::pos2(right, y), egui::Align2::RIGHT_CENTER, h, font, col);
        }
        Hint::Pad(spec) => {
            let x = right - padui::spec_width(p, spec, pad_h);
            padui::paint_spec(p, x, y, spec, pad_h, col);
        }
        Hint::None => {}
    }
}

/// A full-width menu button; the primary one is filled with the accent
/// colour, its label in the display face. Its hint (a key, or the gamepad's
/// buttons) sits at its right end. A disabled primary keeps its shape as an
/// outline, so it reads as "not yet" rather than as broken.
pub fn wide_button(ui: &mut egui::Ui, text: &str, hint: Hint, primary: bool, enabled: bool) -> egui::Response {
    let h = if primary { 46.0 } else { 36.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, text));
    let lit = ease(ui.ctx(), resp.id, enabled && (resp.hovered() || resp.has_focus()));
    let p = ui.painter();
    let (font, col, hc) = if primary {
        if enabled {
            p.rect_filled(rect, R_WIDGET, mix(ACCENT, ACCENT_HI, lit));
            (bold(21.0), ON_ACCENT, ON_ACCENT.gamma_multiply(0.65))
        } else {
            p.rect_stroke(rect, R_WIDGET, Stroke::new(1.0, ACCENT.gamma_multiply(0.3)), egui::StrokeKind::Inside);
            (bold(21.0), ACCENT.gamma_multiply(0.42), FAINT)
        }
    } else {
        p.rect_filled(rect, R_WIDGET, mix(RAISED, RAISED_HI, lit));
        p.rect_stroke(rect, R_WIDGET, Stroke::new(1.0, mix(LINE_HI, Color32::from_white_alpha(60), lit)), egui::StrokeKind::Inside);
        (label(), if enabled { mix(TEXT, Color32::WHITE, lit) } else { FAINT }, DIM)
    };
    let spacing = if primary { 1.5 } else { 0.0 };
    let label = if primary { text.to_uppercase() } else { text.to_string() };
    let mut job = egui::text::LayoutJob::default();
    job.append(&label, 0.0, egui::TextFormat { font_id: font, color: col, extra_letter_spacing: spacing, ..Default::default() });
    let g = p.layout_job(job);
    let at = if primary { egui::pos2(rect.center().x - (g.size().x - spacing) * 0.5, rect.center().y - g.size().y * 0.5) } else { egui::pos2(rect.left() + 14.0, rect.center().y - g.size().y * 0.5) };
    p.galley(at, g, col);
    paint_hint(p, hint, rect.right() - 12.0, rect.center().y, if primary { 22.0 } else { 19.0 }, hc);
    resp
}

/// The flight toolbar's icons, drawn in one line weight.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Icon {
    Flight,
    GoTo,
    Radio,
    Weather,
    Map,
    Camera,
    Pause,
}

/// Draws an icon `s` points across, centred on `c`.
pub fn icon(p: &egui::Painter, which: Icon, c: egui::Pos2, s: f32, col: Color32) {
    use egui::{pos2, vec2};
    let st = Stroke::new(1.7, col);
    let u = s * 0.5;
    let arc = |c: egui::Pos2, r: f32, a0: f32, a1: f32| -> Vec<egui::Pos2> { (0..=16).map(|i| { let a = a0 + (a1 - a0) * i as f32 / 16.0; c + vec2(a.cos(), a.sin()) * r }).collect() };
    match which {
        // an aeroplane seen from above
        Icon::Flight => {
            let pts = [(0.0, -1.0), (0.12, -0.75), (0.12, -0.2), (0.95, 0.2), (0.95, 0.35), (0.12, 0.15), (0.1, 0.65), (0.4, 0.85), (0.4, 0.95), (0.0, 0.85)];
            let mut v: Vec<egui::Pos2> = pts.iter().map(|&(x, y)| c + vec2(x, y) * u).collect();
            v.extend(pts.iter().rev().skip(1).take(pts.len() - 2).map(|&(x, y)| c + vec2(-x, y) * u));
            p.add(egui::Shape::closed_line(v, st));
        }
        // a map pin
        Icon::GoTo => {
            let top = c - vec2(0.0, u * 0.25);
            let mut v = arc(top, u * 0.6, std::f32::consts::PI * 0.8, std::f32::consts::PI * 2.2);
            v.push(c + vec2(0.0, u));
            v.insert(0, c + vec2(0.0, u));
            p.add(egui::Shape::line(v, st));
            p.circle_stroke(top, u * 0.22, st);
        }
        // a mast and its waves
        Icon::Radio => {
            p.line_segment([c - vec2(0.0, u * 0.1), c + vec2(0.0, u)], st);
            p.circle_filled(c - vec2(0.0, u * 0.15), u * 0.14, col);
            let pi = std::f32::consts::PI;
            for r in [0.45, 0.85] {
                // to the right, and to the left
                p.add(egui::Shape::line(arc(c - vec2(0.0, u * 0.15), u * r, -0.75, 0.75), st));
                p.add(egui::Shape::line(arc(c - vec2(0.0, u * 0.15), u * r, pi - 0.75, pi + 0.75), st));
            }
        }
        // a cloud
        Icon::Weather => {
            let b = c + vec2(0.0, u * 0.35);
            let mut v = vec![pos2(b.x - u * 0.9, b.y)];
            v.extend(arc(b + vec2(-u * 0.5, -u * 0.25), u * 0.38, std::f32::consts::PI * 0.75, std::f32::consts::PI * 1.5));
            v.extend(arc(b + vec2(0.05 * u, -u * 0.55), u * 0.5, std::f32::consts::PI * 1.1, std::f32::consts::PI * 1.95));
            v.extend(arc(b + vec2(u * 0.55, -u * 0.2), u * 0.36, -std::f32::consts::PI * 0.4, std::f32::consts::PI * 0.4));
            v.push(pos2(b.x + u * 0.9, b.y + u * 0.08));
            v.push(pos2(b.x - u * 0.9, b.y + u * 0.08));
            v.push(pos2(b.x - u * 0.9, b.y));
            p.add(egui::Shape::line(v, st));
        }
        // a folded map
        Icon::Map => {
            let xs = [-0.9, -0.3, 0.3, 0.9];
            let ys = [(-0.7, 0.8), (-0.85, 0.65), (-0.7, 0.8), (-0.85, 0.65)];
            let top: Vec<egui::Pos2> = xs.iter().zip(ys).map(|(&x, (t, _))| c + vec2(x, t) * u).collect();
            let bot: Vec<egui::Pos2> = xs.iter().zip(ys).map(|(&x, (_, b))| c + vec2(x, b) * u).collect();
            let mut v = top.clone();
            v.extend(bot.iter().rev());
            p.add(egui::Shape::closed_line(v, st));
            for k in 1..3 {
                p.line_segment([top[k], bot[k]], st);
            }
        }
        // a camera
        Icon::Camera => {
            let r = egui::Rect::from_center_size(c + vec2(0.0, u * 0.1), vec2(s, s * 0.66));
            p.rect_stroke(r, 2.0, st, egui::StrokeKind::Middle);
            p.line_segment([pos2(c.x - u * 0.35, r.top() - u * 0.2), pos2(c.x + u * 0.35, r.top() - u * 0.2)], st);
            p.circle_stroke(r.center(), u * 0.36, st);
        }
        // two bars
        Icon::Pause => {
            for x in [-0.3, 0.3] {
                p.rect_filled(egui::Rect::from_center_size(c + vec2(x * u, 0.0), vec2(u * 0.28, u * 1.5)), 1.0, col);
            }
        }
    }
}

/// A chevron pointing right, turned `turn` quarter turns clockwise (1 = down).
pub fn chevron(p: &egui::Painter, c: egui::Pos2, size: f32, turn: f32, col: Color32) {
    let rot = egui::emath::Rot2::from_angle(turn * std::f32::consts::FRAC_PI_2);
    let pts = [egui::vec2(-0.3, -0.6), egui::vec2(0.3, 0.0), egui::vec2(-0.3, 0.6)].map(|v| c + rot * v * size);
    p.line(pts.to_vec(), Stroke::new(1.8, col));
}

/// A collapsible section: a full-width header row (its title, a few words
/// on what is inside while it is shut, a chevron) that opens and closes the
/// body under it. Shut ones keep a first look at the start screen calm.
pub fn section(ui: &mut egui::Ui, id: &str, title: &str, summary: &str, open: bool, body: impl FnOnce(&mut egui::Ui)) {
    let id = ui.make_persistent_id(("section", id));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    let k = state.openness(ui.ctx());
    let lit = ease(ui.ctx(), resp.id, resp.hovered() || resp.has_focus());
    let p = ui.painter();
    // shut: a resting row; open: it becomes the head of its group
    p.rect_filled(rect, R_WIDGET, Color32::from_white_alpha((7.0 + 9.0 * lit) as u8));
    let title_col = mix(TEXT, Color32::WHITE, lit.max(k));
    p.text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, title, label(), title_col);
    if k < 1.0 && !summary.is_empty() {
        let col = DIM.gamma_multiply(1.0 - k);
        p.text(rect.right_center() - egui::vec2(32.0, 0.0), egui::Align2::RIGHT_CENTER, summary, egui::FontId::proportional(13.0), col);
    }
    chevron(p, rect.right_center() - egui::vec2(16.0, 0.0), 5.5, k, mix(DIM, Color32::WHITE, lit));
    if resp.clicked() {
        state.toggle(ui);
    }
    state.show_body_unindented(ui, |ui| {
        ui.add_space(2.0);
        egui::Frame::new().inner_margin(egui::Margin { left: 4, right: 4, top: 0, bottom: 4 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
    });
    ui.add_space(2.0);
}

/// The title bar of a start-screen window: a small capitalised name, then a hairline.
pub fn window_title(ui: &mut egui::Ui, s: &str) {
    ui.label(section_text(s));
    ui.add_space(8.0);
}

/// A small section heading.
pub fn heading(ui: &mut egui::Ui, s: &str) {
    if !ui.layout().is_horizontal() {
        ui.add_space(8.0);
    }
    ui.label(section_text(s).color(DIM));
}

/// A key cap: the key's name on a raised plate with a darker lip below.
pub fn key_cap(ui: &mut egui::Ui, keys: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(keys.to_string(), egui::FontId::monospace(11.5), Color32::WHITE);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(g.size().x + 12.0, 20.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, R_WIDGET, Color32::from_rgb(20, 24, 31));
    p.rect_filled(r.with_max_y(r.bottom() - 2.0), R_WIDGET, RAISED_HI);
    p.galley(egui::pos2(r.left() + 6.0, r.center().y - 1.0 - g.size().y * 0.5), g, Color32::WHITE);
    resp
}

/// One `keys … action` row of the controls list.
pub fn key_row(ui: &mut egui::Ui, keys: &'static str, what: &'static str) {
    key_cap(ui, tr(keys));
    // Truncate the description so a long one can't spill into the next column.
    ui.add(egui::Label::new(RichText::new(tr(what)).font(body()).color(TEXT)).truncate());
    ui.end_row();
}

/// One `buttons … action` row of the gamepad controls list.
pub fn pad_row(ui: &mut egui::Ui, spec: &str, what: &'static str) {
    padui::glyphs(ui, spec, 20.0);
    ui.add(egui::Label::new(RichText::new(tr(what)).font(caption()).color(TEXT)).truncate());
    ui.end_row();
}

/// The gamepad, flying: (buttons, what they do).
pub const PAD_FLYING: &[(&str, &str)] = &[
    ("LS", "pitch and roll"),
    ("LT / RT", "rudder left / right"),
    ("A / B", "throttle up / down (hold)"),
    ("X", "wheel brakes (hold)"),
    ("Y + B", "parking brake on / off"),
    ("Y + Up / Down", "trim nose down / up"),
    ("LB / RB", "flaps up / down a notch"),
    ("L3", "landing gear"),
    ("RS", "look around"),
    ("R3", "reset the view"),
    ("Dpad", "quick views"),
    ("View", "chase / cockpit camera"),
    ("A", "fly again after a crash"),
];

/// The gamepad, in the menus and on the globe.
pub const PAD_MENUS: &[(&str, &[(&str, &str)])] = &[
    ("GAMEPAD · MENUS", &[
        ("Menu", "pause menu"),
        ("Y + View", "flight panel (beside the flying)"),
        ("Dpad", "move between choices"),
        ("A", "press / choose"),
        ("Left / Right", "slide a slider"),
        ("LB / RB", "turn the page"),
        ("B", "back, close"),
    ]),
    ("GAMEPAD · GLOBE", &[
        ("LS", "turn the globe"),
        ("LT / RT", "zoom out / in"),
        ("X", "start at the crosshair"),
        ("Menu", "take off"),
        ("B", "back to the main menu"),
    ]),
];

/// The whole controls list, in two columns of sections: the gamepad's while
/// one is plugged in, else the keyboard's.
pub fn controls_list(ui: &mut egui::Ui) {
    if padui::active() {
        ui.columns(2, |cols| {
            heading(&mut cols[0], tr("GAMEPAD · FLYING"));
            egui::Grid::new("pad_fly").num_columns(2).min_row_height(23.0).spacing([10.0, 3.0]).show(&mut cols[0], |ui| {
                for (k, w) in PAD_FLYING {
                    pad_row(ui, k, w);
                }
            });
            for (title, rows) in PAD_MENUS {
                heading(&mut cols[1], tr(title));
                egui::Grid::new(("pad", *title)).num_columns(2).min_row_height(23.0).spacing([10.0, 3.0]).show(&mut cols[1], |ui| {
                    for (k, w) in rows.iter() {
                        pad_row(ui, k, w);
                    }
                });
                cols[1].add_space(4.0);
            }
        });
        return;
    }
    let count = (CONTROLS.len() + 1) / 2;
    let col_w = (ui.available_width() - 8.0) / 2.0;
    ui.columns(2, |cols| {
        for (c, ui) in cols.iter_mut().enumerate() {
            ui.set_max_width(col_w);
            let range = if c == 0 { 0..count } else { count..CONTROLS.len() };
            for (title, rows) in &CONTROLS[range] {
                heading(ui, tr(title));
                egui::Grid::new(("keys", *title))
                    .num_columns(2)
                    .min_row_height(19.0)
                    .spacing([10.0, 3.0])
                    .show(ui, |ui| {
                        for (k, w) in rows.iter() {
                            key_row(ui, k, w);
                        }
                    });
                ui.add_space(4.0);
            }
        }
    });
}

/// North stays at the top; headings increase clockwise, as in the flight model.
pub fn heading_compass(ui: &mut egui::Ui, heading: &mut f64) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(116.0, 116.0), egui::Sense::click_and_drag());
    let center = rect.center();
    if (response.clicked() || response.dragged()) && ui.is_enabled() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let delta = pointer - center;
            if delta.length_sq() > 36.0 {
                let degrees = (delta.x as f64).atan2(-delta.y as f64).to_degrees();
                *heading = degrees.round().rem_euclid(360.0).to_radians();
            }
        }
    }
    response.on_hover_text(t!("Click or drag to choose your heading", "Cliquez ou glissez pour choisir le cap"));
    let painter = ui.painter();
    painter.circle_filled(center, 39.0, Color32::from_rgb(20, 26, 33));
    painter.circle_stroke(center, 39.0, egui::Stroke::new(1.0, DIM));
    for i in 0..24 {
        let angle = i as f32 * std::f32::consts::TAU / 24.0;
        let direction = egui::vec2(angle.sin(), -angle.cos());
        let inner = if i % 6 == 0 { 31.0 } else { 35.0 };
        painter.line_segment([center + direction * inner, center + direction * 39.0], egui::Stroke::new(1.0, DIM));
    }
    for (label, offset) in [
        ("N", egui::vec2(0.0, -49.0)),
        ("E", egui::vec2(49.0, 0.0)),
        ("S", egui::vec2(0.0, 49.0)),
        (t!("W", "O"), egui::vec2(-49.0, 0.0)),
    ] {
        painter.text(center + offset, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), if label == "N" { ACCENT } else { TEXT });
    }
    let direction = egui::vec2(heading.sin() as f32, -heading.cos() as f32);
    let side = egui::vec2(-direction.y, direction.x);
    painter.line_segment([center - direction * 21.0, center + direction * 20.0], egui::Stroke::new(3.0, ACCENT));
    painter.add(egui::Shape::convex_polygon(
        vec![center + direction * 31.0, center + direction * 15.0 + side * 7.0, center + direction * 15.0 - side * 7.0],
        ACCENT, egui::Stroke::NONE,
    ));
    painter.circle_filled(center, 3.5, ACCENT);
}

/// "hh:mm" for an hour of the day.
pub fn hhmm(h: f64) -> String {
    let h = h.rem_euclid(24.0);
    format!("{:02}:{:02}", h as u32, (h.fract() * 60.0) as u32)
}

/// "37.8085°N  122.4650°W"
pub fn latlon(lat: f64, lon: f64) -> String {
    format!("{:.4}°{}  {:.4}°{}", lat.abs(), if lat >= 0.0 { 'N' } else { 'S' }, lon.abs(), if lon >= 0.0 { 'E' } else { 'W' })
}

/// A big left-aligned button of the full-screen pause menu: its text, a hint
/// (key or gamepad buttons) at the right, the primary one in the accent
/// colour, the chosen page's marked with an accent bar.
pub fn big_button(ui: &mut egui::Ui, text: &str, hint: Hint, primary: bool, chosen: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 50.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, chosen, text));
    let lit = ease(ui.ctx(), resp.id, resp.hovered() || resp.has_focus());
    let pick = ui.ctx().animate_bool_with_time_and_easing(resp.id.with("chosen"), chosen, 0.18, egui::emath::easing::cubic_out);
    let p = ui.painter();
    let (fill, col) = if primary {
        (mix(ACCENT, ACCENT_HI, lit), ON_ACCENT)
    } else {
        let rest = Color32::from_rgba_premultiplied(14, 18, 24, 214);
        (mix(mix(rest, Color32::from_rgba_premultiplied(36, 43, 54, 232), lit), Color32::from_rgba_premultiplied(44, 52, 66, 240), pick), mix(TEXT, Color32::WHITE, lit.max(pick)))
    };
    p.rect_filled(rect, R_WIDGET, fill);
    if !primary {
        // the page it opens is showing: a short accent mark grows at the left
        if pick > 0.0 {
            let h = rect.height() * 0.5 * pick;
            p.rect_filled(egui::Rect::from_center_size(egui::pos2(rect.left() + 1.5, rect.center().y), egui::vec2(3.0, h)), 1.0, ACCENT);
        }
        p.rect_stroke(rect, R_WIDGET, Stroke::new(1.0, mix(LINE, Color32::from_white_alpha(40), lit)), egui::StrokeKind::Inside);
    }
    let font = if primary { bold(22.0) } else { item_title() };
    let x = rect.left() + 20.0 + 4.0 * lit * (!primary as u8 as f32);
    if primary {
        let mut job = egui::text::LayoutJob::default();
        job.append(&text.to_uppercase(), 0.0, egui::TextFormat { font_id: font, color: col, extra_letter_spacing: 1.5, ..Default::default() });
        let g = p.layout_job(job);
        p.galley(egui::pos2(x, rect.center().y - g.size().y * 0.5), g, col);
    } else {
        p.text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, text, font, col);
    }
    let hc = if primary { ON_ACCENT.gamma_multiply(0.65) } else { DIM };
    paint_hint(p, hint, rect.right() - 16.0, rect.center().y, 20.0, hc);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screens_parse() {
        assert_eq!(Screen::parse("main"), Some(Screen::Main));
        assert_eq!(Screen::parse("Fly"), Some(Screen::Fly));
        assert_eq!(Screen::parse("globe"), Some(Screen::Fly));
        assert_eq!(Screen::parse("aircraft"), Some(Screen::Aircraft));
        assert_eq!(Screen::parse("settings"), Some(Screen::Settings));
        assert_eq!(Screen::parse("nowhere"), None);
    }

    #[test]
    fn first_globe_view_is_on_the_day_side() {
        for h in 0..96 {
            let utc = h as f64 * 0.25;
            let sub = -15.0 * (utc - 12.0);
            let lon = day_side_lon(utc);
            assert!((-180.0..180.0).contains(&lon));
            // well inside the lit half (90° from the subsolar point)
            let d = ((lon - sub + 540.0).rem_euclid(360.0) - 180.0).abs();
            assert!(d <= 50.0, "utc {utc}: lon {lon} is {d}° from the subsolar point");
        }
    }
}
