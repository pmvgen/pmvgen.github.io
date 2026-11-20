// PMV as template: an existing PMV is analysed – music, cuts, layouts (split screens) and flashes – and its
// timing drives a new show with your own clips. Two ways:
// - Classic (a port of pmvscan.js): the video's frames are scaled down to 96×54 and measured 20 times a
//   second (brightness, colour, difference to the frame before per region, dividers, mirroring); cuts and
//   flashes move onto the music's beats. The PMV's music is the song.
// - Exact cuts: every frame of the PMV is looked at (at its own frame rate, up to 60 per second); where it cuts
//   to another scene comes from the shot detector (shots.rs – a neural network and classical measures), exact
//   to the frame and independent of the beats; layouts, cuts in single fields of a split screen and flashes
//   from the same 96×54 measures. The timeline is kept in the PMV's own time; the music (the PMV's or the same
//   song as a file) and where it starts are applied with shifted() / with_music().
// Effects like glitch or text are burned into the picture and can't be taken over – timing and structure are.

use crate::ff;
use crate::song::{self, Song};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

const GW: usize = crate::shots::TW; // measuring picture for layouts and cuts
const GH: usize = crate::shots::TH;
/// Classic: frames looked at per second of the PMV. The web version plays the video at 3× and measures every
/// frame the screen shows – about 20 per second of video on a 60 Hz screen; its thresholds are calibrated on that.
const SAMPLE_FPS: f64 = 20.0;
/// Exact cuts: the most frames per second looked at (a 120 fps PMV is looked at in 60)
const MAX_FPS: f64 = 60.0;

/// How a PMV is used as template
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    /// cuts and flashes follow the music's beats; the PMV's music
    #[default]
    Classic,
    /// every cut exactly where the PMV has it, whatever the beat; music and its start can be chosen
    Exact,
}

