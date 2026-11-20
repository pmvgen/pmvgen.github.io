// Shot boundaries: where a video cuts to another scene – a hard cut or a gradual transition (fade, dissolve,
// wipe) – told apart from motion, camera moves, flashes and glitches.
//
// Hybrid, every frame:
// 1. A classical detector measures each frame against the one before on a 64×36 picture: how much of the
//    picture can't be found again nearby after compensating for motion and brightness (block matching), how
//    much the colours change (histogram) and how many edges appear / vanish. A frame that differs a lot more
//    than its neighbours do is a candidate; a picture that comes back within 0.2 s was a flash or a glitch.
// 2. A neural network made for exactly this, TransNetV2 (Souček & Lokoč, MIT licence, ONNX run by tract in
//    pure Rust), looks at 100 frames at a time on a 48×27 picture and gives each frame the probability that
//    a new shot starts there. It decides the doubtful candidates, or – for a PMV used as template – runs over
//    every frame, the classical measures then only filter flashes and glitches.

use crate::ff;
use anyhow::{bail, Result};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use tract_onnx::prelude::*;

/// The network's picture
pub const NW: usize = 48;
pub const NH: usize = 27;
const NSZ: usize = NW * NH * 3;
/// The classical measuring picture
const CW: usize = 64;
const CH: usize = 36;
/// Kept of every frame for looking further back and ahead (flashes, gradual changes)
const SW: usize = 32;
const SH: usize = 18;
/// The template's measuring picture (template.rs), decoded alongside when asked for
pub const TW: usize = 96;
pub const TH: usize = 54;

/// The network's window: 100 frames, of which the middle 50 are trusted (25 on each side are context)
const WIN: usize = 100;
const KEEP: usize = 50;
const CTX: usize = 25;

/// Network probability from which a frame starts a new shot (TransNetV2's own threshold)
const P_CUT: f32 = 0.5;

// ---------- The network ----------

static MODEL_BYTES: &[u8] = include_bytes!("../models/transnetv2.onnx");

type Net = Arc<TypedSimplePlan>;

fn net() -> Option<&'static Net> {
    static NET: OnceLock<Option<Net>> = OnceLock::new();
    NET.get_or_init(|| {
        let load = || -> TractResult<Net> {
            tract_onnx::onnx()
                .model_for_read(&mut std::io::Cursor::new(MODEL_BYTES))?
                .with_input_fact(0, f32::fact([1, WIN, NH, NW, 3]).into())?
                .into_optimized()?
                .into_runnable()
        };
        match load() {
            Ok(n) => Some(n),
            Err(e) => {
                eprintln!("[pmvgen] shot detection network unavailable: {e:#}");
                None
            }
        }
    })
    .as_ref()
}

/// Is the network there (it is built in – only a broken build lacks it)?
pub fn network_ready() -> bool {
    net().is_some()
}

/// At most this many network windows run at once in the whole program (each one keeps a core busy)
fn slots() -> &'static (Mutex<usize>, Condvar) {
    static S: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    S.get_or_init(|| {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        (Mutex::new((n * 3 / 4).clamp(1, 16)), Condvar::new())
    })
}
struct Slot;
impl Slot {
    fn take() -> Slot {
        let (m, cv) = slots();
        let mut n = m.lock().unwrap_or_else(|e| e.into_inner());
        while *n == 0 {
            n = cv.wait(n).unwrap_or_else(|e| e.into_inner());
        }
        *n -= 1;
        Slot
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        let (m, cv) = slots();
        *m.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        cv.notify_one();
    }
}

/// The network over 100 frames from frame `first` (may be negative: before the start / past the end the
/// first / last frame is repeated, as TransNetV2's own code does) → its 100 values
fn run_window(net: &Net, rgb: &[u8], n: usize, first: isize) -> Option<Vec<f32>> {
    let _slot = Slot::take();
    let mut x = tract_ndarray::Array5::<f32>::zeros((1, WIN, NH, NW, 3));
    {
        let buf = x.as_slice_mut()?;
        for k in 0..WIN {
            let f = (first + k as isize).clamp(0, n as isize - 1) as usize;
            let src = &rgb[f * NSZ..(f + 1) * NSZ];
            for (d, s) in buf[k * NSZ..(k + 1) * NSZ].iter_mut().zip(src) {
                *d = *s as f32;
            }
        }
    }
    let out = net.run(tvec!(x.into_tensor().into())).ok()?;
    let a = out.first()?.to_plain_array_view::<f32>().ok()?;
    let v: Vec<f32> = a.iter().copied().collect();
    (v.len() >= WIN).then_some(v)
}

