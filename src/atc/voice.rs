//! the calls said out loud: each line of the text atc goes to the voice
//! thread, then on the radio in the order it was said. a controller keeps one
//! voice and one radio character per station, the pilot has their own.

use super::lexicon::DIGITS as DIGIT;
use super::tts::{Speech, Tts, VOICES};
use crate::audio::radio::{Station, Transmission};
use crossbeam_channel::{Receiver, TryRecvError};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// a voice later than this is dropped, the words stay on screen.
const LATE: Duration = Duration::from_secs(12);

/// the pilot's own voice.
const PILOT: &str = "am_michael";

struct Item {
    rx: Receiver<Result<Speech, String>>,
    st: Station,
    signal: f32,
    seed: u32,
    since: Instant,
}

#[derive(Default)]
pub struct Voice {
    queue: VecDeque<Item>,
}

impl Voice {
    /// queues one line. `station` is the name the controller goes by, empty for none.
    pub fn speak(&mut self, pilot: bool, station: &str, text: &str, signal: f32) {
        self.speak_words(pilot, station, &spoken(text), signal);
    }

    /// as `speak`, the words already written as they are said.
    pub fn speak_words(&mut self, pilot: bool, station: &str, words: &str, signal: f32) {
        if words.trim().is_empty() || self.queue.len() >= 8 {
            return;
        }
        let seed = hash(station);
        let (voice, speed, st) = if pilot { (PILOT, 1.05, Station::RADAR) } else { (controller(seed), 1.15, filter(station)) };
        let rx = Tts::get().say(words, voice, speed);
        self.queue.push_back(Item { rx, st, signal: if pilot { 1.0 } else { signal.clamp(0.15, 1.0) }, seed: seed ^ self.queue.len() as u32, since: Instant::now() });
    }

    /// sends what is ready, in order, to the radio. a late or failed voice is skipped.
    pub fn pump(&mut self, audio: Option<&crate::audio::Audio>) {
        while let Some(it) = self.queue.front() {
            match it.rx.try_recv() {
                Ok(Ok(s)) => {
                    let it = self.queue.pop_front().expect("front");
                    if let Some(a) = audio {
                        a.radio(Transmission::new(s.samples.to_vec(), s.rate, it.st, it.signal, it.seed));
                    }
                }
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                    self.queue.pop_front();
                }
                Err(TryRecvError::Empty) if it.since.elapsed() > LATE => {
                    self.queue.pop_front();
                }
                Err(TryRecvError::Empty) => return,
            }
        }
    }

    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

fn hash(s: &str) -> u32 {
    s.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619))
}

/// a controller's voice: any but the pilot's, the same for a station every time.
fn controller(seed: u32) -> &'static str {
    let v: Vec<&'static str> = VOICES.iter().copied().filter(|v| *v != PILOT).collect();
    v[seed as usize % v.len()]
}

/// how the station sounds: ground and delivery gritty, the tower firm, radar clean.
fn filter(station: &str) -> Station {
    let last = station.rsplit(' ').next().unwrap_or("");
    match last {
        "Ground" | "Delivery" => Station::GROUND,
        "Tower" | "Traffic" | "Unicom" | "Information" => Station::TOWER,
        _ => Station::RADAR,
    }
}

/// plain numbers say nine, not niner.
const ONES: [&str; 10] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
const TEENS: [&str; 10] = ["ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"];
const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];

fn digits(s: &str) -> String {
    s.chars().filter_map(|c| c.to_digit(10)).map(|d| DIGIT[d as usize]).collect::<Vec<_>>().join(" ")
}

/// 0..99 as words.
fn small(n: u32) -> String {
    match n {
        0..=9 => ONES[n as usize].into(),
        10..=19 => TEENS[(n - 10) as usize].into(),
        _ if n % 10 == 0 => TENS[(n / 10) as usize].into(),
        _ => format!("{} {}", TENS[(n / 10) as usize], small(n % 10)),
    }
}

/// an altitude in feet as atc says it: "three thousand five hundred", "one one thousand".
fn feet(n: u32) -> String {
    let (th, hu) = (n / 1000, (n % 1000) / 100);
    let mut out = Vec::new();
    if th > 0 {
        out.push(if th >= 10 { digits(&th.to_string()) } else { DIGIT[th as usize].into() });
        out.push("thousand".to_string());
    }
    if hu > 0 {
        out.push(DIGIT[hu as usize].into());
        out.push("hundred".into());
    }
    if out.is_empty() {
        out.push(small(n % 100));
    }
    out.join(" ")
}

/// the text as it is said: no written forms in brackets, numbers in words
/// the way atc reads them (altitudes in thousands, the rest digit by digit,
/// distances as plain numbers).
pub fn spoken(text: &str) -> String {
    // drop "(28L)" and the like: the words before them already say it.
    let mut s = String::with_capacity(text.len());
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(toks.len());
    for (i, tok) in toks.iter().enumerate() {
        let core = tok.trim_end_matches([',', '.', ';', ':']);
        let punct = &tok[core.len()..];
        let next = toks.get(i + 1).map(|t| t.trim_end_matches([',', '.']).to_lowercase()).unwrap_or_default();
        let word = if core.is_empty() || !core.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            core.to_string()
        } else if let Some((a, b)) = core.split_once('.') {
            // a frequency: "118.700" is "one one eight point seven".
            let b = b.trim_end_matches('0');
            let b = if b.is_empty() { "0" } else { b };
            format!("{} point {}", digits(a), digits(b))
        } else if !core.chars().all(|c| c.is_ascii_digit()) {
            // a runway or a fix like "28L": digit by digit, the letters as they are.
            core.chars().map(|c| c.to_digit(10).map(|d| DIGIT[d as usize].to_string()).unwrap_or_else(|| c.to_string())).collect::<Vec<_>>().join(" ")
        } else {
            let n: u32 = core.parse().unwrap_or(0);
            if next == "feet" {
                feet(n)
            } else if next == "miles" && n < 100 {
                small(n)
            } else {
                digits(core)
            }
        };
        out.push(format!("{word}{punct}"));
    }
    out.join(" ").replace(" feet", "").replace(" ,", ",").replace(" .", ".").replace("  ", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_numbers_as_atc_does() {
        assert_eq!(spoken("runway two eight left (28L), cleared for takeoff."), "runway two eight left, cleared for takeoff.");
        assert_eq!(spoken("climb and maintain 3500 feet, squawk 4521."), "climb and maintain three thousand five hundred, squawk four five two one.");
        assert_eq!(spoken("maintain 11000 feet"), "maintain one one thousand");
        assert_eq!(spoken("contact Oakland Center 133.200."), "contact Oakland Center one three three point two.");
        assert_eq!(spoken("wind 290 at 12, QNH 1013."), "wind two niner zero at one two, QNH one zero one three.");
        assert_eq!(spoken("10 miles out"), "ten miles out");
        assert_eq!(spoken("flight level 350"), "flight level three five zero");
    }

    #[test]
    fn stations_keep_their_voice() {
        assert_eq!(controller(hash("Heathrow Tower")), controller(hash("Heathrow Tower")));
        assert_ne!(controller(hash("x")), PILOT);
        assert_eq!(filter("San Francisco Ground").drive, Station::GROUND.drive);
        assert_eq!(filter("Oakland Center").drive, Station::RADAR.drive);
    }
}
