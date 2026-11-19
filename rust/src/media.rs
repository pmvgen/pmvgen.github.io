// Clips for the shots: a pool of prepared clips (a good spot found in each video – best moments, clean
// cuts, variety, pinned moments, the speed of the music, motion on the beat), the pick for each shot shortly
// before it comes on (shape, match cut, calm/busy), and one ffmpeg decoder per shot that delivers its frames
// at the field's size. Ports prepareFrom(), nextSource(), bySpeed(), takeMedia() and friends.
//
// Every pick is recorded (`Pick`): a generated PMV is its plan plus one pick per shot, so it can be played,
// edited, saved and rendered again exactly. Picks that are already known (a project, the timeline) are used
// as they are; only missing ones are chosen.

use crate::ff;
use crate::library::{shape_ok, Item, Kind};
use crate::motion::{self, SpeedSeen};
use crate::plan::{Plan, Shot};
use crate::settings::Settings;
use crate::smart::{self, Sig};
use crate::util::{cmp_f64, wait_timeout, LockExt};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;

pub const SNAP: f64 = 1.16; // "Fit": a clip this close to the field's shape (ratio of the two aspect ratios) fills it

/// Show clock shared between the player / exporter and the decoders (f64 seconds as bits)
#[derive(Default)]
pub struct Clock(AtomicU64);
impl Clock {
    pub fn set(&self, t: f64) {
        self.0.store(t.to_bits(), Ordering::Relaxed)
    }
    pub fn get(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }
}

// 3 min → 15 s, 10 min → 24 s, longer → 30 s; short clips a tenth; plus "Skip the start" (never more than 40 %)
pub fn intro_end(dur: f64, skip_head: f64) -> f64 {
    if dur > 10.0 {
        (30f64.min((dur * 0.04).max(15f64.min(dur * 0.1)))).max(skip_head.min(dur * 0.4))
    } else {
        0.0
    }
}
pub fn tail_x(dur: f64, skip_tail: f64) -> f64 {
    if dur > 10.0 {
        skip_tail.min(dur * 0.4)
    } else {
        0.0
    }
}
/// The skips of one video: its own (per-video setting) or the one for all videos
pub fn skips_of(it: &Item, s: &Settings) -> (f64, f64) {
    (
        it.skip.head.unwrap_or(s.skip_head).max(0.0),
        it.skip.tail.unwrap_or(s.skip_tail).max(0.0),
    )
}
/// Where in a video clips may start: [lo, hi] (a random spot is taken in it)
pub fn start_range(it: &Item, s: &Settings) -> (f64, f64) {
    let dur = it.dur;
    if it.kind != Kind::Video || dur <= 10.0 {
        return (0.0, 0.0);
    }
    let (sh, st) = skips_of(it, s);
    let head = intro_end(dur, sh);
    let hi = (dur * 0.92 - 4.0).min(dur - 4.0 - tail_x(dur, st));
    (head, hi.max(head))
}

/// A clip in a shot: which file, from where, and how it was measured. Saved in project files.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pick {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub w: u32,
    pub h: u32,
    /// length of the video (0 for images)
    pub dur: f64,
    /// where in the video the clip starts (seconds)
    pub start: f64,
    /// where it turns back to its start (just before the video's end)
    pub end_at: Option<f64>,
    /// image drift direction
    pub kb: f64,
    /// motion on the beat: the speed factor that puts the clip's motion on the beat
    #[serde(default)]
    pub beat: Option<f64>,
    /// clip speed follows the music: how fast its action is (motion per second)
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub motion: Option<f64>,
    /// the first frame's measurement (match cuts, smart crop, even out brightness)
    #[serde(default)]
    pub sig: Option<Sig>,
    #[serde(default)]
    pub pinned: bool,
    /// chosen by hand (timeline / change the moment)
    #[serde(default)]
    pub manual: bool,
    /// clean cuts: seconds of the clip from its start before its scene ends (the video cuts to another one);
    /// None = not looked at
    #[serde(default)]
    pub clean: Option<f64>,
}

impl Pick {
    pub fn tempo(&self) -> f64 {
        self.beat.unwrap_or(1.0)
    }
    /// Seconds of the clip played at show time t (from its start spot; after the loop back near its end)
    pub fn clock(&self, plan: &Plan, shot: &Shot, t: f64) -> f64 {
        wrap(
            self.start,
            self.end_at,
            plan.shot_clock(shot, t) * self.tempo(),
        )
    }
    /// Where in the file the clip is at show time t
    pub fn file_time(&self, plan: &Plan, shot: &Shot, t: f64) -> f64 {
        self.start + self.clock(plan, shot, t)
    }
    fn from_item(it: &Item, start: f64, end_at: Option<f64>, kb: f64) -> Pick {
        Pick {
            path: it.path.clone(),
            name: it.name.clone(),
            kind: it.kind,
            w: it.w,
            h: it.h,
            dur: it.dur,
            start,
            end_at,
            kb,
            beat: None,
            speed: None,
            motion: None,
            sig: None,
            pinned: false,
            manual: false,
            clean: None,
        }
    }
    /// The same clip from another moment (change the moment); measured again only when asked
    pub fn with_start(&self, start: f64) -> Pick {
        let mut p = self.clone();
        if p.kind == Kind::Video {
            p.start = start.clamp(0.0, (p.dur - 0.6).max(0.0));
            p.end_at = (p.dur - 0.4 > p.start + 0.5).then_some(p.dur - 0.4);
            // (the rhythm and the scene measured at the old spot don't hold here)
            p.beat = None;
            p.clean = None;
        }
        p.manual = true;
        p
    }
    /// Measure a pick's first frame again (after its moment changed)
    pub fn remeasured(mut self) -> Pick {
        if self.kind == Kind::Video {
            if let Some(s) = tiny_frames(&self.path, self.w, self.h, self.start, &[0.0])
                .into_iter()
                .next()
            {
                self.sig = lean(Some(s));
            }
        }
        self
    }
    /// A clip of a video from a given moment (the end comes from the video's length)
    pub fn video_at(it: &Item, start: f64) -> Pick {
        let start = start.clamp(0.0, (it.dur - 0.6).max(0.0));
        let end_at = (it.dur - 0.4 > start + 0.5).then_some(it.dur - 0.4);
        Pick::from_item(it, start, end_at, 0.0)
    }
}

/// A measurement as a pick keeps it: without the tiny gray picture (only needed while preparing – it isn't
/// saved either, so a pick in memory and the same pick from a project file are equal)
fn lean(s: Option<Sig>) -> Option<Sig> {
    s.map(|mut s| {
        s.gray = Vec::new();
        s
    })
}

/// The loop back near the end: a clip that reaches end_at starts over at its start spot
fn wrap(start: f64, end_at: Option<f64>, c: f64) -> f64 {
    let mut c = c.max(0.0);
    if let Some(end) = end_at {
        let period = end - start;
        if period > 0.1 && start + c >= end {
            c %= period;
        }
    }
    c
}

