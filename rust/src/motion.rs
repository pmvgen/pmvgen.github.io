// Motion of a clip over a short stretch: how fast what is in the picture moves ("Clip speed follows the music")
// and its rhythm ("Motion on the beat"). Ports rhythmOf(), beatFit(), actionMotion(), actionSpeed(),
// motionCurve() and the speed ranking of the web version. Measured on the tiny gray pictures of smart.rs.

use crate::smart::{self, Sig};

/// Step of the evenly spaced motion curve (seconds)
pub const RHYTHM_DT: f64 = 1.0 / 30.0;

/// A rhythm found in a motion curve
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rhythm {
    /// seconds per motion pulse
    pub period: f64,
    /// the half of a back-and-forth, if it is a rhythm of its own
    pub half: Option<f64>,
    /// a time (video seconds) where the motion turns
    pub turn: f64,
    /// how clear the rhythm is (0–1)
    pub q: f64,
}

/// Motion per second between neighbouring frames → [(t, m)]; None on a cut (a scene change mixes two motions)
pub fn motion_curve(fr: &[(f64, Sig)]) -> Option<Vec<(f64, f64)>> {
    let mut pts = Vec::with_capacity(fr.len());
    for w in fr.windows(2) {
        let dt = w[1].0 - w[0].0;
        if dt <= 0.0 {
            continue;
        }
        if smart::is_cut(&w[0].1, &w[1].1) {
            return None;
        }
        pts.push((
            (w[1].0 + w[0].0) / 2.0,
            smart::motion(&w[0].1, &w[1].1) / dt,
        ));
    }
    Some(pts)
}

