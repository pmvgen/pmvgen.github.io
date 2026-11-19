// The direction: what happens on each beat – a port of the generator's onBeat() and its helpers.
// The browser version decides beat by beat while it plays; the decisions only depend on the song
// (beats, energy, bars) and chance, so here the whole song is worked out before it starts: a timeline
// of layouts, shots (which field gets a new clip when), effect triggers and speed ramps. Which clip goes
// into a shot is picked shortly before it comes on (media.rs), like the browser version.

use crate::bars::Bars;
use crate::settings::Settings;
use crate::template::TplEvent;
use crate::util::cmp_f64;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

pub struct LayoutInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub groups: usize,
    pub level: i32,
    pub hint: &'static str,
}
pub const LAYOUTS: [LayoutInfo; 6] = [
    LayoutInfo {
        id: "full",
        name: "Fullscreen",
        groups: 1,
        level: 1,
        hint: "One clip over everything",
    },
    LayoutInfo {
        id: "kaleido",
        name: "Kaleidoscope",
        groups: 1,
        level: 2,
        hint: "One clip, mirrored four times",
    },
    LayoutInfo {
        id: "duo",
        name: "2-way",
        groups: 2,
        level: 2,
        hint: "Two clips side by side",
    },
    LayoutInfo {
        id: "trim",
        name: "3-way mirrored",
        groups: 2,
        level: 3,
        hint: "Left and right the same clip mirrored, a different one in the middle",
    },
    LayoutInfo {
        id: "tri",
        name: "3-way",
        groups: 3,
        level: 3,
        hint: "Three different clips",
    },
    LayoutInfo {
        id: "quad",
        name: "4-way",
        groups: 4,
        level: 4,
        hint: "Four fields",
    },
];
pub fn layout(id: &str) -> &'static LayoutInfo {
    LAYOUTS.iter().find(|l| l.id == id).unwrap_or(&LAYOUTS[0])
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Slot {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub g: usize,
    pub fx: bool,
    pub fy: bool,
}

/// Fields of a layout: rectangle, group (which clip) and mirroring. dir "cols" = side by side, "rows" = stacked.
pub fn slots_for(id: &str, w: i32, h: i32, dir: &str) -> Vec<Slot> {
    let wide = dir != "rows";
    let rnd = |x: f64| x.round() as i32;
    let split = |n: i32, groups: &[usize], mirror: &[bool]| -> Vec<Slot> {
        groups
            .iter()
            .enumerate()
            .map(|(i, &g)| {
                let total = if wide { w } else { h } as f64;
                let a = rnd(i as f64 * total / n as f64);
                let b = rnd((i + 1) as f64 * total / n as f64);
                if wide {
                    Slot {
                        x: a,
                        y: 0,
                        w: b - a,
                        h,
                        g,
                        fx: mirror[i],
                        fy: false,
                    }
                } else {
                    Slot {
                        x: 0,
                        y: a,
                        w,
                        h: b - a,
                        g,
                        fx: mirror[i],
                        fy: false,
                    }
                }
            })
            .collect()
    };
    let quad = |group_of: &dyn Fn(usize) -> usize,
                mirror_of: &dyn Fn(usize) -> (bool, bool)|
     -> Vec<Slot> {
        (0..4)
            .map(|i| {
                let x0 = rnd((i % 2) as f64 * w as f64 / 2.0);
                let y0 = rnd((i / 2) as f64 * h as f64 / 2.0);
                let (fx, fy) = mirror_of(i);
                Slot {
                    x: x0,
                    y: y0,
                    w: rnd(w as f64 / 2.0),
                    h: rnd(h as f64 / 2.0),
                    g: group_of(i),
                    fx,
                    fy,
                }
            })
            .collect()
    };
    match id {
        "duo" => split(2, &[0, 1], &[false, false]),
        "trim" => split(3, &[0, 1, 0], &[false, false, true]),
        "tri" => split(3, &[0, 1, 2], &[false, false, false]),
        "quad" => quad(&|i| i, &|_| (false, false)),
        "kaleido" => quad(&|_| 0, &|i| (i % 2 == 1, i >= 2)),
        _ => vec![Slot {
            x: 0,
            y: 0,
            w,
            h,
            g: 0,
            fx: false,
            fy: false,
        }],
    }
}
pub fn aspect_of_group(slots: &[Slot], g: usize) -> f64 {
    let s = slots.iter().find(|x| x.g == g).unwrap_or(&slots[0]);
    s.w as f64 / s.h.max(1) as f64
}

