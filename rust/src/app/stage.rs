// Playing a session: the song through the sound card (with the clips' own sound mixed in), the clips decoded
// a few seconds ahead, and the frame drawn for whatever the song's position is. Used by the live show and by the
// studio's preview. Jumping somewhere (the timeline) rebuilds the clip side at that moment – sound and session
// stay.

use crate::clipaudio::ClipMix;
use crate::library::Item;
use crate::media::Pick;
use crate::player::Player;
use crate::project::Session;
use crate::show::{self, BuildOpts, Built, Pins};
use crate::util::LockExt;
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Stage {
    pub sess: Session,
    built: Arc<Mutex<Built>>,
    pub player: Player,
    pub mix: Arc<ClipMix>,
    /// the clips' scheduler of the current build
    run: Arc<AtomicBool>,
    /// the clips on screen are there (after the start or a jump)
    pub ready: Arc<AtomicBool>,
    pub tex: Option<egui::TextureHandle>,
    pub short: f64,
    pins: Pins,
    items: Vec<Item>,
    last_t: f64,
    dirty: bool,
    /// keep redrawing until then (a clip that was just swapped decodes its first frame)
    redraw_until: Option<Instant>,
    /// play as soon as the clips are ready
    pub want_play: bool,
    /// frames drawn per second (for the clip info)
    pub fps: (Instant, u32, f32),
    pub size: (u32, u32),
}

