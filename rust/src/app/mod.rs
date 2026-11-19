// The window: the setup page (1 Music · 2 Clips · 3 Style · 4 Go – laid out like the web version), the live
// show, and the studio (preview, timeline, editing, project files and export) for a generated PMV.

mod live;
mod setup;
mod stage;
mod studio;
pub mod theme;
pub mod themes;

use crate::library::{self, Item, Kind};
use crate::player;
use crate::project::{self, Session};
use crate::settings::{self, Settings, Stored};
use crate::show;
use crate::song::{self, Song};
use crate::template::Template;
use crate::util::LockExt;
use eframe::egui::{self, Color32, Vec2};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use theme::*;

// ---------- state ----------

pub enum Load {
    Loading(f32),
    Ready(Box<Song>),
    Failed(String),
}

pub struct SongEntry {
    pub path: PathBuf,
    pub from: String,
    pub to: String,
    pub state: Arc<Mutex<Load>>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Song,
    Template,
}

/// PMV as template: the scan running, or its result
#[derive(Default)]
pub struct Tpl {
    pub query: String,
    pub scan: Option<Job<(Song, Template)>>,
    pub name: String,
    /// how the PMV is used: classic (follows the beat) or exact cuts – and how the current one was analysed
    pub kind: crate::template::Kind,
    pub scanned: crate::template::Kind,
    /// what the show uses: the music and the template in the music's time
    pub result: Option<(Arc<Song>, Arc<Template>)>,
    /// the PMV and its scan (its own music, the timeline in the PMV's time)
    pub pmv: Option<PathBuf>,
    pub raw: Option<(Arc<Song>, Arc<Template>)>,
    /// the music chosen and the PMV time where the new PMV begins (where the music segment starts)
    pub music: Option<crate::template::Music>,
    pub start: f64,
    /// how sure the song file's place in the PMV is
    pub fit: Option<crate::align::Fit>,
    /// the start as typed: time / frame
    pub start_text: String,
    pub frame_text: String,
    /// applying a music choice / start (loading the song part, finding a song file's place)
    pub load: Option<Job<TplMusicDone>>,
}

/// A music choice applied: the song, the template in its time, the music, the start, a song file's place
pub type TplMusicDone = (
    Song,
    Template,
    crate::template::Music,
    f64,
    Option<crate::align::Fit>,
);

impl Tpl {
    /// The start as text fields show it
    pub fn sync_text(&mut self) {
        let fps = self.raw.as_ref().map(|r| r.1.fps).unwrap_or(0.0);
        self.start_text = crate::util::fmt_ms(self.start);
        self.frame_text = if fps > 0.0 {
            format!("{}", (self.start * fps).round() as i64)
        } else {
            String::new()
        };
    }
}

/// A job in a thread: progress, cancel, result
pub struct Job<T> {
    pub prog: Arc<Mutex<(f32, String)>>,
    pub cancel: Arc<AtomicBool>,
    pub result: Arc<Mutex<Option<Result<T, String>>>>,
}
impl<T: Send + 'static> Job<T> {
    pub fn spawn(
        f: impl FnOnce(&(dyn Fn(f32, String) + Sync), &AtomicBool) -> anyhow::Result<T> + Send + 'static,
    ) -> Job<T> {
        let prog = Arc::new(Mutex::new((0.0, String::new())));
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let (p2, c2, r2) = (prog.clone(), cancel.clone(), result.clone());
        std::thread::spawn(move || {
            let report = |f: f32, m: String| *p2.lk() = (f, m);
            // (a panic in the job becomes an error message, not a dead window)
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&report, &c2)));
            *r2.lk() = Some(match r {
                Ok(Ok(v)) => Ok(v),
                Ok(Err(e)) => Err(format!("{e:#}")),
                Err(_) => Err("Something went wrong inside (see the console)".into()),
            });
        });
        Job {
            prog,
            cancel,
            result,
        }
    }
    pub fn take(&self) -> Option<Result<T, String>> {
        self.result.lk().take()
    }
    pub fn progress(&self) -> (f32, String) {
        self.prog.lk().clone()
    }
}

/// Which encoders work here: (codec, encoder kind, ffmpeg encoder)
pub type EncSurvey = Vec<(&'static str, &'static str, String)>;

pub enum Screen {
    Setup,
    Live(Box<live::Live>),
    Studio,
}

pub struct Toast {
    pub msg: String,
    pub ok: bool,
    pub at: Instant,
}

/// Window-only state of the setup page
#[derive(Default)]
pub struct UiState {
    pub open: BTreeMap<&'static str, bool>,
    pub folder_q: String,
    pub video_q: String,
    pub per_video_open: bool,
    pub preset_sel: String,
    pub save_name: Option<String>,
    pub show_terms: bool,
}