/// The rhythm in a motion curve; None without a clear one
pub fn rhythm_of(pts: &[(f64, f64)]) -> Option<Rhythm> {
    if pts.len() < 30 {
        return None;
    }
    // evenly spaced (frames can be dropped or come at 24/25/60 fps)
    let t0 = pts[0].0;
    let n = ((pts[pts.len() - 1].0 - t0) / RHYTHM_DT).floor() as usize + 1;
    let mut x = vec![0f64; n];
    let mut j = 0usize;
    for (i, xi) in x.iter_mut().enumerate() {
        let t = t0 + i as f64 * RHYTHM_DT;
        while j < pts.len() - 2 && pts[j + 1].0 < t {
            j += 1;
        }
        let (a, b) = (pts[j], pts[j + 1]);
        let f = if b.0 > a.0 {
            ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        *xi = a.1 + (b.1 - a.1) * f;
    }
    let mean = x.iter().sum::<f64>() / n as f64;
    for v in x.iter_mut() {
        *v -= mean;
    }
    let sd = (x.iter().map(|v| v * v).sum::<f64>() / n as f64).sqrt();
    if mean < 0.3 || sd / mean < 0.15 {
        return None; // hardly any motion, or motion without ups and downs
    }
    // autocorrelation over 0.2–0.9 s; the shortest lag nearly as strong as the strongest
    let lo = (0.2 / RHYTHM_DT).round() as usize;
    let hi = ((0.9 / RHYTHM_DT).round() as usize).min((n as f64 * 0.6).floor() as usize);
    if hi <= lo + 2 {
        return None;
    }
    let corr = |l: usize| -> f64 {
        let (mut s, mut ea, mut eb) = (0.0, 0.0, 0.0);
        let mut i = 0;
        while i + l < n {
            s += x[i] * x[i + l];
            ea += x[i] * x[i];
            eb += x[i + l] * x[i + l];
            i += 1;
        }
        if ea != 0.0 && eb != 0.0 {
            s / (ea * eb).sqrt()
        } else {
            0.0
        }
    };
    // r[L] for the lags looked at (None = not computed – the web version's sparse array)
    let size = (2 * hi + 8).max(hi + 2);
    let mut r: Vec<Option<f64>> = vec![None; size];
    for (l, v) in r.iter_mut().enumerate().take(hi + 2).skip(lo - 1) {
        *v = Some(corr(l));
    }
    let rv = |r: &Vec<Option<f64>>, l: usize| r.get(l).copied().flatten();
    let peak_at = |r: &Vec<Option<f64>>, l: usize| -> f64 {
        let (a, b, c) = (
            rv(r, l - 1).unwrap_or(0.0),
            rv(r, l).unwrap_or(0.0),
            rv(r, l + 1).unwrap_or(0.0),
        );
        let den = a - 2.0 * b + c;
        l as f64
            + if den != 0.0 {
                (0.5 * (a - c) / den).clamp(-0.5, 0.5)
            } else {
                0.0
            }
    };
    let mut top = 0f64;
    for l in lo..=hi {
        top = top.max(rv(&r, l).unwrap_or(0.0));
    }
    if top < 0.35 {
        return None;
    }
    let mut lag = None;
    for l in lo..=hi {
        let (a, b, c) = (rv(&r, l - 1), rv(&r, l), rv(&r, l + 1));
        if let (Some(a), Some(b), Some(c)) = (a, b, c) {
            if b >= 0.8 * top && b >= a && b >= c {
                lag = Some(l);
                break;
            }
        }
    }
    let lag = lag?;
    let mut period = peak_at(&r, lag) * RHYTHM_DT;
    // finer: the peak near twice the lag measures two periods with the same frame step
    if (2 * lag + 4) as f64 > 0.0 && ((2 * lag + 4) as f64) < n as f64 * 0.75 {
        for l in 2 * lag - 4..=2 * lag + 4 {
            if l < r.len() && r[l].is_none() {
                r[l] = Some(corr(l));
            }
        }
        let mut k = 2 * lag - 3;
        for l in k + 1..=2 * lag + 3 {
            if rv(&r, l).unwrap_or(f64::NEG_INFINITY) > rv(&r, k).unwrap_or(f64::NEG_INFINITY) {
                k = l;
            }
        }
        let (a, b, c) = (rv(&r, k - 1), rv(&r, k), rv(&r, k + 1));
        if let (Some(a), Some(b), Some(c)) = (a, b, c) {
            if b >= 0.25 && b >= a && b >= c {
                let p2 = peak_at(&r, k) * RHYTHM_DT / 2.0;
                if (p2 - period).abs() < 1.6 * RHYTHM_DT {
                    period = p2;
                }
            }
        }
    }
    // the two halves of a back-and-forth often differ: then the strongest lag is the whole movement, and its
    // half counts as well if it is a rhythm of its own
    let h = (lag as f64 / 2.0).round() as usize;
    let peak_near = |l: usize| -> Option<usize> {
        let mut k = l;
        for j in [l.wrapping_sub(1), l + 1] {
            if let (Some(rj), Some(rk)) = (rv(&r, j), rv(&r, k)) {
                if rj > rk {
                    k = j;
                }
            }
        }
        let (a, b, c) = (rv(&r, k.wrapping_sub(1)), rv(&r, k)?, rv(&r, k + 1));
        match (a, c) {
            (Some(a), Some(c)) if b >= a && b >= c => Some(k),
            _ => None,
        }
    };
    let hk = if h >= lo { peak_near(h) } else { None };
    let half = match hk {
        Some(k) if k >= lo && rv(&r, k).unwrap_or(0.0) >= 0.3 => Some(period / 2.0),
        _ => None,
    };
    // phase from the curve's fundamental around the middle: a turn is half a pulse after the most motion
    let pulse = half.unwrap_or(period);
    let mid = (n as f64 - 1.0) / 2.0;
    let (mut c, mut s) = (0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let w = 2.0 * std::f64::consts::PI * (i as f64 - mid) * RHYTHM_DT / pulse;
        c += v * w.cos();
        s += v * w.sin();
    }
    let most = s.atan2(c) / (2.0 * std::f64::consts::PI) * pulse;
    Some(Rhythm {
        period,
        half,
        turn: t0 + mid * RHYTHM_DT + most + pulse / 2.0,
        q: top,
    })
}

/// Speed factor that puts motion pulses of `period` on the beat: one pulse per beat, two, or one per two beats –
/// whichever needs the least change, at most ±12 %; None if none fits
pub fn beat_fit(period: f64, bpm: f64) -> Option<f64> {
    // (also None for NaN)
    if bpm.is_nan() || bpm <= 0.0 {
        return None;
    }
    let beat = 60.0 / bpm;
    let mut rate: Option<f64> = None;
    for k in [1.0, 2.0, 0.5] {
        let f = period * k / beat;
        if (0.88..=1.12).contains(&f) && rate.is_none_or(|r| (f - 1.0).abs() < (r - 1.0).abs()) {
            rate = Some(f);
        }
    }
    rate
}

fn shifts(rx: i32, ry: i32) -> Vec<(i32, i32)> {
    let mut l = Vec::new();
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            l.push((dx, dy));
        }
    }
    // (stable: equal distances keep their order, like Array.sort in the browser)
    l.sort_by_key(|(x, y)| x.abs() + y.abs());
    l
}