fn start(
    sess: &Session,
    short: f64,
    pins: &Pins,
    items: &[Item],
    mix: &Arc<ClipMix>,
    t: f64,
) -> (Arc<Mutex<Built>>, Arc<AtomicBool>, Arc<AtomicBool>) {
    let built = show::build(
        sess,
        BuildOpts {
            short,
            export: false,
            fps: 30.0,
            pins: pins.clone(),
            items: items.to_vec(),
        },
    );
    mix.clear();
    show::hook_clip_audio(&built.media, mix, sess.song.pcm.rate);
    let media = built.media.clone();
    let built = Arc::new(Mutex::new(built));
    let run = Arc::new(AtomicBool::new(true));
    let ready = Arc::new(AtomicBool::new(false));
    let (r2, ready2) = (run.clone(), ready.clone());
    std::thread::spawn(move || {
        // first the clips for the moment we start at (at most 20 s, like the browser version) …
        show::preroll(&media, t, Instant::now() + Duration::from_secs(20));
        ready2.store(true, Ordering::SeqCst);
        // … then keep them coming
        while r2.load(Ordering::SeqCst) && !media.stopped() {
            media.tick(media.clock.get());
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    (built, run, ready)
}

impl Stage {
    /// items: clips to choose from for shots without one (a live show); empty for a generated session
    pub fn new(sess: Session, short: f64, pins: Pins, items: Vec<Item>, at: f64) -> Stage {
        let s = &sess.settings;
        let mix = ClipMix::new((s.clip_vol / 100.0) as f32);
        let player = Player::new(
            sess.song.pcm.clone(),
            (s.song_vol / 100.0) as f32,
            Some(mix.clone()),
        );
        let at = at.clamp(0.0, sess.plan.duration);
        player.seek(at);
        let (built, run, ready) = start(&sess, short, &pins, &items, &mix, at);
        let size = {
            let b = built.lk();
            (b.plan.w as u32, b.plan.h as u32)
        };
        Stage {
            sess,
            built,
            player,
            mix,
            run,
            ready,
            tex: None,
            short,
            pins,
            items,
            last_t: -1.0,
            dirty: true,
            redraw_until: None,
            want_play: false,
            fps: (Instant::now(), 0, 0.0),
            size,
        }
    }

    pub fn duration(&self) -> f64 {
        self.sess.plan.duration
    }
    pub fn time(&self) -> f64 {
        self.player.time().clamp(0.0, self.duration())
    }
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
    pub fn playing(&self) -> bool {
        self.player.playing()
    }

    pub fn play(&mut self) {
        if self.is_ready() {
            if self.player.ended() {
                self.seek(0.0);
                self.want_play = true;
                return;
            }
            self.player.play();
        } else {
            self.want_play = true;
        }
    }
    pub fn pause(&mut self) {
        self.want_play = false;
        self.player.pause();
    }
    pub fn toggle(&mut self) {
        if self.playing() || self.want_play {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Keep the picks a live show chose (so a jump or a save keeps them)
    fn keep_picks(&mut self) {
        let p = self.built.lk().media.picks();
        if p.len() == self.sess.picks.len() {
            for (dst, src) in self.sess.picks.iter_mut().zip(p) {
                if dst.is_none() {
                    *dst = src;
                }
            }
        }
    }

    fn rebuild(&mut self, at: f64) {
        self.keep_picks();
        self.run.store(false, Ordering::SeqCst);
        self.built.lk().media.stop();
        let (built, run, ready) = start(
            &self.sess,
            self.short,
            &self.pins,
            &self.items,
            &self.mix,
            at,
        );
        self.built = built;
        self.run = run;
        self.ready = ready;
        self.poke();
    }

    /// Jump to t (playing on there when it was playing)
    pub fn seek(&mut self, t: f64) {
        let t = t.clamp(0.0, (self.duration() - 0.05).max(0.0));
        let was = self.playing() || self.want_play;
        self.player.pause();
        self.player.seek(t);
        self.rebuild(t);
        self.want_play = was;
    }

    /// Other clips for some shots (the timeline): switched where they are on screen, kept in the session
    pub fn set_picks(&mut self, changed: &[(usize, Pick)]) {
        let media = self.built.lk().media.clone();
        for (i, p) in changed {
            if let Some(slot) = self.sess.picks.get_mut(*i) {
                *slot = Some(p.clone());
                media.repick(*i, p.clone());
            }
        }
        self.poke();
    }

    /// Draw again for a moment (also while paused) – new clips show up as soon as they are decoded
    pub fn poke(&mut self) {
        self.dirty = true;
        self.redraw_until = Some(Instant::now() + Duration::from_millis(1500));
    }

    /// The whole set of picks (undo)
    pub fn replace_picks(&mut self, picks: Vec<Option<Pick>>) {
        let t = self.time();
        self.sess.picks = picks;
        let was = self.playing();
        self.player.pause();
        self.rebuild(t);
        self.want_play = was;
    }

    pub fn set_volumes(&mut self, song: f64, clips: f64) {
        self.player.set_volume((song / 100.0) as f32);
        self.mix.set_volume((clips / 100.0) as f32);
        self.sess.settings.song_vol = song;
        self.sess.settings.clip_vol = clips;
    }

    /// The session with every pick the show has made so far
    pub fn session(&mut self) -> Session {
        self.keep_picks();
        self.sess.clone()
    }

    /// Per frame: start when ready, keep the clips' clock with the song, draw the picture if it changed
    pub fn update(&mut self, ctx: &egui::Context) {
        if self.want_play && self.is_ready() {
            self.want_play = false;
            if !self.player.playing() {
                self.player.play();
            }
        }
        let t = self.time();
        let b = self.built.lk().media.clone();
        b.clock.set(t);
        self.mix.prune((t * self.sess.song.pcm.rate as f64) as u64);
        let redraw = self.redraw_until.is_some_and(|u| Instant::now() < u);
        if redraw {
            ctx.request_repaint_after(Duration::from_millis(30));
        } else {
            self.redraw_until = None;
        }
        if self.is_ready()
            && ((t - self.last_t).abs() > 1e-4 || self.dirty || redraw || self.tex.is_none())
        {
            self.dirty = false;
            self.last_t = t;
            let mut built = self.built.lk();
            let frame = built.renderer.frame(t);
            let img = egui::ColorImage::from_rgba_premultiplied(
                [frame.width() as usize, frame.height() as usize],
                frame.data(),
            );
            self.size = (frame.width(), frame.height());
            drop(built);
            let opts = egui::TextureOptions::LINEAR;
            match &mut self.tex {
                Some(tx) => tx.set(img, opts),
                None => self.tex = Some(ctx.load_texture("pmv-stage", img, opts)),
            }
            let (t0, n, _) = self.fps;
            if t0.elapsed() >= Duration::from_secs(1) {
                self.fps = (Instant::now(), 0, n as f32 / t0.elapsed().as_secs_f32());
            } else {
                self.fps.1 += 1;
            }
        }
    }

    /// The clip info (I): (field, clip, state) and the clips left out
    pub fn info(&self) -> (Vec<(usize, String, String)>, Vec<String>) {
        let b = self.built.lk();
        let t = self.time();
        let fails = b
            .media
            .failures
            .lk()
            .iter()
            .map(|f| format!("{} – {}", f.name, f.why))
            .collect();
        (b.renderer.info(t), fails)
    }

    /// The shots on screen at t: (field number, shot)
    pub fn on_screen(&self, t: f64) -> Vec<(usize, usize)> {
        let (_, cur, _) = self.sess.plan.shots_at(t);
        let mut seen = std::collections::HashSet::new();
        cur.iter()
            .enumerate()
            .filter_map(|(g, s)| s.map(|s| (g + 1, s)))
            .filter(|(_, s)| seen.insert(*s))
            .collect()
    }

    /// The pick of a shot as the show has it (a live show's own choices too)
    pub fn pick(&self, shot: usize) -> Option<Pick> {
        self.sess
            .picks
            .get(shot)
            .cloned()
            .flatten()
            .or_else(|| self.built.lk().media.picks().get(shot).cloned().flatten())
    }

    pub fn shutdown(&mut self) {
        self.run.store(false, Ordering::SeqCst);
        self.player.pause();
        self.built.lk().media.stop();
        self.mix.clear();
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.shutdown();
    }
}
