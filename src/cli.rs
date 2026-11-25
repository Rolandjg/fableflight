//! The command line: the options, their parsing and the usage line.

use crate::*;

/// A scripted mouse event (--click, --wheel), for screenshots and checks:
/// when, where the mouse goes first, and what it does (0 button up, 1 down,
/// 100 + n the wheel turned n notches).
#[derive(Clone, Copy, Debug)]
pub(crate) struct MouseScript {
    pub(crate) t: f64,
    pub(crate) at: Option<(f64, f64)>,
    pub(crate) ev: f64,
}

pub(crate) struct Args {
    pub(crate) lat: f64,
    pub(crate) lon: f64,
    pub(crate) alt: Option<f64>,
    /// starting height above the ground, instead of `alt` above sea level
    pub(crate) agl: Option<f64>,
    pub(crate) heading: f64,
    pub(crate) place: Option<String>,
    /// a route to start with.
    pub(crate) route: Option<String>,
    pub(crate) max_zoom: u8,
    pub(crate) hour: f64,
    pub(crate) clouds: f32,
    /// cumulonimbus storm cell amount 0..1
    pub(crate) storms: Option<f32>,
    /// storm cells placed by hand: distance (km), compass bearing (deg), tower radius (km)
    pub(crate) storm_at: Vec<(f64, f64, f64, Option<weather::CellKind>)>,
    /// --birds off|low|medium|high: birds for this run (birds.rs)
    pub(crate) birds: Option<graphics::Level>,
    /// --flock KIND,KM[,BEARING[,AGL]]: a flock that far from the start (repeatable)
    pub(crate) flocks: Vec<(birds::Kind, f64, f64, Option<f64>)>,
    /// --squall-at: (km, bearing, length km) of squall lines
    pub(crate) squall_at: Vec<(f64, f64, f64)>,
    /// km,bearing: where the rain deck's area is centred, from the spawn
    pub(crate) rain_at: Option<(f64, f64)>,
    pub(crate) time_rate: Option<f64>,
    pub(crate) alto: Option<f32>,
    /// wind in knots and the direction it blows from, haze, day of year; open the F3 panel
    pub(crate) wind: Option<(f64, f64)>,
    pub(crate) haze: Option<f32>,
    pub(crate) day: Option<f64>,
    pub(crate) weather_panel: bool,
    /// real weather on for this run (else the saved setting, off when the command line sets the weather)
    pub(crate) live_weather: bool,
    /// print where it is storming now (weather radar) and quit
    pub(crate) find_storms: bool,
    pub(crate) vsync: bool,
    pub(crate) taa: bool,
    /// graphics preset and render resolution for this run (not saved; on a
    /// GPU without memory of its own the preset defaults to
    /// `graphics::integrated_preset` until the graphics settings are saved)
    pub(crate) graphics: Option<graphics::Preset>,
    pub(crate) storm_quality: Option<graphics::Level>,
    /// --dynamic FPS: dynamic quality on for this run, aiming for FPS (0: the display's)
    pub(crate) dynamic: Option<u16>,
    pub(crate) contrails: Option<bool>,
    pub(crate) render_scale: Option<f32>,
    pub(crate) upscaler: Option<graphics::Upscaler>,
    /// log the frame rate once a second (average and slowest frame)
    pub(crate) fps_log: bool,
    /// --bench SECONDS: the launcher's graphics test (see `Bench`)
    pub(crate) bench: Option<f64>,
    /// draw without showing anything (benchmarks and scripted shots with the screen off)
    pub(crate) headless: bool,
    pub(crate) minimap_plan: Option<bool>,
    pub(crate) minimap_heading_up: Option<bool>,
    /// `--full-map`: start with the full-size map open (scripted screenshots)
    pub(crate) full_map: bool,
    pub(crate) offline: bool,
    /// --snow auto|off|always: snow on the ground for this run (snow.rs)
    pub(crate) snow: Option<snow::Mode>,
    pub(crate) shots: Vec<(f64, PathBuf, Option<(f64, f64, f64)>)>,
    /// record frames + audio to a directory: (dir, start, seconds), fixed 30 fps steps
    pub(crate) record: Option<(PathBuf, f64, f64)>,
    pub(crate) crash_at: Option<f64>,
    /// from this many seconds on, full throttle with the boost key held (the afterburners), for scripted screenshots and recordings
    pub(crate) boost_at: Option<f64>,
    /// --ap mode@secs: an autopilot button pressed then (ap, hdg, nav, apr, alt, vs), for scripted tests
    pub(crate) ap_script: Vec<(f64, String)>,
    /// a rocket drops its booster after this many seconds, likewise
    pub(crate) stage_at: Option<f64>,
    /// a rocket burns this engine group (0 all) once it is let go, or starts as the ship alone coming back
    pub(crate) engines: Option<usize>,
    pub(crate) ship: bool,
    /// game time stands still for this many wall-clock seconds at the start (tiles stream in)
    pub(crate) hold: f64,
    pub(crate) size: (u32, u32),
    /// forced window or full screen.
    pub(crate) windowed: Option<bool>,
    pub(crate) imagery: String,
    /// Google Maps API key (else GOOGLE_MAPS_API_KEY, else the saved one)
    pub(crate) google_key: Option<String>,
    /// switch Google imagery on or off for good from the command line
    pub(crate) google: Option<bool>,
    /// and Google's photorealistic 3D tiles
    pub(crate) photoreal: Option<bool>,
    /// how much of the 3D tiles to stream
    pub(crate) photo_detail: Option<gmaps::Detail>,
    pub(crate) cam: Option<(f64, f64, f64)>,
    /// for scripted screenshots: the mouse at (x, y) window pixels, and the
    /// left button pressed and released at these times (s)
    pub(crate) cursor: Option<(f64, f64)>,
    pub(crate) clicks: Vec<MouseScript>,
    pub(crate) start_view: Option<u8>,
    /// --mp server[,name]: multiplayer on, through this relay
    pub(crate) mp: Option<(String, Option<String>)>,
    /// --mp-list URL: read the server list from there (testing a relay)
    pub(crate) mp_list: Option<String>,
    /// --mp-mirror: a copy of our own aircraft flies 25 m off the left wing as
    /// another player would be drawn (to check the drawing, no network)
    pub(crate) mp_mirror: bool,
    /// cockpit vertical field of view (deg)
    pub(crate) fov: Option<f32>,
    pub(crate) mute: bool,
    /// interface language (else the saved one, else the system's)
    pub(crate) lang: Option<i18n::Lang>,
    /// start on the start menu (default unless a place or scripted run is given):
    /// the main menu, or the screen `menu_screen` names (--menu-view and
    /// --airport open the globe, --settings and --hangar their screens)
    pub(crate) menu: Option<bool>,
    pub(crate) menu_view: Option<(f64, f64, f64)>,
    pub(crate) menu_go: Option<f64>,
    /// start already damaged (see `Damage::apply_spec`), for trying the damage model out
    pub(crate) damage: Option<String>,
    /// open the in-flight menu on this page after this many seconds, for scripted screenshots
    /// (page, settings section when the page is Settings, seconds)
    pub(crate) pause_menu: Option<(menu::Page, Option<settings_ui::Section>, f64)>,
    /// open the flight menu after this many seconds, for scripted screenshots.
    pub(crate) flight_menu: Option<f64>,
    /// `--settings SECTION`: the start screen opens with the settings screen on that section (scripted screenshots)
    pub(crate) settings: Option<settings_ui::Section>,
    /// `--hangar mine|download[:FOLDER]`: the start screen opens with the hangar on that page (scripted screenshots)
    pub(crate) hangar: Option<(hangar_ui::Tab, Option<String>)>,
    /// the start menu's screen to open on (`--menu-screen`)
    pub(crate) menu_screen: Option<menu::Screen>,
    /// a start position was given (--lat, --lon, --place): the globe opens on it
    pub(crate) pos_set: bool,
    /// start at an airport (ICAO or IATA code), on a runway or on approach
    pub(crate) airport: Option<String>,
    pub(crate) runway: Option<String>,
    pub(crate) approach: bool,
    /// which aircraft to fly (`aircraft::by_key`)
    pub(crate) aircraft: Option<String>,
    /// the flown type's paint scheme (an index of `aircraft::liveries`)
    pub(crate) livery: Option<usize>,
    /// start with the HUD hidden (H), for screenshots
    pub(crate) no_hud: bool,
    /// rain 0..1 (wets the windows), and how wet the glass starts (default: settled in that rain)
    pub(crate) rain: Option<f32>,
    pub(crate) wet: Option<f32>,
    /// airborne start speed (kt) instead of the aircraft's cruise
    pub(crate) speed: Option<f64>,
    /// a weather preset by name (`--weather storm`: thunderstorms all round)
    pub(crate) weather_preset: Option<String>,
    /// lightning strikes forced at given times, for screenshots
    pub(crate) strikes: Vec<storm::Forced>,
    /// fuel to start with (fraction of full tanks), and the engine off on the ground
    pub(crate) fuel: Option<f64>,
    pub(crate) cold: bool,
    /// --arcade: arcade mode for this run
    pub(crate) arcade: bool,
    /// the AI copilot's loopback control service (on unless --no-mcp)
    pub(crate) mcp: bool,
    pub(crate) mcp_port: u16,
}