/// How fast what is in the picture moves – not the camera. Block matching on the small gray picture: first the
/// camera (the best shift of the whole frame), then how far each 4×4 block with some texture moved on top of
/// that. The fastest 6 % of the blocks count → pixels (of 64) per frame step
pub fn action_motion(a: &[f32], b: &[f32], w: usize) -> f64 {
    const B: usize = 4;
    if w == 0 || a.len() != b.len() {
        return 0.0;
    }
    let h = a.len() / w;
    if w < 4 * B || h < 4 * B {
        return 0.0;
    }
    type Shifts = Vec<(i32, i32)>;
    thread_local! {
        static SH: (Shifts, Shifts) = (shifts(6, 4), shifts(12, 12));
    }
    SH.with(|(cam_shifts, block_shifts)| {
        let (wi, hi) = (w as i32, h as i32);
        let at = |p: &[f32], x: i32, y: i32| -> f64 {
            if x < 0 || y < 0 || x >= wi || y >= hi {
                0.0
            } else {
                p[(y * wi + x) as usize] as f64
            }
        };
        // the camera: the shift that fits the whole frame best (every 2nd pixel of the middle)
        let (mut cb, mut cx, mut cy) = (f64::INFINITY, 0i32, 0i32);
        for &(dx, dy) in cam_shifts {
            let mut s = 0.0;
            let mut y = 4;
            'rows: while y < hi - 4 {
                let mut x = 6;
                while x < wi - 6 {
                    s += (at(a, x, y) - at(b, x + dx, y + dy)).abs();
                    x += 2;
                }
                if s >= cb {
                    break 'rows;
                }
                y += 2;
            }
            if s < cb {
                cb = s;
                cx = dx;
                cy = dy;
            }
        }
        let bi = B as i32;
        let diff = |x0: i32, y0: i32, dx: i32, dy: i32, max: f64| -> f64 {
            if x0 + dx < 0 || y0 + dy < 0 || x0 + dx + bi > wi || y0 + dy + bi > hi {
                return f64::INFINITY;
            }
            let mut s = 0.0;
            for y in y0..y0 + bi {
                if s >= max {
                    break;
                }
                for x in x0..x0 + bi {
                    s += (at(a, x, y) - at(b, x + dx, y + dy)).abs();
                }
            }
            s
        };
        let mut mags: Vec<f64> = Vec::new();
        let mut y0 = 0;
        while y0 + bi <= hi {
            let mut x0 = 0;
            while x0 + bi <= wi {
                let mut tex = 0.0;
                for y in y0..y0 + bi {
                    for x in x0..x0 + bi {
                        tex += (at(a, (x + 1).min(wi - 1), y) - at(a, x, y)).abs()
                            + (at(a, x, (y + 1).min(hi - 1)) - at(a, x, y)).abs();
                    }
                }
                if tex >= (B * B * 6) as f64 {
                    let still = diff(x0, y0, cx, cy, f64::INFINITY);
                    if still.is_finite() {
                        // only a clearly better place counts as a move; a far place has to fit better than a
                        // near one (2 per pixel of way)
                        let mut bc = (0.7 * still).min(still - (B * B * 5) as f64);
                        let (mut mx, mut my, mut moved) = (0i32, 0i32, false);
                        for &(dx, dy) in block_shifts {
                            let pen = 2.0 * (dx.abs() + dy.abs()) as f64;
                            if pen >= bc {
                                break;
                            }
                            let s = diff(x0, y0, cx + dx, cy + dy, bc - pen);
                            if s + pen < bc {
                                bc = s + pen;
                                mx = dx;
                                my = dy;
                                moved = true;
                            }
                        }
                        mags.push(if moved {
                            ((mx * mx + my * my) as f64).sqrt()
                        } else {
                            0.0
                        });
                    }
                }
                x0 += bi;
            }
            y0 += bi;
        }
        if mags.is_empty() {
            return 0.0;
        }
        mags.sort_by(|p, q| q.total_cmp(p));
        let k = ((mags.len() as f64 * 0.06).round() as usize)
            .max(2)
            .min(mags.len());
        mags[..k].iter().sum::<f64>() / k as f64
    })
}

/// The speed of the action from frames of a video → motion per second, or None (a cut, too few frames)
pub fn action_speed(fr: &[(f64, Sig)], gray_w: usize) -> Option<f64> {
    let mut pts = Vec::new();
    for w in fr.windows(2) {
        let dt = w[1].0 - w[0].0;
        if dt <= 0.0 {
            continue;
        }
        if smart::is_cut(&w[0].1, &w[1].1) {
            return None;
        }
        pts.push(action_motion(&w[0].1.gray, &w[1].1.gray, gray_w) / dt);
    }
    (pts.len() >= 5).then(|| pts.iter().sum::<f64>() / pts.len() as f64)
}

