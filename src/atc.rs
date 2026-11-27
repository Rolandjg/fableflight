//! air traffic control in text. the com radio tunes the airports' real
//! frequencies (ourairports, public domain: assets/frequencies.tsv.gz, built
//! by assets/mkfreq.py). the controller on the frequency tuned answers the
//! pilot's calls: atis, clearance, taxi, takeoff and landing, and the
//! handoffs to departure and approach. at an airport without a tower the
//! calls go out to the traffic on the ctaf. what was said is kept as a log
//! and shown as subtitles. not real phraseology training, a game's version.
//!
//! with `auto` on ("talk me through"), `controller` follows the flight's
//! phases and makes the controllers' calls on its own, from the taxi
//! clearance to "taxi to parking", tuning com 1 to each station in turn; the
//! pilot's own calls are then kept to the ctaf.

pub mod controller;
pub mod install;
pub mod lexicon;
pub mod listen;
pub mod phrase;
pub mod tts;
pub mod voice;

use crate::airports::{self, Apt, Db};
use crate::{flightui, geo, menu, tr_en, Game};
use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

/// what a frequency is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Atis,
    Afis,
    Del,
    Gnd,
    Twr,
    Ctaf,
    Unic,
    App,
    Dep,
    Ctr,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "ATIS" => Kind::Atis,
            "AFIS" => Kind::Afis,
            "DEL" => Kind::Del,
            "GND" => Kind::Gnd,
            "TWR" => Kind::Twr,
            "CTAF" => Kind::Ctaf,
            "UNIC" => Kind::Unic,
            "APP" => Kind::App,
            "DEP" => Kind::Dep,
            "CTR" => Kind::Ctr,
            _ => return None,
        })
    }

    /// what the station calls itself after the place.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Atis => "Information",
            Kind::Afis => "Information",
            Kind::Del => "Delivery",
            Kind::Gnd => "Ground",
            Kind::Twr => "Tower",
            Kind::Ctaf => "Traffic",
            Kind::Unic => "Unicom",
            Kind::App => "Approach",
            Kind::Dep => "Departure",
            Kind::Ctr => "Center",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Kind::Atis => "ATIS",
            Kind::Afis => "AFIS",
            Kind::Del => "DEL",
            Kind::Gnd => "GND",
            Kind::Twr => "TWR",
            Kind::Ctaf => "CTAF",
            Kind::Unic => "UNICOM",
            Kind::App => "APP",
            Kind::Dep => "DEP",
            Kind::Ctr => "CTR",
        }
    }
}

/// one station: the airport, what it is, its frequency (mhz).
#[derive(Clone, Debug, PartialEq)]
pub struct Freq {
    pub apt: u32,
    pub kind: Kind,
    pub mhz: f32,
}

impl Freq {
    /// "Heathrow Tower".
    pub fn name(&self) -> String {
        let a = &Db::get().apts[self.apt as usize];
        format!("{} {}", place_name(a), self.kind.word())
    }
}

/// the airports' frequencies by their ident.
fn table() -> &'static HashMap<String, Vec<(Kind, f32)>> {
    static T: OnceLock<HashMap<String, Vec<(Kind, f32)>>> = OnceLock::new();
    T.get_or_init(|| {
        let mut text = String::new();
        let _ = flate2::read::GzDecoder::new(&include_bytes!("../assets/frequencies.tsv.gz")[..]).read_to_string(&mut text);
        let mut m: HashMap<String, Vec<(Kind, f32)>> = HashMap::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let (Some(k), Some(mhz)) = (f.get(1).and_then(|s| Kind::parse(s)), f.get(2).and_then(|s| s.parse::<f32>().ok())) else { continue };
            let v = m.entry(f[0].to_string()).or_default();
            if !v.iter().any(|x| x.0 == k && (x.1 - mhz).abs() < 0.001) {
                v.push((k, mhz));
            }
        }
        m
    })
}

/// the short name a controller goes by: the city for a big airport, else the airport's own.
pub(crate) fn place_name(a: &Apt) -> String {
    let n = a.name.replace(" International Airport", "").replace(" Airport", "").replace(" Airfield", "").replace(" Aerodrome", "");
    let n = n.trim();
    if n.is_empty() { a.ident.clone() } else { n.to_string() }
}

/// an airport's stations, one per kind (the first listed).
pub fn stations(apt: u32) -> Vec<Freq> {
    let a = &Db::get().apts[apt as usize];
    let mut out: Vec<Freq> = Vec::new();
    for &(kind, mhz) in table().get(&a.ident).map(|v| &v[..]).unwrap_or(&[]) {
        if !out.iter().any(|f| f.kind == kind) {
            out.push(Freq { apt, kind, mhz });
        }
    }
    out
}

/// has a tower (or an afis) that hands out clearances.
pub fn towered(apt: u32) -> bool {
    stations(apt).iter().any(|f| f.kind == Kind::Twr)
}

/// the runway end into the wind: the long paved ones first.
pub fn runway_for(a: &Apt, wind_from: f64, wind_kt: f64) -> Option<String> {
    let mut best: Option<(f64, String)> = None;
    for r in &a.rwys {
        for e in 0..2 {
            let id = &r.ident[e];
            let Some(hd) = airports::ident_heading(id) else { continue };
            let head = ((hd as f64 - wind_from).to_radians()).cos() * wind_kt.max(1.0);
            let score = r.len as f64 * 0.3048 * if r.paved { 1.0 } else { 0.5 } + head * 300.0;
            if best.as_ref().is_none_or(|b| score > b.0) {
                best = Some((score, airports::norm_ident(id)));
            }
        }
    }
    best.map(|b| b.1)
}

const PHONETIC: [&str; 26] = ["Alfa", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliett", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango", "Uniform", "Victor", "Whiskey", "X-ray", "Yankee", "Zulu"];
const DIGITS: [&str; 10] = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Niner"];

/// a callsign as it is said: "N172FF" letter by letter, "FABLE 21" as a word and its digits.
pub fn say(cs: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for w in cs.split_whitespace() {
        if w.chars().all(|c| c.is_ascii_alphabetic()) && w.len() > 3 {
            let mut s = w.to_lowercase();
            if let Some(f) = s.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            out.push(s);
            continue;
        }
        for c in w.chars() {
            if let Some(d) = c.to_digit(10) {
                out.push(DIGITS[d as usize].into());
            } else if c.is_ascii_alphabetic() {
                out.push(PHONETIC[(c.to_ascii_uppercase() as u8 - b'A') as usize].into());
            }
        }
    }
    out.join(" ")
}

/// a runway as it is said: "two eight left".
fn say_rwy(r: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for c in r.chars() {
        match c {
            '0'..='9' => out.push(DIGITS[c.to_digit(10).unwrap_or(0) as usize]),
            'L' => out.push("left"),
            'R' => out.push("right"),
            'C' => out.push("center"),
            _ => {}
        }
    }
    format!("{} ({r})", out.join(" ").to_lowercase())
}

/// a transponder code: four octal digits, clear of 1200 and the emergency codes.
fn squawk(seed: u32) -> u16 {
    let d = |k: u32| ((seed >> (3 * k)) & 7) as u16;
    (2 + d(0) % 4) * 1000 + d(1) * 100 + d(2) * 10 + d(3)
}

