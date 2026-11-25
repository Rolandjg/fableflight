mod airframe;
mod airfield;
mod airmodels;
mod airports;
mod approach;
mod audio;
mod autopilot;
mod birds;
mod buildings;
mod aircraft;
mod cockpit;
mod copilot;
mod cars;
mod crash;
mod damage;
mod destruct;
mod fetch;
mod flightlog;
mod frame_pacer;
mod fsr3;
mod fsr4;
mod fgmodel;
mod flightplan;
mod navdata;
mod fluid;
mod dynamic;
mod gputime;
mod geo;
mod globeclouds;
mod gust;
mod hangar;
mod hangar_ui;
mod bind;
mod i18n;
mod jsbsim;
mod live;
mod radar;
mod livewx;
mod mp;
mod mp_proto;
mod nav;
mod settings;
mod settings_ui;
mod systems;
mod gamemode;
mod gauges;
mod gmaps;
mod graphics;
mod graphics_core;
mod progress;
mod tiles3d;
mod impostor;
mod materials;
mod flightui;
mod mainmenu;
mod menu;
mod minimap;
mod vfrmap;
mod padmap;
mod padui;
mod pave;
mod photonight;
mod relight;
mod rain;
mod storm;
mod wake;
mod contrails;
mod weather;
mod weapons;
mod mvt;
mod plane;
mod render;
mod single_sample;
mod stroke_font;
mod roads;
mod scenery;
mod skin;
mod sources;
mod traffic;
mod trees;
mod upscale;
mod ocean;
mod orographic;
mod shore;
mod skyglow;
mod snow;
mod snowmap;
mod snowtrack;
mod world;
mod yasim;
mod app;
mod cli;
mod devtools;
mod game;

use app::*;
use cli::*;
use devtools::*;

#[derive(Copy, Clone, PartialEq)]
enum Mode {
    Menu,
    Fly,
}

/// What the gamepad's buttons work at the moment, besides flying.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Nav {
    Off,
    Globe,
    Pause,
    Panel,
}

/// the fewest points of height the ui is laid out for.
pub const UI_MIN_H: f32 = 860.0;
/// how long a damage or failure flash stays at full strength, then how long it fades (s).
const FLASH_HOLD: f64 = 1.2;
const FLASH_FADE: f64 = 0.8;

/// What was chosen in the in-flight menu, carried out once the UI is drawn.
#[derive(Copy, Clone)]
enum PauseAct {
    Resume,
    Respawn,
    GoTo,
    FlyTo(f64, f64, &'static str),
    /// start at an airport: which, on final approach (else on the runway)
    Airport(u32, bool),
    Globe,
    Quit,
}

use crash::Crash;
use damage::Tear;
use fablegame::mcp;
use glam::{DMat4, DQuat, DVec2, DVec3, Mat4, Vec2, Vec3};
use i18n::{t, tr_en};
use padui::Hint;
use plane::{Controls, Plane};
use render::{DrawObj, Frame, Globals, PSim, Renderer};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

/// The other players on the start screen's globe (multiplayer): a pink
/// that no city, airport or the chosen spot uses, and that shows on sea and land.
const MP_COLOUR: egui::Color32 = egui::Color32::from_rgb(255, 112, 186);
/// route colour.
const ROUTE_COLOUR: egui::Color32 = egui::Color32::from_rgb(214, 104, 255);

const SUN_E: f32 = 10.0;
/// Exposure of the start screen's globe seen from orbit (a day-lit planet in
/// black space; flight's daytime exposure is 0.5).
const GLOBE_EXPOSURE: f32 = 0.5;
/// Flight's exposure at night, away from city lights (by day it is 0.5).
const NIGHT_EXPOSURE: f32 = 5.0;
/// How many stops the night exposure closes down over the brightest downtown
/// (`city_light` 1); a suburb's thinner lights take it down about a third as far.
const CITY_STOPS: f32 = 1.4;

/// How lit the ground around (lat, lon) (degrees) is at night, 0..1 (1: the
/// brightest downtown, NASA Black Marble): the mean over a square that
/// widens with the height `agl` (m), as the view takes in more ground.
fn city_light(lat: f64, lon: f64, agl: f64) -> f32 {
    let cell = (900.0 + agl * 0.6).min(6000.0);
    let g = sources::NightLights::get().grid(lat, lon, 5, cell);
    (g.iter().sum::<f32>() / g.len() as f32 / sources::NL_PEAK).clamp(0.0, 1.0)
}
/// The aircraft's own shadow map: half width (m), how far back toward the sun
/// it starts, and how deep it reaches.
const PSHADOW_R: f32 = 6.2;
const PSHADOW_BACK: f32 = 40.0;
const PSHADOW_DEPTH: f32 = 1600.0;
/// Moonlight relative to sunlight. Far brighter than the real ratio, so a
/// moonlit night reads like a film's night scene instead of a black screen.
const MOON_E: f32 = SUN_E * 0.0035;
/// What a 3D tiles detail level costs, as measured over San Francisco (see
/// docs/photorealistic-3d-tiles-plan.md).
/// the stations the radios are tuned to, for the moving map.
fn minimap_tuned(r: &nav::Radios) -> vfrmap::Tuned {
    vfrmap::Tuned { nav: r.nav.clone(), obs: r.obs, adf: r.adf.clone() }
}

fn detail_hint(d: gmaps::Detail) -> &'static str {
    match d {
        gmaps::Detail::Low => t!("game.low_softer_buildings_4_km"),
        gmaps::Detail::Medium => t!("game.medium_6_km_around_the"),
        gmaps::Detail::High => t!("game.high_default_9_km_around"),
        gmaps::Detail::Ultra => t!("game.ultra_sharpest_close_up_9"),
    }
}

