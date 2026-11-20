// Where a song lies in a PMV's soundtrack: the PMV time at which the song file's start plays (negative when
// the PMV begins in the middle of the song). Both are heard as onsets in 12 frequency bands (log energy rises,
// 86 per second); the two are cross-correlated over every possible offset with an FFT, the peak refined to a
// fraction of a step, and checked piece by piece: 20 s pieces of the PMV each look for their own best offset –
// when most agree, the PMV plays the song unedited (sound effects on top don't matter much).

use crate::ff;
use anyhow::{bail, Result};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

const SR: f64 = 11025.0;
const WIN: usize = 1024;
const HOP: usize = 128;
const BANDS: usize = 12;
/// feature steps per second
pub const RATE: f64 = SR / HOP as f64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// PMV time of the song's start (seconds)
    pub offset: f64,
    /// how clearly the best offset stands out (z-score of the correlation peak)
    pub clarity: f64,
    /// share of the PMV's pieces that find the same offset (0–1; 1 = the song plays unedited)
    pub agree: f64,
}

impl Fit {
    /// Sure enough to use without asking
    pub fn good(&self) -> bool {
        self.clarity >= 8.0 && self.agree >= 0.5
    }
}

fn decode(path: &Path, cancel: &AtomicBool) -> Result<Vec<f32>> {
    let mut c = ff::ffmpeg()?;
    c.arg("-i").arg(path);
    c.args([
        "-vn", "-sn", "-dn", "-ac", "1", "-ar", "11025", "-f", "f32le", "pipe:1",
    ]);
    let out = c.stdout(Stdio::piped()).stderr(Stdio::null()).output()?;
    if cancel.load(Ordering::SeqCst) {
        bail!("Cancelled");
    }
    let v: Vec<f32> = out
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    if v.len() < SR as usize {
        bail!("no sound in {}", path.display());
    }
    Ok(v)
}

// ---------- FFT (radix 2, in place) ----------

fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
    if inverse {
        for (r, i) in re.iter_mut().zip(im.iter_mut()) {
            *r /= n as f64;
            *i /= n as f64;
        }
    }
}

/// Onsets per band, each band normalized (zero mean, unit variance): [band][step]
fn onsets(x: &[f32]) -> Vec<Vec<f64>> {
    onsets_with(x, WIN, HOP, BANDS, 80.0)
}

