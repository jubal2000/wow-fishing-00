//! Log-mel spectrogram features and template matching.
//!
//! Pipeline: 48 kHz mono -> 16 kHz (decimate by 3) -> 512-pt FFT, 10 ms hop
//! -> 40 mel bands -> log. A reference clip becomes a (frames x bands)
//! template; the live stream is scored by normalized cross-correlation
//! against a sliding window of the same length.

use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::collections::VecDeque;
use std::sync::Arc;

pub const SR: usize = 16_000;
pub const N_FFT: usize = 512;
pub const HOP: usize = 160; // 10 ms
pub const N_MELS: usize = 40;
const F_MIN: f32 = 100.0;
const F_MAX: f32 = 7600.0;

/// Frames per second of the feature stream.
pub const FPS: f32 = SR as f32 / HOP as f32;

fn hz_to_mel(f: f32) -> f32 {
    2595.0 * (1.0 + f / 700.0).log10()
}
fn mel_to_hz(m: f32) -> f32 {
    700.0 * (10f32.powf(m / 2595.0) - 1.0)
}

fn mel_filterbank() -> Vec<Vec<(usize, f32)>> {
    let n_bins = N_FFT / 2 + 1;
    let (lo, hi) = (hz_to_mel(F_MIN), hz_to_mel(F_MAX));
    let pts: Vec<f32> = (0..N_MELS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f32 / (N_MELS + 1) as f32))
        .map(|hz| hz * N_FFT as f32 / SR as f32)
        .collect();
    (0..N_MELS)
        .map(|m| {
            let (l, c, r) = (pts[m], pts[m + 1], pts[m + 2]);
            (0..n_bins)
                .filter_map(|b| {
                    let x = b as f32;
                    let w = if x > l && x <= c {
                        (x - l) / (c - l)
                    } else if x > c && x < r {
                        (r - x) / (r - c)
                    } else {
                        0.0
                    };
                    (w > 0.0).then_some((b, w))
                })
                .collect()
        })
        .collect()
}

/// One feature frame: log-mel bands plus frame energy in dBFS.
#[derive(Clone)]
pub struct Frame {
    pub mel: [f32; N_MELS],
    pub db: f32,
}

/// Streaming feature extractor. Push 16 kHz samples, get frames out.
pub struct Extractor {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    bank: Vec<Vec<(usize, f32)>>,
    buf: VecDeque<f32>,
    scratch: Vec<Complex32>,
}

impl Extractor {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(N_FFT);
        let window = (0..N_FFT)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos())
            .collect();
        Self {
            fft,
            window,
            bank: mel_filterbank(),
            buf: VecDeque::with_capacity(N_FFT * 4),
            scratch: vec![Complex32::default(); N_FFT],
        }
    }

    pub fn push(&mut self, samples: &[f32], out: &mut Vec<Frame>) {
        self.buf.extend(samples.iter().copied());
        while self.buf.len() >= N_FFT {
            let mut energy = 0.0f32;
            for (i, s) in self.buf.iter().take(N_FFT).enumerate() {
                energy += s * s;
                self.scratch[i] = Complex32::new(s * self.window[i], 0.0);
            }
            self.fft.process(&mut self.scratch);
            let mut mel = [0f32; N_MELS];
            for (m, filt) in self.bank.iter().enumerate() {
                let e: f32 = filt.iter().map(|&(b, w)| self.scratch[b].norm_sqr() * w).sum();
                mel[m] = (e + 1e-8).ln();
            }
            let rms = (energy / N_FFT as f32).sqrt();
            out.push(Frame { mel, db: 20.0 * (rms + 1e-9).log10() });
            self.buf.drain(..HOP);
        }
    }
}

pub fn extract_all(samples16k: &[f32]) -> Vec<Frame> {
    let mut ex = Extractor::new();
    let mut out = Vec::new();
    ex.push(samples16k, &mut out);
    out
}

/// Decimate 48 kHz (or any multiple of 16 kHz) to 16 kHz with a box filter.
/// Keeps leftover samples between calls.
pub struct Resampler {
    factor: usize,
    acc: f32,
    n: usize,
}