/// Network probabilities for the frames in `ranges` (NaN elsewhere), windows spread over threads. Each
/// window trusts its middle 50 frames; a track of at most 90 frames is done in one window, centred.
fn net_probs(
    rgb: &[u8],
    n: usize,
    ranges: &[(usize, usize)],
    cancel: &AtomicBool,
) -> Option<Vec<f32>> {
    let net = net()?;
    // (first frame of the window, its positions kept)
    let mut jobs: Vec<(isize, usize, usize)> = Vec::new();
    if n <= 90 {
        let c = (WIN - n) / 2;
        jobs.push((-(c as isize), c, c + n));
    } else {
        let mut last: Option<usize> = None;
        for &(a, b) in ranges {
            let mut s = a;
            while s < b.min(n) {
                if last.is_none_or(|l| s >= l + KEEP) {
                    jobs.push((s as isize - CTX as isize, CTX, CTX + KEEP));
                    last = Some(s);
                }
                s += KEEP;
            }
        }
    }
    let mut p = vec![f32::NAN; n];
    if jobs.is_empty() {
        return Some(p);
    }
    let threads = std::thread::available_parallelism()
        .map(|t| t.get())
        .unwrap_or(4)
        .min(jobs.len())
        .clamp(1, 16);
    let next = std::sync::atomic::AtomicUsize::new(0);
    // (window's first frame, positions kept, its values)
    type Done = (isize, usize, usize, Vec<f32>);
    let results: Mutex<Vec<Done>> = Mutex::new(Vec::new());
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(first, a, b)) = jobs.get(k) else {
                    break;
                };
                if let Some(v) = run_window(net, rgb, n, first) {
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((first, a, b, v));
                }
            });
        }
    });
    // (the network marks the last frame of a shot – here a frame's value says that a new shot starts with it)
    for (first, a, b, v) in results.into_inner().unwrap_or_else(|e| e.into_inner()) {
        for (k, x) in v.iter().enumerate().take(b).skip(a) {
            let f = first + k as isize + 1;
            if f >= 0 {
                if let Some(d) = p.get_mut(f as usize) {
                    *d = *x;
                }
            }
        }
    }
    Some(p)
}

// ---------- Classical measures ----------

/// What is kept of a frame
#[derive(Clone)]
struct Small {
    g: Vec<f32>, // SW×SH gray
    st: Vec<u8>, // CW×CH structure: gradient magnitude relative to the picture's mean (light-independent)
    hist: [f32; 64],
    luma: f32,     // 0–1
    contrast: f32, // mean absolute deviation of the gray, 0–255
}

/// A frame against the one before
#[derive(Clone, Copy, Debug, Default)]
pub struct Pair {
    /// mean of the blocks' remaining difference after motion and brightness compensation (0 = all found again,
    /// ~1 = unrelated pictures)
    pub mc: f32,
    /// share of the blocks that weren't found again
    pub mcf: f32,
    /// colour histogram change (0–1)
    pub hd: f32,
    /// edge change ratio (0–1)
    pub ecr: f32,
}

fn gray_of(rgb: &[u8], w: usize, h: usize) -> Vec<f32> {
    (0..w * h)
        .map(|i| {
            let j = i * 3;
            0.299 * rgb[j] as f32 + 0.587 * rgb[j + 1] as f32 + 0.114 * rgb[j + 2] as f32
        })
        .collect()
}

fn hist_of(rgb: &[u8]) -> [f32; 64] {
    let mut h = [0f32; 64];
    let n = rgb.len() / 3;
    for p in rgb.as_chunks::<3>().0 {
        let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let u = (b - y) * 0.565 + 128.0;
        let v = (r - y) * 0.713 + 128.0;
        let q = |x: f32| ((x / 64.0) as usize).min(3);
        h[q(y) * 16 + q(u) * 4 + q(v)] += 1.0;
    }
    for x in h.iter_mut() {
        *x /= n.max(1) as f32;
    }
    h
}

fn hist_dist(a: &[f32; 64], b: &[f32; 64]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / 2.0
}

fn downscale(g: &[f32], w: usize, h: usize, f: usize) -> Vec<f32> {
    let (ow, oh) = (w / f, h / f);
    let mut o = vec![0f32; ow * oh];
    for y in 0..oh {
        for x in 0..ow {
            let mut s = 0.0;
            for yy in 0..f {
                for xx in 0..f {
                    s += g[(y * f + yy) * w + x * f + xx];
                }
            }
            o[y * ow + x] = s / (f * f) as f32;
        }
    }
    o
}