/// a frequency as written: 118.700.
pub fn mhz(f: f32) -> String {
    format!("{f:.3}")
}

/// who said it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Who {
    Pilot,
    Atc,
    Info,
}

pub struct Line {
    /// game seconds when it was said.
    pub t: f64,
    pub who: Who,
    pub station: String,
    pub text: String,
}

/// how far the flight has come with the controllers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Ground,
    Cleared,
    Taxi,
    Holding,
    Takeoff,
    Departing,
    Enroute,
    Inbound,
    Landing,
    Rollout,
    TaxiIn,
}

/// a call the pilot can make.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Req {
    Clearance,
    Taxi,
    Ready,
    CheckIn,
    FlightFollowing,
    Inbound,
    LandingReq,
    TaxiParking,
    Advisory,
    AnnounceTaxi,
    AnnounceDeparture,
    AnnounceInbound,
    AnnounceFinal,
    AnnounceClear,
    Cancel,
}

impl Req {
    pub fn label(self) -> &'static str {
        match self {
            Req::Clearance => "Request departure clearance",
            Req::Taxi => "Request taxi",
            Req::Ready => "Ready for departure",
            Req::CheckIn => "Check in",
            Req::FlightFollowing => "Request flight following",
            Req::Inbound => "Request approach",
            Req::LandingReq => "Inbound for landing",
            Req::TaxiParking => "Request taxi to parking",
            Req::Advisory => "Request airport advisory",
            Req::AnnounceTaxi => "Announce taxi",
            Req::AnnounceDeparture => "Announce departure",
            Req::AnnounceInbound => "Announce inbound",
            Req::AnnounceFinal => "Announce final",
            Req::AnnounceClear => "Announce clear of the runway",
            Req::Cancel => "Cancel, resume own navigation",
        }
    }
}

/// what the controllers need to know of the flight, read fresh each time.
pub struct Ctx {
    pub now: f64,
    pub lat: f64,
    pub lon: f64,
    pub alt_ft: f64,
    pub agl_ft: f64,
    pub on_ground: bool,
    pub kt: f64,
    pub wind_from: f64,
    pub wind_kt: f64,
    pub qnh: f64,
    pub utc: f64,
    /// the flight plan's arrival and its cruise level (ft), if there is a plan.
    pub dest: Option<u32>,
    pub cruise_ft: Option<f64>,
    /// aircraft on the runway or on final at the field the player is at.
    pub runway_busy: bool,
    /// a jet: the departure clearance climbs higher.
    pub jet: bool,
}

/// a reply due: when, who, the station, the words shown. a call from the
/// automatic controllers also carries the words as said, its frequency (com 1
/// is tuned to it) and how well it comes in.
#[derive(Clone)]
struct Due {
    at: f64,
    who: Who,
    station: String,
    text: String,
    said: Option<String>,
    mhz: Option<f32>,
    signal: Option<f32>,
}

impl Due {
    fn reply(at: f64, who: Who, station: &str, text: String) -> Self {
        Due { at, who, station: station.into(), text, said: None, mhz: None, signal: None }
    }
}

pub struct Atc {
    pub on: bool,
    /// the controllers call the flight through on their own (`controller`).
    pub auto: bool,
    pub controller: controller::Controller,
    /// com 1, active and standby (mhz).
    pub com: f32,
    pub standby: f32,
    pub phase: Phase,
    pub log: Vec<Line>,
    /// replies due.
    pending: Vec<Due>,
    /// the departure airport, the runway given, the altitude given (ft).
    pub from: Option<u32>,
    pub rwy: Option<String>,
    pub alt_ft: Option<f64>,
    pub squawk: u16,
    /// the atis letter at each airport heard.
    atis: HashMap<u32, char>,
    pub violations: u32,
    /// ifr (a clearance was given) or vfr.
    pub ifr: bool,
    /// when the phase last changed, and when the last reminder went out.
    t_phase: f64,
    t_nag: f64,
    /// on the ground last step, and how long it has been in the air.
    ground: bool,
    up_for: f64,
    /// stopped on the ground this long (s).
    still: f64,
    /// what the pilot said into the mic, logged in place of the made up call.
    heard: Option<String>,
    /// the calls said out loud, when on; the tuned station's strength (0..1).
    pub voice: voice::Voice,
    pub speak: bool,
    signal: f32,
    /// when the signal was last worked out, and when the atis last began (game s).
    t_signal: f64,
    t_atis: f64,
}

impl Default for Atc {
    fn default() -> Self {
        Self { on: true, auto: false, controller: controller::Controller::new(""), com: 122.800, standby: 121.500, phase: Phase::Ground, log: Vec::new(), pending: Vec::new(), from: None, rwy: None, alt_ft: None, squawk: 1200, atis: HashMap::new(), violations: 0, ifr: false, t_phase: 0.0, t_nag: 0.0, ground: true, up_for: 0.0, still: 0.0, heard: None, voice: voice::Voice::default(), speak: false, signal: 1.0, t_signal: -10.0, t_atis: -1e9 }
    }
}

/// push to talk's state in the game: the mic, the words on their way back.
#[derive(Default)]
pub struct Ptt {
    mic: Option<listen::Mic>,
    ears: Option<crossbeam_channel::Receiver<Result<String, String>>>,
    warned: bool,
    /// the station called, for the subtitle while on the air.
    station: String,
}

impl Ptt {
    /// on the air now, or the call being written down.
    pub fn busy(&self) -> (bool, bool) {
        (self.mic.as_ref().is_some_and(|m| m.on), self.ears.is_some())
    }
}

/// how far a vhf call carries (nm): the radio horizon from the aircraft's height.
fn reach_nm(agl_ft: f64) -> f64 {
    (1.23 * agl_ft.max(0.0).sqrt()).max(25.0) + 10.0
}

fn dist_nm(a: &Apt, lat: f64, lon: f64) -> f64 {
    crate::flightplan::dist((a.lat, a.lon), (lat, lon)) / 1852.0
}

impl Atc {
    /// with the automatic controllers on or off (the saved setting).
    pub fn with_auto(auto: bool) -> Self {
        Self { auto, ..Default::default() }
    }

    /// starts over for a new flight, keeping the radio as tuned.
    pub fn reset(&mut self) {
        let (on, auto, com, standby) = (self.on, self.auto, self.com, self.standby);
        *self = Self { on, auto, com, standby, ..Default::default() };
    }

    fn set(&mut self, p: Phase, now: f64) {
        self.phase = p;
        self.t_phase = now;
    }

    fn say(&mut self, now: f64, who: Who, station: &str, text: String) {
        self.say_as(now, who, station, text, None, None);
    }

    /// as `say`, with the words as said when they differ from those shown
    /// (the automatic controllers write both), and the station's own signal.
    fn say_as(&mut self, now: f64, who: Who, station: &str, text: String, said: Option<&str>, signal: Option<f32>) {
        // the pilot's own words when they spoke the call, and then not said again.
        let spoke = who == Who::Pilot && self.heard.is_some();
        let text = if who == Who::Pilot { self.heard.take().unwrap_or(text) } else { text };
        if self.speak && who != Who::Info && !spoke {
            let signal = signal.unwrap_or(self.signal);
            match said {
                Some(w) => self.voice.speak_words(who == Who::Pilot, station, w, signal),
                None => self.voice.speak(who == Who::Pilot, station, &text, signal),
            }
        }
        self.log.push(Line { t: now, who, station: station.into(), text });
        if self.log.len() > 60 {
            self.log.remove(0);
        }
    }