fn attribution_default() -> &'static str {
    t!("game.imagery_esri_maxar_earthstar_geographics")
}
fn attribution_rest() -> &'static str {
    t!("game.elevation_mapzen_aws_terrain_tiles")
}

/// Data sizes ("12.3 MB"), with the units of the language ("12.3 Mo" in French).
fn bytes(n: u64) -> String {
    let s = gmaps::bytes(n);
    let u = |k: &str| format!(" {}", i18n::tr(k));
    s.replace(" GB", &u("unit.gb")).replace(" MB", &u("unit.mb")).replace(" KB", &u("unit.kb")).replace(" B", &u("unit.byte"))
}

/// An aircraft's model and its meshes on the GPU.
struct Loaded {
    spec: &'static aircraft::Spec,
    model: airframe::Model,
    piece_mesh: Vec<usize>,
    group_mesh: Vec<usize>,
    piece_cut: Vec<bool>,
    piece_inner: Vec<bool>,
    group_pivots: Vec<(Vec3, f32)>,
    /// what the anti-aliasing takes to be flying along with the camera:
    /// everything within the aircraft's bounds, and a little around them for
    /// the gear, flaps and control surfaces as they move
    plane_box: [[f32; 4]; 2],
    /// its outside lights (glow sprites, landing-light beam)
    lamps: Vec<airframe::Lamp>,
}

impl Loaded {
    fn build(spec: &'static aircraft::Spec, r: &mut Renderer) -> Loaded {
        Self::upload(spec, Self::make(spec), r)
    }

    /// The CPU half (mesh generation): safe to run off the main thread.
    fn make(spec: &'static aircraft::Spec) -> airframe::Model {
        let t0 = Instant::now();
        let model = match spec.ext.map(|e| e.build()) {
            Some(Ok(m)) => m,
            Some(Err(e)) => {
                log::warn!("{}: its own model doesn't load ({e}); drawing the stand-in", spec.name);
                (spec.build)()
            }
            None => (spec.build)(),
        };
        log::info!("{}: {} pieces, {} vertices, built in {:.0} ms", spec.name, model.pieces.len(), model.pieces.iter().map(|p| p.mesh.v.len()).sum::<usize>(), t0.elapsed().as_secs_f64() * 1000.0);
        crate::progress::step("aircraft");
        model
    }

    fn upload(spec: &'static aircraft::Spec, model: airframe::Model, r: &mut Renderer) -> Loaded {
        let piece_mesh: Vec<usize> = model.pieces.iter().map(|p| r.add_mesh(&p.mesh.v, &p.mesh.i)).collect();
        let piece_cut: Vec<bool> = model.pieces.iter().map(|p| p.mesh.v.iter().any(|v| airframe::cut_by(v.mat[1]))).collect();
        // cabin and cockpit pieces, cabin materials only and no outer skin, culled from far outside.
        let inner_pat = |m: f32| matches!(m as u32, 7..=12 | 14 | 15 | 18..=20);
        let outer_pat = |m: f32| matches!(m as u32, 1..=6 | 13 | 16 | 17);
        let piece_inner: Vec<bool> = model.pieces.iter().map(|p| !p.glass && p.mesh.v.iter().any(|v| inner_pat(v.mat[1])) && !p.mesh.v.iter().any(|v| outer_pat(v.mat[1]))).collect();
        let mut group_mesh: Vec<usize> = model.groups.iter().map(|m| r.add_mesh(&m.v, &m.i)).collect();
        // A downloaded aircraft's own textures, for its pieces and its wreckage.
        if let Some(t) = &model.tex {
            let bind = r.texture_set(t);
            for &i in piece_mesh.iter().chain(group_mesh.iter()) {
                r.set_mesh_bind(i, bind.clone());
            }
        }
        for m in crash::shards() {
            group_mesh.push(r.add_mesh(&m.v, &m.i));
        }
        // Each group's wreckage tumbles about the middle of its bounds.
        let group_pivots = model
            .groups
            .iter()
            .map(|m| {
                let (lo, hi) = m.v.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| (lo.min(v.pos.into()), hi.max(v.pos.into())));
                if m.v.is_empty() { (Vec3::ZERO, 0.5) } else { ((lo + hi) * 0.5, ((hi - lo).max_element() * 0.35).max(0.3)) }
            })
            .collect();
        let (c, h) = model.bounds();
        let plane_box = [c.extend(0.0).to_array(), (h + Vec3::splat(0.25 * spec.span / 5.5)).extend(0.0).to_array()];
        let lamps = model.lamps();
        Loaded { spec, model, piece_mesh, group_mesh, piece_cut, piece_inner, group_pivots, plane_box, lamps }
    }
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Look a place name up with OpenStreetMap's Nominatim. Returns lat, lon (degrees) and a label.
fn geocode(f: &fetch::Fetcher, q: &str) -> Option<(f64, f64, String)> {
    let url = format!("https://nominatim.openstreetmap.org/search?format=json&limit=1&accept-language={}&q={}", i18n::lang().code(), url_encode(q));
    let j: serde_json::Value = serde_json::from_slice(&f.get(&url).ok()?).ok()?;
    let r = j.get(0)?;
    Some((r["lat"].as_str()?.parse().ok()?, r["lon"].as_str()?.parse().ok()?, r["display_name"].as_str().unwrap_or(q).to_string()))
}

fn ground_elev(src: &sources::Sources, lat: f64, lon: f64) -> f64 {
    let id = geo::TileId::at(lat.to_radians(), lon.to_radians(), 11);
    let (u, v) = id.frac(lat.to_radians(), lon.to_radians());
    src.elev(id).map(|e| e.sample(u, v) as f64).unwrap_or(0.0).max(0.0)
}