pub struct App {
    pub s: Settings,
    pub stored: Stored,
    saved_json: String,
    save_check: Instant,
    pub lib: library::Shared,
    pub probing: Arc<AtomicUsize>,
    pub songs: Vec<SongEntry>,
    pub sel: usize,
    pub mode: Mode,
    pub tpl: Tpl,
    pub ui: UiState,
    pub screen: Screen,
    pub studio: Option<Box<studio::Studio>>,
    pub gen: Option<Job<Session>>,
    pub opening: Option<Job<Session>>,
    toasts: Vec<Toast>,
    bg: Background,
    applied_zoom: f32,
    pub device_rate: u32,
    pub ff_err: Option<String>,
    pub encoders: Arc<Mutex<Option<EncSurvey>>>,
    auto_started: bool,
    pub gate_ok: bool,
    /// developer check: PMVGEN_SHOT=file – the window draws itself for a moment, saves a picture of itself
    /// (only this window) as raw RGBA next to `file`, and closes
    shot: Option<(PathBuf, u32)>,
    pub shot_scroll: Option<f32>,
    /// the theme now applied (the palette changes when the setting does)
    applied_theme: String,
    /// developer check PMVGEN_SHOT_LONG: the whole page, scrolled through and captured piece by piece
    pub shot_long: bool,
    /// the setup page's scroll: (content height, viewport height, viewport top on screen) in points
    pub shot_page: Option<(f32, f32, f32)>,
    shot_next: u32,
}

const TERMS_VERSION: &str = "1";