    fn reply(&mut self, at: f64, station: &str, text: String) {
        self.pending.push(Due::reply(at, Who::Atc, station, text));
    }

    /// the stations near enough to hear, nearest airport first.
    pub fn nearby(c: &Ctx) -> Vec<(Freq, f64)> {
        let db = Db::get();
        let reach = reach_nm(c.agl_ft);
        let mut near = db.near(c.lat, c.lon, reach.min(80.0) * 1852.0);
        near.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut out = Vec::new();
        for (i, d) in near {
            let st = stations(i);
            if st.is_empty() {
                continue;
            }
            for f in st {
                out.push((f, d / 1852.0));
            }
            if out.len() > 40 {
                break;
            }
        }
        out
    }

    /// the station on the active frequency, the nearest one using it (a part
    /// time tower's frequency is also its ctaf: the tower first).
    pub fn tuned(&self, c: &Ctx) -> Option<Freq> {
        let rank = |k: Kind| match k {
            Kind::Twr => 0,
            Kind::Gnd | Kind::Del => 1,
            Kind::App | Kind::Dep | Kind::Ctr => 2,
            Kind::Atis => 3,
            _ => 4,
        };
        Self::nearby(c).into_iter().filter(|(f, _)| (f.mhz - self.com).abs() < 0.004).min_by(|a, b| a.1.total_cmp(&b.1).then(rank(a.0.kind).cmp(&rank(b.0.kind)))).map(|x| x.0)
    }

    fn atis_letter(&mut self, apt: u32, utc: f64) -> char {
        // a new report every hour.
        let hour = (utc.floor() as i64).rem_euclid(24) as u8;
        let seed = (apt as u64).wrapping_mul(2654435761) as u8;
        *self.atis.entry(apt).or_insert((b'A' + (seed.wrapping_add(hour)) % 26) as char)
    }

    /// what the atis is saying now.
    pub fn atis_text(&mut self, apt: u32, c: &Ctx) -> String {
        let a = &Db::get().apts[apt as usize];
        let letter = self.atis_letter(apt, c.utc);
        let rwy = runway_for(a, c.wind_from, c.wind_kt).unwrap_or_else(|| "?".into());
        let wind = if c.wind_kt < 2.0 { "calm".to_string() } else { format!("{:03.0} degrees {:.0} knots", (c.wind_from / 10.0).round() * 10.0, c.wind_kt) };
        format!(
            "{} information {}, time {:02}{:02} zulu. Wind {wind}. QNH {:.0}, altimeter {:.2}. Landing and departing runway {}. Advise on initial contact you have information {}.",
            place_name(a), PHONETIC[(letter as u8 - b'A') as usize], c.utc.floor() as u32 % 24, 0, c.qnh, c.qnh * 0.02953, say_rwy(&rwy), PHONETIC[(letter as u8 - b'A') as usize]
        )
    }

    /// the calls that make sense now on the frequency tuned.
    pub fn requests(&self, c: &Ctx) -> Vec<Req> {
        let Some(f) = self.tuned(c) else { return Vec::new() };
        let mut v = Vec::new();
        match f.kind {
            Kind::Del if c.on_ground && self.phase == Phase::Ground => v.push(Req::Clearance),
            Kind::Gnd if c.on_ground => match self.phase {
                Phase::Ground | Phase::Cleared => {
                    if self.phase == Phase::Ground && !stations(f.apt).iter().any(|s| s.kind == Kind::Del) {
                        v.push(Req::Clearance);
                    }
                    v.push(Req::Taxi);
                }
                Phase::Rollout | Phase::TaxiIn | Phase::Landing => v.push(Req::TaxiParking),
                _ => {}
            },
            Kind::Twr => {
                if c.on_ground && matches!(self.phase, Phase::Ground | Phase::Cleared | Phase::Taxi | Phase::Holding) {
                    v.push(Req::Ready);
                }
                if !c.on_ground && matches!(self.phase, Phase::Enroute | Phase::Inbound | Phase::Departing | Phase::Ground | Phase::Cleared | Phase::Taxi) {
                    v.push(Req::LandingReq);
                }
                if c.on_ground && matches!(self.phase, Phase::Rollout | Phase::Landing) {
                    v.push(Req::TaxiParking);
                }
            }
            Kind::Dep | Kind::App | Kind::Ctr if !c.on_ground => {
                v.push(Req::CheckIn);
                if !self.ifr && self.squawk == 1200 {
                    v.push(Req::FlightFollowing);
                }
                v.push(Req::Inbound);
                if self.squawk != 1200 {
                    v.push(Req::Cancel);
                }
            }
            Kind::Ctaf | Kind::Unic | Kind::Afis => {
                v.push(Req::Advisory);
                if c.on_ground {
                    v.push(Req::AnnounceTaxi);
                    v.push(Req::AnnounceDeparture);
                    v.push(Req::AnnounceClear);
                } else {
                    v.push(Req::AnnounceInbound);
                    v.push(Req::AnnounceFinal);
                }
            }
            _ => {}
        }
        if self.auto {
            v.retain(|r| matches!(r, Req::Advisory | Req::AnnounceTaxi | Req::AnnounceDeparture | Req::AnnounceInbound | Req::AnnounceFinal | Req::AnnounceClear));
        }
        v
    }

    /// the automatic controllers (`auto`): one step of the flight's phases;
    /// their calls are queued one after another, each `after` seconds behind
    /// the one before (the pilot reads back in between).
    pub fn drive(&mut self, s: &controller::Situation, dt: f64, cs: &str, now: f64) {
        if !self.on || !self.auto {
            return;
        }
        if self.controller.callsign != cs {
            self.controller.callsign = cs.to_string();
        }
        let mut at = self.pending.iter().map(|p| p.at).fold(now, f64::max);
        for call in self.controller.step(s, dt) {
            at += call.after;
            self.pending.push(Due { at, who: Who::Atc, station: call.station, text: call.phrase.shown, said: Some(call.phrase.said), mhz: Some(call.freq_khz as f32 / 1000.0), signal: Some(call.signal) });
        }
    }

