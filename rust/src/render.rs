// One frame of the show at show time t: the plan says what is on screen, the media give the pictures,
// the compositor draws them. Used by the live show and by the export alike.

use crate::compositor::{Compositor, FrameIn, Leave, Tile, Triggers};
use crate::library::Kind;
use crate::media::{Assigned, Media};
use crate::plan::{Fx, Plan};
use crate::smart;
use crate::util::LockExt;
use std::sync::Arc;
use std::time::Duration;
use tiny_skia::Pixmap;

/// A clip on screen with its frame and when it came on
type Kept = (Arc<Assigned>, Arc<Pixmap>, f64);
/// … and one scrolling out (+ direction)
type KeptLeave = (Arc<Assigned>, Arc<Pixmap>, f64, i32);

pub struct Renderer {
    pub plan: Arc<Plan>,
    pub media: Arc<Media>,
    pub comp: Compositor,
    fx_idx: usize,
    trig: Triggers,
    pic_flash: Option<(usize, f64, f64)>,
    pic_over: Option<(usize, f64, f64, f64)>,
    pub title: String,
    pub bpm: f64,
    pub wait: bool, // export: wait for every frame
    last_t: f64,
    pub draw_time: f64,
}

/// A tiny copy of a frame (nearest pixels) for the focus / brightness measurement
fn tiny_sig(p: &Pixmap, w: f64, h: f64) -> smart::Sig {
    let (tw, th) = smart::tiny(w as u32, h as u32);
    let (pw, ph) = (p.width() as usize, p.height() as usize);
    let mut px = vec![0u8; (tw * th * 4) as usize];
    let d = p.data();
    for y in 0..th as usize {
        let sy = (y * ph / th as usize).min(ph - 1);
        for x in 0..tw as usize {
            let sx = (x * pw / tw as usize).min(pw - 1);
            let i = (sy * pw + sx) * 4;
            let o = (y * tw as usize + x) * 4;
            px[o..o + 4].copy_from_slice(&d[i..i + 4]);
        }
    }
    smart::measure(&px, tw as usize, th as usize, 4)
}

impl Renderer {
    pub fn new(
        plan: Arc<Plan>,
        media: Arc<Media>,
        title: String,
        bpm: f64,
        wait: bool,
    ) -> Renderer {
        let comp = Compositor::new(plan.w as u32, plan.h as u32, &media.s, plan.seed);
        Renderer {
            plan,
            media,
            comp,
            fx_idx: 0,
            trig: Triggers::default(),
            pic_flash: None,
            pic_over: None,
            title,
            bpm,
            wait,
            last_t: -1.0,
            draw_time: 0.0,
        }
    }

    fn advance_fx(&mut self, t: f64) {
        if t < self.last_t {
            // (went back: start over)
            self.fx_idx = 0;
            self.trig = Triggers::default();
            self.pic_flash = None;
            self.pic_over = None;
        }
        self.last_t = t;
        let fx = &self.plan.fx;
        while self.fx_idx < fx.len() && fx[self.fx_idx].t <= t {
            let ev = &fx[self.fx_idx];
            let et = ev.t;
            match &ev.fx {
                Fx::Flash { a, color, .. } => self.trig.flash = Some((et, *a, *color)),
                Fx::Shake { until } => self.trig.shake_until = *until,
                Fx::Glitch { until } => self.trig.glitch_until = *until,
                Fx::Rgb { amt, dur } => self.trig.rgb = Some((et, *amt, *dur)),
                Fx::Invert { until } => self.trig.invert_until = *until,
                Fx::Tunnel { dur } => self.trig.tunnel = Some((et, *dur)),
                Fx::Strobe => self.trig.strobe = Some(et),
                Fx::Text { word } => self.trig.text = Some((et, word.clone())),
                Fx::PicFlash { t1, pic } => self.pic_flash = Some((*pic, et, *t1)),
                Fx::PicOver { t1, alpha, pic } => self.pic_over = Some((*pic, et, *t1, *alpha)),
            }
            self.fx_idx += 1;
        }
    }

    fn frame_of(&self, a: &Assigned, t: f64) -> Option<Arc<Pixmap>> {
        let shot = &self.plan.shots[a.shot];
        let idx = if a.pick.kind == Kind::Image {
            0
        } else {
            a.index_at(&self.plan, shot, t)
        };
        let wait = if self.wait {
            Some(Duration::from_secs(60))
        } else {
            None
        };
        a.frames.get(idx, wait)
    }

    fn track(&self, a: &Assigned, pix: &Pixmap, t: f64) {
        let mut l = a.live.lk();
        if t - l.measured_t > 0.4 || t < l.measured_t {
            l.measured_t = t;
            let s = tiny_sig(pix, a.pick.w as f64, a.pick.h as f64);
            if self.media.s.smart_crop {
                l.target = s.focus;
            }
            l.lum = Some(match l.lum {
                Some(v) => v + (s.lum - v) * 0.3,
                None => s.lum,
            });
        }
        if self.media.s.smart_crop {
            let tg = l.target;
            l.focus.0 += (tg.0 - l.focus.0) * 0.06;
            l.focus.1 += (tg.1 - l.focus.1) * 0.06;
        }
    }