fn app_icon() -> egui::IconData {
    use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};
    let Some(mut pm) = Pixmap::new(64, 64) else {
        return egui::IconData::default();
    };
    let mut p = Paint {
        anti_alias: true,
        ..Default::default()
    };
    p.set_color_rgba8(35, 15, 31, 255);
    if let Some(r) = Rect::from_xywh(0.0, 0.0, 64.0, 64.0) {
        pm.fill_rect(r, &p, Transform::identity(), None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(16.0, 26.0);
    pb.cubic_to(16.0, 26.0, 8.0, 21.1, 5.8, 16.1);
    pb.cubic_to(4.2, 12.5, 6.4, 8.5, 10.3, 8.5);
    pb.cubic_to(12.6, 8.5, 14.4, 9.9, 16.0, 11.9);
    pb.cubic_to(17.5, 9.9, 19.3, 8.5, 21.6, 8.5);
    pb.cubic_to(25.5, 8.5, 27.8, 12.5, 26.2, 16.1);
    pb.cubic_to(24.0, 21.1, 16.0, 26.0, 16.0, 26.0);
    pb.close();
    p.set_color_rgba8(255, 62, 138, 255);
    if let Some(path) = pb.finish() {
        pm.fill_path(
            &path,
            &p,
            FillRule::Winding,
            Transform::from_scale(2.0, 2.0),
            None,
        );
    }
    egui::IconData {
        rgba: pm.data().to_vec(),
        width: 64,
        height: 64,
    }
}

pub fn open_folder(p: &Path) {
    let dir = if p.is_dir() {
        p
    } else {
        p.parent().unwrap_or(p)
    };
    #[cfg(windows)]
    {
        let _ = crate::ff::command(Path::new("explorer"))
            .arg(format!("/select,{}", p.display()))
            .spawn();
        let _ = dir;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(dir).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

pub fn open_url(u: &str) {
    #[cfg(windows)]
    {
        let _ = crate::ff::command(Path::new("cmd"))
            .args(["/C", "start", "", u])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(u).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(u).spawn();
    }
}

pub const AUDIO_EXT: [&str; 12] = [
    "mp3", "m4a", "aac", "wav", "ogg", "oga", "opus", "flac", "wma", "aiff", "aif", "alac",
];

impl App {
    fn new(cc: &eframe::CreationContext) -> App {
        install_fonts(&cc.egui_ctx);
        install_style(&cc.egui_ctx);
        let stored = settings::load_stored();
        let s = settings::settings_from_stored(&stored);
        set_glass(s.glass);
        let lib: library::Shared = Default::default();
        {
            let mut l = lib.lk();
            for (k, v) in &stored.clip_skips {
                l.skips.insert(k.clone(), *v);
            }
        }
        let probing = Arc::new(AtomicUsize::new(0));
        // the folders from last time are added again (in the background)
        let folders = stored.folders.clone();
        if !folders.is_empty() {
            let (lib2, pr) = (lib.clone(), probing.clone());
            std::thread::spawn(move || {
                for f in folders {
                    let p = PathBuf::from(&f);
                    if p.is_dir() {
                        library::add_folder(&lib2, &p);
                    }
                }
                library::probe_all(&lib2, &pr);
            });
        }
        let ff_err = crate::ff::available().err().map(|e| e.to_string());
        // which encoders work here (test encodes take a few seconds – in the background)
        let encoders = Arc::new(Mutex::new(None));
        if ff_err.is_none() {
            let e2 = encoders.clone();
            std::thread::spawn(move || {
                let v = crate::enc::survey();
                *e2.lk() = Some(v);
            });
        }
        let gate_ok = stored
            .settings
            .as_ref()
            .and_then(|v| v.get("termsAccepted"))
            .and_then(|v| v.as_str())
            == Some(TERMS_VERSION);
        let mut ui = UiState::default();
        // all sections open (like the web version); the ones closed last time stay closed
        if let Some(c) = stored
            .settings
            .as_ref()
            .and_then(|v| v.get("collapsed"))
            .and_then(|v| v.as_object())
        {
            for (k, v) in c {
                if v.as_bool() == Some(true) {
                    if let Some(key) = setup::SECTIONS.iter().find(|s| s.0 == k).map(|s| s.0) {
                        ui.open.insert(key, false);
                    }
                }
            }
        }
        App {
            saved_json: serde_json::to_string(&s).unwrap_or_default(),
            s,
            stored,
            save_check: Instant::now(),
            lib,
            probing,
            songs: Vec::new(),
            sel: 0,
            tpl: Tpl::default(),
            ui,
            screen: Screen::Setup,
            studio: None,
            gen: None,
            opening: None,
            toasts: Vec::new(),
            bg: Background::default(),
            applied_zoom: 0.0,
            device_rate: player::device_rate(),
            ff_err,
            encoders,
            auto_started: false,
            gate_ok,
            shot: std::env::var_os("PMVGEN_SHOT").map(|p| (PathBuf::from(p), 0)),
            mode: if std::env::var("PMVGEN_SHOT_MODE").as_deref() == Ok("tpl") {
                Mode::Template
            } else {
                Mode::Song
            },
            shot_scroll: std::env::var("PMVGEN_SHOT_SCROLL")
                .ok()
                .and_then(|v| v.parse().ok()),
            applied_theme: String::new(),
            shot_long: std::env::var_os("PMVGEN_SHOT_LONG").is_some(),
            shot_page: None,
            shot_next: 0,
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>, ok: bool) {
        let msg = msg.into();
        if !ok {
            eprintln!("[pmvgen] {msg}");
        }
        self.toasts.push(Toast {
            msg,
            ok,
            at: Instant::now(),
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    /// Settings, folders, pins … to disk when they changed (checked twice a second)
    fn persist(&mut self, force: bool) {
        if !force && self.save_check.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.save_check = Instant::now();
        let j = serde_json::to_string(&self.s).unwrap_or_default();
        let folders: Vec<String> = self
            .lib
            .lk()
            .roots
            .iter()
            .filter(|r| r.is_dir())
            .map(|r| r.to_string_lossy().to_string())
            .collect();
        let collapsed: serde_json::Map<String, serde_json::Value> = self
            .ui
            .open
            .iter()
            .filter(|(_, o)| !**o)
            .map(|(k, _)| (k.to_string(), serde_json::Value::Bool(true)))
            .collect();
        let mut v = serde_json::to_value(&self.s).unwrap_or(serde_json::Value::Null);
        if let serde_json::Value::Object(m) = &mut v {
            m.insert("collapsed".into(), serde_json::Value::Object(collapsed));
            if self.gate_ok {
                m.insert("termsAccepted".into(), TERMS_VERSION.into());
            }
        }
        let changed = force
            || j != self.saved_json
            || folders != self.stored.folders
            || self.stored.settings.as_ref() != Some(&v);
        if changed {
            self.stored.settings = Some(v);
            self.stored.folders = folders;
            match settings::save_stored(&self.stored) {
                Ok(()) => self.saved_json = j,
                Err(e) => eprintln!("[pmvgen] saving the settings: {e:#}"),
            }
        }
    }

    pub fn pins(&self) -> show::Pins {
        self.stored
            .pins
            .iter()
            .map(|(k, v)| (PathBuf::from(k), v.clone()))
            .collect()
    }

    // ---------- songs ----------

    pub fn add_song(&mut self, path: PathBuf) {
        if self.songs.iter().any(|s| s.path == path) {
            return;
        }
        let e = SongEntry {
            path,
            from: String::new(),
            to: String::new(),
            state: Arc::new(Mutex::new(Load::Loading(0.0))),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        self.start_load(&e, 0.0, None);
        self.songs.push(e);
        if self.songs.len() == 1 {
            self.sel = 0;
        }
        self.auto_started = false;
    }

    pub fn start_load(&self, e: &SongEntry, from: f64, to: Option<f64>) {
        let (path, st, cancel, rate) = (
            e.path.clone(),
            e.state.clone(),
            e.cancel.clone(),
            self.device_rate,
        );
        *st.lk() = Load::Loading(0.0);
        std::thread::spawn(move || {
            let st2 = st.clone();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                song::load(
                    &path,
                    from,
                    to,
                    rate,
                    &|p| *st2.lk() = Load::Loading(p),
                    &cancel,
                )
            }));
            *st.lk() = match r {
                Ok(Ok(s)) => Load::Ready(Box::new(s)),
                Ok(Err(e)) => Load::Failed(format!("{e:#}")),
                Err(_) => Load::Failed("reading it failed".into()),
            };
        });
    }

    pub fn song_ready(&self, i: usize) -> Option<Song> {
        match &*self.songs.get(i)?.state.lk() {
            Load::Ready(s) => Some((**s).clone()),
            _ => None,
        }
    }
    pub fn with_song(&mut self, f: impl FnOnce(&mut Song)) {
        if let Some(e) = self.songs.get(self.sel) {
            if let Load::Ready(s) = &mut *e.state.lk() {
                f(s);
            }
        }
    }

    /// The song the show / generator uses: the selected one, or the template's music
    pub fn current_song(&self) -> Option<Arc<Song>> {
        match self.mode {
            Mode::Template => self.tpl.result.as_ref().map(|r| r.0.clone()),
            Mode::Song => self.song_ready(self.sel).map(Arc::new),
        }
    }
    pub fn current_template(&self) -> Option<Arc<Template>> {
        match self.mode {
            Mode::Template => self.tpl.result.as_ref().map(|r| r.1.clone()),
            Mode::Song => None,
        }
    }

    // ---------- files ----------

    /// Dropped / opened paths: songs become songs, folders and video / image files clips, a project opens
    pub fn open_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let mut clips = Vec::new();
        let mut folders = 0;
        for p in paths {
            let ext = ext_of(&p);
            if ext == project::EXT {
                self.open_project(p);
            } else if p.is_dir() {
                folders += library::add_folder(&self.lib, &p);
            } else if AUDIO_EXT.contains(&ext.as_str()) {
                self.add_song(p);
            } else if library::kind_of(&p).is_some() {
                clips.push(p);
            }
        }
        let mut n = folders;
        if !clips.is_empty() {
            n += library::add_files(&self.lib, &clips);
        }
        if n > 0 {
            self.toast(
                format!("{n} {} added", if n == 1 { "clip" } else { "clips" }),
                true,
            );
        }
        library::probe_all(&self.lib, &self.probing);
    }

    pub fn add_folder_dialog(&mut self) {
        let mut d = rfd::FileDialog::new();
        if let Some(l) = &self.stored.last_dir {
            d = d.set_directory(l);
        }
        if let Some(dirs) = d.pick_folders() {
            if let Some(f) = dirs.first() {
                self.stored.last_dir = Some(f.to_string_lossy().to_string());
            }
            self.open_paths(dirs);
        }
    }
    pub fn add_files_dialog(&mut self) {
        let mut exts: Vec<&str> = Vec::new();
        exts.extend(library::VIDEO);
        exts.extend(library::IMAGE);
        if let Some(f) = rfd::FileDialog::new()
            .add_filter("Videos and images", &exts)
            .pick_files()
        {
            self.open_paths(f);
        }
    }

    // ---------- projects ----------

    pub fn open_project_dialog(&mut self) {
        let mut d = rfd::FileDialog::new().add_filter("PMV project", &[project::EXT]);
        if let Some(l) = &self.stored.last_dir {
            d = d.set_directory(l);
        }
        if let Some(p) = d.pick_file() {
            self.open_project(p);
        }
    }

    pub fn open_project(&mut self, p: PathBuf) {
        if self.opening.is_some() {
            return;
        }
        let rate = self.device_rate;
        self.stored.recent_projects.retain(|x| Path::new(x) != p);
        self.stored
            .recent_projects
            .insert(0, p.to_string_lossy().to_string());
        self.stored.recent_projects.truncate(8);
        self.opening = Some(Job::spawn(move |report, cancel| {
            report(0.0, "Reading the project …".into());
            let pf = project::read(&p)?;
            project::open(
                pf,
                rate,
                &|f| report(f, "Reading the song …".into()),
                cancel,
            )
        }));
    }

    // ---------- start / generate ----------

    pub fn clip_lists(&self) -> (Vec<Item>, Vec<Item>) {
        let l = self.lib.lk();
        (l.matching(&self.s), l.images())
    }

    /// The live show of the selected song (with several songs: one after another)
    pub fn go(&mut self) {
        let n = self.songs.len();
        let order: Vec<usize> = if self.mode == Mode::Template || n == 0 {
            vec![self.sel]
        } else {
            let mut o: Vec<usize> = (0..n).collect();
            if self.s.shuffle {
                use rand::seq::SliceRandom;
                o.shuffle(&mut rand::thread_rng());
                if let Some(p) = o.iter().position(|&i| i == self.sel) {
                    o.swap(0, p);
                }
            } else {
                o.rotate_left(self.sel.min(n.saturating_sub(1)));
            }
            o
        };
        self.start_live(order, 0);
    }

    pub fn start_live(&mut self, order: Vec<usize>, pos: usize) {
        let song = match self.mode {
            Mode::Template => self.current_song(),
            Mode::Song => order
                .get(pos)
                .and_then(|&i| self.song_ready(i))
                .map(Arc::new),
        };
        let Some(song) = song else {
            self.toast("The song isn't ready yet", false);
            return;
        };
        let (items, images) = self.clip_lists();
        let tpl = self.current_template();
        match show::live_session(song, &items, images, &self.s, tpl.as_deref()) {
            Ok(sess) => {
                let st = stage::Stage::new(sess, self.s.show_quality, self.pins(), items, 0.0);
                if let Some(e) = &st.player.device_error {
                    let e = e.clone();
                    self.toast(format!("No sound ({e}) – the show runs silently"), false);
                }
                let mut l = live::Live::new(st, order, pos);
                l.stage.want_play = true;
                self.screen = Screen::Live(Box::new(l));
            }
            Err(e) => self.toast(format!("{e:#}"), false),
        }
    }

    /// Generate a PMV (choose every clip) and open it in the studio
    pub fn generate(&mut self) {
        if self.gen.is_some() {
            return;
        }
        let Some(song) = self.current_song() else {
            self.toast("Pick a song (and wait until it is read) first", false);
            return;
        };
        let (items, images) = self.clip_lists();
        if items.is_empty() {
            self.toast(
                "No clips match the filters – add a folder with videos (or images) first",
                false,
            );
            return;
        }
        let s = self.s.clone();
        let pins = self.pins();
        let tpl = self.current_template();
        self.gen = Some(Job::spawn(move |report, cancel| {
            show::generate(
                song,
                items,
                images,
                &s,
                show::GenOpts {
                    pins,
                    template: tpl.as_deref(),
                    seed: None,
                    progress: &|f, m| report(f, m),
                    cancel,
                },
            )
        }));
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.gen.as_ref().and_then(|g| g.take()) {
            self.gen = None;
            match r {
                Ok(sess) => self.open_studio(sess),
                Err(e) if e == "Cancelled" => {}
                Err(e) => self.toast(e, false),
            }
        }
        if let Some(r) = self.opening.as_ref().and_then(|g| g.take()) {
            self.opening = None;
            match r {
                Ok(sess) => {
                    // the project's settings become the window's (its output settings stay as chosen here)
                    let mut s = sess.settings.clone();
                    s.encoder = self.s.encoder.clone();
                    s.hwaccel = self.s.hwaccel;
                    s.show_quality = self.s.show_quality;
                    s.glass = self.s.glass;
                    self.s = s;
                    self.open_studio(sess);
                    self.toast("Project opened", true);
                }
                Err(e) if e == "Cancelled" => {}
                Err(e) => self.toast(format!("The project can't be opened: {e}"), false),
            }
        }
        if let Some(t) = &self.tpl.scan {
            if let Some(r) = t.take() {
                self.tpl.scan = None;
                match r {
                    Ok((song, tpl)) if self.tpl.scanned == crate::template::Kind::Classic => {
                        let mut tpl = tpl;
                        tpl.name = self.tpl.name.clone();
                        let song = Arc::new(song);
                        let tpl = Arc::new(tpl);
                        self.tpl.raw = Some((song.clone(), tpl.clone()));
                        self.tpl.result = Some((song, tpl));
                        self.tpl.music = None;
                        self.tpl.start = 0.0;
                        self.tpl.fit = None;
                    }
                    Ok((song, tpl)) => {
                        let mut tpl = tpl;
                        tpl.name = self.tpl.name.clone();
                        let song = Arc::new(song);
                        let shown = tpl.shifted(0.0, song.duration());
                        self.tpl.raw = Some((song.clone(), Arc::new(tpl)));
                        self.tpl.result = Some((song, Arc::new(shown)));
                        self.tpl.music = Some(crate::template::Music::Pmv);
                        self.tpl.start = 0.0;
                        self.tpl.fit = None;
                        self.tpl.sync_text();
                    }
                    Err(e) if e == "Cancelled" => {}
                    Err(e) => self.toast(format!("Template: {e}"), false),
                }
            }
        }
        if let Some(t) = &self.tpl.load {
            if let Some(r) = t.take() {
                self.tpl.load = None;
                match r {
                    Ok((song, tpl, music, start, fit)) => {
                        self.tpl.result = Some((Arc::new(song), Arc::new(tpl)));
                        self.tpl.music = Some(music);
                        self.tpl.start = start;
                        if fit.is_some() {
                            self.tpl.fit = fit;
                        }
                        if let Some(f) = fit.filter(|f| !f.good()) {
                            self.toast(
                                format!(
                                    "The song was found at {} of the PMV, but not clearly ({} % of the PMV matches) – check the start",
                                    crate::util::fmt_ms(f.offset.max(0.0)),
                                    (f.agree * 100.0).round()
                                ),
                                false,
                            );
                        }
                    }
                    Err(e) if e == "Cancelled" => {}
                    Err(e) => self.toast(format!("Music: {e}"), false),
                }
                self.tpl.sync_text();
            }
        }
        if self.gen.is_some()
            || self.opening.is_some()
            || self.tpl.scan.is_some()
            || self.tpl.load.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // Start by itself: right after the song is read
        if self.s.auto_start
            && !self.auto_started
            && matches!(self.screen, Screen::Setup)
            && self.current_song().is_some()
            && self.ff_err.is_none()
        {
            self.auto_started = true;
            self.go();
        }
    }

    fn open_studio(&mut self, sess: Session) {
        if let Some(old) = self.studio.as_mut() {
            old.close();
        }
        self.studio = Some(Box::new(studio::Studio::new(sess, self)));
        self.screen = Screen::Studio;
    }

    pub fn scan_template(&mut self, path: PathBuf, name: String) {
        if self.tpl.scan.is_some() {
            return;
        }
        let rate = self.device_rate;
        self.tpl.name = name;
        self.tpl.result = None;
        self.tpl.raw = None;
        self.tpl.pmv = Some(path.clone());
        let kind = self.tpl.kind;
        self.tpl.scanned = kind;
        self.tpl.scan = Some(Job::spawn(move |report, cancel| {
            crate::template::scan(&path, rate, kind, &|f, m| report(f, m.to_string()), cancel)
        }));
    }

    /// PMV as template: use this music, beginning at PMV time `start`. `find`: look for where the song file
    /// plays in the PMV first (the start then follows it, unless the PMV starts in the middle of the song)
    pub fn template_music(&mut self, music: crate::template::Music, start: f64, find: bool) {
        let (Some(pmv), Some((_, raw))) = (self.tpl.pmv.clone(), self.tpl.raw.clone()) else {
            return;
        };
        if let Some(j) = &self.tpl.load {
            j.cancel.store(true, Ordering::SeqCst);
        }
        let rate = self.device_rate;
        self.tpl.load = Some(Job::spawn(move |report, cancel| {
            let mut music = music;
            let mut start = start;
            let mut fit = None;
            if let crate::template::Music::File { path, at } = &mut music {
                if find {
                    report(0.05, "Finding the song in the PMV …".into());
                    let f = crate::align::find(&pmv, path, cancel)?;
                    *at = f.offset;
                    // the new PMV begins where the song does – or at the PMV's start if it begins mid-song
                    start = f.offset.max(0.0);
                    fit = Some(f);
                }
                // (it can't begin before the song does)
                start = start.max(*at);
            }
            report(0.3, "Reading the music …".into());
            let (song, tpl) = crate::template::with_music(
                &pmv,
                &raw,
                &music,
                start,
                rate,
                &|p| report(0.3 + 0.7 * p, "Reading the music …".into()),
                cancel,
            )?;
            Ok((song, tpl, music, start, fit))
        }));
    }

    // ---------- the window's scale ----------

    /// Adaptive scaling: the screen's own scale × the window's size (a small window gets a bit smaller text, a
    /// big monitor a bit bigger) × the zoom chosen with Ctrl + / Ctrl − (Ctrl 0 resets it)
    fn scale(&mut self, ctx: &egui::Context) {
        let (inc, dec, reset) = ctx.input_mut(|i| {
            let c = egui::Modifiers::COMMAND;
            (
                i.consume_key(c, egui::Key::Equals) || i.consume_key(c, egui::Key::Plus),
                i.consume_key(c, egui::Key::Minus),
                i.consume_key(c, egui::Key::Num0),
            )
        });
        let mut user = self.stored.ui_zoom.unwrap_or(1.0);
        if inc {
            user = (user + 0.1).min(2.5);
        }
        if dec {
            user = (user - 0.1).max(0.5);
        }
        if reset {
            user = 1.0;
        }
        if inc || dec || reset {
            self.stored.ui_zoom = Some((user * 10.0).round() / 10.0);
            self.persist(true);
            self.toast(format!("Zoom {:.0} %", user * 100.0), true);
        }
        let native = ctx.native_pixels_per_point().unwrap_or(1.0).max(0.25);
        let phys_w = ctx.screen_rect().width() * ctx.pixels_per_point();
        let logical_w = phys_w / native;
        // 1280–1600 wide: as designed; narrower down to 0.85, wider up to 1.35
        let auto = if logical_w < 1280.0 {
            (logical_w / 1280.0).max(0.85)
        } else if logical_w > 1600.0 {
            (logical_w / 1600.0).powf(0.6).min(1.35)
        } else {
            1.0
        };
        let z = (auto * user).clamp(0.5, 3.0);
        if (z - self.applied_zoom).abs() > 0.01 {
            self.applied_zoom = z;
            ctx.set_zoom_factor(z);
        }
    }

    // ---------- developer screenshots ----------

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, n)) = self.shot.as_mut() else {
            return;
        };
        *n += 1;
        ctx.request_repaint();
        let wait: u32 = std::env::var("PMVGEN_SHOT_FRAMES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let long = self.shot_long;
        if long {
            // scroll, wait for the picture to settle, capture; until the end of the page
            if *n == wait {
                self.shot_scroll = Some(0.0);
                self.shot_next = *n + 25;
            }
            if *n == self.shot_next {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        } else if *n == wait {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let mut bytes = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                bytes.extend_from_slice(&p.to_array());
            }
            if long {
                let off = self.shot_scroll.unwrap_or(0.0);
                let (content, view, top) = self.shot_page.unwrap_or((0.0, 1.0, 0.0));
                let ppp = ctx.pixels_per_point();
                let out = path.with_extension(format!(
                    "part{:06}.{w}x{h}.{}.{}.{}.{}.rgba",
                    (off * ppp) as u32,
                    (top * ppp) as u32,
                    (view * ppp) as u32,
                    (content * ppp) as u32,
                    (off * ppp) as u32
                ));
                let _ = std::fs::write(&out, bytes);
                eprintln!("[pmvgen] part {}", out.display());
                if off + view >= content - 1.0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    self.shot_scroll = Some((off + view * 0.85).min(content - view));
                    self.shot_next = *n + 25;
                }
                return;
            }
            let out = path.with_extension(format!("{w}x{h}.rgba"));
            let _ = std::fs::write(&out, bytes);
            eprintln!("[pmvgen] screenshot {}", out.display());
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if !long && *n > wait + 600 || *n > wait + 6000 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    // ---------- toasts, gate ----------

    fn paint_toasts(&mut self, ctx: &egui::Context) {
        self.toasts
            .retain(|t| t.at.elapsed() < Duration::from_secs(if t.ok { 4 } else { 9 }));
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let mut close = None;
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -24.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for (i, t) in self.toasts.iter().enumerate() {
                    let r = egui::Frame::new()
                        .fill(if glass() {
                            alpha(c().bg2, 0.92)
                        } else {
                            c().bg2
                        })
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            if t.ok { c().bg3 } else { c().danger },
                        ))
                        .corner_radius(if glass() { 255 } else { 6 })
                        .inner_margin(egui::Margin::symmetric(16, 10))
                        .shadow(egui::Shadow {
                            offset: [0, 8],
                            blur: 24,
                            spread: 0,
                            color: Color32::from_black_alpha(120),
                        })
                        .show(ui, |ui| {
                            ui.set_max_width(560.0);
                            ui.horizontal(|ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
                                icon(
                                    ui.painter(),
                                    r,
                                    if t.ok { Icon::Check } else { Icon::Info },
                                    if t.ok { c().ok } else { c().danger },
                                );
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&t.msg).color(c().text))
                                        .wrap(),
                                );
                            });
                        });
                    if r.response.interact(egui::Sense::click()).clicked() {
                        close = Some(i);
                    }
                    ui.add_space(6.0);
                }
            });
        if let Some(i) = close {
            self.toasts.remove(i);
        }
    }

    /// "Before you start" (the web version asks the same before it opens)
    fn gate(&mut self, ctx: &egui::Context, required: bool) -> bool {
        let mut ok = false;
        let mut leave = false;
        egui::Area::new(egui::Id::new("gate-veil"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                let r = ctx.screen_rect();
                ui.painter()
                    .rect_filled(r, 0.0, themes::rgba(20, 8, 18, 235));
                ui.allocate_rect(r, egui::Sense::click());
            });
        egui::Area::new(egui::Id::new("gate"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let resp = egui::Frame::new()
                    .fill(c().bg2)
                    .stroke(egui::Stroke::new(1.0_f32, c().bg3))
                    .corner_radius(8)
                    .inner_margin(egui::Margin {
                        left: 28,
                        right: 28,
                        top: 26,
                        bottom: 22,
                    })
                    .shadow(egui::Shadow {
                        offset: [0, 20],
                        blur: 60,
                        spread: 0,
                        color: Color32::from_black_alpha(128),
                    })
                    .show(ui, |ui| {
                        ui.set_width(ctx.screen_rect().width().min(640.0) - 56.0);
                        ui.label(egui::RichText::new("Before you start").font(display(30.0)).color(c().text));
                        ui.add_space(4.0);
                        let body = |ui: &mut egui::Ui, parts: &[(&str, bool)]| {
                            let mut job = egui::text::LayoutJob::default();
                            for (t, b) in parts {
                                job.append(
                                    t,
                                    0.0,
                                    egui::TextFormat {
                                        font_id: if *b { bold(14.5) } else { text(14.5) },
                                        color: if *b { c().text } else { c().text2 },
                                        ..Default::default()
                                    },
                                );
                            }
                            job.wrap.max_width = ui.available_width();
                            ui.label(job);
                        };
                        body(
                            ui,
                            &[
                                ("This is a tool for making music edits from ", false),
                                ("your own", true),
                                (" media. By entering you confirm that:", false),
                            ],
                        );
                        let items: [(&str, &str); 5] = [
                            ("You are 18 or older", " (or of legal age where you live)."),
                            ("You only use media you own or have permission to use", " – videos, images and music."),
                            ("You never use content showing anyone under 18.", ""),
                            ("You never use footage of real people without their consent", " – in particular, you don't create or share sexual content of anyone who hasn't agreed to it."),
                            ("You follow the law", " where you live, including copyright and privacy law."),
                        ];
                        for (i, (b, t)) in items.iter().enumerate() {
                            ui.horizontal_top(|ui| {
                                ui.label(egui::RichText::new(format!("{}.", i + 1)).font(bold(14.5)).color(c().accent));
                                ui.vertical(|ui| body(ui, &[(b, true), (t, false)]));
                            });
                        }
                        egui::Frame::new()
                            .fill(c().base)
                            .corner_radius(6)
                            .inner_margin(egui::Margin::symmetric(14, 12))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                let mut job = egui::text::LayoutJob::default();
                                for (t, b) in [
                                    ("Everything runs on your device: nothing is uploaded, and the author can't see, store or control what you make. ", false),
                                    ("You alone are responsible for what you create and share with this tool.", true),
                                    (" The author doesn't condone any misuse and accepts no liability for it. The software is provided \u{201c}as is\u{201d}, without warranty of any kind (MIT license).", false),
                                ] {
                                    job.append(t, 0.0, egui::TextFormat {
                                        font_id: if b { bold(13.0) } else { text(13.0) },
                                        color: if b { c().text } else { c().text2 },
                                        ..Default::default()
                                    });
                                }
                                job.wrap.max_width = ui.available_width();
                                ui.label(job);
                            });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if required {
                                if btn(ui, Btn::Primary, None, "I agree – enter", true).clicked() {
                                    ok = true;
                                }
                                if btn(ui, Btn::Ghost, None, "Leave", true).clicked() {
                                    leave = true;
                                }
                            } else if btn(ui, Btn::Primary, None, "Close", true).clicked() {
                                ok = true;
                            }
                        });
                    });
                let _ = resp;
            });
        if leave {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ok
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.applied_theme != self.s.theme {
            self.applied_theme = self.s.theme.clone();
            let pal = themes::by_id(&self.s.theme);
            apply_palette(ctx, &pal);
        }
        self.scale(ctx);
        set_glass(self.s.glass);
        self.poll_jobs(ctx);
        // dropped files (not during a show)
        if !matches!(self.screen, Screen::Live(_)) {
            let dropped: Vec<PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });
            if !dropped.is_empty() {
                self.open_paths(dropped);
            }
        }
        let screen = std::mem::replace(&mut self.screen, Screen::Setup);
        self.screen = match screen {
            Screen::Live(mut l) => {
                if l.ui(ctx, self) {
                    // (ended: back to where it was started from)
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                    l.stage.shutdown();
                    if self.studio.is_some() && l.from_studio {
                        Screen::Studio
                    } else {
                        Screen::Setup
                    }
                } else if let Some((order, pos)) = l.next_song.take() {
                    l.stage.shutdown();
                    drop(l);
                    self.start_live(order, pos);
                    std::mem::replace(&mut self.screen, Screen::Setup)
                } else {
                    Screen::Live(l)
                }
            }
            Screen::Studio => {
                let mut st = self.studio.take();
                let r = match st.as_mut() {
                    Some(s) => s.ui(ctx, self),
                    None => studio::Next::Setup,
                };
                self.studio = st;
                match r {
                    studio::Next::Stay => Screen::Studio,
                    studio::Next::Setup => {
                        if let Some(s) = self.studio.as_mut() {
                            s.pause();
                        }
                        Screen::Setup
                    }
                    studio::Next::Live(l) => Screen::Live(l),
                }
            }
            Screen::Setup => {
                setup::page(ctx, self);
                Screen::Setup
            }
        };
        if !self.gate_ok {
            if self.gate(ctx, true) {
                self.gate_ok = true;
                self.persist(true);
            }
        } else if self.ui.show_terms && self.gate(ctx, false) {
            self.ui.show_terms = false;
        }
        self.paint_toasts(ctx);
        self.persist(false);
        self.screenshot(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.shot.is_some() {
            return;
        }
        if let Screen::Live(l) = &mut self.screen {
            l.stage.shutdown();
        }
        if let Some(s) = self.studio.as_mut() {
            s.close();
        }
        for e in &self.songs {
            e.cancel.store(true, Ordering::SeqCst);
        }
        if let Some(j) = &self.gen {
            j.cancel.store(true, Ordering::SeqCst);
        }
        self.persist(true);
    }
}