pub(crate) const CLI_USAGE: &str = "fablegame [--version] [--menu | --no-menu] [--menu-screen main|fly|aircraft|settings] [--menu-view LAT,LON,ALT_M] [--hangar mine|download[:FOLDER]] [--place \"city or address\"] [--lat D --lon D] [--alt M | --agl M] [--heading DEG] [--speed KT] [--time HOURS] [--time-rate X] [--day 0..364] [--clouds 0..1] [--storms 0..1] [--alto 0..1] [--wind KT,FROM_DEG] [--haze 0.2..40] [--rain 0..1] [--rain-at KM,BEARING] [--wet 0..1] [--birds off|low|medium|high] [--flock gull|crow|starling|buzzard|goose,KM[,BEARING[,AGL_M]]] [--snow auto|off|always] [--weather [clear|fair|hazy|overcast|windy|storm]] [--storm-at KM,BEARING[,RADIUS_KM][,shower|thunder|severe|supercell]] [--squall-at KM,BEARING[,LENGTH_KM]] [--strike cg|ic,KM,BEARING@SECONDS] [--flight-menu SECONDS] [--pause-menu flight|places|radio|weather|game|settings[:SECTION]|controls|failures[@SECONDS]] [--settings general|graphics|sound|camera|controls|map] [--airport CODE [--runway 28L] [--approach]] [--route \"KSFO OAK 37.9,-122.1 KSMF\"] [--aircraft KEY] [--max-zoom 8..19] [--cockpit | --tail-view] [--fov DEG] [--windowed | --fullscreen] [--width PX --height PX] [--no-vsync] [--no-taa] [--graphics very-low|low|medium|high|ultra] [--storm-quality low|medium|high|ultra] [--contrails | --no-contrails] [--dynamic FPS] [--render-scale 0.35..2] [--upscaler bilinear|fsr1|fsr3|fsr4] [--fsr4-install] [--headless] [--minimap plan|satellite] [--minimap-up north|heading] [--full-map] [--fps-log] [--bench SECONDS] [--mute] [--lang CODE] [--fuel 0..1] [--cold] [--arcade] [--mcp | --no-mcp] [--mcp-port PORT] [--offline] [--imagery URL_TEMPLATE] [--google | --no-google] [--photoreal | --no-photoreal] [--photo-detail low|medium|high|ultra] [--no-pave] [--google-key KEY] [--hold SECONDS] [--no-scenery] [--livery N] [--still] [--boost-at SECONDS] [--stage-at SECONDS] [--engines GROUP] [--ship] [--damage ...]";

