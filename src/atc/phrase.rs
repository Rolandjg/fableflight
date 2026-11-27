//! ATC messages, written twice as they are put together: the way they are
//! shown ("FAB123, runway 28R, cleared for takeoff, wind 290/12") and the way
//! they are said ("Fable one two three, runway two eight right, cleared for
//! takeoff, wind two niner zero degrees, one two knots"), in the ICAO style:
//! digits one at a time, "niner", whole thousands, "flight level".

/// A message being built: what is shown, and what the voice says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Phrase {
    pub shown: String,
    pub said: String,
}

const DIGIT: [&str; 10] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "niner"];

/// Digits said one at a time ("280" → "two eight zero").
pub fn digits(s: &str) -> String {
    s.chars().filter_map(|c| c.to_digit(10)).map(|d| DIGIT[d as usize]).collect::<Vec<_>>().join(" ")
}

/// "Fable 123" → "Fable one two three"; letters on their own are spelled
/// with the phonetic alphabet ("N172SP" → "November one seven two Sierra Papa").
pub fn callsign_said(c: &str) -> String {
    const ALPHA: [&str; 26] = [
        "Alfa", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliett", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango", "Uniform", "Victor",
        "Whiskey", "X-ray", "Yankee", "Zulu",
    ];
    let mut out: Vec<String> = Vec::new();
    for word in c.split_whitespace() {
        // a word of letters is a name ("Fable"), unless it is a lone letter, a
        // registration ("F-GJKL") or runs into digits ("N172SP"): then every
        // character is spelled
        let spell = word.chars().any(|ch| ch.is_ascii_digit() || ch == '-') || word.chars().count() == 1;
        if !spell {
            out.push(word.to_owned());
            continue;
        }
        for ch in word.chars() {
            if let Some(d) = ch.to_digit(10) {
                out.push(DIGIT[d as usize].into());
            } else if ch.is_ascii_alphabetic() {
                out.push(ALPHA[(ch.to_ascii_uppercase() as u8 - b'A') as usize].into());
            }
        }
    }
    out.join(" ")
}

/// A runway ident: "28R" → "two eight right", "09" → "zero niner".
pub fn runway_said(id: &str) -> String {
    let num: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    let side = match id.trim_start_matches(|c: char| c.is_ascii_digit()).to_ascii_uppercase().as_str() {
        "L" => " left",
        "R" => " right",
        "C" => " center",
        _ => "",
    };
    format!("{}{side}", digits(&format!("{num:0>2}")))
}

/// An altitude in feet: below the transition, whole thousands and hundreds
/// ("one zero thousand", "three thousand five hundred"); above it, a flight
/// level. `transition` is in feet (18,000 in the US, 5,000 to 13,000 elsewhere).
pub fn altitude(ft: f64, transition: f64) -> (String, String) {
    if ft >= transition {
        let fl = (ft / 100.0).round() as i64;
        return (format!("FL{fl:03}"), format!("flight level {}", digits(&format!("{fl:03}"))));
    }
    let ft = ((ft / 100.0).round() * 100.0).max(0.0) as i64;
    let (th, hu) = (ft / 1000, (ft % 1000) / 100);
    let mut said = Vec::new();
    if th > 0 {
        // ten thousand and above is said digit by digit: "one zero thousand"
        said.push(if th >= 10 { digits(&th.to_string()) } else { DIGIT[th as usize].replace("niner", "nine") });
        said.push("thousand".into());
    }
    if hu > 0 {
        said.push(DIGIT[hu as usize].replace("niner", "nine"));
        said.push("hundred".into());
    }
    if said.is_empty() {
        said.push("zero".into());
    }
    let shown = if ft >= 1000 { format!("{},{:03}", ft / 1000, ft % 1000) } else { ft.to_string() };
    (format!("{shown} ft"), said.join(" "))
}