/// Colours of the layouts in the template timeline (the web version's)
pub const LAYOUT_COLORS: [(&str, [u8; 3]); 6] = [
    ("full", [0x6d, 0x3a, 0x63]),
    ("kaleido", [0xb4, 0x8c, 0xff]),
    ("duo", [0x8f, 0xe3, 0xff]),
    ("trim", [0xff, 0x3e, 0x8a]),
    ("tri", [0xff, 0x9e, 0xc4]),
    ("quad", [0xff, 0xd1, 0x66]),
];
pub fn layout_color(id: &str) -> [u8; 3] {
    LAYOUT_COLORS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, c)| *c)
        .unwrap_or([0x55, 0x55, 0x55])
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TplEvent {
    /// A new layout (all fields get new clips)
    Layout { t: f64, layout: String, dir: String },
    /// New clips in some fields; None = all of them
    Cut { t: f64, groups: Option<Vec<usize>> },
    Flash {
        t: f64,
        strength: f64,
        color: [u8; 3],
    },
}
impl TplEvent {
    pub fn t(&self) -> f64 {
        match self {
            TplEvent::Layout { t, .. } | TplEvent::Cut { t, .. } | TplEvent::Flash { t, .. } => *t,
        }
    }
    fn set_t(&mut self, v: f64) {
        match self {
            TplEvent::Layout { t, .. } | TplEvent::Cut { t, .. } | TplEvent::Flash { t, .. } => {
                *t = v
            }
        }
    }
    fn is_layout(&self) -> bool {
        matches!(self, TplEvent::Layout { .. })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TplSeg {
    pub t: f64,
    pub end: f64,
    pub layout: String,
    pub dir: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TplStats {
    pub cuts: usize,
    pub layout_changes: usize,
    pub flashes: usize,
    pub layout_time: BTreeMap<String, f64>,
    pub frames: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub events: Vec<TplEvent>,
    pub segs: Vec<TplSeg>,
    pub stats: TplStats,
    /// the PMV's frame rate as it was looked at (for times as frame numbers)
    #[serde(default)]
    pub fps: f64,
    /// the PMV's length
    #[serde(default)]
    pub duration: f64,
}

impl Template {
    /// The template for music that starts at PMV time `shift` and lasts `dur` seconds: every time moved by
    /// −shift, what lies outside left out; it opens with the layout showing at `shift`
    pub fn shifted(&self, shift: f64, dur: f64) -> Template {
        let inside = |t: f64| t >= -1e-9 && t < dur;
        let mut events: Vec<TplEvent> = Vec::new();
        // the layout at the new start
        if let Some(sg) = self.segs.iter().rev().find(|sg| sg.t <= shift + 1e-9) {
            events.push(TplEvent::Layout {
                t: 0.0,
                layout: sg.layout.clone(),
                dir: sg.dir.clone(),
            });
        }
        for ev in &self.events {
            let t = ev.t() - shift;
            if t > 1e-9 && inside(t) {
                let mut e = ev.clone();
                e.set_t(t);
                events.push(e);
            }
        }
        let segs: Vec<TplSeg> = self
            .segs
            .iter()
            .filter(|sg| sg.end - shift > 0.0 && sg.t - shift < dur)
            .map(|sg| TplSeg {
                t: (sg.t - shift).max(0.0),
                end: (sg.end - shift).min(dur),
                layout: sg.layout.clone(),
                dir: sg.dir.clone(),
            })
            .collect();
        let mut layout_time: BTreeMap<String, f64> = BTreeMap::new();
        for sg in &segs {
            *layout_time.entry(sg.layout.clone()).or_default() += sg.end - sg.t;
        }
        let stats = TplStats {
            cuts: events
                .iter()
                .filter(|e| !matches!(e, TplEvent::Flash { .. }))
                .count(),
            layout_changes: events.iter().filter(|e| e.is_layout()).count(),
            flashes: events
                .iter()
                .filter(|e| matches!(e, TplEvent::Flash { .. }))
                .count(),
            layout_time,
            frames: self.stats.frames,
        };
        Template {
            name: self.name.clone(),
            events,
            segs,
            stats,
            fps: self.fps,
            duration: dur,
        }
    }
}

// ---------- Measuring one frame ----------

#[derive(Default, Clone, Debug)]
struct Diff {
    all: f64,
    c3: [f64; 3],
    c2: [f64; 2],
    q4: [f64; 4],
    r3: [f64; 3],
    r2: [f64; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Lay {
    id: &'static str,
    dir: &'static str,
}

struct Frame {
    t: f64,
    luma: f64,
    color: [f64; 3],
    diff: Diff,
    layout: Option<Lay>,
    flash: bool,
}

struct Edges {
    c13: f64,
    c23: f64,
    c12: f64,
    r12: f64,
    r13: f64,
    r23: f64,
}
struct Mirror {
    lr3: f64,
    lr2: f64,
    tb2: f64,
    tb3: f64,
}

/// Key figures of a measuring picture (RGBA, GW×GH) against the previous frame's gray values
fn measure(px: &[u8], prev: Option<&[f32]>) -> (Frame, Vec<f32>) {
    let n = GW * GH;
    let mut gray = vec![0f32; n];
    let (mut sum, mut r, mut g, mut b) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (i, gv) in gray.iter_mut().enumerate() {
        let j = i * 4;
        let (pr, pg, pb) = (px[j] as f64, px[j + 1] as f64, px[j + 2] as f64);
        let y = 0.299 * pr + 0.587 * pg + 0.114 * pb;
        *gv = y as f32;
        sum += y;
        r += pr;
        g += pg;
        b += pb;
    }
    let luma = sum / n as f64 / 255.0;
    let var: f64 = gray
        .iter()
        .map(|&v| (v as f64 - luma * 255.0).powi(2))
        .sum();
    let std = (var / n as f64).sqrt();

    let mut diff = Diff::default();
    if let Some(prev) = prev {
        let mut cnt = Diff::default();
        for y in 0..GH {
            for x in 0..GW {
                let i = y * GW + x;
                let d = (gray[i] - prev[i]).abs() as f64 / 255.0;
                diff.all += d;
                let c3 = ((x * 3) / GW).min(2);
                let c2 = usize::from(x >= GW / 2);
                let r3 = ((y * 3) / GH).min(2);
                let r2 = usize::from(y >= GH / 2);
                diff.c3[c3] += d;
                cnt.c3[c3] += 1.0;
                diff.c2[c2] += d;
                cnt.c2[c2] += 1.0;
                diff.q4[r2 * 2 + c2] += d;
                cnt.q4[r2 * 2 + c2] += 1.0;
                diff.r3[r3] += d;
                cnt.r3[r3] += 1.0;
                diff.r2[r2] += d;
                cnt.r2[r2] += 1.0;
            }
        }
        diff.all /= n as f64;
        let div = |a: &mut [f64], c: &[f64]| {
            for (v, k) in a.iter_mut().zip(c) {
                *v /= k.max(1.0);
            }
        };
        div(&mut diff.c3, &cnt.c3);
        div(&mut diff.c2, &cnt.c2);
        div(&mut diff.q4, &cnt.q4);
        div(&mut diff.r3, &cnt.r3);
        div(&mut diff.r2, &cnt.r2);
    }

    // Dividers: a real border between two clips runs through almost every row (or column) – the jump across
    // it is clearly bigger than the differences right next to it
    let gv = |x: isize, y: isize| -> f64 {
        let x = x.clamp(0, GW as isize - 1) as usize;
        let y = y.clamp(0, GH as isize - 1) as usize;
        gray[y * GW + x] as f64
    };
    let col_cover = |at: f64| -> f64 {
        let x0 = (at * GW as f64).round() as isize;
        let mut best = 0.0f64;
        for c in x0 - 1..=x0 + 1 {
            let mut hit = 0;
            for y in 0..GH as isize {
                let across = (gv(c + 1, y) - gv(c - 2, y)).abs();
                let left = (gv(c - 2, y) - gv(c - 5, y)).abs();
                let right = (gv(c + 4, y) - gv(c + 1, y)).abs();
                if across > 2.0 * left.max(right) + 10.0 {
                    hit += 1;
                }
            }
            best = best.max(hit as f64 / GH as f64);
        }
        best
    };
    let row_cover = |at: f64| -> f64 {
        let y0 = (at * GH as f64).round() as isize;
        let mut best = 0.0f64;
        for r in y0 - 1..=y0 + 1 {
            let mut hit = 0;
            for x in 0..GW as isize {
                let across = (gv(x, r + 1) - gv(x, r - 2)).abs();
                let up = (gv(x, r - 2) - gv(x, r - 5)).abs();
                let down = (gv(x, r + 4) - gv(x, r + 1)).abs();
                if across > 2.0 * up.max(down) + 10.0 {
                    hit += 1;
                }
            }
            best = best.max(hit as f64 / GW as f64);
        }
        best
    };
    let edges = Edges {
        c13: col_cover(1.0 / 3.0),
        c23: col_cover(2.0 / 3.0),
        c12: col_cover(0.5),
        r12: row_cover(0.5),
        r13: row_cover(1.0 / 3.0),
        r23: row_cover(2.0 / 3.0),
    };
    // Mirror similarity (0 = identically mirrored), relative to the contrast
    let mirror = |x0: usize, x1: usize, y0: usize, y1: usize, fx: bool, fy: bool| -> f64 {
        let (mut s, mut c) = (0.0f64, 0usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let mx = if fx { GW - 1 - x } else { x };
                let my = if fy { GH - 1 - y } else { y };
                s += (gray[y * GW + x] - gray[my * GW + mx]).abs() as f64;
                c += 1;
            }
        }
        s / c.max(1) as f64 / (std + 8.0)
    };
    let third = GW / 3;
    let mir = Mirror {
        lr3: mirror(0, third - 2, 0, GH, true, false),
        lr2: mirror(0, GW / 2 - 2, 0, GH, true, false),
        tb2: mirror(0, GW, 0, GH / 2 - 2, false, true),
        tb3: mirror(0, GW, 0, GH / 3 - 2, false, true),
    };
    let f = Frame {
        t: 0.0,
        luma,
        color: [r / n as f64, g / n as f64, b / n as f64],
        diff,
        layout: classify(&edges, &mir, std),
        flash: false,
    };
    (f, gray)
}

/// Layout of a frame from dividers and mirroring (thresholds calibrated by the web version on a recorded PMV)
fn classify(e: &Edges, m: &Mirror, std: f64) -> Option<Lay> {
    let l = |id, dir| Some(Lay { id, dir });
    if std < 6.0 {
        return None; // (almost) one colour, e.g. a flash
    }
    if m.lr2 < 0.08 && m.tb2 < 0.08 {
        return l("kaleido", "cols");
    }
    if m.lr3 < 0.08 {
        return l("trim", "cols");
    }
    if m.tb3 < 0.08 && e.r13.max(e.r23) > 0.3 {
        return l("trim", "rows");
    }
    let cols3 = e.c13.max(e.c23) > 0.45 || e.c13 + e.c23 > 0.8;
    let rows3 = e.r13.max(e.r23) > 0.45 || e.r13 + e.r23 > 0.8;
    if e.c12 > 0.45 && e.r12 > 0.45 {
        return l("quad", "cols");
    }
    if cols3 && e.c12 < 0.3 {
        return l("tri", "cols");
    }
    if rows3 && e.r12 < 0.3 {
        return l("tri", "rows");
    }
    if e.c12 > 0.5 {
        return l("duo", "cols");
    }
    if e.r12 > 0.5 {
        return l("duo", "rows");
    }
    l("full", "cols")
}

// ---------- Building the template ----------

/// The regions of each field per layout (what a cut in that field changes)
fn regions(layout: &str, dir: &str) -> Vec<(&'static str, Vec<usize>)> {
    let cols = dir != "rows";
    match layout {
        "duo" if cols => vec![("c2", vec![0]), ("c2", vec![1])],
        "duo" => vec![("r2", vec![0]), ("r2", vec![1])],
        "trim" if cols => vec![("c3", vec![0, 2]), ("c3", vec![1])],
        "trim" => vec![("r3", vec![0, 2]), ("r3", vec![1])],
        "tri" if cols => vec![("c3", vec![0]), ("c3", vec![1]), ("c3", vec![2])],
        "tri" => vec![("r3", vec![0]), ("r3", vec![1]), ("r3", vec![2])],
        "quad" => (0..4).map(|i| ("q4", vec![i])).collect(),
        _ => vec![("all", vec![])],
    }
}
fn region_diff(f: &Frame, spec: &(&str, Vec<usize>)) -> f64 {
    let arr: &[f64] = match spec.0 {
        "c3" => &f.diff.c3,
        "c2" => &f.diff.c2,
        "q4" => &f.diff.q4,
        "r3" => &f.diff.r3,
        "r2" => &f.diff.r2,
        _ => return f.diff.all,
    };
    spec.1
        .iter()
        .map(|&i| arr.get(i).copied().unwrap_or(0.0))
        .fold(0.0, f64::max)
}

/// Classic: cuts from the differences between frames, events moved onto beats within 80 ms (the web version)
fn build_classic(frames: &mut [Frame], beats: &[f64], duration: f64, name: &str) -> Template {
    let pink = [0xff, 0x3e, 0x8a];
    // 1. Flashes: the brightness jumps up sharply
    let mut events: Vec<TplEvent> = Vec::new();
    let mut flashes = 0;
    for i in 1..frames.len() {
        let (pl, fl) = (frames[i - 1].luma, frames[i].luma);
        if fl - pl > 0.22 && fl > 0.55 {
            let [r, g, b] = frames[i].color;
            let is_pink = r > 200.0 && b > 90.0 && g < r - 50.0;
            events.push(TplEvent::Flash {
                t: frames[i].t,
                strength: ((fl - pl) * 2.0).min(1.0),
                color: if is_pink { pink } else { [255, 255, 255] },
            });
            frames[i].flash = true;
            flashes += 1;
        }
    }
    // 2. Cuts: a big difference in at least one region, against the motion just before
    let mut cuts_idx: Vec<usize> = Vec::new();
    let mut recent: Vec<f64> = Vec::new();
    for i in 1..frames.len() {
        if frames[i].flash || frames[i - 1].flash {
            continue;
        }
        let f = &frames[i];
        let local = if recent.is_empty() {
            0.02
        } else {
            let mut s = recent.clone();
            s.sort_by(|a, b| a.total_cmp(b));
            s[s.len() / 2]
        };
        let regional = f
            .diff
            .c3
            .iter()
            .chain(&f.diff.c2)
            .chain(&f.diff.q4)
            .chain(&f.diff.r3)
            .fold(f.diff.all, |a, &b| a.max(b));
        let thr = (local * 3.2).max(0.09);
        if regional > thr && cuts_idx.last().is_none_or(|&l| f.t - frames[l].t > 0.15) {
            cuts_idx.push(i);
        }
        recent.push(f.diff.all);
        if recent.len() > 12 {
            recent.remove(0);
        }
    }
    // 3. Sections between cuts: the layout most of the frames in its middle show
    let mut bounds = vec![0usize];
    bounds.extend(&cuts_idx);
    bounds.push(frames.len());
    struct Seg {
        t: f64,
        i: usize,
        layout: &'static str,
        dir: &'static str,
    }
    let mut segs: Vec<Seg> = Vec::new();
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= frames.len() {
            break;
        }
        let lo = a + (b - a) / 5;
        let hi = (b - (b - a) / 5).max(lo + 1).min(frames.len());
        let mut votes: Vec<(Lay, usize)> = Vec::new();
        for f in &frames[lo.min(frames.len())..hi] {
            if let Some(l) = f.layout {
                match votes.iter_mut().find(|(v, _)| *v == l) {
                    Some(v) => v.1 += 1,
                    None => votes.push((l, 1)),
                }
            }
        }
        // (the first with the most votes – the web version's sort keeps the first of equals too)
        let best = votes
            .iter()
            .fold(None::<&(Lay, usize)>, |acc, v| match acc {
                Some(a) if a.1 >= v.1 => Some(a),
                _ => Some(v),
            })
            .map(|v| v.0);
        let prev = segs.last();
        let (mut id, mut dir) = match (best, prev) {
            (Some(l), _) => (l.id, l.dir),
            (None, Some(p)) => (p.layout, p.dir),
            (None, None) => ("full", "cols"),
        };
        // very short sections (1–3 frames) are too uncertain for a layout change
        if let Some(p) = prev {
            if b - a < 4 {
                id = p.layout;
                dir = p.dir;
            }
        }
        segs.push(Seg {
            t: frames[a].t,
            i: a,
            layout: id,
            dir,
        });
    }
    // 4. Events: a layout change, or a cut of some fields
    for (k, sg) in segs.iter().enumerate() {
        let prev = if k > 0 { segs.get(k - 1) } else { None };
        if prev.is_none_or(|p| p.layout != sg.layout || p.dir != sg.dir) {
            events.push(TplEvent::Layout {
                t: sg.t,
                layout: sg.layout.into(),
                dir: sg.dir.into(),
            });
            continue;
        }
        let f = &frames[sg.i];
        let specs = regions(sg.layout, sg.dir);
        let ds: Vec<f64> = specs.iter().map(|sp| region_diff(f, sp)).collect();
        let top = ds.iter().copied().fold(0.0, f64::max);
        let groups: Vec<usize> = ds
            .iter()
            .enumerate()
            .filter(|(_, &d)| d > (top * 0.45).max(0.07))
            .map(|(g, _)| g)
            .collect();
        events.push(TplEvent::Cut {
            t: sg.t,
            groups: if groups.len() == specs.len() {
                None
            } else {
                Some(groups)
            },
        });
    }
    // 5. Snap to the beat (the sampling is coarser than the music): an event within 80 ms of a beat moves onto
    //    the nearer of the beats around it. (The web version means to do this, but its reduce() starts from the
    //    event's own time – distance 0 – so it never moves anything; here it does what its comment says.)
    for ev in events.iter_mut() {
        let t = ev.t();
        let i = beats.partition_point(|&b| b <= t);
        let near = [i.checked_sub(1), Some(i)]
            .into_iter()
            .flatten()
            .filter_map(|k| beats.get(k).copied())
            .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()));
        if let Some(b) = near {
            if (b - t).abs() < 0.08 {
                ev.set_t(b);
            }
        }
    }
    events.sort_by(|a, b| {
        a.t()
            .total_cmp(&b.t())
            .then(b.is_layout().cmp(&a.is_layout()))
    });

    let mut layout_time: BTreeMap<String, f64> = BTreeMap::new();
    let mut out_segs = Vec::new();
    for (k, sg) in segs.iter().enumerate() {
        let end = segs.get(k + 1).map(|s| s.t).unwrap_or(duration);
        *layout_time.entry(sg.layout.to_string()).or_default() += end - sg.t;
        out_segs.push(TplSeg {
            t: sg.t,
            end,
            layout: sg.layout.into(),
            dir: sg.dir.into(),
        });
    }
    let stats = TplStats {
        cuts: events
            .iter()
            .filter(|e| !matches!(e, TplEvent::Flash { .. }))
            .count(),
        layout_changes: events.iter().filter(|e| e.is_layout()).count(),
        flashes,
        layout_time,
        frames: frames.len(),
    };
    Template {
        name: name.to_string(),
        events,
        segs: out_segs,
        stats,
        fps: SAMPLE_FPS,
        duration,
    }
}

/// Exact cuts. `full`: the frames where the shot detector saw the picture change to another scene
fn build_exact(
    frames: &mut [Frame],
    full: &[usize],
    fps: f64,
    duration: f64,
    name: &str,
) -> Template {
    let pink = [0xff, 0x3e, 0x8a];
    // 1. Flashes: the brightness jumps up sharply (within 50 ms – the web version's frame step)
    let k = ((fps / 20.0).round() as usize).max(1);
    let mut events: Vec<TplEvent> = Vec::new();
    let mut flashes = 0;
    let mut last_flash = f64::NEG_INFINITY;
    for i in k..frames.len() {
        let (pl, fl) = (frames[i - k].luma, frames[i].luma);
        let rising = frames[i].luma > frames[i - 1].luma;
        if fl - pl > 0.22 && fl > 0.55 && rising && frames[i].t - last_flash > 0.1 {
            let [r, g, b] = frames[i].color;
            let is_pink = r > 200.0 && b > 90.0 && g < r - 50.0;
            events.push(TplEvent::Flash {
                t: frames[i].t,
                strength: ((fl - pl) * 2.0).min(1.0),
                color: if is_pink { pink } else { [255, 255, 255] },
            });
            last_flash = frames[i].t;
            flashes += 1;
        }
        if fl - pl > 0.22 && fl > 0.55 {
            frames[i].flash = true;
        }
    }
    // 2. Cuts: where the whole picture goes to another scene (the shot detector), and – in a split screen –
    //    where one field gets a new clip: a big difference in a region against the motion just before
    let mut cuts_idx: Vec<usize> = full
        .iter()
        .copied()
        .filter(|&i| i > 0 && i < frames.len())
        .collect();
    let near_full = |i: usize| full.iter().any(|&f| f.abs_diff(i) <= 2);
    let mut recent: Vec<f64> = Vec::new();
    let gap = (0.15 * fps).round() as usize;
    let mut last_reg: Option<usize> = None;
    for i in 1..frames.len() {
        let f = &frames[i];
        let regional = f
            .diff
            .c3
            .iter()
            .chain(&f.diff.c2)
            .chain(&f.diff.q4)
            .chain(&f.diff.r3)
            .fold(0.0f64, |a, &b| a.max(b));
        let split = f.layout.is_some_and(|l| l.id != "full")
            && frames[i - 1].layout.is_some_and(|l| l.id != "full");
        if split && !f.flash && !frames[i - 1].flash && !near_full(i) {
            let local = if recent.is_empty() {
                0.02
            } else {
                let mut s = recent.clone();
                s.sort_by(|a, b| a.total_cmp(b));
                s[s.len() / 2]
            };
            if regional > (local * 3.2).max(0.1) && last_reg.is_none_or(|l| i - l > gap) {
                cuts_idx.push(i);
                last_reg = Some(i);
            }
        }
        recent.push(regional);
        if recent.len() > ((0.6 * fps) as usize).max(12) {
            recent.remove(0);
        }
    }
    cuts_idx.sort_unstable();
    cuts_idx.dedup();
    // 3. Sections between cuts: the layout most of the frames in its middle show
    let mut bounds = vec![0usize];
    bounds.extend(&cuts_idx);
    bounds.push(frames.len());
    struct Seg {
        t: f64,
        i: usize,
        layout: &'static str,
        dir: &'static str,
    }
    let mut segs: Vec<Seg> = Vec::new();
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= frames.len() {
            break;
        }
        let lo = a + (b - a) / 5;
        let hi = (b - (b - a) / 5).max(lo + 1).min(frames.len());
        let mut votes: Vec<(Lay, usize)> = Vec::new();
        for f in &frames[lo.min(frames.len())..hi] {
            if let Some(l) = f.layout {
                match votes.iter_mut().find(|(v, _)| *v == l) {
                    Some(v) => v.1 += 1,
                    None => votes.push((l, 1)),
                }
            }
        }
        // (the first with the most votes – the web version's sort keeps the first of equals too)
        let best = votes
            .iter()
            .fold(None::<&(Lay, usize)>, |acc, v| match acc {
                Some(a) if a.1 >= v.1 => Some(a),
                _ => Some(v),
            })
            .map(|v| v.0);
        let prev = segs.last();
        let (mut id, mut dir) = match (best, prev) {
            (Some(l), _) => (l.id, l.dir),
            (None, Some(p)) => (p.layout, p.dir),
            (None, None) => ("full", "cols"),
        };
        // very short sections (1–3 frames) are too uncertain for a layout change
        if let Some(p) = prev {
            if b - a < 4 {
                id = p.layout;
                dir = p.dir;
            }
        }
        segs.push(Seg {
            t: frames[a].t,
            i: a,
            layout: id,
            dir,
        });
    }
    // 4. Events: a layout change, or a cut of some fields
    for (k, sg) in segs.iter().enumerate() {
        let prev = if k > 0 { segs.get(k - 1) } else { None };
        if prev.is_none_or(|p| p.layout != sg.layout || p.dir != sg.dir) {
            events.push(TplEvent::Layout {
                t: sg.t,
                layout: sg.layout.into(),
                dir: sg.dir.into(),
            });
            continue;
        }
        let f = &frames[sg.i];
        let specs = regions(sg.layout, sg.dir);
        let ds: Vec<f64> = specs.iter().map(|sp| region_diff(f, sp)).collect();
        let top = ds.iter().copied().fold(0.0, f64::max);
        let groups: Vec<usize> = ds
            .iter()
            .enumerate()
            .filter(|(_, &d)| d > (top * 0.45).max(0.07))
            .map(|(g, _)| g)
            .collect();
        events.push(TplEvent::Cut {
            t: sg.t,
            groups: if groups.len() == specs.len() {
                None
            } else {
                Some(groups)
            },
        });
    }
    events.sort_by(|a, b| {
        a.t()
            .total_cmp(&b.t())
            .then(b.is_layout().cmp(&a.is_layout()))
    });

