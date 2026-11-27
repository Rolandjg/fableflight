//! The controllers: who talks to the player, and when, from a flight's
//! phases. A state machine over what the aircraft is doing (`Situation`,
//! taken each frame), handing the player from one controller to the next:
//!
//! ```text
//! Parked ─engine running─▶ Taxi ─at the hold/on the runway─▶ Takeoff
//!   ─airborne─▶ Departure ─far out or high─▶ Enroute ─near the destination─▶ Approach
//!   ─close in and low─▶ Landing ─down and slow─▶ TaxiIn ─engine off─▶ Parked
//!                         └─climbing away─▶ GoAround ─back down─▶ Landing
//! ```
//!
//! Only airports of medium size and up have a tower (OurAirports' kinds);
//! from a small field the player meets the centre once airborne. Starting in
//! the air, or after a jump to another place, the machine picks up where the
//! flight is: on a final approach the tower clears it to land.
//!
//! The stations are the airport's real ones where OurAirports lists them
//! (`atc::stations`, the same the COM radio tunes), named as the text ATC
//! names them ("San Francisco Tower"). An airport without one for the job
//! gets a made-up but plausible frequency, always the same for the same
//! station. The centre is always made up (the data has too few): named after
//! the nearest large airport's town, "Center" in North America and "Control"
//! elsewhere.

use super::phrase::Phrase;
use crate::airports::{self, Db};
use crate::flightplan;

/// The airfield the aircraft is at or close to, with its layout loaded (world.rs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    /// index into `airports::Db`
    pub idx: u32,
    /// the runway end in use ("28R")
    pub runway: Option<String>,
    /// the runway end the aircraft is on, facing
    pub on_runway: Option<String>,
    /// at a holding point of a runway
    pub at_hold: bool,
}

/// What the aircraft is doing, each frame.
#[derive(Clone, Debug, Default)]
pub struct Situation {
    pub lat: f64,
    pub lon: f64,
    pub alt_ft: f64,
    pub agl_ft: f64,
    pub gs_kt: f64,
    pub vs_fpm: f64,
    pub on_ground: bool,
    pub engine: bool,
    pub field: Option<Field>,
    /// surface wind (degrees from, knots) and the QNH (hPa)
    pub wind_from: f64,
    pub wind_kt: f64,
    pub qnh: f64,
    /// the flight plan's destination airport, and its cruise altitude (0: none)
    pub dest: Option<u32>,
    pub cruise_ft: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Facility {
    Ground,
    Tower,
    Departure,
    Approach,
    Center,
}

/// One transmission from a controller.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub facility: Facility,
    /// "San Francisco Tower"
    pub station: String,
    pub freq_khz: u32,
    pub phrase: Phrase,
    /// how far after the previous call it comes, s (a pilot reads back in between)
    pub after: f64,
    /// signal strength, 0..1
    pub signal: f32,
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    /// not sorted out yet (a new flight, or a jump)
    Start,
    /// on the ground at a towered airport, waiting for the engine
    Parked { apt: u32, still: f64 },
    /// cleared to taxi to the runway
    Taxi { apt: u32, rwy: String },
    Takeoff { apt: u32, rwy: String },
    Departure { apt: u32 },
    Enroute { center: String },
    Approach { apt: u32, rwy: String },
    Landing { apt: u32, rwy: String, lowest: f64 },
    GoAround { apt: u32, rwy: String, since: f64 },
    TaxiIn { apt: u32, still: f64 },
    /// on the ground where nobody controls (a small field)
    Uncontrolled,
}

pub struct Controller {
    pub callsign: String,
    state: State,
    /// seconds since the start, and when the state last changed
    t: f64,
    since: f64,
    last: Option<(f64, f64)>,
}

/// How far out a destination's approach controller takes over, m.
const APPROACH_RANGE: f64 = 55_000.0;
/// Inside this distance and height the tower clears the aircraft to land.
const TOWER_RANGE: f64 = 16_000.0;
const TOWER_AGL: f64 = 4_000.0;
/// The departure controller lets go this far out, or this high.
const DEPARTURE_RANGE: f64 = 45_000.0;
const DEPARTURE_TOP: f64 = 12_000.0;