/// The picture's structure: gradient magnitude divided by its mean – stays when the light, the colours or the
/// contrast change (flashes, grading, glow), changes with another picture
fn structure(g: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut m = vec![0f32; w * h];
    let mut tot = 0.0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let gx = g[i + 1] - g[i - 1];
            let gy = g[i + w] - g[i - w];
            let v = (gx * gx + gy * gy).sqrt();
            m[i] = v;
            tot += v;
        }
    }
    let mean = tot / ((w - 2) * (h - 2)) as f32 + 2.0;
    for v in m.iter_mut() {
        *v = (*v / mean).min(4.0) * 40.0;
    }
    m
}

/// Motion- and brightness-compensated block difference of `cur` against `prev` (w×h gray): every block of
/// `cur` (bw×bh) is searched for in `prev` within ±rx / ±ry pixels, means removed. → (mean ratio, share > 0.55)
#[allow(clippy::too_many_arguments)]
fn block_match(
    prev: &[f32],
    cur: &[f32],
    w: usize,
    h: usize,
    bw: usize,
    bh: usize,
    rx: isize,
    ry: isize,
) -> (f32, f32) {
    let n = (bw * bh) as f32;
    let (mut sum, mut bad, mut cnt) = (0f32, 0f32, 0f32);
    for by in (0..=h - bh).step_by(bh) {
        for bx in (0..=w - bw).step_by(bw) {
            let mut mc = 0.0;
            for y in 0..bh {
                for x in 0..bw {
                    mc += cur[(by + y) * w + bx + x];
                }
            }
            mc /= n;
            let mut tc = 0.0;
            for y in 0..bh {
                for x in 0..bw {
                    tc += (cur[(by + y) * w + bx + x] - mc).abs();
                }
            }
            tc /= n;
            let mut best = f32::INFINITY;
            let mut best_tp = 0.0;
            let mut best_mean = 0.0;
            for dy in -ry..=ry {
                let py = by as isize + dy;
                if py < 0 || py as usize + bh > h {
                    continue;
                }
                for dx in -rx..=rx {
                    let px = bx as isize + dx;
                    if px < 0 || px as usize + bw > w {
                        continue;
                    }
                    let (px, py) = (px as usize, py as usize);
                    let mut mp = 0.0;
                    for y in 0..bh {
                        for x in 0..bw {
                            mp += prev[(py + y) * w + px + x];
                        }
                    }
                    mp /= n;
                    let mut sad = 0.0;
                    for y in 0..bh {
                        let (rc, rp) = ((by + y) * w + bx, (py + y) * w + px);
                        for x in 0..bw {
                            sad += ((cur[rc + x] - mc) - (prev[rp + x] - mp)).abs();
                        }
                    }
                    sad /= n;
                    if sad < best {
                        best = sad;
                        best_mean = mp;
                        let mut tp = 0.0;
                        for y in 0..bh {
                            for x in 0..bw {
                                tp += (prev[(py + y) * w + px + x] - mp).abs();
                            }
                        }
                        best_tp = tp / n;
                    }
                }
            }
            if !best.is_finite() {
                continue;
            }
            // flat blocks (sky, walls, black bars) say little – unless their brightness jumped
            let ratio = if tc < 3.0 && best_tp < 3.0 {
                if (mc - best_mean).abs() > 25.0 {
                    1.0
                } else {
                    continue;
                }
            } else {
                best / ((tc + best_tp) / 2.0 + 4.0)
            };
            let r = ratio.min(1.5);
            sum += r;
            if r > 0.55 {
                bad += 1.0;
            }
            cnt += 1.0;
        }
    }
    if cnt == 0.0 {
        (0.0, 0.0)
    } else {
        (sum / cnt, bad / cnt)
    }
}

fn edges(g: &[f32], w: usize, h: usize) -> Vec<bool> {
    let mut m = vec![0f32; w * h];
    let mut tot = 0.0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let v = (g[i + 1] - g[i - 1]).abs() + (g[i + w] - g[i - w]).abs();
            m[i] = v;
            tot += v;
        }
    }
    let thr = (2.0 * tot / ((w - 2) * (h - 2)) as f32).max(24.0);
    m.iter().map(|&v| v > thr).collect()
}

fn dilate(e: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut o = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if e[y * w + x] {
                for yy in y.saturating_sub(1)..(y + 2).min(h) {
                    for xx in x.saturating_sub(1)..(x + 2).min(w) {
                        o[yy * w + xx] = true;
                    }
                }
            }
        }
    }
    o
}

