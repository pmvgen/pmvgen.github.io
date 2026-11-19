// Bars and phrases from the beats – a port of bars.js: which beat starts a bar (the "one") and which
// bar starts a new phrase (4 bars).

const PER: usize = 4; // 4/4 – what PMV music almost always is

#[derive(Clone, Debug)]
pub struct Bars {
    pub downbeat: Vec<bool>,
    pub pos: Vec<u8>,
    pub phrase: Vec<bool>,
}

/// mono: the song (channels averaged) at `sr`
fn accents(mono: &[f32], sr: f64, beats: &[f64]) -> Vec<f32> {
    let frames = mono.len();
    let n = beats.len();
    let a = 1.0 - (-2.0 * std::f64::consts::PI * 150.0 / sr).exp(); // one-pole lowpass at about 150 Hz
    let span = |t0: f64, t1: f64| -> (f64, f64) {
        let i0 = (t0 * sr).floor().max(0.0) as usize;
        let i1 = (frames as f64).min(((i0 + 1) as f64).max((t1 * sr).floor())) as usize;
        let mut lo = 0.0;
        let mut al = 0.0;
        let mut y = 0.0;
        let step = 2; // every second sample is plenty
        let mut i = i0;
        while i < i1 {
            let x = mono[i] as f64;
            y += a * step as f64 * (x - y);
            lo += y * y;
            al += x * x;
            i += step;
        }
        let m = ((i1 as f64 - i0 as f64) / step as f64).max(1.0);
        (lo / m, al / m)
    };
    let mut low_e = vec![0.0f32; n * 2];
    let mut all_e = vec![0.0f32; n * 2];
    for k in 0..n {
        let t = beats[k];
        let next = beats
            .get(k + 1)
            .copied()
            .filter(|&v| v != 0.0)
            .unwrap_or(t + 0.5);
        let len = next - t;
        let pre = span(t - 0.3f64.min(0.4 * len), t - 0.02);
        let hit = span(t - 0.02, t + 0.15f64.min(0.4 * len));
        low_e[2 * k] = pre.0 as f32;
        low_e[2 * k + 1] = hit.0 as f32;
        all_e[2 * k] = pre.1 as f32;
        all_e[2 * k + 1] = hit.1 as f32;
    }
    let e = 1e-9;
    let raw: Vec<f32> = (0..n)
        .map(|k| {
            let (lp, lh) = (low_e[2 * k] as f64, low_e[2 * k + 1] as f64);
            let (ap, ah) = (all_e[2 * k] as f64, all_e[2 * k + 1] as f64);
            let jl = ((lh + e).ln() - (lp + e).ln()).max(0.0);
            let ja = ((ah + e).ln() - (ap + e).ln()).max(0.0);
            (0.6 * jl + 0.4 * ja + 0.25 * (lh + e).ln()) as f32
        })
        .collect();
    relative(&raw, 8)
}

// Value minus the average of its surroundings (±r), in units of the local spread
fn relative(v: &[f32], r: usize) -> Vec<f32> {
    let n = v.len();
    (0..n)
        .map(|k| {
            let a = k.saturating_sub(r);
            let b = (k + r).min(n - 1);
            let c = (b - a + 1) as f64;
            let mean = v[a..=b].iter().map(|&x| x as f64).sum::<f64>() / c;
            let q: f64 = v[a..=b].iter().map(|&x| (x as f64 - mean).powi(2)).sum();
            let mut sd = (q / c).sqrt();
            if sd == 0.0 {
                sd = 1.0;
            }
            ((v[k] as f64 - mean) / sd) as f32
        })
        .collect()
}

// Which of the `per` positions holds the "one" – per element: globally the best, moved in a window only
// when another is clearly better there
fn phases<V: Copy + Into<f64>>(
    vals: &[V],
    per: usize,
    win: usize,
    hop: usize,
    margin: f64,
) -> Vec<u8> {
    let n = vals.len();
    let sums = |a: usize, b: usize| {
        let mut s = vec![0.0f64; per];
        for k in a..b {
            s[k % per] += vals[k].into();
        }
        s
    };
    let best = |s: &[f64]| {
        let mut bi = 0;
        for (i, x) in s.iter().enumerate() {
            if *x > s[bi] {
                bi = i;
            }
        }
        bi
    };
    let mut out = vec![0u8; n];
    let mut cur = best(&sums(0, n));
    let mut h = 0;
    while h * hop < n {
        let a = (h * hop).saturating_sub(((win - hop) as f64 / 2.0).round() as usize);
        let b = n.min(a + win);
        let s = sums(a, b);
        let bi = best(&s);
        if bi != cur && s[bi] - s[cur] > margin {
            cur = bi;
        }
        for o in out.iter_mut().take(n.min((h + 1) * hop)).skip(h * hop) {
            *o = cur as u8;
        }
        h += 1;
    }
    out
}

pub fn analyze_bars(mono: &[f32], sr: f64, beats: &[f64], energy: &[f64]) -> Option<Bars> {
    if beats.len() < 4 * PER || mono.len() < 4 {
        return None;
    }
    let acc = accents(mono, sr, beats);
    let n = beats.len();
    let ph = phases(&acc, PER, 8 * PER, PER, 2.5);
    let mut pos = vec![0u8; n];
    let mut downbeat = vec![false; n];
    for k in 0..n {
        pos[k] = ((k as i64 - ph[k] as i64).rem_euclid(PER as i64)) as u8;
        downbeat[k] = pos[k] == 0;
    }
    // Phrases: per bar the accent of its "one" plus how much louder the bar is than the one before
    let bars: Vec<usize> = (0..n).filter(|&k| downbeat[k]).collect();
    let mean = |a: usize, z: usize| {
        let mut s = 0.0;
        let mut c = 0.0;
        for j in a..z {
            s += energy.get(j).copied().unwrap_or(0.0);
            c += 1.0;
        }
        if c > 0.0 {
            s / c
        } else {
            0.0
        }
    };
    let bar_val: Vec<f64> = bars
        .iter()
        .enumerate()
        .map(|(b, &k)| {
            let end = bars.get(b + 1).copied().unwrap_or(n);
            let rise = if b > 0 {
                mean(k, end) - mean(bars[b - 1], k)
            } else {
                0.0
            };
            acc[k] as f64 + 4.0 * rise.abs()
        })
        .collect();
    let mut phrase = vec![false; n];
    if bars.len() >= 8 {
        let pp = phases(&bar_val, 4, 16, 4, 3.0);
        for (b, &k) in bars.iter().enumerate() {
            if (b as i64 - pp[b] as i64).rem_euclid(4) == 0 {
                phrase[k] = true;
            }
        }
    } else {
        for (b, &k) in bars.iter().enumerate() {
            if b % 4 == 0 {
                phrase[k] = true;
            }
        }
    }
    Some(Bars {
        downbeat,
        pos,
        phrase,
    })
}
