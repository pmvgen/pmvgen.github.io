// The studio: a generated PMV before it is rendered – the preview (with sound), a visual timeline of every shot
// (layouts, clips with thumbnails, drops), swapping the clips of one or more shots (automatically like the
// generator, at random, or by hand, from the sources you choose), changing a clip's moment, undo / redo, saving
// the project and exporting the video.

use super::live::{icon_btn, moments_ui, panel_frame, Live, Moments};
use super::stage::Stage;
use super::theme::*;
use super::{App, Job};
use crate::export::{self, Progress};
use crate::library::{Item, Kind};
use crate::media::{self, Media, MediaOpts, Pick};
use crate::project::{self, Session};
use crate::settings::{self, Settings};
use crate::template::layout_color;
use crate::util::{fmt_at, fmt_time, LockExt};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub enum Next {
    Stay,
    Setup,
    Live(Box<Live>),
}

/// Where swapped clips come from
#[derive(Clone, PartialEq, Debug)]
enum Source {
    /// the clips of the library that match the filters
    Library,
    /// one folder of the library
    Folder(PathBuf),
    /// the clips this PMV already uses
    Used,
}

struct ExportRun {
    prog: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<PathBuf, String>>>>,
    started: Instant,
}

/// Thumbnails of the shots (decoded in the background, tiny)
struct Thumbs {
    tex: HashMap<(PathBuf, i64), Option<egui::TextureHandle>>,
    tx: mpsc::Sender<(PathBuf, i64, f64, bool)>,
    rx: mpsc::Receiver<((PathBuf, i64), Option<egui::ColorImage>)>,
}
const THUMB: (u32, u32) = (96, 54);
impl Thumbs {
    fn new() -> Thumbs {
        let (tx, req_rx) = mpsc::channel::<(PathBuf, i64, f64, bool)>();
        let (res_tx, rx) = mpsc::channel();
        let req_rx = Arc::new(Mutex::new(req_rx));
        for _ in 0..3 {
            let (req_rx, res_tx) = (req_rx.clone(), res_tx.clone());
            std::thread::spawn(move || loop {
                let job = req_rx.lk().recv();
                let Ok((path, key, at, image)) = job else {
                    return;
                };
                let img = crate::ff::decode_rgba(&path, (!image).then_some(at), THUMB.0, THUMB.1)
                    .ok()
                    .map(|v| {
                        egui::ColorImage::from_rgba_unmultiplied(
                            [THUMB.0 as usize, THUMB.1 as usize],
                            &v,
                        )
                    });
                if res_tx.send(((path, key), img)).is_err() {
                    return;
                }
            });
        }
        Thumbs {
            tex: HashMap::new(),
            tx,
            rx,
        }
    }
    fn key(p: &Pick) -> (PathBuf, i64) {
        (p.path.clone(), (p.start * 10.0).round() as i64)
    }
    fn get(&mut self, ctx: &egui::Context, p: &Pick) -> Option<egui::TextureId> {
        while let Ok((k, img)) = self.rx.try_recv() {
            let t = img.map(|i| {
                ctx.load_texture(
                    format!("thumb-{}-{}", k.0.display(), k.1),
                    i,
                    egui::TextureOptions::LINEAR,
                )
            });
            self.tex.insert(k, t);
        }
        let k = Self::key(p);
        match self.tex.get(&k) {
            Some(Some(t)) => Some(t.id()),
            Some(None) => None,
            None => {
                self.tex.insert(k.clone(), None);
                let _ = self
                    .tx
                    .send((k.0, k.1, p.start + 0.3, p.kind == Kind::Image));
                None
            }
        }
    }
}

pub struct Studio {
    pub stage: Stage,
    undo: Vec<Vec<Option<Pick>>>,
    redo: Vec<Vec<Option<Pick>>>,
    sel: BTreeSet<usize>,
    anchor: Option<usize>,
    zoom: f32,
    scroll: f64,
    band: Option<(Pos2, Pos2)>,
    thumbs: Thumbs,
    source: Source,
    clip_q: String,
    manual: Option<(Item, f64)>,
    swap: Option<Job<Vec<(usize, Pick)>>>,
    fill: Option<Job<Vec<Option<Pick>>>>,
    export: Option<ExportRun>,
    last_export: Option<PathBuf>,
    moments: Moments,
    path: Option<PathBuf>,
    dirty: bool,
    applied: String,
    follow: bool,
    /// developer check (PMVGEN_SHOT_ACTION): an action the studio does by itself once it is ready
    script: Option<String>,
}

/// The settings that only change how the plan is drawn and heard – they follow the window's settings after
/// generating (cut timing, layouts and triggered effects stay as generated)
fn render_settings(base: &Settings, from: &Settings) -> Settings {
    let mut s = base.clone();
    s.look = from.look.clone();
    s.look_amt = from.look_amt;
    s.look_color = from.look_color.clone();
    s.bright = from.bright;
    s.look_even = from.look_even;
    s.smooth = from.smooth;
    s.soft = from.soft;
    s.soft_amt = from.soft_amt;
    s.div_w = from.div_w;
    s.edge = from.edge.clone();
    s.edge_amt = from.edge_amt;
    s.fit = from.fit.clone();
    s.smart_crop = from.smart_crop;
    s.pulse_amt = from.pulse_amt;
    for k in [
        "zoom", "hue", "vhs", "kenburns", "lines", "echo", "whip", "zoomin", "voice",
    ] {
        *s.fx.get_mut(k) = *from.fx.clone().get_mut(k);
    }
    s.voice_mode = from.voice_mode.clone();
    s.song_vol = from.song_vol;
    s.clip_vol = from.clip_vol;
    s.intro = from.intro;
    s.outro = from.outro;
    s.title = from.title.clone();
    s.font = from.font.clone();
    s.quality = from.quality;
    s.export_fps = from.export_fps;
    s.export_crf = from.export_crf;
    s.codec = from.codec.clone();
    s.encoder = from.encoder.clone();
    s.enc_speed = from.enc_speed.clone();
    s.hwaccel = from.hwaccel;
    s.show_quality = from.show_quality;
    s.fs_on = from.fs_on;
    s.fs_pace = from.fs_pace.clone();
    s.fs_size = from.fs_size.clone();
    s.fs_where = from.fs_where.clone();
    s.fs_style = from.fs_style.clone();
    s.fs_rests = from.fs_rests;
    s.fs_accent = from.fs_accent;
    s.fs_speed = from.fs_speed;
    s
}

