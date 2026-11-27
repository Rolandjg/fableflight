//! The main menu's music: four chord progressions (assets/audio/menu/chords*.ogg,
//! made in LMMS with the Vital synth) chained in a random order, so it never
//! settles into one loop, with now and then a light aircraft flying over
//! (flyby*.ogg).
//!
//! The progressions are 110 bpm, two of 24 bars and two of 16, each followed
//! by about two and a half seconds of its last chord dying away. The next one
//! comes in on the bar the last one's music ends (a whole number of four-bar
//! phrases from its start), with that release ringing on under it, so the
//! chain stays in time. The music fades in over its first few seconds and
//! out when the menu closes; it starts again from a fresh progression the
//! next time the menu opens.
//!
//! The flybys come 30 s to a minute apart, each at a level picked between a
//! quiet floor and a modest ceiling. The recordings are mono passes that
//! already swell and fade; each is played slightly faster or slower (another
//! aircraft, another speed), faded in from silence and out again round its
//! closest point (the recording's loudest moment, give or take two seconds),
//! panned across from the left or the right with the middle at that moment,
//! and dulled while far off, as the air soaks up the highs of a distant
//! aircraft. There is no Doppler shift added: the recordings carry their own.
//!
//! Sources: the progressions are the project's own. The flybys, going by
//! their file names, are Pixabay downloads (Pixabay Content License; the
//! licence of each is worth confirming on its page): "Small propeller plane
//! flyby" (kalsstockmedia, 341994), "Single prop plane flyover 3" (flutie8211,
//! 641804), "Plane flying overhead" (freesound_community, 70830). Encoded
//! with `ffmpeg -c:a libvorbis` (-q:a 5 stereo progressions, -q:a 4 mono
//! flybys, the 48 kHz one resampled to 44.1 kHz).
use super::{kt, pan, Bq, Rng, RATE};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

const CHORDS: [&[u8]; 4] = [
    include_bytes!("../../assets/audio/menu/chords1.ogg"),
    include_bytes!("../../assets/audio/menu/chords2.ogg"),
    include_bytes!("../../assets/audio/menu/chords3.ogg"),
    include_bytes!("../../assets/audio/menu/chords4.ogg"),
];
const FLYBYS: [&[u8]; 3] = [
    include_bytes!("../../assets/audio/menu/flyby1.ogg"),
    include_bytes!("../../assets/audio/menu/flyby2.ogg"),
    include_bytes!("../../assets/audio/menu/flyby3.ogg"),
];

/// One four-bar phrase at 110 bpm, s: the grid the progressions are cut to.
const PHRASE: f64 = 4.0 * 4.0 * 60.0 / 110.0;
/// The music's fade in when the menu opens, and out when it closes, s.
const FADE_IN: f32 = 9.0;
const FADE_OUT: f32 = 1.5;
/// The music's level (the progressions are mastered with peaks near -2 dBFS).
const MUSIC_GAIN: f32 = 0.85;
/// A flyby's loudness at its closest, against a recording levelled to a peak of 1: picked between these.
const FLYBY_MIN: f32 = 0.10;
const FLYBY_MAX: f32 = 0.30;
/// Time from one flyby starting to the next, s (the first comes a little sooner).
const FLYBY_GAP: (f32, f32) = (30.0, 60.0);
const FLYBY_FIRST: (f32, f32) = (18.0, 40.0);
/// How far the loudest moment may sit from the recording's own, s.
const APEX_JITTER: f32 = 2.0;
/// The low-pass on a flyby: open at its closest, this low far off, Hz.
const LP_NEAR: f32 = 16000.0;
const LP_FAR: f32 = 1100.0;
/// Filters, pan and fades are updated every this many samples.
const TICK: u32 = 64;

/// A decoded recording, at its own sample rate.
struct Track {
    l: Vec<f32>,
    r: Vec<f32>,
    rate: f64,
    /// progressions: where its music ends and the next one comes in (in its samples);
    /// flybys: its loudest moment
    mark: f64,
}

impl Track {
    fn len(&self) -> f64 {
        self.l.len() as f64
    }
    fn at(&self, pos: f64) -> (f32, f32) {
        let i = pos as usize;
        if i + 1 >= self.l.len() {
            return (0.0, 0.0);
        }
        let f = pos.fract() as f32;
        (self.l[i] + (self.l[i + 1] - self.l[i]) * f, self.r[i] + (self.r[i + 1] - self.r[i]) * f)
    }
}