impl Resampler {
    pub fn new(in_rate: usize) -> Self {
        assert!(in_rate % SR == 0, "input rate must be a multiple of 16 kHz");
        Self { factor: in_rate / SR, acc: 0.0, n: 0 }
    }
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &s in input {
            self.acc += s;
            self.n += 1;
            if self.n == self.factor {
                out.push(self.acc / self.factor as f32);
                self.acc = 0.0;
                self.n = 0;
            }
        }
    }
}

pub fn resample_to_16k(samples: &[f32], rate: usize) -> Vec<f32> {
    if rate == SR {
        return samples.to_vec();
    }
    if rate % SR == 0 {
        let mut r = Resampler::new(rate);
        let mut out = Vec::new();
        r.process(samples, &mut out);
        return out;
    }
    // Generic linear interpolation for odd rates (e.g. 44.1 kHz wav files).
    let ratio = rate as f64 / SR as f64;
    let n = (samples.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let i0 = p as usize;
            let t = (p - i0 as f64) as f32;
            let a = samples[i0];
            let b = *samples.get(i0 + 1).unwrap_or(&a);
            a + (b - a) * t
        })
        .collect()
}

/// A reference sound to look for.
pub struct Template {
    /// Mean-removed, unit-norm flattened log-mel matrix.
    vec: Vec<f32>,
    pub frames: usize,
    /// Average loudness of the clip (dBFS) — used for the energy gate.
    pub db: f32,
}

/// Dynamic range kept below the loudest bin (natural log of power; 7 ≈ 30 dB).
/// Clamping the quiet bins makes matching insensitive to the background
/// noise floor, which differs between the template and the live stream.
const RANGE: f32 = 7.0;

fn normalize(frames: &[&[f32; N_MELS]]) -> Option<Vec<f32>> {
    let n = frames.len() * N_MELS;
    let max = frames.iter().flat_map(|f| f.iter()).fold(f32::MIN, |m, &x| m.max(x));
    let floor = max - RANGE;
    let mut v: Vec<f32> = frames.iter().flat_map(|f| f.iter()).map(|&x| x.max(floor)).collect();
    let mean: f32 = v.iter().sum::<f32>() / n as f32;
    v.iter_mut().for_each(|x| *x -= mean);
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-6 {
        return None;
    }
    v.iter_mut().for_each(|x| *x /= norm);
    Some(v)
}

impl Template {
    pub fn from_frames(frames: &[Frame]) -> anyhow::Result<Self> {
        anyhow::ensure!(frames.len() >= 5, "template clip is too short (need >= 50 ms)");
        let refs: Vec<_> = frames.iter().map(|f| &f.mel).collect();
        let vec = normalize(&refs).ok_or_else(|| anyhow::anyhow!("template clip is silent"))?;
        let db = mean_db(frames);
        Ok(Self { vec, frames: frames.len(), db })
    }

    pub fn score(&self, window: &[Frame]) -> f32 {
        debug_assert_eq!(window.len(), self.frames);
        let refs: Vec<_> = window.iter().map(|f| &f.mel).collect();
        match normalize(&refs) {
            Some(v) => v.iter().zip(&self.vec).map(|(a, b)| a * b).sum(),
            None => 0.0,
        }
    }
}

pub fn mean_db(frames: &[Frame]) -> f32 {
    let p: f32 = frames.iter().map(|f| 10f32.powf(f.db / 10.0)).sum::<f32>() / frames.len() as f32;
    10.0 * (p + 1e-12).log10()
}

/// What the detector reports for one score peak.
#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// Peak >= threshold. `template` = index of the best-matching template.
    Hit { score: f32, template: usize },
    /// Peak in [near, threshold): shown so you can tune the threshold.
    NearMiss { score: f32, template: usize },
}

/// Sliding-window detector with a loudness gate and peak picking.
/// Several templates may be given; the best match wins.
pub struct Detector {
    pub templates: Vec<Template>,
    pub threshold: f32,
    /// Peaks at or above this (but below threshold) are reported as NearMiss.
    pub near: f32,
    /// Windows quieter than this (dBFS) are ignored. Game audio levels vary a
    /// lot (volume sliders, 3D distance), so this is an absolute floor.
    pub min_db: f32,
    /// Frames the score must stop improving for before a peak is reported.
    pub peak_hold: usize,
    hist: VecDeque<Frame>,
    max_frames: usize,
    armed: Option<(f32, usize)>,
    armed_age: usize,
    /// Frames left to ignore after a hit (one sound = one trigger).
    refractory: usize,
    /// Frames left before another near-miss may be reported (print only).
    near_quiet: usize,
    pub last_score: f32,
    pub last_db: f32,
}