/// Frames of one shot
pub struct Frames {
    pub fw: u32,
    pub fh: u32,
    pub fps: f64,
    pub still: Option<Arc<Pixmap>>,
    /// the first index decoded (a shot started in its middle – after a seek – is decoded from there)
    pub base: usize,
    slots: Mutex<Vec<Option<Arc<Pixmap>>>>,
    decoded: AtomicUsize,
    pub done: AtomicBool,
    pub failed: Mutex<Option<String>>,
    cancel: Arc<AtomicBool>,
    cond: Condvar,
    gate: Mutex<()>,
}
impl Frames {
    fn new(fw: u32, fh: u32, fps: f64, n: usize, base: usize) -> Frames {
        Frames {
            fw,
            fh,
            fps,
            still: None,
            base,
            slots: Mutex::new(vec![None; n]),
            decoded: AtomicUsize::new(base),
            done: AtomicBool::new(false),
            failed: Mutex::new(None),
            cancel: Arc::new(AtomicBool::new(false)),
            cond: Condvar::new(),
            gate: Mutex::new(()),
        }
    }
    /// Frames decoded so far (counting from index 0)
    pub fn decoded(&self) -> usize {
        self.decoded.load(Ordering::SeqCst)
    }
    /// Has any frame come yet?
    pub fn any(&self) -> bool {
        self.still.is_some() || self.decoded() > self.base
    }
    fn finish(&self) {
        self.done.store(true, Ordering::SeqCst);
        let _g = self.gate.lk();
        self.cond.notify_all();
    }
    /// Frame `idx` (or, while it isn't there, the newest one before it). wait: block until it is decoded.
    pub fn get(&self, idx: usize, wait: Option<Duration>) -> Option<Arc<Pixmap>> {
        if let Some(s) = &self.still {
            return Some(s.clone());
        }
        if let Some(limit) = wait {
            let t0 = Instant::now();
            let mut g = self.gate.lk();
            while self.decoded() <= idx && !self.done.load(Ordering::SeqCst) {
                let left = limit.saturating_sub(t0.elapsed());
                if left.is_zero() {
                    break;
                }
                g = wait_timeout(&self.cond, g, left.min(Duration::from_millis(50)));
            }
        }
        let slots = self.slots.lk();
        let have = self.decoded().min(slots.len());
        if have == 0 {
            return None;
        }
        let top = idx.min(have - 1);
        if let Some(f) = slots[..=top].iter().rev().flatten().next() {
            return Some(f.clone());
        }
        // (dropped below, or before where decoding started: the first one kept after it)
        slots[top..have].iter().flatten().next().cloned()
    }
    fn drop_below(&self, idx: usize) {
        let mut s = self.slots.lk();
        let n = idx.min(s.len());
        for x in s.iter_mut().take(n) {
            *x = None;
        }
    }
}
impl Drop for Frames {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

pub struct LiveSig {
    pub focus: (f64, f64),
    pub target: (f64, f64),
    pub lum: Option<f64>,
    pub measured_t: f64,
}

/// A clip on screen in a shot: its pick and its frames
pub struct Assigned {
    pub shot: usize,
    pub pick: Pick,
    pub frames: Arc<Frames>,
    pub live: Mutex<LiveSig>,
    pub smart: bool, // (false: a quick pick without analysis)
}

impl Assigned {
    /// Video frame index at show time t
    pub fn index_at(&self, plan: &Plan, shot: &Shot, t: f64) -> usize {
        (self.pick.clock(plan, shot, t) * self.frames.fps).floor() as usize
    }
}

pub fn fit_for(fit: &str, field_w: f64, field_h: f64, mw: f64, mh: f64) -> &'static str {
    let r = (field_w / field_h / (mw / mh)).max(mw / mh / (field_w / field_h));
    if fit == "contain" && r <= SNAP {
        "cover"
    } else if fit == "contain" {
        "contain"
    } else {
        "cover"
    }
}

/// The size a clip is decoded at: as it is drawn into its field (fill or fit)
fn decode_size(fit: &str, fw: f64, fh: f64, mw: f64, mh: f64) -> (u32, u32) {
    let f = fit_for(fit, fw, fh, mw, mh);
    let r = if f == "contain" {
        (fw / mw).min(fh / mh)
    } else {
        (fw / mw).max(fh / mh)
    };
    // (never larger than 8K on a side, whatever the numbers – memory)
    (
        (mw * r).round().clamp(2.0, 8192.0) as u32,
        (mh * r).round().clamp(2.0, 8192.0) as u32,
    )
}

pub fn to_pixmap(w: u32, h: u32, rgba: Vec<u8>) -> Option<Pixmap> {
    // (opaque pictures are the same premultiplied; see-through ones are premultiplied here)
    let mut data = rgba;
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a < 255 {
            px[0] = ((px[0] as u32 * a + 127) / 255) as u8;
            px[1] = ((px[1] as u32 * a + 127) / 255) as u8;
            px[2] = ((px[2] as u32 * a + 127) / 255) as u8;
        }
    }
    Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)
}

// ---------- Tiny frames for the analysis ----------

/// Tiny RGB frames of a video at `times` seconds after `t0` (one ffmpeg run)
fn tiny_frames(path: &Path, w: u32, h: u32, t0: f64, times: &[f64]) -> Vec<Sig> {
    let (tw, th) = smart::tiny(w, h);
    let last = times.iter().cloned().fold(0.0, f64::max);
    let Ok(mut c) = ff::ffmpeg() else {
        return Vec::new();
    };
    c.arg("-ss")
        .arg(format!("{:.3}", t0.max(0.0)))
        .arg("-i")
        .arg(path);
    c.arg("-t").arg(format!("{:.3}", last + 0.12));
    c.args(["-an", "-sn", "-dn", "-threads", "2", "-vf"])
        .arg(format!("fps=20,scale={tw}:{th}:flags=area"));
    c.args(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]);
    let Ok(out) = c.stdout(Stdio::piped()).stderr(Stdio::null()).output() else {
        return Vec::new();
    };
    let fsz = (tw * th * 3) as usize;
    let frames: Vec<&[u8]> = out.stdout.chunks_exact(fsz).collect();
    if frames.is_empty() {
        return Vec::new();
    }
    times
        .iter()
        .map(|t| {
            let i = ((t * 20.0).round() as usize).min(frames.len() - 1);
            smart::measure(frames[i], tw as usize, th as usize, 3)
        })
        .collect()
}

/// Every frame of `secs` seconds of a video from `t0`, measured (for its speed and rhythm): [(video time, sig)]
fn capture_frames(path: &Path, w: u32, h: u32, t0: f64, secs: f64) -> Vec<(f64, Sig)> {
    let (tw, th) = smart::tiny(w, h);
    let fps = 1.0 / motion::RHYTHM_DT;
    let Ok(mut c) = ff::ffmpeg() else {
        return Vec::new();
    };
    c.arg("-ss")
        .arg(format!("{:.3}", t0.max(0.0)))
        .arg("-i")
        .arg(path);
    c.arg("-t").arg(format!("{secs:.3}"));
    c.args(["-an", "-sn", "-dn", "-threads", "2", "-vf"])
        .arg(format!("fps={fps},scale={tw}:{th}:flags=area"));
    c.args(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]);
    let Ok(out) = c.stdout(Stdio::piped()).stderr(Stdio::null()).output() else {
        return Vec::new();
    };
    let fsz = (tw * th * 3) as usize;
    out.stdout
        .chunks_exact(fsz)
        .enumerate()
        .map(|(i, f)| {
            (
                t0 + i as f64 / fps,
                smart::measure(f, tw as usize, th as usize, 3),
            )
        })
        .collect()
}

fn image_sig(path: &Path, w: u32, h: u32) -> Option<Sig> {
    let (tw, th) = smart::tiny(w, h);
    let px = ff::decode_rgba(path, None, tw, th).ok()?;
    Some(smart::measure(&px, tw as usize, th as usize, 4))
}

/// A clip of a video from a given moment, measured there (the timeline's manual pick, change the moment)
pub fn measured_pick(it: &Item, start: f64) -> Pick {
    let mut p = if it.kind == Kind::Image {
        let mut p = Pick::from_item(it, 0.0, None, 1.0);
        p.sig = lean(image_sig(&it.path, it.w, it.h));
        p
    } else {
        let mut p = Pick::video_at(it, start);
        p.sig = lean(
            tiny_frames(&it.path, it.w, it.h, p.start, &[0.0])
                .into_iter()
                .next(),
        );
        p
    };
    p.manual = true;
    p
}

/// A random clip of a video (random moment in its allowed range) – the timeline's "Random"
pub fn random_pick(it: &Item, s: &Settings) -> Pick {
    let mut rng = rand::thread_rng();
    if it.kind == Kind::Image {
        let mut p = Pick::from_item(it, 0.0, None, if rng.gen::<bool>() { 1.0 } else { -1.0 });
        p.sig = lean(image_sig(&it.path, it.w, it.h));
        p.manual = true;
        return p;
    }
    let (lo, hi) = start_range(it, s);
    measured_pick(it, lo + rng.gen::<f64>() * (hi - lo))
}

// ---------- The pool ----------

#[derive(Clone)]
struct Ready {
    id: usize,
    pick: Pick,
}

struct Pool {
    sources: Vec<Item>,
    src_idx: usize,
    ready: Vec<Ready>,
    preparing: usize,
    prep_keys: Vec<usize>,
    recent: VecDeque<usize>,
    spots: HashMap<usize, Vec<f64>>,
    bad: HashMap<usize, String>,
    shown: HashSet<usize>,
    measuring: usize,
    speed_seen: SpeedSeen,
    /// clip speed follows the music: spots of each video and how fast it moves there
    speed_maps: HashMap<usize, Vec<(f64, f64)>>,
}

pub struct Failure {
    pub name: String,
    pub why: String,
}