/// The speeds seen so far: "fast" and "calm" are relative to the library
#[derive(Default, Clone, Debug)]
pub struct SpeedSeen {
    seen: std::collections::VecDeque<f64>,
    sorted: Option<Vec<f64>>,
}
impl SpeedSeen {
    pub fn note(&mut self, v: Option<f64>) {
        let Some(v) = v.filter(|v| *v >= 0.0 && v.is_finite()) else {
            return;
        };
        self.seen.push_back(v);
        if self.seen.len() > 600 {
            self.seen.pop_front();
        }
        self.sorted = None;
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.seen.len()
    }
    /// Where a speed stands among those seen (0 = calmest, 1 = fastest); None while fewer than 8 are known
    pub fn pct(&mut self, v: Option<f64>) -> Option<f64> {
        let v = v?;
        if self.seen.len() < 8 {
            return None;
        }
        let s = self.sorted.get_or_insert_with(|| {
            let mut s: Vec<f64> = self.seen.iter().copied().collect();
            s.sort_by(|a, b| a.total_cmp(b));
            s
        });
        let below = s.partition_point(|&x| x < v);
        let upto = s.partition_point(|&x| x <= v);
        Some((below as f64 + (upto - below) as f64 / 2.0) / s.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_fit_picks_the_smallest_change() {
        // 120 BPM = 0.5 s per beat
        assert!((beat_fit(0.5, 120.0).unwrap() - 1.0).abs() < 1e-9);
        assert!((beat_fit(0.26, 120.0).unwrap() - 1.04).abs() < 1e-9); // two pulses per beat
        assert!((beat_fit(1.0, 120.0).unwrap() - 1.0).abs() < 1e-9); // one per two beats
        assert!(beat_fit(0.7, 120.0).is_none());
        assert!(beat_fit(0.5, 0.0).is_none());
    }

    #[test]
    fn rhythm_of_a_steady_motion() {
        // motion pulsing every 0.4 s, sampled at 30 fps for 1.8 s
        let pts: Vec<(f64, f64)> = (0..54)
            .map(|i| {
                let t = 10.0 + i as f64 / 30.0;
                (
                    t,
                    2.0 + 1.5 * (2.0 * std::f64::consts::PI * (t - 10.0) / 0.4).sin(),
                )
            })
            .collect();
        let r = rhythm_of(&pts).expect("a rhythm");
        assert!((r.period - 0.4).abs() < 0.02, "{r:?}");
        assert!(r.q > 0.5);
        assert!(r.turn > 9.0 && r.turn < 12.5);
        // no motion: none
        let flat: Vec<(f64, f64)> = (0..54).map(|i| (i as f64 / 30.0, 0.0)).collect();
        assert!(rhythm_of(&flat).is_none());
        assert!(rhythm_of(&pts[..10]).is_none());
    }

    #[test]
    fn action_motion_sees_a_moving_block() {
        let (w, h) = (64usize, 36usize);
        // a textured, still background (it anchors the camera) and a textured block that moves
        let noise = |x: usize, y: usize, k: usize| -> f32 {
            let v = (x * 7919 + y * 104_729 + k * 31) % 1009;
            (v % 200) as f32 + 20.0
        };
        let pic = |ox: usize| -> Vec<f32> {
            let mut v: Vec<f32> = (0..w * h).map(|i| noise(i % w, i / w, 1)).collect();
            for y in 10..22 {
                for x in 0..12 {
                    let xx = x + ox;
                    if xx < w {
                        v[y * w + xx] = noise(x, y, 2);
                    }
                }
            }
            v
        };
        let still = action_motion(&pic(10), &pic(10), w);
        let moved = action_motion(&pic(10), &pic(15), w);
        assert!(still < 0.5, "{still}");
        assert!(moved > 3.0, "{moved}");
    }

    #[test]
    fn speed_ranking() {
        let mut s = SpeedSeen::default();
        assert_eq!(s.pct(Some(1.0)), None);
        for v in 1..=10 {
            s.note(Some(v as f64));
        }
        s.note(None);
        s.note(Some(f64::NAN));
        assert_eq!(s.len(), 10);
        assert!((s.pct(Some(1.0)).unwrap() - 0.05).abs() < 1e-9);
        assert!((s.pct(Some(10.0)).unwrap() - 0.95).abs() < 1e-9);
        assert!((s.pct(Some(100.0)).unwrap() - 1.0).abs() < 1e-9);
    }
}