fn ecr(a: &[bool], b: &[bool], w: usize, h: usize) -> f32 {
    let (da, db) = (dilate(a, w, h), dilate(b, w, h));
    let na = a.iter().filter(|&&x| x).count();
    let nb = b.iter().filter(|&&x| x).count();
    if na < 8 && nb < 8 {
        return 0.0;
    }
    let out = a.iter().zip(&db).filter(|(&e, &d)| e && !d).count();
    let inn = b.iter().zip(&da).filter(|(&e, &d)| e && !d).count();
    (out as f32 / na.max(1) as f32).max(inn as f32 / nb.max(1) as f32)
}

/// The frames of a stretch of video as the detector keeps them
pub struct Track {
    pub fps: f64,
    /// time of frame 0 in the video
    pub t0: f64,
    nn: Vec<u8>,
    small: Vec<Small>,
    pairs: Vec<Pair>,
    prev: Option<(Vec<f32>, Vec<bool>)>,
}

impl Track {
    pub fn new(fps: f64, t0: f64) -> Track {
        Track {
            fps,
            t0,
            nn: Vec::new(),
            small: Vec::new(),
            pairs: Vec::new(),
            prev: None,
        }
    }
    pub fn len(&self) -> usize {
        self.small.len()
    }
    pub fn is_empty(&self) -> bool {
        self.small.is_empty()
    }
    pub fn time(&self, i: usize) -> f64 {
        self.t0 + i as f64 / self.fps
    }
    pub fn pair(&self, i: usize) -> Pair {
        self.pairs.get(i).copied().unwrap_or_default()
    }
    /// Add a frame: the network's 48×27 RGB and the 64×36 RGB measuring picture
    pub fn push(&mut self, nn_rgb: &[u8], cls_rgb: &[u8]) {
        self.nn.extend_from_slice(&nn_rgb[..NSZ]);
        let g = gray_of(cls_rgb, CW, CH);
        let e = edges(&g, CW, CH);
        let hist = hist_of(&cls_rgb[..CW * CH * 3]);
        let mean = g.iter().sum::<f32>() / g.len() as f32;
        let contrast = g.iter().map(|v| (v - mean).abs()).sum::<f32>() / g.len() as f32;
        let pair = match (&self.prev, self.small.last()) {
            (Some((pg, pe)), Some(ps)) => {
                let (mc, mcf) = block_match(pg, &g, CW, CH, 8, 6, 6, 4);
                Pair {
                    mc,
                    mcf,
                    hd: hist_dist(&ps.hist, &hist),
                    ecr: ecr(pe, &e, CW, CH),
                }
            }
            _ => Pair::default(),
        };
        self.pairs.push(pair);
        self.small.push(Small {
            g: downscale(&g, CW, CH, 2),
            st: structure(&g, CW, CH)
                .iter()
                .map(|&v| v.round() as u8)
                .collect(),
            hist,
            luma: mean / 255.0,
            contrast,
        });
        self.prev = Some((g, e));
    }

    /// Classical cut strength of frame i against i−1 (0 = nothing changed, ≥ 1 = a clear new picture)
    fn strength(&self, i: usize) -> f32 {
        let p = self.pair(i);
        (0.45 * (p.mc / 0.8).min(1.5)
            + 0.35 * (p.mcf / 0.8).min(1.3)
            + 0.6 * (p.hd / 0.5).min(1.5)
            + 0.3 * p.ecr)
            / 1.4
    }

    /// The frames a and b compared on the small picture (motion compensated) → share of blocks not found again
    fn small_diff(&self, a: usize, b: usize) -> f32 {
        let (x, y) = (&self.small[a], &self.small[b]);
        let (_, f) = block_match(&x.g, &y.g, SW, SH, 4, 3, 4, 3);
        f.max(hist_dist(&x.hist, &y.hist) * 1.2)
    }

    /// How much frame b is another picture than frame a, by structure (motion compensated): share of blocks
    /// not found again
    pub fn scene_dist(&self, a: usize, b: usize) -> f32 {
        let f = |v: &[u8]| v.iter().map(|&x| x as f32).collect::<Vec<f32>>();
        let (x, y) = (f(&self.small[a].st), f(&self.small[b].st));
        block_match(&x, &y, CW, CH, 8, 6, 6, 4).1
    }