impl Controller {
    pub fn new(callsign: &str) -> Self {
        Controller { callsign: callsign.to_owned(), state: State::Start, t: 0.0, since: 0.0, last: None }
    }

    /// The phase, for the radio page ("Taxi", "Approach").
    pub fn phase(&self) -> &'static str {
        match self.state {
            State::Start | State::Uncontrolled => "",
            State::Parked { .. } => "Parked",
            State::Taxi { .. } => "Taxi",
            State::Takeoff { .. } => "Takeoff",
            State::Departure { .. } => "Departure",
            State::Enroute { .. } => "En route",
            State::Approach { .. } => "Approach",
            State::Landing { .. } | State::GoAround { .. } => "Landing",
            State::TaxiIn { .. } => "Taxi in",
        }
    }

    fn go(&mut self, s: State) {
        self.state = s;
        self.since = self.t;
    }

    /// Moves the flight on by `dt` seconds; what the controllers say.
    pub fn step(&mut self, s: &Situation, dt: f64) -> Vec<Call> {
        self.t += dt;
        // A jump (a new place picked, a respawn): start over.
        if let Some(prev) = self.last {
            if flightplan::dist(prev, (s.lat, s.lon)) > 3_000.0 + 400.0 * dt {
                self.state = State::Start;
            }
        }
        self.last = Some((s.lat, s.lon));
        let mut out = Vec::new();
        let state = self.state.clone();
        let held = self.t - self.since;
        match state {
            State::Start => self.start(s, &mut out),
            State::Uncontrolled => {
                if !s.on_ground && s.agl_ft > 1_500.0 {
                    let center = center_name(s.lat, s.lon);
                    self.center_hello(s, &center, 3.0, &mut out);
                    self.go(State::Enroute { center });
                } else if s.on_ground && tower_at(s).is_some() {
                    self.go(State::Start);
                }
            }
            State::Parked { apt, still } => {
                let still = if s.engine && s.gs_kt < 3.0 { still + dt } else { 0.0 };
                if !s.on_ground {
                    self.airborne_from(apt, s, &mut out);
                } else if still > 4.0 {
                    let rwy = s.field.as_ref().and_then(|f| f.runway.clone()).unwrap_or_else(|| runway_for(apt, s));
                    let (name, f) = station(apt, Facility::Ground);
                    let us = inches(apt);
                    let p = Phrase::new().call(&self.callsign).p(",").t(&name).p(",").runway(&rwy).p(",").t("taxi to holding point").p(",").altimeter(s.qnh, us).done();
                    out.push(call(Facility::Ground, &name, f, p, 1.0, 1.0));
                    self.go(State::Taxi { apt, rwy });
                } else {
                    self.state = State::Parked { apt, still };
                }
            }
            State::Taxi { apt, rwy } => {
                let field = s.field.as_ref();
                let ready = field.is_some_and(|f| (f.at_hold && s.gs_kt < 5.0) || f.on_runway.is_some());
                if !s.on_ground {
                    self.airborne_from(apt, s, &mut out);
                } else if ready {
                    let rwy = field.and_then(|f| f.on_runway.clone()).unwrap_or(rwy);
                    let (name, f) = station(apt, Facility::Tower);
                    let p = Phrase::new().call(&self.callsign).p(",").t(&name).p(",").wind(s.wind_from, s.wind_kt).p(",").runway(&rwy).p(",").t("cleared for takeoff").done();
                    out.push(call(Facility::Tower, &name, f, p, 1.5, 1.0));
                    self.go(State::Takeoff { apt, rwy });
                }
            }
            State::Takeoff { apt, .. } => {
                if !s.on_ground && s.agl_ft > 600.0 {
                    self.airborne_from(apt, s, &mut out);
                } else if s.on_ground && s.gs_kt < 3.0 && held > 120.0 {
                    // never went: back to the start of it
                    self.go(State::Parked { apt, still: 0.0 });
                }
            }
            State::Departure { apt } => {
                let a = &Db::get().apts[apt as usize];
                let far = flightplan::dist((a.lat, a.lon), (s.lat, s.lon));
                if let Some(d) = self.arriving(s).filter(|&d| d != apt || far > 15_000.0) {
                    self.approach_from(Facility::Departure, apt, None, d, s, &mut out);
                } else if far > DEPARTURE_RANGE || s.alt_ft > DEPARTURE_TOP {
                    let (dep, f) = station(apt, Facility::Departure);
                    let center = center_name(s.lat, s.lon);
                    let (_, cf) = center_station(&center);
                    let p = Phrase::new().call(&self.callsign).p(",").t("contact").t(&center).freq(cf).p(",").t("good day").done();
                    out.push(call(Facility::Departure, &dep, f, p, 1.0, 0.9));
                    self.center_hello(s, &center, 7.0, &mut out);
                    self.go(State::Enroute { center });
                } else if s.on_ground && s.gs_kt < 40.0 {
                    self.go(State::TaxiIn { apt, still: 0.0 });
                }
            }
            State::Enroute { center } => {
                if let Some(d) = self.arriving(s) {
                    self.approach_from(Facility::Center, 0, Some(&center), d, s, &mut out);
                } else if held > 900.0 {
                    // a new centre further on: a hand-off (not more than one
                    // every quarter of an hour: sectors are big)
                    let next = center_name(s.lat, s.lon);
                    if next != center {
                        let cf_old = center_station(&center).1;
                        let (_, cf) = center_station(&next);
                        let p = Phrase::new().call(&self.callsign).p(",").t("contact").t(&next).freq(cf).p(",").t("good day").done();
                        out.push(call(Facility::Center, &center, cf_old, p, 1.0, 0.75));
                        let hello = Phrase::new().call(&self.callsign).p(",").t(&next).p(",").t("radar contact").done();
                        out.push(call(Facility::Center, &next, cf, hello, 7.0, 0.75));
                        self.go(State::Enroute { center: next });
                    } else {
                        // still the same one: look again in a minute
                        self.since = self.t - 840.0;
                    }
                }
                if s.on_ground && s.gs_kt < 3.0 {
                    self.go(State::Start);
                }
            }
            State::Approach { apt, rwy } => {
                let a = &Db::get().apts[apt as usize];
                let d = flightplan::dist((a.lat, a.lon), (s.lat, s.lon));
                if d < TOWER_RANGE && s.agl_ft < TOWER_AGL {
                    let rwy = s.field.as_ref().filter(|f| f.idx == apt).and_then(|f| f.runway.clone()).unwrap_or(rwy);
                    let (app, af) = station(apt, Facility::Approach);
                    let (twr, tf) = station(apt, Facility::Tower);
                    let p = Phrase::new().call(&self.callsign).p(",").t("contact").t(&twr).freq(tf).done();
                    out.push(call(Facility::Approach, &app, af, p, 1.0, 1.0));
                    let p = Phrase::new().call(&self.callsign).p(",").t(&twr).p(",").runway(&rwy).p(",").t("cleared to land").p(",").wind(s.wind_from, s.wind_kt).done();
                    out.push(call(Facility::Tower, &twr, tf, p, 6.0, 1.0));
                    self.go(State::Landing { apt, rwy, lowest: s.agl_ft });
                } else if d > APPROACH_RANGE * 1.5 {
                    // flew away from it
                    let center = center_name(s.lat, s.lon);
                    self.go(State::Enroute { center });
                }
            }
            State::Landing { apt, rwy, lowest } => {
                let lowest = lowest.min(s.agl_ft);
                if s.on_ground && s.gs_kt < 50.0 {
                    let (twr, tf) = station(apt, Facility::Tower);
                    let (gnd, gf) = station(apt, Facility::Ground);
                    let p = Phrase::new().call(&self.callsign).p(",").t("vacate the runway when able").p(",").t("contact ground").freq(gf).done();
                    out.push(call(Facility::Tower, &twr, tf, p, 2.0, 1.0));
                    let p = Phrase::new().call(&self.callsign).p(",").t(&gnd).p(",").t("taxi to parking").done();
                    out.push(call(Facility::Ground, &gnd, gf, p, 7.0, 1.0));
                    self.go(State::TaxiIn { apt, still: 0.0 });
                } else if !s.on_ground && lowest < 800.0 && s.agl_ft > lowest + 400.0 && s.vs_fpm > 300.0 {
                    // climbing away: a go-around
                    let (twr, tf) = station(apt, Facility::Tower);
                    let p = Phrase::new().call(&self.callsign).p(",").t("going around").p(",").t("fly runway heading").p(",").t("climb and maintain").altitude(field_elev_ft(apt) + 2_000.0, 18_000.0).done();
                    out.push(call(Facility::Tower, &twr, tf, p, 1.0, 1.0));
                    self.go(State::GoAround { apt, rwy, since: self.t });
                } else {
                    self.state = State::Landing { apt, rwy, lowest };
                }
            }
            State::GoAround { apt, rwy, since } => {
                let back_down = self.t - since > 45.0 && s.vs_fpm < -200.0 && s.agl_ft < 2_000.0;
                if back_down {
                    let (twr, tf) = station(apt, Facility::Tower);
                    let p = Phrase::new().call(&self.callsign).p(",").runway(&rwy).p(",").t("cleared to land").p(",").wind(s.wind_from, s.wind_kt).done();
                    out.push(call(Facility::Tower, &twr, tf, p, 1.0, 1.0));
                    self.go(State::Landing { apt, rwy, lowest: s.agl_ft });
                } else if s.on_ground && s.gs_kt < 50.0 {
                    self.go(State::TaxiIn { apt, still: 0.0 });
                } else if s.agl_ft > 5_000.0 {
                    self.go(State::Departure { apt });
                }
            }
            State::TaxiIn { apt, still } => {
                // parked with the engine off, or long still: a new flight can begin
                let still = if s.gs_kt < 2.0 { still + dt } else { 0.0 };
                if !s.engine && still > 5.0 || still > 90.0 {
                    self.go(State::Parked { apt, still: 0.0 });
                } else if !s.on_ground && s.agl_ft > 600.0 {
                    self.airborne_from(apt, s, &mut out);
                } else {
                    self.state = State::TaxiIn { apt, still };
                }
            }
        }
        out
    }

    /// Where the flight is when the machine starts (or after a jump).
    fn start(&mut self, s: &Situation, out: &mut Vec<Call>) {
        if s.on_ground {
            match tower_at(s) {
                Some(f) => {
                    let apt = f.idx;
                    if f.on_runway.is_some() {
                        // lined up on the runway: cleared for takeoff once the engine runs
                        let rwy = f.on_runway.clone().unwrap();
                        self.go(State::Taxi { apt, rwy });
                    } else {
                        self.go(State::Parked { apt, still: 0.0 });
                    }
                }
                None => self.go(State::Uncontrolled),
            }
            return;
        }
        if let Some(d) = self.arriving(s) {
            let a = &Db::get().apts[d as usize];
            if flightplan::dist((a.lat, a.lon), (s.lat, s.lon)) < TOWER_RANGE && s.agl_ft < TOWER_AGL {
                // on a final approach: straight to the tower
                let rwy = s.field.as_ref().filter(|f| f.idx == d).and_then(|f| f.runway.clone()).unwrap_or_else(|| runway_for(d, s));
                let (twr, tf) = station(d, Facility::Tower);
                let p = Phrase::new().call(&self.callsign).p(",").t(&twr).p(",").runway(&rwy).p(",").t("cleared to land").p(",").wind(s.wind_from, s.wind_kt).done();
                out.push(call(Facility::Tower, &twr, tf, p, 3.0, 1.0));
                self.go(State::Landing { apt: d, rwy, lowest: s.agl_ft });
            } else {
                let rwy = runway_for(d, s);
                let (app, af) = station(d, Facility::Approach);
                let p = self.approach_hello(&app, d, &rwy, s);
                out.push(call(Facility::Approach, &app, af, p, 3.0, signal_from(d, s)));
                self.go(State::Approach { apt: d, rwy });
            }
            return;
        }
        let center = center_name(s.lat, s.lon);
        self.center_hello(s, &center, 3.0, out);
        self.go(State::Enroute { center });
    }

    /// The airport the flight is coming in to, once close enough: the
    /// plan's destination, or a towered field it is descending towards.
    fn arriving(&self, s: &Situation) -> Option<u32> {
        let db = Db::get();
        if let Some(d) = s.dest.filter(|&d| db.apts[d as usize].kind >= 1) {
            let a = &db.apts[d as usize];
            let dist = flightplan::dist((a.lat, a.lon), (s.lat, s.lon));
            return (dist < APPROACH_RANGE.max(s.agl_ft * 1.8)).then_some(d);
        }
        if s.agl_ft < 5_000.0 && s.vs_fpm < -300.0 {
            return db.near(s.lat, s.lon, 30_000.0).into_iter().map(|(i, _)| i).find(|&i| db.apts[i as usize].kind >= 1);
        }
        None
    }

    /// Off the ground from `apt`: departure takes over (the tower first, from a towered field).
    fn airborne_from(&mut self, apt: u32, s: &Situation, out: &mut Vec<Call>) {
        let (twr, tf) = station(apt, Facility::Tower);
        let (dep, df) = station(apt, Facility::Departure);
        let p = Phrase::new().call(&self.callsign).p(",").t("contact").t(&dep).freq(df).p(",").t("good day").done();
        out.push(call(Facility::Tower, &twr, tf, p, 1.0, 1.0));
        let elev = field_elev_ft(apt);
        let target = if s.cruise_ft > 0.0 { s.cruise_ft.min(elev + 10_000.0) } else if s.gs_kt > 160.0 { elev + 10_000.0 } else { elev + 4_000.0 };
        let target = ((target / 500.0).round() * 500.0).max(elev + 2_000.0);
        let p = Phrase::new().call(&self.callsign).p(",").t(&dep).p(",").t("radar contact").p(",").t("climb and maintain").altitude(target, transition(apt)).done();
        out.push(call(Facility::Departure, &dep, df, p, 7.0, 1.0));
        self.go(State::Departure { apt });
    }

    /// Handed to the destination's approach controller by `from` (the
    /// departure controller of `from_apt`, or the centre `center`).
    fn approach_from(&mut self, from: Facility, from_apt: u32, center: Option<&str>, d: u32, s: &Situation, out: &mut Vec<Call>) {
        let (app, af) = station(d, Facility::Approach);
        let (who, wf) = match from {
            Facility::Departure => station(from_apt, Facility::Departure),
            _ => {
                let c = center.map_or_else(|| center_name(s.lat, s.lon), str::to_owned);
                let f = center_station(&c).1;
                (c, f)
            }
        };
        let p = Phrase::new().call(&self.callsign).p(",").t("contact").t(&app).freq(af).done();
        out.push(call(from, &who, wf, p, 1.0, 0.8));
        let rwy = runway_for(d, s);
        let p = self.approach_hello(&app, d, &rwy, s);
        out.push(call(Facility::Approach, &app, af, p, 7.0, signal_from(d, s)));
        self.go(State::Approach { apt: d, rwy });
    }

    fn approach_hello(&self, app: &str, d: u32, rwy: &str, s: &Situation) -> Phrase {
        let mut p = Phrase::new();
        p.call(&self.callsign).p(",").t(app).p(",").t("expect").runway(rwy).p(",");
        let down_to = field_elev_ft(d) + 3_000.0;
        let down_to = (down_to / 500.0).round() * 500.0;
        if s.alt_ft > down_to + 500.0 {
            p.t("descend and maintain").altitude(down_to, transition(d)).p(",");
        }
        p.altimeter(s.qnh, inches(d)).done()
    }

    fn center_hello(&self, s: &Situation, center: &str, after: f64, out: &mut Vec<Call>) {
        let (_, cf) = center_station(center);
        let mut p = Phrase::new();
        p.call(&self.callsign).p(",").t(center).p(",").t("radar contact");
        if s.cruise_ft > s.alt_ft + 1_000.0 {
            let tl = if center.ends_with("Center") { 18_000.0 } else { 6_000.0 };
            p.p(",").t("climb and maintain").altitude(s.cruise_ft, tl);
        }
        out.push(call(Facility::Center, center, cf, p.done(), after, 0.75));
    }
}