    let mut layout_time: BTreeMap<String, f64> = BTreeMap::new();
    let mut out_segs = Vec::new();
    for (k, sg) in segs.iter().enumerate() {
        let end = segs.get(k + 1).map(|s| s.t).unwrap_or(duration);
        *layout_time.entry(sg.layout.to_string()).or_default() += end - sg.t;
        out_segs.push(TplSeg {
            t: sg.t,
            end,
            layout: sg.layout.into(),
            dir: sg.dir.into(),
        });
    }
    let stats = TplStats {
        cuts: events
            .iter()
            .filter(|e| !matches!(e, TplEvent::Flash { .. }))
            .count(),
        layout_changes: events.iter().filter(|e| e.is_layout()).count(),
        flashes,
        layout_time,
        frames: frames.len(),
    };
    Template {
        name: name.to_string(),
        events,
        segs: out_segs,
        stats,
        fps,
        duration,
    }
}

/// Analyse a PMV: its music (as a song) and its structure. progress: 0–1 with a text
pub fn scan(
    path: &Path,
    rate: u32,
    kind: Kind,
    progress: &(dyn Fn(f32, &str) + Sync),
    cancel: &AtomicBool,
) -> Result<(Song, Template)> {
    match kind {
        Kind::Classic => scan_classic(path, rate, progress, cancel),
        Kind::Exact => scan_exact(path, rate, progress, cancel),
    }
}