    /// Is the change at frame i only a short interruption – a flash, a glitch, a stutter – rather than a new
    /// shot? Either the picture before it comes back within 0.2 s, or frame i is that picture coming back.
    fn transient(&self, i: usize) -> bool {
        if i == 0 {
            return false;
        }
        let m = ((self.fps * 0.2).round() as usize).max(2);
        let leaving = (i + 1..(i + m + 1).min(self.len())).any(|j| self.small_diff(i - 1, j) < 0.3);
        let back =
            (i.saturating_sub(m + 1)..i.saturating_sub(1)).any(|j| self.small_diff(j, i) < 0.3);
        leaving || back
    }
}

/// A new shot
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boundary {
    /// the first frame of the new shot (of a gradual transition: its middle)
    pub i: usize,
    /// the transition's frames [from, to] (from == to == i for a hard cut)
    pub from: usize,
    pub to: usize,
    /// how sure (network probability, or the classical strength)
    pub p: f32,
}

/// How much the network is used
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NetUse {
    /// classical decides the clear cases, the network the doubtful ones; without the network a doubtful
    /// candidate counts as a cut (the safe side for clean cuts)
    Doubtful,
    /// the network looks at every frame (a PMV template: exactness counts more than time)
    Always,
}

/// Gradual transitions in the network's probabilities: a long, lower hill (a dissolve, a fade, a wipe) –
/// hard cuts are decided frame by frame in detect()
fn net_gradual(p: &[f32]) -> Vec<Boundary> {
    let mut out = Vec::new();
    let n = p.len();
    let mut i = 0;
    while i < n {
        if p[i].is_nan() || p[i] < 0.1 {
            i += 1;
            continue;
        }
        let a = i;
        while i < n && p[i] >= 0.1 {
            i += 1;
        }
        let b = i - 1;
        let (mut top, mut at, mut mass) = (0f32, a, 0f32);
        for (k, &v) in p.iter().enumerate().take(b + 1).skip(a) {
            mass += v;
            if v > top {
                top = v;
                at = k;
            }
        }
        if top < P_CUT && b - a >= 2 && mass >= 0.8 {
            out.push(Boundary {
                i: at,
                from: a,
                to: b,
                p: top,
            });
        }
    }
    out
}