fn preview_short(s: &Settings) -> f64 {
    s.show_quality.min(720.0)
}

/// A pleasant colour per clip (pink–violet range, from its path)
fn clip_color(p: &std::path::Path) -> Color32 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    let v = h.finish();
    let hue = 0.78 + (v % 1000) as f32 / 1000.0 * 0.35; // violet … pink … orange
    let c = egui::ecolor::Hsva::new(hue.fract(), 0.45, 0.55, 1.0);
    Color32::from(c)
}

impl Studio {
    pub fn new(mut sess: Session, app: &App) -> Studio {
        sess.settings = render_settings(&sess.settings, &app.s);
        let applied = serde_json::to_string(&sess.settings).unwrap_or_default();
        let complete = sess.complete();
        let mut st = Studio {
            stage: Stage::new(sess, preview_short(&app.s), app.pins(), Vec::new(), 0.0),
            undo: Vec::new(),
            redo: Vec::new(),
            sel: BTreeSet::new(),
            anchor: None,
            zoom: 1.0,
            scroll: 0.0,
            band: None,
            thumbs: Thumbs::new(),
            source: Source::Library,
            clip_q: String::new(),
            manual: None,
            swap: None,
            fill: None,
            export: None,
            last_export: None,
            moments: Moments::default(),
            path: None,
            dirty: true,
            applied,
            follow: true,
            script: std::env::var("PMVGEN_SHOT_ACTION").ok(),
        };
        if !complete {
            st.fill_missing(app);
        }
        // (developer screenshots: a selection to show)
        if let Ok(v) = std::env::var("PMVGEN_SHOT_SELECT") {
            st.sel = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        }
        st
    }

    pub fn pause(&mut self) {
        self.stage.pause();
    }
    pub fn close(&mut self) {
        if let Some(e) = &self.export {
            e.cancel.store(true, Ordering::SeqCst);
        }
        if let Some(j) = &self.swap {
            j.cancel.store(true, Ordering::SeqCst);
        }
        self.stage.shutdown();
    }

    /// Shots a live show didn't reach get their clips like the generator would
    fn fill_missing(&mut self, app: &App) {
        let sess = self.stage.session();
        let (items, images) = app.clip_lists();
        let items = if items.is_empty() {
            used_items(&sess)
        } else {
            items
        };
        let pins = app.pins();
        let s = sess.settings.clone();
        let plan = sess.plan.clone();
        let picks = sess.picks.clone();
        let pace = crate::show::pace_of(&sess.song);
        let bpm = sess.song.a.bpm;
        let _ = images;
        let imgs = sess.images.clone();
        self.fill = Some(Job::spawn(move |report, cancel| {
            let m = Media::new(
                &s,
                plan,
                items,
                imgs,
                MediaOpts {
                    export: false,
                    decode: false,
                    fps: 30.0,
                    picks: Some(picks),
                    pins,
                    pace: Some(pace),
                    bpm,
                },
            );
            Ok(m.resolve(
                &|d, t| {
                    report(
                        d as f32 / t.max(1) as f32,
                        format!("Choosing the missing clips … {d} / {t}"),
                    )
                },
                cancel,
            ))
        }));
    }