/// Classic: the PMV looked at 20 times a second, its music as the song. progress: 0–1 with a text
fn scan_classic(
    path: &Path,
    rate: u32,
    progress: &(dyn Fn(f32, &str) + Sync),
    cancel: &AtomicBool,
) -> Result<(Song, Template)> {
    ff::available()?;
    progress(0.0, "Reading music …");
    let song = song::load(
        path,
        0.0,
        None,
        rate,
        &|p| progress(0.1 * p, "Reading music …"),
        cancel,
    )
    .map_err(|e| anyhow::anyhow!("The music of this video can't be read: {e}"))?;
    if song.a.beats.len() < 8 {
        bail!("Too few beats detected – does the video have music?");
    }
    let duration = song.duration();
    progress(0.1, "Looking through the PMV …");
    let mut c = ff::ffmpeg()?;
    c.arg("-i").arg(path);
    c.args(["-an", "-sn", "-dn", "-vf"]).arg(format!(
        "fps={SAMPLE_FPS},scale={GW}:{GH}:flags=bilinear,format=rgba"
    ));
    c.args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg gave no output"))?;
    let errp = child.stderr.take();
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut e) = errp {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let mut frames: Vec<Frame> = Vec::new();
    let mut prev: Option<Vec<f32>> = None;
    let mut buf = vec![0u8; GW * GH * 4];
    let mut i = 0usize;
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Cancelled");
        }
        match ff::read_full(&mut out, &mut buf) {
            Ok(true) => {}
            _ => break,
        }
        let (mut f, gray) = measure(&buf, prev.as_deref());
        f.t = i as f64 / SAMPLE_FPS;
        prev = Some(gray);
        frames.push(f);
        i += 1;
        if i.is_multiple_of(20) && duration > 0.0 {
            progress(
                0.1 + 0.85 * ((i as f64 / SAMPLE_FPS) / duration).min(1.0) as f32,
                "Looking through the PMV …",
            );
        }
    }
    let _ = child.wait();
    let err = err_t.join().unwrap_or_default();
    if frames.len() < 10 {
        bail!(
            "Too few frames read – is this a video?{}",
            err.lines()
                .next()
                .map(|l| format!(" ({l})"))
                .unwrap_or_default()
        );
    }
    progress(0.97, "Evaluating …");
    let name = song.name.clone();
    let tpl = build_classic(&mut frames, &song.a.beats, duration, &name);
    progress(1.0, "Done");
    Ok((song, tpl))
}