pub const SCROLL: f64 = 0.34; // seconds a scroll cut takes

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Phase {
    pub t: f64,
    pub layout: String,
    /// "cols" = fields side by side, "rows" = stacked (a template can switch per layout)
    pub dir: String,
    pub slots: Vec<Slot>,
    pub groups: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shot {
    pub id: usize,
    pub phase: usize,
    pub group: usize,
    pub t_on: f64,
    pub t_off: f64,  // when the next clip takes the field (or the layout changes)
    pub until: f64,  // when it is off the screen (a scroll cut keeps it a moment longer)
    pub scroll: i32, // this shot came in by a scroll cut (direction), 0 = normal cut
    pub prev: Option<usize>, // the shot it replaces (match cut)
    pub aspect: f64, // the field's shape
    pub strict: bool, // (per-layout clip shape: a single field rather keeps its clip than shows the wrong shape)
    pub energy: f64,  // the music's energy when it is picked
    pub zoom_dir: i32, // zoom-in entry: alternating strong (1) / soft (-1)
    pub field_w: i32, // the largest field of its group (for the decoding size)
    pub field_h: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pulse {
    pub t: f64,
    pub energy: f64,
    pub beat_t: f64,
    pub beat_amt: f64,
    pub next_t: Option<f64>,
    pub next_amt: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Fx {
    /// force: shown even with the flash effect off (a template's flashes)
    Flash {
        a: f64,
        color: [u8; 3],
        #[serde(default)]
        force: bool,
    },
    Shake {
        until: f64,
    },
    Glitch {
        until: f64,
    },
    Rgb {
        amt: f64,
        dur: f64,
    },
    Invert {
        until: f64,
    },
    Tunnel {
        dur: f64,
    },
    Strobe,
    Text {
        word: String,
    },
    PicFlash {
        t1: f64,
        pic: usize,
    },
    PicOver {
        t1: f64,
        alpha: f64,
        pic: usize,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FxEvent {
    pub t: f64,
    pub fx: Fx,
}

/// (phase, the shot of each group, the shot scrolling out of each group: (shot, since, direction))
pub type ShotsAt = (usize, Vec<Option<usize>>, Vec<Option<(usize, f64, i32)>>);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub w: i32,
    pub h: i32,
    pub duration: f64,
    /// the chance behind every decision – also the effects' (the same plan always draws the same pictures)
    #[serde(default)]
    pub seed: u64,
    pub phases: Vec<Phase>,
    pub shots: Vec<Shot>,
    pub pulses: Vec<Pulse>,
    pub fx: Vec<FxEvent>,       // sorted by t
    pub rates: Vec<(f64, f64)>, // (from t, rate), sorted
    #[serde(skip)]
    rate_cum: Vec<f64>, // ∫ rate up to each change
    pub stutters: Vec<f64>,
    pub cut_times: Vec<f64>,
    pub reveal: Option<(f64, f64)>, // (until: when the opening ends, end_t: its full size)
    /// drops (time, beat length): clip audio "only on drops" fades the main clip in there for four beats
    #[serde(default)]
    pub drops: Vec<(f64, f64)>,
}

impl Plan {
    /// Sorts what must be sorted and builds the lookup tables (after making or loading a plan)
    pub fn finish(&mut self) {
        self.fx.sort_by(|a, b| cmp_f64(a.t, b.t));
        self.pulses.sort_by(|a, b| cmp_f64(a.t, b.t));
        self.stutters.sort_by(|a, b| cmp_f64(*a, *b));
        self.rates.sort_by(|a, b| cmp_f64(a.0, b.0));
        self.rate_cum.clear();
        let (mut acc, mut prev_t, mut prev_r) = (0.0, 0.0, 1.0);
        for &(t, r) in &self.rates {
            acc += (t - prev_t) * prev_r;
            self.rate_cum.push(acc);
            prev_t = t;
            prev_r = r;
        }
    }

    /// Checks a plan from a file: everything in range, so drawing it can't go wrong
    pub fn validate(&self) -> anyhow::Result<()> {
        use anyhow::bail;
        if !(self.duration.is_finite() && self.duration > 0.0) {
            bail!("the length of the show is missing");
        }
        if self.w < 16 || self.h < 16 || self.w > 16384 || self.h > 16384 {
            bail!("the picture size {}×{} isn't possible", self.w, self.h);
        }
        if self.phases.is_empty() || self.phases[0].t > 0.0 {
            bail!("the layouts are missing");
        }
        for ph in &self.phases {
            if ph.groups == 0 || ph.slots.iter().any(|s| s.g >= ph.groups) {
                bail!("a layout has fields without a clip");
            }
        }
        for (i, sh) in self.shots.iter().enumerate() {
            if sh.id != i
                || sh.phase >= self.phases.len()
                || sh.group >= self.phases[sh.phase].groups
            {
                bail!("shot {i} doesn't fit its layout");
            }
            if sh.prev.is_some_and(|p| p >= i) {
                bail!("shot {i} refers to a later one");
            }
            if !(sh.t_on.is_finite() && sh.t_off >= sh.t_on && sh.until >= sh.t_off) {
                bail!("shot {i} has broken times");
            }
        }
        if self
            .shots
            .windows(2)
            .any(|w| w[0].phase > w[1].phase || w[0].t_on > w[1].t_on)
        {
            bail!("the shots are out of order");
        }
        if self.rates.iter().any(|r| !(r.1.is_finite() && r.1 > 0.0)) {
            bail!("a speed ramp is broken");
        }
        Ok(())
    }

    /// The same plan for another picture size (the preview draws smaller than the export)
    pub fn resized(&self, w: i32, h: i32) -> Plan {
        let mut p = self.clone();
        p.w = w;
        p.h = h;
        for ph in p.phases.iter_mut() {
            ph.slots = slots_for(&ph.layout, w, h, &ph.dir);
        }
        for sh in p.shots.iter_mut() {
            let ph = &p.phases[sh.phase];
            let (mut fw, mut fh) = (0, 0);
            for s in ph.slots.iter().filter(|s| s.g == sh.group) {
                if s.w * s.h > fw * fh {
                    fw = s.w;
                    fh = s.h;
                }
            }
            sh.field_w = fw.max(2);
            sh.field_h = fh.max(2);
        }
        p
    }

    /// The shot in group g at time t (the clip in that field then)
    pub fn shot_in(&self, g: usize, t: f64) -> Option<usize> {
        let (_, cur, _) = self.shots_at(t);
        cur.get(g).copied().flatten()
    }

    pub fn phase_at(&self, t: f64) -> usize {
        self.phases.partition_point(|p| p.t <= t).saturating_sub(1)
    }
    pub fn pulse_at(&self, t: f64) -> Option<&Pulse> {
        let i = self.pulses.partition_point(|p| p.t <= t);
        if i == 0 {
            None
        } else {
            Some(&self.pulses[i - 1])
        }
    }
    pub fn cuts_before(&self, t: f64) -> usize {
        self.cut_times.partition_point(|&c| c <= t)
    }
    /// ∫ rate from 0 to t
    pub fn rate_integral(&self, t: f64) -> f64 {
        let i = self.rates.partition_point(|r| r.0 <= t);
        if i == 0 {
            t // rate 1 before the first change
        } else {
            self.rate_cum[i - 1] + (t - self.rates[i - 1].0) * self.rates[i - 1].1
        }
    }
    /// Video time of a shot at show time t (seconds from the clip's start spot; before wrapping)
    pub fn shot_clock(&self, shot: &Shot, t: f64) -> f64 {
        let t = t.max(shot.t_on);
        // the last stutter while the shot is in its field (not while it scrolls out)
        let lim = t.min(shot.t_off);
        let i = self.stutters.partition_point(|&s| s <= lim);
        let mut base_t = shot.t_on;
        let mut base = 0.0;
        if i > 0 {
            let ts = self.stutters[i - 1];
            if ts > shot.t_on {
                base_t = ts;
                base = (ts - shot.t_on).max(0.0) * 0.25;
            }
        }
        base + self.rate_integral(t) - self.rate_integral(base_t)
    }
    /// The shot of each group on screen at t in the current phase, and those scrolling out
    pub fn shots_at(&self, t: f64) -> ShotsAt {
        let pi = self.phase_at(t);
        let ph = &self.phases[pi];
        let mut cur = vec![None; ph.groups];
        let mut leave = vec![None; ph.groups];
        // shots are in t_on order; the phase's shots are a contiguous run
        let start = self.shots.partition_point(|s| s.phase < pi);
        let end = self.shots.partition_point(|s| s.phase <= pi);
        for s in &self.shots[start..end] {
            if s.t_on <= t {
                cur[s.group] = Some(s.id);
            }
        }
        for g in 0..ph.groups {
            if let Some(id) = cur[g] {
                let s = &self.shots[id];
                if s.scroll != 0 && t - s.t_on < SCROLL {
                    if let Some(p) = s.prev {
                        if self.shots[p].phase == pi {
                            leave[g] = Some((p, s.t_on, s.scroll));
                        }
                    }
                }
            }
        }
        (pi, cur, leave)
    }
}

pub struct Input<'a> {
    pub beats: &'a [f64],
    pub energy: &'a [f64],
    pub duration: f64,
    pub bars: Option<&'a Bars>,
    pub s: &'a Settings,
    pub w: i32,
    pub h: i32,
    pub n_images: usize,
    /// layout id → can it be used (per-layout clip shape: enough clips of that shape in the library)
    pub layout_ok: &'a dyn Fn(&str) -> bool,
    pub seed: u64,
    /// PMV as template: its cuts, layouts and flashes instead of the direction's own
    pub template: Option<&'a [TplEvent]>,
}

struct Dir<'a> {
    inp: &'a Input<'a>,
    rng: StdRng,
    layouts: Vec<&'static str>,
    layout: &'static str,
    dir: String,
    cur_dir: &'static str,
    tpl: bool,
    drops: Vec<(f64, f64)>,
    phases: Vec<Phase>,
    shots: Vec<Shot>,
    cur: Vec<usize>, // group → current shot id
    cut_k: Vec<i64>,
    cut_t: Vec<f64>,
    next_group: usize,
    layout_beat: i64,
    layout_hold: i64,
    last_drop: i64,
    last_cut: i64,
    center_k: Option<i64>,
    scroll_dir: i32,
    side_n: usize,
    cur_k: i64,
    energy_now: f64,
    zoom_flip: bool,
    fx: Vec<FxEvent>,
    pulses: Vec<Pulse>,
    rates: Vec<(f64, f64)>,
    rate: f64,
    rate_hold: f64,
    stutters: Vec<f64>,
    cut_times: Vec<f64>,
    revealing: bool,
    reveal: Option<(i64, f64)>, // (end beat, end time)
    reveal_until: f64,
    words: Vec<String>,
    pic_order: Vec<usize>,
    pic_spent: Vec<usize>,
    flash_until: f64,
    over_until: f64,
}

impl<'a> Dir<'a> {
    fn energy(&self, k: i64) -> f64 {
        if k < 0 {
            0.0
        } else {
            self.inp.energy.get(k as usize).copied().unwrap_or(0.0)
        }
    }
    fn is_bar(&self, k: usize) -> bool {
        match self.inp.bars {
            Some(b) => b.downbeat.get(k).copied().unwrap_or(false),
            None => k.is_multiple_of(4),
        }
    }
    fn trig(&mut self, t: f64, fx: Fx) {
        let s = &self.inp.s.fx;
        let on = match &fx {
            Fx::Flash { force, .. } => s.flash || *force,
            Fx::Shake { .. } => s.shake,
            Fx::Glitch { .. } => s.glitch,
            Fx::Rgb { .. } => s.rgb,
            Fx::Invert { .. } => s.invert,
            Fx::Tunnel { .. } => s.tunnel,
            Fx::Strobe => s.strobe,
            Fx::Text { .. } => s.text && !self.words.is_empty(),
            Fx::PicFlash { .. } | Fx::PicOver { .. } => true,
        };
        if on {
            self.fx.push(FxEvent { t, fx });
        }
    }

    fn new_shot(
        &mut self,
        g: usize,
        t: f64,
        prev: Option<usize>,
        strict: bool,
        scroll: i32,
    ) -> usize {
        let pi = self.phases.len() - 1;
        let ph = &self.phases[pi];
        // the largest field of the group decides how big its clip is decoded
        let (mut fw, mut fh) = (0, 0);
        for s in ph.slots.iter().filter(|s| s.g == g) {
            if s.w * s.h > fw * fh {
                fw = s.w;
                fh = s.h;
            }
        }
        self.zoom_flip = !self.zoom_flip;
        let id = self.shots.len();
        self.shots.push(Shot {
            id,
            phase: pi,
            group: g,
            t_on: t,
            t_off: f64::INFINITY,
            until: f64::INFINITY,
            scroll,
            prev,
            aspect: aspect_of_group(&ph.slots, g),
            strict,
            energy: self.energy_now,
            zoom_dir: if self.zoom_flip { 1 } else { -1 },
            field_w: fw.max(2),
            field_h: fh.max(2),
        });
        id
    }
    fn close(&mut self, id: usize, t: f64, keep: f64) {
        let s = &mut self.shots[id];
        if s.t_off.is_infinite() {
            s.t_off = t;
            s.until = t + keep;
        }
    }

    fn cut_group(&mut self, gi: usize, t: f64, scroll: i32) {
        let old = self.cur[gi];
        self.close(old, t, if scroll != 0 { SCROLL } else { 0.0 });
        let id = self.new_shot(gi, t, Some(old), true, scroll);
        self.cur[gi] = id;
        self.cut_t[gi] = t;
        self.cut_k[gi] = self.cur_k;
        self.cut_times.push(t);
    }

    fn set_layout(&mut self, id: &'static str, t: f64) {
        let dir = self.dir.clone();
        self.set_layout_dir(id, t, &dir);
    }
    fn set_layout_dir(&mut self, id: &'static str, t: f64, dir: &str) {
        let old = std::mem::take(&mut self.cur);
        for &o in &old {
            self.close(o, t, 0.0);
        }
        self.layout = id;
        let dir = if dir == "rows" { "rows" } else { "cols" };
        self.cur_dir = dir;
        let slots = slots_for(id, self.inp.w, self.inp.h, dir);
        let n = layout(id).groups;
        self.phases.push(Phase {
            t,
            layout: id.to_string(),
            dir: dir.to_string(),
            slots,
            groups: n,
        });
        self.cut_t = vec![t; n];
        self.cut_k = vec![self.cur_k; n];
        for gi in 0..n {
            let prev = if old.is_empty() {
                None
            } else {
                Some(old[gi % old.len()])
            };
            let sid = self.new_shot(gi, t, prev, prev.is_some(), 0);
            self.cur.push(sid);
        }
        self.next_group = 0;
        self.cut_times.push(t);
    }

    fn level_for(e: f64, drop: bool) -> i32 {
        if drop {
            4
        } else if e > 0.6 {
            3
        } else if e > 0.35 {
            2
        } else {
            1
        }
    }

    fn pick_layout(&mut self, e: f64, drop: bool) -> Option<&'static str> {
        let mut want = Self::level_for(e, drop);
        if want == 3 && e > 0.8 && self.rng.gen::<f64>() < 0.2 {
            want = 4;
        }
        let dist = |k: &str| (layout(k).level - want).abs();
        let one = self.layouts.len() == 1;
        let cands: Vec<&'static str> = self
            .layouts
            .iter()
            .copied()
            .filter(|k| (*k != self.layout || one) && (self.inp.layout_ok)(k))
            .collect();
        if cands.is_empty() {
            return None;
        }
        let min = cands.iter().map(|k| dist(k)).min().unwrap();
        let pool: Vec<&'static str> = cands.into_iter().filter(|k| dist(k) == min).collect();
        Some(pool[self.rng.gen_range(0..pool.len())])
    }

    fn pulse_for(&self, k: i64, e: f64, bar: bool, drop: bool) -> f64 {
        let on = self.inp.s.pulse_on.as_str();
        if !drop && ((on == "2" && k % 2 != 0) || (on == "bar" && !bar)) {
            return 0.0;
        }
        0.03 + 0.07 * e + if drop { 0.1 } else { 0.0 }
    }

    fn set_rate(&mut self, t: f64, rate: f64, hold: f64) {
        if hold == 0.0 && self.rate_hold > 0.0 && t < self.rate_hold {
            return;
        }
        self.rate_hold = if hold > 0.0 { t + hold } else { 0.0 };
        if rate != self.rate {
            self.rate = rate;
            self.rates.push((t, rate));
        }
    }

    fn take_pic(&mut self) -> Option<usize> {
        let n = self.inp.n_images;
        if n == 0 {
            return None;
        }
        if self.pic_order.is_empty() {
            let mut v: Vec<usize> = (0..n).collect();
            for i in (1..v.len()).rev() {
                let j = self.rng.gen_range(0..=i);
                v.swap(i, j);
            }
            // (not the ones just shown first)
            v.sort_by_key(|x| self.pic_spent.contains(x));
            v.reverse();
            self.pic_order = v;
        }
        let p = self.pic_order.pop()?;
        self.pic_spent.push(p);
        if self.pic_spent.len() > 24 {
            self.pic_spent.remove(0);
        }
        Some(p)
    }

    fn pic_beat(
        &mut self,
        k: usize,
        e: f64,
        bar: bool,
        len: f64,
        pending: &mut Vec<(f64, u8, f64)>,
    ) {
        let s = self.inp.s;
        let beats = self.inp.beats;
        let t0 = beats[k];
        if s.pic_flash && t0 >= self.flash_until && self.rng.gen::<f64>() * 100.0 < s.pic_flash_pct
        {
            let rate = if s.pic_flash_rate == "auto" {
                if e > 0.72 {
                    4.0
                } else if e > 0.4 {
                    2.0
                } else {
                    1.0
                }
            } else {
                s.pic_flash_rate.parse::<f64>().unwrap_or(1.0).max(1.0)
            };
            let step = len / rate;
            let burst = s.pic_flash_mode == "burst"
                || (s.pic_flash_mode != "single" && self.rng.gen::<f64>() < 0.5);
            let n = if burst {
                ((if s.pic_flash_len > 0.0 {
                    s.pic_flash_len
                } else {
                    2.0
                }) * rate)
                    .round()
                    .max(1.0) as usize
            } else {
                1
            };
            for i in 0..n {
                pending.push((t0 + i as f64 * step, 0, step * 0.96));
            }
            self.flash_until = t0 + n as f64 * step;
        }
        let on_bar = if self.inp.bars.is_some() {
            bar
        } else {
            k.is_multiple_of(4)
        };
        if s.pic_overlay
            && on_bar
            && t0 >= self.over_until
            && self.rng.gen::<f64>() * 100.0 < s.pic_overlay_pct
        {
            let n = (if s.pic_overlay_len > 0.0 {
                s.pic_overlay_len
            } else {
                4.0
            })
            .round()
            .max(1.0) as usize;
            let dur = beats.get(k + n).map(|b| b - t0).unwrap_or(n as f64 * len);
            self.over_until = t0 + dur;
            pending.push((t0, 1, dur));
        }
    }

    fn on_beat(&mut self, k: usize, t: f64, pending: &mut Vec<(f64, u8, f64)>) {
        let s = self.inp.s;
        let ki = k as i64;
        self.cur_k = ki;
        let min_b = s.min_beats.round().max(0.0) as i64;
        let max_b = s.max_beats.round().max(0.0) as i64;
        let beats = self.inp.beats;
        let e = self.energy(ki);
        let prev = self.energy(ki - 4);
        let mut drop = e - prev > 0.35 && e > 0.6 && ki - self.last_drop >= 16;
        let mut reveal_ends = false;
        if self.revealing {
            if let Some((end_k, _)) = self.reveal {
                if ki >= end_k {
                    reveal_ends = true;
                    self.revealing = false;
                    self.reveal_until = t;
                    drop = true;
                }
            }
        }
        if drop {
            self.last_drop = ki;
        }
        let bars = self.inp.bars;
        let bar = self.is_bar(k);
        let len = beats.get(k + 1).copied().unwrap_or(beats[k] + 0.5) - beats[k];
        self.energy_now = e;
        let next_bar = self.is_bar(k + 1);
        self.pulses.push(Pulse {
            t,
            energy: e,
            beat_t: beats[k],
            beat_amt: self.pulse_for(ki, e, bar, drop),
            next_t: beats.get(k + 1).copied(),
            next_amt: if beats.get(k + 1).is_some() {
                self.pulse_for(ki + 1, self.energy(ki + 1), next_bar, false)
            } else {
                0.0
            },
        });

        // Change layout: on drops right away; otherwise every 2 bars if the mood has changed or – in loud parts – for variety
        let want = Self::level_for(e, drop);
        let mood_changed = layout(self.layout).level != want
            && self
                .layouts
                .iter()
                .any(|x| layout(x).level == want || want > 1);
        let due = if let Some(b) = bars {
            let ph = b.phrase.get(k).copied().unwrap_or(false);
            (ph && ki - self.layout_beat >= self.layout_hold
                && (mood_changed || (want >= 3 && self.rng.gen::<f64>() < 0.6)))
                || (bar && mood_changed && ki - self.layout_beat >= 24)
        } else {
            bar && ki - self.layout_beat >= self.layout_hold
                && (mood_changed || (want >= 3 && self.rng.gen::<f64>() < 0.6))
        };
        if drop {
            self.drops.push((t, len));
        }
        let mut changed = false;
        if self.tpl {
            // cuts and layouts come from the template (apply_event)
            changed = true;
        } else if self.layouts.len() > 1
            && ((drop && ki - self.layout_beat >= min_b) || due)
            && !self.revealing
        {
            if let Some(next) = self.pick_layout(e, drop) {
                changed = true;
                self.set_layout(next, t);
                self.layout_beat = ki;
                self.center_k = Some(ki);
                let tri = self.layout == "tri" || self.layout == "trim";
                self.scroll_dir =
                    if s.scroll && !drop && e <= 0.9 && tri && self.rng.gen::<f64>() < 0.75 {
                        if self.rng.gen::<f64>() < 0.5 {
                            -1
                        } else {
                            1
                        }
                    } else {
                        0
                    };
                self.side_n = 0;
                self.layout_hold = (if drop { 4 } else { 8 }).max(min_b);
                self.last_cut = ki;
                self.trig(
                    t,
                    Fx::Flash {
                        a: if drop { 0.9 } else { 0.35 },
                        color: if drop {
                            [255, 62, 138]
                        } else {
                            [255, 255, 255]
                        },
                        force: false,
                    },
                );
            }
        }
        if !changed {
            // Within the layout: re-cut the fields in turn
            if reveal_ends && self.layouts.len() < 2 {
                self.trig(
                    t,
                    Fx::Flash {
                        a: 0.9,
                        color: [255, 62, 138],
                        force: false,
                    },
                );
            }
            let pace = match s.pace.as_str() {
                "slow" => [8, 4, 2],
                "fast" => [2, 1, 1],
                _ => [4, 2, 1],
            };
            let mut every: i64 = if s.cut == "auto" {
                if e > 0.72 {
                    pace[2]
                } else if e > 0.4 {
                    pace[1]
                } else {
                    pace[0]
                }
            } else {
                s.cut.parse::<i64>().unwrap_or(1).max(1)
            };
            if self.scroll_dir != 0 {
                every = every.max(2);
            }
            let on_grid = match bars {
                Some(b) if 4 % every == 0 => {
                    (b.pos.get(k).copied().unwrap_or(0) as i64) % every == 0
                        && ki - self.last_cut >= every.min(2)
                }
                _ => ki - self.last_cut >= every,
            };
            let tri = self.layout == "tri" || self.layout == "trim";
            if self.revealing {
                // Reveal opening: one clip only, it just grows – no cuts until the drop
            } else if self.scroll_dir != 0 && tri {
                // The middle clip (group 1) runs for four bars; the sides scroll in turn
                if bar && ki - self.center_k.unwrap_or(self.layout_beat) >= 16 {
                    self.cut_group(1, t, 0);
                    self.center_k = Some(ki);
                    self.last_cut = ki;
                } else if on_grid {
                    let sides: &[usize] = if self.layout == "tri" { &[0, 2] } else { &[0] };
                    let g = sides[self.side_n % sides.len()];
                    self.side_n += 1;
                    self.cut_group(g, t, self.scroll_dir);
                    self.last_cut = ki;
                }
            } else if on_grid || (every == 4 && bar && ki - self.last_cut >= 2) {
                let n = layout(self.layout).groups;
                let mut gi: Option<usize> = Some(self.next_group % n);
                if min_b > 0 {
                    gi = None;
                    for j in 0..n {
                        let c = (self.next_group + j) % n;
                        if ki - self.cut_k[c] >= min_b {
                            gi = Some(c);
                            break;
                        }
                    }
                }
                if let Some(gi) = gi {
                    self.cut_group(gi, t, 0);
                    self.next_group = gi + 1;
                    self.last_cut = ki;
                    if e > 0.5 {
                        self.trig(
                            t,
                            Fx::Flash {
                                a: 0.15 + 0.3 * e,
                                color: if bar { [255, 62, 138] } else { [255, 255, 255] },
                                force: false,
                            },
                        );
                    }
                }
            }
        }

        // Longest clip: a field whose clip has been on for that long is cut now, whatever the pace says
        if max_b > 0 && !self.revealing && !self.tpl {
            let n = layout(self.layout).groups;
            for gi in 0..n {
                if ki - self.cut_k[gi] >= max_b {
                    self.cut_group(gi, t, 0);
                    self.last_cut = ki;
                    break;
                }
            }
        }

        if s.fx.speed {
            let r = if drop {
                2.0
            } else if e < 0.35 {
                0.6
            } else if e < 0.7 {
                1.0
            } else {
                1.3
            };
            self.set_rate(t, r, if drop { len * 2.0 } else { 0.0 });
        }
        if e > 0.7 {
            self.trig(
                t,
                Fx::Shake {
                    until: t + 0.1 + 0.1 * e,
                },
            );
        }
        if drop || (e > 0.85 && bar) {
            self.trig(
                t,
                Fx::Glitch {
                    until: t + if drop { 0.3 } else { 0.14 },
                },
            );
        }
        if drop {
            self.trig(
                t,
                Fx::Rgb {
                    amt: 26.0,
                    dur: 0.45,
                },
            );
        } else if e > 0.62 && (bar || ki == self.last_cut) {
            self.trig(
                t,
                Fx::Rgb {
                    amt: 6.0 + 12.0 * e,
                    dur: 0.14,
                },
            );
        }
        if drop {
            self.trig(
                t,
                Fx::Tunnel {
                    dur: (len * 2.0).min(1.2),
                },
            );
        }
        if drop || (e > 0.85 && k.is_multiple_of(8)) {
            self.trig(
                t,
                Fx::Invert {
                    until: t + len / 4.0,
                },
            );
        }
        if (drop || (bar && e > 0.6 && self.rng.gen::<f64>() < 0.45)) && !self.words.is_empty() {
            let w = self.words[self.rng.gen_range(0..self.words.len())].clone();
            self.trig(t, Fx::Text { word: w });
        }
        // Half beats in loud parts: stutter and strobe
        if let Some(nb) = beats.get(k + 1) {
            if e > 0.8 && !drop {
                let mid = (beats[k] + nb) / 2.0;
                if s.fx.stutter && k % 2 == 1 {
                    pending.push((mid, 2, 0.0));
                }
                if s.fx.strobe {
                    pending.push((mid, 3, 0.0));
                }
            }
        }
        if s.fx.strobe && e > 0.85 {
            self.trig(t, Fx::Strobe);
        }
        if (s.pic_flash || s.pic_overlay) && self.inp.n_images > 0 && !self.revealing {
            self.pic_beat(k, e, bar, len, pending);
        }
    }

    /// A template's event (port of applyEvent)
    fn apply_event(&mut self, ev: &TplEvent, t: f64) {
        let e = self.energy_now;
        match ev {
            TplEvent::Flash {
                strength, color, ..
            } => {
                self.trig(
                    t,
                    Fx::Flash {
                        a: strength.max(0.35),
                        color: *color,
                        force: true,
                    },
                );
                return;
            }
            TplEvent::Layout {
                layout: id, dir, ..
            } => {
                let l = layout(id).id;
                self.set_layout_dir(l, t, dir);
                if e > 0.6 {
                    self.trig(
                        t,
                        Fx::Rgb {
                            amt: 8.0 + 14.0 * e,
                            dur: 0.16,
                        },
                    );
                }
                if e > 0.8 {
                    self.trig(t, Fx::Glitch { until: t + 0.14 });
                }
            }
            TplEvent::Cut { groups: None, .. } => {
                let (l, d) = (self.layout, self.cur_dir);
                self.set_layout_dir(l, t, d);
            }
            TplEvent::Cut {
                groups: Some(gs), ..
            } => {
                let n = layout(self.layout).groups;
                let list: Vec<usize> = if gs.is_empty() {
                    vec![self.next_group % n]
                } else {
                    gs.iter().copied().filter(|&g| g < n).collect()
                };
                for &g in &list {
                    self.cut_group(g, t, 0);
                }
                self.next_group = list.last().copied().unwrap_or(0) + 1;
            }
        }
        if e > 0.65 {
            self.trig(
                t,
                Fx::Rgb {
                    amt: 5.0 + 10.0 * e,
                    dur: 0.12,
                },
            );
        }
    }

    // Queued events (half beats, pictures) whose time has come
    fn run_pending(&mut self, upto: f64, pending: &mut Vec<(f64, u8, f64)>) {
        pending.sort_by(|a, b| cmp_f64(a.0, b.0));
        while !pending.is_empty() && pending[0].0 <= upto {
            let (t, kind, dur) = pending.remove(0);
            match kind {
                0 => {
                    if let Some(pic) = self.take_pic() {
                        self.trig(t, Fx::PicFlash { t1: t + dur, pic });
                    }
                }
                1 => {
                    if let Some(pic) = self.take_pic() {
                        let alpha = (if self.inp.s.pic_overlay_amt > 0.0 {
                            self.inp.s.pic_overlay_amt
                        } else {
                            35.0
                        } / 100.0)
                            .clamp(0.1, 0.9);
                        self.trig(
                            t,
                            Fx::PicOver {
                                t1: t + dur,
                                alpha,
                                pic,
                            },
                        );
                    }
                }
                2 => {
                    // Stutter: all running clips jump back briefly; the pulse restarts
                    self.stutters.push(t);
                    if let Some(last) = self.pulses.last().cloned() {
                        self.pulses.push(Pulse {
                            t,
                            beat_t: t,
                            beat_amt: 0.05,
                            ..last
                        });
                    }
                }
                _ => self.trig(t, Fx::Strobe),
            }
        }
    }
}

// Reveal opening: where it ends – the first drop (not in the first 5 s, and within a minute), else after 32 beats
fn make_reveal(beats: &[f64], energy: &[f64]) -> (i64, f64) {
    let mut last: i64 = -99;
    let mut end_k = 32.min(beats.len() as i64 - 1);
    let mut k = 0usize;
    while k < beats.len() && beats[k] < 75.0 {
        let e = energy.get(k).copied().unwrap_or(0.0);
        let prev = if k >= 4 {
            energy.get(k - 4).copied().unwrap_or(0.0)
        } else {
            0.0
        };
        if e - prev > 0.35 && e > 0.6 && k as i64 - last >= 16 {
            last = k as i64;
            if beats[k] >= 5.0 {
                end_k = k as i64;
                break;
            }
        }
        k += 1;
    }
    (end_k, beats[end_k.max(0) as usize].max(2.0))
}

pub fn make(inp: &Input) -> Plan {
    let s = inp.s;
    let mut layouts: Vec<&'static str> = LAYOUTS
        .iter()
        .map(|l| l.id)
        .filter(|id| s.layouts.get(id))
        .collect();
    if layouts.is_empty() {
        layouts = vec!["full"];
    }
    let words: Vec<String> = s
        .words
        .split([',', ';', '\n'])
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty())
        .collect();
    let reveal =
        if s.reveal && inp.template.is_none() && inp.beats.len() > 16 && !inp.energy.is_empty() {
            Some(make_reveal(inp.beats, inp.energy))
        } else {
            None
        };
    let mut d = Dir {
        inp,
        rng: StdRng::seed_from_u64(inp.seed),
        layouts: layouts.clone(),
        layout: "full",
        dir: s.split.clone(),
        cur_dir: "cols",
        tpl: inp.template.is_some(),
        drops: Vec::new(),
        phases: Vec::new(),
        shots: Vec::new(),
        cur: Vec::new(),
        cut_k: Vec::new(),
        cut_t: Vec::new(),
        next_group: 0,
        layout_beat: 0,
        layout_hold: 8,
        last_drop: -99,
        last_cut: -99,
        center_k: None,
        scroll_dir: 0,
        side_n: 0,
        cur_k: 0,
        energy_now: 0.0,
        zoom_flip: false,
        fx: Vec::new(),
        pulses: Vec::new(),
        rates: Vec::new(),
        rate: 1.0,
        rate_hold: 0.0,
        stutters: Vec::new(),
        cut_times: Vec::new(),
        revealing: reveal.is_some(),
        reveal,
        reveal_until: f64::INFINITY,
        words,
        pic_order: Vec::new(),
        pic_spent: Vec::new(),
        flash_until: -1.0,
        over_until: -1.0,
    };
    // The first layout: a template's first, the reveal opens in fullscreen, else fullscreen if allowed, else
    // the first chosen
    let tpl_events: &[TplEvent] = inp.template.unwrap_or(&[]);
    let tpl_first = tpl_events.iter().enumerate().find_map(|(i, e)| match e {
        TplEvent::Layout { layout: l, dir, .. } => Some((i, layout(l).id, dir.clone())),
        _ => None,
    });
    // (the template's first layout is the opening one – it isn't set a second time when its event comes)
    let mut skip_ev = None;
    if let Some((i, l, dir)) = tpl_first {
        d.set_layout_dir(l, 0.0, &dir);
        skip_ev = Some(i);
    } else {
        let first = if reveal.is_some() || layouts.contains(&"full") {
            "full"
        } else {
            layouts[0]
        };
        d.set_layout(first, 0.0);
    }
    let lead = s.cut_lead.max(0.0) / 1000.0;
    let mut pending: Vec<(f64, u8, f64)> = Vec::new();
    let mut ti = 0usize; // next template event
    for k in 0..inp.beats.len() {
        let t = (inp.beats[k] - lead).max(0.0);
        if t >= inp.duration {
            break;
        }
        // template events before this beat (an event on the beat comes after it, like in the web version,
        // where a frame first runs the beats, then the events)
        while ti < tpl_events.len() && tpl_events[ti].t() < t {
            let et = tpl_events[ti].t().clamp(0.0, inp.duration);
            if skip_ev != Some(ti) {
                d.run_pending(et, &mut pending);
                d.apply_event(&tpl_events[ti], et);
            }
            ti += 1;
        }
        d.run_pending(t, &mut pending);
        d.on_beat(k, t, &mut pending);
        while ti < tpl_events.len() && tpl_events[ti].t() <= t {
            if skip_ev != Some(ti) {
                d.apply_event(&tpl_events[ti], t);
            }
            ti += 1;
        }
    }
    while ti < tpl_events.len() && tpl_events[ti].t() < inp.duration {
        let et = tpl_events[ti].t().max(0.0);
        if skip_ev != Some(ti) {
            d.run_pending(et, &mut pending);
            d.apply_event(&tpl_events[ti], et);
        }
        ti += 1;
    }
    d.run_pending(inp.duration, &mut pending);
    let end = inp.duration;
    for id in d.cur.clone() {
        d.close(id, end, 0.0);
    }
    for sh in d.shots.iter_mut() {
        if sh.t_off.is_infinite() {
            sh.t_off = end;
            sh.until = end;
        }
    }
    let reveal_out = d.reveal.map(|(_, end_t)| (d.reveal_until, end_t));
    let mut p = Plan {
        w: inp.w,
        h: inp.h,
        duration: inp.duration,
        seed: inp.seed,
        phases: d.phases,
        shots: d.shots,
        pulses: d.pulses,
        fx: d.fx,
        rates: d.rates,
        rate_cum: Vec::new(),
        stutters: d.stutters,
        cut_times: d.cut_times,
        reveal: reveal_out,
        drops: d.drops,
    };
    p.finish();
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_is_consistent() {
        let beats: Vec<f64> = (0..400).map(|i| i as f64 * 0.5).collect();
        let energy: Vec<f64> = (0..400)
            .map(|i| if (i / 32) % 2 == 0 { 0.2 } else { 0.9 })
            .collect();
        let mut st = Settings::default();
        st.fx.speed = true;
        st.fx.stutter = true;
        st.reveal = true;
        st.scroll = true;
        let ok = |_: &str| true;
        let p = make(&Input {
            beats: &beats,
            energy: &energy,
            duration: 200.0,
            bars: None,
            s: &st,
            w: 1280,
            h: 720,
            n_images: 0,
            layout_ok: &ok,
            seed: 7,
            template: None,
        });
        assert!(p.phases.len() > 3);
        assert!(p.shots.len() > 50);
        for (i, s) in p.shots.iter().enumerate() {
            assert_eq!(s.id, i);
            assert!(s.t_off >= s.t_on && s.until >= s.t_off, "{s:?}");
            assert!(s.group < p.phases[s.phase].groups);
        }
        // shots are in phase order
        assert!(p
            .shots
            .windows(2)
            .all(|w| w[0].phase <= w[1].phase && w[0].t_on <= w[1].t_on));
        // every group has a shot at any time
        for i in 0..400 {
            let t = i as f64 * 0.49;
            let (pi, cur, _) = p.shots_at(t);
            assert!(cur.iter().all(|c| c.is_some()), "t {t} phase {pi}");
        }
        // clock is monotonic between stutters and never negative
        let s = &p.shots[10];
        assert!(p.shot_clock(s, s.t_on) == 0.0);
        assert!(p.rate_integral(10.0) > 0.0);
        assert!(p.validate().is_ok());
        assert!(!p.drops.is_empty(), "the loud parts start with drops");

        // the same seed → the same plan; saved and loaded → the same clock
        let again = make(&Input {
            beats: &beats,
            energy: &energy,
            duration: 200.0,
            bars: None,
            s: &st,
            w: 1280,
            h: 720,
            n_images: 0,
            layout_ok: &ok,
            seed: 7,
            template: None,
        });
        assert_eq!(again.shots.len(), p.shots.len());
        assert!(again
            .shots
            .iter()
            .zip(&p.shots)
            .all(|(a, b)| a.t_on == b.t_on && a.group == b.group));
        let json = serde_json::to_string(&p).unwrap();
        let mut back: Plan = serde_json::from_str(&json).unwrap();
        back.finish();
        assert!(back.validate().is_ok());
        for t in [3.0, 50.5, 120.25] {
            assert_eq!(back.rate_integral(t), p.rate_integral(t));
        }

        // smaller picture: same structure, fields scaled
        let small = p.resized(640, 360);
        assert_eq!(small.shots.len(), p.shots.len());
        for (a, b) in small.phases.iter().zip(&p.phases) {
            assert_eq!(a.layout, b.layout);
            assert_eq!(a.slots.len(), b.slots.len());
            assert!(a.slots.iter().all(|s| s.x + s.w <= 640 && s.y + s.h <= 360));
        }
    }

    #[test]
    fn template_drives_the_cuts() {
        let beats: Vec<f64> = (0..120).map(|i| i as f64 * 0.5).collect();
        let energy = vec![0.5; 120];
        let st = Settings::default();
        let ok = |_: &str| true;
        let ev = vec![
            TplEvent::Layout {
                t: 0.0,
                layout: "duo".into(),
                dir: "rows".into(),
            },
            TplEvent::Cut {
                t: 2.0,
                groups: Some(vec![1]),
            },
            TplEvent::Flash {
                t: 3.0,
                strength: 0.2,
                color: [255, 255, 255],
            },
            TplEvent::Layout {
                t: 10.0,
                layout: "tri".into(),
                dir: "cols".into(),
            },
            TplEvent::Cut {
                t: 12.0,
                groups: None,
            },
        ];
        let mut no_flash = st.clone();
        no_flash.fx.flash = false;
        let p = make(&Input {
            beats: &beats,
            energy: &energy,
            duration: 60.0,
            bars: None,
            s: &no_flash,
            w: 1280,
            h: 720,
            n_images: 0,
            layout_ok: &ok,
            seed: 1,
            template: Some(&ev),
        });
        assert!(p.validate().is_ok());
        // only the template's layouts, in its directions
        let lays: Vec<(&str, &str)> = p
            .phases
            .iter()
            .map(|ph| (ph.layout.as_str(), ph.dir.as_str()))
            .collect();
        assert_eq!(
            lays,
            vec![("duo", "rows"), ("tri", "cols"), ("tri", "cols")]
        );
        // the cut at 2 s changes field 1 only; no other cuts in between
        let in_first: Vec<&Shot> = p.shots.iter().filter(|s| s.phase == 0).collect();
        assert_eq!(in_first.len(), 3);
        assert_eq!(in_first[2].group, 1);
        assert_eq!(in_first[2].t_on, 2.0);
        // the template's flash comes even with the flash effect off
        assert!(p
            .fx
            .iter()
            .any(|e| matches!(e.fx, Fx::Flash { force: true, .. }) && e.t == 3.0));
        assert!(!p
            .fx
            .iter()
            .any(|e| matches!(e.fx, Fx::Flash { force: false, .. })));
    }
}