    /// the pilot makes a call; the controller answers a moment later.
    pub fn call(&mut self, r: Req, cs: &str, c: &Ctx) {
        let Some(f) = self.tuned(c) else { return };
        let st = f.name();
        let db = Db::get();
        let a = &db.apts[f.apt as usize];
        let you = say(cs);
        let rwy = runway_for(a, c.wind_from, c.wind_kt).unwrap_or_else(|| "?".into());
        let wind = if c.wind_kt < 2.0 { "wind calm".to_string() } else { format!("wind {:03.0} at {:.0}", (c.wind_from / 10.0).round() * 10.0, c.wind_kt) };
        let qnh = format!("QNH {:.0}", c.qnh);
        let later = c.now + 2.2;
        let letter = self.atis_letter(f.apt, c.utc);
        let info = PHONETIC[(letter as u8 - b'A') as usize];
        let dest_name = c.dest.map(|d| db.apts[d as usize].ident.clone());
        let freq_of = |apt: u32, k: &[Kind]| stations(apt).into_iter().find(|s| k.contains(&s.kind));
        match r {
            Req::Clearance => {
                let to = dest_name.clone().unwrap_or_else(|| "the local area".into());
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, request clearance to {to}, with information {info}."));
                self.squawk = squawk(f.apt.wrapping_mul(2654435761).wrapping_add(c.now as u32));
                let climb = if c.jet { 5000.0 } else { 3000.0 };
                self.alt_ft = Some(climb + (a.elev as f64 / 1000.0).ceil() * 1000.0);
                self.ifr = c.dest.is_some();
                self.from = Some(f.apt);
                self.rwy = Some(rwy.clone());
                let dep = freq_of(f.apt, &[Kind::Dep, Kind::App]).map(|d| format!(", departure frequency {}", mhz(d.mhz))).unwrap_or_default();
                let alt = self.alt_ft.unwrap_or(climb);
                let text = if self.ifr {
                    format!("{you}, cleared to {to} as filed, runway {}, climb and maintain {alt:.0} feet, expect {} ten minutes after departure{dep}, squawk {:04}.", say_rwy(&rwy), c.cruise_ft.map(|x| format!("flight level {:03.0}", x / 100.0)).unwrap_or_else(|| "requested level".into()), self.squawk)
                } else {
                    format!("{you}, VFR departure approved, runway {}, maintain {alt:.0} feet or below{dep}, squawk {:04}.", say_rwy(&rwy), self.squawk)
                };
                self.reply(later, &st, text);
                self.set(Phase::Cleared, c.now);
            }
            Req::Taxi => {
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, at the ramp, request taxi, with information {info}."));
                self.rwy = Some(rwy.clone());
                self.from = Some(f.apt);
                self.reply(later, &st, format!("{you}, runway {}, taxi to holding point, {qnh}.", say_rwy(&rwy)));
                self.set(Phase::Taxi, c.now);
            }
            Req::Ready => {
                let rw = self.rwy.clone().unwrap_or(rwy);
                self.from = Some(f.apt);
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, holding point runway {}, ready for departure.", say_rwy(&rw)));
                if c.runway_busy {
                    self.reply(later, &st, format!("{you}, hold short runway {}, traffic on the runway. Expect departure shortly.", say_rwy(&rw)));
                    self.set(Phase::Holding, c.now);
                } else {
                    self.reply(later, &st, format!("{you}, {wind}, runway {}, cleared for takeoff.", say_rwy(&rw)));
                    self.set(Phase::Takeoff, c.now);
                }
                self.rwy = Some(rw);
            }
            Req::CheckIn => {
                let alt = (c.alt_ft / 100.0).round() * 100.0;
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, passing {alt:.0} feet."));
                let text = match (self.ifr, c.cruise_ft) {
                    (true, Some(cr)) => {
                        self.alt_ft = Some(cr);
                        format!("{you}, {st}, radar contact, climb and maintain flight level {:03.0}.", cr / 100.0)
                    }
                    _ => format!("{you}, {st}, radar contact, {qnh}."),
                };
                self.reply(later, &st, text);
                self.set(Phase::Enroute, c.now);
            }
            Req::FlightFollowing => {
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, {:.0} feet, request flight following.", (c.alt_ft / 100.0).round() * 100.0));
                self.squawk = squawk((c.now as u32).wrapping_mul(40503).wrapping_add(7));
                self.reply(later, &st, format!("{you}, squawk {:04}, radar contact, maintain VFR.", self.squawk));
                self.set(Phase::Enroute, c.now);
            }
            Req::Inbound => {
                let to = c.dest.unwrap_or(f.apt);
                let ta = &db.apts[to as usize];
                let r2 = runway_for(ta, c.wind_from, c.wind_kt).unwrap_or_else(|| "?".into());
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, {:.0} feet, inbound {}, request approach.", (c.alt_ft / 100.0).round() * 100.0, place_name(ta)));
                let down = ((ta.elev as f64 + 2500.0) / 500.0).round() * 500.0;
                self.alt_ft = Some(down.max(c.alt_ft.min(down)));
                let twr = freq_of(to, &[Kind::Twr, Kind::Ctaf, Kind::Unic]).map(|t| format!(" Contact {} {} at ten miles.", t.name(), mhz(t.mhz))).unwrap_or_default();
                let text = if self.ifr {
                    format!("{you}, descend and maintain {down:.0} feet, expect the approach for runway {}.{twr}", say_rwy(&r2))
                } else {
                    format!("{you}, proceed inbound, runway {} in use, {qnh}.{twr}", say_rwy(&r2))
                };
                self.reply(later, &st, text);
                self.rwy = Some(r2);
                self.set(Phase::Inbound, c.now);
            }
            Req::LandingReq => {
                let d = dist_nm(a, c.lat, c.lon);
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, {d:.0} miles out, inbound for landing, with information {info}."));
                self.rwy = Some(rwy.clone());
                if c.runway_busy {
                    self.reply(later, &st, format!("{you}, runway {}, continue approach, number two, traffic on the runway.", say_rwy(&rwy)));
                    self.set(Phase::Inbound, c.now);
                } else {
                    self.reply(later, &st, format!("{you}, runway {}, {wind}, cleared to land.", say_rwy(&rwy)));
                    self.set(Phase::Landing, c.now);
                }
            }
            Req::TaxiParking => {
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, clear of the runway, request taxi to parking."));
                self.reply(later, &st, format!("{you}, taxi to parking via the taxiways, {qnh}."));
                self.set(Phase::TaxiIn, c.now);
            }
            Req::Advisory => {
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, request airport advisory."));
                self.reply(later, &st, format!("{you}, {}, {wind}, favoured runway {}, {qnh}, no reported traffic.", place_name(a), say_rwy(&rwy)));
            }
            Req::AnnounceTaxi | Req::AnnounceDeparture | Req::AnnounceInbound | Req::AnnounceFinal | Req::AnnounceClear => {
                let p = place_name(a);
                let what = match r {
                    Req::AnnounceTaxi => format!("taxiing to runway {}", say_rwy(&rwy)),
                    Req::AnnounceDeparture => format!("departing runway {}", say_rwy(&rwy)),
                    Req::AnnounceInbound => format!("{:.0} miles out, inbound for landing", dist_nm(a, c.lat, c.lon)),
                    Req::AnnounceFinal => format!("final runway {}", say_rwy(&rwy)),
                    _ => format!("clear of runway {}", say_rwy(&rwy)),
                };
                self.say(c.now, Who::Pilot, &st, format!("{p} traffic, {you}, {what}, {p}."));
                // nobody answers the ctaf; a departure announced counts as cleared.
                match r {
                    Req::AnnounceDeparture => self.set(Phase::Takeoff, c.now),
                    Req::AnnounceFinal => self.set(Phase::Landing, c.now),
                    Req::AnnounceTaxi => self.set(Phase::Taxi, c.now),
                    _ => {}
                }
                self.rwy = Some(rwy);
            }
            Req::Cancel => {
                self.say(c.now, Who::Pilot, &st, format!("{st}, {you}, cancel services, resume own navigation."));
                self.squawk = 1200;
                self.reply(later, &st, format!("{you}, squawk VFR, frequency change approved."));
            }
        }
    }

