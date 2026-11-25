//! Building the game and its settings: the aircraft, the livery, the view, full screen, graphics and language.

use crate::*;

impl Game {
    pub(crate) fn new(window: Arc<Window>, args: &Args) -> anyhow::Result<Self> {
        let (prefs, fresh) = settings::Settings::load_new();
        if fresh && !std::env::args().any(|a| a == "--headless" || a == "--shot" || a == "--record" || a == "--bench") {
            prefs.save();
        }
        // Saved graphics settings; the command line overrides them for this run.
        let mut gfx = graphics::Graphics::load();
        if let Some(l) = args.graphics {
            gfx.apply(l);
        }
        if let Some(l) = args.storm_quality {
            gfx.storms = l;
        }
        if let Some(f) = args.dynamic {
            gfx.dynamic = true;
            gfx.dyn_fps = f;
        }
        if let Some(on) = args.contrails {
            gfx.contrails = on;
        }
        gfx.vsync &= args.vsync;
        if args.bench.is_some() {
            // the graphics test measures the GPU, not a frame limit
            gfx.fps_cap = 0;
        }
        gfx.taa &= args.taa && prefs.taa;
        // A first start (nothing saved, no preset asked for) leaves the MSAA
        // and the preset to the GPU found: lighter on integrated graphics.
        let first = !graphics::Graphics::saved() && args.graphics.is_none();
        let samples = std::env::var("FABLE_MSAA").ok().and_then(|v| v.parse().ok()).or((!first).then_some(gfx.msaa as u32));
        let r = pollster::block_on(Renderer::new(window.clone(), gfx.vsync, samples))?;
        graphics::set_integrated(r.integrated);
        if r.integrated && first {
            let p = graphics::integrated_preset(r.vendor);
            gfx.apply(p);
            if let Some(l) = args.storm_quality {
                gfx.storms = l;
            }
            if let Some(on) = args.contrails {
                gfx.contrails = on;
            }
            log::info!("integrated graphics: {} preset until the graphics settings are saved", p.name());
        }
        if let Some(u) = args.upscaler {
            gfx.upscaler = u;
        }
        if let Some(sc) = args.render_scale {
            gfx.scale = sc;
        }
        let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".")).join("fablegame");
        {
            let prune_dir = cache.clone();
            let _ = std::thread::Builder::new().name("cache-prune".into()).spawn(move || {
                fetch::prune_cache(&prune_dir, fetch::CACHE_MAX_BYTES);
            });
        }
        let mut r = r;
        r.headless = args.headless;
        // Scripted pictures show the aircraft from the first frame they take.
        r.wait_objs = !args.shots.is_empty() || args.record.is_some();
        // Cached scans load offline too; missing ones keep the procedural layer.
        r.load_ground_photos(fetch::Fetcher::new(cache.clone(), args.offline));
        let google_key = args.google_key.clone().or_else(|| std::env::var("FABLE_GOOGLE_KEY").ok().filter(|s| !s.is_empty()));
        let google = Arc::new(gmaps::Google::load(google_key));
        if let Some(on) = args.google {
            google.set_enabled(on);
        }
        if let Some(on) = args.photoreal {
            google.set_photoreal(on);
        }
        if let Some(d) = args.photo_detail {
            google.set_detail(d);
        }
        let src = Arc::new(sources::Sources::with_google(fetch::Fetcher::new(cache, args.offline), args.imagery.clone(), google));
        let (mut lat, mut lon, mut status) = (args.lat, args.lon, String::new());
        if let Some(p) = &args.place {
            match geocode(&src.fetch, p) {
                Some((la, lo, name)) => {
                    (lat, lon) = (la, lo);
                    status = name;
                }
                None => status = t!("game.could_not_find", p),
            }
        }
        let alt = args.alt.unwrap_or_else(|| ground_elev(&src, lat, lon) + args.agl.unwrap_or(450.0));
        // (How high above the ground it starts, for the wind there.)
        let spawn_agl = match args.alt {
            Some(a) => a - ground_elev(&src, lat, lon),
            None => args.agl.unwrap_or(450.0),
        };
        let spec = args.aircraft.as_deref().and_then(aircraft::by_key).unwrap_or(aircraft::BUILTIN[0]);
        let mut plane = Plane::spawn(spec, lat.to_radians(), lon.to_radians(), alt, args.heading.to_radians(), args.speed.map(|kt| kt / 1.94384).unwrap_or(spec.cruise));
        if args.ship {
            plane.rocket_return();
        }
        // (Aircraft-picker stills show it as if parked: gear down, engines
        // stopped, so its propeller blades are drawn rather than their blur.)
        if still_mode() {
            plane.gear_down = true;
            plane.gear = 1.0;
            plane.cold_and_dark();
        }
        if let Some(spec) = &args.damage {
            plane.dmg.apply_spec(spec);
        }
        let mut world = world::World::new(sources::Loader::new(src.clone(), 14), args.max_zoom);
        let mut r = r;
        let livery = args.livery.filter(|&n| n < aircraft::liveries(spec).len()).unwrap_or(aircraft::default_livery(spec));
        let lv = aircraft::liveries(spec).get(livery);
        r.set_paint(lv.and_then(|l| l.paint).or(spec.paint.as_ref()));
        r.set_livery(aircraft::livery_uniform(spec, lv));
        let Loaded { spec: _, model, piece_mesh, group_mesh, piece_cut, piece_inner, group_pivots, plane_box, lamps } = Loaded::build(spec, &mut r);
        let tm = trees::build_meshes();
        r.set_tree_meshes(&tm.meshes);
        world.tree_dims = tm.dims;
        let mut solids = cars::build_models();
        solids.push(cars::cube());
        solids.extend((0..3).map(|k| cars::rock(k * 17 + 3)));
        solids.extend(destruct::slab_meshes());
        solids.extend(destruct::rebar_meshes());
        debug_assert_eq!(solids.len() as u32, airmodels::BASE);
        solids.extend(airmodels::build());
        debug_assert_eq!(solids.len() as u32, birds::BASE);
        solids.extend(birds::models().meshes.iter().cloned());
        r.set_inst_meshes(&solids);
        r.set_scenery(scenery::packs());
        let mut menu = menu::Menu::new(lat, lon, args.hour);
        menu.fuel = args.fuel.unwrap_or(menu.fuel);
        menu.cold = args.cold;
        menu.aircraft = aircraft::index_of(spec);
        menu.liveries.insert(spec.name, livery);
        if let Some((la, lo, al)) = args.menu_view {
            menu.look_at(la, lo, al);
            menu.alt = al;
        }
        // The screen to open on: the main menu unless asked otherwise.
        if args.menu_view.is_some() || args.airport.is_some() {
            menu.screen = menu::Screen::Fly;
        }
        if args.settings.is_some() {
            menu.screen = menu::Screen::Settings;
        }
        if let Some((tab, sel)) = &args.hangar {
            menu.screen = menu::Screen::Aircraft;
            menu.air_tab = *tab;
            menu.air_view = menu.aircraft;
            menu.hangar_sel = sel.clone();
            if *tab == hangar_ui::Tab::Download {
                hangar::open();
            }
        }
        if let Some(sc) = args.menu_screen {
            menu.screen = sc;
            menu.air_view = menu.aircraft;
        }
        // The sun on the globe is the real one: open on the day side (unless
        // a view, a start position or a start time was given).
        let time_given = std::env::args().any(|a| matches!(a.as_str(), "--time" | "--day" | "--time-rate"));
        if args.menu_view.is_none() && !args.pos_set && !time_given {
            menu.lon = menu::day_side_lon(weather::real_clock().0);
            menu.lat = 25.0;
        }
        let mut wx = weather::Weather::new(args.clouds);
        match args.weather_preset.as_deref() {
            Some(p) if p.eq_ignore_ascii_case("storm") || p.eq_ignore_ascii_case("storms") || p.eq_ignore_ascii_case("thunderstorm") => wx.storm_field(plane.pos, args.heading),
            Some(p) => {
                if !wx.preset(p) {
                    eprintln!("unknown weather preset {p}");
                }
            }
            None => {}
        }
        if let Some(s) = args.storms {
            wx.storms = s.clamp(0.0, 1.0);
        }
        if let Some(s) = args.alto {
            wx.alto = s.clamp(0.0, 1.0);
        }
        wx.rain = args.rain.unwrap_or(0.0).clamp(0.0, 1.0);
        wx.rain_set = args.rain.map(|r| r.clamp(0.0, 1.0));
        wx.deck_place = args.rain_at;
        // The rain deck closes in from the start; the windows start as wet as
        // the rain where the aircraft is (none above the deck).
        wx.step_deck(0.0, plane.pos, None);
        wx.rain = wx.deck_rain(plane.pos) as f32;
        let mut lightning = storm::Lightning::new();
        lightning.forced = args.strikes.clone();
        // (As wet as the rain's share: snow does not run down the glass.)
        let start_snow = {
            let (pla, _, palt) = geo::pos_to_lla(plane.pos);
            let day = args.day.unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) / 86400.0 % 365.25);
            snow::snow_share(palt, snow::freezing_guess(args.snow.unwrap_or(prefs.snow), pla.to_degrees(), day)) as f32
        };
        let mut glass = rain::Glass::new(wx.rain * (1.0 - start_snow));
        if let Some(w) = args.wet {
            glass.wet = w.clamp(0.0, 1.0);
        }
        for &(d, b, r, kind) in &args.storm_at {
            wx.place_storm(plane.pos, d, b, r.clamp(1.5, 24.0), kind);
        }
        if let Some(r) = args.time_rate {
            wx.time_rate = r.clamp(0.0, 3600.0);
        }
        if let Some((kt, from)) = args.wind {
            (wx.wind, wx.wind_from) = (kt / 1.94384, from);
        } else if let Some(&(_, b, _)) = args.squall_at.first() {
            // A squall line comes on broadside with the wind: from where it was put.
            (wx.wind, wx.wind_from) = (wx.wind.max(8.0), b.rem_euclid(360.0));
        }
        for &(d, b, l) in &args.squall_at {
            wx.place_squall(plane.pos, d, b, l.clamp(10.0, 400.0));
        }
        if let Some(h) = args.haze {
            wx.haze = h.clamp(0.2, 40.0);
        }
        // Real weather, unless the command line sets the weather itself.
        let scripted = std::env::args().any(|a| matches!(a.as_str(), "--clouds" | "--rain" | "--storms" | "--alto" | "--wind" | "--haze" | "--squall-at"));
        wx.live = (prefs.live_weather && !scripted) || args.live_weather;
        let set_time = std::env::args().any(|a| matches!(a.as_str(), "--time" | "--day" | "--time-rate"));
        wx.real_time = prefs.real_time && !set_time;
        // The spawn speed is through the air: start out moving with the wind,
        // or a headwind starts it off fast (and climbing it away).
        plane.wind = wx.steady(plane.pos, spawn_agl.max(2.0));
        plane.vel += plane.wind;
        let egui_ctx = egui::Context::default();
        load_fonts(&egui_ctx);
        menu::apply_theme(&egui_ctx);
        let egui_state = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, Some(4096));
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let doy = (secs / 86400.0) % 365.25;
        let cam_pos = plane.pos - plane.forward() * spec.cam_dist;
        let weapons = weapons::Weapons::new(spec, &model.pieces);
        // What the anti-aliasing takes to be flying along with the camera:
        // everything within the aircraft's bounds, and a little around them
        // for the gear, flaps and control surfaces as they move.
        let (cy, cp, cd) = args.cam.unwrap_or((0.0, 0.2, spec.cam_dist));
        let cockpit_controls = aircraft::cockpit_controls(spec, &model.pieces);
        let pit_blocks = cockpit::blockers(&model.pieces);
        let mut g = Self {
            gkey: src.google.key(), gkey_show: false, gm_logo: None, stills: std::collections::HashMap::new(), window, r, world, src, spec, model, piece_mesh, group_mesh, piece_cut, piece_inner, group_pivots, hangar: Vec::new(), building: Vec::new(), crashed: false, crash: None, crash_real: 0.0, egui_ctx, egui_state, keys: HashSet::new(), drag: false, lean_drag: None, last_cursor: None,
            minimap: minimap::Minimap::new(&prefs),
            cam_yaw: cy.to_radians(), cam_pitch: cp, cam_dist: cd, cam_mode: args.start_view.unwrap_or(prefs.cam_mode), view_before_tail: 0, cam_pos, cam_dir: -plane.forward(), cam_look: plane.pos, orbit: 0.0, cam_rise: 0.0, cam_clear_t: 0.0, cam_ang: None, cam_hold: 0.0,
            utc: (args.hour - lon / 15.0).rem_euclid(24.0), doy: args.day.unwrap_or(doy), wx, time: 0.0, real: 0.0, last: Instant::now(), acc: 0.0, wave_off: DVec2::ZERO, shore: shore::Shore::default(), ocean_off: DVec2::ZERO, prev_cam: cam_pos, prev_vp: [[0.0; 4]; 4], gfx, gfx_applied: None, casc_keep: [([[0.0; 4]; 4], DVec3::ZERO); 2], casc_ok: 0, casc_res: 0, gfx_eff: gfx, dynq: dynamic::Dynamic::default(), storms_seen: false, dyn_cli: args.dynamic.is_some(), prev_plane: Mat4::IDENTITY, plane_box, lamps,
            frame: 0, fps: 60.0, fps_log: (args.fps_log, Instant::now(), 0, 0.0), paused: false, show_help: false, show_hud: !args.no_hud && prefs.show_hud, hud_cockpit: prefs.hud_cockpit, path_marker: prefs.path_marker, fullscreen: prefs.fullscreen, plan: flightplan::Plan::new(), mouse_yoke: false, yoke: (0.0, 0.0), yoke_thr: 0.0, plan_text: String::new(), plan_msg: String::new(), plan_pick: false, arcade: args.arcade || prefs.arcade, topple_buildings: prefs.topple_buildings, goto_open: false, goto_text: String::new(), goto_rx: None,
            status, status_until: 8.0, shots: args.shots.clone(), crash_at: args.crash_at, boost_at: args.boost_at, stage_at: args.stage_at, engines: args.engines, hold: args.hold, gilrs: open_gamepads(), pad: Controls::default(), pad_id: None, pad_fixed: Vec::new(), pad_events: Vec::new(), pad_move: None, pad_skip: None, pad_rep: None, pad_home: false, pad_y: false, pad_y_combo: false, pad_held: Vec::new(), pad_note: (String::new(), 0.0), pad_aim: false, pad_live: None, ap: autopilot::Autopilot::default(), ap_off_t: -100.0, ap_script: args.ap_script.clone(), cam_home: (cy.to_radians(), cp), rng: crash::Rng(secs.to_bits() | 1),
            audio: if args.record.is_some() { Some(audio::Audio::offline()) } else if args.mute { None } else { audio::Audio::new() },
            record: args.record.clone(), rec_audio: Vec::new(), rec_n: 0, rec_t0: Instant::now(), sound_on: prefs.sound_on, volume: prefs.volume, snow: args.snow.unwrap_or(prefs.snow), info_open: args.weather_panel, radial: Default::default(), info_page: if args.weather_panel { menu::Page::Weather } else { menu::Page::Flight }, pause_open: false, pause_page: menu::Page::Game, flight: None, logbook: flightlog::Logbook::load(), logbook_saves: !std::env::args().any(|a| a == "--headless" || a == "--shot" || a == "--record" || a == "--bench"), pause_at: args.pause_menu, radial_at: args.flight_menu, settings_sec: args.settings.unwrap_or(settings_ui::Section::General), look_sens: prefs.look_sens, invert_look_cockpit: prefs.invert_look_cockpit, invert_look_outside_x: prefs.invert_look_outside_x, invert_look_outside_y: prefs.invert_look_outside_y, head: cockpit::Head::new(cy.to_radians(), cp), cockpit_fov: args.fov.unwrap_or(prefs.cockpit_fov).clamp(30.0, 90.0), head_live: false, warn: audio::Warn::default(), warn_t: [-10.0; 4],
            g_slow: 1.0, last_bump: 0.0, was_ground: false, prev_vs: 0.0, prev_gear: plane.gear, prev_flaps: plane.flaps, quit: false,
            mode: if args.menu.unwrap_or(true) { Mode::Menu } else { Mode::Fly }, menu, menu_go: args.menu_go, heading: args.heading.to_radians(), destr: destruct::Destruction::new(secs.to_bits()),
            sheds: damage::Sheds::default(), fx_anchor: plane.pos, fx_shift: DVec3::ZERO, smoke_acc: 0.0, flash: Vec::new(), airport_note: (String::new(), -100.0), weapon_flash: None, kills: Vec::new(), damage_spec: args.damage.clone(), wreck_dirty: false, wreck_t: 0.0, wreck_seen: 0.0,
            start_rx: None, loading: None, name_rx: None, last_click: -10.0, n_cars: 0, n_scenery: 0, skyglow: Default::default(), snowmap: Default::default(), snowtrack: Default::default(), birds: birds::Birds::with_flocks(args.flocks.clone()), birds_level: args.birds, contrails: contrails::Contrails::new(), contrail_draw: contrails::DrawData::default(), bird_insts: 0, bird_shape: ("", birds::Shape::default()), gauges: gauges::Gauges::new(&plane, secs.to_bits()), glass, lightning, rain_field: storm::RainField::default(), cam_rain: 0.0, cam_hail: 0.0, snow_field: storm::RainField::default(), cam_snow: start_snow, cam_blow: 0.0, fresh: snow::Fresh::default(), fresh_line: 0.0, storm_adapt: 0.0, city_adapt: 0.0, city_probe: None, plane, weapons,
            airport_rx: None, hold_spot: None, bench: args.bench.map(Bench::new),
            prefs_lang: args.lang.or(prefs.lang),
            prefs_t: 0.0,
            live: live::Live::new(prefs.live_traffic),
            mp_on: (prefs.multiplayer && !mp_kept_off(args)) || args.mp.is_some(),
            mp_held: prefs.multiplayer && mp_kept_off(args) && args.mp.is_none(),
            mp_server: args.mp.as_ref().map_or(prefs.mp_server.clone(), |m| m.0.clone()),
            mp_name: args.mp.as_ref().and_then(|m| m.1.clone()).unwrap_or(prefs.mp_name.clone()),
            mp: None,
            mp_players: Vec::new(),
            mp_world: Vec::new(),
            mp_world_total: 0,
            mp_world_known: false,
            mp_dir: None,
            mp_custom: false,
            mp_list: args.mp_list.iter().cloned().chain(mp::DIRECTORY_URLS.iter().map(|u| u.to_string())).collect(),
            mp_puppets: std::collections::HashMap::new(),
            mp_mirror: args.mp_mirror,
            live_view: None,
            cursor: args.cursor,
            mouse_captured: false,
            script_cursor: args.cursor,
            script_clicks: args.clicks.clone(),
            hot_press: None,
            wheel_hold: Vec::new(),
            wheel_acc: cockpit::WheelSteps::default(),
            hot_keys: HashSet::new(),
            cockpit_controls,
            pit_blocks,
            livewx: livewx::LiveWx::new(),
            oro: orographic::Orography::default(),
            wakes: wake::Wakes::default(),
            gclouds: globeclouds::GlobeClouds::default(),
            menu_time_set: set_time,
            menu_time_seen: (f64::NAN, f64::NAN),
            binds_stamp: String::new(),
            binds: bind::Bindings::load(),
            fail_random: false,
            fail_mean: 30.0,
            fail_next: None,
            radios: nav::Radios::default(),
            prefs: prefs.clone(),
            last_start: prefs.last.clone(),
            pending_airport: args.airport.as_deref().and_then(|c| {
                let i = airports::Db::get().find(c);
                if i.is_none() {
                    eprintln!("unknown airport {c}");
                }
                i.map(|i| (i, args.runway.clone(), args.approach))
            }),
            mcp_calls: None,
            mcp_shots: Vec::new(),
            mcp_shot_n: 0,
            mcp_throttle: None,
            throttle_axis: None,
        };
        if args.mcp {
            match mcp::rpc::serve(args.mcp_port) {
                Ok(rx) => g.mcp_calls = Some(rx),
                Err(e) => log::warn!("AI copilot service on 127.0.0.1:{}: {e}", args.mcp_port),
            }
        }
        std::thread::spawn(|| {
            navdata::Db::get();
        });
        if let Some(r) = &args.route {
            let (lat, lon, _) = geo::pos_to_lla(g.plane.pos);
            let here = (lat.to_degrees(), lon.to_degrees());
            let bad = g.plan.add_route(r, Some(g.menu.pick.unwrap_or(here)));
            if !bad.is_empty() {
                log::warn!("--route: not found: {}", bad.join(", "));
            }
            if g.mode == Mode::Fly {
                g.plan.begin(here);
            }
        }
        if let Some(plan) = args.minimap_plan { g.minimap.plan = plan; }
        if let Some(up) = args.minimap_heading_up { g.minimap.heading_up = up; }
        if args.full_map { g.minimap.toggle_full(); }
        // With --menu, an --airport is chosen on the globe rather than flown from.
        if args.menu == Some(true) {
            if let Some((i, rw, appr)) = g.pending_airport.take() {
                g.choose_airport(i);
                g.menu.runway = rw;
                g.menu.start = if appr { menu::Start::Approach } else { menu::Start::Runway };
            }
        }
        // otherwise open the globe on the last start.
        if g.menu.pick.is_none() && !args.pos_set && args.menu_view.is_none() && args.aircraft.is_none() {
            g.restore_last_start();
        }
        progress::step("world");
        Ok(g)
    }

    /// the hud shows when on, but in the cockpit only when turned on there.
    pub(crate) fn hud_on(&self) -> bool {
        self.show_hud && (self.cam_mode != 1 || self.hud_cockpit)
    }

    /// puts the last flight's start and aircraft back on the globe.
    pub(crate) fn restore_last_start(&mut self) {
        let Some(l) = self.last_start.clone() else { return };
        if let Some(spec) = aircraft::by_key(&l.aircraft) {
            self.menu.aircraft = aircraft::index_of(spec);
            self.menu.air_view = self.menu.aircraft;
        }
        match l.airport.as_deref().and_then(|c| airports::Db::get().find(c)) {
            Some(i) => {
                self.choose_airport(i);
                self.menu.runway = l.runway.clone();
            }
            None => {
                self.menu.pick = Some((l.lat, l.lon));
                self.menu.pick_name = l.name.clone();
                self.menu.airport = None;
            }
        }
        self.menu.start = match l.start.as_str() {
            "approach" => menu::Start::Approach,
            "overhead" => menu::Start::Overhead,
            _ => menu::Start::Runway,
        };
        self.menu.look_at(l.lat, l.lon, 30_000.0);
    }

    /// the place and aircraft for the fly again button.
    pub(crate) fn last_start_label(&self) -> Option<(String, &'static str)> {
        let l = self.last_start.as_ref()?;
        let spec = aircraft::by_key(&l.aircraft)?;
        let place = match (&l.airport, &l.runway, l.start.as_str()) {
            (Some(a), Some(r), "runway") => format!("{a} {r}"),
            (Some(a), _, "approach") => t!("game.approach", a),
            (Some(a), _, _) => a.clone(),
            (None, _, _) if !l.name.is_empty() => l.name.chars().take(32).collect(),
            _ => menu::latlon(l.lat, l.lon),
        };
        Some((place, spec.name))
    }

    /// Fly another aircraft from the next spawn on: its model is built (once
    /// a session) and swapped in.
    /// The next view: outside, the cockpit, and the other cockpit of a type that has two.
    pub(crate) fn next_view(&mut self) {
        self.cancel_cockpit_input();
        self.head.center_seat();
        if self.cam_mode == 1 && !self.head.rear && self.spec.eye2.is_some() {
            self.head.rear = true;
        } else {
            self.head.rear = false;
            self.cam_mode = if self.cam_mode == 1 { 0 } else { 1 };
        }
    }

    pub(crate) fn toggle_tail_view(&mut self) {
        self.cancel_cockpit_input();
        if self.cam_mode == settings::TAIL_VIEW {
            self.cam_mode = self.view_before_tail;
        } else {
            self.view_before_tail = self.cam_mode;
            self.cam_mode = settings::TAIL_VIEW;
        }
        self.status = match self.cam_mode {
            settings::TAIL_VIEW => t!("game.fixed_rear_view"),
            1 => t!("game.cockpit_view"),
            _ => t!("game.free_chase_view"),
        }.into();
        self.status_until = self.real + 2.0;
    }

    pub(crate) fn set_aircraft(&mut self, spec: &'static aircraft::Spec) {
        if std::ptr::eq(spec, self.spec) {
            return;
        }
        self.head.rear = false;
        self.head.center_seat();
        self.restore_wreck_meshes();
        self.sheds.items.clear();
        self.contrails.clear();
        let next = match self.hangar.iter().position(|l| std::ptr::eq(l.spec, spec)) {
            Some(i) => self.hangar.swap_remove(i),
            None => match self.building.iter().position(|(s, _)| std::ptr::eq(*s, spec)) {
                // already building in the background: wait for that rather than start over
                Some(i) => {
                    let (_, rx) = self.building.swap_remove(i);
                    match rx.recv() {
                        Ok(m) => Loaded::upload(spec, m, &mut self.r),
                        Err(_) => Loaded::build(spec, &mut self.r),
                    }
                }
                None => Loaded::build(spec, &mut self.r),
            },
        };
        let old = Loaded {
            spec: self.spec,
            model: std::mem::replace(&mut self.model, next.model),
            piece_mesh: std::mem::replace(&mut self.piece_mesh, next.piece_mesh),
            group_mesh: std::mem::replace(&mut self.group_mesh, next.group_mesh),
            piece_cut: std::mem::replace(&mut self.piece_cut, next.piece_cut),
            piece_inner: std::mem::replace(&mut self.piece_inner, next.piece_inner),
            group_pivots: std::mem::replace(&mut self.group_pivots, next.group_pivots),
            plane_box: std::mem::replace(&mut self.plane_box, next.plane_box),
            lamps: std::mem::replace(&mut self.lamps, next.lamps),
        };
        self.hangar.push(old);
        self.spec = spec;
        self.cancel_cockpit_input();
        self.cockpit_controls = aircraft::cockpit_controls(spec, &self.model.pieces);
        self.pit_blocks = cockpit::blockers(&self.model.pieces);
        self.weapons.reset(spec, &self.model.pieces);
        self.cam_dist = spec.cam_dist;
        self.menu.aircraft = aircraft::index_of(spec);
        self.apply_livery();
    }

    /// Paint the flown type in the scheme picked for it: its paint (a scheme's
    /// own baked one, else the type's) and the scheme's uniform.
    pub(crate) fn apply_livery(&mut self) {
        let lv = aircraft::liveries(self.spec).get(self.menu.livery(self.spec));
        self.r.set_paint(lv.and_then(|l| l.paint).or(self.spec.paint.as_ref()));
        self.r.set_livery(aircraft::livery_uniform(self.spec, lv));
    }

    /// Menu selection: swap to `spec` without stalling the frame. A model not
    /// built yet is generated on a worker thread and swapped in once it's done
    /// (only the GPU upload stays on the main thread).
    pub(crate) fn want_aircraft(&mut self, spec: &'static aircraft::Spec) {
        if self.hangar.iter().any(|l| std::ptr::eq(l.spec, spec)) {
            return self.set_aircraft(spec);
        }
        match self.building.iter().position(|(s, _)| std::ptr::eq(*s, spec)) {
            Some(i) => match self.building[i].1.try_recv() {
                Ok(m) => {
                    self.building.swap_remove(i);
                    let l = Loaded::upload(spec, m, &mut self.r);
                    self.hangar.push(l);
                    self.set_aircraft(spec);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(_) => {
                    self.building.swap_remove(i);
                    self.set_aircraft(spec);
                }
            },
            None => {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(Loaded::make(spec));
                });
                self.building.push((spec, rx));
            }
        }
    }

    /// The payload chosen on the start screen (kg), or the aircraft's usual.
    pub(crate) fn start_payload(&self) -> f64 {
        let [usual, most, _] = self.spec.payload;
        self.menu.payload.map(|f| f * most).unwrap_or(usual)
    }

    /// toggles full screen.
    pub(crate) fn toggle_fullscreen(&mut self) {
        self.set_fullscreen(self.window.fullscreen().is_none());
    }

    pub(crate) fn set_fullscreen(&mut self, on: bool) {
        self.window.set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)));
        self.fullscreen = on;
    }

    /// The settings worth keeping, as they are now.
    pub(crate) fn live_settings(&self) -> settings::Settings {
        settings::Settings { lang: self.prefs_lang, sound_on: self.sound_on, volume: self.volume, cam_mode: self.cam_mode, cockpit_fov: self.cockpit_fov, look_sens: self.look_sens, invert_look_cockpit: self.invert_look_cockpit, invert_look_outside_x: self.invert_look_outside_x, invert_look_outside_y: self.invert_look_outside_y, show_hud: self.show_hud, hud_cockpit: self.hud_cockpit, path_marker: self.path_marker, minimap: self.minimap.visible, landing_guidance: self.minimap.landing_guidance, arcade: self.arcade, topple_buildings: self.topple_buildings, minimap_plan: self.minimap.plan, minimap_heading_up: self.minimap.heading_up, minimap_zoom: self.minimap.zoom, minimap_size: self.minimap.size.x, minimap_height: self.minimap.size.y, map_airports: self.minimap.layers.airports, map_navaids: self.minimap.layers.navaids, taa: self.gfx.taa, fullscreen: self.fullscreen, live_traffic: self.live.enabled(), live_weather: self.wx.live, real_time: self.wx.real_time, snow: self.snow, last: self.last_start.clone(), multiplayer: self.mp_on || self.mp_held, mp_server: self.mp_server.clone(), mp_name: self.mp_name.clone() }
    }

    /// Saves the settings when they have changed (looked at once a second,
    /// so dragging a slider does not write the file every frame).
    pub(crate) fn keep_settings(&mut self, now: bool) {
        if std::env::args().any(|a| a == "--headless" || a == "--shot" || a == "--record" || a == "--bench") { return; }
        if !now && self.real - self.prefs_t < 1.0 {
            return;
        }
        self.prefs_t = self.real;
        let s = self.live_settings();
        if s != self.prefs {
            s.save();
            self.prefs = s;
        }
        let stamp = self.binds.stamp();
        if self.binds_stamp.is_empty() {
            self.binds_stamp = stamp;
        } else if stamp != self.binds_stamp {
            self.binds.save();
            self.binds_stamp = stamp;
        }
    }

    /// The languages found in `assets/locales`, as selectable buttons, and a
    /// button to read the `.lng` files again (a new or edited translation).
    pub(crate) fn language_picker(&mut self, ui: &mut egui::Ui) {
        let mut cur = i18n::lang();
        menu::segments(ui, |ui| {
            for l in i18n::Lang::all() {
                menu::seg(ui, &mut cur, l, l.name());
            }
        });
        if ui.small_button(t!("settings.reload_languages")).on_hover_text(t!("settings.reload_languages_hint")).clicked() {
            i18n::reload();
            cur = i18n::lang();
        }
        if cur != i18n::lang() {
            i18n::set_lang(cur);
            self.prefs_lang = Some(cur);
        }
    }

    /// Hands changed graphics settings to the renderer and the world.
    pub(crate) fn apply_graphics(&mut self) {
        let g = self.dynq.settings(&self.gfx, self.storms_seen);
        self.gfx_eff = g;
        // (from the player's setting, not the lowered one: dynamic terrain
        // detail must not drop loaded tiles that would then be fetched again)
        self.world.tile_cap = self.gfx.tile_cap();
        if self.gfx_applied == Some(g) {
            return;
        }
        // The streaming budget, from the GPU's memory less what the renderer
        // holds at the player's resolution (not the lowered one, which comes back up).
        self.world.set_vram(self.r.vram, self.r.fixed_bytes(self.gfx.render_scale()));
        self.r.set_upscaler(g.upscaler);
        self.r.set_scale(g.render_scale());
        self.r.set_shadow_res(g.shadow_res());
        self.r.set_vsync(g.vsync);
        self.r.ao = g.ao;
        self.r.cloud_step = g.cloud_step();
        self.r.clouds = g.clouds_on();
        self.r.cloud_shadows = g.clouds_on() && g.cloud_shadows;
        self.r.set_cloud_shadow_slices(g.cloud_shadow_slices());
        self.world.lod_k = g.lod_k();
        self.world.reach_k = g.reach();
        self.world.city_k = g.city_k();
        self.world.spare_builds = g.spare_builds();
        self.world.max_build_jobs = g.max_build_jobs();
        self.world.tree_far = g.tree_far();
        self.world.tree_imp_k = g.tree_imp_k();
        self.world.cars = g.car_density();
        if g.dynamic && self.gfx_applied.is_some_and(|a| a.dynamic) {
            log::info!("dynamic quality: step {}/{} ({:.1} ms), {:.0}%, shadows {}, clouds {} (shadows {}), storms {}, terrain {}, city {}, trees {}, AO {}, water {}", self.dynq.down, self.dynq.depth(), self.dynq.ema, g.render_scale() * 100.0, g.shadows.name(), g.clouds.name(), g.cloud_shadows, g.storms.name(), g.terrain.name(), g.city.name(), g.trees.name(), g.ao, g.water.name());
        } else if self.gfx_applied.is_some() {
            log::info!("graphics: {} at {:.0}%, shadows {}, clouds {} (shadows {}), storms {}, contrails {}, terrain {}, scenery {}, city {}, trees {}, traffic {}, AO {}, water {}, MSAA {}x", g.matches().map(graphics::Preset::name).unwrap_or("custom"), g.scale * 100.0, g.shadows.name(), g.clouds.name(), g.cloud_shadows, g.storms.name(), g.contrails, g.terrain.name(), g.scenery.name(), g.city.name(), g.trees.name(), g.cars.name(), g.ao, g.water.name(), g.msaa);
        }
        self.gfx_applied = Some(g);
    }
}