pub fn run(open: Vec<PathBuf>) -> anyhow::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PMV Generator")
            .with_maximized(std::env::var_os("PMVGEN_SHOT_MAX").is_some())
            .with_inner_size(
                std::env::var("PMVGEN_SHOT_SIZE")
                    .ok()
                    .and_then(|v| {
                        let (w, h) = v.split_once('x')?;
                        Some([w.parse().ok()?, h.parse().ok()?])
                    })
                    .unwrap_or([1360.0, 900.0]),
            )
            .with_min_inner_size([560.0, 480.0])
            .with_icon(app_icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "PMV Generator",
        opts,
        Box::new(move |cc| {
            let mut app = App::new(cc);
            app.open_paths(open);
            // (developer screenshots: a template scanned right away)
            if let Some(t) = std::env::var_os("PMVGEN_SHOT_TPL") {
                let p = PathBuf::from(t);
                let n = crate::song::song_name(&p);
                app.scan_template(p, n);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Is this a video of the library that could be a template (names with "PMV" first)?
pub fn template_candidates(lib: &library::Library, q: &str) -> Vec<Item> {
    let ql = q.to_lowercase();
    let mut v: Vec<Item> = lib
        .items
        .iter()
        .filter(|i| i.kind == Kind::Video && i.bad.is_none())
        .filter(|i| ql.is_empty() || i.path.to_string_lossy().to_lowercase().contains(&ql))
        .cloned()
        .collect();
    v.sort_by(|a, b| {
        let pa = !a.name.to_lowercase().contains("pmv");
        let pb = !b.name.to_lowercase().contains("pmv");
        pa.cmp(&pb).then_with(|| a.name.cmp(&b.name))
    });
    v.truncate(12);
    v
}