fn call(facility: Facility, station: &str, freq_khz: u32, phrase: Phrase, after: f64, signal: f32) -> Call {
    Call { facility, station: station.to_owned(), freq_khz, phrase, after, signal }
}

/// The towered field the aircraft is on, if any.
fn tower_at(s: &Situation) -> Option<&Field> {
    s.field.as_ref().filter(|f| Db::get().apts[f.idx as usize].kind >= 1)
}

fn field_elev_ft(apt: u32) -> f64 {
    Db::get().apts[apt as usize].elev as f64 / airports::FT as f64
}

/// North America sets altimeters in inches and has its transition at 18,000 ft.
fn inches(apt: u32) -> bool {
    matches!(Db::get().apts[apt as usize].country.as_str(), "US" | "CA")
}

fn transition(apt: u32) -> f64 {
    if inches(apt) { 18_000.0 } else { 6_000.0 }
}

/// How well an approach controller near `apt` comes in.
fn signal_from(apt: u32, s: &Situation) -> f32 {
    let a = &Db::get().apts[apt as usize];
    let d = flightplan::dist((a.lat, a.lon), (s.lat, s.lon));
    (1.0 - d / 150_000.0).clamp(0.55, 1.0) as f32
}

/// The runway end to expect at `apt` before its layout is loaded: the most
/// into the wind of the longest paved runways in the database.
pub fn runway_for(apt: u32, s: &Situation) -> String {
    let a = &Db::get().apts[apt as usize];
    let mut best: Option<(f64, &str)> = None;
    for r in &a.rwys {
        for id in &r.ident {
            let Some(h) = airports::ident_heading(id) else { continue };
            let head = ((h as f64 - s.wind_from).to_radians()).cos() * s.wind_kt.max(0.5);
            let score = r.len as f64 * if r.paved { 1.0 } else { 0.5 } + head * 600.0;
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, id));
            }
        }
    }
    best.map(|b| b.1.to_owned()).or_else(|| a.rwys.first().map(|r| r.ident[0].clone())).unwrap_or_else(|| "in use".into())
}