pub(crate) fn parse_args() -> Args {
    let v: Vec<String> = std::env::args().skip(1).collect();
    if v.iter().any(|s| s == "--help" || s == "-h") {
        println!("{CLI_USAGE}");
        std::process::exit(0);
    }
    match parse_args_from(&v) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\nRun with --help for usage.");
            std::process::exit(2);
        }
    }
}

pub(crate) fn parse_args_from(v: &[String]) -> Result<Args, String> {
    let mut a = Args {
        lat: 37.8085,
        lon: -122.4650,
        alt: None,
        agl: None,
        heading: 95.0,
        place: None,
        route: None,
        max_zoom: 17,
        hour: 16.6,
        clouds: 0.5,
        storms: None,
        storm_at: Vec::new(),
        birds: None,
        flocks: Vec::new(),
        squall_at: Vec::new(),
        rain_at: None,
        time_rate: None,
        alto: None,
        wind: None,
        haze: None,
        day: None,
        weather_panel: false,
        live_weather: false,
        find_storms: false,
        vsync: true,
        taa: true,
        graphics: None,
        storm_quality: None,
        dynamic: None,
        contrails: None,
        render_scale: None,
        upscaler: None,
        fps_log: false,
        bench: None,
        headless: false,
        minimap_plan: None,
        minimap_heading_up: None,
        full_map: false,
        offline: false,
        snow: None,
        shots: Vec::new(),
        record: None,
        crash_at: None,
        mp: None,
        mp_mirror: false,
        mp_list: None,
        cursor: None,
        clicks: Vec::new(),
        boost_at: None,
        ap_script: Vec::new(),
        stage_at: None,
        engines: None,
        ship: false,
        hold: 0.0,
        size: (1600, 900),
        windowed: None,
        imagery: "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}".into(),
        google_key: std::env::var("GOOGLE_MAPS_API_KEY").ok(),
        google: None,
        photoreal: None,
        photo_detail: None,
        cam: None,
        start_view: None,
        fov: None,
        mute: false,
        lang: None,
        menu: None,
        menu_view: None,
        menu_go: None,
        damage: None,
        pause_menu: None,
        flight_menu: None,
        settings: None,
        hangar: None,
        menu_screen: None,
        pos_set: false,
        airport: None,
        runway: None,
        approach: false,
        aircraft: None,
        livery: None,
        no_hud: false,
        rain: None,
        wet: None,
        speed: None,
        weather_preset: None,
        strikes: Vec::new(),
        fuel: None,
        cold: false,
        arcade: false,
        mcp: true,
        mcp_port: mcp::rpc::DEFAULT_PORT,
    };
    let mut i = 0;
    let req = |flag: &str, i: usize| -> Result<&str, String> {
        v.get(i + 1).map(|s| s.as_str()).ok_or_else(|| format!("{flag} requires a value"))
    };
    let req_num = |flag: &str, i: usize| -> Result<f64, String> {
        let s = req(flag, i)?;
        let x: f64 = s.parse().map_err(|_| format!("{flag}: invalid number '{s}'"))?;
        if !x.is_finite() {
            return Err(format!("{flag}: number must be finite ('{s}')"));
        }
        Ok(x)
    };
    let req_nums = |flag: &str, i: usize, min: usize, max: usize| -> Result<Vec<f64>, String> {
        let s = req(flag, i)?;
        let mut out = Vec::new();
        for part in s.split(',') {
            let x: f64 = part.parse().map_err(|_| format!("{flag}: invalid number '{part}' in '{s}'"))?;
            if !x.is_finite() {
                return Err(format!("{flag}: non-finite number '{part}' in '{s}'"));
            }
            out.push(x);
        }
        if out.len() < min || out.len() > max {
            return Err(format!("{flag}: expected {} value(s), got '{}'", if min == max { min.to_string() } else { format!("{min}..={max}") }, s));
        }
        Ok(out)
    };
    while i < v.len() {
        let mut used = 2;
        let flag = v[i].as_str();
        match flag {
            "--minimap" => {
                let s = req(flag, i)?;
                a.minimap_plan = Some(match s {
                    "plan" => true,
                    "satellite" => false,
                    _ => return Err(format!("{flag} takes plan or satellite, got '{s}'")),
                });
            }
            "--full-map" => {
                a.full_map = true;
                used = 1;
            }
            "--minimap-up" => {
                let s = req(flag, i)?;
                a.minimap_heading_up = Some(match s {
                    "north" => false,
                    "heading" => true,
                    _ => return Err(format!("{flag} takes north or heading, got '{s}'")),
                });
            }
            "--lat" => {
                a.lat = req_num(flag, i)?;
                if !(-90.0..=90.0).contains(&a.lat) {
                    return Err(format!("{flag}: latitude must be within -90..=90"));
                }
                a.pos_set = true;
                a.menu.get_or_insert(false);
            }
            "--lon" => {
                a.lon = req_num(flag, i)?;
                if !(-180.0..=180.0).contains(&a.lon) {
                    return Err(format!("{flag}: longitude must be within -180..=180"));
                }
                a.pos_set = true;
                a.menu.get_or_insert(false);
            }
            "--menu-go" => a.menu_go = Some(req_num(flag, i)?),
            "--menu-view" => {
                // lat,lon,altitude of the menu camera, for scripted screenshots
                let p = req_nums(flag, i, 3, 3)?;
                a.menu_view = Some((p[0], p[1], p[2]));
            }
            "--menu" => {
                a.menu = Some(true);
                used = 1;
            }
            "--menu-screen" => {
                // the start menu's screen: main (the default), fly (the globe), aircraft, settings
                let s = req(flag, i)?;
                a.menu_screen = Some(menu::Screen::parse(s).ok_or_else(|| format!("{flag}: expected main, fly, aircraft or settings, got '{s}'"))?);
                a.menu.get_or_insert(true);
            }
            "--no-menu" => {
                a.menu = Some(false);
                used = 1;
            }
            "--cockpit" | "--tail-view" => {
                a.start_view = Some(if v[i] == "--cockpit" { 1 } else { settings::TAIL_VIEW });
                used = 1;
            }
            "--fov" => {
                a.fov = Some(req_num(flag, i)? as f32);
                used = 2;
            }
            "--airport" => {
                a.airport = Some(req(flag, i)?.to_string());
                a.menu.get_or_insert(false);
            }
            "--runway" => a.runway = Some(req(flag, i)?.to_string()),
            "--livery" => a.livery = Some(req(flag, i)?.parse().map_err(|_| format!("bad --livery"))?),
            "--aircraft" => {
                let k = req(flag, i)?;
                if aircraft::by_key(k).is_none() {
                    return Err(format!("unknown aircraft '{k}' (try {})", aircraft::list().iter().map(|s| s.keys[0]).collect::<Vec<_>>().join(", ")));
                }
                a.aircraft = Some(k.to_string());
            }
            "--approach" => {
                a.approach = true;
                used = 1;
            }
            "--alt" => a.alt = Some(req_num(flag, i)?),
            "--agl" => a.agl = Some(req_num(flag, i)?),
            "--heading" => a.heading = req_num(flag, i)?,
            "--route" => a.route = Some(req(flag, i)?.to_string()),
            "--place" => {
                a.place = Some(req(flag, i)?.to_string());
                a.pos_set = true;
                a.menu.get_or_insert(false);
            }
            "--max-zoom" => a.max_zoom = req_num(flag, i)?.clamp(8.0, 19.0) as u8,
            "--time" => a.hour = req_num(flag, i)?,
            "--clouds" => a.clouds = req_num(flag, i)? as f32,
            "--storms" => a.storms = Some(req_num(flag, i)? as f32),
            "--birds" => {
                let s = req(flag, i)?;
                a.birds = Some(graphics::Level::parse(s).ok_or_else(|| format!("{flag}: invalid level '{s}'"))?);
            }
            "--flock" => {
                let s = req(flag, i)?;
                let (kind, values) = s.split_once(',').ok_or_else(|| format!("{flag}: expected kind,km[,bearing[,agl]], got '{s}'"))?;
                let kind = birds::Kind::parse(kind).ok_or_else(|| format!("{flag}: unknown bird kind '{kind}'"))?;
                let values: Vec<f64> = values.split(',').map(|part| {
                    part.parse::<f64>().ok().filter(|n| n.is_finite())
                        .ok_or_else(|| format!("{flag}: invalid number '{part}' in '{s}'"))
                }).collect::<Result<_, _>>()?;
                if values.len() > 3 {
                    return Err(format!("{flag}: expected kind,km[,bearing[,agl]], got '{s}'"));
                }
                // Without a bearing, place the flock ahead on the aircraft's path.
                a.flocks.push((kind, values[0], values.get(1).copied().unwrap_or(f64::NAN), values.get(2).copied()));
            }
            "--storm-at" => {
                // km,bearing[,radius km[,shower|thunder|severe|supercell]]: a convective cell that far
                // from the spawn, a mature cumulonimbus unless a kind says otherwise (repeatable)
                let s = req(flag, i)?;
                let f: Vec<&str> = s.split(',').collect();
                let p: Vec<f64> = f.iter().take(3).map_while(|x| x.parse::<f64>().ok().filter(|n| n.is_finite())).collect();
                let rest = &f[p.len()..];
                let kind = match rest {
                    [] => None,
                    [k] => Some(weather::CellKind::parse(k).ok_or_else(|| format!("{flag}: unknown storm kind '{k}' in '{s}'"))?),
                    _ => return Err(format!("{flag}: expected km,bearing[,radius][,kind], got '{s}'")),
                };
                if p.len() < 2 {
                    return Err(format!("{flag}: expected km,bearing[,radius][,kind], got '{s}'"));
                }
                let r = p.get(2).copied().unwrap_or(match kind {
                    Some(weather::CellKind::Shower) => 3.2,
                    Some(weather::CellKind::Thunder) => 7.5,
                    Some(weather::CellKind::Supercell) => weather::SUPER_R,
                    _ => 10.0,
                });
                a.storm_at.push((p[0], p[1], r, kind));
            }
            "--squall-at" => {
                // km,bearing[,length km]: a squall line that far from the spawn, lying
                // across that bearing (the wind aloft blows from there unless --wind says)
                let p = req_nums(flag, i, 2, 3)?;
                a.squall_at.push((p[0], p[1], p.get(2).copied().unwrap_or(100.0)));
            }
            "--rain-at" => {
                // km,bearing: centre the rain deck's area (--rain) that far from the spawn
                let p = req_nums(flag, i, 2, 2)?;
                a.rain_at = Some((p[0], p[1]));
            }
            "--time-rate" => a.time_rate = Some(req_num(flag, i)?),
            "--alto" => a.alto = Some(req_num(flag, i)? as f32),
            "--wind" => {
                // knots,from-degrees
                let p = req_nums(flag, i, 2, 2)?;
                a.wind = Some((p[0], p[1]));
            }
            "--haze" => a.haze = Some(req_num(flag, i)? as f32),
            "--day" => a.day = Some(req_num(flag, i)?),
            "--weather" => {
                // a preset name (clear, fair, hazy, overcast, windy, storm), or on its own: open the weather panel
                match v.get(i + 1).filter(|s| !s.starts_with("--")) {
                    Some(p) => a.weather_preset = Some(p.clone()),
                    None => {
                        a.weather_panel = true;
                        used = 1;
                    }
                }
            }
            "--strike" => {
                // cg|ic,km,bearing@seconds: a lightning strike that far from the aircraft at that game time
                let s = req(flag, i)?;
                let (head, at_s) = s.split_once('@').ok_or_else(|| format!("{flag}: expected cg|ic,km[,bearing]@seconds, got '{s}'"))?;
                let at: f64 = at_s.parse().map_err(|_| format!("{flag}: invalid time '{at_s}' in '{s}'"))?;
                let p: Vec<&str> = head.split(',').collect();
                let kind = match p.first().copied().unwrap_or("") {
                    k if k.eq_ignore_ascii_case("ic") => storm::Kind::Cloud,
                    k if k.eq_ignore_ascii_case("cg") => storm::Kind::Ground,
                    k => return Err(format!("{flag}: expected cg or ic, got '{k}' in '{s}'")),
                };
                let km: f64 = p.get(1).ok_or_else(|| format!("{flag}: missing distance in '{s}'"))?.parse().map_err(|_| format!("{flag}: invalid distance in '{s}'"))?;
                let bearing: f64 = match p.get(2) {
                    Some(b) => b.parse().map_err(|_| format!("{flag}: invalid bearing '{b}' in '{s}'"))?,
                    None => 0.0,
                };
                a.strikes.push(storm::Forced { at, kind, km, bearing });
            }
            "--imagery" => a.imagery = req(flag, i)?.to_string(),
            "--crash-at" => a.crash_at = Some(req_num(flag, i)?),
            "--boost-at" => a.boost_at = Some(req_num(flag, i)?),
            "--ap" => {
                let s = req(flag, i)?;
                let (m, t) = s.split_once('@').ok_or_else(|| format!("{flag}: expected mode@seconds, got '{s}'"))?;
                let t: f64 = t.parse().map_err(|_| format!("{flag}: invalid time '{t}' in '{s}'"))?;
                a.ap_script.push((t, m.to_ascii_lowercase()));
            }
            "--stage-at" => a.stage_at = Some(req_num(flag, i)?),
            "--engines" => {
                let k = req_num(flag, i)?;
                if k < 0.0 {
                    return Err(format!("{flag}: expected non-negative integer, got {k}"));
                }
                a.engines = Some(k as usize);
            }
            "--ship" => {
                a.ship = true;
                used = 1;
            }
            "--hold" => a.hold = req_num(flag, i)?,
            "--damage" => a.damage = Some(req(flag, i)?.to_string()),
            "--flight-menu" => a.flight_menu = Some(req_num(flag, i)?),
            "--pause-menu" => {
                // page[@seconds]: open the flight panel (Tab) or the pause menu (Esc) on that page, for scripted screenshots.
                // `settings:SECTION` (or a section's name alone, e.g. `graphics`) opens the settings screen there.
                let s = req(flag, i)?;
                let (spec, t) = match s.split_once('@') {
                    Some((sp, ts)) => (sp, ts.parse::<f64>().map_err(|_| format!("{flag}: invalid time '{ts}' in '{s}'"))?),
                    None => (s, 0.0),
                };
                let (pg, sec) = match spec.split_once(':') {
                    Some((p, sec_s)) => (p, Some(settings_ui::Section::parse(sec_s).ok_or_else(|| format!("{flag}: unknown settings section '{sec_s}'"))?)),
                    None => (spec, None),
                };
                let page = match (menu::Page::parse(pg), settings_ui::Section::parse(pg)) {
                    (Some(p), _) => (p, sec),
                    (None, Some(s)) => (menu::Page::Settings, Some(s)),
                    _ => return Err(format!("{flag}: unknown page '{pg}' in '{s}'")),
                };
                a.pause_menu = Some((page.0, page.1, t));
            }
            "--settings" => {
                // the start menu's settings screen, open on this section
                let s = req(flag, i)?;
                let sec = settings_ui::Section::parse(s).ok_or_else(|| format!("{flag}: unknown section '{s}'"))?;
                a.settings = Some(sec);
                a.menu.get_or_insert(true);
            }
            "--hangar" => {
                let f = req(flag, i)?;
                let (page, sel) = f.split_once(':').map(|(p, s)| (p, Some(s.to_string()))).unwrap_or((f, None));
                let tab = if page.starts_with('d') {
                    hangar_ui::Tab::Download
                } else if page.starts_with('m') {
                    hangar_ui::Tab::Mine
                } else {
                    return Err(format!("{flag}: expected mine or download[:FOLDER], got '{f}'"));
                };
                a.hangar = Some((tab, sel));
                a.menu.get_or_insert(true);
            }
            "--width" => {
                let w = req_num(flag, i)?;
                if w < 1.0 {
                    return Err(format!("{flag}: width must be positive"));
                }
                a.size.0 = w as u32;
                a.windowed.get_or_insert(true);
            }
            "--height" => {
                let h = req_num(flag, i)?;
                if h < 1.0 {
                    return Err(format!("{flag}: height must be positive"));
                }
                a.size.1 = h as u32;
                a.windowed.get_or_insert(true);
            }
            "--windowed" => {
                a.windowed = Some(true);
                used = 1;
            }
            "--fullscreen" => {
                a.windowed = Some(false);
                used = 1;
            }
            "--cam" => {
                // yaw,pitch,distance of the chase camera, for scripted screenshots
                let p = req_nums(flag, i, 3, 3)?;
                a.cam = Some((p[0], p[1], p[2]));
            }
            "--cursor" => {
                // x,y window pixels: the mouse, for scripted screenshots
                let p = req_nums(flag, i, 2, 2)?;
                a.cursor = Some((p[0], p[1]));
            }
            "--click" | "--wheel" => {
                // --click seconds[,held seconds[,x,y]]: a left click (at --cursor, or x,y)
                // --wheel seconds,notches[,x,y]: the mouse wheel turned
                let p = req_nums(flag, i, 1, 4)?;
                let at = (p.len() >= 4).then(|| (p[2], p[3]));
                let t = p[0];
                if flag == "--click" {
                    a.clicks.push(MouseScript { t, at, ev: 1.0 });
                    a.clicks.push(MouseScript { t: t + p.get(1).copied().unwrap_or(0.1), at: None, ev: 0.0 });
                } else {
                    a.clicks.push(MouseScript { t, at, ev: 100.0 + p.get(1).copied().unwrap_or(1.0) });
                }
                a.clicks.sort_by(|x, y| x.t.total_cmp(&y.t));
            }
            "--shot" => {
                // path@seconds[@yaw,pitch,distance]
                let s = req(flag, i)?;
                let f: Vec<&str> = s.split('@').collect();
                if f.len() < 2 || f.len() > 3 {
                    return Err(format!("{flag}: expected path@seconds[@yaw,pitch,distance], got '{s}'"));
                }
                let t: f64 = f[1].parse().map_err(|_| format!("{flag}: invalid time '{}' in '{s}'", f[1]))?;
                let cam = if let Some(cs) = f.get(2) {
                    let c: Vec<f64> = cs.split(',').map(|x| x.parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("{flag}: invalid camera '{cs}' in '{s}'"))?;
                    if c.len() != 3 {
                        return Err(format!("{flag}: camera needs yaw,pitch,distance, got '{cs}'"));
                    }
                    Some((c[0], c[1], c[2]))
                } else {
                    None
                };
                a.shots.push((t, PathBuf::from(f[0]), cam));
                a.menu.get_or_insert(false);
            }
            "--record" => {
                // dir@start@seconds: every frame steps 1/30 s; PNG frames + audio.wav
                let s = req(flag, i)?;
                let f: Vec<&str> = s.split('@').collect();
                if f.len() != 3 {
                    return Err(format!("{flag}: expected dir@start@seconds, got '{s}'"));
                }
                let start: f64 = f[1].parse().map_err(|_| format!("{flag}: invalid start '{}' in '{s}'", f[1]))?;
                let secs: f64 = f[2].parse().map_err(|_| format!("{flag}: invalid duration '{}' in '{s}'", f[2]))?;
                a.record = Some((PathBuf::from(f[0]), start, secs));
                a.menu.get_or_insert(false);
            }
            "--no-vsync" => {
                a.vsync = false;
                used = 1;
            }
            "--no-taa" => {
                a.taa = false;
                used = 1;
            }
            "--rain" => a.rain = Some(req_num(flag, i)? as f32),
            "--wet" => a.wet = Some(req_num(flag, i)? as f32),
            "--speed" => a.speed = Some(req_num(flag, i)?),
            "--graphics" => {
                let s = req(flag, i)?;
                a.graphics = Some(graphics::Preset::parse(s).ok_or_else(|| format!("{flag}: expected very-low, low, medium, high or ultra, got '{s}'"))?);
            }
            "--dynamic" => a.dynamic = Some(req_num(flag, i)?.clamp(0.0, 240.0) as u16),
            "--storm-quality" => {
                let s = req(flag, i)?;
                a.storm_quality = Some(graphics::Level::parse(s).filter(|l| *l != graphics::Level::Off).ok_or_else(|| format!("{flag}: expected low, medium, high or ultra, got '{s}'"))?);
            }
            "--contrails" => {
                a.contrails = Some(true);
                used = 1;
            }
            "--no-contrails" => {
                a.contrails = Some(false);
                used = 1;
            }
            "--render-scale" => a.render_scale = Some((req_num(flag, i)? as f32).clamp(graphics::SCALE_MIN, graphics::SCALE_MAX)),
            "--upscaler" => {
                let s = req(flag, i)?;
                a.upscaler = Some(graphics::Upscaler::parse(s).ok_or_else(|| format!("{flag}: expected bilinear, fsr1, fsr3 or fsr4, got '{s}'"))?);
            }
            "--headless" => {
                a.headless = true;
                used = 1;
            }
            "--live-weather" => {
                a.live_weather = true;
                used = 1;
            }
            "--find-storms" => {
                a.find_storms = true;
                used = 1;
            }
            "--bench" => a.bench = Some(req_num(flag, i)?.clamp(3.0, 120.0)),
            "--fps-log" => {
                a.fps_log = true;
                used = 1;
            }
            "--no-hud" => {
                a.no_hud = true;
                used = 1;
            }
            // Without the hand-made airport scenery: the airfields as the game builds them.
            "--no-scenery" => {
                scenery::DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
                used = 1;
            }
            "--still" => {
                // (read by `still_mode`): --shot pictures cut the aircraft out
                // on a transparent background, for the aircraft picker
                a.no_hud = true;
                used = 1;
            }
            "--fuel" => a.fuel = Some(req_num(flag, i)?.clamp(0.0, 1.0)),
            "--arcade" => {
                a.arcade = true;
                used = 1;
            }
            "--cold" => {
                a.cold = true;
                used = 1;
            }
            "--mcp" => {
                a.mcp = true;
                used = 1;
            }
            "--no-mcp" => {
                a.mcp = false;
                used = 1;
            }
            "--mcp-port" => {
                let s = req(flag, i)?;
                let p: u16 = s.parse().map_err(|_| format!("{flag}: invalid port '{s}'"))?;
                if p == 0 {
                    return Err(format!("{flag}: port must be 1..=65535"));
                }
                a.mcp_port = p;
            }
            "--photo-detail" => {
                let s = req(flag, i)?;
                a.photo_detail = Some(gmaps::Detail::parse(s).ok_or_else(|| format!("{flag} takes low, medium, high or ultra, got '{s}'"))?);
            }
            "--no-pave" => {
                pave::ON.store(false, std::sync::atomic::Ordering::Relaxed);
                used = 1;
            }
            "--mute" => {
                a.mute = true;
                used = 1;
            }
            "--lang" => {
                let s = req(flag, i)?;
                a.lang = Some(i18n::Lang::parse(s).ok_or_else(|| {
                    let codes = i18n::Lang::all().iter().map(|l| l.code()).collect::<Vec<_>>().join(", ");
                    format!("{flag} takes one of: {codes}, got '{s}'")
                })?);
            }
            "--mp" => {
                let s = req(flag, i)?;
                let (server, name) = match s.split_once(',') {
                    Some((a, b)) => (a.to_string(), Some(b.to_string())),
                    None => (s.to_string(), None),
                };
                a.mp = Some((server, name));
            }
            "--mp-list" => a.mp_list = Some(req(flag, i)?.to_string()),
            "--mp-mirror" => {
                a.mp_mirror = true;
                used = 1;
            }
            "--snow" => {
                let s = req(flag, i)?;
                a.snow = Some(snow::Mode::parse(s).ok_or_else(|| format!("{flag} takes auto, off or always, got '{s}'"))?);
            }
            "--offline" => {
                a.offline = true;
                used = 1;
            }
            "--google-key" => a.google_key = Some(req(flag, i)?.to_string()),
            "--google" => {
                a.google = Some(true);
                used = 1;
            }
            "--no-google" => {
                a.google = Some(false);
                used = 1;
            }
            "--photoreal" => {
                a.photoreal = Some(true);
                used = 1;
            }
            "--no-photoreal" => {
                a.photoreal = Some(false);
                used = 1;
            }
            other => {
                return Err(format!("unknown argument '{other}'"));
            }
        }
        i += used;
    }
    a.shots.sort_by(|x, y| x.0.total_cmp(&y.0));
    Ok(a)
}