    fn push_undo(&mut self) {
        self.undo.push(self.stage.sess.picks.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }
    fn do_undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(self.stage.sess.picks.clone());
            self.stage.replace_picks(p);
            self.dirty = true;
        }
    }
    fn do_redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(self.stage.sess.picks.clone());
            self.stage.replace_picks(p);
            self.dirty = true;
        }
    }

    /// The clips swaps may come from
    fn sources(&self, app: &App) -> Vec<Item> {
        match &self.source {
            Source::Used => used_items(&self.stage.sess),
            Source::Library => {
                let (items, _) = app.clip_lists();
                if items.is_empty() {
                    used_items(&self.stage.sess)
                } else {
                    items
                }
            }
            Source::Folder(f) => {
                let (items, _) = app.clip_lists();
                items.into_iter().filter(|i| i.root == *f).collect()
            }
        }
    }

    fn start_swap(&mut self, app: &mut App, how: &str) {
        if self.sel.is_empty() || self.swap.is_some() {
            return;
        }
        let sources = self.sources(app);
        if sources.is_empty() && how != "manual" {
            app.toast(
                "No clips to take from – add clips in step 2 or choose other sources",
                false,
            );
            return;
        }
        let shots: Vec<usize> = self.sel.iter().copied().collect();
        let sess = self.stage.session();
        match how {
            "auto" => {
                let pins = app.pins();
                self.swap = Some(Job::spawn(move |report, cancel| {
                    Ok(crate::show::swap_auto(
                        &sess,
                        &shots,
                        sources,
                        pins,
                        &|f| report(f, "Choosing clips …".into()),
                        cancel,
                    ))
                }));
            }
            "random" => {
                self.swap = Some(Job::spawn(move |report, cancel| {
                    crate::show::swap_random(
                        &sess,
                        &shots,
                        &sources,
                        &|f| report(f, "Choosing clips at random …".into()),
                        cancel,
                    )
                }));
            }
            _ => {
                let Some((it, start)) = self.manual.clone() else {
                    return;
                };
                self.swap = Some(Job::spawn(move |_, _| {
                    let p = media::measured_pick(&it, start);
                    Ok(shots.iter().map(|&i| (i, p.clone())).collect())
                }));
            }
        }
    }

    fn poll(&mut self, app: &mut App) {
        if let Some(r) = self.swap.as_ref().and_then(|j| j.take()) {
            self.swap = None;
            match r {
                Ok(ch) if !ch.is_empty() => {
                    self.push_undo();
                    let n = ch.len();
                    self.stage.set_picks(&ch);
                    app.toast(
                        format!("{n} {} swapped", if n == 1 { "clip" } else { "clips" }),
                        true,
                    );
                }
                Ok(_) => app.toast("No clip could be chosen", false),
                Err(e) if e == "Cancelled" => {}
                Err(e) => app.toast(e, false),
            }
        }
        if let Some(r) = self.fill.as_ref().and_then(|j| j.take()) {
            self.fill = None;
            match r {
                Ok(p) => {
                    let ch: Vec<(usize, Pick)> = p
                        .into_iter()
                        .enumerate()
                        .filter_map(|(i, x)| x.map(|x| (i, x)))
                        .filter(|(i, _)| self.stage.sess.picks.get(*i).is_some_and(|c| c.is_none()))
                        .collect();
                    self.stage.set_picks(&ch);
                }
                Err(e) => app.toast(e, false),
            }
        }
        let done = self.export.as_ref().and_then(|e| e.result.lk().take());
        if let Some(r) = done {
            self.export = None;
            match r {
                Ok(p) => {
                    app.toast(format!("Saved {}", p.display()), true);
                    self.last_export = Some(p);
                }
                Err(e) if e == "Cancelled" => app.toast("Export cancelled", false),
                Err(e) => app.toast(e, false),
            }
        }
        // the window's render settings changed (look, sound, output …): the preview follows
        let want = render_settings(&self.stage.sess.settings, &app.s);
        let j = serde_json::to_string(&want).unwrap_or_default();
        if j != self.applied {
            self.applied = j;
            let size_changed = want.show_quality != self.stage.sess.settings.show_quality;
            let title = if want.title.trim().is_empty() {
                self.stage.sess.song.name.clone()
            } else {
                want.title.trim().to_string()
            };
            self.stage.sess.title = title;
            let vol_only = {
                let mut a = want.clone();
                a.song_vol = self.stage.sess.settings.song_vol;
                a.clip_vol = self.stage.sess.settings.clip_vol;
                serde_json::to_string(&a).ok()
                    == serde_json::to_string(&self.stage.sess.settings).ok()
            };
            if vol_only {
                self.stage.set_volumes(want.song_vol, want.clip_vol);
            } else {
                let t = self.stage.time();
                let was = self.stage.playing();
                let sess = Session {
                    settings: want.clone(),
                    ..self.stage.session()
                };
                let pins = app.pins();
                self.stage.shutdown();
                self.stage = Stage::new(sess, preview_short(&want), pins, Vec::new(), t);
                self.stage.want_play = was;
                let _ = size_changed;
            }
        }
    }

    fn save_project(&mut self, app: &mut App, ask: bool) {
        let path = if ask || self.path.is_none() {
            let name =
                crate::util::safe_file_name(&format!("PMV - {}", self.stage.sess.title), 100);
            let mut d = rfd::FileDialog::new()
                .add_filter("PMV project", &[project::EXT])
                .set_file_name(format!("{name}.{}", project::EXT));
            if let Some(l) = &app.stored.last_dir {
                d = d.set_directory(l);
            }
            match d.save_file() {
                Some(mut p) => {
                    if crate::app::theme::ext_of(&p) != project::EXT {
                        p.set_extension(project::EXT);
                    }
                    p
                }
                None => return,
            }
        } else {
            self.path.clone().unwrap_or_default()
        };
        let sess = self.stage.session();
        if !sess.complete() {
            app.toast(
                "Some shots have no clip yet – wait until they are chosen",
                false,
            );
            return;
        }
        match sess.save(&path) {
            Ok(()) => {
                app.toast(format!("Project saved: {}", path.display()), true);
                app.stored.last_dir = path.parent().map(|p| p.to_string_lossy().to_string());
                app.stored
                    .recent_projects
                    .retain(|x| std::path::Path::new(x) != path);
                app.stored
                    .recent_projects
                    .insert(0, path.to_string_lossy().to_string());
                app.stored.recent_projects.truncate(8);
                self.path = Some(path);
                self.dirty = false;
            }
            Err(e) => app.toast(format!("The project couldn't be saved: {e:#}"), false),
        }
    }

    fn start_export(&mut self, app: &mut App) {
        if self.export.is_some() {
            return;
        }
        let mut sess = self.stage.session();
        if !sess.complete() {
            app.toast(
                "Some shots have no clip yet – wait until they are chosen",
                false,
            );
            return;
        }
        sess.settings = render_settings(&sess.settings, &app.s);
        let codec = sess.settings.codec.clone();
        let ext = crate::enc::extension(&codec);
        let mut d = rfd::FileDialog::new()
            .add_filter(
                if ext == "webm" {
                    "WebM video"
                } else {
                    "MP4 video"
                },
                &[ext],
            )
            .set_file_name(export::file_name(&sess.title, &codec));
        if let Some(l) = &app.stored.last_dir {
            d = d.set_directory(l);
        }
        let Some(out) = d.save_file() else { return };
        app.stored.last_dir = out.parent().map(|p| p.to_string_lossy().to_string());
        self.stage.pause();
        let prog = Arc::new(Mutex::new(Progress::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let job = export::Job {
            out,
            pins: app.pins(),
            range: None,
        };
        {
            let (prog, cancel, result) = (prog.clone(), cancel.clone(), result.clone());
            std::thread::spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    export::run(&sess, &job, &prog, &cancel)
                }));
                *result.lk() = Some(match r {
                    Ok(Ok(p)) => Ok(p),
                    Ok(Err(e)) => Err(format!("{e:#}")),
                    Err(_) => Err("The export stopped because of an internal error".into()),
                });
            });
        }
        self.export = Some(ExportRun {
            prog,
            cancel,
            result,
            started: Instant::now(),
        });
    }

    pub fn ui(&mut self, ctx: &egui::Context, app: &mut App) -> Next {
        self.poll(app);
        if self.stage.is_ready() {
            if let Some(a) = self.script.take() {
                for step in a.split(';') {
                    match step.split_once(':') {
                        Some(("seek", t)) => self.stage.seek(t.parse().unwrap_or(0.0)),
                        Some(("swap", how)) => self.start_swap(app, how),
                        _ if step == "play" => self.stage.play(),
                        _ => {}
                    }
                }
            }
        }
        let mut next = Next::Stay;
        // keys
        if !ctx.wants_keyboard_input() {
            let (space, undo, redo, home, end, left, right, esc, sel_all) = ctx.input_mut(|i| {
                let c = egui::Modifiers::COMMAND;
                (
                    i.key_pressed(egui::Key::Space),
                    i.consume_key(c, egui::Key::Z),
                    i.consume_key(c, egui::Key::Y)
                        || i.consume_key(c | egui::Modifiers::SHIFT, egui::Key::Z),
                    i.key_pressed(egui::Key::Home),
                    i.key_pressed(egui::Key::End),
                    i.key_pressed(egui::Key::ArrowLeft),
                    i.key_pressed(egui::Key::ArrowRight),
                    i.key_pressed(egui::Key::Escape),
                    i.consume_key(c, egui::Key::A),
                )
            });
            if space {
                self.stage.toggle();
            }
            if undo {
                self.do_undo();
            }
            if redo {
                self.do_redo();
            }
            if home {
                self.stage.seek(0.0);
            }
            if end {
                self.stage.seek(self.stage.duration() - 1.0);
            }
            if esc {
                self.sel.clear();
            }
            if sel_all {
                self.sel = (0..self.stage.sess.plan.shots.len()).collect();
            }
            if left || right {
                // the previous / next shot (in time) becomes the selection
                let plan = self.stage.sess.plan.clone();
                let cur = self
                    .sel
                    .iter()
                    .next()
                    .copied()
                    .or_else(|| plan.shot_in(0, self.stage.time()));
                if let Some(c) = cur {
                    let n = if right {
                        (c + 1).min(plan.shots.len() - 1)
                    } else {
                        c.saturating_sub(1)
                    };
                    self.sel = [n].into();
                    self.anchor = Some(n);
                    self.stage.seek(plan.shots[n].t_on + 0.01);
                }
            }
        }
        self.stage.update(ctx);
        if self.stage.playing()
            || !self.stage.is_ready()
            || self.export.is_some()
            || self.swap.is_some()
            || self.fill.is_some()
        {
            ctx.request_repaint();
        }
        // background
        let screen = ctx.screen_rect();
        let bgp = ctx.layer_painter(egui::LayerId::background());
        bgp.rect_filled(screen, 0.0, c().bg);
        // top bar
        egui::TopBottomPanel::top("studio-top")
            .frame(egui::Frame::new().fill(alpha(c().base, 0.9)).inner_margin(egui::Margin::symmetric(14, 10)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if btn(ui, Btn::Ghost, Some(Icon::Prev), "Settings", true).on_hover_text("Back to the settings – this PMV stays open").clicked() {
                        next = Next::Setup;
                    }
                    ui.separator();
                    let sess = &self.stage.sess;
                    ui.label(egui::RichText::new(&sess.title).font(display(22.0)).color(c().text));
                    ui.label(
                        egui::RichText::new(format!(
                            "{} shots · {} BPM · {}{}{}",
                            sess.plan.shots.len(),
                            sess.song.a.bpm.round(),
                            fmt_time(sess.plan.duration),
                            sess.template.as_ref().map(|t| format!(" · template {t}")).unwrap_or_default(),
                            if self.dirty { " · not saved" } else { "" }
                        ))
                        .font(medium(13.0))
                        .color(c().accent2),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let exporting = self.export.is_some();
                        if btn(ui, Btn::Primary, Some(Icon::Download), "Export video …", !exporting).clicked() {
                            self.start_export(app);
                        }
                        if btn(ui, Btn::Normal, Some(Icon::Save), "Save project …", true).on_hover_text("A small file with every cut and clip – open it again to render exactly this PMV").clicked() {
                            self.save_project(app, true);
                        }
                        if btn(ui, Btn::Ghost, Some(Icon::Expand), "Play fullscreen", self.fill.is_none()).clicked() {
                            let t = self.stage.time();
                            self.stage.pause();
                            let sess = self.stage.session();
                            let st = Stage::new(sess, app.s.show_quality, app.pins(), Vec::new(), t);
                            let mut l = Live::new(st, vec![app.sel], 0);
                            l.from_studio = true;
                            l.stage.want_play = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                            next = Next::Live(Box::new(l));
                        }
                        ui.separator();
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 255, c().bg3);
                        }
                        // redo: the undo arrow mirrored
                        let p = ui.painter();
                        let c = if resp.hovered() { c().text } else { c().text2 };
                        let s2 = r.shrink(9.0);
                        let sx = |x: f32| s2.right() - (x / 24.0) * s2.width();
                        let sy = |y: f32| s2.top() + (y / 24.0) * s2.height();
                        p.add(egui::Shape::line(vec![Pos2::new(sx(9.0), sy(5.0)), Pos2::new(sx(4.0), sy(10.0)), Pos2::new(sx(9.0), sy(15.0))], Stroke::new(2.0_f32, c)));
                        p.add(egui::Shape::line(vec![Pos2::new(sx(4.0), sy(10.0)), Pos2::new(sx(15.0), sy(10.0)), Pos2::new(sx(19.0), sy(13.0)), Pos2::new(sx(19.0), sy(17.0)), Pos2::new(sx(16.0), sy(20.0)), Pos2::new(sx(11.0), sy(20.0))], Stroke::new(2.0_f32, c)));
                        if resp.on_hover_text("Redo (Ctrl+Y)").clicked() {
                            self.do_redo();
                        }
                        if icon_btn(ui, Icon::Undo, "Undo (Ctrl+Z)").clicked() {
                            self.do_undo();
                        }
                    });
                });
            });
        // inspector
        let side_w = (screen.width() * 0.28).clamp(300.0, 400.0);
        egui::SidePanel::right("studio-side")
            .resizable(false)
            .exact_width(side_w)
            .frame(
                egui::Frame::new()
                    .fill(c().bg2)
                    .inner_margin(egui::Margin::symmetric(14, 12)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("inspector")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        self.inspector(ui, app);
                    });
            });
        // preview + timeline
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(c().bg)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                let tl_h = (avail.height() * 0.42).clamp(190.0, 360.0);
                let prev_rect = Rect::from_min_max(
                    avail.min,
                    Pos2::new(avail.right(), avail.bottom() - tl_h - 54.0),
                );
                // the preview
                ui.painter().rect_filled(prev_rect, 8.0, Color32::BLACK);
                if let Some(tx) = &self.stage.tex {
                    let sz = tx.size_vec2();
                    let k = (prev_rect.width() / sz.x).min(prev_rect.height() / sz.y);
                    let r = Rect::from_center_size(prev_rect.center(), sz * k);
                    ui.painter().image(
                        tx.id(),
                        r,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                if !self.stage.is_ready() {
                    ui.painter().text(
                        prev_rect.center(),
                        Align2::CENTER_CENTER,
                        "Getting the clips ready …",
                        display(22.0),
                        c().text,
                    );
                }
                if let Some(j) = &self.fill {
                    let (f, m) = j.progress();
                    ui.painter().text(
                        prev_rect.center_bottom() - Vec2::new(0.0, 20.0),
                        Align2::CENTER_BOTTOM,
                        format!("{m} ({:.0} %)", f * 100.0),
                        medium(14.0),
                        c().text,
                    );
                }
                // transport
                let tr = Rect::from_min_max(
                    Pos2::new(avail.left(), prev_rect.bottom() + 8.0),
                    Pos2::new(avail.right(), prev_rect.bottom() + 48.0),
                );
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(tr), |ui| {
                    self.transport(ui);
                });
                // the timeline
                let tl =
                    Rect::from_min_max(Pos2::new(avail.left(), avail.bottom() - tl_h), avail.max);
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(tl), |ui| {
                    self.timeline(ui, tl);
                });
            });
        next
    }

    fn transport(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let playing = self.stage.playing() || self.stage.want_play;
            if icon_btn(
                ui,
                if playing { Icon::Pause } else { Icon::Play },
                "Play / pause (Space)",
            )
            .clicked()
            {
                self.stage.toggle();
            }
            if icon_btn(ui, Icon::Prev, "To the start (Home)").clicked() {
                self.stage.seek(0.0);
            }
            let t = self.stage.time();
            let d = self.stage.duration();
            ui.label(
                egui::RichText::new(format!("{} / {}", fmt_at(t), fmt_time(d)))
                    .font(medium(14.0))
                    .color(c().text),
            );
            ui.separator();
            ui.label(
                egui::RichText::new("Zoom")
                    .font(medium(13.0))
                    .color(c().text2),
            );
            ui.add(
                egui::Slider::new(&mut self.zoom, 1.0..=60.0)
                    .logarithmic(true)
                    .show_value(false)
                    .trailing_fill(true),
            );
            if btn(ui, Btn::Ghost, None, "Fit", true).clicked() {
                self.zoom = 1.0;
                self.scroll = 0.0;
            }
            ui.checkbox(&mut self.follow, "Follow")
                .on_hover_text("The timeline scrolls along while it plays");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let plan = &self.stage.sess.plan;
                let ph = &plan.phases[plan.phase_at(t)];
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {} cuts",
                        crate::plan::layout(&ph.layout).name,
                        plan.cuts_before(t)
                    ))
                    .font(text(13.0))
                    .color(c().faint),
                );
            });
        });
    }

    fn timeline(&mut self, ui: &mut Ui, rect: Rect) {
        let plan = self.stage.sess.plan.clone();
        let dur = plan.duration.max(0.1);
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 8.0, c().base);
        let left = rect.left() + 8.0;
        let width = (rect.width() - 16.0).max(50.0);
        let pps = (width as f64 / dur) * self.zoom as f64; // pixels per second
        let visible = width as f64 / pps;
        // follow the playhead
        let t_now = self.stage.time();
        if self.follow
            && self.stage.playing()
            && (t_now < self.scroll || t_now > self.scroll + visible * 0.9)
        {
            self.scroll = (t_now - visible * 0.1).max(0.0);
        }
        // the interaction surface; wheel: scroll, ctrl + wheel: zoom around the pointer
        let resp = ui.interact(rect, ui.id().with("timeline"), Sense::click_and_drag());
        if resp.hovered() {
            let (wheel, ctrl, pos) = ui.input(|i| {
                (
                    i.smooth_scroll_delta,
                    i.modifiers.command,
                    i.pointer.hover_pos(),
                )
            });
            if ctrl && wheel.y != 0.0 {
                let anchor = pos
                    .map(|p| self.scroll + ((p.x - left) as f64 / pps))
                    .unwrap_or(t_now);
                let z = (self.zoom * (1.0 + wheel.y * 0.004)).clamp(1.0, 60.0);
                self.zoom = z;
                let npps = (width as f64 / dur) * z as f64;
                let ax = pos.map(|p| p.x).unwrap_or(left);
                self.scroll = anchor - ((ax - left) as f64 / npps);
            } else if wheel.x != 0.0 || wheel.y != 0.0 {
                let d = if wheel.x.abs() > wheel.y.abs() {
                    wheel.x
                } else {
                    wheel.y
                };
                self.scroll -= d as f64 / pps;
            }
        }
        let pps = (width as f64 / dur) * self.zoom as f64;
        let visible = width as f64 / pps;
        self.scroll = self.scroll.clamp(0.0, (dur - visible).max(0.0));
        let scroll = self.scroll;
        let x_of = |t: f64| left + ((t - scroll) * pps) as f32;
        let t_of = |x: f32| scroll + ((x - left) as f64 / pps);
        let ruler = Rect::from_min_size(
            rect.min + Vec2::new(0.0, 4.0),
            Vec2::new(rect.width(), 20.0),
        );
        let band = Rect::from_min_size(
            Pos2::new(rect.left(), ruler.bottom() + 2.0),
            Vec2::new(rect.width(), 20.0),
        );
        let lanes_top = band.bottom() + 6.0;
        let marks_h = 12.0;
        let lane_n = plan
            .phases
            .iter()
            .map(|ph| ph.groups)
            .max()
            .unwrap_or(1)
            .max(1);
        let lane_h =
            ((rect.bottom() - lanes_top - marks_h - 8.0) / lane_n as f32).clamp(18.0, 64.0);
        // ruler: ticks
        let step = [0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0]
            .into_iter()
            .find(|s| s * pps >= 70.0)
            .unwrap_or(600.0);
        let mut tk = (self.scroll / step).floor() * step;
        while tk <= self.scroll + visible {
            let x = x_of(tk);
            if x >= rect.left() {
                p.line_segment(
                    [
                        Pos2::new(x, ruler.top() + 12.0),
                        Pos2::new(x, ruler.bottom()),
                    ],
                    Stroke::new(1.0_f32, c().faint),
                );
                p.text(
                    Pos2::new(x + 3.0, ruler.top()),
                    Align2::LEFT_TOP,
                    fmt_at(tk),
                    text(11.0),
                    c().faint,
                );
            }
            tk += step;
        }
        // layout band
        for (i, ph) in plan.phases.iter().enumerate() {
            let end = plan.phases.get(i + 1).map(|n| n.t).unwrap_or(dur);
            let (a, b) = (x_of(ph.t), x_of(end));
            if b < rect.left() || a > rect.right() {
                continue;
            }
            let lc = layout_color(&ph.layout);
            let r = Rect::from_min_max(
                Pos2::new(a.max(rect.left()), band.top()),
                Pos2::new(b.min(rect.right()) - 1.0, band.bottom()),
            );
            p.rect_filled(r, 2.0, super::themes::rgba(lc[0], lc[1], lc[2], 200));
            if r.width() > 60.0 {
                // (dark text on light colours, light on dark)
                let lum = 0.299 * lc[0] as f32 + 0.587 * lc[1] as f32 + 0.114 * lc[2] as f32;
                p.text(
                    r.left_center() + Vec2::new(5.0, 0.0),
                    Align2::LEFT_CENTER,
                    crate::plan::layout(&ph.layout).name,
                    medium(11.5),
                    if lum > 140.0 { c().on_accent } else { c().text },
                );
            }
        }
        // shots
        let hover = resp.hover_pos();
        let mut hovered_shot = None;
        let ctx = ui.ctx().clone();
        for sh in plan.shots.iter() {
            let (a, b) = (x_of(sh.t_on), x_of(sh.t_off));
            if b < rect.left() || a > rect.right() || sh.t_off <= sh.t_on {
                continue;
            }
            let y = lanes_top + sh.group as f32 * lane_h;
            let r = Rect::from_min_max(
                Pos2::new(a.max(rect.left()) + 0.5, y + 1.0),
                Pos2::new((b.min(rect.right()) - 0.5).max(a + 1.5), y + lane_h - 1.0),
            );
            let pick = self.stage.sess.picks.get(sh.id).cloned().flatten();
            let selected = self.sel.contains(&sh.id);
            let is_hover = hover.is_some_and(|h| r.contains(h));
            if is_hover {
                hovered_shot = Some(sh.id);
            }
            let base = pick
                .as_ref()
                .map(|p| clip_color(&p.path))
                .unwrap_or(Color32::from_gray(60));
            let fill = if is_hover {
                super::themes::mix(base, Color32::WHITE, 0.15)
            } else {
                base
            };
            p.rect_filled(r, 3.0, fill);
            if let Some(pk) = &pick {
                if r.width() > 34.0 && lane_h >= 30.0 {
                    if let Some(tid) = self.thumbs.get(&ctx, pk) {
                        let th = r.height() - 4.0;
                        let tw = (th * 16.0 / 9.0).min(r.width() - 4.0);
                        let tr =
                            Rect::from_min_size(r.min + Vec2::new(2.0, 2.0), Vec2::new(tw, th));
                        p.image(
                            tid,
                            tr,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        if r.width() > tw + 40.0 {
                            let g = p.layout(
                                pk.name.clone(),
                                text(11.5),
                                c().text,
                                r.width() - tw - 10.0,
                            );
                            p.with_clip_rect(r).galley(
                                Pos2::new(tr.right() + 4.0, r.top() + 3.0),
                                g,
                                c().text,
                            );
                        }
                    } else if r.width() > 40.0 {
                        let g = p.layout(pk.name.clone(), text(11.5), c().text, r.width() - 8.0);
                        p.with_clip_rect(r).galley(
                            Pos2::new(r.left() + 4.0, r.top() + 3.0),
                            g,
                            c().text,
                        );
                    }
                }
                if pk.manual {
                    p.circle_filled(Pos2::new(r.right() - 5.0, r.top() + 5.0), 2.5, c().shine);
                }
            }
            if selected {
                p.rect_stroke(r, 3.0, Stroke::new(2.0_f32, c().accent), StrokeKind::Inside);
            }
        }
        // drops and flashes
        let marks_y = rect.bottom() - marks_h - 2.0;
        for (t, _) in &plan.drops {
            let x = x_of(*t);
            if x >= rect.left() && x <= rect.right() {
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x - 1.0, marks_y), Vec2::new(3.0, marks_h)),
                    1.0,
                    c().accent,
                );
            }
        }
        for e in &plan.fx {
            if let crate::plan::Fx::Flash { a, .. } = e.fx {
                if a >= 0.8 {
                    continue;
                }
                let x = x_of(e.t);
                if x >= rect.left() && x <= rect.right() {
                    p.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(x, marks_y + 4.0),
                            Vec2::new(1.0, marks_h - 4.0),
                        ),
                        0.0,
                        alpha(c().text, 0.4),
                    );
                }
            }
        }
        // rubber band selection
        if resp.drag_started() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if pos.y < band.bottom() {
                    self.band = None;
                } else if hovered_shot.is_none() {
                    self.band = Some((pos, pos));
                }
            }
        }
        if resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if let Some(b) = &mut self.band {
                    b.1 = pos;
                } else if pos.y < band.bottom() + 2.0 {
                    // dragging on the ruler: scrub
                    self.stage.seek(t_of(pos.x).clamp(0.0, dur));
                }
            }
        }
        if let Some((a, b)) = self.band {
            let r = Rect::from_two_pos(a, b);
            p.rect_filled(r, 2.0, alpha(c().accent, 0.12));
            p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, c().accent), StrokeKind::Inside);
            if resp.drag_stopped() {
                let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                if !add {
                    self.sel.clear();
                }
                let (t0, t1) = (t_of(r.left()), t_of(r.right()));
                for sh in &plan.shots {
                    let y = lanes_top + sh.group as f32 * lane_h;
                    let lane = Rect::from_min_max(
                        Pos2::new(r.left(), y),
                        Pos2::new(r.right(), y + lane_h),
                    );
                    if sh.t_off > t0 && sh.t_on < t1 && lane.intersects(r) && sh.t_off > sh.t_on {
                        self.sel.insert(sh.id);
                    }
                }
                self.band = None;
            }
        }
        // clicks
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let mods = ui.input(|i| i.modifiers);
                match hovered_shot {
                    Some(id) => {
                        if mods.command {
                            if !self.sel.remove(&id) {
                                self.sel.insert(id);
                            }
                            self.anchor = Some(id);
                        } else if mods.shift && self.anchor.is_some() {
                            let a = self.anchor.unwrap_or(id);
                            let (lo, hi) = (
                                plan.shots[a].t_on.min(plan.shots[id].t_on),
                                plan.shots[a].t_on.max(plan.shots[id].t_on),
                            );
                            for sh in &plan.shots {
                                if sh.t_on >= lo && sh.t_on <= hi && sh.t_off > sh.t_on {
                                    self.sel.insert(sh.id);
                                }
                            }
                        } else {
                            self.sel = [id].into();
                            self.anchor = Some(id);
                            self.manual = None;
                        }
                    }
                    None => {
                        if pos.y > band.bottom() {
                            self.sel.clear();
                        }
                        self.stage.seek(t_of(pos.x).clamp(0.0, dur));
                    }
                }
            }
        }
        if resp.double_clicked() {
            if let Some(id) = hovered_shot {
                self.stage.seek(plan.shots[id].t_on + 0.01);
            }
        }
        // hover tip
        if let Some(id) = hovered_shot {
            if let Some(pk) = self.stage.sess.picks.get(id).cloned().flatten() {
                let sh = &plan.shots[id];
                resp.clone().on_hover_text(format!(
                    "{}\nfrom {} · on {}–{} · field {}{}{}",
                    pk.name,
                    fmt_at(pk.start),
                    fmt_at(sh.t_on),
                    fmt_at(sh.t_off),
                    sh.group + 1,
                    if pk.beat.is_some() {
                        " · on the beat"
                    } else {
                        ""
                    },
                    if pk.manual { " · changed by hand" } else { "" }
                ));
            }
        }
        // the playhead
        let x = x_of(t_now);
        if x >= rect.left() && x <= rect.right() {
            p.line_segment(
                [
                    Pos2::new(x, rect.top() + 2.0),
                    Pos2::new(x, rect.bottom() - 2.0),
                ],
                Stroke::new(2.0_f32, c().accent),
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x - 6.0, rect.top() + 2.0),
                    Pos2::new(x + 6.0, rect.top() + 2.0),
                    Pos2::new(x, rect.top() + 10.0),
                ],
                c().accent,
                Stroke::NONE,
            ));
        }
        // a scrollbar when zoomed in
        if self.zoom > 1.01 {
            let sb =
                Rect::from_min_size(Pos2::new(left, rect.bottom() - 4.0), Vec2::new(width, 3.0));
            p.rect_filled(sb, 1.5, c().bg3);
            let a = left + (self.scroll / dur) as f32 * width;
            let w = (visible / dur) as f32 * width;
            p.rect_filled(
                Rect::from_min_size(Pos2::new(a, sb.top()), Vec2::new(w.max(8.0), 3.0)),
                1.5,
                c().faint,
            );
        }
    }

    fn inspector(&mut self, ui: &mut Ui, app: &mut App) {
        // export progress
        if let Some(e) = &self.export {
            let p = e.prog.lk().clone();
            panel_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new("Exporting")
                        .font(display(20.0))
                        .color(c().text),
                );
                ui.label(
                    egui::RichText::new(&p.msg)
                        .font(medium(13.5))
                        .color(c().text2),
                );
                progress_bar(ui, p.frac);
                let el = e.started.elapsed().as_secs_f64();
                hint(
                    ui,
                    &format!(
                        "{}{}{}",
                        fmt_time(el),
                        p.eta
                            .map(|x| format!(" · about {} left", fmt_time(x)))
                            .unwrap_or_default(),
                        if p.encoder.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", p.encoder)
                        }
                    ),
                );
                if !p.fails.is_empty() {
                    hint(
                        ui,
                        &format!("{} clips couldn't be read and were left out", p.fails.len()),
                    );
                }
                if btn(ui, Btn::Ghost, None, "Cancel", true).clicked() {
                    e.cancel.store(true, Ordering::SeqCst);
                }
            });
        } else if let Some(p) = self.last_export.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("Saved:")
                        .font(medium(13.0))
                        .color(c().ok),
                );
                ui.label(
                    egui::RichText::new(
                        p.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    )
                    .font(medium(13.0))
                    .color(c().text),
                );
                if btn(ui, Btn::Ghost, Some(Icon::Folder), "Show", true).clicked() {
                    super::open_folder(&p);
                }
            });
        }
        // the selection
        let plan = self.stage.sess.plan.clone();
        let n = self.sel.len();
        ui.label(
            egui::RichText::new(if n == 0 {
                "Clips".into()
            } else if n == 1 {
                "1 shot selected".into()
            } else {
                format!("{n} shots selected")
            })
            .font(display(22.0))
            .color(c().text),
        );
        if n == 0 {
            hint(ui, "Click a shot in the timeline to select it – Ctrl+click adds one, Shift+click a range, drag over empty space selects all in the box, Ctrl+A everything. Then swap the clips, or change the moment they are taken from.");
            let mut counts: HashMap<String, usize> = HashMap::new();
            for p in self.stage.sess.picks.iter().flatten() {
                *counts.entry(p.name.clone()).or_default() += 1;
            }
            let mut v: Vec<(String, usize)> = counts.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            lab(
                ui,
                &format!("{} different clips", v.len()),
                "most used first",
            );
            for (name, n) in v.iter().take(12) {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{n}×"))
                            .font(medium(12.5))
                            .color(c().accent),
                    );
                    ui.label(egui::RichText::new(name).font(text(13.0)).color(c().text2));
                });
            }
        } else {
            // what is in them
            let names: Vec<String> = self
                .sel
                .iter()
                .filter_map(|&i| self.stage.sess.picks.get(i).cloned().flatten())
                .map(|p| p.name)
                .collect();
            let mut uniq = names.clone();
            uniq.dedup();
            if uniq.len() == 1 {
                hint(ui, &uniq[0]);
            } else {
                hint(
                    ui,
                    &format!("{} different clips", {
                        let mut u = names.clone();
                        u.sort();
                        u.dedup();
                        u.len()
                    }),
                );
            }
            if n == 1 {
                if let Some(&id) = self.sel.iter().next() {
                    let sh = &plan.shots[id];
                    hint(
                        ui,
                        &format!(
                            "On {}–{} · field {} of {}",
                            fmt_at(sh.t_on),
                            fmt_at(sh.t_off),
                            sh.group + 1,
                            crate::plan::layout(&plan.phases[sh.phase].layout).name
                        ),
                    );
                    if btn(ui, Btn::Ghost, Some(Icon::Play), "Play from here", true).clicked() {
                        self.stage.seek(sh.t_on);
                        self.stage.play();
                    }
                }
            }
            group_head(ui, "Swap", "new clips for the selected shots");
            lab(ui, "Take them from", "");
            let mut opts: Vec<(Source, String)> = vec![
                (Source::Library, "The library (step 2's filters)".into()),
                (Source::Used, "The clips of this PMV".into()),
            ];
            for r in app.lib.lk().roots.iter() {
                opts.push((
                    Source::Folder(r.clone()),
                    format!(
                        "Folder: {}",
                        r.file_name()
                            .map(|x| x.to_string_lossy().to_string())
                            .unwrap_or_default()
                    ),
                ));
            }
            select(
                ui,
                "swap-source",
                &mut self.source,
                &opts,
                ui.available_width() - 8.0,
            );
            let busy = self.swap.is_some();
            ui.horizontal_wrapped(|ui| {
                if btn(ui, Btn::Primary, Some(Icon::Bolt), "Auto", !busy)
                    .on_hover_text(
                        "Like the generator: best moments, match cuts, the music's pace …",
                    )
                    .clicked()
                {
                    self.start_swap(app, "auto");
                }
                if btn(ui, Btn::Normal, Some(Icon::Dice), "Random", !busy)
                    .on_hover_text("Any clip of the sources, from a random moment")
                    .clicked()
                {
                    self.start_swap(app, "random");
                }
            });
            if let Some(j) = &self.swap {
                let (f, m) = j.progress();
                ui.label(egui::RichText::new(m).font(medium(13.0)).color(c().text2));
                progress_bar(ui, f);
            }
            // by hand
            group_head(ui, "By hand", "a clip you choose, from a moment you choose");
            let w = ui.available_width();
            field(ui, &mut self.clip_q, "Find a clip …", w);
            let q = self.clip_q.to_lowercase();
            let list: Vec<Item> = self
                .sources(app)
                .into_iter()
                .filter(|i| q.is_empty() || i.path.to_string_lossy().to_lowercase().contains(&q))
                .take(40)
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("manual-list")
                .max_height(170.0)
                .show(ui, |ui| {
                    for it in &list {
                        let on = self.manual.as_ref().is_some_and(|m| m.0.path == it.path);
                        if ui
                            .selectable_label(on, egui::RichText::new(&it.name).font(text(13.0)))
                            .on_hover_text(it.path.to_string_lossy())
                            .clicked()
                        {
                            let mut it = it.clone();
                            if it.w == 0 {
                                if let Ok((w, h, d)) = crate::library::probe_item(&it.path, it.kind)
                                {
                                    it.w = w;
                                    it.h = h;
                                    it.dur = d;
                                }
                            }
                            let (lo, _) = media::start_range(&it, &self.stage.sess.settings);
                            self.manual = Some((it, lo));
                        }
                    }
                });
            if let Some((it, start)) = &mut self.manual {
                if it.kind == Kind::Video && it.dur > 1.0 {
                    let max = (it.dur - 0.6).max(0.0);
                    ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(80.0);
                    ui.add(
                        egui::Slider::new(start, 0.0..=max)
                            .show_value(false)
                            .trailing_fill(true),
                    );
                    hint(
                        ui,
                        &format!("From {} of {}", fmt_at(*start), fmt_at(it.dur)),
                    );
                }
                if btn(
                    ui,
                    Btn::Normal,
                    Some(Icon::Check),
                    "Use this clip",
                    !busy && it.w > 0,
                )
                .clicked()
                {
                    self.start_swap(app, "manual");
                }
            }
            // the moment
            group_head(
                ui,
                "Change the moment",
                "the same clip, from another moment of its video",
            );
            let shots: Vec<(usize, usize)> = self
                .sel
                .iter()
                .take(6)
                .map(|&i| (plan.shots[i].group + 1, i))
                .collect();
            if self.sel.len() > 6 {
                hint(ui, "(the first 6 of the selection)");
            }
            let before = self.stage.sess.picks.clone();
            moments_ui(ui, &mut self.stage, app, &shots, &mut self.moments);
            if self.stage.sess.picks != before {
                self.undo.push(before);
                self.redo.clear();
                self.dirty = true;
            }
            if n == 1 {
                if let Some(pk) = self
                    .sel
                    .iter()
                    .next()
                    .and_then(|&i| self.stage.sess.picks.get(i).cloned().flatten())
                {
                    if btn(
                        ui,
                        Btn::Ghost,
                        None,
                        "Select every shot with this clip",
                        true,
                    )
                    .clicked()
                    {
                        self.sel = self
                            .stage
                            .sess
                            .picks
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| p.as_ref().is_some_and(|p| p.path == pk.path))
                            .map(|(i, _)| i)
                            .collect();
                    }
                }
            }
        }
        // output summary
        ui.add_space(6.0);
        group_head(ui, "Export", "");
        let s = &mut app.s;
        let q: Vec<(f64, String)> = settings::QUALITIES
            .iter()
            .map(|(v, l)| (*v, l.to_string()))
            .collect();
        ui.horizontal(|ui| {
            select(ui, "st-quality", &mut s.quality, &q, 120.0);
            let c: Vec<(String, String)> = settings::CODECS
                .iter()
                .map(|(v, l)| (v.to_string(), l.to_string()))
                .collect();
            select(ui, "st-codec", &mut s.codec, &c, 150.0);
        });
        seg(
            ui,
            &mut s.enc_speed,
            &[
                ("fast".to_string(), "Fastest", ""),
                ("balanced".into(), "Balanced", ""),
                ("small".into(), "Smallest", ""),
            ],
        );
        let (w, h) = crate::show::size_for(&self.stage.sess.settings, s.quality);
        hint(
            ui,
            &format!(
                "{w}×{h} · {} fps · {} – more in the settings (Output)",
                s.export_fps,
                settings::ENCODERS
                    .iter()
                    .find(|e| e.0 == s.encoder)
                    .map(|e| e.1)
                    .unwrap_or("")
            ),
        );
        if let Some(p) = &self.path {
            hint(ui, &format!("Project: {}", p.display()));
            if self.dirty && btn(ui, Btn::Ghost, Some(Icon::Save), "Save changes", true).clicked() {
                self.save_project(app, false);
            }
        }
    }
}