/// Decode an Ogg Vorbis file to two channels (a mono one to both).
fn decode(b: &[u8]) -> Option<Track> {
    let mut rd = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(b)).map_err(|e| log::warn!("menu music: {e}")).ok()?;
    let ch = rd.ident_hdr.audio_channels.max(1) as usize;
    let rate = rd.ident_hdr.audio_sample_rate as f64;
    let (mut l, mut r) = (Vec::new(), Vec::new());
    loop {
        match rd.read_dec_packet_itl() {
            Ok(Some(p)) => {
                for f in p.chunks_exact(ch) {
                    l.push(f[0] as f32 / 32768.0);
                    r.push(f[ch.min(2) - 1] as f32 / 32768.0);
                }
            }
            Ok(None) => break,
            Err(e) => {
                log::warn!("menu music: {e}");
                break;
            }
        }
    }
    (!l.is_empty()).then_some(Track { l, r, rate, mark: 0.0 })
}

struct Tracks {
    chords: Vec<Track>,
    flybys: Vec<Track>,
}

fn load() -> Tracks {
    let chords = CHORDS
        .iter()
        .filter_map(|b| decode(b))
        .map(|mut t| {
            // The whole phrases in it; what is left over is the last chord's release.
            let phrase = PHRASE * t.rate;
            let n = ((t.len() - 0.25 * t.rate) / phrase).floor();
            t.mark = if n >= 1.0 { n * phrase } else { t.len() };
            t.l.iter_mut().chain(t.r.iter_mut()).for_each(|x| *x *= MUSIC_GAIN);
            t
        })
        .collect();
    let flybys = FLYBYS
        .iter()
        .filter_map(|b| decode(b))
        .map(|mut t| {
            // Levelled to a peak of 1, and its loudest quarter second found.
            let peak = t.l.iter().chain(&t.r).fold(1e-6f32, |m, x| m.max(x.abs()));
            t.l.iter_mut().chain(t.r.iter_mut()).for_each(|x| *x /= peak);
            let w = (t.rate * 0.25) as usize;
            let loud = t.l.chunks(w.max(1)).map(|c| c.iter().map(|x| x * x).sum::<f32>()).enumerate().max_by(|a, b| a.1.total_cmp(&b.1)).map_or(0, |(i, _)| i);
            t.mark = (loud as f64 + 0.5) * w as f64;
            t
        })
        .collect();
    Tracks { chords, flybys }
}

static TRACKS: OnceLock<Tracks> = OnceLock::new();
static LOADING: AtomicBool = AtomicBool::new(false);

/// Decode the recordings on a thread of their own (a few hundred ms), once;
/// the music stays silent until they are ready.
pub(super) fn preload() {
    if !LOADING.swap(true, Ordering::Relaxed) {
        let spawned = std::thread::Builder::new().name("menu-music".into()).spawn(|| {
            TRACKS.get_or_init(load);
        });
        if spawned.is_err() {
            LOADING.store(false, Ordering::Relaxed);
        }
    }
}

/// A progression playing: which, and how far through (its samples).
struct Chord {
    i: usize,
    pos: f64,
}

/// An aircraft going over.
struct Flyby {
    i: usize,
    pos: f64,
    /// samples of the recording per sample of output
    step: f64,
    /// its closest point (its samples), and how long it takes about it to get there and away
    apex: f64,
    len: f64,
    /// the side it comes from: -1 left, 1 right
    from: f32,
    gain: f32,
    /// the envelope, pan and low-pass, retuned every tick
    env: f32,
    pan: f32,
    lp: [Bq; 2],
}

pub(super) struct Music {
    rng: Rng,
    n: u32,
    /// the volume asked for, followed smoothly
    vol: f32,
    /// the fade in and out, 0..1
    fade: f32,
    playing: bool,
    chords: Vec<Chord>,
    /// output samples until the next progression comes in
    next: f64,
    last: usize,
    flyby: Option<Flyby>,
    /// seconds until the next flyby
    flyby_in: f32,
    last_flyby: usize,
}

impl Music {
    pub(super) fn new(seed: u32) -> Self {
        Self { rng: Rng(seed | 1), n: 0, vol: 0.0, fade: 0.0, playing: false, chords: Vec::with_capacity(3), next: 0.0, last: usize::MAX, flyby: None, flyby_in: 0.0, last_flyby: usize::MAX }
    }

    /// Pick a progression, never the one just heard (that would sound like a loop).
    fn pick(rng: &mut Rng, n: usize, not: usize) -> usize {
        if n < 2 {
            return 0;
        }
        let mut i = (rng.u() * n as f32) as usize % n;
        if i == not {
            i = (i + 1 + (rng.u() * (n - 1) as f32) as usize % (n - 1)) % n;
        }
        i
    }