/// What a station at `apt` is called ("San Francisco Tower"), and its
/// frequency (kHz): the real one when the airport lists it (departure falls
/// back on approach and the other way round), else a made-up one.
pub fn station(apt: u32, fac: Facility) -> (String, u32) {
    use super::Kind;
    let want: &[Kind] = match fac {
        Facility::Ground => &[Kind::Gnd],
        Facility::Tower => &[Kind::Twr],
        Facility::Departure => &[Kind::Dep, Kind::App],
        Facility::Approach => &[Kind::App, Kind::Dep],
        Facility::Center => &[],
    };
    let real = super::stations(apt);
    if let Some(f) = want.iter().find_map(|k| real.iter().find(|f| f.kind == *k)) {
        return (f.name(), (f.mhz * 1000.0).round() as u32);
    }
    let a = &Db::get().apts[apt as usize];
    let town = super::place_name(a);
    let what = match fac {
        Facility::Ground => "Ground",
        Facility::Tower => "Tower",
        Facility::Departure => "Departure",
        Facility::Approach => "Approach",
        Facility::Center => "Center",
    };
    let h = hash(&format!("{}{what}", a.ident));
    let khz = match fac {
        // ground 121.600–121.975, tower 118.000–120.975, approach and departure 119.000–127.975
        Facility::Ground => 121_600 + (h % 16) as u32 * 25,
        Facility::Tower => 118_000 + (h % 120) as u32 * 25,
        _ => 119_000 + (h % 360) as u32 * 25,
    };
    (format!("{town} {what}"), khz)
}

