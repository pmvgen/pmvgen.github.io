// Beat detection – a port of beats.js.
// 1. Onset curve: energy rises in the bass (kick drum) and in the whole signal.
// 2. Tempo: autocorrelation of the onset curve, weighted around 120 BPM (against octave errors).
// 3. Beats: dynamic programming after D. Ellis (2007) – follows small tempo fluctuations.
// 4. Energy per beat (0–1): this gives the calm parts and the drops.
// 5. Pace per beat (0–1): how busy the music is.
//
// Every song is boiled down to frame energies while it is decoded (22.05 kHz, 256 samples per frame =
// the same frame rate as the browser version's 44.1 kHz / 512), so any length works. Songs over 45 min
// (DJ mixes) get their tempo per 2-minute stretch, like the browser version.

pub const LSR: f64 = 22050.0;
pub const LHOP: usize = 256;
pub const LONG: f64 = 45.0 * 60.0;
pub const FPS: f64 = LSR / LHOP as f64;

/// Collects frame energies (bass and all) and frame peaks at 22.05 kHz
pub struct FrameSink {
    hop: usize,
    pub e_low: Vec<f32>,
    pub e_all: Vec<f32>,
    pub peak: Vec<f32>,
    acc: f64,
    acc_low: f64,
    max: f32,
    k: usize,
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl FrameSink {
    /// The long-file reader of the browser version: 22.05 kHz, 256 per frame, low-pass Q = 1/√2
    pub fn long() -> Self {
        Self::with(LSR, LHOP, std::f64::consts::FRAC_1_SQRT_2)
    }
    /// sample rate, samples per frame, the low-pass filter's Q (linear)
    pub fn with(rate: f64, hop: usize, q: f64) -> Self {
        // RBJ biquad low-pass at 160 Hz
        let w = 2.0 * std::f64::consts::PI * 160.0 / rate;
        let alpha = w.sin() / (2.0 * q);
        let cos = w.cos();
        let a0 = 1.0 + alpha;
        FrameSink {
            hop,
            e_low: Vec::new(),
            e_all: Vec::new(),
            peak: Vec::new(),
            acc: 0.0,
            acc_low: 0.0,
            max: 0.0,
            k: 0,
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
    pub fn push(&mut self, x: &[f32]) {
        for &v in x {
            let v = v as f64;
            let y = self.b0 * v + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = v;
            self.y2 = self.y1;
            self.y1 = y;
            self.acc += v * v;
            self.acc_low += y * y;
            let av = v.abs() as f32;
            if av > self.max {
                self.max = av;
            }
            self.k += 1;
            if self.k == self.hop {
                self.e_all.push(self.acc as f32);
                self.e_low.push(self.acc_low as f32);
                self.peak.push(self.max);
                self.acc = 0.0;
                self.acc_low = 0.0;
                self.max = 0.0;
                self.k = 0;
            }
        }
    }
    pub fn n(&self) -> usize {
        self.e_all.len()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub duration: f64,
    pub bpm: f64,
    pub beats: Vec<f64>,
    pub energy: Vec<f64>,
    pub pace: Vec<f64>,
    pub peaks: Vec<f32>,
    pub long: bool,
}

pub const HOP: usize = 512;
pub const SR: f64 = 44100.0;

/// A song (mono, 44.1 kHz) – analyzeBuffer() of the browser version
pub fn analyze_short(mono: &[f32]) -> Analysis {
    let sr = SR;
    let fps = sr / HOP as f64;
    // Web Audio's low-pass: Q is in dB there (default 1 dB)
    let mut sink = FrameSink::with(sr, HOP, 10f64.powf(1.0 / 20.0));
    sink.push(mono);
    let n = sink.n();
    let duration = mono.len() as f64 / sr;
    let peaks = overview(mono, 800);
    if n < 16 {
        return Analysis {
            duration,
            bpm: 120.0,
            peaks,
            ..Default::default()
        };
    }
    let onset = onset_curve(&sink.e_low, &sink.e_all, fps);
    let period = estimate_period(&onset, fps);
    let frames = track_beats(&onset, period);
    let beats: Vec<f64> = frames.iter().map(|&f| (f * HOP) as f64 / sr).collect();
    Analysis {
        duration,
        bpm: 60.0 * fps / period,
        energy: beat_energy(mono, sr, &beats),
        pace: beat_pace(&onset, &frames, period),
        beats,
        peaks,
        long: false,
    }
}

fn beat_energy(mono: &[f32], sr: f64, beats: &[f64]) -> Vec<f64> {
    let rms: Vec<f64> = beats
        .iter()
        .enumerate()
        .map(|(k, &t)| {
            let a = (t * sr).floor().max(0.0) as usize;
            let next = beats
                .get(k + 1)
                .copied()
                .filter(|&x| x != 0.0)
                .unwrap_or(t + 0.5);
            let b = (mono.len() as f64).min((next * sr).floor()).max(0.0) as usize;
            let mut s = 0.0f64;
            for v in mono.iter().take(b).skip(a) {
                s += (*v as f64) * (*v as f64);
            }
            (s / (b.saturating_sub(a)).max(1) as f64).sqrt()
        })
        .collect();
    norm_energy(&rms)
}

/// A long file (DJ mix) from its frame energies – songFromFrames() of the browser version
pub fn analyze_long(sink: &FrameSink) -> Analysis {
    let n = sink.n();
    let fps = FPS;
    let duration = n as f64 / fps;
    let peaks = overview(&sink.peak, 800);
    if n < 16 {
        return Analysis {
            duration,
            bpm: 120.0,
            peaks,
            long: true,
            ..Default::default()
        };
    }
    let onset = onset_curve(&sink.e_low, &sink.e_all, fps);
    // Tempo per 2-minute stretch (a mix changes tempo), beats per stretch, stitched in the overlaps
    let w = (120.0 * fps).round() as usize;
    let step = (100.0 * fps).round() as usize;
    let margin = (10.0 * fps).round() as usize;
    let mut beats: Vec<usize> = Vec::new();
    let mut periods = Vec::new();
    let mut s = 0;
    while s < n {
        let e = (s + w).min(n);
        let seg = &onset[s..e];
        if (seg.len() as f64) < fps * 8.0 {
            break;
        }
        let p = estimate_period(seg, fps);
        periods.push(p);
        let lo = if s == 0 { 0 } else { s + margin };
        let hi = if e >= n { n } else { e - margin };
        for f in track_beats(seg, p) {
            let g = f + s;
            if g < lo || g >= hi {
                continue;
            }
            if let Some(&last) = beats.last() {
                if ((g - last) as f64) < 0.5 * p {
                    continue;
                }
            }
            beats.push(g);
        }
        if e >= n {
            break;
        }
        s += step;
    }
    periods.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = periods.get(periods.len() / 2).copied().unwrap_or(fps / 2.0);
    let rms: Vec<f64> = beats
        .iter()
        .enumerate()
        .map(|(k, &f)| {
            let end = beats
                .get(k + 1)
                .copied()
                .unwrap_or_else(|| n.min(f + p.round() as usize));
            let s: f64 = (f..end.max(f)).map(|i| sink.e_all[i] as f64).sum();
            (s / ((end.saturating_sub(f)) * LHOP).max(1) as f64).sqrt()
        })
        .collect();
    Analysis {
        duration,
        bpm: 60.0 * fps / p,
        beats: beats.iter().map(|&f| f as f64 / fps).collect(),
        energy: norm_energy(&rms),
        pace: beat_pace(&onset, &beats, p),
        peaks,
        long: true,
    }
}

fn onset_curve(e_low: &[f32], e_all: &[f32], fps: f64) -> Vec<f32> {
    let n = e_low.len();
    let mut raw = vec![0.0f32; n];
    for i in 1..n {
        let dl = (e_low[i] as f64 + 1e-6).ln() - (e_low[i - 1] as f64 + 1e-6).ln();
        let da = (e_all[i] as f64 + 1e-6).ln() - (e_all[i - 1] as f64 + 1e-6).ln();
        raw[i] = (0.65 * dl.max(0.0) + 0.35 * da.max(0.0)) as f32;
    }
    // Subtract the moving average (only real peaks count) and normalize
    let w = ((fps * 0.4).round() as usize).max(1);
    let mut out = vec![0.0f32; n];
    let mut acc = 0.0f64;
    for i in 0..n {
        acc += raw[i] as f64;
        if i >= w {
            acc -= raw[i - w] as f64;
        }
        out[i] = (raw[i] as f64 - acc / ((i + 1).min(w) as f64)).max(0.0) as f32;
    }
    let nn = if n == 0 { 1.0 } else { n as f64 };
    let mean = out.iter().map(|&x| x as f64).sum::<f64>() / nn;
    let mut sd = (out.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / nn).sqrt();
    if sd == 0.0 || !sd.is_finite() {
        sd = 1.0;
    }
    for x in out.iter_mut() {
        *x = (*x as f64 / sd) as f32;
    }
    out
}

fn autocorr(o: &[f32], lag: usize) -> f64 {
    if lag >= o.len() {
        return 0.0;
    }
    let n = o.len() - lag;
    let mut s = 0.0f64;
    for i in 0..n {
        s += o[i] as f64 * o[i + lag] as f64;
    }
    s / n as f64
}

fn estimate_period(o: &[f32], fps: f64) -> f64 {
    let lag_min = (60.0 * fps / 200.0).floor() as usize;
    let lag_max = (60.0 * fps / 60.0).ceil() as usize;
    let score = |l: usize| {
        let bpm = 60.0 * fps / l as f64;
        let w = (-0.5 * ((bpm / 120.0).log2() / 0.9).powi(2)).exp();
        w * (autocorr(o, l) + 0.5 * autocorr(o, 2 * l))
    };
    let lo = lag_min - 1;
    let s: Vec<f64> = (lo..=lag_max + 1).map(score).collect();
    let at = |l: usize| s[l - lo];
    let mut best = lag_min;
    let mut best_s = f64::NEG_INFINITY;
    for l in lag_min..=lag_max {
        if at(l) > best_s {
            best_s = at(l);
            best = l;
        }
    }
    // Parabola through the neighbors → period with decimals
    let (a, b, c) = (at(best - 1), at(best), at(best + 1));
    let d = a - 2.0 * b + c;
    best as f64 + if d < 0.0 { 0.5 * (a - c) / d } else { 0.0 }
}

fn track_beats(o: &[f32], period: f64) -> Vec<usize> {
    let n = o.len();
    if n == 0 {
        return Vec::new();
    }
    let alpha = 100.0;
    let lo = (2.0 * period).round() as usize;
    let hi = ((period / 2.0).round() as usize).max(1);
    // penalty per distance
    let pen: Vec<f64> = (0..=lo.max(hi))
        .map(|d| {
            if d == 0 {
                0.0
            } else {
                alpha * ((d as f64) / period).ln().powi(2)
            }
        })
        .collect();
    let mut score = vec![0.0f32; n];
    let mut back = vec![-1isize; n];
    for i in 0..n {
        let mut best = f64::NEG_INFINITY;
        let mut arg: isize = -1;
        if i >= hi {
            let j0 = i.saturating_sub(lo);
            for j in j0..=(i - hi) {
                let v = score[j] as f64 - pen[i - j];
                if v > best {
                    best = v;
                    arg = j as isize;
                }
            }
        }
        let ok = arg >= 0 && best > 0.0;
        score[i] = (o[i] as f64 + if ok { best } else { 0.0 }) as f32;
        back[i] = if ok { arg } else { -1 };
    }
    // Find the best end in the last bar and trace back
    let mut end = n - 1;
    for i in n.saturating_sub(period.round() as usize)..n {
        if score[i] > score[end] {
            end = i;
        }
    }
    let mut beats = Vec::new();
    let mut i = end as isize;
    while i >= 0 {
        beats.push(i as usize);
        i = back[i as usize];
    }
    beats.reverse();
    // Fill gaps at the start (quiet intro) with the grid
    let first = beats.first().copied().unwrap_or(0) as f64;
    let mut pre = Vec::new();
    let mut f = first - period;
    while f >= 0.0 {
        pre.push(f.round() as usize);
        f -= period;
    }
    pre.reverse();
    pre.extend(beats);
    pre
}

// How busy each beat is: the mean onset strength between it and the next (frames) → 0–1 like the energy
fn beat_pace(onset: &[f32], frames: &[usize], period: f64) -> Vec<f64> {
    let busy: Vec<f64> = frames
        .iter()
        .enumerate()
        .map(|(k, &f)| {
            let end = onset.len().min(
                frames
                    .get(k + 1)
                    .copied()
                    .unwrap_or(f + period.round() as usize),
            );
            let s: f64 = (f..end.max(f)).map(|i| onset[i] as f64).sum();
            s / (end.saturating_sub(f)).max(1) as f64
        })
        .collect();
    norm_energy(&busy)
}

// Loudness per beat → 0–1 (10th to 95th percentile), smoothed over a bar
pub fn norm_energy(rms: &[f64]) -> Vec<f64> {
    if rms.is_empty() {
        return Vec::new();
    }
    let mut sorted = rms.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = |p: f64| sorted[((p * sorted.len() as f64).floor() as usize).min(sorted.len() - 1)];
    let lo = q(0.1);
    let mut span = q(0.95) - lo;
    if span == 0.0 {
        span = 1.0;
    }
    let e: Vec<f64> = rms
        .iter()
        .map(|r| ((r - lo) / span).clamp(0.0, 1.0))
        .collect();
    // Smooth over one bar so the mood doesn't flicker
    (0..e.len())
        .map(|k| {
            let mut s = 0.0;
            let mut c = 0.0;
            for j in k as isize - 2..=k as isize + 1 {
                if j >= 0 && (j as usize) < e.len() {
                    s += e[j as usize];
                    c += 1.0;
                }
            }
            s / c
        })
        .collect()
}

fn resample_energy(e: &[f64], len: usize) -> Vec<f64> {
    (0..len)
        .map(|i| {
            if e.is_empty() {
                0.0
            } else {
                e[((i * e.len()) / len.max(1)).min(e.len() - 1)]
            }
        })
        .collect()
}

pub fn overview(x: &[f32], buckets: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; buckets];
    let per = (x.len() / buckets).max(1);
    for (b, o) in out.iter_mut().enumerate() {
        let start = b * per;
        let end = (start + per).min(x.len());
        let mut m = 0.0f32;
        for v in x.iter().take(end).skip(start) {
            m = m.max(v.abs());
        }
        *o = m;
    }
    out
}

impl Analysis {
    /// Halve/double the tempo afterwards (if the detection is off by an octave)
    pub fn rescale(&self, factor: f64) -> Analysis {
        let b = &self.beats;
        let beats: Vec<f64> = if factor == 2.0 {
            let mut v = Vec::new();
            for i in 0..b.len() {
                v.push(b[i]);
                if i + 1 < b.len() {
                    v.push((b[i] + b[i + 1]) / 2.0);
                }
            }
            v
        } else {
            b.iter()
                .enumerate()
                .filter(|(i, _)| i % 2 == 0)
                .map(|(_, x)| *x)
                .collect()
        };
        Analysis {
            energy: resample_energy(&self.energy, beats.len()),
            pace: resample_energy(&self.pace, beats.len()),
            bpm: self.bpm * factor,
            beats,
            ..self.clone()
        }
    }
    /// Shift the beat grid (seconds), e.g. when the cuts come slightly too early
    pub fn shift(&self, sec: f64) -> Analysis {
        // (the energies stay with their beats; beats pushed out of the song drop out with theirs)
        let mut beats = Vec::new();
        let mut energy = Vec::new();
        let mut pace = Vec::new();
        for (i, t) in self.beats.iter().enumerate() {
            let t2 = t + sec;
            if t2 >= 0.0 && t2 < self.duration {
                beats.push(t2);
                energy.push(self.energy.get(i).copied().unwrap_or(0.0));
                pace.push(self.pace.get(i).copied().unwrap_or(0.0));
            }
        }
        Analysis {
            beats,
            energy,
            pace,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn click_track_120() {
        // 30 s of a kick every 0.5 s at 22.05 kHz
        let n = (30.0 * SR) as usize;
        let mut x = vec![0.0f32; n];
        for (i, v) in x.iter_mut().enumerate() {
            let t = i as f64 / SR;
            let ph = t % 0.5;
            if ph < 0.08 {
                *v = (0.9 * (2.0 * std::f64::consts::PI * 60.0 * ph).sin() * (-ph * 40.0).exp())
                    as f32;
            }
            *v += 0.01 * ((i * 7919 % 1000) as f32 / 1000.0 - 0.5);
        }
        let a = analyze_short(&x);
        assert!((a.bpm - 120.0).abs() < 1.5, "bpm {}", a.bpm);
        // beats sit on the clicks
        for t in a.beats.iter().skip(2).take(20) {
            let ph = t % 0.5;
            let d = ph.min(0.5 - ph);
            assert!(d < 0.03, "beat at {t}");
        }
    }
}