    /// the pilot spoke on the frequency (push to talk): the call it sounds
    /// like is made, a readback is taken, anything else gets a say again.
    pub fn heard(&mut self, words: &str, cs: &str, c: &Ctx) {
        let words = words.trim();
        if words.is_empty() {
            return;
        }
        let Some(f) = self.tuned(c) else {
            self.heard = Some(words.to_string());
            self.say(c.now, Who::Pilot, "", String::new());
            return;
        };
        let st = f.name();
        if let Some(r) = listen::intent(words, &self.requests(c)) {
            self.heard = Some(words.to_string());
            self.call(r, cs, c);
            return;
        }
        let last = self.log.iter().rev().find(|l| l.who == Who::Atc && c.now - l.t < 60.0).map(|l| l.text.clone());
        self.heard = Some(words.to_string());
        self.say(c.now, Who::Pilot, &st, String::new());
        let you = say(cs);
        if let Some(l) = last.filter(|l| listen::readback(words, l)) {
            // a clearance read back is confirmed, the rest is just taken.
            if l.contains("squawk") {
                self.reply(c.now + 1.5, &st, format!("{you}, readback correct."));
            }
        } else if !matches!(f.kind, Kind::Ctaf | Kind::Unic | Kind::Atis) {
            self.reply(c.now + 1.5, &st, format!("{you}, say again."));
        }
    }

    /// once a step: replies come in, takeoffs and landings are seen, handoffs given.
    pub fn step(&mut self, dt: f64, cs: &str, c: &Ctx) {
        if !self.on {
            return;
        }
        if self.speak && c.now - self.t_signal > 1.0 {
            self.t_signal = c.now;
            let tuned = self.tuned(c);
            // weaker towards the edge of the radio horizon.
            if let Some(f) = &tuned {
                let d = dist_nm(&Db::get().apts[f.apt as usize], c.lat, c.lon);
                self.signal = (1.0 - 0.75 * (d / reach_nm(c.agl_ft)).powi(2)).clamp(0.2, 1.0) as f32;
            }
            // the atis is a recording that goes round while it is tuned.
            match tuned {
                Some(f) if f.kind == Kind::Atis => {
                    if c.now - self.t_atis > 50.0 {
                        self.t_atis = c.now;
                        let text = self.atis_text(f.apt, c);
                        self.voice.speak(false, &f.name(), &text, self.signal);
                    }
                }
                _ => self.t_atis = -1e9,
            }
        }
        // the replies due.
        let mut due: Vec<Due> = Vec::new();
        self.pending.retain(|p| {
            if p.at <= c.now {
                due.push(p.clone());
                false
            } else {
                true
            }
        });
        for d in due {
            // an automatic controller's call: com 1 is on its frequency.
            if let Some(f) = d.mhz {
                self.com = f;
            }
            self.say_as(c.now, d.who, &d.station, d.text, d.said.as_deref(), d.signal);
        }
        // the automatic controllers see to the takeoffs, landings and hand-offs.
        if self.auto {
            return;
        }
        let you = say(cs);
        // takeoff and landing, a short hop is a bounce.
        if !c.on_ground {
            self.up_for += dt;
        }
        let took_off = self.ground && !c.on_ground;
        let landed = !self.ground && c.on_ground && self.up_for > 8.0;
        self.ground = c.on_ground;
        if c.on_ground {
            if landed {
                self.up_for = 0.0;
            }
            self.still = if c.kt < 1.0 { self.still + dt } else { 0.0 };
        }
        let db = Db::get();
        let near = db.near(c.lat, c.lon, 6_000.0).into_iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0);
        if took_off {
            if let Some(apt) = near {
                if towered(apt) && self.phase != Phase::Takeoff {
                    self.violations += 1;
                    self.say(c.now, Who::Info, "", tr_en("Pilot deviation: took off without a takeoff clearance.").into());
                    let st = stations(apt).into_iter().find(|s| s.kind == Kind::Twr).map(|s| s.name()).unwrap_or_default();
                    self.say(c.now, Who::Atc, &st, format!("{you}, you departed without a takeoff clearance. Possible pilot deviation, advise you call the tower after landing."));
                }
            }
            self.set(Phase::Departing, c.now);
        }
        if landed {
            if let Some(apt) = near {
                if towered(apt) && self.phase != Phase::Landing {
                    self.violations += 1;
                    self.say(c.now, Who::Info, "", tr_en("Pilot deviation: landed without a landing clearance.").into());
                    let st = stations(apt).into_iter().find(|s| s.kind == Kind::Twr).map(|s| s.name()).unwrap_or_default();
                    self.say(c.now, Who::Atc, &st, format!("{you}, you landed without a landing clearance. Possible pilot deviation."));
                } else if let Some(t) = stations(apt).into_iter().find(|s| s.kind == Kind::Twr) {
                    let gnd = stations(apt).into_iter().find(|s| s.kind == Kind::Gnd).map(|g| format!(", contact ground {}", mhz(g.mhz))).unwrap_or_default();
                    self.reply(c.now + 3.0, &t.name(), format!("{you}, vacate the runway when able{gnd}."));
                }
            }
            self.set(Phase::Rollout, c.now);
        }
        // tower hands a departure on once it is clear of the field.
        if self.phase == Phase::Departing && !c.on_ground && (c.agl_ft > 1500.0 || c.now - self.t_phase > 120.0) {
            if let Some(apt) = self.from.filter(|&a| towered(a)) {
                let twr = stations(apt).into_iter().find(|s| s.kind == Kind::Twr);
                let dep = stations(apt).into_iter().find(|s| matches!(s.kind, Kind::Dep | Kind::App));
                if let Some(t) = twr {
                    let text = match dep {
                        Some(d) => format!("{you}, contact {} {}, good day.", d.name(), mhz(d.mhz)),
                        None => format!("{you}, frequency change approved, good day."),
                    };
                    if (t.mhz - self.com).abs() < 0.004 {
                        self.reply(c.now + 1.0, &t.name(), text);
                    }
                }
            }
            self.set(Phase::Enroute, c.now);
        }
        // a clearance waited out: the runway comes free.
        if self.phase == Phase::Holding && c.now - self.t_phase > 25.0 && !c.runway_busy {
            if let (Some(apt), Some(rw)) = (self.from, self.rwy.clone()) {
                if let Some(t) = stations(apt).into_iter().find(|s| s.kind == Kind::Twr) {
                    let wind = format!("wind {:03.0} at {:.0}", (c.wind_from / 10.0).round() * 10.0, c.wind_kt);
                    self.reply(c.now + 0.5, &t.name(), format!("{you}, {wind}, runway {}, cleared for takeoff.", say_rwy(&rw)));
                    self.set(Phase::Takeoff, c.now);
                }
            }
        }
        if self.phase == Phase::Inbound && !c.on_ground && c.now - self.t_phase > 30.0 {
            // number two: cleared once the runway is free.
            if let Some(apt) = near.or(c.dest) {
                if let Some(t) = stations(apt).into_iter().find(|s| s.kind == Kind::Twr).filter(|t| (t.mhz - self.com).abs() < 0.004 && !c.runway_busy) {
                    let rw = self.rwy.clone().unwrap_or_else(|| "?".into());
                    self.reply(c.now + 0.5, &t.name(), format!("{you}, runway {}, cleared to land.", say_rwy(&rw)));
                    self.set(Phase::Landing, c.now);
                }
            }
        }
        // parked again: the flight is over as far as the controllers go.
        if matches!(self.phase, Phase::TaxiIn | Phase::Rollout) && self.still > 30.0 {
            self.set(Phase::Ground, c.now);
        }
        // an altitude given and not kept.
        if let (Some(want), true) = (self.alt_ft, matches!(self.phase, Phase::Enroute | Phase::Inbound) && !c.on_ground) {
            if c.alt_ft > want + 500.0 && c.now - self.t_nag > 60.0 && self.ifr {
                self.t_nag = c.now;
                let st = self.tuned(c).map(|f| f.name()).unwrap_or_default();
                if !st.is_empty() {
                    self.say(c.now, Who::Atc, &st, format!("{you}, check altitude, maintain {want:.0} feet."));
                }
            }
        }
    }
}