/// Exact cuts: every frame, the shot detector. progress: 0–1 with a text
fn scan_exact(
    path: &Path,
    rate: u32,
    progress: &(dyn Fn(f32, &str) + Sync),
    cancel: &AtomicBool,
) -> Result<(Song, Template)> {
    ff::available()?;
    let probe = ff::probe(path)?;
    if !probe.has_video {
        bail!("This file has no video");
    }
    progress(0.0, "Reading music …");
    // (the PMV's own music – also when a song file is used instead, its beats drive the effects then)
    let song = song::load(
        path,
        0.0,
        None,
        rate,
        &|p| progress(0.08 * p, "Reading music …"),
        cancel,
    )
    .map_err(|e| anyhow::anyhow!("The music of this video can't be read: {e}"))?;
    let duration = if probe.dur > 0.0 {
        probe.dur
    } else {
        song.duration()
    };
    let fps = if probe.fps > 0.0 {
        probe.fps.min(MAX_FPS)
    } else {
        25.0
    };
    progress(0.08, "Looking at every frame …");
    let mut frames: Vec<Frame> = Vec::new();
    let mut prev: Option<Vec<f32>> = None;
    let mut on_frame = |i: usize, px: &[u8]| {
        let (mut f, gray) = measure(px, prev.as_deref());
        f.t = i as f64 / fps;
        prev = Some(gray);
        frames.push(f);
    };
    let track = crate::shots::decode(
        path,
        0.0,
        None,
        fps,
        Some(&mut on_frame),
        &|t| {
            if duration > 0.0 {
                progress(
                    0.08 + 0.6 * (t / duration).min(1.0) as f32,
                    "Looking at every frame …",
                );
            }
        },
        cancel,
    )?;
    if frames.len() < 10 {
        bail!("Too few frames read – is this a video?");
    }
    progress(0.7, "Finding every cut …");
    let cuts = crate::shots::detect(&track, crate::shots::NetUse::Always, cancel);
    if cancel.load(Ordering::SeqCst) {
        bail!("Cancelled");
    }
    progress(0.97, "Evaluating …");
    let full: Vec<usize> = cuts.iter().map(|b| b.i).collect();
    let tpl = build_exact(&mut frames, &full, fps, duration, &song.name);
    progress(1.0, "Done");
    Ok((song, tpl))
}