/// The centre an aircraft at (lat, lon) talks to: named after the nearest
/// large airport's town within 600 km.
pub fn center_name(lat: f64, lon: f64) -> String {
    let db = Db::get();
    let near = db.near(lat, lon, 600_000.0).into_iter().find(|&(i, _)| db.apts[i as usize].kind >= 2);
    match near {
        Some((i, _)) => {
            let a = &db.apts[i as usize];
            let town = if a.city.trim().is_empty() { a.name.as_str() } else { a.city.as_str() };
            let town = town.split(['/', ',', '(']).next().unwrap_or(town).trim();
            let what = if matches!(a.country.as_str(), "US" | "CA") { "Center" } else { "Control" };
            format!("{town} {what}")
        }
        None => "Oceanic Control".into(),
    }
}

/// A centre's made-up frequency: 124.000–135.975.
pub fn center_station(name: &str) -> (String, u32) {
    (name.to_owned(), 124_000 + (hash(name) % 480) as u32 * 25)
}

/// FNV-1a: the same station gets the same frequency and voice every time.
pub fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sfo() -> u32 {
        Db::get().find("KSFO").expect("KSFO in the database")
    }

    fn at(apt: u32) -> Situation {
        let a = &Db::get().apts[apt as usize];
        Situation { lat: a.lat, lon: a.lon, alt_ft: field_elev_ft(apt), wind_from: 290.0, wind_kt: 12.0, qnh: 1013.0, on_ground: true, ..Default::default() }
    }

    /// Runs `s` for `secs` in steps of half a second; everything said.
    fn run(c: &mut Controller, s: &Situation, secs: f64) -> Vec<Call> {
        let mut out = Vec::new();
        for _ in 0..(secs * 2.0) as usize {
            out.extend(c.step(s, 0.5));
        }
        out
    }

    fn said(calls: &[Call]) -> String {
        calls.iter().map(|c| format!("[{}] {}", c.station, c.phrase.shown)).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_whole_flight_out_of_sfo() {
        let apt = sfo();
        let mut c = Controller::new("Fable 123");
        let mut s = at(apt);
        s.field = Some(Field { idx: apt, runway: Some("28R".into()), on_runway: None, at_hold: false });
        // parked, engine off: nothing
        assert!(run(&mut c, &s, 10.0).is_empty());
        assert_eq!(c.phase(), "Parked");
        // engine on: ground clears us to the runway in use
        s.engine = true;
        let calls = run(&mut c, &s, 6.0);
        assert_eq!(calls.len(), 1, "{}", said(&calls));
        assert_eq!(calls[0].facility, Facility::Ground);
        assert!(calls[0].phrase.shown.contains("runway 28R") && calls[0].phrase.shown.contains("altimeter 29.91"), "{}", said(&calls));
        assert_eq!(calls[0].station, "San Francisco Ground");
        // at the hold, stopped: cleared for takeoff
        s.field.as_mut().unwrap().at_hold = true;
        let calls = run(&mut c, &s, 2.0);
        assert!(calls.len() == 1 && calls[0].phrase.shown.contains("cleared for takeoff") && calls[0].phrase.shown.contains("wind 290/12"), "{}", said(&calls));
        // airborne: tower hands us to departure, who climbs us
        s.field = None;
        s.on_ground = false;
        s.agl_ft = 1_200.0;
        s.alt_ft = 1_200.0;
        s.gs_kt = 100.0;
        let calls = run(&mut c, &s, 1.0);
        assert_eq!(calls.iter().map(|c| c.facility).collect::<Vec<_>>(), [Facility::Tower, Facility::Departure], "{}", said(&calls));
        assert!(calls[0].phrase.shown.contains("contact San Francisco Departure"), "{}", said(&calls));
        assert!(calls[1].phrase.shown.contains("climb and maintain 4,000 ft"), "{}", said(&calls));
        // high up: the centre
        s.alt_ft = 13_000.0;
        s.agl_ft = 13_000.0;
        let calls = run(&mut c, &s, 1.0);
        assert!(calls.len() == 2 && calls[1].facility == Facility::Center && calls[1].station.ends_with("Center"), "{}", said(&calls));
        assert_eq!(c.phase(), "En route");
    }

    #[test]
    fn coming_in_to_a_destination() {
        let apt = sfo();
        let a = &Db::get().apts[apt as usize];
        let mut c = Controller::new("N172SP");
        // 40 km east of San Francisco at 8,000 ft, going there
        let mut s = Situation { lat: a.lat, lon: a.lon + 0.45, alt_ft: 8_000.0, agl_ft: 8_000.0, gs_kt: 140.0, dest: Some(apt), wind_from: 290.0, wind_kt: 12.0, qnh: 1013.0, ..Default::default() };
        let calls = run(&mut c, &s, 1.0);
        assert!(calls.len() == 1 && calls[0].facility == Facility::Approach, "{}", said(&calls));
        assert!(calls[0].phrase.shown.contains("expect runway 28") && calls[0].phrase.shown.contains("descend and maintain 3,000 ft"), "{}", said(&calls));
        assert!(calls[0].phrase.said.starts_with("November one seven two Sierra Papa"));
        // close in and low: the tower clears us to land (flown there, not a jump)
        c.last = None;
        s.lon = a.lon + 0.1;
        s.alt_ft = 2_000.0;
        s.agl_ft = 2_000.0;
        s.vs_fpm = -500.0;
        let calls = run(&mut c, &s, 1.0);
        assert!(calls.len() == 2 && calls[1].facility == Facility::Tower && calls[1].phrase.shown.contains("cleared to land"), "{}", said(&calls));
        // down and slowing: off the runway, then ground
        s.on_ground = true;
        s.agl_ft = 0.0;
        s.gs_kt = 30.0;
        s.vs_fpm = 0.0;
        let calls = run(&mut c, &s, 1.0);
        assert!(calls.len() == 2 && calls[0].phrase.shown.contains("vacate") && calls[1].phrase.shown.contains("taxi to parking"), "{}", said(&calls));
        // engine off at the stand: ready for another flight
        s.gs_kt = 0.0;
        s.engine = false;
        run(&mut c, &s, 8.0);
        assert_eq!(c.phase(), "Parked");
    }

    #[test]
    fn going_around() {
        let apt = sfo();
        let a = &Db::get().apts[apt as usize];
        let mut c = Controller::new("Fable 123");
        let mut s = Situation { lat: a.lat, lon: a.lon + 0.05, alt_ft: 1_500.0, agl_ft: 1_500.0, gs_kt: 90.0, vs_fpm: -600.0, dest: Some(apt), wind_from: 290.0, wind_kt: 5.0, qnh: 1013.0, ..Default::default() };
        let calls = run(&mut c, &s, 1.0);
        assert!(calls.len() == 1 && calls[0].phrase.shown.contains("cleared to land"), "starting on final: {}", said(&calls));
        s.agl_ft = 200.0;
        s.alt_ft = 200.0;
        run(&mut c, &s, 1.0);
        s.agl_ft = 900.0;
        s.alt_ft = 900.0;
        s.vs_fpm = 1_200.0;
        let calls = run(&mut c, &s, 1.0);
        assert!(calls.len() == 1 && calls[0].phrase.shown.contains("going around"), "{}", said(&calls));
    }

    #[test]
    fn a_jump_starts_over() {
        let apt = sfo();
        let mut c = Controller::new("Fable 123");
        let mut s = at(apt);
        s.field = Some(Field { idx: apt, runway: Some("28R".into()), ..Default::default() });
        run(&mut c, &s, 2.0);
        assert_eq!(c.phase(), "Parked");
        // to the middle of nowhere, in the air
        s = Situation { lat: 40.0, lon: -100.0, alt_ft: 9_000.0, agl_ft: 6_000.0, gs_kt: 150.0, qnh: 1013.0, ..Default::default() };
        let calls = run(&mut c, &s, 1.0);
        assert_eq!(c.phase(), "En route");
        assert!(calls.len() == 1 && calls[0].facility == Facility::Center, "{}", said(&calls));
    }

    #[test]
    fn stations_are_stable_and_plausible() {
        let apt = sfo();
        let (name, f) = station(apt, Facility::Tower);
        assert_eq!((name.as_str(), f), (station(apt, Facility::Tower).0.as_str(), station(apt, Facility::Tower).1));
        assert!((118_000..121_000).contains(&f) && f % 25 == 0);
        assert!((121_600..122_000).contains(&station(apt, Facility::Ground).1));
        assert!(center_name(51.5, -0.1).ends_with("Control"), "{}", center_name(51.5, -0.1));
        assert!(center_name(37.6, -122.4).ends_with("Center"));
    }
}