/// the readings the controllers are given, from the game.
impl Game {
    pub(crate) fn atc_ctx(&self) -> Ctx {
        let (la, lo, h) = geo::pos_to_lla(self.plane.pos);
        let (lat, lon) = (la.to_degrees(), lo.to_degrees());
        let agl = self.world.ground(la, lo).map_or(h, |g| h - g.h);
        let busy = self.world.airfield_near(self.plane.pos, 6_000.0).is_some_and(|f| {
            f.traffic.movers.iter().any(|m| matches!(m.phase, crate::traffic::Phase::Final | crate::traffic::Phase::Roll | crate::traffic::Phase::Rollout))
        });
        let db = Db::get();
        let dest = self.plan.destination().and_then(|w| db.find(&w.ident));
        Ctx {
            now: self.time,
            lat,
            lon,
            alt_ft: h * 3.28084,
            agl_ft: agl * 3.28084,
            on_ground: self.plane.on_ground,
            kt: self.plane.vel.length() * 1.94384,
            wind_from: self.wx.wind_from,
            wind_kt: self.wx.wind * 1.94384,
            qnh: self.wx.qnh,
            utc: self.utc,
            dest,
            cruise_ft: (dest.is_some() && self.plan.cruise_ft > 0.0).then_some(self.plan.cruise_ft),
            runway_busy: busy,
            jet: self.spec.is_jet(),
        }
    }

    /// the callsign, set in the atc page or made up from the aircraft.
    pub(crate) fn callsign(&self) -> String {
        if !self.profile.callsign.trim().is_empty() {
            return self.profile.callsign.trim().to_uppercase();
        }
        if self.spec.is_jet() && matches!(self.spec.id, 1 | 6 | 9) {
            "FABLE 21".into()
        } else if self.spec.is_jet() {
            "RAVEN 1".into()
        } else {
            "N172FF".into()
        }
    }

    /// what the automatic controllers go by: where the aircraft is and what
    /// it is doing, the airfield it is at, the weather and the flight plan.
    pub(crate) fn atc_situation(&self) -> controller::Situation {
        let p = &self.plane;
        let (la, lo, alt) = geo::pos_to_lla(p.pos);
        let up = p.pos.normalize();
        let vs = p.vel.dot(up);
        let gs = (p.vel - up * vs).length();
        let agl = (alt - self.world.ground(la, lo).map_or(0.0, |g| g.h) - p.wheel_height()).max(0.0);
        let wind_kt = self.wx.wind * 1.94384;
        let field = self.world.airfield_near(p.pos, 18_000.0).map(|f| {
            let g = &f.geo;
            let runway = f.traffic.active.or_else(|| g.active(self.wx.wind_from as f32, wind_kt as f32)).map(|(ri, e)| g.runways[ri].ident[e].clone());
            let lp = g.local(p.pos);
            let (mut on_runway, mut at_hold) = (None, false);
            if p.on_ground {
                let fw = p.forward();
                let hd = glam::Vec2::new(fw.dot(g.e) as f32, fw.dot(g.n) as f32).normalize_or_zero();
                on_runway = g.on_runway(lp, 0.0).map(|ri| {
                    let r = &g.runways[ri];
                    r.ident[if hd.dot(r.dir) >= 0.0 { 0 } else { 1 }].clone()
                });
                at_hold = g.holds.iter().any(|h| (h.p - lp).length() < 40.0);
            }
            controller::Field { idx: g.idx, runway, on_runway, at_hold }
        });
        let dest = self.plan.destination().and_then(|w| Db::get().find(&w.ident));
        controller::Situation {
            lat: la.to_degrees(),
            lon: lo.to_degrees(),
            alt_ft: alt / 0.3048,
            agl_ft: agl / 0.3048,
            gs_kt: gs * 1.94384,
            vs_fpm: vs * 196.85,
            on_ground: p.on_ground,
            engine: p.running,
            field,
            wind_from: self.wx.wind_from,
            wind_kt,
            qnh: self.wx.qnh,
            dest,
            cruise_ft: if self.plan.is_empty() { 0.0 } else { self.plan.cruise_ft },
        }
    }

    pub(crate) fn atc_step(&mut self, dt: f64) {
        let c = self.atc_ctx();
        let cs = self.callsign();
        self.atc.speak = self.atc_voices && self.sound_on && self.audio.is_some() && tts::Tts::available();
        if !self.atc.speak {
            self.atc.voice.clear();
        }
        if self.atc.auto && self.atc.on {
            let s = self.atc_situation();
            self.atc.drive(&s, dt, &cs, c.now);
        }
        self.atc.step(dt, &cs, &c);
        self.atc.voice.pump(self.audio.as_ref());
        self.atc_ptt(&cs, &c);
    }