impl Phrase {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, shown: &str, said: &str) -> &mut Self {
        let glue = |s: &mut String, t: &str| {
            if !s.is_empty() && !t.starts_with([',', '.', '?']) {
                s.push(' ');
            }
            s.push_str(t);
        };
        glue(&mut self.shown, shown);
        glue(&mut self.said, said);
        self
    }

    /// Words shown and said alike ("cleared for takeoff").
    pub fn t(&mut self, text: &str) -> &mut Self {
        self.push(text, text)
    }
    /// A pause: "," or ".".
    pub fn p(&mut self, mark: &str) -> &mut Self {
        self.push(mark, mark)
    }
    pub fn call(&mut self, callsign: &str) -> &mut Self {
        self.push(callsign, &callsign_said(callsign))
    }
    pub fn runway(&mut self, id: &str) -> &mut Self {
        self.push(&format!("runway {id}"), &format!("runway {}", runway_said(id)))
    }
    /// (for radar vectors, which the controllers don't give yet)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn heading(&mut self, deg: f64) -> &mut Self {
        let h = ((deg.rem_euclid(360.0) / 5.0).round() * 5.0) as i64;
        let h = if h == 0 { 360 } else { h };
        self.push(&format!("heading {h:03}"), &format!("heading {}", digits(&format!("{h:03}"))))
    }
    pub fn altitude(&mut self, ft: f64, transition: f64) -> &mut Self {
        let (shown, said) = altitude(ft, transition);
        self.push(&shown, &said)
    }
    /// The surface wind, from `from` degrees at `kt` knots.
    pub fn wind(&mut self, from: f64, kt: f64) -> &mut Self {
        if kt < 2.5 {
            return self.push("wind calm", "wind calm");
        }
        let d = ((from.rem_euclid(360.0) / 10.0).round() * 10.0) as i64;
        let d = if d == 0 { 360 } else { d };
        let k = kt.round() as i64;
        self.push(&format!("wind {d:03}/{k}"), &format!("wind {} degrees, {} knots", digits(&format!("{d:03}")), digits(&k.to_string())))
    }
    /// A frequency in kHz (118700 → "118.700" shown, "one one eight point seven" said).
    pub fn freq(&mut self, khz: u32) -> &mut Self {
        let (mhz, frac) = (khz / 1000, khz % 1000);
        let tail = format!("{frac:03}");
        let tail = tail.trim_end_matches('0');
        let said = format!("{} point {}", digits(&mhz.to_string()), if tail.is_empty() { "zero".into() } else { digits(tail) });
        self.push(&format!("{mhz}.{frac:03}"), &said)
    }
    /// The altimeter setting: inches of mercury in North America
    /// ("altimeter two niner niner two"), hectopascals elsewhere ("QNH one zero one three").
    pub fn altimeter(&mut self, hpa: f64, inches: bool) -> &mut Self {
        if inches {
            let v = (hpa * 0.029_529_98 * 100.0).round() as i64;
            self.push(&format!("altimeter {}.{:02}", v / 100, v % 100), &format!("altimeter {}", digits(&v.to_string())))
        } else {
            let v = hpa.round() as i64;
            self.push(&format!("QNH {v}"), &format!("Q N H {}", digits(&v.to_string())))
        }
    }

    pub fn done(&mut self) -> Phrase {
        let mut p = std::mem::take(self);
        for s in [&mut p.shown, &mut p.said] {
            if !s.ends_with(['.', '?', '!']) {
                s.push('.');
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_the_icao_way() {
        assert_eq!(digits("290"), "two niner zero");
        assert_eq!(runway_said("28R"), "two eight right");
        assert_eq!(runway_said("9"), "zero niner");
        assert_eq!(runway_said("36C"), "three six center");
        assert_eq!(altitude(10_000.0, 18_000.0).1, "one zero thousand");
        assert_eq!(altitude(3_500.0, 18_000.0), ("3,500 ft".into(), "three thousand five hundred".into()));
        assert_eq!(altitude(9_000.0, 18_000.0).1, "nine thousand");
        assert_eq!(altitude(35_000.0, 18_000.0), ("FL350".into(), "flight level three five zero".into()));
        assert_eq!(altitude(7_000.0, 6_000.0).1, "flight level zero seven zero");
    }

    #[test]
    fn callsigns() {
        assert_eq!(callsign_said("Fable 123"), "Fable one two three");
        assert_eq!(callsign_said("N172SP"), "November one seven two Sierra Papa");
        assert_eq!(callsign_said("F-GJKL"), "Foxtrot Golf Juliett Kilo Lima");
        assert_eq!(callsign_said("F G J K L"), "Foxtrot Golf Juliett Kilo Lima");
    }

    #[test]
    fn a_takeoff_clearance() {
        let p = Phrase::new().call("Fable 123").p(",").t("San Francisco Tower").p(",").wind(290.0, 12.0).p(",").runway("28R").p(",").t("cleared for takeoff").done();
        assert_eq!(p.shown, "Fable 123, San Francisco Tower, wind 290/12, runway 28R, cleared for takeoff.");
        assert_eq!(p.said, "Fable one two three, San Francisco Tower, wind two niner zero degrees, one two knots, runway two eight right, cleared for takeoff.");
    }

    #[test]
    fn frequencies_headings_altimeters() {
        let p = Phrase::new().t("contact").freq(118_700).p(",").heading(3.0).p(",").altimeter(1013.2, false).p(",").altimeter(1013.2, true).done();
        assert_eq!(p.shown, "contact 118.700, heading 005, QNH 1013, altimeter 29.92.");
        assert_eq!(p.said, "contact one one eight point seven, heading zero zero five, Q N H one zero one three, altimeter two niner niner two.");
        assert_eq!(Phrase::new().freq(121_000).done().said, "one two one point zero.");
        assert_eq!(Phrase::new().freq(124_350).done().said, "one two four point three five.");
        assert_eq!(Phrase::new().heading(359.0).done().shown, "heading 360.");
        assert_eq!(Phrase::new().wind(10.0, 1.0).done().said, "wind calm.");
    }
}