/// The loading screen between the start screen and the flight: when it
/// began, the most tiles seen loading at once, how far along it is, since
/// when hardly anything has been loading, when it was done (it then fades),
/// and which tip it shows.
#[derive(Clone, Debug)]
struct Loading {
    t0: f64,
    most: usize,
    frac: f64,
    quiet_since: Option<f64>,
    done: Option<f64>,
    tip: usize,
    /// tiles left.
    left: usize,
}
impl Loading {
    /// shown at least this long, at most this long (s), fading out over FADE
    const MIN: f64 = 1.2;
    const MAX: f64 = 25.0;
    const FADE: f64 = 0.6;
    fn new(now: f64) -> Self {
        Loading { t0: now, most: 0, frac: 0.0, quiet_since: None, done: None, tip: (now * 1000.0) as usize, left: 0 }
    }
}

/// The bar for the aircraft's shaders: the pipelines done filled in, and a
/// glint running over the one being compiled (it gives no progress of its own).
fn prep_bar(p: &egui::Painter, bar: egui::Rect, prep: render::AircraftPrep, now: f64, a: f32) {
    let fade = |c: egui::Color32| c.gamma_multiply(a);
    p.rect_filled(bar, menu::R_WIDGET, fade(egui::Color32::from_gray(45)));
    let step = bar.width() / prep.total.max(1) as f32;
    let mut done = bar;
    done.set_width(step * prep.done as f32);
    p.rect_filled(done, menu::R_WIDGET, fade(menu::ACCENT));
    let x0 = bar.left() + done.width();
    let glint = (step * 0.3).min(80.0);
    let x = x0 + ((now * 0.6).fract() as f32) * (step + glint) - glint;
    let g = egui::Rect::from_x_y_ranges(x.max(x0)..=(x + glint).min(x0 + step), bar.y_range());
    if g.width() > 0.0 {
        p.rect_filled(g, menu::R_WIDGET, fade(menu::ACCENT.gamma_multiply(0.55)));
    }
}

/// The aircraft's shaders: the bar `w` wide with its top centred on `top`,
/// what they are at and why it takes so long under it (wrapped to `w`), on a
/// dark panel if `panel`.
fn draw_prep(p: &egui::Painter, top: egui::Pos2, w: f32, prep: render::AircraftPrep, now: f64, a: f32, panel: bool) {
    let fade = |c: egui::Color32| c.gamma_multiply(a);
    let s = prep.secs as u64;
    // (centred line by line: a galley laid out about x = 0)
    let centred = |text: String, col: egui::Color32| {
        let mut job = egui::text::LayoutJob::simple(text, menu::caption(), fade(col), w);
        job.halign = egui::Align::Center;
        p.layout_job(job)
    };
    let head = centred(t!("game.aircraft_shaders", prep.done + 1, prep.total, s / 60, s % 60), menu::ACCENT);
    let note = centred(t!("game.aircraft_shaders_note").to_string(), egui::Color32::from_gray(150));
    let bar = egui::Rect::from_center_size(egui::pos2(top.x, top.y + 3.0), egui::vec2(w, 6.0));
    let y_head = bar.bottom() + 10.0;
    let y_note = y_head + head.size().y + 4.0;
    if panel {
        let r = egui::Rect::from_min_max(egui::pos2(top.x - w / 2.0 - 18.0, top.y - 16.0), egui::pos2(top.x + w / 2.0 + 18.0, y_note + note.size().y + 14.0));
        p.rect_filled(r, 8.0, egui::Color32::from_black_alpha((190.0 * a) as u8));
    }
    prep_bar(p, bar, prep, now, a);
    p.galley(egui::pos2(top.x, y_head), head, egui::Color32::WHITE);
    p.galley(egui::pos2(top.x, y_note), note, egui::Color32::WHITE);
}

/// Shown on the loading screen, one at a time (translated in i18n.rs).
const LOADING_TIPS: &[&str] = &[
    "C switches between the chase view and the cockpit; drag the mouse to look around.",
    "F4 starts or stops the engine; for a start by hand: Ins battery, Page Up mixture, I magnetos, hold O.",
    "The Radio page of the flight menu (Tab) tunes VORs and ILSs; APR on the autopilot flies the ILS down.",
    "X sets the altimeter to the QNH before take-off and on the way down.",
    "F and V move the flaps a notch; L the landing gear; B the wheel brakes.",
    "Tab opens the flight menu: the aircraft, places, radio, autopilot, weather, the map and the camera.",
    "R starts again in the air over where you are; G goes to any place or address.",
    "The weather and the traffic are real: METAR at the airports, live aircraft around you.",
    "F1 lists every key; joysticks and pedals are set up in Settings, Bindings.",
];