/// Find the new shots in a track. Calibrated against TransNetV2 on every frame of a heavily edited music
/// video and by eye: the network misses cuts through flashes and in fast montages, the classical measures take
/// lighting effects and colour changes for cuts – the structure comparison settles both.
pub fn detect(tr: &Track, use_net: NetUse, cancel: &AtomicBool) -> Vec<Boundary> {
    let n = tr.len();
    if n < 2 {
        return Vec::new();
    }
    let s: Vec<f32> = (0..n)
        .map(|i| if i == 0 { 0.0 } else { tr.strength(i) })
        .collect();
    // the usual change around a frame (motion) – a cut stands out against it
    let w = ((tr.fps * 0.6).round() as usize).max(4);
    let base = |i: usize| {
        let mut v: Vec<f32> = (i.saturating_sub(w)..(i + w + 1).min(n))
            .filter(|&k| k + 1 < i || k > i + 1)
            .map(|k| s[k])
            .collect();
        if v.is_empty() {
            return 0.0;
        }
        let m = v.len() / 2;
        v.select_nth_unstable_by(m, |a, b| a.total_cmp(b));
        v[m]
    };
    let peak = |i: usize| (i.saturating_sub(1)..(i + 2).min(n)).all(|k| k == i || s[k] <= s[i]);
    // candidates: a frame that changes a lot more than the ones around it
    let mut cand: Vec<usize> = (1..n)
        .filter(|&i| peak(i) && s[i] >= 0.25 && s[i] / (base(i) + 0.05) >= 1.6)
        .collect();
    // gradual change: the picture 0.5 s apart is another one while no single frame changed much
    let k = ((tr.fps * 0.5).round() as usize).max(3);
    let mut slow: Vec<usize> = Vec::new();
    let mut i = k;
    while i + k < n {
        if tr.small_diff(i - k, i + k) > 0.6 && s[i - k + 1..=i + k].iter().all(|&x| x < 0.9) {
            slow.push(i);
        }
        i += (k / 2).max(1);
    }
    let sd: Vec<f32> = {
        let mut v = vec![f32::NAN; n];
        for &i in &cand {
            v[i] = tr.scene_dist(i - 1, i);
        }
        v
    };
    let clear = |i: usize| s[i] >= 0.9 && sd[i] >= 0.85;
    let net: Option<Vec<f32>> = match use_net {
        NetUse::Always => net_probs(&tr.nn, n, &[(0, n)], cancel),
        NetUse::Doubtful => {
            // the network only where the measures can't tell: neither the same structure nor a clear new picture
            let need: Vec<(usize, usize)> = cand
                .iter()
                .filter(|&&i| sd[i] >= 0.25 && !clear(i))
                .chain(&slow)
                .map(|&i| (i.saturating_sub(4), i + 4))
                .collect();
            if need.is_empty() {
                None
            } else {
                net_probs(&tr.nn, n, &merge(need), cancel)
            }
        }
    };
    let p = |i: usize| net.as_ref().map(|v| v[i]).filter(|x| !x.is_nan());
    // the network's own cuts are candidates too (it sees what the measures miss)
    if let Some(v) = &net {
        for i in 1..n {
            let pk = (i.saturating_sub(1)..(i + 2).min(n))
                .all(|k| k == i || v[k].is_nan() || v[k] <= v[i]);
            if v[i] >= 0.3 && pk && !cand.contains(&i) {
                cand.push(i);
            }
        }
        cand.sort_unstable();
    }
    let mut out: Vec<Boundary> = Vec::new();
    for &i in &cand {
        if tr.transient(i) {
            continue;
        }
        let d = if sd[i].is_nan() {
            tr.scene_dist(i - 1, i)
        } else {
            sd[i]
        };
        let cut = match p(i) {
            Some(q) => (q >= P_CUT && d >= 0.25) || (q >= 0.3 && d >= 0.6) || clear(i),
            // (no network here: a clear new picture, or – doubtful – a changed structure counts, the safe side
            // for clean cuts; the template always has the network)
            None => clear(i) || (use_net == NetUse::Doubtful && d >= 0.6),
        };
        if cut {
            out.push(Boundary {
                i,
                from: i,
                to: i,
                p: p(i).unwrap_or(s[i].min(1.0)),
            });
        }
    }
    // a flash between two cuts close together (picture → white → other picture) is one cut: the new picture
    let flashy = |a: usize, b: usize| {
        (a..b).all(|k| {
            let sm = &tr.small[k];
            sm.luma > 0.8 || sm.luma < 0.05 || sm.contrast < 5.0
        })
    };
    let gap = ((tr.fps * 0.15).round() as usize).max(2);
    let mut merged: Vec<Boundary> = Vec::new();
    for b in out {
        if let Some(l) = merged.last() {
            if b.i - l.i <= gap && flashy(l.i, b.i) {
                merged.pop();
            }
        }
        merged.push(b);
    }
    // gradual transitions the network saw (not where a hard cut already is)
    if let Some(v) = &net {
        for g in net_gradual(v) {
            if !merged.iter().any(|b| b.i + 2 >= g.from && b.i <= g.to + 2) {
                merged.push(g);
            }
        }
    } else if use_net == NetUse::Doubtful {
        // no network: a slow change counts as a transition (safe side)
        for &i in &slow {
            if !merged.iter().any(|b| b.i.abs_diff(i) <= k) {
                merged.push(Boundary {
                    i,
                    from: i.saturating_sub(k / 2),
                    to: (i + k / 2).min(n - 1),
                    p: 0.5,
                });
            }
        }
    }
    merged.sort_by_key(|b| b.i);
    merged
}

fn merge(mut r: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    r.sort();
    let mut o: Vec<(usize, usize)> = Vec::new();
    for (a, b) in r {
        match o.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => o.push((a, b)),
        }
    }
    o
}

// ---------- Decoding ----------

/// Gets every frame's index and its 96×54 RGBA picture (the template's measures)
pub type FrameSink<'a> = dyn FnMut(usize, &[u8]) + 'a;