/// What clip speed follows the music needs of the song
pub struct PaceInfo {
    pub beats: Vec<f64>,
    pub energy: Vec<f64>,
    pub pace: Vec<f64>,
}
impl PaceInfo {
    /// How fast the music feels at t (0–1)
    pub fn at(&self, t: f64) -> f64 {
        if self.beats.is_empty() {
            return 0.5;
        }
        let k = self.beats.partition_point(|&b| b <= t).saturating_sub(1);
        let e = self.energy.get(k).copied().unwrap_or(0.0);
        if self.pace.is_empty() {
            e
        } else {
            0.6 * self.pace.get(k).copied().unwrap_or(0.0) + 0.4 * e
        }
    }
}

/// Called when a shot gets its clip (clip audio is rendered from it)
pub type AssignHook = Arc<dyn Fn(usize, &Pick) + Send + Sync>;

pub struct MediaOpts {
    /// rendering a file: wait for every clip and frame rather than drop or take a quick one
    pub export: bool,
    /// start decoders (false: only choose clips – generating a PMV)
    pub decode: bool,
    pub fps: f64,
    /// known picks per shot (a project); missing ones are chosen
    pub picks: Option<Vec<Option<Pick>>>,
    /// pinned moments: file path → seconds
    pub pins: HashMap<PathBuf, Vec<f64>>,
    pub pace: Option<Arc<PaceInfo>>,
    pub bpm: f64,
}

pub struct Media {
    pub s: Settings,
    pub plan: Arc<Plan>,
    bpm: f64,
    song_dur: f64,
    pub images: Vec<Item>,
    pool: Mutex<Pool>,
    pool_cond: Condvar,
    pub clock: Arc<Clock>,
    pub assigned: Mutex<HashMap<usize, Arc<Assigned>>>,
    picks: Mutex<Vec<Option<Pick>>>,
    next_assign: AtomicUsize,
    pub failures: Mutex<Vec<Failure>>,
    pub export: bool,
    decode: bool,
    fps: f64,
    pic_cache: Mutex<HashMap<(usize, u32, u32), Arc<Pixmap>>>,
    stop: AtomicBool,
    max_prep: usize,
    pub lookahead: f64,
    pins: HashMap<PathBuf, Vec<f64>>,
    pace: Option<Arc<PaceInfo>>,
    hook: Mutex<Option<AssignHook>>,
    /// clean cuts: how long (clip seconds) a clip should run on in one scene – most shots of the plan fit
    clean_need: f64,
}

impl Media {
    pub fn new(
        s: &Settings,
        plan: Arc<Plan>,
        items: Vec<Item>,
        images: Vec<Item>,
        o: MediaOpts,
    ) -> Arc<Media> {
        let mut rng = rand::thread_rng();
        let mut sources = items.clone();
        for i in (1..sources.len()).rev() {
            let j = rng.gen_range(0..=i);
            sources.swap(i, j);
        }
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let mut picks = o.picks.unwrap_or_default();
        picks.resize(plan.shots.len(), None);
        let song_dur = plan.duration;
        let clean_need = {
            let mut d: Vec<f64> = plan
                .shots
                .iter()
                .map(|sh| plan.shot_clock(sh, sh.t_off))
                .filter(|d| d.is_finite() && *d > 0.0)
                .collect();
            d.sort_by(|a, b| cmp_f64(*a, *b));
            d.get(d.len() * 3 / 4)
                .copied()
                .unwrap_or(3.0)
                .clamp(1.5, 10.0)
        };
        Arc::new(Media {
            s: s.clone(),
            plan,
            bpm: o.bpm,
            song_dur,
            images,
            pool: Mutex::new(Pool {
                sources,
                src_idx: 0,
                ready: Vec::new(),
                preparing: 0,
                prep_keys: Vec::new(),
                recent: VecDeque::new(),
                spots: HashMap::new(),
                bad: HashMap::new(),
                shown: Default::default(),
                measuring: 0,
                speed_seen: SpeedSeen::default(),
                speed_maps: HashMap::new(),
            }),
            pool_cond: Condvar::new(),
            clock: Arc::new(Clock::default()),
            assigned: Mutex::new(HashMap::new()),
            picks: Mutex::new(picks),
            next_assign: AtomicUsize::new(0),
            failures: Mutex::new(Vec::new()),
            export: o.export,
            decode: o.decode,
            fps: o.fps.clamp(1.0, 240.0),
            pic_cache: Mutex::new(HashMap::new()),
            stop: AtomicBool::new(false),
            // At most 3 at once in the browser (each decodes and seeks its video); a desktop with more cores can do more
            max_prep: if o.export || !o.decode {
                (cores / 2).clamp(3, 8)
            } else {
                (cores / 4).clamp(3, 6)
            },
            lookahead: if o.export { 2.0 } else { 3.0 },
            pins: o.pins,
            pace: o.pace,
            hook: Mutex::new(None),
            clean_need,
        })
    }