impl Detector {
    pub fn new(templates: Vec<Template>, threshold: f32, min_db: f32) -> Self {
        let max_frames = templates.iter().map(|t| t.frames).max().unwrap_or(1);
        Self {
            hist: VecDeque::with_capacity(max_frames + 1),
            max_frames,
            templates,
            threshold,
            near: threshold,
            min_db,
            peak_hold: 6,
            armed: None,
            armed_age: 0,
            refractory: 0,
            near_quiet: 0,
            last_score: 0.0,
            last_db: -120.0,
        }
    }

    /// Feed one frame. Returns an event when a score peak completes.
    pub fn feed(&mut self, f: Frame) -> Option<Event> {
        self.hist.push_back(f);
        if self.hist.len() > self.max_frames {
            self.hist.pop_front();
        }
        if self.hist.len() < self.max_frames {
            return None;
        }
        // Score every template against the most recent frames of its length.
        let mut best = (0f32, 0usize);
        let mut loudest = -200f32;
        for (i, t) in self.templates.iter().enumerate() {
            let skip = self.hist.len() - t.frames;
            let win: Vec<Frame> = self.hist.iter().skip(skip).cloned().collect();
            let db = mean_db(&win);
            loudest = loudest.max(db);
            let sc = if db >= self.min_db { t.score(&win) } else { 0.0 };
            if sc > best.0 {
                best = (sc, i);
            }
        }
        let (score, tpl) = best;
        self.last_score = score;
        self.last_db = loudest;
        if self.refractory > 0 {
            self.refractory -= 1;
            return None;
        }
        self.near_quiet = self.near_quiet.saturating_sub(1);

        let arm_level = self.near.min(self.threshold);
        match self.armed {
            None if score >= arm_level => {
                self.armed = Some((score, tpl));
                self.armed_age = 0;
                None
            }
            None => None,
            Some((peak, ptpl)) => {
                // Keep climbing to the true peak; report once the score has
                // not improved for `peak_hold` frames.
                if score > peak {
                    self.armed = Some((score, tpl));
                    self.armed_age = 0;
                    return None;
                }
                self.armed_age += 1;
                if self.armed_age < self.peak_hold {
                    return None;
                }
                self.armed = None;
                if peak >= self.threshold {
                    self.refractory = self.max_frames;
                    Some(Event::Hit { score: peak, template: ptpl })
                } else if self.near_quiet == 0 {
                    // One report per sound; detection itself is not paused.
                    self.near_quiet = self.max_frames;
                    Some(Event::NearMiss { score: peak, template: ptpl })
                } else {
                    None
                }
            }
        }
    }

    pub fn reset(&mut self) {
        self.hist.clear();
        self.armed = None;
        self.refractory = 0;
        self.near_quiet = 0;
    }
}

/// Pick the loudest event in a recording and trim around it.
/// Returns (start_sample, end_sample) in 16 kHz samples.
pub fn auto_trim(frames: &[Frame], max_secs: f32, drop_db: f32) -> Option<(usize, usize)> {
    let (peak_i, peak) = frames
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.db.total_cmp(&b.1.db))?;
    let floor = peak.db - drop_db;
    let max_frames = (max_secs * FPS) as usize;
    let mut s = peak_i;
    while s > 0 && frames[s - 1].db > floor && peak_i - s < max_frames / 3 {
        s -= 1;
    }
    let mut e = peak_i;
    while e + 1 < frames.len() && frames[e + 1].db > floor && e - s < max_frames {
        e += 1;
    }
    // At least ~150 ms so short clicks still make a usable template.
    let min_frames = (0.15 * FPS) as usize;
    while e - s < min_frames && (s > 0 || e + 1 < frames.len()) {
        if e + 1 < frames.len() { e += 1; } else { s -= 1; }
    }
    Some((s * HOP, (e + 1) * HOP + N_FFT))
}