    /// push to talk: the mic records while the key is held, whisper writes the
    /// call down, and the controller on the frequency answers it.
    fn atc_ptt(&mut self, cs: &str, c: &Ctx) {
        let talk = crate::bind::ACTIONS.iter().find(|a| a.id == "atc_talk").is_some_and(|a| self.keys.contains(&a.key)) && self.atc.on && !self.pause_open;
        if talk && !tts::Tts::available() {
            if !self.ptt.warned {
                self.ptt.warned = true;
                self.status = crate::t!("game.atc_needs_voice_pack").into();
            }
            return;
        }
        if talk && !self.ptt.mic.as_ref().is_some_and(|m| m.on) {
            // on the air: whisper loads while the pilot talks, the station is kept for the subtitle.
            listen::Ears::get().warm();
            self.ptt.station = self.atc.tuned(c).map(|f| f.name()).unwrap_or_default();
        }
        if talk || self.ptt.mic.is_some() {
            self.ptt.mic.get_or_insert_with(listen::Mic::new).set(talk);
        }
        if let Some((rec, rate)) = self.ptt.mic.as_ref().and_then(|m| m.take()) {
            // a click of the key, not a call.
            if rec.len() as f32 > 0.35 * rate as f32 {
                self.ptt.ears = Some(listen::Ears::get().hear(rec, rate));
            }
        }
        if let Some(rx) = &self.ptt.ears {
            match rx.try_recv() {
                Ok(r) => {
                    self.ptt.ears = None;
                    match r {
                        Ok(words) => self.atc.heard(&words, cs, c),
                        Err(e) => log::warn!("push to talk: {e}"),
                    }
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => self.ptt.ears = None,
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
        }
    }

    /// the atc page of the flight panel: the com radio, the calls, what was said.
    pub(crate) fn atc_page(&mut self, ui: &mut egui::Ui) {
        use egui::{Color32, RichText};
        use flightui::{big, cells, key, note, pick_row, tag, wide_tile, LIT, SOFT};
        let c = self.atc_ctx();
        let tuned = self.atc.tuned(&c);
        let mut tune: Option<f32> = None;
        let mut swap = false;
        let mut step = 0.0f32;
        cells(ui, 2, 112.0, |ui, i| {
            if i == 0 {
                tag(ui, "COM 1");
                big(ui, &mhz(self.atc.com), "MHz", LIT);
                note(ui, &format!("{} {}", tr_en("standby"), mhz(self.atc.standby)));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    // whole megahertz and 25 khz steps, then the standby swapped in.
                    let w = ((ui.available_width() - 4.0 * 4.0) / 5.0).floor().max(44.0);
                    for (t, d) in [("−1", -1.0), ("−.025", -0.025), ("+.025", 0.025), ("+1", 1.0)] {
                        if key(ui, t, false, w, 26.0).clicked() {
                            step = d;
                        }
                    }
                    if key(ui, tr_en("SWAP"), false, w, 26.0).on_hover_text(tr_en("Swap the active and standby frequencies")).clicked() {
                        swap = true;
                    }
                });
            } else {
                tag(ui, tr_en("Station"));
                match &tuned {
                    Some(f) => {
                        ui.label(RichText::new(f.name()).font(menu::semibold(17.0)).color(Color32::WHITE));
                        note(ui, &format!("{}  ·  {}", Db::get().apts[f.apt as usize].ident, f.kind.code()));
                    }
                    None => {
                        ui.label(RichText::new(tr_en("Nobody on this frequency")).font(menu::semibold(15.0)).color(SOFT));
                        note(ui, tr_en("Tune a station from the list below."));
                    }
                }
                note(ui, &format!("{} {:04}  ·  {}", tr_en("squawk"), self.atc.squawk, if self.atc.ifr { "IFR" } else { "VFR" }));
                if self.atc.auto {
                    let phase = self.atc.controller.phase();
                    note(ui, &if phase.is_empty() { tr_en("Talk me through").to_string() } else { format!("{}  ·  {}", tr_en("Talk me through"), tr_en(phase)) });
                }
            }
        });
        // the atis reads itself out while it is tuned.
        if let Some(f) = tuned.as_ref().filter(|f| matches!(f.kind, Kind::Atis | Kind::Afis)) {
            let text = self.atc.atis_text(f.apt, &c);
            wide_tile(ui, |ui| {
                tag(ui, "ATIS");
                ui.add(egui::Label::new(RichText::new(text).font(menu::caption()).color(Color32::WHITE)).wrap());
            });
        }
        let reqs = self.atc.requests(&c);
        let mut asked: Option<Req> = None;
        if !reqs.is_empty() {
            wide_tile(ui, |ui| {
                tag(ui, tr_en("Call"));
                for r in &reqs {
                    if key(ui, tr_en(r.label()), false, ui.available_width(), 26.0).clicked() {
                        asked = Some(*r);
                    }
                }
            });
        }
        // what was said, the latest at the foot.
        if !self.atc.log.is_empty() {
            wide_tile(ui, |ui| {
                tag(ui, tr_en("Radio"));
                let n = self.atc.log.len();
                for l in &self.atc.log[n.saturating_sub(8)..] {
                    let col = match l.who {
                        Who::Pilot => LIT,
                        Who::Atc => Color32::WHITE,
                        Who::Info => Color32::from_rgb(255, 150, 120),
                    };
                    ui.add(egui::Label::new(RichText::new(&l.text).font(menu::caption()).color(col)).wrap());
                }
            });
        }
        // the stations in range, to tune with a click.
        let near = Atc::nearby(&c);
        if near.is_empty() {
            note(ui, tr_en("No stations in range."));
        } else {
            tag(ui, tr_en("Stations in range"));
            let mut last_apt = u32::MAX;
            for (f, d) in near.iter().take(16) {
                let a = &Db::get().apts[f.apt as usize];
                let right = if f.apt != last_apt { format!("{}  {d:.0} nm", a.ident) } else { String::new() };
                last_apt = f.apt;
                if pick_row(ui, &mhz(f.mhz), &f.name(), &right, (f.mhz - self.atc.com).abs() < 0.004).clicked() {
                    tune = Some(f.mhz);
                }
            }
        }
        ui.add_space(4.0);
        let hint = self.callsign();
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr_en("Callsign")).font(menu::caption()).color(SOFT));
            let r = ui.add(egui::TextEdit::singleline(&mut self.profile.callsign).desired_width(140.0).hint_text(hint).font(menu::caption()));
            if r.lost_focus() {
                self.profile.callsign = self.profile.callsign.trim().to_uppercase();
                self.profile.save();
            }
            menu::at_right(ui, "atc_on", |ui| {
                if key(ui, if self.atc.on { tr_en("ATC on") } else { tr_en("ATC off") }, self.atc.on, 80.0, 24.0).clicked() {
                    self.atc.on = !self.atc.on;
                }
                if key(ui, if self.ai.on { tr_en("AI traffic on") } else { tr_en("AI traffic off") }, self.ai.on, 110.0, 24.0).clicked() {
                    self.ai.on = !self.ai.on;
                    if !self.ai.on {
                        self.live.planes.retain(|p| !p.hex.starts_with("~ai"));
                        self.live.ai = false;
                    }
                }
            });
        });
        if self.atc.violations > 0 {
            note(ui, &format!("{}: {}", tr_en("Pilot deviations this flight"), self.atc.violations));
        }
        if self.ai.on {
            note(ui, &format!("{} {}", self.ai.count(), tr_en("game-managed flights in the area")));
        }
        if let Some(r) = asked {
            let cs = self.callsign();
            self.atc.call(r, &cs, &c);
        }
        if let Some(f) = tune {
            self.atc.com = f;
        }
        if swap {
            std::mem::swap(&mut self.atc.com, &mut self.atc.standby);
        }
        if step != 0.0 {
            let f = ((self.atc.com + step) * 1000.0 / 5.0).round() * 5.0 / 1000.0;
            self.atc.com = if f < 118.0 { 136.975 } else if f > 136.99 { 118.0 } else { f };
        }
    }

    /// the last thing said on the radio, at the bottom centre for a few seconds.
    pub(crate) fn atc_subtitles(&self, ctx: &egui::Context) {
        if !self.atc.on || self.info_open {
            return;
        }
        let (tx, rx) = self.ptt.busy();
        if tx || rx {
            // on the air: the station called, the pilot's words to come.
            let station = self.ptt.station.clone();
            let lift = flightui::bottom_stack(ctx, "atc_sub");
            egui::Area::new(egui::Id::unique("atc_sub")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -lift]).interactable(false).show(ctx, |ui| {
                flightui::panel(ui, flightui::glass().inner_margin(12.0), |ui| {
                    ui.horizontal(|ui| {
                        let (tag, col) = if tx { ("TX", egui::Color32::from_rgb(255, 90, 80)) } else { ("…", flightui::SOFT) };
                        ui.label(egui::RichText::new(tag).font(menu::semibold(12.0)).color(col));
                        ui.label(egui::RichText::new(station).font(menu::semibold(11.5)).color(flightui::SOFT));
                    });
                });
            });
            return;
        }
        let Some(l) = self.atc.log.last() else { return };
        let age = self.time - l.t;
        if !(0.0..9.0).contains(&age) {
            return;
        }
        let a = (1.0 - ((age - 8.0) / 1.0).clamp(0.0, 1.0)) as f32;
        let screen = ctx.content_rect();
        let col = match l.who {
            Who::Pilot => flightui::LIT,
            Who::Atc => egui::Color32::WHITE,
            Who::Info => egui::Color32::from_rgb(255, 150, 120),
        };
        let w = (screen.width() * 0.5).clamp(360.0, 760.0);
        // over whatever else sits at the bottom centre.
        let lift = flightui::bottom_stack(ctx, "atc_sub");
        egui::Area::new(egui::Id::unique("atc_sub")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -lift]).interactable(false).show(ctx, |ui| {
            ui.multiply_opacity(a);
            flightui::panel(ui, flightui::glass().inner_margin(12.0), |ui| {
                ui.set_max_width(w);
                if !l.station.is_empty() {
                    ui.label(egui::RichText::new(&l.station).font(menu::semibold(11.5)).color(flightui::SOFT));
                }
                ui.add(egui::Label::new(egui::RichText::new(&l.text).font(menu::medium(14.0)).color(col)).wrap());
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spoken_call_gets_its_answer() {
        let db = Db::get();
        let sfo = &db.apts[db.find("KSFO").expect("sfo") as usize];
        let gnd = stations(db.find("KSFO").unwrap()).into_iter().find(|f| f.kind == Kind::Gnd).expect("ground");
        let c = Ctx { now: 100.0, lat: sfo.lat, lon: sfo.lon, alt_ft: 13.0, agl_ft: 0.0, on_ground: true, kt: 0.0, wind_from: 280.0, wind_kt: 10.0, qnh: 1013.0, utc: 12.0, dest: None, cruise_ft: None, runway_busy: false, jet: false };
        let mut a = Atc { com: gnd.mhz, ..Default::default() };
        let said = "San Francisco Ground, Fable 123, at the ramp, request taxi with information alpha.";
        a.heard(said, "FABLE 123", &c);
        let l = a.log.last().expect("the call");
        assert_eq!((l.who, l.text.as_str()), (Who::Pilot, said), "the pilot's own words are logged");
        assert_eq!(a.phase, Phase::Taxi);
        a.step(0.1, "FABLE 123", &Ctx { now: 103.0, ..c });
        assert!(a.log.last().is_some_and(|l| l.who == Who::Atc && l.text.contains("taxi")), "ground answers");
        // a readback is taken without a say again; mumbling gets one.
        let n = a.log.len();
        a.heard("taxi to holding point runway two eight right fable one two three", "FABLE 123", &Ctx { now: 105.0, ..c });
        a.step(0.1, "FABLE 123", &Ctx { now: 110.0, ..c });
        assert!(a.log[n..].iter().all(|l| !l.text.contains("say again")));
        a.heard("uh hello", "FABLE 123", &Ctx { now: 120.0, ..c });
        a.step(0.1, "FABLE 123", &Ctx { now: 125.0, ..c });
        assert!(a.log.last().is_some_and(|l| l.text.ends_with("say again.")));
    }

    /// talk me through: the controllers call on their own, one after the
    /// other, on the airport's real frequencies, and com 1 follows them; the
    /// pilot is left only the ctaf calls.
    #[test]
    fn talk_me_through_calls_and_tunes() {
        let db = Db::get();
        let sfo = db.find("KSFO").expect("sfo");
        let a = &db.apts[sfo as usize];
        let c = |now: f64| Ctx { now, lat: a.lat, lon: a.lon, alt_ft: 13.0, agl_ft: 0.0, on_ground: true, kt: 0.0, wind_from: 290.0, wind_kt: 9.0, qnh: 1013.0, utc: 12.0, dest: None, cruise_ft: None, runway_busy: false, jet: false };
        let mut atc = Atc::with_auto(true);
        let field = controller::Field { idx: sfo, runway: Some("28R".into()), ..Default::default() };
        let mut s = controller::Situation { lat: a.lat, lon: a.lon, alt_ft: 13.0, on_ground: true, engine: true, field: Some(field), wind_from: 290.0, wind_kt: 9.0, qnh: 1013.0, ..Default::default() };
        let mut now = 0.0;
        let run = |atc: &mut Atc, s: &controller::Situation, secs: f64, now: &mut f64| {
            for _ in 0..(secs * 2.0) as usize {
                *now += 0.5;
                atc.drive(s, 0.5, "N172SP", *now);
                atc.step(0.5, "N172SP", &c(*now));
            }
        };
        run(&mut atc, &s, 10.0, &mut now);
        let gnd = stations(sfo).into_iter().find(|f| f.kind == Kind::Gnd).expect("ground");
        let l = atc.log.last().expect("ground calls");
        assert!(l.who == Who::Atc && l.station == gnd.name() && l.text.contains("taxi to holding point"), "{}: {}", l.station, l.text);
        assert!((atc.com - gnd.mhz).abs() < 0.001, "com 1 on ground: {}", atc.com);
        assert!(atc.requests(&c(now)).is_empty(), "the tower's calls are the controllers'");
        // at the hold: the tower clears the takeoff; the climb out goes to departure.
        s.field.as_mut().unwrap().at_hold = true;
        run(&mut atc, &s, 5.0, &mut now);
        assert!(atc.log.last().is_some_and(|l| l.text.contains("cleared for takeoff")));
        (s.field, s.on_ground, s.agl_ft, s.alt_ft, s.gs_kt) = (None, false, 1_200.0, 1_200.0, 100.0);
        run(&mut atc, &s, 2.0, &mut now);
        let n = atc.log.len();
        assert!(atc.log.last().is_some_and(|l| l.text.contains("contact")), "the tower hands over first");
        run(&mut atc, &s, 10.0, &mut now);
        assert!(atc.log.len() == n + 1 && atc.log[n].text.contains("radar contact"), "then departure, a moment later");
        assert_eq!(atc.violations, 0, "the takeoff was cleared");
    }

    #[test]
    fn heathrow_has_a_tower_and_callsigns_read_out() {
        let db = Db::get();
        let egll = db.find("EGLL").expect("heathrow");
        assert!(towered(egll));
        assert!(stations(egll).iter().any(|f| f.kind == Kind::Gnd));
        assert_eq!(say("N172FF"), "November One Seven Two Foxtrot Foxtrot");
        assert_eq!(say("FABLE 21"), "Fable Two One");
        let rwy = runway_for(&db.apts[egll as usize], 270.0, 15.0).expect("runway");
        assert!(rwy.starts_with("27"), "{rwy}");
    }
}
