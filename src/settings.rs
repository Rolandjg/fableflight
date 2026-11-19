//! Settings kept between runs, in `<config dir>/fablegame/settings.json`
//! (`~/.config/fablegame` on Linux): the language, sound, camera, HUD and
//! anti-aliasing. Command-line flags still win over them for one run.

use crate::i18n::Lang;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// None: follow the system's language
    pub lang: Option<Lang>,
    pub sound_on: bool,
    pub volume: f32,
    /// 0 chase, 1 cockpit
    pub cam_mode: u8,
    pub cockpit_fov: f32,
    pub look_sens: f64,
    pub show_hud: bool,
    /// the hud in the cockpit view too.
    pub hud_cockpit: bool,
    pub taa: bool,
    pub minimap: bool,
    pub landing_guidance: bool,
    /// arcade mode: the game's fun features (missiles, boost, respawn at will, weather keys)
    pub arcade: bool,
    /// the auto-trim assist: winds the elevator trim while the pilot flies the
    /// stick, on aircraft with a real trim (on by default)
    pub auto_trim: bool,
    pub minimap_plan: bool,
    pub minimap_heading_up: bool,
    pub minimap_zoom: u8,
    pub minimap_size: f32,
    pub minimap_height: f32,
    /// real air traffic (see live.rs)
    pub live_traffic: bool,
    /// real weather (see livewx.rs)
    pub live_weather: bool,
    /// the clock follows the real time and date
    pub real_time: bool,
    /// snow lying on the ground (snow.rs)
    pub snow: crate::snow::Mode,
    /// multiplayer (mp.rs): on, the relay ("host" or "host:port"), our name
    pub multiplayer: bool,
    pub mp_server: String,
    pub mp_name: String,
    /// where the last flight started.
    pub last: Option<LastStart>,
}

/// the start of the last flight.
#[derive(Clone, Debug, PartialEq)]
pub struct LastStart {
    pub lat: f64,
    pub lon: f64,
    /// the start point's name.
    pub name: String,
    /// the airport's ident, if any.
    pub airport: Option<String>,
    /// "runway", "approach" or "overhead".
    pub start: String,
    /// the runway end, none for into the wind.
    pub runway: Option<String>,
    /// the aircraft's key.
    pub aircraft: String,
}

impl LastStart {
    fn from_json(j: &serde_json::Value) -> Option<Self> {
        let lat = j["lat"].as_f64().filter(|x| (-90.0..=90.0).contains(x))?;
        let lon = j["lon"].as_f64().filter(|x| (-180.0..=180.0).contains(x))?;
        let s = |k: &str| j[k].as_str().map(str::to_string);
        Some(Self { lat, lon, name: s("name").unwrap_or_default(), airport: s("airport"), start: s("start").unwrap_or_else(|| "runway".into()), runway: s("runway"), aircraft: s("aircraft").unwrap_or_default() })
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "lat": self.lat, "lon": self.lon, "name": self.name, "airport": self.airport, "start": self.start, "runway": self.runway, "aircraft": self.aircraft })
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self { lang: None, sound_on: true, volume: 1.0, cam_mode: 0, cockpit_fov: crate::cockpit::FOV, look_sens: 1.0, show_hud: true, hud_cockpit: false, minimap: true, landing_guidance: true, arcade: false, auto_trim: true, minimap_plan: false, minimap_heading_up: false, minimap_zoom: 12, minimap_size: 280.0, minimap_height: 280.0, taa: true, live_traffic: true, live_weather: true, real_time: true, snow: crate::snow::Mode::Auto, last: None, multiplayer: false, mp_server: "auto".into(), mp_name: String::new() }
    }
}

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("fablegame").join("settings.json"))
}

impl Settings {
    /// The saved settings; the defaults for anything missing or unreadable.
    pub fn load() -> Self {
        let Some(j) = path().and_then(|p| std::fs::read(p).ok()).and_then(|d| serde_json::from_slice::<serde_json::Value>(&d).ok()) else {
            return Self::default();
        };
        Self::from_json(&j)
    }

