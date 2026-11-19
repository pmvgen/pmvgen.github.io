// Image analysis for the smart crop and for picking good moments – a port of pmvsmart.js.
// Measured on a tiny image (at most 64 px wide): skin share, contrast (edges) and – with a second frame – motion.

pub const MAX: u32 = 64;

pub fn tiny(w: u32, h: u32) -> (u32, u32) {
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    if w >= h {
        (MAX.max(8), ((MAX as f64 * h / w).round() as u32).max(8))
    } else {
        (((MAX as f64 * w / h).round() as u32).max(8), MAX.max(8))
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sig {
    pub focus: (f64, f64),
    pub skin: f64,
    pub contrast: f64,
    /// the tiny picture's gray values (motion between frames) – not saved: only needed while preparing
    #[serde(skip)]
    pub gray: Vec<f32>,
    pub hist: [f64; 16],
    pub color: (f64, f64, f64),
    pub lum: f64,
}

fn is_skin(r: f64, g: f64, b: f64) -> bool {
    r > 95.0
        && g > 40.0
        && b > 20.0
        && r > g
        && r > b
        && r - g > 15.0
        && r.max(g).max(b) - r.min(g).min(b) > 15.0
}

/// px: tiny picture, `bpp` bytes per pixel (3 = RGB, 4 = RGBA)
pub fn measure(px: &[u8], sw: usize, sh: usize, bpp: usize) -> Sig {
    let all = sw * sh;
    let mut gray = vec![0f32; all];
    let (mut sr, mut sg, mut sb) = (0.0, 0.0, 0.0);
    let mut hist = [0f64; 16];
    for (i, gv) in gray.iter_mut().enumerate() {
        let j = i * bpp;
        let (r, g, b) = (px[j] as f64, px[j + 1] as f64, px[j + 2] as f64);
        let v = 0.299 * r + 0.587 * g + 0.114 * b;
        *gv = v as f32;
        hist[((v as i32) >> 4).clamp(0, 15) as usize] += 1.0;
        sr += r;
        sg += g;
        sb += b;
    }
    let (mut wx, mut wy, mut wsum, mut skin, mut edge) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for y in 1..sh.saturating_sub(1) {
        for x in 1..sw.saturating_sub(1) {
            let i = y * sw + x;
            let j = i * bpp;
            let s = if is_skin(px[j] as f64, px[j + 1] as f64, px[j + 2] as f64) {
                1.0
            } else {
                0.0
            };
            let gxy = (gray[i + 1] - gray[i - 1]).abs() as f64
                + (gray[i + sw] - gray[i - sw]).abs() as f64;
            let e = (gxy / 80.0).min(1.0);
            skin += s;
            edge += e;
            let cx = x as f64 / sw as f64 - 0.5;
            let cy = y as f64 / sh as f64 - 0.5;
            let prior = (-(cx * cx + cy * cy) / 0.5).exp();
            let wgt = (0.7 * s + 0.3 * e) * prior;
            wx += wgt * (x as f64 / sw as f64);
            wy += wgt * (y as f64 / sh as f64);
            wsum += wgt;
        }
    }
    let n = ((sw.saturating_sub(2)) * (sh.saturating_sub(2))).max(1) as f64;
    let a = all.max(1) as f64;
    for h in hist.iter_mut() {
        *h /= a;
    }
    Sig {
        focus: if wsum > n * 0.02 {
            (wx / wsum, wy / wsum)
        } else {
            (0.5, 0.5)
        },
        skin: skin / n,
        contrast: edge / n,
        gray,
        hist,
        color: (sr / a, sg / a, sb / a),
        lum: (0.299 * sr + 0.587 * sg + 0.114 * sb) / a,
    }
}

/// How well two frames fit together at a cut (0 = equal, larger = more different)
pub fn match_dist(a: Option<&Sig>, b: Option<&Sig>) -> f64 {
    let (Some(a), Some(b)) = (a, b) else {
        return 0.5;
    };
    let dc = ((a.color.0 - b.color.0).powi(2)
        + (a.color.1 - b.color.1).powi(2)
        + (a.color.2 - b.color.2).powi(2))
    .sqrt()
        / 441.0;
    let dl = (a.lum - b.lum).abs() / 255.0;
    let df = ((a.focus.0 - b.focus.0).powi(2) + (a.focus.1 - b.focus.1).powi(2)).sqrt() / 1.414;
    1.2 * dc + 0.8 * dl + 1.0 * df
}

pub fn motion(a: &Sig, b: &Sig) -> f64 {
    if a.gray.is_empty() || a.gray.len() != b.gray.len() {
        return 0.0;
    }
    let s: f64 = a
        .gray
        .iter()
        .zip(&b.gray)
        .map(|(x, y)| (x - y).abs() as f64)
        .sum();
    (s / a.gray.len() as f64 / 40.0).min(1.0)
}

/// Did the scene change between two frames (a hard cut)?
pub fn is_cut(a: &Sig, b: &Sig) -> bool {
    let h: f64 = a
        .hist
        .iter()
        .zip(&b.hist)
        .map(|(x, y)| (x - y).abs())
        .sum::<f64>()
        / 2.0;
    let dc = ((a.color.0 - b.color.0).powi(2)
        + (a.color.1 - b.color.1).powi(2)
        + (a.color.2 - b.color.2).powi(2))
    .sqrt()
        / 441.0;
    motion(a, b) > 0.5 && (h > 0.35 || dc > 0.18)
}

/// How "good" a spot is: motion counts most, then skin, then contrast
pub fn spot_score(a: &Sig, b: &Sig, bonus: f64) -> f64 {
    3.0 * motion(a, b) + 1.2 * (a.skin * 2.5).min(1.0) + 0.6 * (a.contrast * 3.0).min(1.0) + bonus
}