fn onsets_with(x: &[f32], win: usize, hop: usize, bands: usize, f_lo: f64) -> Vec<Vec<f64>> {
    let steps = if x.len() > win {
        (x.len() - win) / hop + 1
    } else {
        0
    };
    let hann: Vec<f64> = (0..win)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / win as f64).cos())
        .collect();
    // band edges (FFT bins), log-spaced f_lo – 5 kHz
    let bin = |f: f64| ((f / SR * win as f64).round() as usize).clamp(1, win / 2);
    let edges: Vec<usize> = (0..=bands)
        .map(|k| bin(f_lo * (5000.0f64 / f_lo).powf(k as f64 / bands as f64)))
        .collect();
    let mut log = vec![vec![0f64; steps]; bands];
    let (mut re, mut im) = (vec![0f64; win], vec![0f64; win]);
    for t in 0..steps {
        for i in 0..win {
            re[i] = x[t * hop + i] as f64 * hann[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im, false);
        for b in 0..bands {
            let (a, z) = (edges[b], edges[b + 1].max(edges[b] + 1));
            let e: f64 = (a..z).map(|k| re[k] * re[k] + im[k] * im[k]).sum();
            log[b][t] = (e + 1e-9).ln();
        }
    }
    log.into_iter()
        .map(|l| {
            let mut o: Vec<f64> = (0..l.len())
                .map(|t| {
                    if t == 0 {
                        0.0
                    } else {
                        (l[t] - l[t - 1]).max(0.0)
                    }
                })
                .collect();
            let n = o.len().max(1) as f64;
            let mean = o.iter().sum::<f64>() / n;
            let sd = (o.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt() + 1e-9;
            for v in o.iter_mut() {
                *v = (*v - mean) / sd;
            }
            o
        })
        .collect()
}

/// Cross-correlation of every band, summed: c[lag + (len_s − 1)] = Σ_t p[t]·s[t − lag]
fn xcorr(p: &[Vec<f64>], s: &[Vec<f64>]) -> Vec<f64> {
    let (np, ns) = (p[0].len(), s[0].len());
    let n = (np + ns).next_power_of_two();
    let mut acc = vec![0f64; np + ns - 1];
    for b in 0..p.len() {
        let (mut ar, mut ai) = (vec![0f64; n], vec![0f64; n]);
        let (mut br, mut bi) = (vec![0f64; n], vec![0f64; n]);
        ar[..np].copy_from_slice(&p[b]);
        // the song reversed: correlation becomes convolution
        for (k, v) in s[b].iter().enumerate() {
            br[ns - 1 - k] = *v;
        }
        fft(&mut ar, &mut ai, false);
        fft(&mut br, &mut bi, false);
        for k in 0..n {
            let (r, i) = (ar[k] * br[k] - ai[k] * bi[k], ar[k] * bi[k] + ai[k] * br[k]);
            ar[k] = r;
            ai[k] = i;
        }
        fft(&mut ar, &mut ai, true);
        for (k, v) in acc.iter_mut().enumerate() {
            *v += ar[k];
        }
    }
    acc
}

/// The best offset (in steps, refined) of p against s, with its z-score; only offsets where at least
/// `min_overlap` steps overlap count
fn best(p: &[Vec<f64>], s: &[Vec<f64>], min_overlap: usize) -> Option<(f64, f64)> {
    let (np, ns) = (p[0].len(), s[0].len());
    let c = xcorr(p, s);
    // per overlapping step (an offset with little overlap would otherwise lose for having less to add up)
    let score: Vec<f64> = c
        .iter()
        .enumerate()
        .map(|(k, v)| {
            let lag = k as isize - (ns as isize - 1);
            let lo = lag.max(0);
            let hi = (np as isize).min(ns as isize + lag);
            let ov = (hi - lo).max(0) as usize;
            if ov >= min_overlap {
                v / (ov as f64 * p.len() as f64)
            } else {
                f64::NAN
            }
        })
        .collect();
    let valid: Vec<f64> = score.iter().copied().filter(|v| !v.is_nan()).collect();
    if valid.len() < 3 {
        return None;
    }
    let mean = valid.iter().sum::<f64>() / valid.len() as f64;
    let sd =
        (valid.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / valid.len() as f64).sqrt() + 1e-12;
    let (k, top) = score
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_nan())
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    // parabola through the peak and its neighbours: a fraction of a step
    let at = |i: isize| score.get(i as usize).copied().filter(|v| !v.is_nan());
    let frac = match (at(k as isize - 1), at(k as isize + 1)) {
        (Some(a), Some(b)) => {
            let d = a - 2.0 * top + b;
            if d.abs() > 1e-12 {
                (0.5 * (a - b) / d).clamp(-0.5, 0.5)
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    Some((k as f64 + frac - (ns as f64 - 1.0), (top - mean) / sd))
}

/// A finer look around a found offset (seconds): onsets every 16 samples (1.45 ms) from 23 ms windows,
/// compared at every lag within ±60 ms → the offset to about a millisecond
fn refine(a: &[f32], b: &[f32], coarse: f64) -> f64 {
    const W: usize = 256;
    const H: usize = 16;
    let rate = SR / H as f64;
    let p = onsets_with(a, W, H, 6, 200.0);
    let s = onsets_with(b, W, H, 6, 200.0);
    let (np, ns) = (p[0].len() as isize, s[0].len() as isize);
    let center = (coarse * rate).round() as isize;
    let span = (0.06 * rate) as isize;
    let mut sc: Vec<(isize, f64)> = Vec::new();
    for lag in center - span..=center + span {
        // p[t] ↔ s[t − lag]
        let lo = lag.max(0);
        let hi = np.min(ns + lag);
        if hi - lo < 1000 {
            continue;
        }
        let mut sum = 0.0;
        for band in 0..p.len() {
            let (pb, sb) = (&p[band], &s[band]);
            for t in lo..hi {
                sum += pb[t as usize] * sb[(t - lag) as usize];
            }
        }
        sc.push((lag, sum / (hi - lo) as f64));
    }
    let Some(k) = (0..sc.len()).max_by(|&x, &y| sc[x].1.total_cmp(&sc[y].1)) else {
        return coarse;
    };
    let frac = if k > 0 && k + 1 < sc.len() {
        let (a, c, b) = (sc[k - 1].1, sc[k].1, sc[k + 1].1);
        let d = a - 2.0 * c + b;
        if d.abs() > 1e-12 {
            (0.5 * (a - b) / d).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    } else {
        0.0
    };
    // (the windows start at different sample phases in the two – the offset is between their starts)
    (sc[k].0 as f64 + frac) / rate
}

/// Where the song file lies in the PMV's soundtrack
pub fn find(pmv: &Path, song: &Path, cancel: &AtomicBool) -> Result<Fit> {
    let a = decode(pmv, cancel)?;
    let b = decode(song, cancel)?;
    let (p, s) = (onsets(&a), onsets(&b));
    if p[0].len() < 100 || s[0].len() < 100 {
        bail!("too short to compare");
    }
    let min_ov = ((10.0 * RATE) as usize).min(p[0].len().min(s[0].len()) / 2);
    let (lag, clarity) = best(&p, &s, min_ov).ok_or_else(|| anyhow::anyhow!("no match"))?;
    // pieces of 20 s (every 10 s): do they find the same offset?
    let piece = (20.0 * RATE) as usize;
    let (mut n, mut same) = (0, 0);
    let mut st = 0;
    while st + piece <= p[0].len() {
        if cancel.load(Ordering::SeqCst) {
            bail!("Cancelled");
        }
        let part: Vec<Vec<f64>> = p.iter().map(|b| b[st..st + piece].to_vec()).collect();
        if let Some((l, z)) = best(&part, &s, piece / 2) {
            n += 1;
            if z > 5.0 && (l + st as f64 - lag).abs() <= 3.0 {
                same += 1;
            }
        }
        st += piece / 2;
    }
    Ok(Fit {
        offset: refine(&a, &b, lag / RATE),
        clarity,
        agree: if n == 0 { 0.0 } else { same as f64 / n as f64 },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise_song(len: usize, seed: u64) -> Vec<f32> {
        // clicks and tones at irregular times: something to line up
        let mut x = vec![0f32; len];
        let mut r = seed;
        let mut t = 0usize;
        while t < len {
            r = r
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let f = 200.0 + (r >> 40) as f32 % 3000.0;
            for k in 0..800.min(len - t) {
                x[t + k] += (k as f32 * f / SR as f32 * std::f32::consts::TAU).sin()
                    * (1.0 - k as f32 / 800.0);
            }
            t += 1500 + (r >> 33) as usize % 6000;
        }
        x
    }

    #[test]
    fn finds_where_the_song_starts() {
        let song = noise_song(SR as usize * 60, 3);
        // a "PMV": 7.3 s of something else, then the song from its 2nd second on, with noise on top
        let intro = noise_song((SR * 7.3) as usize, 99);
        let mut pmv = intro.clone();
        pmv.extend_from_slice(&song[SR as usize * 2..]);
        let mut r = 1u64;
        for v in pmv.iter_mut() {
            r = r.wrapping_mul(6364136223846793005).wrapping_add(1);
            *v = *v * 0.8 + ((r >> 41) as f32 / (1u64 << 23) as f32 - 0.5) * 0.3;
        }
        let (p, s) = (onsets(&pmv), onsets(&song));
        let (lag, z) = best(&p, &s, 500).unwrap();
        // the song's start would be at 7.3 − 2 = 5.3 s of the PMV – within a quarter of a 60 fps frame after refining
        assert!((lag / RATE - 5.3).abs() < 0.03, "{}", lag / RATE);
        let off = refine(&pmv, &song, lag / RATE);
        assert!((off - 5.3).abs() < 0.006, "{off}");
        assert!(z > 8.0, "{z}");
    }

    #[test]
    fn fft_round_trip() {
        let mut re: Vec<f64> = (0..64).map(|i| (i as f64 * 0.3).sin()).collect();
        let orig = re.clone();
        let mut im = vec![0.0; 64];
        fft(&mut re, &mut im, false);
        fft(&mut re, &mut im, true);
        assert!(re.iter().zip(&orig).all(|(a, b)| (a - b).abs() < 1e-9));
    }
}