/// The clips a session uses, as library items (to swap among them)
fn used_items(sess: &Session) -> Vec<Item> {
    let mut seen = std::collections::HashSet::new();
    sess.picks
        .iter()
        .flatten()
        .filter(|p| seen.insert(p.path.clone()))
        .enumerate()
        .map(|(i, p)| Item {
            id: 2_000_000 + i,
            kind: p.kind,
            path: p.path.clone(),
            name: p.name.clone(),
            root: p.path.parent().map(|x| x.to_path_buf()).unwrap_or_default(),
            w: p.w,
            h: p.h,
            dur: p.dur,
            probed: true,
            bad: None,
            skip: Default::default(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_settings_keep_the_plan_side() {
        let mut base = Settings {
            cut: "2".into(),
            ..Default::default()
        };
        base.layouts.quad = false;
        let mut from = Settings {
            cut: "1".into(),
            look: "noir".into(),
            codec: "av1".into(),
            ..Default::default()
        };
        from.fx.vhs = true;
        from.fx.flash = false;
        let r = render_settings(&base, &from);
        assert_eq!(r.cut, "2");
        assert!(!r.layouts.quad);
        assert_eq!(r.look, "noir");
        assert_eq!(r.codec, "av1");
        assert!(r.fx.vhs);
        // (flash is a trigger in the plan – unchanged)
        assert!(r.fx.flash);
    }
}