/// Runs that stay off the network unless `--mp` asks: offline, and the
/// scripted ones (screenshots, recordings, benchmarks, headless checks),
/// so they neither show up to the other pilots nor depend on the relays.
pub(crate) fn mp_kept_off(a: &Args) -> bool {
    a.offline || a.bench.is_some() || !a.shots.is_empty() || std::env::args().any(|x| matches!(x.as_str(), "--headless" | "--record" | "--still"))
}

/// `--still`: screenshots for the aircraft picker, the aircraft alone on a
/// transparent background (post.wgsl `finish` keys it out by depth, so fly it
/// clear of the ground and anything else within 150 m), saved with alpha.
pub(crate) fn still_mode() -> bool {
    static S: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *S.get_or_init(|| std::env::args().any(|a| a == "--still"))
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn cli_rejects_bad_values_and_unknown_flags() {
        assert!(parse_args_from(&s(&["--lat", "abc"])).is_err());
        assert!(parse_args_from(&s(&["--lat"])).is_err());
        assert!(parse_args_from(&s(&["--lat", "95"])).is_err());
        assert!(parse_args_from(&s(&["--unknown-flag"])).is_err());
        assert!(parse_args_from(&s(&["--lang", "zz"])).is_err());
        assert!(parse_args_from(&s(&["--cam", "1,2"])).is_err());
        assert!(parse_args_from(&s(&["--mcp-port", "0"])).is_err());
    }

    #[test]
    fn cli_accepts_valid_flags() {
        let a = parse_args_from(&s(&["--lat", "37.6", "--lon", "-122.37", "--lang", "ko", "--weather", "--no-mcp"])).unwrap();
        assert!((a.lat - 37.6).abs() < 1e-9);
        assert!((a.lon + 122.37).abs() < 1e-9);
        assert_eq!(a.lang.map(|lang| lang.code()), Some("ko"));
        assert!(a.weather_panel);
        assert!(!a.mcp);
    }
}