    /// What is on screen at t – for the info overlay: (field, clip name, state)
    pub fn info(&self, t: f64) -> Vec<(usize, String, String)> {
        let (_, cur, _) = self.plan.shots_at(t);
        let a = self.media.assigned.lk();
        cur.iter()
            .enumerate()
            .map(|(g, id)| match id.and_then(|i| a.get(&i)) {
                Some(x) => {
                    let st = if let Some(e) = x.frames.failed.lk().clone() {
                        format!("failed: {e}")
                    } else if !x.frames.any() && x.pick.kind == Kind::Video {
                        "loading".into()
                    } else {
                        format!(
                            "{}{}",
                            if x.pick.kind == Kind::Image {
                                "image"
                            } else {
                                "playing"
                            },
                            if x.smart { "" } else { " (quick pick)" }
                        )
                    };
                    (g + 1, x.pick.name.clone(), st)
                }
                None => (g + 1, "—".into(), "no clip yet".into()),
            })
            .collect()
    }

    pub fn frame(&mut self, t: f64) -> &Pixmap {
        self.advance_fx(t);
        let plan = self.plan.clone();
        let (pi, cur, leave) = plan.shots_at(t);
        let ph = &plan.phases[pi];
        let assigned = self.media.assigned.lk().clone();
        // per group: the clip and its frame (a shot whose clip isn't there yet shows the one before it)
        let mut keep: Vec<Option<Kept>> = Vec::new();
        for id in cur.iter() {
            let mut got = None;
            let mut sid = *id;
            let mut hops = 0;
            while let Some(i) = sid {
                if let Some(a) = assigned.get(&i) {
                    if let Some(p) = self.frame_of(a, t) {
                        let cut_t = plan.shots[id.unwrap_or(i)].t_on;
                        got = Some((a.clone(), p, cut_t));
                        break;
                    }
                }
                sid = plan.shots[i].prev;
                hops += 1;
                if hops > 3 {
                    break;
                }
            }
            keep.push(got);
        }
        let mut leave_keep: Vec<Option<KeptLeave>> = Vec::new();
        for lv in leave.iter() {
            leave_keep.push(lv.and_then(|(pid, lt, dir)| {
                let a = assigned.get(&pid)?;
                let p = self.frame_of(a, t)?;
                Some((a.clone(), p, lt, dir))
            }));
        }
        for (a, p, _) in keep.iter().flatten() {
            self.track(a, p, t);
        }
        let smart_crop = self.media.s.smart_crop;
        fn tile<'a>(
            plan: &Plan,
            a: &Arc<Assigned>,
            p: &'a Arc<Pixmap>,
            cut_t: f64,
            t: f64,
            smart_crop: bool,
        ) -> Tile<'a> {
            let l = a.live.lk();
            let shot = &plan.shots[a.shot];
            Tile {
                pix: p.as_ref(),
                w: a.pick.w as f64,
                h: a.pick.h as f64,
                image: a.pick.kind == Kind::Image,
                since: t - cut_t,
                focus: if smart_crop { Some(l.focus) } else { None },
                lum: l.lum,
                kb: a.pick.kb,
                zoom_dir: shot.zoom_dir,
            }
        }
        let groups: Vec<Option<Tile>> = keep
            .iter()
            .map(|x| {
                x.as_ref()
                    .map(|(a, p, ct)| tile(&plan, a, p, *ct, t, smart_crop))
            })
            .collect();
        let leaves: Vec<Option<Leave>> = leave_keep
            .iter()
            .map(|x| {
                x.as_ref().map(|(a, p, lt, dir)| Leave {
                    tile: tile(&plan, a, p, plan.shots[a.shot].t_on, t, smart_crop),
                    t: *lt,
                    dir: *dir,
                })
            })
            .collect();
        let pulse = plan.pulse_at(t).cloned();
        let (pw, phh) = (plan.w as u32, plan.h as u32);
        let pf = self.pic_flash.and_then(|(i, t0, t1)| {
            let it = self.media.images.get(i)?;
            let p = self.media.picture(i, pw, phh, true)?;
            Some((p, it.w as f64, it.h as f64, t0, t1))
        });
        let po = self.pic_over.and_then(|(i, t0, t1, al)| {
            let it = self.media.images.get(i)?;
            let p = self.media.picture(i, pw, phh, false)?;
            Some((p, it.w as f64, it.h as f64, t0, t1, al))
        });
        let reveal = plan.reveal.and_then(|(until, end_t)| {
            if t < until {
                Some((t / end_t).clamp(0.0, 1.0))
            } else {
                None
            }
        });
        let credits = format!(
            "{} Clips · {} BPM",
            self.media.shown_count(),
            self.bpm.round() as i64
        );
        let st = FrameIn {
            t,
            slots: &ph.slots,
            groups,
            leave: leaves,
            energy: pulse.as_ref().map(|p| p.energy).unwrap_or(0.0),
            beat_t: pulse.as_ref().map(|p| p.beat_t).unwrap_or(-9.0),
            beat_amt: pulse.as_ref().map(|p| p.beat_amt).unwrap_or(0.0),
            next_t: pulse.as_ref().and_then(|p| p.next_t),
            next_amt: pulse.as_ref().map(|p| p.next_amt).unwrap_or(0.0),
            cut_count: plan.cuts_before(t),
            reveal,
            trig: self.trig.clone(),
            pic_flash: pf.as_ref().map(|(p, w, h, a, b)| (&**p, *w, *h, *a, *b)),
            pic_over: po
                .as_ref()
                .map(|(p, w, h, a, b, al)| (&**p, *w, *h, *a, *b, *al)),
            song_start: 0.0,
            duration: plan.duration,
            title: self.title.clone(),
            credits,
        };
        // (st borrows the pixmaps kept alive in keep / leave_keep / pf / po)
        let t0 = std::time::Instant::now();
        self.comp.draw(&st);
        self.draw_time += t0.elapsed().as_secs_f64();
        &self.comp.canvas
    }
}