/// Decode a stretch of a video for the detector: `fps` frames per second from `t0` for `secs` (None: to the
/// end). With `tpl`, every frame's 96×54 RGBA picture is handed to it as well (the template's measures).
/// Returns the track.
pub fn decode(
    path: &Path,
    t0: f64,
    secs: Option<f64>,
    fps: f64,
    mut tpl: Option<&mut FrameSink<'_>>,
    progress: &dyn Fn(f64),
    cancel: &AtomicBool,
) -> Result<Track> {
    let width = if tpl.is_some() { TW } else { CW };
    let mut c = ff::ffmpeg()?;
    if t0 > 0.0 {
        c.arg("-ss").arg(format!("{t0:.3}"));
    }
    c.arg("-i").arg(path);
    if let Some(s) = secs {
        c.arg("-t").arg(format!("{s:.3}"));
    }
    // (one small picture first – then the three sizes from it: cheap even for 4K)
    let mut g = format!(
        "fps={fps},scale=192:108:flags=area,format=rgb24,split={k}[a][b]{c3};\
         [a]scale={NW}:{NH}:flags=bicubic,pad={width}:{NH}[x];\
         [b]scale={CW}:{CH}:flags=area,pad={width}:{CH}[y]",
        k = if tpl.is_some() { 3 } else { 2 },
        c3 = if tpl.is_some() { "[c]" } else { "" },
    );
    if tpl.is_some() {
        g.push_str(&format!(
            ";[c]scale={TW}:{TH}:flags=bilinear[z];[x][y][z]vstack=inputs=3"
        ));
    } else {
        g.push_str(";[x][y]vstack=inputs=2");
    }
    c.args([
        "-an", "-sn", "-dn", "-vf", &g, "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1",
    ]);
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg gave no output"))?;
    let errp = child.stderr.take();
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut e) = errp {
            let _ = std::io::Read::read_to_string(&mut e, &mut s);
        }
        s
    });
    let rows = NH + CH + if tpl.is_some() { TH } else { 0 };
    let mut buf = vec![0u8; width * rows * 3];
    let mut nn = vec![0u8; NSZ];
    let mut cls = vec![0u8; CW * CH * 3];
    let mut tp = vec![0u8; TW * TH * 4];
    let mut tr = Track::new(fps, t0);
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Cancelled");
        }
        match ff::read_full(&mut out, &mut buf) {
            Ok(true) => {}
            _ => break,
        }
        let row = |y: usize, w: usize| &buf[y * width * 3..y * width * 3 + w * 3];
        for y in 0..NH {
            nn[y * NW * 3..(y + 1) * NW * 3].copy_from_slice(row(y, NW));
        }
        for y in 0..CH {
            cls[y * CW * 3..(y + 1) * CW * 3].copy_from_slice(row(NH + y, CW));
        }
        if let Some(f) = tpl.as_mut() {
            for y in 0..TH {
                let r = row(NH + CH + y, TW);
                for x in 0..TW {
                    let (s, d) = (x * 3, (y * TW + x) * 4);
                    tp[d..d + 3].copy_from_slice(&r[s..s + 3]);
                    tp[d + 3] = 255;
                }
            }
            f(tr.len(), &tp);
        }
        tr.push(&nn, &cls);
        if tr.len().is_multiple_of(25) {
            progress(tr.len() as f64 / fps);
        }
    }
    let _ = child.wait();
    let err = err_t.join().unwrap_or_default();
    if tr.is_empty() && !err.trim().is_empty() {
        bail!("{}", err.lines().next().unwrap_or("ffmpeg failed"));
    }
    Ok(tr)
}

/// The shots of a stretch of video: [start, end) in video seconds, transitions left out
pub fn shots_of(tr: &Track, cuts: &[Boundary]) -> Vec<(f64, f64)> {
    let mut v = Vec::new();
    let mut a = 0usize;
    for b in cuts {
        if b.from > a {
            v.push((tr.time(a), tr.time(b.from)));
        }
        a = if b.from == b.to { b.i } else { b.to + 1 };
    }
    if a < tr.len() {
        v.push((tr.time(a), tr.time(tr.len())));
    }
    v
}