    pub fn set_hook(&self, h: Option<AssignHook>) {
        *self.hook.lk() = h;
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.assigned.lk().clear();
        self.pool_cond.notify_all();
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// The picks so far (one per shot; None = not chosen yet)
    pub fn picks(&self) -> Vec<Option<Pick>> {
        self.picks.lk().clone()
    }

    /// How many different clips the show has used so far
    pub fn shown_count(&self) -> usize {
        let p = self.picks.lk();
        let n: HashSet<&Path> = p.iter().flatten().map(|x| x.path.as_path()).collect();
        n.len()
    }
    pub fn ready_count(&self) -> (usize, usize) {
        let p = self.pool.lk();
        (p.ready.len(), p.preparing)
    }

    fn note_fail(&self, name: &str, why: &str) {
        let mut f = self.failures.lk();
        if !f.iter().any(|x| x.name == name) {
            f.push(Failure {
                name: name.to_string(),
                why: why.to_string(),
            });
        }
    }

    fn pace_at(&self, t: f64) -> Option<f64> {
        if !self.s.pace_match {
            return None;
        }
        self.pace.as_ref().map(|p| p.at(t))
    }

    /// About how long a clip prepared now waits until it comes on (seconds)
    fn ready_wait(&self, p: &Pool, now: f64) -> f64 {
        let ct = &self.plan.cut_times;
        let i = ct.partition_point(|&c| c <= now);
        let last = &ct[i.saturating_sub(9)..i];
        let gap = if last.len() >= 3 {
            (last[last.len() - 1] - last[0]) / (last.len() - 1) as f64
        } else if self.bpm > 0.0 {
            120.0 / self.bpm
        } else {
            1.0
        };
        (0.5 + (p.ready.len() + 1) as f64 * gap.max(0.2)).clamp(1.5, 20.0)
    }

    // Variety: the same clip not within the last 24, and not while a copy is ready or being prepared
    fn is_recent(p: &Pool, id: usize) -> bool {
        p.recent.contains(&id) || p.ready.iter().any(|r| r.id == id) || p.prep_keys.contains(&id)
    }

    /// A spot of this video shown lately? (within 3 % of its length, at least 6 s)
    fn used_near(p: &Pool, id: usize, t: f64, dur: f64) -> bool {
        let r = (dur * 0.03).max(6.0);
        p.spots
            .get(&id)
            .is_some_and(|l| l.iter().any(|u| (u - t).abs() < r))
    }

    fn seq_window_at(&self, dur: f64, now: f64) -> Option<(f64, f64)> {
        if !self.s.sequenced || dur <= 10.0 || self.song_dur <= 0.0 {
            return None;
        }
        let prog = (now / self.song_dur).clamp(0.0, 1.0);
        let half = (dur * 0.12).max(6.0);
        let lo = (prog * dur - half).min(dur - 6.0).max(0.0);
        Some((lo, (dur - 3.0).min(lo.max(prog * dur + half))))
    }

    /// How near a video's best known spot comes to a speed (0–1 among those seen); None without a map
    fn map_fit(&self, p: &mut Pool, x: &Item, goal: f64, now: f64) -> Option<f64> {
        if x.kind != Kind::Video {
            return None;
        }
        let map = p.speed_maps.get(&x.id)?.clone();
        let seq = self.seq_window_at(x.dur, now);
        let (sh, st) = skips_of(x, &self.s);
        let mut d: Option<f64> = None;
        for (t, speed) in map {
            let Some(pct) = p.speed_seen.pct(Some(speed)) else {
                continue;
            };
            if t < intro_end(x.dur, sh)
                || t >= x.dur - 3.0 - tail_x(x.dur, st)
                || seq.is_some_and(|(lo, hi)| t < lo || t > hi)
            {
                continue;
            }
            let v = (pct - goal).abs()
                + if Self::used_near(p, x.id, t, x.dur) {
                    0.5
                } else {
                    0.0
                };
            if d.is_none_or(|d| v < d) {
                d = Some(v);
            }
        }
        d
    }

    /// Clip speed follows the music: the video with a spot whose speed fits best, of the next 24 in line (or
    /// one shown before this round, when none of those fits well). → one to take again, or None (then the next
    /// in line is the one – moved up)
    fn by_speed(&self, p: &mut Pool, goal: f64, now: f64) -> Option<Item> {
        let mut rng = rand::thread_rng();
        let end = p.sources.len().min(p.src_idx + 24);
        let mut best = p.src_idx;
        let mut low = f64::INFINITY;
        for n in p.src_idx..end {
            let x = p.sources[n].clone();
            if p.bad.contains_key(&x.id) || (self.s.variety && Self::is_recent(p, x.id)) {
                continue;
            }
            let d = self.map_fit(p, &x, goal, now).unwrap_or(0.3) + rng.gen::<f64>() * 0.05;
            if d < low {
                low = d;
                best = n;
            }
        }
        if low > 0.12 {
            for n in 0..p.src_idx.min(p.sources.len()) {
                let x = p.sources[n].clone();
                if p.bad.contains_key(&x.id) || Self::is_recent(p, x.id) {
                    continue;
                }
                let Some(f) = self.map_fit(p, &x, goal, now) else {
                    continue;
                };
                let d = f
                    + 0.25 * p.spots.get(&x.id).map(|l| l.len()).unwrap_or(0) as f64
                    + rng.gen::<f64>() * 0.05;
                if d < low {
                    low = d;
                    best = n;
                }
            }
        }
        if best < p.src_idx {
            return Some(p.sources[best].clone());
        }
        if best > p.src_idx && best < p.sources.len() {
            let x = p.sources.remove(best);
            p.sources.insert(p.src_idx, x);
        }
        None
    }

    fn next_source(&self, p: &mut Pool, goal: Option<f64>, now: f64) -> Option<Item> {
        if p.sources.is_empty() {
            return None;
        }
        let n = p.sources.len();
        for tries in 0..=n * 2 {
            if p.src_idx >= p.sources.len() {
                // a new round, shuffled again
                let mut rng = rand::thread_rng();
                for i in (1..p.sources.len()).rev() {
                    let j = rng.gen_range(0..=i);
                    p.sources.swap(i, j);
                }
                p.src_idx = 0;
            }
            if let (Some(g), 0) = (goal, tries) {
                if let Some(again) = self.by_speed(p, g, now) {
                    return Some(again);
                }
            }
            let s = p.sources[p.src_idx].clone();
            p.src_idx += 1;
            if p.bad.len() >= n {
                return None;
            }
            if p.bad.contains_key(&s.id) {
                continue;
            }
            if !self.s.variety || tries >= n || !Self::is_recent(p, s.id) {
                return Some(s);
            }
        }
        p.sources
            .iter()
            .find(|s| !p.bad.contains_key(&s.id))
            .cloned()
    }

    /// Keep the pool topped up (in the background)
    pub fn fill_pool(self: &Arc<Self>) {
        let cap = if self.s.match_cut || self.s.pace_match {
            8
        } else {
            6
        };
        loop {
            let (item, goal) = {
                let mut p = self.pool.lk();
                if self.stopped()
                    || p.preparing >= self.max_prep
                    || p.ready.len() + p.preparing >= cap
                {
                    return;
                }
                let now = self.clock.get();
                // clip speed follows the music: how fast the music will feel when this clip comes on
                let goal = if self.s.pace_match {
                    let w = self.ready_wait(&p, now);
                    self.pace_at(now + 1.0 + rand::thread_rng().gen::<f64>() * w)
                } else {
                    None
                };
                let Some(it) = self.next_source(&mut p, goal, now) else {
                    return;
                };
                p.preparing += 1;
                p.prep_keys.push(it.id);
                (it, goal)
            };
            let me = self.clone();
            std::thread::spawn(move || {
                let res = me.prepare(&item, goal);
                let fail = {
                    let mut p = me.pool.lk();
                    p.preparing = p.preparing.saturating_sub(1);
                    if let Some(i) = p.prep_keys.iter().position(|&k| k == item.id) {
                        p.prep_keys.remove(i);
                    }
                    match res {
                        Ok(r) => {
                            if !me.stopped() {
                                p.ready.push(Ready {
                                    id: item.id,
                                    pick: r,
                                });
                            }
                            None
                        }
                        Err((why, skip)) => {
                            // (a clip that fails, or doesn't fit the filters, isn't tried again)
                            p.bad.insert(item.id, why.clone());
                            (!skip).then_some(why)
                        }
                    }
                };
                if let Some(why) = fail {
                    me.note_fail(&item.name, &why);
                }
                me.pool_cond.notify_all();
                if !me.stopped() {
                    me.fill_pool();
                }
            });
        }
    }

    /// Prepare a clip: find a good spot. Err((why, skip)) – skip = doesn't fit the filters (not an error)
    fn prepare(&self, s: &Item, goal: Option<f64>) -> Result<Pick, (String, bool)> {
        let mut it = s.clone();
        if it.w == 0 || it.dur <= 0.0 && it.kind == Kind::Video {
            match crate::library::probe_item(&it.path, it.kind) {
                Ok((w, h, d)) => {
                    it.w = w;
                    it.h = h;
                    it.dur = d;
                }
                Err(e) => return Err((e, false)),
            }
        }
        if !shape_ok(it.w, it.h, &self.s.shape) || (it.kind == Kind::Video && it.dur <= 4.0) {
            return Err(("doesn't fit the filters".into(), true));
        }
        let mut rng = rand::thread_rng();
        if it.kind == Kind::Image {
            let sig = image_sig(&it.path, it.w, it.h);
            if sig.is_none() {
                return Err(("this picture can't be read".into(), false));
            }
            let mut p = Pick::from_item(
                &it,
                0.0,
                None,
                if rng.gen::<f64>() < 0.5 { 1.0 } else { -1.0 },
            );
            p.sig = lean(sig);
            return Ok(p);
        }
        let now = self.clock.get();
        let dur = it.dur;
        let seq = self.seq_window_at(dur, now);
        let in_seq = |t: f64| seq.is_none_or(|(lo, hi)| t >= lo && t <= hi);
        let (skip_h, skip_t) = skips_of(&it, &self.s);
        let head = intro_end(dur, skip_h);
        let tail = tail_x(dur, skip_t);
        let rand_t = |rng: &mut rand::rngs::ThreadRng| -> f64 {
            if let Some((lo, hi)) = seq {
                let a = lo.max(head.min(hi));
                a + rng.gen::<f64>() * (hi - lo.max(head)).max(0.0)
            } else if dur > 10.0 {
                head + rng.gen::<f64>() * ((dur * 0.92 - 4.0).min(dur - 4.0 - tail) - head).max(0.0)
            } else {
                0.0
            }
        };
        let (used, ready_n) = {
            let p = self.pool.lk();
            (
                p.spots.get(&it.id).cloned().unwrap_or_default(),
                p.ready.len(),
            )
        };
        let r = (dur * 0.03).max(6.0);
        let fresh = |t: f64| !used.iter().any(|u| (u - t).abs() < r);
        let fresh_rand = |rng: &mut rand::rngs::ThreadRng| {
            let mut t = rand_t(rng);
            for _ in 0..4 {
                if fresh(t) {
                    break;
                }
                t = rand_t(rng);
            }
            t
        };
        let mut start = fresh_rand(&mut rng);
        let mut info: Option<Sig> = None;
        let mut mo: Option<f64> = None;
        let mut spd: Option<f64> = None;
        // Clip speed follows the music: the spot of the video's speed map that fits the goal
        let map_cands: Option<Vec<(f64, f64)>> = goal.and_then(|g| {
            let mut p = self.pool.lk();
            let map = p.speed_maps.get(&it.id)?.clone();
            let mut c: Vec<(f64, f64, f64)> = map
                .into_iter()
                .filter(|(t, _)| *t >= head && *t < dur - 3.0 - tail && in_seq(*t))
                .map(|(t, sp)| {
                    let d = (p.speed_seen.pct(Some(sp)).unwrap_or(0.5) - g).abs()
                        + if fresh(t) { 0.0 } else { 0.5 }
                        + rng.gen::<f64>() * 0.12;
                    (t, sp, d)
                })
                .collect();
            c.sort_by(|a, b| cmp_f64(a.2, b.2));
            Some(c.into_iter().map(|(t, sp, _)| (t, sp)).collect())
        });
        let map_pick = map_cands.as_ref().and_then(|c| c.first().copied());
        // Pinned moments: most of the time a clip of this video starts at one of them
        let pins: Vec<f64> = self
            .pins
            .get(&it.path)
            .map(|l| {
                l.iter()
                    .copied()
                    .filter(|&x| x > 0.0 && x < dur - 1.5)
                    .collect()
            })
            .unwrap_or_default();
        let pinned = !pins.is_empty() && rng.gen::<f64>() < 0.6;
        if pinned {
            start = pins[rng.gen_range(0..pins.len())];
        } else if let Some((t, sp)) = map_pick {
            start = t;
            spd = Some(sp);
        } else if self.s.best_spots && dur > 10.0 {
            // Several random spots, two frames each for motion; a low supply looks at fewer
            let big = (it.w as u64 * it.h as u64) > 2560 * 1440;
            let urgent = (ready_n < 2 && !self.export && self.decode) || big;
            let n = if urgent { 2 } else { 4 };
            let mut scored: Vec<(f64, f64, Sig, f64)> = Vec::new();
            for _ in 0..n {
                if self.stopped() {
                    break;
                }
                let c = fresh_rand(&mut rng);
                let sigs = tiny_frames(&it.path, it.w, it.h, c, &[0.0, 0.35]);
                if sigs.len() < 2 {
                    continue;
                }
                let (a, b) = (&sigs[0], &sigs[1]);
                // (a "motion" that is a cut isn't one)
                let cut = smart::is_cut(a, b);
                let sc = if cut {
                    smart::spot_score(a, a, 0.0) - 5.0
                } else {
                    smart::spot_score(a, b, 0.0)
                };
                scored.push((
                    sc,
                    c,
                    a.clone(),
                    if cut { 0.0 } else { smart::motion(a, b) },
                ));
            }
            scored.sort_by(|x, y| cmp_f64(y.0, x.0));
            if let Some((_, c, sig, m)) = scored.first().cloned() {
                start = c;
                info = Some(sig);
                mo = Some(m);
            }
        }
        // Clean cuts: the clip starts where its scene runs on long enough for most shots – found by looking at
        // every frame around the spot (shots.rs). A pinned moment stays; the others move into their scene.
        let mut clean: Option<f64> = None;
        let mut clean_start = start;
        // (not while a live show is about to run out of clips – they're needed fast then)
        let low_supply = ready_n < 2 && !self.export && self.decode;
        if self.s.clean_cuts && !low_supply && !self.stopped() && dur > 2.0 {
            let lo = head.min(dur - 1.0).max(0.0);
            let hi = (dur - tail - 0.3).max(lo + 0.5);
            if let Some((s2, len)) = self.fit_in_scene(&it, start, lo, hi, !pinned) {
                if (s2 - start).abs() > 0.05 {
                    // (measured again where it starts now)
                    info = None;
                    mo = None;
                    spd = None;
                }
                start = s2;
                clean = Some(len);
                clean_start = s2;
            }
        }
        self.use_spot(it.id, start);
        // Measure the motion here: its speed (clip speed follows the music) and its rhythm (motion on the beat –
        // if it fits the song's tempo the clip starts on a turn of the motion and plays a little faster or
        // slower). Not with big videos (above 1440p: decoding them costs too much)
        const SECS: f64 = 1.8;
        let sync = self.s.beat_sync && self.bpm > 0.0;
        let mut beat: Option<f64> = None;
        let can_measure = {
            let mut p = self.pool.lk();
            let ok = (sync || self.s.pace_match)
                && (it.w as u64 * it.h as u64) <= 2560 * 1440
                && start + SECS < dur - 1.0
                && p.measuring < 3;
            if ok {
                p.measuring += 1;
            }
            ok
        };
        if can_measure {
            if !self.stopped() {
                let gw = smart::tiny(it.w, it.h).0 as usize;
                let mut fr = capture_frames(&it.path, it.w, it.h, start, SECS);
                let mut ms = if self.s.pace_match {
                    motion::action_speed(&fr, gw)
                } else {
                    None
                };
                {
                    let mut p = self.pool.lk();
                    // (a spot picked from the map for its speed doesn't count among the speeds seen)
                    if map_pick.is_none() {
                        p.speed_seen.note(ms);
                    }
                    Self::add_spot(&mut p, it.id, start, ms);
                }
                // the map is coarse: far off the wanted pace, the next best spot is tried and the better taken
                if let (Some(g), Some((mt, _)), Some(c)) = (goal, map_pick, map_cands.as_ref()) {
                    let off =
                        |p: &mut Pool, x: Option<f64>| (p.speed_seen.pct(x).unwrap_or(g) - g).abs();
                    let far = {
                        let mut p = self.pool.lk();
                        ms.is_some() && off(&mut p, ms) > 0.35
                    };
                    if start == mt && c.len() > 1 && far {
                        let alt = c[1].0;
                        let fr2 = capture_frames(&it.path, it.w, it.h, alt, SECS);
                        let ms2 = motion::action_speed(&fr2, gw);
                        let mut p = self.pool.lk();
                        Self::add_spot(&mut p, it.id, alt, ms2);
                        if ms2.is_some() && off(&mut p, ms2) < off(&mut p, ms) {
                            start = alt;
                            fr = fr2;
                            ms = ms2;
                        }
                    }
                }
                if ms.is_some() {
                    spd = ms;
                }
                if sync {
                    if let Some(r) =
                        motion::motion_curve(&fr).and_then(|pts| motion::rhythm_of(&pts))
                    {
                        let mut rate = motion::beat_fit(r.period, self.bpm);
                        if rate.is_none() {
                            if let Some(h) = r.half {
                                rate = motion::beat_fit(h, self.bpm);
                            }
                        }
                        // starts on the first turn from the start on – also when the tempo doesn't fit
                        let step = r.half.unwrap_or(r.period);
                        if step > 0.0 {
                            let turn = r.turn - ((r.turn - start) / step).floor() * step;
                            if !pinned && turn < dur - 1.0 && turn >= 0.0 {
                                start = turn;
                            }
                        }
                        beat = rate;
                    }
                }
            }
            let mut p = self.pool.lk();
            p.measuring = p.measuring.saturating_sub(1);
        }
        if info.is_none() {
            let sigs = tiny_frames(&it.path, it.w, it.h, start, &[0.0]);
            if sigs.is_empty() {
                return Err(("no picture – ffmpeg can't decode this video".into(), false));
            }
            info = sigs.into_iter().next();
        }
        let end_at = (dur - 0.4 > start + 0.5).then_some(dur - 0.4);
        let mut p = Pick::from_item(&it, start, end_at, 0.0);
        // (motion on the beat may have moved the start a little later into the scene)
        p.clean = clean.map(|c| (c - (start - clean_start)).max(0.0));
        p.sig = lean(info);
        p.motion = mo;
        p.speed = spd;
        p.beat = beat;
        p.pinned = pinned;
        Ok(p)
    }

    /// Clean cuts: the scene around `want` (video seconds, between lo and hi). With `may_move`, the start moves
    /// so the clip runs on in one scene for `clean_need` seconds (the nearest scene that is long enough, else
    /// the longest). → (start, seconds until the scene ends)
    fn fit_in_scene(
        &self,
        it: &Item,
        want: f64,
        lo: f64,
        hi: f64,
        may_move: bool,
    ) -> Option<(f64, f64)> {
        let need = self.clean_need;
        let margin = 0.08;
        let stop = AtomicBool::new(false);
        // the scenes between a and b (every frame through the network too: a few seconds cost a second or two)
        let scenes = |a: f64, b: f64| -> Option<Vec<(f64, f64)>> {
            if b - a < 0.5 {
                return None;
            }
            let tr =
                crate::shots::decode(&it.path, a, Some(b - a), 25.0, None, &|_| {}, &stop).ok()?;
            if tr.len() < 4 {
                return None;
            }
            let cuts = crate::shots::detect(&tr, crate::shots::NetUse::Always, &stop);
            Some(crate::shots::shots_of(&tr, &cuts))
        };
        // 1. just the stretch the clip needs: most spots are in a scene that runs on long enough
        // (where a window stops the scene may go on – it counts as ending there: the safe side)
        let end1 = (want + need + 0.6).min(hi.max(want + 0.5));
        let first = scenes((want - 0.3).max(0.0), end1)?;
        let here = first
            .iter()
            .find(|(s0, s1)| want >= *s0 - 0.05 && want < *s1)
            .map(|s| (s.1 - want - margin).max(0.0));
        if !may_move || here.is_some_and(|l| l >= need) || end1 >= hi && here.is_some() {
            return here.map(|l| (want, l));
        }
        // 2. a cut too soon: the nearest scene around that is long enough (else the longest)
        let a = (want - need).max(lo);
        let b = (want + need * 1.6 + 0.5).min(hi.max(want + 0.5));
        let wide = scenes(a, b)?;
        let mut best: Option<(f64, f64, f64)> = None; // (distance, start, length)
        let mut longest: Option<(f64, f64)> = None;
        for &(s0, s1) in &wide {
            let (s0, s1) = (s0 + margin, s1 - margin);
            if s1 - s0 <= 0.3 {
                continue;
            }
            if longest.is_none_or(|l| s1 - s0 > l.1) {
                longest = Some((s0, s1 - s0));
            }
            if s1 - s0 >= need {
                let st = want.clamp(s0, s1 - need);
                let d = (st - want).abs();
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, st, s1 - st));
                }
            }
        }
        best.map(|b| (b.1, b.2)).or(longest)
    }

    fn use_spot(&self, id: usize, t: f64) {
        let mut p = self.pool.lk();
        let l = p.spots.entry(id).or_default();
        l.push(t);
        if l.len() > 8 {
            l.remove(0);
        }
    }

    /// A measured spot into the video's speed map (what was known about that spot gives way; at most 24)
    fn add_spot(p: &mut Pool, id: usize, t: f64, speed: Option<f64>) {
        let Some(speed) = speed else { return };
        let map = p.speed_maps.entry(id).or_default();
        map.retain(|(x, _)| (x - t).abs() >= 1.0);
        map.push((t, speed));
        while map.len() > 24 {
            map.remove(0);
        }
    }

    /// A quick clip without analysis (the pool ran dry)
    fn quick(&self) -> Option<Pick> {
        let it = {
            let mut p = self.pool.lk();
            let now = self.clock.get();
            self.next_source(&mut p, None, now)?
        };
        let mut it = it;
        if it.w == 0 {
            let (w, h, d) = crate::library::probe_item(&it.path, it.kind).ok()?;
            it.w = w;
            it.h = h;
            it.dur = d;
        }
        let mut rng = rand::thread_rng();
        if it.kind == Kind::Image {
            return Some(Pick::from_item(
                &it,
                0.0,
                None,
                if rng.gen::<f64>() < 0.5 { 1.0 } else { -1.0 },
            ));
        }
        let (lo, hi) = start_range(&it, &self.s);
        Some(Pick::video_at(&it, lo + rng.gen::<f64>() * (hi - lo)))
    }

    /// The best of the ready clips for a field: shape fits, match cut, calm/busy (or the music's speed),
    /// motion on the beat, variety
    fn take(&self, shot: &Shot, out: Option<&Pick>, layout: &str) -> Option<Pick> {
        let mut p = self.pool.lk();
        if p.ready.is_empty() {
            return None;
        }
        let out_sig = if self.s.match_cut {
            out.and_then(|o| o.sig.clone())
        } else {
            None
        };
        let mut cand: Vec<usize> = (0..p.ready.len()).collect();
        let want = self
            .s
            .layout_shape
            .get(layout)
            .map(|s| s.as_str())
            .unwrap_or("all");
        if want == "landscape" || want == "portrait" {
            let fit: Vec<usize> = cand
                .iter()
                .copied()
                .filter(|&i| {
                    let r = &p.ready[i].pick;
                    if want == "portrait" {
                        r.h > r.w
                    } else {
                        r.w >= r.h
                    }
                })
                .collect();
            if !fit.is_empty() {
                cand = fit;
            } else if shot.strict {
                return None;
            }
        }
        // Copies of one clip count once
        let mut seen = HashSet::new();
        cand.retain(|&i| seen.insert(p.ready[i].id));
        // (clip speed follows the music: all of them – the ones just prepared for a change of pace are the newest)
        if !self.s.pace_match {
            cand.truncate(if self.s.match_cut { 8 } else { 5 });
        }
        let pace = self.pace_at(shot.t_on);
        // Best moments: calm music gets the calmer clips, loud parts the ones where the most is going on
        let mut rank: HashMap<usize, f64> = HashMap::new();
        if self.s.best_spots && pace.is_none() {
            let mut known: Vec<(usize, f64)> = cand
                .iter()
                .filter_map(|&i| p.ready[i].pick.motion.map(|m| (i, (m / 0.3).min(1.0))))
                .collect();
            known.sort_by(|a, b| cmp_f64(a.1, b.1));
            if known.len() >= 2 {
                let n = known.len() - 1;
                for (k, (i, _)) in known.iter().enumerate() {
                    rank.insert(*i, k as f64 / n as f64);
                }
            }
        }
        let mut best = cand[0];
        let mut score = f64::INFINITY;
        for &i in &cand {
            let id = p.ready[i].id;
            let (w, h, speed, beat) = {
                let m = &p.ready[i].pick;
                (m.w, m.h, m.speed, m.beat)
            };
            let mut sc = ((w as f64 / h.max(1) as f64) / shot.aspect).ln().abs();
            if let Some(os) = &out_sig {
                sc += 1.5 * smart::match_dist(Some(os), p.ready[i].pick.sig.as_ref());
            }
            if let Some(pc) = pace {
                sc += 2.5 * (p.speed_seen.pct(speed).unwrap_or(0.5) - pc).abs();
            } else if !rank.is_empty() {
                sc += 1.2 * (rank.get(&i).copied().unwrap_or(0.5) - shot.energy).abs();
            }
            if self.s.beat_sync && beat.is_some() {
                sc -= 1.0; // motion on the beat: clips whose rhythm fits come first
            }
            if self.s.variety && p.recent.contains(&id) {
                sc += 3.0;
            }
            // clean cuts: a clip whose scene ends before the shot does would show that cut – others first
            if self.s.clean_cuts {
                if let Some(c) = p.ready[i].pick.clean {
                    let need = self.plan.shot_clock(shot, shot.t_off) * p.ready[i].pick.tempo();
                    if c + 0.05 < need {
                        sc += 4.0 + 4.0 * ((need - c) / need.max(0.1)).min(1.0);
                    }
                }
            }
            if sc < score {
                score = sc;
                best = i;
            }
        }
        let m = p.ready.remove(best);
        p.shown.insert(m.id);
        p.recent.push_back(m.id);
        while p.recent.len() > 24 {
            p.recent.pop_front();
        }
        Some(m.pick)
    }

    /// The clip for a shot that has none yet: from the pool (waiting for it when rendering or generating),
    /// else the field keeps its clip (a strict shape), else a quick one. → (pick, smart)
    fn choose(self: &Arc<Self>, i: usize) -> Option<(Pick, bool)> {
        let plan = self.plan.clone();
        let shot = &plan.shots[i];
        let layout = plan.phases[shot.phase].layout.as_str();
        let out = shot
            .prev
            .and_then(|p| self.picks.lk().get(p).cloned().flatten());
        let mut r = self.take(shot, out.as_ref(), layout);
        if r.is_none() && (self.export || !self.decode) {
            // offline: wait for the pool rather than take a quick one
            let t0 = Instant::now();
            while r.is_none() && t0.elapsed() < Duration::from_secs(40) && !self.stopped() {
                self.fill_pool();
                {
                    let p = self.pool.lk();
                    let p = wait_timeout(&self.pool_cond, p, Duration::from_millis(100));
                    if p.preparing == 0 && p.ready.is_empty() && p.bad.len() >= p.sources.len() {
                        break;
                    }
                }
                r = self.take(shot, out.as_ref(), layout);
            }
        }
        if let Some(r) = r {
            return Some((r, true));
        }
        // nothing ready: a field rather keeps its clip (strict) – it plays on from where it is – else a quick one
        if shot.strict {
            if let (Some(o), Some(prev)) = (&out, shot.prev) {
                let mut k = o.clone();
                if k.kind == Kind::Video {
                    let from = o.file_time(&plan, &plan.shots[prev], shot.t_on);
                    if k.dur - 0.4 > from + 0.5 {
                        k.start = from;
                    }
                }
                return Some((k, false));
            }
        }
        self.quick().map(|q| (q, false))
    }

    /// Assign clips to the shots that come on soon, start their decoders, let go of the ones that are over.
    /// Called often (live: a timer thread; export: before each frame).
    pub fn tick(self: &Arc<Self>, now: f64) {
        self.clock.set(now);
        let from = self.next_assign.load(Ordering::SeqCst);
        let missing = self.picks.lk().iter().skip(from).any(|x| x.is_none());
        if missing {
            self.fill_pool();
        }
        let plan = self.plan.clone();
        loop {
            let i = self.next_assign.load(Ordering::SeqCst);
            if i >= plan.shots.len() || plan.shots[i].t_on > now + self.lookahead || self.stopped()
            {
                break;
            }
            // (a shot already over – after a seek – needs no clip)
            if plan.shots[i].until > now - 0.05 {
                self.assign(i);
            }
            self.next_assign.store(i + 1, Ordering::SeqCst);
        }
        // let go of what is over (kept a moment: a frame may still be drawn)
        let mut a = self.assigned.lk();
        a.retain(|&id, _| plan.shots[id].until > now - 0.5);
    }

    /// Jump to t: the clips on screen then are started from there (preview / timeline)
    pub fn seek(self: &Arc<Self>, t: f64) {
        self.assigned.lk().clear();
        let plan = self.plan.clone();
        // the first shot still on screen at t (shots are in t_on order; a long one may come early)
        let first = plan
            .shots
            .iter()
            .position(|s| s.until > t)
            .unwrap_or(plan.shots.len());
        self.next_assign.store(first, Ordering::SeqCst);
        self.tick(t);
    }

    fn assign(self: &Arc<Self>, i: usize) {
        let known = self.picks.lk().get(i).cloned().flatten();
        let (pick, smart) = match known {
            Some(p) => (p, true),
            None => match self.choose(i) {
                Some(x) => x,
                None => return,
            },
        };
        if let Some(slot) = self.picks.lk().get_mut(i) {
            *slot = Some(pick.clone());
        }
        if !self.decode {
            return;
        }
        let a = self.start_shot(i, pick.clone(), smart);
        self.assigned.lk().insert(i, a);
        let hook = self.hook.lk().clone();
        if let Some(h) = hook {
            h(i, &pick);
        }
    }

    /// Put another clip into a shot (timeline, change the moment). A shot on screen or coming up switches at once.
    pub fn repick(self: &Arc<Self>, i: usize, pick: Pick) {
        if i >= self.plan.shots.len() {
            return;
        }
        self.picks.lk()[i] = Some(pick.clone());
        let on = self.assigned.lk().contains_key(&i);
        if on && self.decode {
            let a = self.start_shot(i, pick.clone(), true);
            self.assigned.lk().insert(i, a);
            let hook = self.hook.lk().clone();
            if let Some(h) = hook {
                h(i, &pick);
            }
        }
    }

    /// Choose clips for every shot that has none (generating a PMV: nothing is decoded). progress(done, total)
    pub fn resolve(
        self: &Arc<Self>,
        progress: &dyn Fn(usize, usize),
        cancel: &AtomicBool,
    ) -> Vec<Option<Pick>> {
        let plan = self.plan.clone();
        let total = plan.shots.len();
        for (i, shot) in plan.shots.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) || self.stopped() {
                break;
            }
            let missing = self.picks.lk()[i].is_none();
            if missing {
                // the clock runs ahead of the shots like in a show (scene timeline, music pace)
                self.clock.set((shot.t_on - self.lookahead).max(0.0));
                self.fill_pool();
                if let Some((p, _)) = self.choose(i) {
                    self.picks.lk()[i] = Some(p);
                }
            }
            progress(i + 1, total);
        }
        self.stop();
        self.picks()
    }

    fn start_shot(self: &Arc<Self>, i: usize, pick: Pick, smart: bool) -> Arc<Assigned> {
        let plan = self.plan.clone();
        let shot = &plan.shots[i];
        let (mw, mh) = (pick.w.max(1) as f64, pick.h.max(1) as f64);
        let (fw, fh) = decode_size(
            &self.s.fit,
            shot.field_w as f64,
            shot.field_h as f64,
            mw,
            mh,
        );
        let focus = pick.sig.as_ref().map(|s| s.focus).unwrap_or((0.5, 0.5));
        let live = Mutex::new(LiveSig {
            focus,
            target: focus,
            lum: pick.sig.as_ref().map(|s| s.lum),
            measured_t: shot.t_on,
        });
        if pick.kind == Kind::Image {
            let key = (path_key(&pick.path), fw, fh);
            let cached = self.pic_cache.lk().get(&key).cloned();
            let frames = match cached {
                Some(p) => {
                    let mut f = Frames::new(fw, fh, 1.0, 0, 0);
                    f.still = Some(p);
                    f.done.store(true, Ordering::SeqCst);
                    Arc::new(f)
                }
                None => {
                    let f = Arc::new(Frames::new(fw, fh, 1.0, 1, 0));
                    let me = self.clone();
                    let (path, name) = (pick.path.clone(), pick.name.clone());
                    let fc = f.clone();
                    std::thread::spawn(move || {
                        match ff::decode_rgba(&path, None, fw, fh)
                            .ok()
                            .and_then(|v| to_pixmap(fw, fh, v))
                        {
                            Some(p) => {
                                let p = Arc::new(p);
                                me.pic_cache.lk().insert(key, p.clone());
                                fc.slots.lk()[0] = Some(p);
                                fc.decoded.store(1, Ordering::SeqCst);
                            }
                            None => {
                                *fc.failed.lk() = Some("this picture can't be read".into());
                                me.note_fail(&name, "this picture can't be read");
                            }
                        }
                        fc.finish();
                    });
                    f
                }
            };
            return Arc::new(Assigned {
                shot: i,
                pick,
                frames,
                live,
                smart,
            });
        }
        // Video: how far into the clip this shot goes (speed ramps, stutter, the loop back near its end)
        let fps = self.fps;
        let now = self.clock.get();
        let end = shot.until.min(plan.duration + 1.0);
        let from_t = now.max(shot.t_on);
        let (mut max_idx, mut min_idx) = (0usize, usize::MAX);
        let mut t = shot.t_on;
        while t <= end + 1e-9 {
            let k = (pick.clock(&plan, shot, t) * fps).floor() as usize;
            max_idx = max_idx.max(k);
            if t >= from_t - 1e-9 {
                min_idx = min_idx.min(k);
            }
            t += 1.0 / 60.0;
        }
        let n = max_idx + 3;
        // (a shot that starts on screen decodes from its start; one joined in its middle – a seek – from there)
        let base = if now > shot.t_on + 0.05 && min_idx != usize::MAX {
            min_idx.min(n - 1)
        } else {
            0
        };
        let frames = Arc::new(Frames::new(fw, fh, fps, n, base));
        let a = Arc::new(Assigned {
            shot: i,
            pick: pick.clone(),
            frames: frames.clone(),
            live,
            smart,
        });
        let me = self.clone();
        let view = PlanView::from(&plan, shot);
        let a2 = Arc::downgrade(&a);
        std::thread::spawn(move || {
            let hw = me.s.hwaccel;
            let job = DecodeJob {
                f: &frames,
                pick: &pick,
                n,
                started_t: from_t,
                view: &view,
                me_a: &a2,
            };
            let mut got = me.decode(&job, hw);
            if got == 0 && hw && !frames.cancel.load(Ordering::SeqCst) {
                got = me.decode(&job, false);
            }
            if got == 0 && !frames.cancel.load(Ordering::SeqCst) && !me.stopped() {
                let why = frames
                    .failed
                    .lk()
                    .clone()
                    .unwrap_or_else(|| "no picture – ffmpeg can't decode this video".into());
                me.note_fail(&pick.name, &why);
            }
            frames.finish();
        });
        a
    }

    /// Run one ffmpeg for a shot; returns how many frames came
    fn decode(&self, j: &DecodeJob, hw: bool) -> usize {
        let f = j.f;
        let Ok(mut c) = ff::ffmpeg() else { return 0 };
        if hw {
            c.args(["-hwaccel", "auto"]);
        }
        let from = j.pick.start + f.base as f64 / f.fps;
        c.arg("-ss")
            .arg(format!("{:.3}", from.max(0.0)))
            .arg("-i")
            .arg(&j.pick.path);
        let count = j.n.saturating_sub(f.base);
        c.arg("-t")
            .arg(format!("{:.3}", count as f64 / f.fps + 0.2));
        let flags = if self.export { "bicubic" } else { "bilinear" };
        c.args(["-an", "-sn", "-dn", "-vf"]).arg(format!(
            "fps={},scale={}:{}:flags={flags},format=rgba",
            f.fps, f.fw, f.fh
        ));
        c.args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
        let mut child = match c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(ch) => ch,
            Err(e) => {
                *f.failed.lk() = Some(format!("ffmpeg didn't start: {e}"));
                return 0;
            }
        };
        let (Some(mut out), Some(mut errp)) = (child.stdout.take(), child.stderr.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return 0;
        };
        let err_t = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = errp.read_to_string(&mut s);
            s
        });
        let size = (f.fw * f.fh * 4) as usize;
        let Some(isz) = tiny_skia::IntSize::from_wh(f.fw, f.fh) else {
            let _ = child.kill();
            let _ = child.wait();
            return 0;
        };
        let mut got = 0;
        let mut last_drop = Instant::now();
        while got < count {
            if f.cancel.load(Ordering::SeqCst) || self.stopped() {
                break;
            }
            // Don't run too far ahead (memory): at most ~2× the time the shot has been on, plus a margin
            loop {
                if f.cancel.load(Ordering::SeqCst) || self.stopped() {
                    break;
                }
                let now = self.clock.get();
                let ahead_s = got as f64 / f.fps;
                let allowed = ((now - j.started_t + self.lookahead + 0.5).max(0.0)) * 2.0 + 1.5;
                if ahead_s <= allowed {
                    break;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            let mut buf = vec![0u8; size];
            match ff::read_full(&mut out, &mut buf) {
                Ok(true) => {}
                _ => break,
            }
            let Some(pm) = Pixmap::from_vec(buf, isz) else {
                break;
            };
            let at = f.base + got;
            {
                let mut s = f.slots.lk();
                if at < s.len() {
                    s[at] = Some(Arc::new(pm));
                }
            }
            got += 1;
            f.decoded.store(f.base + got, Ordering::SeqCst);
            {
                let _g = f.gate.lk();
                f.cond.notify_all();
            }
            // Frames no longer needed by this shot are let go
            if last_drop.elapsed() > Duration::from_millis(250) {
                last_drop = Instant::now();
                if let Some(a) = j.me_a.upgrade() {
                    let now = self.clock.get().max(j.view.shot.t_on);
                    let mut lo = usize::MAX;
                    let mut t = now;
                    while t <= j.view.shot.until {
                        lo = lo.min(j.view.index(&a, t));
                        t += 1.0 / 30.0;
                    }
                    // (a decoder that fell behind keeps its newest frame – better late than black)
                    if lo != usize::MAX && lo > 2 {
                        f.drop_below((lo - 2).min((f.base + got).saturating_sub(1)));
                    }
                } else {
                    break; // nobody needs this shot any more
                }
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let err = err_t.join().unwrap_or_default();
        if got == 0 && !err.trim().is_empty() {
            *f.failed.lk() = Some(err.lines().last().unwrap_or("").to_string());
        }
        got
    }

    /// A picture for the image flash / overlay, at the frame's size (cached)
    pub fn picture(&self, idx: usize, w: u32, h: u32, cover_ok: bool) -> Option<Arc<Pixmap>> {
        let it = self.images.get(idx)?;
        if it.w == 0 {
            return None;
        }
        let (mw, mh) = (it.w as f64, it.h as f64);
        let fit = if cover_ok {
            fit_for("contain", w as f64, h as f64, mw, mh)
        } else {
            "contain"
        };
        let r = if fit == "contain" {
            (w as f64 / mw).min(h as f64 / mh)
        } else {
            (w as f64 / mw).max(h as f64 / mh)
        };
        let (fw, fh) = (
            ((mw * r).round() as u32).max(2),
            ((mh * r).round() as u32).max(2),
        );
        let key = (usize::MAX - path_key(&it.path), fw, fh);
        if let Some(p) = self.pic_cache.lk().get(&key) {
            return Some(p.clone());
        }
        let p = Arc::new(to_pixmap(
            fw,
            fh,
            ff::decode_rgba(&it.path, None, fw, fh).ok()?,
        )?);
        let mut c = self.pic_cache.lk();
        if c.len() > 64 {
            c.clear();
        }
        c.insert(key, p.clone());
        Some(p)
    }

    /// Decode the pictures the plan will flash ahead of time (export / start of a show)
    pub fn warm_pictures(self: &Arc<Self>, w: u32, h: u32) {
        let mut ids: Vec<(usize, bool)> = Vec::new();
        for e in &self.plan.fx {
            match e.fx {
                crate::plan::Fx::PicFlash { pic, .. } => ids.push((pic, true)),
                crate::plan::Fx::PicOver { pic, .. } => ids.push((pic, false)),
                _ => {}
            }
        }
        ids.sort();
        ids.dedup();
        let me = self.clone();
        std::thread::spawn(move || {
            for (i, flash) in ids {
                if me.stopped() {
                    break;
                }
                let _ = me.picture(i, w, h, flash);
            }
        });
    }
}

/// A cache key of a file path (the library's ids are new in every run; paths stay)
fn path_key(p: &Path) -> usize {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    (h.finish() as usize) >> 1
}

struct DecodeJob<'a> {
    f: &'a Arc<Frames>,
    pick: &'a Pick,
    n: usize,
    started_t: f64,
    view: &'a PlanView,
    me_a: &'a Weak<Assigned>,
}

/// What a decoder needs of the plan to know which frames its shot still needs
pub struct PlanView {
    plan_rates: Vec<(f64, f64)>,
    cum: Vec<f64>,
    stutters: Vec<f64>,
    shot: Shot,
}
impl PlanView {
    fn from(plan: &Plan, shot: &Shot) -> PlanView {
        let lo = shot.t_on - 1.0;
        let hi = shot.until + 1.0;
        // (the rate before the shot matters too: keep everything up to its end – it's small)
        let rates: Vec<(f64, f64)> = plan.rates.iter().copied().filter(|r| r.0 <= hi).collect();
        let cum = rates.iter().map(|r| plan.rate_integral(r.0)).collect();
        PlanView {
            plan_rates: rates,
            cum,
            stutters: plan
                .stutters
                .iter()
                .copied()
                .filter(|&s| s >= lo && s <= hi)
                .collect(),
            shot: shot.clone(),
        }
    }
    fn integral(&self, t: f64) -> f64 {
        let i = self.plan_rates.partition_point(|r| r.0 <= t);
        if i == 0 {
            t
        } else {
            self.cum[i - 1] + (t - self.plan_rates[i - 1].0) * self.plan_rates[i - 1].1
        }
    }
    fn index(&self, a: &Assigned, t: f64) -> usize {
        let shot = &self.shot;
        let t = t.max(shot.t_on);
        let lim = t.min(shot.t_off);
        let i = self.stutters.partition_point(|&s| s <= lim);
        let (mut bt, mut base) = (shot.t_on, 0.0);
        if i > 0 && self.stutters[i - 1] > shot.t_on {
            bt = self.stutters[i - 1];
            base = (bt - shot.t_on) * 0.25;
        }
        let c = (base + self.integral(t) - self.integral(bt)) * a.pick.tempo();
        (wrap(a.pick.start, a.pick.end_at, c) * a.frames.fps).floor() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_loops_near_the_end() {
        assert_eq!(wrap(10.0, Some(20.0), 5.0), 5.0);
        assert!((wrap(10.0, Some(20.0), 12.0) - 2.0).abs() < 1e-9);
        assert_eq!(wrap(10.0, None, 50.0), 50.0);
        assert_eq!(wrap(10.0, Some(20.0), -1.0), 0.0);
    }

    #[test]
    fn frames_from_a_base() {
        let f = Frames::new(2, 2, 30.0, 10, 4);
        assert!(!f.any());
        assert!(f.get(5, None).is_none());
        let pm = Arc::new(Pixmap::new(2, 2).unwrap());
        f.slots.lk()[4] = Some(pm.clone());
        f.slots.lk()[5] = Some(pm.clone());
        f.decoded.store(6, Ordering::SeqCst);
        assert!(f.any());
        // before the base: the first one there
        assert!(f.get(1, None).is_some());
        assert!(f.get(5, None).is_some());
        // ahead: the newest
        assert!(f.get(9, None).is_some());
    }

    #[test]
    fn pace_info() {
        let p = PaceInfo {
            beats: vec![0.0, 1.0, 2.0],
            energy: vec![0.0, 1.0, 0.5],
            pace: vec![],
        };
        assert_eq!(p.at(1.5), 1.0);
        let q = PaceInfo {
            pace: vec![1.0, 0.0, 0.0],
            ..p
        };
        assert!((q.at(0.2) - 0.6).abs() < 1e-9);
    }
}