    fn start_chord(&mut self, t: &Tracks) {
        let i = Self::pick(&mut self.rng, t.chords.len(), self.last);
        self.last = i;
        let c = &t.chords[i];
        self.next += c.mark / (c.rate / RATE as f64);
        if self.chords.len() >= 3 {
            self.chords.remove(0);
        }
        self.chords.push(Chord { i, pos: 0.0 });
    }

    fn start_flyby(&mut self, t: &Tracks) {
        let r = &mut self.rng;
        let i = Self::pick(r, t.flybys.len(), self.last_flyby);
        self.last_flyby = i;
        let tr = &t.flybys[i];
        let apex = (tr.mark + r.range(-APEX_JITTER, APEX_JITTER) as f64 * tr.rate).clamp(tr.rate, (tr.len() - tr.rate).max(tr.rate));
        let from = if r.u() < 0.5 { -1.0 } else { 1.0 };
        let gain = r.range(FLYBY_MIN, FLYBY_MAX);
        let step = tr.rate / RATE as f64 * r.range(0.93, 1.07) as f64;
        let lp = [Bq::new(Bq::lp, LP_FAR, 0.7); 2];
        self.flyby = Some(Flyby { i, pos: 0.0, step, apex, len: tr.len(), from, gain, env: 0.0, pan: from * 0.85, lp });
        self.flyby_in = r.range(FLYBY_GAP.0, FLYBY_GAP.1);
    }