/// For checking the detector from the command line: every boundary with its measures
pub fn report(path: &Path, from: f64, secs: Option<f64>, use_net: NetUse) -> Result<String> {
    let pr = ff::probe(path)?;
    let fps = std::env::var("PMVGEN_SHOTS_FPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if pr.fps > 0.0 { pr.fps.min(60.0) } else { 25.0 });
    let cancel = Arc::new(AtomicBool::new(false));
    let t = std::time::Instant::now();
    let tr = decode(path, from, secs, fps, None, &|_| {}, &cancel)?;
    let dec = t.elapsed();
    let cuts = detect(&tr, use_net, &cancel);
    let mut s = format!(
        "{} frames at {fps:.3} fps, decoded in {:.1} s, detected in {:.1} s, network: {}\n",
        tr.len(),
        dec.as_secs_f64(),
        (t.elapsed() - dec).as_secs_f64(),
        network_ready()
    );
    for b in &cuts {
        let p = tr.pair(b.i);
        s.push_str(&format!(
            "{:>6} {:>9.3}s p {:.2} {}  mc {:.2} mcf {:.2} hd {:.2} ecr {:.2}\n",
            b.i,
            tr.time(b.i),
            b.p,
            if b.from == b.to {
                "cut".to_string()
            } else {
                format!("gradual {}–{}", b.from, b.to)
            },
            p.mc,
            p.mcf,
            p.hd,
            p.ecr
        ));
    }
    Ok(s)
}

/// Every frame's measures and the network's probability (calibration)
pub fn dump(path: &Path, from: f64, secs: Option<f64>) -> Result<String> {
    let pr = ff::probe(path)?;
    let fps = if pr.fps > 0.0 { pr.fps.min(60.0) } else { 25.0 };
    let cancel = AtomicBool::new(false);
    let tr = decode(path, from, secs, fps, None, &|_| {}, &cancel)?;
    let p = net_probs(&tr.nn, tr.len(), &[(0, tr.len())], &cancel).unwrap_or_default();
    let mut s = String::new();
    for i in 0..tr.len() {
        let q = tr.pair(i);
        s.push_str(&format!(
            "{i} {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} {} {:.3} {:.3}\n",
            p.get(i).copied().unwrap_or(f32::NAN),
            tr.strength(i),
            q.mc,
            q.mcf,
            q.hd,
            q.ecr,
            tr.transient(i) as u8,
            if i > 0 { tr.scene_dist(i - 1, i) } else { 0.0 },
            if i > 2 && i + 2 < tr.len() {
                tr.scene_dist(i - 2, i + 2)
            } else {
                0.0
            },
        ));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(seed: usize, shift: usize) -> (Vec<u8>, Vec<u8>) {
        let mk = |w: usize, h: usize, sc: usize| {
            let mut v = vec![0u8; w * h * 3];
            for y in 0..h {
                for x in 0..w {
                    let xs = x * sc + shift;
                    let ys = y * sc;
                    // smooth blobs: natural-ish, so motion can be found again
                    let (fx, fy) = (0.11 + seed as f32 * 0.03, 0.29 - seed as f32 * 0.02);
                    let f = ((xs as f32 * fx + seed as f32).sin()
                        * (ys as f32 * fy + seed as f32 * 2.0).cos()
                        + (xs as f32 * fy * 0.7 - ys as f32 * fx * 1.3).sin() * 0.5)
                        * 60.0;
                    let i = (y * w + x) * 3;
                    v[i] = (128.0 + f) as u8;
                    v[i + 1] = (100.0 + f * 0.5 + seed as f32 * 7.0) as u8;
                    v[i + 2] = (90.0 - f * 0.6 + seed as f32 * 11.0) as u8;
                }
            }
            v
        };
        (mk(NW, NH, 4), mk(CW, CH, 3))
    }

    #[test]
    fn motion_is_not_a_cut_a_new_picture_is() {
        let mut tr = Track::new(25.0, 0.0);
        // a slow pan over picture A, a cut to picture B, panning on
        for f in 0..60 {
            let (n, c) = if f < 30 { tex(1, f) } else { tex(9, f) };
            tr.push(&n, &c);
        }
        let pan = tr.pair(10);
        let cut = tr.pair(30);
        assert!(pan.mcf < 0.3, "pan {pan:?}");
        assert!(cut.mcf > 0.6 && cut.hd > 0.2, "cut {cut:?}");
        let b = detect(&tr, NetUse::Doubtful, &AtomicBool::new(false));
        assert!(b.iter().any(|b| b.i == 30), "{b:?}");
        assert!(b.iter().all(|b| b.i.abs_diff(30) <= 1), "{b:?}");
    }

    #[test]
    fn a_flash_is_not_a_cut() {
        let mut tr = Track::new(25.0, 0.0);
        let white = (vec![255u8; NSZ], vec![255u8; CW * CH * 3]);
        for f in 0..50 {
            let (n, c) = if (20..22).contains(&f) {
                white.clone()
            } else {
                tex(3, f / 4)
            };
            tr.push(&n, &c);
        }
        let b = detect(&tr, NetUse::Doubtful, &AtomicBool::new(false));
        assert!(b.is_empty(), "{b:?}");
    }

    #[test]
    fn shots_between_boundaries() {
        let tr = Track {
            fps: 10.0,
            t0: 5.0,
            nn: vec![],
            small: vec![
                Small {
                    g: vec![],
                    st: vec![],
                    hist: [0.0; 64],
                    luma: 0.0,
                    contrast: 0.0
                };
                50
            ],
            pairs: vec![],
            prev: None,
        };
        let cuts = [
            Boundary {
                i: 10,
                from: 10,
                to: 10,
                p: 1.0,
            },
            Boundary {
                i: 30,
                from: 28,
                to: 32,
                p: 0.3,
            },
        ];
        let s = shots_of(&tr, &cuts);
        assert_eq!(s, vec![(5.0, 6.0), (6.0, 7.8), (8.3, 10.0)]);
    }
}