/// The music for a PMV used as template
#[derive(Clone, Debug, PartialEq)]
pub enum Music {
    /// the PMV's own soundtrack
    Pmv,
    /// a song file; `at` = the PMV time where the song file's start plays (from align::find, or set by hand)
    File { path: PathBuf, at: f64 },
}

/// The song and the template for the new PMV: it begins at PMV time `start` (the moment the music segment
/// starts). With the PMV's soundtrack the song is that from `start` on; with a song file it is the file from
/// the matching moment (start − at), as far as the PMV goes. → (song, template in the song's time)
pub fn with_music(
    pmv: &Path,
    tpl: &Template,
    music: &Music,
    start: f64,
    rate: u32,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> Result<(Song, Template)> {
    let start = start.clamp(0.0, (tpl.duration - 1.0).max(0.0));
    let song = match music {
        Music::Pmv => song::load(pmv, start, None, rate, progress, cancel)?,
        Music::File { path, at } => {
            let from = start - at;
            if from < -0.001 {
                bail!(
                    "The song only starts at {:.3} s of the PMV – the music can't start before that",
                    at
                );
            }
            let to = from + (tpl.duration - start).max(1.0);
            song::load(path, from.max(0.0), Some(to), rate, progress, cancel)?
        }
    };
    let mut t = tpl.shifted(start, song.duration());
    t.name = tpl.name.clone();
    Ok((song, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(f: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
        let mut v = vec![0u8; GW * GH * 4];
        for y in 0..GH {
            for x in 0..GW {
                let c = f(x, y);
                let i = (y * GW + x) * 4;
                v[i..i + 3].copy_from_slice(&c);
                v[i + 3] = 255;
            }
        }
        v
    }
    // a textured picture (noise-like, no mirror symmetry)
    fn tex(seed: usize) -> impl Fn(usize, usize) -> [u8; 3] {
        move |x, y| {
            let v = ((x * 37 + y * 91 + seed * 13) ^ (x * y + seed)) % 200;
            [v as u8 + 30, (v / 2) as u8 + 40, (255 - v) as u8]
        }
    }

    #[test]
    fn layouts_are_recognised() {
        let full = pic(tex(1));
        assert_eq!(measure(&full, None).0.layout.unwrap().id, "full");
        // three different stripes with hard borders
        let a = tex(1);
        let b = |x: usize, y: usize| {
            let v = (x * 3 + y * 5) % 40;
            [v as u8, v as u8, v as u8]
        };
        let c = tex(7);
        let tri = pic(|x, y| match (x * 3) / GW {
            0 => a(x, y),
            1 => {
                let v = b(x, y);
                [v[0] + 200, v[1] + 210, v[2] + 215]
            }
            _ => c(x, y),
        });
        assert_eq!(measure(&tri, None).0.layout.unwrap().id, "tri");
        // left third mirrored on the right
        let t = tex(3);
        let trim = pic(|x, y| {
            if x < GW / 3 {
                t(x, y)
            } else if x >= GW - GW / 3 {
                t(GW - 1 - x, y)
            } else {
                [250, 250, 250]
            }
        });
        assert_eq!(measure(&trim, None).0.layout.unwrap().id, "trim");
        // flat picture: no layout
        let flat = pic(|_, _| [200, 200, 200]);
        assert!(measure(&flat, None).0.layout.is_none());
    }

    #[test]
    fn classic_cuts_flashes_and_beats() {
        // 4 s at 20 fps: picture A, a cut to B at 1.0 s, a white flash at 2.0 s, back to B
        let a = pic(tex(1));
        let b = pic(tex(9));
        let white = pic(|_, _| [255, 255, 255]);
        let mut frames = Vec::new();
        let mut prev: Option<Vec<f32>> = None;
        for i in 0..80 {
            let t = i as f64 / 20.0;
            let px = if t < 1.0 {
                &a
            } else if (2.0..2.1).contains(&t) {
                &white
            } else {
                &b
            };
            let (mut f, g) = measure(px, prev.as_deref());
            f.t = t;
            prev = Some(g);
            frames.push(f);
        }
        let beats: Vec<f64> = (0..8).map(|i| i as f64 * 0.5 + 0.02).collect();
        let tpl = build_classic(&mut frames, &beats, 4.0, "x");
        assert_eq!(tpl.stats.flashes, 1);
        let cut = tpl
            .events
            .iter()
            .find(|e| matches!(e, TplEvent::Cut { .. }))
            .expect("a cut");
        // snapped onto the beat at 1.02
        assert!((cut.t() - 1.02).abs() < 1e-9, "{}", cut.t());
        assert!(tpl.events[0].is_layout());
        // sorted
        assert!(tpl.events.windows(2).all(|w| w[0].t() <= w[1].t()));
    }

    #[test]
    fn cuts_flashes_exact_to_the_frame() {
        // 4 s at 25 fps: picture A, a cut to B at frame 27 (1.08 s – between beats), a white flash at 2.0 s
        let a = pic(tex(1));
        let b = pic(tex(9));
        let white = pic(|_, _| [255, 255, 255]);
        let fps = 25.0;
        let mut frames = Vec::new();
        let mut prev: Option<Vec<f32>> = None;
        for i in 0..100 {
            let t = i as f64 / fps;
            let px = if i < 27 {
                &a
            } else if (50..53).contains(&i) {
                &white
            } else {
                &b
            };
            let (mut f, g) = measure(px, prev.as_deref());
            f.t = t;
            prev = Some(g);
            frames.push(f);
        }
        let tpl = build_exact(&mut frames, &[27], fps, 4.0, "x");
        assert_eq!(tpl.stats.flashes, 1);
        let cut = tpl
            .events
            .iter()
            .find(|e| matches!(e, TplEvent::Cut { .. }))
            .expect("a cut");
        // exactly at the frame, not moved to a beat
        assert!((cut.t() - 27.0 / 25.0).abs() < 1e-9, "{}", cut.t());
        assert!(tpl.events[0].is_layout());
        assert!(tpl.events.windows(2).all(|w| w[0].t() <= w[1].t()));
        // music starting 0.5 s into the PMV: everything 0.5 s earlier, a layout at 0
        let sh = tpl.shifted(0.5, 3.0);
        assert!(sh.events[0].is_layout() && sh.events[0].t() == 0.0);
        let c2 = sh
            .events
            .iter()
            .find(|e| matches!(e, TplEvent::Cut { .. }))
            .unwrap();
        assert!((c2.t() - (27.0 / 25.0 - 0.5)).abs() < 1e-9);
        assert!(sh.events.iter().all(|e| e.t() >= 0.0 && e.t() < 3.0));
    }
}