    /// Retune the flyby's envelope, pan and filter for where it has got to.
    fn tune(f: &mut Flyby, rate: f64) {
        // Seconds from its closest point (at the recording's own pace).
        let d = ((f.pos - f.apex) / rate) as f32;
        let x = if f.pos < f.apex { (f.pos / f.apex) as f32 } else { ((f.len - f.pos) / (f.len - f.apex).max(1.0)) as f32 };
        let s = (x.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin();
        f.env = s * s;
        // Across from one side to the other, the middle at the closest point.
        f.pan = -f.from * (d / 2.2).tanh() * 0.85;
        let near = 1.0 / (1.0 + (d / 2.0) * (d / 2.0));
        let cut = LP_FAR * (LP_NEAR / LP_FAR).powf(near);
        for b in &mut f.lp {
            b.lp(cut, 0.7);
        }
    }

    /// The next stereo sample. `target` is the music's volume: 0 out of the menu.
    pub(super) fn sample(&mut self, target: f32) -> (f32, f32) {
        let tick = self.n % TICK == 0;
        self.n = self.n.wrapping_add(1);
        if tick {
            let dt = TICK as f32 / RATE as f32;
            self.vol += (target - self.vol) * kt(0.05);
            if target > 0.0 {
                self.fade = (self.fade + dt / FADE_IN).min(1.0);
            } else {
                self.fade = (self.fade - dt / FADE_OUT).max(0.0);
            }
            if self.playing && target <= 0.0 && self.fade <= 0.0 {
                // Gone quiet with the menu closed: next time, start afresh.
                self.playing = false;
                self.chords.clear();
                self.flyby = None;
            }
            if !self.playing && target <= 0.0 {
                return (0.0, 0.0);
            }
            if self.playing {
                self.flyby_in -= dt;
            }
        } else if !self.playing {
            return (0.0, 0.0);
        }
        let Some(t) = TRACKS.get() else {
            if tick {
                preload();
            }
            return (0.0, 0.0);
        };
        if t.chords.is_empty() {
            return (0.0, 0.0);
        }
        if !self.playing {
            self.playing = true;
            self.fade = 0.0;
            self.next = 0.0;
            self.last = usize::MAX;
            self.flyby_in = self.rng.range(FLYBY_FIRST.0, FLYBY_FIRST.1);
        }
        if self.next <= 0.0 {
            self.start_chord(t);
        }
        self.next -= 1.0;

        let (mut l, mut r) = (0.0, 0.0);
        self.chords.retain_mut(|c| {
            let tr = &t.chords[c.i];
            let (a, b) = tr.at(c.pos);
            l += a;
            r += b;
            c.pos += tr.rate / RATE as f64;
            c.pos < tr.len()
        });

        if self.flyby.is_none() && self.flyby_in <= 0.0 && !t.flybys.is_empty() && self.fade > 0.5 {
            self.start_flyby(t);
        }
        if let Some(f) = &mut self.flyby {
            let tr = &t.flybys[f.i];
            if tick {
                Self::tune(f, tr.rate);
            }
            let (a, _) = tr.at(f.pos);
            let y = f.lp[0].run(a);
            let y = f.lp[1].run(y) * f.env * f.gain;
            let (pl, pr) = pan(y, f.pan);
            l += pl;
            r += pr;
            f.pos += f.step;
            if f.pos >= f.len {
                self.flyby = None;
            }
        }

        // The fade squared, so it eases in rather than jumping up.
        let g = self.vol * self.fade * self.fade;
        (l * g, r * g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> &'static Tracks {
        TRACKS.get_or_init(load)
    }

    #[test]
    fn recordings_decode_and_land_on_the_grid() {
        let t = ready();
        assert_eq!((t.chords.len(), t.flybys.len()), (4, 3));
        for c in &t.chords {
            let bars = c.mark / c.rate / (PHRASE / 4.0);
            assert!((bars - 16.0).abs() < 1e-6 || (bars - 24.0).abs() < 1e-6, "progression of {bars} bars");
            let tail = (c.len() - c.mark) / c.rate;
            assert!((1.0..4.0).contains(&tail), "release of {tail} s");
        }
        for f in &t.flybys {
            let at = f.mark / f.rate;
            assert!(at > 2.0 && at < f.len() / f.rate - 2.0, "loudest at {at} s");
        }
    }

    #[test]
    fn fades_in_chains_and_flies_over() {
        ready();
        let mut m = Music::new(7);
        let secs = 150;
        let mut rms = vec![0.0f32; secs];
        let mut side = vec![0.0f32; secs];
        let mut flybys = 0;
        let mut had = false;
        for s in 0..secs {
            for _ in 0..RATE {
                let (l, r) = m.sample(0.8);
                rms[s] += l * l + r * r;
                side[s] += (l - r).abs();
            }
            rms[s] = (rms[s] / (2 * RATE) as f32).sqrt();
            if m.flyby.is_some() && !had {
                flybys += 1;
            }
            had = m.flyby.is_some();
        }
        // Fades in over its first seconds, then holds.
        assert!(rms[0] < rms[12] * 0.2, "fade in {} vs {}", rms[0], rms[12]);
        assert!(rms.iter().skip(12).all(|&x| x > 0.02), "a gap in the music: {rms:?}");
        assert!(rms.iter().all(|&x| x < 0.6), "too loud: {rms:?}");
        // Flybys 30..60 s apart (the first sooner): two to five in two and a half minutes.
        assert!((2..=5).contains(&flybys), "{flybys} flybys");
        // Closing the menu fades it out and stops it.
        for _ in 0..RATE * 2 {
            m.sample(0.0);
        }
        assert!(!m.playing && m.chords.is_empty());
        assert_eq!(m.sample(0.0), (0.0, 0.0));
    }

    #[test]
    fn flyby_pans_across_and_dulls_far_off() {
        let t = ready();
        let mut m = Music::new(3);
        m.start_flyby(t);
        let f = m.flyby.as_mut().unwrap();
        let rate = t.flybys[f.i].rate;
        f.pos = 0.0;
        Music::tune(f, rate);
        let (start, env0) = (f.pan, f.env);
        f.pos = f.apex;
        Music::tune(f, rate);
        let (mid, env1) = (f.pan, f.env);
        f.pos = f.len - 1.0;
        Music::tune(f, rate);
        let end = f.pan;
        assert!(env0 < 0.01 && env1 > 0.99, "envelope {env0} .. {env1}");
        assert!(start * end < 0.0 && start.abs() > 0.4 && end.abs() > 0.4 && mid.abs() < 0.05, "pan {start} {mid} {end}");
    }

    /// `MENU_MUSIC_WAV=path cargo test --release menu_music_render -- --ignored`
    /// writes three minutes of the menu music to listen to.
    #[test]
    #[ignore]
    fn menu_music_render() {
        let Ok(path) = std::env::var("MENU_MUSIC_WAV") else { return };
        ready();
        let mut m = Music::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos());
        let n = RATE as usize * 180;
        let mut out = Vec::with_capacity(44 + n * 4);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + n as u32 * 4).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        for v in [16u32, 1 | 2 << 16, RATE, RATE * 4, 4 | 16 << 16] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(n as u32 * 4).to_le_bytes());
        for _ in 0..n {
            let (l, r) = m.sample(1.0);
            for x in [l, r] {
                out.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
            }
        }
        std::fs::write(path, out).unwrap();
    }
}