    fn from_json(j: &serde_json::Value) -> Self {
        let d = Self::default();
        let b = |k: &str, v: bool| j[k].as_bool().unwrap_or(v);
        let f = |k: &str, v: f64| j[k].as_f64().filter(|x| x.is_finite()).unwrap_or(v);
        Self {
            lang: j["language"].as_str().and_then(Lang::parse),
            sound_on: b("sound", d.sound_on),
            volume: f("volume", d.volume as f64).clamp(0.0, 1.0) as f32,
            cam_mode: if j["camera"].as_str() == Some("cockpit") { 1 } else { 0 },
            cockpit_fov: f("cockpit_fov", d.cockpit_fov as f64).clamp(35.0, 85.0) as f32,
            look_sens: f("mouse_look_speed", d.look_sens).clamp(0.25, 3.0),
            show_hud: b("hud", d.show_hud),
            hud_cockpit: b("hud_cockpit", d.hud_cockpit),
            taa: b("taa", d.taa),
            minimap: b("minimap", d.minimap),
            landing_guidance: b("landing_guidance", d.landing_guidance),
            arcade: b("arcade", d.arcade),
            auto_trim: b("auto_trim", d.auto_trim),
            minimap_plan: b("minimap_plan", d.minimap_plan),
            minimap_heading_up: b("minimap_heading_up", d.minimap_heading_up),
            minimap_size: f("minimap_size", d.minimap_size as f64).clamp(240.0, 640.0) as f32,
            minimap_height: f("minimap_height", f("minimap_size", d.minimap_size as f64)).clamp(240.0, 640.0) as f32,
            minimap_zoom: f("minimap_zoom", d.minimap_zoom as f64).clamp(3.0, 17.0) as u8,
            live_traffic: b("live_traffic", d.live_traffic),
            live_weather: b("live_weather", d.live_weather),
            real_time: b("real_time", d.real_time),
            snow: j["terrain_snow"].as_str().and_then(crate::snow::Mode::parse).unwrap_or(d.snow),
            multiplayer: b("multiplayer", d.multiplayer),
            mp_server: j["mp_server"].as_str().map(|s| s.trim().chars().take(100).collect()).unwrap_or(d.mp_server),
            mp_name: j["mp_name"].as_str().map(crate::mp_proto::clean_name).unwrap_or(d.mp_name),
            last: LastStart::from_json(&j["last_start"]),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "language": self.lang.map(|l| l.code()),
            "sound": self.sound_on,
            "volume": self.volume,
            "camera": if self.cam_mode == 1 { "cockpit" } else { "chase" },
            "cockpit_fov": self.cockpit_fov,
            "mouse_look_speed": self.look_sens,
            "hud": self.show_hud,
            "hud_cockpit": self.hud_cockpit,
            "taa": self.taa,
            "minimap": self.minimap,
            "landing_guidance": self.landing_guidance,
            "arcade": self.arcade,
            "auto_trim": self.auto_trim,
            "minimap_plan": self.minimap_plan,
            "minimap_heading_up": self.minimap_heading_up,
            "minimap_zoom": self.minimap_zoom,
            "minimap_size": self.minimap_size,
            "minimap_height": self.minimap_height,
            "live_traffic": self.live_traffic,
            "live_weather": self.live_weather,
            "real_time": self.real_time,
            "terrain_snow": self.snow.code(),
            "multiplayer": self.multiplayer,
            "mp_server": self.mp_server,
            "mp_name": self.mp_name,
            "last_start": self.last.as_ref().map(LastStart::to_json),
        })
    }

    /// Writes them (through a temporary file, so a crash cannot leave half a file).
    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = p.with_extension("json.tmp");
        let data = serde_json::to_vec_pretty(&self.to_json()).unwrap_or_default();
        if let Err(e) = std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, &p)) {
            log::warn!("saving settings to {}: {e}", p.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Settings { lang: Some(Lang::Fr), sound_on: false, volume: 0.4, cam_mode: 1, cockpit_fov: 50.0, look_sens: 1.5, show_hud: false, hud_cockpit: true, minimap: false, landing_guidance: false, arcade: true, auto_trim: false, minimap_plan: true, minimap_heading_up: true, minimap_zoom: 15, minimap_size: 420.0, minimap_height: 300.0, taa: false, live_traffic: false, live_weather: false, real_time: false, snow: crate::snow::Mode::Always, last: Some(LastStart { lat: 37.6, lon: -122.4, name: "KSFO".into(), airport: Some("KSFO".into()), start: "approach".into(), runway: Some("28L".into()), aircraft: "c172".into() }), multiplayer: true, mp_server: "example.org:7000".into(), mp_name: "Dany".into() };
        assert_eq!(Settings::from_json(&s.to_json()), s);
        assert_eq!(Settings::from_json(&serde_json::json!({"last_start": {"lat": 300.0, "lon": 0.0}})).last, None);
        let legacy = Settings::from_json(&serde_json::json!({"minimap_size": 420}));
        assert_eq!((legacy.minimap_size, legacy.minimap_height), (420.0, 420.0));
        // Missing or odd values fall back to the defaults.
        let j = serde_json::json!({ "volume": 7.0, "language": "xx" });
        let t = Settings::from_json(&j);
        assert_eq!((t.volume, t.lang, t.taa), (1.0, None, true));
        assert_eq!(t.snow, crate::snow::Mode::Auto);
        assert_eq!(Settings::from_json(&serde_json::json!({"terrain_snow": "OFF"})).snow, crate::snow::Mode::Off);
        assert_eq!((t.minimap, t.minimap_heading_up, t.minimap_zoom), (true, false, 12));
        assert_eq!(Settings::from_json(&serde_json::json!({"minimap_size": 9999})).minimap_size, 640.0);
        assert_eq!(Settings::from_json(&serde_json::json!({"minimap_size": -5})).minimap_size, 240.0);
        assert_eq!(Settings::from_json(&serde_json::json!({"minimap_zoom": 999})).minimap_zoom, 17);
        assert_eq!(Settings::from_json(&serde_json::json!({"minimap_zoom": -5})).minimap_zoom, 3);
    }
}