struct Game {
    window: Arc<Window>,
    r: Renderer,
    world: world::World,
    src: Arc<sources::Sources>,
    /// the Google API key as typed in the settings, and whether it is shown
    gkey: String,
    gkey_show: bool,
    /// Google Maps logo, once it has been shown
    gm_logo: Option<egui::TextureHandle>,
    /// the start screen's aircraft pictures (by index into `aircraft::list()`), once shown
    stills: std::collections::HashMap<usize, egui::TextureHandle>,
    /// the aircraft being flown, and its model
    spec: &'static aircraft::Spec,
    model: airframe::Model,
    /// renderer mesh of each model piece, of each breakable group, of the two shard shapes
    piece_mesh: Vec<usize>,
    group_mesh: Vec<usize>,
    /// does the piece have openings cut in it
    piece_cut: Vec<bool>,
    /// is the piece inside the cabin.
    piece_inner: Vec<bool>,
    /// each breakable group's centre and size, for the wreckage
    group_pivots: Vec<(Vec3, f32)>,
    /// the models of the other aircraft flown this session, already on the GPU
    hangar: Vec<Loaded>,
    /// aircraft models being built on a background thread (menu selection)
    building: Vec<(&'static aircraft::Spec, std::sync::mpsc::Receiver<airframe::Model>)>,
    plane: Plane,
    /// the F/A-18's missiles: master arm, seeker and the two wingtip shots
    weapons: weapons::Weapons,
    crashed: bool,
    crash: Option<Crash>,
    crash_real: f64,
    minimap: minimap::Minimap,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    keys: HashSet<KeyCode>,
    drag: bool,
    lean_drag: Option<(f64, f64)>,
    last_cursor: Option<(f64, f64)>,
    /// where the mouse is in the window (pixels), whatever it is doing
    cursor: Option<(f64, f64)>,
    /// mouse-look captures relative motion and aims the cockpit from screen centre
    mouse_captured: bool,
    /// the mouse pinned by --cursor (scripted screenshots)
    script_cursor: Option<(f64, f64)>,
    /// scripted mouse events (--click, --wheel)
    script_clicks: Vec<MouseScript>,
    /// a cockpit hotspot the left button went down on: which, when, and
    /// whether its `hold` action has taken over (the key turned to START)
    hot_press: Option<(usize, f64, bool)>,
    /// keys held for a moment by the mouse wheel over a hotspot (until when)
    wheel_hold: Vec<(KeyCode, f64)>,
    /// wheel movement not yet a whole notch (trackpads)
    wheel_acc: cockpit::WheelSteps,
    hot_keys: HashSet<KeyCode>,
    cockpit_controls: Vec<(aircraft::Hotspot, Option<usize>)>,
    pit_blocks: Vec<(usize, Vec3, Vec3)>,
    cam_yaw: f64,
    cam_pitch: f64,
    cam_dist: f64,
    cam_mode: u8,
    view_before_tail: u8,
    cam_pos: DVec3,
    cam_dir: DVec3,
    cam_look: DVec3,
    orbit: f64,
    /// how far the crash camera rises to see past buildings in the way (a
    /// fraction of its distance), and when that was last checked
    cam_rise: f64,
    cam_clear_t: f64,
    /// the crash camera's bearing round what it watches
    cam_ang: Option<f64>,
    cam_hold: f64,
    utc: f64,
    doy: f64,
    /// clouds, haze, wind, clock speed (the F3 panel)
    wx: weather::Weather,
    time: f64,
    real: f64,
    last: Instant,
    acc: f64,
    wave_off: DVec2,
    /// the shore distance field's squares (shore.rs)
    shore: shore::Shore,
    /// the camera in the ocean's coordinates (ocean.rs, wraps at its WRAP)
    ocean_off: DVec2,
    prev_cam: DVec3,
    prev_vp: [[f32; 4]; 4],
    /// graphics settings (the pause menu's Graphics page; taa toggles with F2),
    /// and what was last handed to the renderer and world
    gfx: graphics::Graphics,
    gfx_applied: Option<graphics::Graphics>,
    /// what is drawn with: `gfx`, or below it under dynamic quality (dynamic.rs)
    gfx_eff: graphics::Graphics,
    dynq: dynamic::Dynamic,
    /// storm cells were on screen last frame (dynamic quality lowers them first)
    storms_seen: bool,
    /// the outer shadow cascades as last drawn (matrix, camera), which the
    /// next frame may reuse (they take turns), which of them are valid, and
    /// the shadow map's size they were drawn at
    casc_keep: [([[f32; 4]; 4], DVec3); 2],
    casc_ok: u8,
    casc_res: u32,
    /// dynamic quality asked for on the command line (so it runs in --bench too)
    dyn_cli: bool,
    /// where the aircraft was drawn last frame, relative to that frame's camera
    prev_plane: Mat4,
    /// centre and half size of the aircraft in its body frame
    plane_box: [[f32; 4]; 2],
    /// the outside lights of the aircraft being flown
    lamps: Vec<airframe::Lamp>,
    frame: u64,
    fps: f64,
    /// --fps-log: on, start of this second, frames in it, its slowest frame
    fps_log: (bool, Instant, u32, f64),
    paused: bool,
    show_help: bool,
    show_hud: bool,
    /// the hud in the cockpit view.
    hud_cockpit: bool,
    /// the flight path marker on the hud (off by default)
    path_marker: bool,
    /// saved full screen choice.
    fullscreen: bool,
    /// flight plan state.
    plan: flightplan::Plan,
    /// mouse yoke state.
    mouse_yoke: bool,
    yoke: (f64, f64),
    yoke_thr: f64,
    plan_text: String,
    plan_msg: String,
    plan_pick: bool,
    /// arcade mode (settings.rs): the fun features on
    arcade: bool,
    /// a building crashed into always comes down whole (settings.rs)
    topple_buildings: bool,
    goto_open: bool,
    goto_text: String,
    goto_rx: Option<std::sync::mpsc::Receiver<Option<(f64, f64, f64, String)>>>,
    status: String,
    status_until: f64,
    shots: Vec<(f64, PathBuf, Option<(f64, f64, f64)>)>,
    crash_at: Option<f64>,
    boost_at: Option<f64>,
    stage_at: Option<f64>,
    engines: Option<usize>,
    hold: f64,
    gilrs: Option<gilrs::Gilrs>,
    pad: Controls,
    /// the gamepad last used (the one whose sticks fly the aircraft)
    pad_id: Option<gilrs::GamepadId>,
    /// pads already given the Xbox layout (so a pad it does not take on is not reopened forever)
    pad_fixed: Vec<[u8; 16]>,
    /// the autopilot (autopilot.rs), and when it last let go (its warning)
    ap: autopilot::Autopilot,
    ap_off_t: f64,
    ap_script: Vec<(f64, String)>,
    /// gamepad input for the menus: key events for the next UI pass, a focus
    /// move out of a text box, a d-pad direction held (and when it repeats),
    /// and a request to pick out the screen's first choice
    pad_events: Vec<egui::Event>,
    pad_move: Option<egui::FocusDirection>,
    /// moving on past text boxes: direction, passes since the last move, moves made
    pad_skip: Option<(egui::FocusDirection, u8, u8)>,
    pad_rep: Option<(egui::Key, f64)>,
    pad_home: bool,
    /// Y (North) held on the gamepad, and whether it has been used with
    /// another button since: a Y chord arms or locks; Y alone fires.
    pad_y: bool,
    pad_y_combo: bool,
    /// the pad's flying actions held now, and the key each holds
    pad_held: Vec<(&'static str, Option<KeyCode>)>,
    /// a word from binding the pad (what lost its binding), until when
    pad_note: (String, f64),
    /// the pad was used last, not the mouse: the middle of the cockpit view
    /// picks out the controls (see `pad_aim_at`)
    pad_aim: bool,
    /// the pad's sticks as read now, for the dead zone boxes in Settings (None: no pad)
    pad_live: Option<padmap::Live>,
    /// the camera look the view starts out at (right stick click goes back to it)
    cam_home: (f64, f64),
    rng: crash::Rng,
    audio: Option<audio::Audio>,
    record: Option<(PathBuf, f64, f64)>,
    rec_audio: Vec<[f32; 2]>,
    rec_n: u32,
    rec_t0: Instant,
    /// sound on (U), and its volume (0..1)
    sound_on: bool,
    volume: f32,
    /// snow lying on the ground (snow.rs, the Graphics settings)
    snow: snow::Mode,
    /// the flight panel (Tab), beside the running game, and the page it shows
    info_open: bool,
    /// the flight menu, a ring of icons on tab.
    radial: flightui::Radial,
    info_page: menu::Page,
    /// the pause menu (Esc): open, the page it shows, and a scripted opening
    pause_open: bool,
    pause_page: menu::Page,
    /// the flight under way, the logbook, and whether this run may write it
    flight: Option<flightlog::Tracker>,
    logbook: flightlog::Logbook,
    logbook_saves: bool,
    pause_at: Option<(menu::Page, Option<settings_ui::Section>, f64)>,
    /// when to open the flight menu, for scripted screenshots.
    radial_at: Option<f64>,
    /// the section the settings screen shows (settings_ui.rs)
    settings_sec: settings_ui::Section,
    /// mouse-look speed (1 = default)
    look_sens: f64,
    /// Settings › Camera: looking around turns the other way in the cockpit, outside (horizontal / vertical).
    invert_look_cockpit: bool,
    invert_look_outside_x: bool,
    invert_look_outside_y: bool,
    /// the pilot's head in the cockpit view, the cockpit's vertical field of
    /// view (deg), and whether the head was in use last frame
    head: cockpit::Head,
    cockpit_fov: f32,
    /// the instruments' own state: lags, gyros, the compass card
    gauges: gauges::Gauges,
    /// water on the windows (rain.rs)
    glass: rain::Glass,
    /// lightning, and the rain falling around the camera (storm.rs)
    lightning: storm::Lightning,
    rain_field: storm::RainField,
    /// rain falling where the camera is, 0..1, and how much of it is hail
    cam_rain: f32,
    cam_hail: f32,
    /// snowflakes around the camera (snow.rs, precip.wgsl): their field, how
    /// much of what falls at the camera is snow, the blowing snow there, and
    /// the fresh snow settling where it snows on the ground
    snow_field: storm::RainField,
    cam_snow: f32,
    cam_blow: f32,
    fresh: snow::Fresh,
    fresh_line: f32,
    /// how far the eye has opened up to the gloom under a storm, 0..1
    storm_adapt: f32,
    /// how far the eye has closed down to the city lights around at night, 0..1
    city_adapt: f32,
    /// where the night lights around were last measured, and how bright they were
    city_probe: Option<(DVec3, f32)>,
    head_live: bool,
    /// the cockpit warnings sounding, and when each latched one last tripped
    /// (pull up, too low, sink rate, gear) so they do not flicker
    warn: audio::Warn,
    warn_t: [f64; 4],
    /// for the sound: slow-following load factor (jolts stand out from it),
    /// when the last jolt was, last frame's ground contact, vertical speed,
    /// gear and flaps, and metal scraping along the ground this frame
    g_slow: f64,
    last_bump: f64,
    was_ground: bool,
    prev_vs: f64,
    prev_gear: f64,
    prev_flaps: f64,
    quit: bool,
    mode: Mode,
    menu: menu::Menu,
    menu_go: Option<f64>,
    heading: f64,
    destr: destruct::Destruction,
    /// parts that have come off the aircraft and are falling
    sheds: damage::Sheds,
    /// origin of the particle system's local frame, and how far it moved this frame
    fx_anchor: DVec3,
    fx_shift: DVec3,
    smoke_acc: f32,
    /// things that just broke, to flash on the HUD: text, when
    flash: Vec<(String, f64)>,
    /// the airport notice and when it last changed.
    airport_note: (String, f64),
    /// a weapon's fireball for the renderer's light: position, power, when
    weapon_flash: Option<(DVec3, f32, f64)>,
    /// aircraft shot down and still falling: they burn on the way down and
    /// blow up again where they hit the ground
    kills: Vec<Kill>,
    damage_spec: Option<String>,
    /// group wreck meshes have been cut down and need restoring on respawn
    wreck_dirty: bool,
    /// how long the aircraft has lain wrecked (unburnt), and when that was last looked at
    wreck_t: f64,
    wreck_seen: f64,
    start_rx: Option<std::sync::mpsc::Receiver<(f64, f64, f64)>>,
    /// the loading screen, from the start screen into the air (see `Loading`)
    loading: Option<Loading>,
    name_rx: Option<std::sync::mpsc::Receiver<String>>,
    last_click: f64,
    n_cars: usize,
    /// hand-made scenery objects drawn last frame (for the credits)
    n_scenery: usize,
    skyglow: skyglow::SkyGlow,
    /// today's real snow cover around the camera (snowmap.rs)
    snowmap: snowmap::SnowMap,
    /// dynamic snow tracks and crash trails carved into lying snow (snowtrack.rs)
    snowtrack: snowtrack::SnowTrack,
    /// birds around the camera (birds.rs), and the setting for this run if given
    birds: birds::Birds,
    birds_level: Option<graphics::Level>,
    /// adaptive-LOD contrail simulation and reusable GPU draw buffer (contrails.rs)
    contrails: contrails::Contrails,
    contrail_draw: contrails::DrawData,
    /// bird instances drawn last frame (the fps log)
    bird_insts: usize,
    /// the flown aircraft's shape for bird strikes, and which type it is of
    bird_shape: (&'static str, birds::Shape),
    /// an airport start being prepared: (airport, the airfield if it could be put together, runway, on approach, keep the local time)
    airport_rx: Option<std::sync::mpsc::Receiver<(u32, Option<Arc<airports::Geo>>, Option<String>, bool, bool)>>,
    /// an airport start asked for on the command line
    pending_airport: Option<(u32, Option<String>, bool)>,
    /// the settings as last saved, the language picked in the game (None: the
    /// system's), and when they were last compared with the live ones
    prefs: settings::Settings,
    /// where the last flight started.
    last_start: Option<settings::LastStart>,
    prefs_lang: Option<i18n::Lang>,
    prefs_t: f64,
    /// real air traffic around the player, and the last frame's camera
    /// (view-projection, position) to put labels on it
    live: live::Live,
    live_view: Option<(Mat4, DVec3)>,
    /// multiplayer (mp.rs): switched on, the relay and our name (settings),
    /// the connection while flying, and the other players this frame
    mp_on: bool,
    /// multiplayer is on in the settings but kept off for this run (scripted
    /// or offline): saved as on all the same, until changed by hand
    mp_held: bool,
    mp_server: String,
    mp_name: String,
    mp: Option<mp::Mp>,
    mp_players: Vec<mp::Remote>,
    /// on the start screen: the network's players for the globe (coarse),
    /// how many fly in all, and whether the relay sends a world map at all
    mp_world: Vec<mp_proto::Spot>,
    mp_world_total: u32,
    mp_world_known: bool,
    /// the network's servers (started when first wanted: settings, or "auto")
    mp_dir: Option<mp::Directory>,
    /// the server is typed in (not picked from the list)
    mp_custom: bool,
    /// where the server list is read from, in turn (--mp-list first)
    mp_list: Vec<String>,
    /// the near ones' stand-in aircraft, for posing their models (by player id)
    mp_puppets: std::collections::HashMap<u32, Plane>,
    /// --mp-mirror (see Args)
    mp_mirror: bool,
    /// real weather (sets `wx` while `wx.live` is on)
    livewx: livewx::LiveWx,
    /// the ground along the wind line: ridge lift, lee waves and rotors (orographic.rs)
    oro: orographic::Orography,
    /// other aircraft's wake vortices (wake.rs)
    wakes: wake::Wakes,
    /// the real clouds on the start screen's globe
    gclouds: globeclouds::GlobeClouds,
    /// the start time has been set by hand (the menu's Time sliders, or
    /// --time/--day): the globe then shows that time instead of the real one
    menu_time_set: bool,
    /// the menu's local time and date as last seen (to notice them being moved)
    menu_time_seen: (f64, f64),
    /// joysticks, pedals and moved keys, and what was last saved of them
    binds: bind::Bindings,
    binds_stamp: String,
    /// random failures: on, the mean interval (minutes), when the next one comes
    fail_random: bool,
    fail_mean: f64,
    fail_next: Option<f64>,
    /// NAV 1, the ADF, the OBS and the altimeter setting
    radios: nav::Radios,
    /// waiting on the ground for the terrain to load: where, and until when at most
    hold_spot: Option<(f64, f64, f64)>,
    /// the AI copilot's tool calls, and the screenshots waiting for a frame
    mcp_calls: Option<crossbeam_channel::Receiver<mcp::rpc::Call>>,
    mcp_shots: Vec<copilot::PendingShot>,
    mcp_shot_n: u64,
    /// the copilot's throttle setting, held until the pilot moves the throttle
    mcp_throttle: Option<f64>,
    /// the bound throttle axis's last reading, to notice the pilot moving it
    throttle_axis: Option<f64>,
    /// --bench: the frame times being gathered for the launcher's graphics test
    bench: Option<Bench>,
}

/// The launcher's graphics test (`--bench SECONDS`): after a settling time
/// for the scenery to stream in, every frame's time is kept for SECONDS of
/// real time; then the figures go to stdout as one line
/// `@fable-bench {json}` and the game quits.
struct Bench {
    secs: f64,
    /// when the frames counted start (real seconds since the first frame)
    from: f64,
    clock: Instant,
    times: Vec<f64>,
}

impl Bench {
    /// Seconds given to the shaders, the terrain and the city to load first
    /// (the aircraft's own pipelines compile about 15 s in, see render.rs).
    const SETTLE: f64 = 16.0;

    fn new(secs: f64) -> Self {
        Bench { secs, from: Self::SETTLE, clock: Instant::now(), times: Vec::new() }
    }

    /// Counts a frame; true once the test is over.
    fn frame(&mut self, dt: f64, gpu: &str, size: (u32, u32), gfx: &graphics::Graphics) -> bool {
        let t = self.clock.elapsed().as_secs_f64();
        if t < self.from {
            return false;
        }
        self.times.push(dt);
        if t < self.from + self.secs {
            return false;
        }
        let mut v = self.times.clone();
        v.sort_by(|a, b| b.total_cmp(a));
        let n = v.len().max(1) as f64;
        let total: f64 = v.iter().sum();
        // the frame rate the slowest 1% and 0.1% of frames stay above (the
        // 99th and 99.9th percentile frame times): steady running, where a
        // mean of the slowest frames would be all the odd loading hitch
        let low = |share: f64| {
            let k = ((n * share) as usize).min(v.len().saturating_sub(1));
            v.get(k).map_or(0.0, |t| if *t > 0.0 { 1.0 / t } else { 0.0 })
        };
        let j = serde_json::json!({
            "fps": if total > 0.0 { n / total } else { 0.0 },
            "low1": low(0.01),
            "low01": low(0.001),
            "worst_ms": v.first().copied().unwrap_or(0.0) * 1000.0,
            // frames over 100 ms: the stutters a pilot notices
            "hitches": v.iter().filter(|t| **t > 0.1).count(),
            "frames": v.len(),
            "secs": total,
            "gpu": gpu,
            "width": size.0,
            "height": size.1,
            "preset": gfx.matches().map(graphics::Preset::name).unwrap_or("Custom"),
            "scale": gfx.render_scale(),
        });
        println!("@fable-bench {j}");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        true
    }
}


/// Right-handed view matrix looking along `dir`.
fn look_to(eye: Vec3, dir: Vec3, up: Vec3) -> Mat4 {
    let f = dir.normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols(s.x_axis4(u, f, 0), s.x_axis4(u, f, 1), s.x_axis4(u, f, 2), glam::Vec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0))
}
trait Axis4 {
    fn x_axis4(self, u: Vec3, f: Vec3, i: usize) -> glam::Vec4;
}
impl Axis4 for Vec3 {
    fn x_axis4(self, u: Vec3, f: Vec3, i: usize) -> glam::Vec4 {
        glam::Vec4::new(self[i], u[i], -f[i], 0.0)
    }
}
/// Infinite far plane, reversed depth (1 at the near plane, 0 at infinity).
fn perspective_rev(fov_y: f32, aspect: f32, near: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    Mat4::from_cols_array(&[f / aspect, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, near, 0.0])
}
fn ortho(l: f32, r: f32, b: f32, t: f32, n: f32, f: f32) -> Mat4 {
    Mat4::from_cols_array(&[2.0 / (r - l), 0.0, 0.0, 0.0, 0.0, 2.0 / (t - b), 0.0, 0.0, 0.0, 0.0, -1.0 / (f - n), 0.0, -(r + l) / (r - l), -(t + b) / (t - b), -n / (f - n), 1.0])
}

fn main() -> anyhow::Result<()> {
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("fablegame {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    progress::step("boot");
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,fablegame=info")).init();
    // A JSBSim aircraft compared with one of ours (see jsbsim.rs), and out.
    let argv: Vec<String> = std::env::args().collect();
    if let Some(i) = argv.iter().position(|a| a == "--jsbsim-compare") {
        std::process::exit(jsbsim_compare(argv.get(i + 1), argv.iter().position(|a| a == "--aircraft").and_then(|k| argv.get(k + 1))));
    }
    if let Some(i) = argv.iter().position(|a| a == "--fg-survey") {
        std::process::exit(hangar::survey(argv.get(i + 1).map(|s| s.as_str())));
    }
    if let Some(i) = argv.iter().position(|a| a == "--jsbsim-fit") {
        std::process::exit(hangar::fit_report(&argv[i + 1..]));
    }
    if let Some(i) = argv.iter().position(|a| a == "--fly-check") {
        std::process::exit(fly_check(argv.get(i + 1)));
    }
    if argv.iter().any(|a| a == "--fsr4-install") {
        // AMD's FSR 4 runtime, as the graphics settings' button fetches it (fsr4.rs).
        let last = std::cell::Cell::new(0u64);
        let r = fsr4::download(&|done, total| {
            if done >= last.get() + (4 << 20) || done == total {
                last.set(done);
                eprint!("\rFSR {} runtime: {} / {}", fsr4::VERSION, bytes(done), bytes(total));
            }
        });
        eprintln!();
        match r.and_then(|_| fsr4::verify()) {
            Ok(dir) => println!("installed and verified in {}", dir.display()),
            Err(e) => {
                eprintln!("failed: {e}");
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }
    if let Some(i) = argv.iter().position(|a| a == "--fg-install") {
        std::process::exit(hangar::install_report(&argv[i + 1..]));
    }
    // Downloaded JSBSim aircraft join the list before `--aircraft` is read.
    hangar::load_installed();
    let args = parse_args();
    if args.find_storms {
        let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".")).join("fablegame");
        match livewx::find_storms(&fetch::Fetcher::new(cache, false)) {
            Ok(s) if s.is_empty() => println!("no strong storms on radar right now"),
            Ok(s) => {
                println!("strongest radar echoes now (fly there with --lat --lon --live-weather):");
                for (lat, lon, z) in s {
                    println!("  --lat {lat:.2} --lon {lon:.2}   {z:.0} dBZ");
                }
            }
            Err(e) => println!("weather radar: {e}"),
        }
        return Ok(());
    }
    i18n::reload();
    i18n::set_lang(args.lang.or(settings::Settings::load().lang).unwrap_or_else(i18n::Lang::detect));
    // gamemode while we fly, if installed.
    let _gamemode = gamemode::GameMode::request();
    let el = EventLoop::new()?;
    el.set_control_flow(ControlFlow::Poll);
    el.run_app(&mut App { args, game: None, frame_pacer: frame_pacer::FramePacer::default() })?;
    Ok(())
}


const REC_FPS: f64 = 30.0;
/// Airborne starts: 107 kt, about a 172's cruise down low.

/// 16-bit stereo WAV.
fn write_wav(path: &std::path::Path, x: &[[f32; 2]], rate: u32) {
    let data = (x.len() * 4) as u32;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    for v in [16u32.to_le_bytes(), 0x0002_0001u32.to_le_bytes(), rate.to_le_bytes(), (rate * 4).to_le_bytes(), 0x0010_0004u32.to_le_bytes()] {
        w.extend_from_slice(&v);
    }
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    for s in x {
        for c in s {
            w.extend_from_slice(&((c.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
    }
    if let Err(e) = std::fs::write(path, w) {
        log::error!("writing {}: {e}", path.display());
    }
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The xpad driver's Xbox layout, in SDL's game controller mapping format.
const XBOX_LAYOUT: &str = "a:b0,b:b1,back:b6,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b8,leftshoulder:b4,leftstick:b9,lefttrigger:a2,leftx:a0,lefty:a1,rightshoulder:b5,rightstick:b10,righttrigger:a5,rightx:a3,righty:a4,start:b7,x:b2,y:b3,platform:Linux,";

/// A pad's sticks and triggers now, before any dead zone (`padmap::Live`).
fn pad_live(pad: &gilrs::Gamepad) -> padmap::Live {
    use gilrs::{Axis, Button};
    // (a pad left without a layout has its triggers as bare Z axes, -1 at rest)
    let trig = |b: Button, z: Axis| match (pad.button_data(b), pad.axis_data(z)) {
        (Some(d), _) => d.value(),
        (None, Some(a)) => ((a.value() + 1.0) * 0.5).clamp(0.0, 1.0),
        _ => 0.0,
    };
    padmap::Live {
        ls: [pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY)],
        rs: [pad.value(Axis::RightStickX), pad.value(Axis::RightStickY)],
        lt: trig(Button::LeftTrigger2, Axis::LeftZ),
        rt: trig(Button::RightTrigger2, Axis::RightZ),
    }
}

/// An Xbox pad gilrs found no mapping for: a firmware newer than its copy of
/// the SDL controller database, say. Left so, its triggers show up as bare
/// axes and X and Y trade places (xpad reports X as BTN_NORTH).
fn needs_xbox_layout(p: &gilrs::Gamepad) -> bool {
    let name = p.os_name().to_lowercase();
    cfg!(target_os = "linux")
        && p.mapping_source() != gilrs::MappingSource::SdlMappings
        && (p.vendor_id() == Some(0x045e) || name.contains("xbox") || name.contains("x-box"))
}

/// The gamepads, with any Xbox pad that has no mapping given the standard Xbox one.
fn open_gamepads() -> Option<gilrs::Gilrs> {
    // (scripted test runs: a pad someone is holding would steer the aircraft)
    if std::env::var_os("FABLE_NO_PAD").is_some() {
        return None;
    }
    let g = gilrs::Gilrs::new().map_err(|e| log::warn!("gamepads: {e}")).ok()?;
    let extra: String = g
        .gamepads()
        .filter(|(_, p)| needs_xbox_layout(p))
        .map(|(_, p)| {
            let guid: String = p.uuid().iter().map(|b| format!("{b:02x}")).collect();
            log::info!("gamepad: {} ({guid}) has no mapping; using the Xbox layout", p.os_name());
            format!("{guid},{},{XBOX_LAYOUT}\n", p.os_name().replace(',', " "))
        })
        .collect();
    if extra.is_empty() {
        return Some(g);
    }
    drop(g);
    let g = gilrs::GilrsBuilder::new().add_mappings(&extra).build().map_err(|e| log::warn!("gamepads: {e}")).ok()?;
    for (_, p) in g.gamepads() {
        log::info!("gamepad: {} mapping {:?}", p.os_name(), p.mapping_source());
    }
    Some(g)
}

/// A button that steps once when pressed, then repeats while held
/// (after 0.4 s, ten steps a second, speeding to twenty-five after 2 s).
fn hold_repeat(r: &egui::Response) -> bool {
    let id = r.id.with("hold_repeat");
    // Keep holding while the primary button stays down over the button —
    // egui's own press tracking drops out after a long press.
    let held_on = r.ctx.input(|i| {
        i.pointer.primary_down() && i.pointer.interact_pos().is_some_and(|p| r.rect.contains(p))
    });
    let started = r.ctx.data(|d| d.get_temp::<(f64, u32)>(id).is_some());
    if !(r.is_pointer_button_down_on() || (started && held_on)) {
        r.ctx.data_mut(|d| d.remove::<(f64, u32)>(id));
        return false;
    }
    r.ctx.request_repaint();
    let t = r.ctx.input(|i| i.time);
    let (start, fired) = r.ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || (t, 0u32)));
    let held = t - start;
    let due = if held < 0.4 {
        1
    } else if held < 2.0 {
        1 + ((held - 0.4) / 0.1) as u32
    } else {
        17 + ((held - 2.0) / 0.04) as u32
    };
    if due > fired {
        r.ctx.data_mut(|d| d.insert_temp(id, (start, fired + 1)));
        true
    } else {
        false
    }
}

/// An aircraft shot down, on its way to the ground.
struct Kill {
    key: weapons::TargetKey,
    pos: DVec3,
    vel: DVec3,
    /// its wingspan (m), which sizes the fire and the explosions
    span: f32,
    /// seconds since the hit, and the trail particles owed
    t: f64,
    acc: f64,
}

/// The top three rows of a piece's pose, for the shader (render.rs `DrawObj::pose`).
fn pose_rows(m: &Mat4) -> [[f32; 4]; 3] {
    [m.row(0).to_array(), m.row(1).to_array(), m.row(2).to_array()]
}
