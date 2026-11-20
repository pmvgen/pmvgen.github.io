// The setup page: 1 Music · 2 Clips · 3 Style (sections) · 4 Go – the web version's layout, labels and hints.

use super::theme::*;
use super::themes::mix;
use super::{template_candidates, App, Load, Mode, Screen};
use crate::library::{self, Kind};
use crate::plan::LAYOUTS;
use crate::settings::{self, ClipSkip, Settings, PRESETS};
use crate::song::{self, Song};
use crate::template::layout_color;
use crate::util::{fmt_time, parse_time, LockExt};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// The style sections: key, name, subtitle (SECTIONS of the web version – without Mouse control)
pub const SECTIONS: [(&str, &str, &str); 8] = [
    ("cut", "Cutting", "When to cut and which split screens"),
    ("fx", "Effects", "What happens on cuts, beats and drops"),
    (
        "pics",
        "Pictures",
        "Images that flash between the clips or lie over them",
    ),
    (
        "look",
        "Color & look",
        "Color mood, brightness and film effects",
    ),
    ("frame", "Picture & frame", "Format, fit, seams and edges"),
    ("sound", "Sound", "Song and clip volume"),
    (
        "fs",
        "Funscript",
        "A script for The Handy that follows the song",
    ),
    ("out", "Output", "Intro, outro and export"),
];

const FX_GROUPS: [(&str, &str, &[&str]); 3] = [
    (
        "On cuts",
        "When a new clip comes in",
        &["whip", "zoomin", "flash"],
    ),
    (
        "On the beat",
        "Run through the whole song",
        &["zoom", "shake", "speed", "stutter", "strobe"],
    ),
    (
        "On drops",
        "The big moments",
        &["rgb", "glitch", "tunnel", "invert", "echo", "text"],
    ),
];
const LOOK_FX: [&str; 4] = ["hue", "vhs", "kenburns", "lines"];
const LOOKS: [(&str, &str); 8] = [
    ("none", "Original"),
    ("warm", "Warm"),
    ("pink", "Pink"),
    ("cold", "Cold"),
    ("vivid", "Vivid"),
    ("bw", "Black & white"),
    ("noir", "Noir"),
    ("custom", "Own color"),
];
const CUTS: [(&str, &str, &str); 4] = [
    (
        "auto",
        "Automatic",
        "Calm: every 4 beats · medium: every 2 · loud: every beat",
    ),
    ("1", "Every beat", ""),
    ("2", "Every 2 beats", ""),
    ("4", "Every 4 beats", ""),
];

pub fn fx_text(k: &str) -> (&'static str, &'static str) {
    match k {
        "flash" => ("Flash", "Bright flash on cuts and drops"),
        "zoom" => ("Zoom pulse", "The picture pumps on every beat"),
        "whip" => (
            "Transitions",
            "New clips whip into the field with motion blur",
        ),
        "zoomin" => (
            "Zoom-in entry",
            "New clips zoom into the picture fast – always zooming in, alternating strong and soft",
        ),
        "speed" => (
            "Speed ramps",
            "Slow motion in calm parts, faster when it gets loud, a burst on drops",
        ),
        "voice" => (
            "Clip audio",
            "Mix in the clips' original audio – only on drops or all the time",
        ),
        "rgb" => (
            "RGB split",
            "Red and cyan tear apart – really hard on drops",
        ),
        "echo" => ("Echo", "The last frame lingers as a ghost (loud parts)"),
        "tunnel" => (
            "Tunnel",
            "On drops the picture sits inside itself three times",
        ),
        "invert" => ("Negative", "Short negative on drops"),
        "glitch" => ("Glitch", "Picture stripes slip on drops"),
        "stutter" => (
            "Stutter",
            "Clips jump back on half beats – a classic PMV effect",
        ),
        "shake" => ("Shake", "Shakes in loud parts"),
        "hue" => ("Color rush", "Colors rotate to the beat"),
        "vhs" => ("VHS", "Noise, scanlines and dark corners"),
        "strobe" => (
            "Strobe",
            "White flashes on half beats – careful if you are sensitive to light",
        ),
        "text" => ("Text", "Your words slam into the picture on drops"),
        "kenburns" => (
            "Image drift",
            "Still images zoom and glide slowly (Ken Burns)",
        ),
        "lines" => (
            "Glowing dividers",
            "The lines between fields flash pink to the beat",
        ),
        _ => ("?", ""),
    }
}

fn pct(v: f64) -> String {
    format!("{} %", v.round() as i64)
}

/// Section summaries (tabSum of the web version)
pub fn tab_sum(s: &Settings, key: &str) -> String {
    let fx_on = settings::FX_KEYS
        .iter()
        .filter(|k| {
            let mut f = s.fx.clone();
            *f.get_mut(k) && !LOOK_FX.contains(k) && **k != "voice"
        })
        .count();
    let lays = settings::LAYOUT_IDS
        .iter()
        .filter(|k| s.layouts.get(k))
        .count();
    match key {
        "cut" => format!(
            "{} · {} {}",
            CUTS.iter()
                .find(|c| c.0 == s.cut)
                .map(|c| c.1)
                .unwrap_or("?"),
            lays,
            if lays == 1 { "layout" } else { "layouts" }
        ),
        "fx" => format!("{fx_on} on"),
        "pics" => {
            let v: Vec<&str> = [(s.pic_flash, "flash"), (s.pic_overlay, "overlay")]
                .iter()
                .filter(|x| x.0)
                .map(|x| x.1)
                .collect();
            if v.is_empty() {
                "off".into()
            } else {
                v.join(" · ")
            }
        }
        "look" => LOOKS
            .iter()
            .find(|l| l.0 == s.look)
            .map(|l| l.1.to_string())
            .unwrap_or_default(),
        "frame" => format!(
            "{} · {}{}",
            s.format,
            if s.fit == "cover" { "fill" } else { "fit" },
            if s.soft { " · soft seams" } else { "" }
        ),
        "sound" => format!(
            "Song {} · Clips {}",
            s.song_vol.round(),
            if s.fx.voice {
                format!("{}", s.clip_vol.round())
            } else {
                "off".into()
            }
        ),
        "fs" => {
            if s.fs_on {
                format!(
                    "{} · {}",
                    if s.fs_pace == "auto" {
                        "follows the song"
                    } else {
                        &s.fs_pace
                    },
                    if s.fs_size == "auto" {
                        "auto size".to_string()
                    } else {
                        s.fs_size.clone()
                    }
                )
            } else {
                "off".into()
            }
        }
        "out" => {
            let mut v: Vec<String> = Vec::new();
            if s.intro {
                v.push("Intro".into());
            }
            if s.outro {
                v.push("Outro".into());
            }
            v.push(format!(
                "{} · {}p",
                settings::CODECS
                    .iter()
                    .find(|c| c.0 == s.codec)
                    .map(|c| c.1)
                    .unwrap_or("?"),
                s.quality
            ));
            v.join(" · ")
        }
        _ => String::new(),
    }
}

// ---------- the page ----------

pub fn page(ctx: &egui::Context, app: &mut App) {
    let screen = ctx.screen_rect();
    let wide = screen.width() >= 1100.0;
    // the background (behind everything)
    let bgp = ctx.layer_painter(egui::LayerId::background());
    app.bg.paint(ctx, &bgp, screen);
    if !wide {
        // narrow window: the Go card stays at the bottom (.kb-pmvg-go sticky bottom, without its summary)
        egui::TopBottomPanel::bottom("go-bottom")
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .fill(Color32::TRANSPARENT),
            )
            .show(ctx, |ui| {
                go_card(ui, app, true);
            });
    }
    let mut row_top = None;
    let mut main_w = 0.0;
    let out = egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(Color32::TRANSPARENT))
        .show(ctx, |ui| {
            let mut sa = egui::ScrollArea::vertical()
                .id_salt("setup-scroll")
                .auto_shrink([false, false]);
            if let Some(y) = app.shot_scroll {
                sa = sa.vertical_scroll_offset(y);
            }
            let o = sa.show(ui, |ui| {
                let full = ui.available_width();
                let pad = (full * 0.03).clamp(16.0, 40.0);
                let content = (full - 2.0 * pad).min(1500.0);
                let side = ((full - content) / 2.0).max(pad);
                ui.horizontal(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_width(content);
                        top_bar(ui, app);
                        header(ui, app);
                        ui.add_space(8.0);
                        let rail = if wide { 320.0 } else { 0.0 };
                        main_w = content - if wide { rail + 16.0 } else { 0.0 };
                        row_top = Some(ui.cursor().top());
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(main_w);
                                ui.spacing_mut().item_spacing.y = 16.0;
                                music_card(ui, app);
                                clips_card(ui, app);
                                style_card(ui, app);
                                ui.add_space(17.0);
                            });
                        });
                    });
                });
            });
            app.shot_page = Some((o.content_size.y, o.inner_rect.height(), o.inner_rect.top()));
        });
    // the Go card: beside the cards, sticky at 16 px from the top while scrolling
    if wide {
        let inner = out.response.rect;
        let full = inner.width();
        let pad = (full * 0.03).clamp(16.0, 40.0);
        let content = (full - 2.0 * pad).min(1500.0);
        let side = ((full - content) / 2.0).max(pad);
        let x = inner.left() + side + content - 320.0;
        let y = row_top
            .unwrap_or(inner.top() + 120.0)
            .max(inner.top() + 16.0);
        // (a whole-page developer screenshot: the card scrolls with the page instead of sticking)
        let y = if app.shot_long {
            row_top.unwrap_or(y)
        } else {
            y
        };
        egui::Area::new(egui::Id::new("go-card"))
            .constrain(!app.shot_long)
            .fixed_pos(Pos2::new(x, y))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.set_max_height(inner.bottom() - y - 8.0);
                egui::ScrollArea::vertical()
                    .id_salt("go-scroll")
                    .max_height(inner.bottom() - y - 8.0)
                    .show(ui, |ui| go_card(ui, app, false));
            });
    }
}

fn top_bar(ui: &mut Ui, app: &mut App) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
        icon(ui.painter(), r, Icon::Heart, c().accent);
        ui.label(
            egui::RichText::new("PMV Generator")
                .font(display(18.0))
                .color(c().text),
        );
        // "Runs locally – no uploads"
        let g = ui.painter().layout_no_wrap(
            "Runs locally – no uploads".into(),
            medium(12.5),
            c().text2,
        );
        let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 40.0, 26.0), Sense::hover());
        let p = ui.painter();
        p.rect_stroke(
            r,
            255,
            Stroke::new(1.0_f32, c().bg3),
            egui::StrokeKind::Inside,
        );
        icon(
            p,
            Rect::from_center_size(Pos2::new(r.left() + 17.0, r.center().y), Vec2::splat(14.0)),
            Icon::Check,
            c().ok,
        );
        p.galley(
            Pos2::new(r.left() + 30.0, r.center().y - g.size().y / 2.0),
            g,
            c().text2,
        );
        resp.on_hover_text(
            "Songs and clips are opened straight from your disk. Nothing is sent anywhere.",
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let link = |ui: &mut Ui, t: &str| {
                ui.add(
                    egui::Label::new(egui::RichText::new(t).font(medium(13.0)).color(c().faint))
                        .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
            };
            if link(ui, "Source").clicked() {
                super::open_url("https://github.com/pmvgen/pmvgen.github.io");
            }
            if link(ui, "Terms").clicked() {
                app.ui.show_terms = true;
            }
            if link(ui, "Open project …").clicked() {
                app.open_project_dialog();
            }
        });
    });
    if let Some(e) = &app.ff_err {
        ui.add_space(8.0);
        tip(ui, e);
    }
}

fn header(ui: &mut Ui, app: &mut App) {
    ui.add_space(14.0);
    let w = ui.available_width();
    // (clamp(36px, 4.8vw, 62px) – of the window's width)
    let size = (ui.ctx().screen_rect().width() * 0.048).clamp(36.0, 62.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width((w - 400.0).max(200.0));
            ui.label(egui::RichText::new("PMV Generator").font(display(size)).color(c().text));
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Pick a song and a folder of clips – on every beat it cuts to the next one. Live, generated and edited, or exported as a video. Everything runs on your computer: your files are never uploaded.",
                    )
                    .font(text(16.0))
                    .color(c().text2),
                )
                .wrap(),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Liquid glass").font(medium(13.5)).color(c().text2));
            if switch(ui, &mut app.s.glass).changed() {
                set_glass(app.s.glass);
            }
            ui.add_space(14.0);
            let opts: Vec<(String, String)> = super::themes::THEMES
                .iter()
                .map(|(k, n)| (k.to_string(), n.to_string()))
                .collect();
            select(ui, "theme", &mut app.s.theme, &opts, 170.0);
        });
    });
}

// ---------- 1 Music ----------

fn draw_wave(ui: &mut Ui, song: &Song) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 70.0), Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, if glass() { 10.0 } else { 4.0 }, alpha(c().base, 0.8));
    let peaks = &song.a.peaks;
    let n = peaks.len().max(1);
    let max = peaks.iter().cloned().fold(0.0001f32, f32::max);
    for (i, v) in peaks.iter().enumerate() {
        let x = r.left() + r.width() * i as f32 / n as f32;
        let h = (v / max) * (r.height() * 0.42);
        p.line_segment(
            [
                Pos2::new(x, r.center().y - h),
                Pos2::new(x, r.center().y + h),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(110, 58, 99)),
        );
    }
    let dur = song.duration().max(0.01);
    for (k, b) in song.a.beats.iter().enumerate() {
        let x = r.left() + r.width() * (*b / dur) as f32;
        let (bar, phrase) = match &song.bars {
            Some(bb) => (
                bb.downbeat.get(k).copied().unwrap_or(false),
                bb.phrase.get(k).copied().unwrap_or(false),
            ),
            None => (k % 4 == 0, k % 16 == 0),
        };
        let (c, hh) = if phrase {
            (c().accent, 1.0)
        } else if bar {
            (alpha(c().accent, 0.6), 0.6)
        } else {
            (white(40), 0.3)
        };
        p.line_segment(
            [
                Pos2::new(x, r.bottom()),
                Pos2::new(x, r.bottom() - r.height() * hh),
            ],
            Stroke::new(1.0_f32, c),
        );
    }
}

fn pick_songs(app: &mut App, replace: bool) {
    let mut d = rfd::FileDialog::new()
        .add_filter("Music", &super::AUDIO_EXT)
        .add_filter("Video (its music)", &library::VIDEO);
    if let Some(l) = &app.stored.last_dir {
        d = d.set_directory(l);
    }
    if let Some(f) = d.pick_files() {
        if replace {
            for e in &app.songs {
                e.cancel.store(true, Ordering::SeqCst);
            }
            app.songs.clear();
            app.sel = 0;
        }
        if let Some(p) = f.first().and_then(|p| p.parent()) {
            app.stored.last_dir = Some(p.to_string_lossy().to_string());
        }
        for p in f {
            app.add_song(p);
        }
    }
}

fn music_card(ui: &mut Ui, app: &mut App) {
    card(ui, Some("1"), "Music", |ui| {
        let mut mode = app.mode;
        if seg(
            ui,
            &mut mode,
            &[
                (Mode::Song, "Your song", ""),
                (
                    Mode::Template,
                    "PMV as template",
                    "Take music, cuts, layouts and flashes from an existing PMV",
                ),
            ],
        ) {
            app.mode = mode;
        }
        match app.mode {
            Mode::Song => song_pane(ui, app),
            Mode::Template => template_pane(ui, app),
        }
    });
}

fn song_pane(ui: &mut Ui, app: &mut App) {
    if app.songs.is_empty() {
        let over = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        if drop_zone(
            ui,
            Icon::Music,
            "Drop a song here",
            "or click · MP3, M4A, WAV, OGG, FLAC – or a video, then its music is used · several songs or a folder: they play one after another",
            over,
            false,
        )
        .clicked()
        {
            pick_songs(app, false);
        }
        return;
    }
    let sel = app.sel.min(app.songs.len() - 1);
    app.sel = sel;
    // the selected song's state
    let (name, state) = {
        let e = &app.songs[sel];
        let st = e.state.lk();
        let s = match &*st {
            Load::Loading(p) => Err(format!("Reading … {:.0} %", p * 100.0)),
            Load::Ready(s) => Ok(s.clone()),
            Load::Failed(m) => Err(format!("⚠ {m}")),
        };
        (song::song_name(&e.path), s)
    };
    match state {
        Err(msg) => {
            let loading = msg.starts_with("Reading");
            drop_zone(ui, Icon::Music, &name, &msg, false, loading);
            if loading {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        Ok(song) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(&song.name)
                        .font(display(22.0))
                        .color(c().text),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{} BPM · {} · {} beats{}",
                        song.a.bpm.round(),
                        fmt_time(song.duration()),
                        song.a.beats.len(),
                        if song.shift_ms != 0.0 {
                            format!(" · {:+.0} ms", song.shift_ms)
                        } else {
                            String::new()
                        }
                    ))
                    .font(medium(14.0))
                    .color(c().accent2),
                );
            });
            draw_wave(ui, &song);
        }
    }
    // several songs
    if app.songs.len() > 1 {
        hint(
            ui,
            &format!("{} songs – they play one after another", app.songs.len()),
        );
        let mut remove = None;
        for (i, e) in app.songs.iter().enumerate() {
            let st = match &*e.state.lk() {
                Load::Loading(p) => format!("reading … {:.0} %", p * 100.0),
                Load::Ready(s) => format!("{:.0} BPM · {}", s.a.bpm, fmt_time(s.duration())),
                Load::Failed(m) => format!("⚠ {m}"),
            };
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        app.sel == i,
                        egui::RichText::new(song::song_name(&e.path)).font(medium(14.0)),
                    )
                    .clicked()
                {
                    app.sel = i;
                }
                ui.label(egui::RichText::new(st).font(text(12.5)).color(c().faint));
                if x_btn(ui, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            app.songs[i].cancel.store(true, Ordering::SeqCst);
            app.songs.remove(i);
            if app.sel >= app.songs.len() {
                app.sel = app.songs.len().saturating_sub(1);
            }
        }
        opt(ui, &mut app.s.shuffle, "Shuffle", "");
    }
    // tempo and shift
    let ready = app.song_ready(app.sel);
    ui.horizontal_wrapped(|ui| {
        let en = ready.is_some();
        if btn(ui, Btn::Normal, None, "½ tempo", en)
            .on_hover_text("If detected twice as fast")
            .clicked()
        {
            app.with_song(|s| s.rescale(0.5));
        }
        if btn(ui, Btn::Normal, None, "2× tempo", en)
            .on_hover_text("If detected half as fast")
            .clicked()
        {
            app.with_song(|s| s.rescale(2.0));
        }
        if btn(ui, Btn::Ghost, None, "Earlier", en)
            .on_hover_text("Cuts 20 ms earlier")
            .clicked()
        {
            app.with_song(|s| s.nudge(-20.0));
        }
        if btn(ui, Btn::Ghost, None, "Later", en)
            .on_hover_text("Cuts 20 ms later")
            .clicked()
        {
            app.with_song(|s| s.nudge(20.0));
        }
        if ready
            .as_ref()
            .is_some_and(|s| s.tempo != 1.0 || s.shift_ms != 0.0)
            && btn(ui, Btn::Ghost, None, "Reset", true).clicked()
        {
            app.with_song(|s| s.reset_grid());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if btn(ui, Btn::Ghost, None, "Other song", true).clicked() {
                pick_songs(app, true);
            }
            if btn(ui, Btn::Ghost, Some(Icon::Plus), "Add songs", true).clicked() {
                pick_songs(app, false);
            }
        });
    });
    // only a part of the song
    if let Some(song) = ready {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Only use")
                    .font(medium(14.0))
                    .color(c().text2),
            );
            let e = &mut app.songs[app.sel];
            field(ui, &mut e.from, "0:00", 80.0);
            ui.label("–");
            let end_hint = fmt_time(song.from + song.duration());
            field(ui, &mut e.to, &end_hint, 80.0);
            let cut = btn(ui, Btn::Normal, None, "Cut", true)
                .on_hover_text("Take just a part of the song (e.g. without the intro)")
                .clicked();
            let trimmed = song.from > 0.0 || song.to.is_some();
            let whole = trimmed && btn(ui, Btn::Ghost, None, "Whole song again", true).clicked();
            if cut || whole {
                let (f, t) = if whole {
                    (Some(0.0), Some(0.0))
                } else {
                    (parse_time(&e.from), parse_time(&e.to))
                };
                match (f, t) {
                    (Some(f), Some(t)) => {
                        let to = (t > 0.0).then_some(t);
                        if to.is_some_and(|t| t <= f + 2.0) {
                            app.toast("From/to don't fit – e.g. 0:45 to 3:30", false);
                        } else {
                            if whole {
                                e.from.clear();
                                e.to.clear();
                            }
                            let (from, to) = if whole { (0.0, None) } else { (f, to) };
                            let e2 = super::SongEntry {
                                path: e.path.clone(),
                                from: e.from.clone(),
                                to: e.to.clone(),
                                state: e.state.clone(),
                                cancel: e.cancel.clone(),
                            };
                            app.start_load(&e2, from, to);
                        }
                    }
                    _ => app.toast("From/to don't fit – e.g. 0:45 to 3:30", false),
                }
            }
        });
    }
}

fn template_timeline(ui: &mut Ui, tpl: &crate::template::Template, dur: f64) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 84.0), Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, if glass() { 10.0 } else { 4.0 }, alpha(c().base, 0.8));
    let d = dur.max(0.01);
    let x = |t: f64| r.left() + r.width() * (t / d) as f32;
    for sg in &tpl.segs {
        let c = layout_color(&sg.layout);
        p.rect_filled(
            Rect::from_min_max(
                Pos2::new(x(sg.t), r.top() + 16.0),
                Pos2::new((x(sg.end) - 1.0).max(x(sg.t) + 1.0), r.bottom() - 16.0),
            ),
            0.0,
            super::themes::rgba(c[0], c[1], c[2], 217),
        );
    }
    for ev in &tpl.events {
        let xx = x(ev.t());
        match ev {
            crate::template::TplEvent::Flash { color, .. } => {
                p.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(xx - 1.0, r.bottom() - 14.0),
                        Vec2::new(3.0, 14.0),
                    ),
                    0.0,
                    Color32::from_rgb(color[0], color[1], color[2]),
                );
            }
            crate::template::TplEvent::Layout { .. } => {
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(xx, r.top()), Vec2::new(2.0, 15.0)),
                    0.0,
                    c().text,
                );
            }
            crate::template::TplEvent::Cut { .. } => {
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(xx, r.top()), Vec2::new(1.0, 10.0)),
                    0.0,
                    alpha(c().text, 0.55),
                );
            }
        }
    }
}

/// PMV as template: which music, and where in the PMV it starts (a time or a frame)
fn template_music(ui: &mut Ui, app: &mut App) {
    use crate::template::Music;
    let Some((_, raw)) = app.tpl.raw.clone() else {
        return;
    };
    let fps = raw.fps;
    let busy = app.tpl.load.is_some();
    let music = app.tpl.music.clone().unwrap_or(Music::Pmv);
    let mut choice = matches!(music, Music::File { .. });
    lab(
        ui,
        "Music",
        "the PMV's own, or the same song as a file – where it plays in the PMV is found by itself",
    );
    let was = choice;
    ui.add_enabled_ui(!busy, |ui| {
        seg(
            ui,
            &mut choice,
            &[
                (
                    false,
                    "From the PMV",
                    "The PMV's soundtrack, from the start below on",
                ),
                (
                    true,
                    "Song file",
                    "The same song as a clean file – lined up with the PMV's soundtrack",
                ),
            ],
        );
    });
    let pick_song = || {
        rfd::FileDialog::new()
            .add_filter("Music", &super::AUDIO_EXT)
            .add_filter("Video (its music)", &library::VIDEO)
            .pick_file()
    };
    if choice != was {
        if choice {
            if let Some(f) = pick_song() {
                app.template_music(Music::File { path: f, at: 0.0 }, app.tpl.start, true);
            }
        } else {
            app.tpl.fit = None;
            app.template_music(Music::Pmv, app.tpl.start, false);
        }
    }
    if let Music::File { path, at } = &music {
        let path = path.clone();
        ui.horizontal_wrapped(|ui| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            icon(ui.painter(), r, Icon::Music, c().accent);
            ui.label(
                egui::RichText::new(name)
                    .font(semibold(14.0))
                    .color(c().text),
            );
            let place = if *at >= 0.0 {
                format!("plays from {} of the PMV", crate::util::fmt_ms(*at))
            } else {
                format!("the PMV begins {} into the song", crate::util::fmt_ms(-*at))
            };
            let sure = match app.tpl.fit {
                Some(f) if f.good() => format!(
                    " · found clearly ({} % of the PMV matches)",
                    (f.agree * 100.0).round()
                ),
                Some(f) => format!(
                    " · unsure ({} % of the PMV matches) – check the start",
                    (f.agree * 100.0).round()
                ),
                None => String::new(),
            };
            ui.label(
                egui::RichText::new(format!("{place}{sure}"))
                    .font(text(13.0))
                    .color(if app.tpl.fit.is_some_and(|f| !f.good()) {
                        c().accent2
                    } else {
                        c().text2
                    }),
            );
            if btn(ui, Btn::Ghost, None, "Find again", !busy).clicked() {
                app.template_music(
                    Music::File {
                        path: path.clone(),
                        at: *at,
                    },
                    app.tpl.start,
                    true,
                );
            }
            if btn(ui, Btn::Ghost, None, "Other song …", !busy).clicked() {
                if let Some(f) = pick_song() {
                    app.template_music(Music::File { path: f, at: 0.0 }, app.tpl.start, true);
                }
            }
        });
    }
    lab(
        ui,
        "Music starts at",
        "the moment of the PMV where the new PMV begins – as a time or a frame; before it is left out",
    );
    let mut apply: Option<f64> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let t = field(ui, &mut app.tpl.start_text, "0:00.000", 120.0);
        if t.lost_focus() {
            if let Some(v) = parse_time(&app.tpl.start_text) {
                apply = Some(v);
            }
        }
        if fps > 0.0 {
            ui.label(
                egui::RichText::new("= frame")
                    .font(text(13.0))
                    .color(c().text2),
            );
            let f = field(ui, &mut app.tpl.frame_text, "0", 90.0);
            if f.lost_focus() {
                if let Ok(n) = app.tpl.frame_text.trim().parse::<f64>() {
                    apply = Some(n.max(0.0) / fps);
                }
            }
            ui.label(
                egui::RichText::new(format!("at {} fps", (fps * 1000.0).round() / 1000.0))
                    .font(text(12.5))
                    .color(c().faint),
            );
            let step = 1.0 / fps;
            if btn(ui, Btn::Normal, None, "− 1 frame", !busy).clicked() {
                apply = Some((app.tpl.start - step).max(0.0));
            }
            if btn(ui, Btn::Normal, None, "+ 1 frame", !busy).clicked() {
                apply = Some(app.tpl.start + step);
            }
        }
        if btn(
            ui,
            Btn::Ghost,
            None,
            "From the beginning",
            !busy && app.tpl.start > 0.0,
        )
        .clicked()
        {
            apply = Some(0.0);
        }
    });
    if let Some(job) = &app.tpl.load {
        let (f, m) = job.progress();
        ui.label(egui::RichText::new(m).font(text(13.0)).color(c().text2));
        progress_bar(ui, f);
    }
    if let Some(v) = apply {
        // (frames are whole: the start sits on one)
        let v = if fps > 0.0 {
            (v * fps).round() / fps
        } else {
            v
        };
        let v = v.clamp(0.0, (raw.duration - 1.0).max(0.0));
        if (v - app.tpl.start).abs() > 1e-6 && !busy {
            app.template_music(music, v, false);
        } else {
            app.tpl.sync_text();
        }
    }
}

fn template_pane(ui: &mut Ui, app: &mut App) {
    use crate::template::Kind;
    // classic: follows the beat · exact cuts: copies the editing frame by frame
    let was = app.tpl.kind;
    ui.add_enabled_ui(app.tpl.scan.is_none(), |ui| {
        seg(
            ui,
            &mut app.tpl.kind,
            &[
                (Kind::Classic, "Follow the beat", "Cuts, layouts and flashes on the music's beats, with the PMV's music"),
                (Kind::Exact, "Exact cuts", "Every cut exactly where the PMV has it, frame by frame – whatever the beat; choose the music and where it starts"),
            ],
        );
    });
    if app.tpl.kind == Kind::Classic {
        hint(ui, "An existing PMV is analyzed: music, every cut, the layouts (split screens) and flashes are taken over – only the content comes from your clips. Effects like glitch or text are burned into the picture; instead, your effects run at the same moments.");
    } else {
        hint(ui, "The PMV is looked at frame by frame: every cut to another scene – exactly where its maker put it, whatever the beat – the layouts and flashes are taken over, only the content comes from your clips. The music can be the PMV's own or the same song as a file, starting where you set it. Takes longer to analyze.");
    }
    // (the other way on a PMV analysed already: analysed again)
    if app.tpl.kind != was && app.tpl.scan.is_none() {
        if let (Some(p), true) = (app.tpl.pmv.clone(), app.tpl.raw.is_some()) {
            let n = app.tpl.name.clone();
            app.tpl.load = None;
            app.scan_template(p, n);
        }
    }
    if let Some(job) = &app.tpl.scan {
        let (f, m) = job.progress();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&app.tpl.name)
                    .font(bold(15.0))
                    .color(c().text),
            );
            ui.label(egui::RichText::new(m).color(c().text2));
        });
        progress_bar(ui, f);
        if btn(ui, Btn::Ghost, None, "Cancel", true).clicked() {
            job.cancel.store(true, Ordering::SeqCst);
        }
        return;
    }
    if let Some((song, tpl)) = app.tpl.result.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(&tpl.name)
                    .font(display(22.0))
                    .color(c().text),
            );
            let st = &tpl.stats;
            ui.label(
                egui::RichText::new(format!(
                    "{} BPM · {} · {} cuts · {} layout changes · {} flashes",
                    song.a.bpm.round(),
                    fmt_time(song.duration()),
                    st.cuts,
                    st.layout_changes,
                    st.flashes
                ))
                .font(medium(14.0))
                .color(c().accent2),
            );
        });
        template_timeline(ui, &tpl, song.duration());
        ui.horizontal_wrapped(|ui| {
            let total: f64 = tpl.stats.layout_time.values().sum::<f64>().max(1e-6);
            let mut v: Vec<(&String, &f64)> = tpl.stats.layout_time.iter().collect();
            v.sort_by(|a, b| b.1.total_cmp(a.1));
            for (k, sec) in v {
                let name = crate::plan::layout(k).name;
                let lc = layout_color(k);
                let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter()
                    .rect_filled(r, 2.0, Color32::from_rgb(lc[0], lc[1], lc[2]));
                ui.label(
                    egui::RichText::new(format!("{name} {} %", (sec / total * 100.0).round()))
                        .font(medium(13.0))
                        .color(c().text2),
                );
                ui.add_space(6.0);
            }
        });
        if app.tpl.scanned == crate::template::Kind::Exact {
            template_music(ui, app);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if btn(ui, Btn::Ghost, None, "Other template", true).clicked() {
                if let Some(j) = &app.tpl.load {
                    j.cancel.store(true, Ordering::SeqCst);
                }
                app.tpl.result = None;
                app.tpl.raw = None;
                app.tpl.load = None;
            }
        });
        return;
    }
    let mut pick: Option<(PathBuf, String)> = None;
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 150.0).max(160.0);
        field(ui, &mut app.tpl.query, "Search a PMV among your clips …", w);
        if btn(ui, Btn::Normal, Some(Icon::Film), "Choose file", true).clicked() {
            if let Some(f) = rfd::FileDialog::new()
                .add_filter("Video", &library::VIDEO)
                .pick_file()
            {
                let n = f
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                pick = Some((f, n));
            }
        }
    });
    let cands = template_candidates(&app.lib.lk(), app.tpl.query.trim());
    if cands.is_empty() {
        hint(
            ui,
            if app.lib.lk().items.is_empty() {
                "Add your clips in step 2 – or choose a video file"
            } else {
                "Nothing found."
            },
        );
    }
    opts_grid(ui, cands.len(), |ui, i| {
        let it = &cands[i];
        let name = it
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let w = ui.available_width();
        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 48.0), Sense::click());
        let p = ui.painter();
        p.rect_filled(
            r,
            if glass() { 12.0 } else { 5.0 },
            if resp.hovered() {
                c().bg3
            } else {
                alpha(c().base, 0.6)
            },
        );
        let ic = Rect::from_min_size(r.min + Vec2::new(6.0, 6.0), Vec2::new(64.0, 36.0));
        p.rect_filled(ic, 3.0, c().bg2);
        icon(
            p,
            Rect::from_center_size(ic.center(), Vec2::splat(18.0)),
            Icon::Film,
            c().accent,
        );
        let tw = w - 84.0;
        let g = p.layout(name.clone(), bold(13.5), c().text, tw);
        p.galley(Pos2::new(ic.right() + 8.0, r.top() + 7.0), g, c().text);
        p.text(
            Pos2::new(ic.right() + 8.0, r.bottom() - 7.0),
            Align2::LEFT_BOTTOM,
            if it.dur > 0.0 {
                fmt_time(it.dur)
            } else {
                it.root.to_string_lossy().to_string()
            },
            text(12.0),
            c().faint,
        );
        if resp.on_hover_text(it.path.to_string_lossy()).clicked() {
            pick = Some((it.path.clone(), name));
        }
    });
    if let Some((p, n)) = pick {
        app.scan_template(p, n);
    }
}

// ---------- 2 Clips ----------

fn clips_card(ui: &mut Ui, app: &mut App) {
    card(ui, Some("2"), "Clips", |ui| {
        let (roots, n_items, n_v, n_i, pending, bad) = {
            let l = app.lib.lk();
            let v = l.items.iter().filter(|i| i.kind == Kind::Video).count();
            (
                l.roots.clone(),
                l.items.len(),
                v,
                l.items.len() - v,
                l.items.iter().filter(|i| !i.probed).count(),
                l.items.iter().filter(|i| i.bad.is_some()).count(),
            )
        };
        if n_items == 0 && roots.is_empty() {
            let over = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
            let (zone, link) = drop_zone_link(
                ui,
                130.0,
                Icon::Folder,
                "Drop folders with videos and images here",
                (
                    "or click to choose a folder · ",
                    "pick single files",
                    " · nothing is uploaded",
                ),
                over,
                false,
            );
            if link {
                app.add_files_dialog();
            } else if zone.clicked() {
                app.add_folder_dialog();
            }
        } else {
            // the library bar (.kb-pmvg-libbar)
            egui::Frame::new()
                .fill(if glass() {
                    alpha(c().bg2, 0.5)
                } else {
                    c().base
                })
                .stroke(Stroke::new(
                    1.0_f32,
                    if glass() { glass_edge() } else { c().bg3 },
                ))
                .corner_radius(if glass() { 14 } else { 6 })
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                        icon(ui.painter(), r, Icon::Folder, c().accent);
                        let pl = |n: usize, one: &str, many: &str| {
                            format!("{n} {}", if n == 1 { one } else { many })
                        };
                        let mut t = format!(
                            "{} · {} in {}",
                            pl(n_v, "video", "videos"),
                            pl(n_i, "image", "images"),
                            pl(roots.len(), "folder", "folders")
                        );
                        if pending > 0 {
                            t.push_str(&format!(" · reading {pending} …"));
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_millis(300));
                        }
                        if bad > 0 {
                            t.push_str(&format!(" · {bad} unreadable"));
                        }
                        ui.label(egui::RichText::new(t).font(medium(14.0)).color(c().text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if btn(ui, Btn::Ghost, None, "Remove all", true).clicked() {
                                let mut l = app.lib.lk();
                                for r in roots.clone() {
                                    l.remove_root(&r);
                                }
                            }
                            if btn(ui, Btn::Ghost, None, "Add files", true).clicked() {
                                app.add_files_dialog();
                            }
                            if btn(ui, Btn::Normal, Some(Icon::Plus), "Add folder", true).clicked()
                            {
                                app.add_folder_dialog();
                            }
                        });
                    });
                });
        }
        // What · Clip shape | Folders
        let cols = ui.available_width() >= 640.0;
        let what = |ui: &mut Ui, s: &mut Settings| {
            ui.spacing_mut().item_spacing.y = 5.0;
            lab(ui, "What", "");
            seg(
                ui,
                &mut s.source,
                &[
                    ("scene".to_string(), "Videos", ""),
                    ("image".into(), "Images", ""),
                    ("both".into(), "Both", ""),
                ],
            );
            lab(ui, "Clip shape", "");
            seg(
                ui,
                &mut s.shape,
                &[
                    ("all".to_string(), "All", ""),
                    ("portrait".into(), "Portrait only", ""),
                    ("landscape".into(), "Landscape only", ""),
                ],
            );
        };
        let lib = app.lib.clone();
        let folder_q = &mut app.ui.folder_q;
        let mut folders = |ui: &mut Ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            lab(ui, "Folders", "including subfolders");
            let mut l = lib.lk();
            let on = l.roots.iter().filter(|r| !l.off.contains(*r)).count();
            hint(
                ui,
                if on == l.roots.len() {
                    "All folders"
                } else {
                    "Only the folders ticked"
                },
            );
            field(ui, folder_q, "Search folders …", ui.available_width());
            let q = folder_q.to_lowercase();
            let mut toggle = None;
            let mut remove = None;
            // the picker (.kb-fpick): a bordered box with the folders, or the hint while there are none
            let g = glass();
            let counts: std::collections::HashMap<PathBuf, usize> =
                l.items.iter().fold(Default::default(), |mut m, i| {
                    *m.entry(i.root.clone()).or_default() += 1;
                    m
                });
            egui::Frame::new()
                .fill(if g {
                    alpha(c().base, 0.42 * 0.9)
                } else {
                    c().base
                })
                .stroke(Stroke::new(1.0_f32, if g { glass_edge() } else { c().bg3 }))
                .corner_radius(if g { 14 } else { 4 })
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if l.roots.is_empty() {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                hint(ui, "Add clips above – their folders show up here")
                            });
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("folders")
                        .max_height(228.0)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for r in l.roots.iter() {
                                let s = r.to_string_lossy().to_string();
                                if !q.is_empty() && !s.to_lowercase().contains(&q) {
                                    continue;
                                }
                                let is_on = !l.off.contains(r);
                                let w = ui.available_width();
                                let (row, resp) =
                                    ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click());
                                let h = anim(ui, resp.id.with("h"), resp.hovered(), 0.12);
                                let p = ui.painter();
                                if h > 0.0 {
                                    p.rect_filled(row, 4.0, alpha(c().bg2, h));
                                }
                                // the box: 15 px, accent and a check when on
                                let bx = Rect::from_center_size(
                                    Pos2::new(row.left() + 16.0, row.center().y),
                                    Vec2::splat(15.0),
                                );
                                if is_on {
                                    p.rect_filled(bx, 3.0, c().accent);
                                    icon(p, bx.shrink(1.5), Icon::Check, c().on_accent);
                                } else {
                                    p.rect_stroke(
                                        bx,
                                        3.0,
                                        Stroke::new(1.5_f32, c().faint),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                let n = r
                                    .file_name()
                                    .map(|x| x.to_string_lossy().to_string())
                                    .unwrap_or(s.clone());
                                let cnt = counts.get(r).copied().unwrap_or(0).to_string();
                                let cg = p.layout_no_wrap(cnt, text(11.5), c().faint);
                                let xr = Rect::from_center_size(
                                    Pos2::new(row.right() - 16.0, row.center().y),
                                    Vec2::splat(20.0),
                                );
                                let name_w = (row.width() - 34.0 - cg.size().x - 44.0).max(20.0);
                                let ng = p.layout(
                                    n,
                                    text(14.0),
                                    if is_on { c().text } else { c().text2 },
                                    name_w,
                                );
                                let ng_h = ng.size().y.min(20.0);
                                p.with_clip_rect(Rect::from_min_size(
                                    Pos2::new(row.left() + 32.0, row.top()),
                                    Vec2::new(name_w, row.height()),
                                ))
                                .galley(
                                    Pos2::new(row.left() + 32.0, row.center().y - ng_h / 2.0),
                                    ng,
                                    c().text,
                                );
                                p.galley(
                                    Pos2::new(
                                        xr.left() - 8.0 - cg.size().x,
                                        row.center().y - cg.size().y / 2.0,
                                    ),
                                    cg,
                                    c().faint,
                                );
                                let xresp = ui.interact(xr, resp.id.with("x"), Sense::click());
                                let xh = anim(ui, xresp.id.with("h"), xresp.hovered(), 0.12);
                                if xh > 0.0 {
                                    ui.painter().rect_filled(xr, 4.0, alpha(c().bg3, xh));
                                }
                                icon(
                                    ui.painter(),
                                    xr.shrink(4.0),
                                    Icon::Close,
                                    mix(c().faint, c().text, xh),
                                );
                                if xresp.on_hover_text("Remove this folder").clicked() {
                                    remove = Some(r.clone());
                                } else if resp.on_hover_text(&s).clicked() {
                                    toggle = Some(r.clone());
                                }
                            }
                        });
                });
            if let Some(r) = toggle {
                if !l.off.remove(&r) {
                    l.off.insert(r);
                }
            }
            if let Some(r) = remove {
                l.remove_root(&r);
            }
        };
        if cols {
            ui.columns(2, |c| {
                what(&mut c[0], &mut app.s);
                folders(&mut c[1]);
            });
        } else {
            what(ui, &mut app.s);
            folders(ui);
        }
        if n_items == 0 {
            // (the empty count line still takes its gaps in the original)
            ui.add_space(17.0);
        } else {
            let (v, i, _) = app.lib.lk().count(&app.s);
            hint(
                ui,
                &format!(
                    "{v} {} and {i} {} match",
                    if v == 1 { "video" } else { "videos" },
                    if i == 1 { "image" } else { "images" }
                ),
            );
        }
        // Clip selection
        lab(ui, "Clip selection", "which part of a scene is taken");
        let s = &mut app.s;
        opts_grid(ui, 5, |ui, i| {
            match i {
                0 => opt(ui, &mut s.best_spots, "Best moments instead of random", "Looks at several spots per video (motion, skin, contrast) and takes the best one"),
                1 => opt(ui, &mut s.pace_match, "Clip speed follows the music (experimental)", "Fast, busy music gets clips with fast motion, calm music slow ones. Measures how fast the motion is at the spot a clip is taken from – the more clips have been prepared, the better it knows your library"),
                2 => opt(ui, &mut s.beat_sync, "Motion on the beat (experimental)", "Measures the rhythm of the motion in each clip and lines it up with the beat – a clip comes in on a turn of the motion and plays up to 12 % faster or slower. Clips take about 1 s longer to prepare; works best with steady, repeating motion, a still camera and clips that stay on for a few beats"),
                3 => opt(ui, &mut s.clean_cuts, "Clean cuts", "A clip starts where its scene runs on long enough for its shot – every frame around it is checked, so no hidden cut inside the clip jumps to another scene by itself"),
                _ => opt(ui, &mut s.smart_crop, "Smart crop", "The crop follows what matters in the clip instead of sticking to the center"),
            };
        });
        range(
            ui,
            Icon::Sliders,
            "Skip the start of each video",
            &mut s.skip_head,
            0.0,
            300.0,
            5.0,
            &|v| format!("{v:.0} s"),
        );
        range(
            ui,
            Icon::Sliders,
            "Skip the end of each video",
            &mut s.skip_tail,
            0.0,
            300.0,
            5.0,
            &|v| format!("{v:.0} s"),
        );
        per_video(ui, app);
        lab(ui, "Clip order", "which clip follows which");
        let s = &mut app.s;
        opts_grid(ui, 3, |ui, i| {
            match i {
                0 => opt(ui, &mut s.match_cut, "Match cuts", "At each cut, the clip that best matches the previous one in color, brightness and composition comes next"),
                1 => opt(ui, &mut s.sequenced, "Follow the scenes' timeline", "A clip comes from the part of its scene that matches how far the song is: the start of the song uses the beginnings of the scenes, the end of the song their endings (a song of known length only)"),
                _ => opt(ui, &mut s.variety, "Variety", "The same clip doesn't come up again shortly after"),
            };
        });
    });
}

/// Per video: its own skip of the start / end (on top of the one for all videos)
fn per_video(ui: &mut Ui, app: &mut App) {
    let n_own = app.lib.lk().skips.len();
    let title = if n_own > 0 {
        format!("Per video – {n_own} with their own skip")
    } else {
        "Per video – skip the start or end of one video".to_string()
    };
    let resp = ui
        .horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            if app.ui.per_video_open {
                icon(ui.painter(), r, Icon::Chevron, c().accent2);
            } else {
                icon(ui.painter(), r.shrink(2.0), Icon::Play, c().accent2);
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&title)
                        .font(medium(13.5))
                        .color(c().accent2),
                )
                .sense(Sense::click()),
            )
        })
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        app.ui.per_video_open = !app.ui.per_video_open;
    }
    if !app.ui.per_video_open {
        return;
    }
    hint(ui, "A video's own skip replaces the one above for that video (e.g. a long intro or credits). Empty = the setting for all videos. Kept for the next time the video is added.");
    let w = ui.available_width();
    field(ui, &mut app.ui.video_q, "Find a video …", w);
    let q = app.ui.video_q.to_lowercase();
    let list: Vec<(PathBuf, String, f64, ClipSkip)> = {
        let l = app.lib.lk();
        let mut v: Vec<_> = l
            .videos()
            .filter(|i| q.is_empty() || i.path.to_string_lossy().to_lowercase().contains(&q))
            .map(|i| (i.path.clone(), i.name.clone(), i.dur, i.skip))
            .collect();
        // the ones with their own skip first
        v.sort_by(|a, b| {
            b.3.is_empty()
                .cmp(&a.3.is_empty())
                .reverse()
                .then(a.1.cmp(&b.1))
        });
        v.truncate(60);
        v
    };
    if list.is_empty() {
        hint(ui, "No videos found");
        return;
    }
    let mut change: Option<(PathBuf, ClipSkip)> = None;
    egui::ScrollArea::vertical()
        .id_salt("per-video")
        .max_height(300.0)
        .show(ui, |ui| {
            egui::Grid::new("per-video-grid")
                .num_columns(4)
                .spacing([10.0, 6.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Video")
                            .font(medium(12.5))
                            .color(c().faint),
                    );
                    ui.label(
                        egui::RichText::new("Length")
                            .font(medium(12.5))
                            .color(c().faint),
                    );
                    ui.label(
                        egui::RichText::new("Skip start")
                            .font(medium(12.5))
                            .color(c().faint),
                    );
                    ui.label(
                        egui::RichText::new("Skip end")
                            .font(medium(12.5))
                            .color(c().faint),
                    );
                    ui.end_row();
                    for (path, name, dur, skip) in &list {
                        ui.label(
                            egui::RichText::new(name)
                                .font(text(13.5))
                                .color(if skip.is_empty() { c().text2 } else { c().text }),
                        )
                        .on_hover_text(path.to_string_lossy());
                        ui.label(
                            egui::RichText::new(if *dur > 0.0 {
                                fmt_time(*dur)
                            } else {
                                "…".into()
                            })
                            .font(text(13.0))
                            .color(c().faint),
                        );
                        let mut s2 = *skip;
                        let maxv = if *dur > 0.0 {
                            (dur * 0.4).floor()
                        } else {
                            600.0
                        };
                        for (k, v) in [(0, s2.head), (1, s2.tail)] {
                            ui.horizontal(|ui| {
                                let mut on = v.is_some();
                                if ui
                                    .checkbox(&mut on, "")
                                    .on_hover_text("Its own skip")
                                    .changed()
                                {
                                    let nv = if on {
                                        Some(if k == 0 {
                                            app.s.skip_head
                                        } else {
                                            app.s.skip_tail
                                        })
                                    } else {
                                        None
                                    };
                                    if k == 0 {
                                        s2.head = nv
                                    } else {
                                        s2.tail = nv
                                    }
                                    change = Some((path.clone(), s2));
                                }
                                if let Some(mut x) = v {
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut x)
                                                .range(0.0..=maxv)
                                                .speed(1.0)
                                                .suffix(" s"),
                                        )
                                        .changed()
                                    {
                                        if k == 0 {
                                            s2.head = Some(x)
                                        } else {
                                            s2.tail = Some(x)
                                        }
                                        change = Some((path.clone(), s2));
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new("as above")
                                            .font(text(12.5))
                                            .color(c().faint),
                                    );
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
        });
    if let Some((p, sk)) = change {
        app.lib.lk().set_skip(&p, sk);
        let key = p.to_string_lossy().to_string();
        if sk.is_empty() {
            app.stored.clip_skips.remove(&key);
        } else {
            app.stored.clip_skips.insert(key, sk);
        }
        let _ = settings::save_stored(&app.stored);
    }
}

// ---------- 3 Style ----------

fn style_card(ui: &mut Ui, app: &mut App) {
    card(ui, Some("3"), "Style", |ui| {
        if app.mode == Mode::Template {
            tip(ui, "Template active: cuts, layouts and flashes come from the PMV. Here you choose the effects that are added at those moments.");
        }
        // .kb-pmvg-presets: 16 px down, a line, 14 px, then the label and the chips 8 px apart
        ui.add_space(16.0 - 8.0);
        let (line, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(line, 0.0, c().line);
        ui.add_space(14.0 - 8.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            lab(ui, "Mood", "sets cutting, layouts and effects in one go");
            ui.horizontal_wrapped(|ui| {
                for p in PRESETS.iter() {
                    if chip(ui, p.name, false, None).clicked() {
                        app.s.apply_mood(p);
                    }
                }
            });
        });
        ui.add_space(4.0);
        let all_closed = SECTIONS
            .iter()
            .all(|s| !*app.ui.open.get(s.0).unwrap_or(&true));
        ui.horizontal(|ui| {
            lab(ui, "Settings", "");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if all_closed {
                    "Open all"
                } else {
                    "Collapse all"
                };
                if btn(
                    ui,
                    Btn::Normal,
                    Some(if all_closed {
                        Icon::Chevron
                    } else {
                        Icon::ChevronUp
                    }),
                    label,
                    true,
                )
                .clicked()
                {
                    for s in SECTIONS {
                        app.ui.open.insert(s.0, all_closed);
                    }
                }
            });
        });
        ui.spacing_mut().item_spacing.y = 8.0;
        for (key, name, sub) in SECTIONS {
            let mut open = *app.ui.open.get(key).unwrap_or(&true);
            let sum = tab_sum(&app.s, key);
            section(ui, name, &sum, sub, &mut open, |ui| match key {
                "cut" => sec_cut(ui, app),
                "fx" => sec_fx(ui, app),
                "pics" => sec_pics(ui, &mut app.s),
                "look" => sec_look(ui, &mut app.s),
                "frame" => sec_frame(ui, &mut app.s),
                "sound" => sec_sound(ui, &mut app.s),
                "fs" => sec_fs(ui, &mut app.s),
                _ => sec_out(ui, app),
            });
            app.ui.open.insert(key, open);
        }
    });
}

fn sec_cut(ui: &mut Ui, app: &mut App) {
    let tpl = app.mode == Mode::Template;
    let s = &mut app.s;
    group_head(ui, "Timing", "When the picture changes");
    lab(ui, "When to cut", "");
    ui.add_enabled_ui(!tpl, |ui| {
        let opts: Vec<(String, &str, &str)> =
            CUTS.iter().map(|c| (c.0.to_string(), c.1, c.2)).collect();
        seg_full(ui, &mut s.cut, &opts);
    });
    if s.cut == "auto" {
        // (the pace box is a block: label and seg sit on one line)
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            lab(ui, "Pace", "how fast “Automatic” cuts");
            seg(
                ui,
                &mut s.pace,
                &[
                    (
                        "slow".to_string(),
                        "Slow",
                        "Calm: every 8 beats · medium: every 4 · loud: every 2",
                    ),
                    (
                        "normal".into(),
                        "Normal",
                        "Calm: every 4 beats · medium: every 2 · loud: every beat",
                    ),
                    (
                        "fast".into(),
                        "Fast",
                        "Calm: every 2 beats · medium and loud: every beat",
                    ),
                ],
            );
        });
    }
    opts_grid(ui, 4, |ui, i| match i {
        0 => {
            range(
                ui,
                Icon::Sliders,
                "Cut ahead of the beat",
                &mut s.cut_lead,
                0.0,
                80.0,
                5.0,
                &|v| format!("{v:.0} ms"),
            );
        }
        1 => {
            range(
                ui,
                Icon::Sliders,
                "Shortest clip",
                &mut s.min_beats,
                0.0,
                16.0,
                1.0,
                &|v| {
                    if v == 0.0 {
                        "off".into()
                    } else {
                        format!("{v:.0} beats")
                    }
                },
            );
        }
        2 => {
            range(
                ui,
                Icon::Sliders,
                "Longest clip",
                &mut s.max_beats,
                0.0,
                32.0,
                1.0,
                &|v| {
                    if v == 0.0 {
                        "off".into()
                    } else {
                        format!("{v:.0} beats")
                    }
                },
            );
        }
        _ => {
            opt(ui, &mut s.bars, "Bars and phrases", "Finds the \"one\" of each bar and where a phrase begins: cuts land on the strong beats, split screens change at the start of a phrase");
        }
    });
    group_head(ui, "Split screens", "Which layouts and how they are filled");
    lab(
        ui,
        "Layouts",
        "change to the beat, the louder the more fields",
    );
    ui.add_enabled_ui(!tpl, |ui| {
        ui.horizontal_wrapped(|ui| {
            for l in LAYOUTS.iter() {
                let on = s.layouts.get(l.id);
                if chip(ui, l.name, on, Some(l.id))
                    .on_hover_text(l.hint)
                    .clicked()
                {
                    let n = settings::LAYOUT_IDS
                        .iter()
                        .filter(|k| s.layouts.get(k))
                        .count();
                    // (at least one stays on)
                    if !(on && n == 1) {
                        *s.layouts.get_mut(l.id) = !on;
                    }
                }
            }
        });
    });
    lab(ui, "Clip shape per layout", "e.g. landscape clips only in full screen, portrait only in 3-way (set “Clip shape” in What to “All”)");
    // (.kb-pmvg-lshape: rows 6 px apart, 6 px down; the select on the right)
    ui.add_space(6.0 - 8.0 + 6.0);
    for l in LAYOUTS.iter() {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(24.0, 15.0), Sense::hover());
            layout_icon(ui.painter(), r, l.id, c().text2);
            let (r, _) = ui.allocate_exact_size(Vec2::new(150.0, 20.0), Sense::hover());
            ui.painter().text(
                r.left_center(),
                Align2::LEFT_CENTER,
                l.name,
                medium(14.0),
                c().text,
            );
            let mut v = s
                .layout_shape
                .get(l.id)
                .cloned()
                .unwrap_or_else(|| "all".into());
            let opts = vec![
                ("all".to_string(), "All".to_string()),
                ("landscape".into(), "Landscape only".into()),
                ("portrait".into(), "Portrait only".into()),
            ];
            ui.add_space((ui.available_width() - 150.0).max(0.0));
            if select(ui, ("ls", l.id), &mut v, &opts, 150.0) {
                if v == "all" {
                    s.layout_shape.remove(l.id);
                } else {
                    s.layout_shape.insert(l.id.to_string(), v);
                }
            }
        });
    }
    lab(ui, "Fields in 2-/3-way layouts", "");
    seg_full(
        ui,
        &mut s.split,
        &[
            (
                "cols".to_string(),
                "side by side",
                "Columns – also in portrait format",
            ),
            ("rows".into(), "stacked", "Rows"),
        ],
    );
    if s.layouts.tri
        && s.shape == "portrait"
        && settings::ratio_of(&s.format).is_some_and(|r| r < 1.0)
    {
        tip(
            ui,
            "Three full-size portrait clips side by side need a landscape format (16:9).",
        );
    }
    group_head(ui, "Extras", "Opening and scrolling");
    opts_grid(ui, 2, |ui, i| {
        if i == 0 {
            opt(ui, &mut s.reveal, "Reveal opening", "The first clip sits small in the middle with rounded corners and slowly grows – at the first drop the picture opens up into the layouts (songs with a known length; not with templates)");
        } else {
            opt(ui, &mut s.scroll, "Scrolling sides", "In 3-way layouts the middle clip stays longer while the clips at the sides scroll up or down, like swiping through a feed (needs the 3-way layouts)");
        }
    });
}

fn sec_fx(ui: &mut Ui, app: &mut App) {
    let s = &mut app.s;
    group_head(
        ui,
        "Zoom pulse",
        "How strong and on which beats the picture pumps",
    );
    range(
        ui,
        Icon::Sliders,
        "Strength",
        &mut s.pulse_amt,
        0.0,
        100.0,
        5.0,
        &pct,
    );
    seg_full(
        ui,
        &mut s.pulse_on,
        &[
            ("beat".to_string(), "Every beat", "On every beat"),
            ("2".into(), "Every 2nd", "On every other beat"),
            (
                "bar".into(),
                "Each bar",
                "Only on the first beat of each bar",
            ),
        ],
    );
    for (name, sub, keys) in FX_GROUPS {
        if group_head_btn(ui, name, sub, Some("All on")) {
            for k in keys {
                *s.fx.get_mut(k) = true;
            }
        }
        opts_grid(ui, keys.len(), |ui, i| {
            let (n, d) = fx_text(keys[i]);
            opt(ui, s.fx.get_mut(keys[i]), n, d);
        });
        if keys.contains(&"text") && s.fx.text {
            let w = ui.available_width();
            field(
                ui,
                &mut s.words,
                "Words for “Text”, comma separated – e.g. DROP, MORE, YES",
                w,
            );
        }
    }
}

fn sec_pics(ui: &mut Ui, s: &mut Settings) {
    hint(ui, "Still images from your library, on top of the clips – in a tempo of their own. Needs images in the library (add image files or a folder with images); the clips can be videos only.");
    group_head(ui, "Image flash", "Pictures cover the clips for a moment");
    opts_grid(ui, 1, |ui, _| {
        opt(
            ui,
            &mut s.pic_flash,
            "Image flash",
            "Pictures flash up on the beat – up to eight per beat",
        );
    });
    ui.add_enabled_ui(s.pic_flash, |ui| {
        // (the flash box is a block: Speed · seg · How · seg flow on one line)
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            lab(ui, "Speed", "pictures per beat");
            seg(
                ui,
                &mut s.pic_flash_rate,
                &[
                    (
                        "auto".to_string(),
                        "Follow the song",
                        "Calm parts one per beat, louder parts two, loud parts four",
                    ),
                    ("1".into(), "1", ""),
                    ("2".into(), "2", ""),
                    ("4".into(), "4", ""),
                    ("8".into(), "8", ""),
                ],
            );
            ui.add_space(4.0);
            lab(ui, "How", "");
            seg(
                ui,
                &mut s.pic_flash_mode,
                &[
                    (
                        "single".to_string(),
                        "One picture",
                        "One picture, as long as one step of the speed above",
                    ),
                    (
                        "burst".into(),
                        "A burst",
                        "Several pictures in a row, as many as fit in the burst length",
                    ),
                    ("both".into(), "Both", ""),
                ],
            );
        });
        range(
            ui,
            Icon::Sliders,
            "Burst length",
            &mut s.pic_flash_len,
            1.0,
            8.0,
            1.0,
            &|v| format!("{v:.0} beats"),
        );
        range(
            ui,
            Icon::Sliders,
            "How often",
            &mut s.pic_flash_pct,
            5.0,
            100.0,
            5.0,
            &pct,
        );
    });
    group_head(ui, "Image overlay", "A picture lies over the playing clips");
    opts_grid(ui, 1, |ui, _| {
        opt(
            ui,
            &mut s.pic_overlay,
            "Image overlay",
            "A see-through picture fades in over the clips and out again",
        );
    });
    ui.add_enabled_ui(s.pic_overlay, |ui| {
        range(
            ui,
            Icon::Sliders,
            "Opacity",
            &mut s.pic_overlay_amt,
            10.0,
            90.0,
            5.0,
            &pct,
        );
        range(
            ui,
            Icon::Sliders,
            "Length",
            &mut s.pic_overlay_len,
            1.0,
            16.0,
            1.0,
            &|v| format!("{v:.0} beats"),
        );
        range(
            ui,
            Icon::Sliders,
            "How often",
            &mut s.pic_overlay_pct,
            5.0,
            100.0,
            5.0,
            &pct,
        );
    });
}

fn look_seg(ui: &mut Ui, s: &mut Settings) {
    let g = glass();
    egui::Frame::new()
        .fill(if g { alpha(c().bg3, 0.38) } else { c().base })
        .stroke(Stroke::new(1.0_f32, if g { glass_edge() } else { c().bg3 }))
        .corner_radius(if g { 255 } else { 4 })
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            // (straight inside its group: as wide as the group)
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                for (v, l) in LOOKS {
                    let on = s.look == v;
                    let gal = ui.painter().layout_no_wrap(
                        l.into(),
                        medium(13.0),
                        if on { c().on_accent } else { c().text2 },
                    );
                    let (r, resp) = ui
                        .allocate_exact_size(Vec2::new(gal.size().x + 40.0, 30.0), Sense::click());
                    let p = ui.painter();
                    if on {
                        p.rect_filled(r, if g { 255 } else { 2 }, c().accent);
                    } else if resp.hovered() {
                        p.rect_filled(r, if g { 255 } else { 2 }, c().bg2);
                    }
                    let dot = Pos2::new(r.left() + 16.0, r.center().y);
                    let dot_c = match v {
                        "warm" => Color32::from_rgb(0xf0, 0xa0, 0x60),
                        "pink" => c().accent,
                        "cold" => Color32::from_rgb(0x5a, 0xa0, 0xff),
                        "vivid" => Color32::from_rgb(0xff, 0xd1, 0x66),
                        "bw" => Color32::from_gray(200),
                        "noir" => Color32::from_gray(80),
                        "custom" => c().second,
                        _ => Color32::from_rgb(0xb0, 0xa0, 0xa6),
                    };
                    p.circle_filled(dot, 5.0, dot_c);
                    if v == "bw" {
                        p.add(egui::Shape::convex_polygon(
                            vec![
                                dot + Vec2::new(0.0, -5.0),
                                dot + Vec2::new(5.0, 0.0),
                                dot + Vec2::new(0.0, 5.0),
                            ],
                            Color32::from_gray(20),
                            Stroke::NONE,
                        ));
                    }
                    p.circle_stroke(dot, 5.0, Stroke::new(1.0_f32, white(90)));
                    p.galley(
                        Pos2::new(r.left() + 28.0, r.center().y - gal.size().y / 2.0),
                        gal,
                        if on { c().on_accent } else { c().text2 },
                    );
                    if resp.clicked() {
                        s.look = v.to_string();
                    }
                }
            });
        });
}

fn sec_look(ui: &mut Ui, s: &mut Settings) {
    group_head(ui, "Color", "Mood and brightness");
    lab(ui, "Color look", "all clips in the same color mood");
    look_seg(ui, s);
    if s.look != "none" {
        pair(
            ui,
            |ui| {
                range(
                    ui,
                    Icon::Sliders,
                    "Strength",
                    &mut s.look_amt,
                    0.0,
                    100.0,
                    5.0,
                    &pct,
                );
            },
            |ui| {
                if s.look == "custom" {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Color").font(bold(14.0)));
                        let mut c = crate::compositor::hex_rgb(&s.look_color);
                        if ui.color_edit_button_srgb(&mut c).changed() {
                            s.look_color = format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
                        }
                    });
                }
            },
        );
    }
    lab(
        ui,
        "Brighter",
        "lifts the mid-tones smoothly, without burning out the highlights",
    );
    range(
        ui,
        Icon::Eye,
        "Brightness",
        &mut s.bright,
        0.0,
        100.0,
        5.0,
        &pct,
    );
    opts_grid(ui, 1, |ui, _| {
        opt(ui, &mut s.look_even, "Even out brightness", "Clips that are too dark get brightened, too bright ones toned down – looks all of a piece");
    });
    group_head(ui, "Film effects", "Old-film and zoom looks on every clip");
    opts_grid(ui, LOOK_FX.len(), |ui, i| {
        let (n, d) = fx_text(LOOK_FX[i]);
        opt(ui, s.fx.get_mut(LOOK_FX[i]), n, d);
    });
}

fn sec_frame(ui: &mut Ui, s: &mut Settings) {
    group_head(ui, "Format", "Shape of the picture");
    lab(
        ui,
        "Picture",
        "“Fit” shows the whole clip, “Fill” crops it to fill the frame",
    );
    ui.horizontal_wrapped(|ui| {
        let opts: Vec<(String, String)> = settings::RATIOS
            .iter()
            .map(|r| (r.0.to_string(), r.1.to_string()))
            .collect();
        select(ui, "format", &mut s.format, &opts, 210.0);
        seg(
            ui,
            &mut s.fit,
            &[
                ("contain".to_string(), "Fit", "Whole picture, rest blurred"),
                (
                    "cover".into(),
                    "Fill",
                    "Picture fills everything, edges are cropped",
                ),
            ],
        );
    });
    ui.horizontal_wrapped(|ui| {
        for (r, l) in [
            ("16:9", "16:9"),
            ("9:16", "9:16"),
            ("1:1", "1:1"),
            ("4:3", "4:3"),
            ("21:9", "21:9"),
            ("32:9", "32:9"),
        ] {
            if chip(ui, l, s.format == r, None).clicked() {
                s.format = r.to_string();
            }
        }
    });
    group_head(
        ui,
        "Seams and edges",
        "Where clips meet and the rim of the picture",
    );
    lab(
        ui,
        "Soft seams",
        "the line between the clips of a split screen is soft instead of sharp",
    );
    opts_grid(ui, 1, |ui, _| {
        opt(ui, &mut s.soft, "Soften the seams", "No sharp lines between the fields: the seam is smeared softly (the clips don't overlap)");
    });
    if s.soft {
        range(
            ui,
            Icon::Sliders,
            "Softness",
            &mut s.soft_amt,
            0.0,
            100.0,
            5.0,
            &pct,
        );
    } else {
        range(
            ui,
            Icon::Sliders,
            "Divider width",
            &mut s.div_w,
            1.0,
            8.0,
            1.0,
            &|v| format!("{v:.0} px"),
        );
    }
    lab(
        ui,
        "Rim of the picture",
        "only the edges, the middle stays sharp",
    );
    seg_full(ui, &mut s.edge, &[("off".to_string(), "Off", ""), ("blur".into(), "Blur", "Soft blur towards the edges"), ("motion".into(), "Motion", "Light motion blur: streaks sideways at the left and right edge, up and down at the top and bottom"), ("lens".into(), "Lens", "The edges look bent, like through a lens")]);
    if s.edge != "off" {
        range(
            ui,
            Icon::Sliders,
            "Strength",
            &mut s.edge_amt,
            0.0,
            100.0,
            5.0,
            &pct,
        );
    }
    group_head(ui, "Quality", "");
    opts_grid(ui, 2, |ui, i| {
        if i == 0 {
            opt(ui, &mut s.smooth, "Smooth scaling", "Clips are scaled with the best quality – less pixelated and jagged, a bit more work for the computer");
        } else {
            opt(ui, &mut s.hwaccel, "Graphics card decoding", "The graphics card decodes the clips when it can (falls back to the processor by itself)");
        }
    });
}

fn sec_sound(ui: &mut Ui, s: &mut Settings) {
    lab(ui, "Volume", "");
    pair(
        ui,
        |ui| {
            range(
                ui,
                Icon::Music,
                "Song",
                &mut s.song_vol,
                0.0,
                100.0,
                5.0,
                &pct,
            );
        },
        |ui| {
            ui.add_enabled_ui(s.fx.voice, |ui| {
                range(
                    ui,
                    Icon::Film,
                    "Clips",
                    &mut s.clip_vol,
                    0.0,
                    100.0,
                    5.0,
                    &pct,
                );
            });
        },
    );
    opts_grid(ui, 1, |ui, _| {
        let (n, d) = fx_text("voice");
        opt(ui, &mut s.fx.voice, n, d);
    });
    ui.add_enabled_ui(s.fx.voice, |ui| {
        lab(ui, "Clip audio plays", "");
        seg(
            ui,
            &mut s.voice_mode,
            &[
                (
                    "drops".to_string(),
                    "Only on drops",
                    "Fade in briefly on drops only – like the voice-overs in real PMVs",
                ),
                (
                    "always".into(),
                    "Always",
                    "The clips can be heard all the time, under the song",
                ),
            ],
        );
    });
    hint(
        ui,
        if !s.fx.voice {
            "Clip audio is off – only the song can be heard."
        } else if s.voice_mode == "always" {
            "The clips play audibly under the song."
        } else {
            "On every drop, the original audio of the largest clip fades in for a few beats."
        },
    );
    hint(ui, "Both end up in the exported video exactly like this. With several clips at once (split screen) they share the clip volume.");
}

fn sec_fs(ui: &mut Ui, s: &mut Settings) {
    opts_grid(ui, 1, |ui, _| {
        opt(ui, &mut s.fs_on, "Make a funscript with the video", "Built from the song's beats and energy – saved next to the exported video (.funscript), for The Handy and other devices");
    });
    if !s.fs_on {
        return;
    }
    lab(ui, "Pace", "strokes per beat");
    seg(
        ui,
        &mut s.fs_pace,
        &[
            (
                "auto".to_string(),
                "Follow the song",
                "Calm parts slow, loud parts and drops fast",
            ),
            ("slow".into(), "Slow", "One stroke every 2 beats"),
            ("normal".into(), "Normal", "One stroke per beat"),
            ("fast".into(), "Fast", "Two strokes per beat"),
        ],
    );
    lab(ui, "Stroke size", "");
    seg(
        ui,
        &mut s.fs_size,
        &[
            (
                "auto".to_string(),
                "Follow the song",
                "Louder = bigger strokes",
            ),
            ("small".into(), "Small", ""),
            ("medium".into(), "Medium", ""),
            ("large".into(), "Large", ""),
            ("full".into(), "Full", "The whole length, 0–100"),
        ],
    );
    lab(ui, "Where on the stroke", "");
    seg(
        ui,
        &mut s.fs_where,
        &[
            ("low".to_string(), "Low", "Strokes in the lower part"),
            ("mid".into(), "Middle", ""),
            ("high".into(), "High", "Strokes in the upper part"),
        ],
    );
    lab(ui, "Style", "");
    seg(
        ui,
        &mut s.fs_style,
        &[
            (
                "sharp".to_string(),
                "Sharp",
                "Straight lines from beat to beat",
            ),
            ("smooth".into(), "Smooth", "Rounded, flowing strokes"),
        ],
    );
    opts_grid(ui, 2, |ui, i| {
        if i == 0 {
            opt(
                ui,
                &mut s.fs_rests,
                "Gentle in calm parts",
                "Quiet passages get slow, small strokes",
            );
        } else {
            opt(
                ui,
                &mut s.fs_accent,
                "Accents",
                "A bigger stroke on the first beat of each bar and on drops",
            );
        }
    });
    range(
        ui,
        Icon::Sliders,
        "Top speed",
        &mut s.fs_speed,
        100.0,
        600.0,
        25.0,
        &|v| format!("{v:.0}"),
    );
    hint(ui, "Top speed limits how fast the device is asked to move (units per second) – fast strokes get smaller instead.");
}

fn sec_out(ui: &mut Ui, app: &mut App) {
    let survey = app.encoders.lk().clone();
    let s = &mut app.s;
    group_head(ui, "Title cards", "Intro and outro");
    opts_grid(ui, 3, |ui, i| match i {
        0 => {
            opt(
                ui,
                &mut s.intro,
                "Intro",
                "Title card at the start: your title slams in, comic-SFX style",
            );
        }
        1 => {
            opt(
                ui,
                &mut s.outro,
                "Outro",
                "Credits at the end: the picture fades dark, title and number of clips",
            );
        }
        _ => {
            opt(ui, &mut s.auto_start, "Start by itself", "Starts the show as soon as it can – right after you pick a song – with the settings as you left them");
        }
    });
    let w = ui.available_width();
    field(
        ui,
        &mut s.title,
        "Title for intro/outro – empty = song name",
        w,
    );
    // font of the title cards and words
    let mut font_msg: Option<(String, bool)> = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Font")
                .font(medium(13.5))
                .color(c().text2),
        );
        let name = if s.font.trim().is_empty() {
            "Anton (built-in)".to_string()
        } else {
            std::path::Path::new(&s.font)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        };
        ui.label(egui::RichText::new(name).font(bold(14.0)).color(c().text))
            .on_hover_text("Used for the intro, the outro and the words of “Text”");
        if btn(ui, Btn::Normal, None, "Choose font …", true).clicked() {
            if let Some(f) = rfd::FileDialog::new()
                .add_filter("Font", &["ttf", "otf", "ttc"])
                .pick_file()
            {
                match crate::text::Font::load(&f) {
                    Ok(font) => {
                        s.font = f.to_string_lossy().to_string();
                        font_msg = Some((format!("Font: {}", font.name), true));
                    }
                    Err(e) => font_msg = Some((format!("This font can't be used: {e:#}"), false)),
                }
            }
        }
        if !s.font.is_empty() && btn(ui, Btn::Ghost, None, "Built-in", true).clicked() {
            s.font.clear();
        }
    });
    group_head(
        ui,
        "Export",
        "Renders the PMV to a video file – every frame, however slow the computer",
    );
    lab(ui, "Quality", "the short side of the picture");
    let q: Vec<(f64, &str, &str)> = settings::QUALITIES
        .iter()
        .map(|(v, l)| (*v, *l, ""))
        .collect();
    seg(ui, &mut s.quality, &q);
    let (w2, h2) = crate::show::size_for(s, s.quality);
    hint(ui, &format!("{w2} × {h2} pixels"));
    lab(ui, "Frame rate", "");
    let f: Vec<(f64, String, &str)> = settings::FPS_CHOICES
        .iter()
        .map(|v| (*v, format!("{v:.0} fps"), ""))
        .collect();
    let f2: Vec<(f64, &str, &str)> = f.iter().map(|x| (x.0, x.1.as_str(), x.2)).collect();
    seg(ui, &mut s.export_fps, &f2);
    lab(ui, "Video format", "");
    seg(
        ui,
        &mut s.codec,
        &[
            ("h264".to_string(), "H.264", "Plays everywhere"),
            (
                "h265".into(),
                "H.265",
                "Smaller files at the same quality; newer players",
            ),
            (
                "av1".into(),
                "AV1",
                "Smallest files; slower to make without a recent graphics card",
            ),
            (
                "web".into(),
                "Web",
                "VP9 + Opus in WebM – for websites and browsers",
            ),
        ],
    );
    lab(
        ui,
        "Encoder",
        "the graphics card is much faster; the processor makes slightly smaller files",
    );
    let opts: Vec<(String, String)> = settings::ENCODERS
        .iter()
        .filter(|(k, _)| cfg!(windows) || *k != "mf")
        .filter(|(k, _)| cfg!(target_os = "linux") || *k != "vaapi")
        .map(|(k, l)| {
            let note = match (&survey, *k) {
                (_, "auto") | (_, "cpu") => String::new(),
                (None, _) => " – checking …".into(),
                (Some(v), hw) => {
                    if v.iter().any(|(c, h, _)| *c == s.codec && *h == hw) {
                        " ✓".into()
                    } else {
                        " – not here".into()
                    }
                }
            };
            (k.to_string(), format!("{l}{note}"))
        })
        .collect();
    select(ui, "encoder", &mut s.encoder, &opts, 260.0);
    if let Some(v) = &survey {
        let used = if s.encoder == "auto" {
            v.iter()
                .find(|(c, h, _)| *c == s.codec && *h != "cpu")
                .or_else(|| v.iter().find(|(c, _, _)| *c == s.codec))
                .map(|x| x.2.clone())
        } else {
            v.iter()
                .find(|(c, h, _)| *c == s.codec && *h == s.encoder)
                .map(|x| x.2.clone())
        };
        hint(
            ui,
            &match used {
                Some(n) => format!("Encodes with {n}"),
                None => "Not available here – the processor encodes".into(),
            },
        );
    }
    lab(ui, "Preset", "");
    seg(
        ui,
        &mut s.enc_speed,
        &[
            (
                "fast".to_string(),
                "Fastest",
                "Quickest to make, bigger files",
            ),
            ("balanced".into(), "Balanced", ""),
            (
                "small".into(),
                "Smallest file",
                "Slower, smaller files at about the same look",
            ),
        ],
    );
    range(
        ui,
        Icon::Sliders,
        "Quality level",
        &mut s.export_crf,
        12.0,
        40.0,
        1.0,
        &|v| format!("{v:.0}"),
    );
    hint(ui, "Lower = better picture and bigger files (18–23 looks very good; 28+ is for small previews).");
    group_head(ui, "Live show and preview", "");
    let q: Vec<(f64, &str, &str)> = settings::SHOW_QUALITIES
        .iter()
        .map(|(v, l)| (*v, *l, ""))
        .collect();
    seg(ui, &mut s.show_quality, &q);
    hint(ui, "The picture size of the live show and the preview – smaller runs smoother on a slow computer. Exports are never affected.");
    if let Some((m, ok)) = font_msg {
        app.toast(m, ok);
    }
}

// ---------- 4 Go ----------

fn go_card(ui: &mut Ui, app: &mut App, compact: bool) {
    card(ui, Some("4"), "Go", |ui| {
        let s = &app.s;
        if !compact {
            let clip_opts: Vec<&str> = [
                (s.best_spots, "best moments"),
                (s.pace_match, "clip speed follows the music"),
                (s.beat_sync, "motion on the beat"),
                (s.clean_cuts, "clean cuts"),
                (s.smart_crop, "smart crop"),
                (s.match_cut, "match cuts"),
                (s.sequenced, "scene timeline"),
                (s.variety, "variety"),
            ]
            .iter()
            .filter(|x| x.0)
            .map(|x| x.1)
            .collect();
            let src = match s.source.as_str() {
                "image" => "Images",
                "both" => "Videos + images",
                _ => "Videos",
            };
            let (n_roots, n_off) = {
                let l = app.lib.lk();
                (l.roots.len(), l.off.len())
            };
            let where_ = if n_off == 0 || n_roots == 0 {
                "from all folders".to_string()
            } else {
                let n = n_roots - n_off;
                format!("from {} {}", n, if n == 1 { "folder" } else { "folders" })
            };
            sum_row(
                ui,
                "Clips",
                &format!(
                    "{src} {where_}{}",
                    if s.shape != "all" {
                        format!(" · {} only", s.shape)
                    } else {
                        String::new()
                    }
                ),
            );
            sum_row(
                ui,
                "Selection",
                &if clip_opts.is_empty() {
                    "random".into()
                } else {
                    clip_opts.join(", ")
                },
            );
            sum_row(
                ui,
                "Cutting",
                &if app.mode == Mode::Template {
                    "from the template".into()
                } else {
                    tab_sum(s, "cut")
                },
            );
            let look = LOOKS
                .iter()
                .find(|l| l.0 == s.look)
                .map(|l| l.1)
                .unwrap_or("");
            sum_row(
                ui,
                "Effects",
                &format!(
                    "{}{}",
                    tab_sum(s, "fx"),
                    if s.look != "none" {
                        format!(" · look {look}")
                    } else {
                        String::new()
                    }
                ),
            );
            sum_row(
                ui,
                "Sound",
                &format!(
                    "Song {} % · clips {}",
                    s.song_vol.round(),
                    if s.fx.voice {
                        format!(
                            "{} %, {}",
                            s.clip_vol.round(),
                            if s.voice_mode == "always" {
                                "always"
                            } else {
                                "only on drops"
                            }
                        )
                    } else {
                        "off".into()
                    }
                ),
            );
            if s.fs_on {
                sum_row(ui, "Funscript", &tab_sum(s, "fs"));
            }
            sum_row(
                ui,
                "Output",
                &format!("{} · {}", s.format, tab_sum(s, "out")),
            );
        }
        // the buttons
        let song = app.current_song();
        let busy_song = app.mode == Mode::Song
            && app
                .songs
                .get(app.sel)
                .is_some_and(|e| matches!(&*e.state.lk(), Load::Loading(_)));
        let label = if song.is_some() {
            "Start the show"
        } else if busy_song {
            "Reading the song …"
        } else if app.mode == Mode::Template {
            "Pick a PMV first"
        } else {
            "Pick a song first"
        };
        let ok = song.is_some() && app.ff_err.is_none() && app.gen.is_none();
        if wide_btn(ui, Btn::Primary, Some(Icon::Bolt), label, ok)
            .on_hover_text("Plays the PMV live, fullscreen – clips are chosen as it plays")
            .clicked()
        {
            app.go();
        }
        if let Some(job) = &app.gen {
            let (f, m) = job.progress();
            ui.label(egui::RichText::new(m).font(medium(13.5)).color(c().text2));
            progress_bar(ui, f);
            if btn(ui, Btn::Ghost, None, "Cancel", true).clicked() {
                job.cancel.store(true, Ordering::SeqCst);
            }
        } else if wide_btn(ui, Btn::Normal, Some(Icon::Film), "Generate, preview & edit", ok)
            .on_hover_text("Chooses every clip first, then shows the PMV with a timeline: preview it, swap clips, save the project, export the video")
            .clicked()
        {
            app.generate();
        }
        if let Some(job) = &app.opening {
            let (f, m) = job.progress();
            ui.label(
                egui::RichText::new(if m.is_empty() {
                    "Opening the project …".into()
                } else {
                    m
                })
                .font(medium(13.5))
                .color(c().text2),
            );
            progress_bar(ui, f);
        }
        if app.studio.is_some()
            && wide_btn(ui, Btn::Ghost, Some(Icon::Edit), "Back to the edit", true).clicked()
        {
            app.screen = Screen::Studio;
        }
        if !compact {
            hint(ui, "Keys while it runs: Space pause · F fullscreen · H hide the bar · I clip info · M change a clip's moment · N / P next / previous song · Esc stop");
            ui.add_space(8.0);
            my_settings(ui, app);
        }
    });
}

fn my_settings(ui: &mut Ui, app: &mut App) {
    lab(ui, "My settings", "");
    let names: Vec<String> = app.stored.presets.keys().cloned().collect();
    let mut msg: Option<(String, bool)> = None;
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 90.0).max(120.0);
        let shown = if names.is_empty() {
            "Nothing saved yet".to_string()
        } else if app.ui.preset_sel.is_empty() {
            "Load saved settings …".into()
        } else {
            app.ui.preset_sel.clone()
        };
        egui::ComboBox::from_id_salt("mypreset")
            .selected_text(egui::RichText::new(shown).font(text(14.0)))
            .width(w)
            .show_ui(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(app.ui.preset_sel == *n, n).clicked() {
                        if let Some(o) = app.stored.presets.get(n) {
                            app.s = app.s.with_preset(o);
                            app.ui.preset_sel = n.clone();
                            msg = Some((format!("Loaded: {n}"), true));
                        }
                    }
                }
            });
        if btn(ui, Btn::Normal, None, "Save …", true)
            .on_hover_text(
                "Save everything about clips, cutting, effects, look and sound under a name",
            )
            .clicked()
        {
            app.ui.save_name = Some(app.ui.preset_sel.clone());
        }
    });
    if let Some(name) = &mut app.ui.save_name {
        let mut done = None;
        ui.horizontal(|ui| {
            field(ui, name, "Name", (ui.available_width() - 150.0).max(100.0));
            if btn(ui, Btn::Primary, None, "Save", !name.trim().is_empty()).clicked() {
                done = Some(true);
            }
            if btn(ui, Btn::Ghost, None, "Cancel", true).clicked() {
                done = Some(false);
            }
        });
        if done == Some(true) {
            let n = name.trim().to_string();
            app.stored.presets.insert(n.clone(), app.s.snapshot());
            app.ui.preset_sel = n.clone();
            msg = Some((format!("Saved: {n}"), true));
        }
        if done.is_some() {
            app.ui.save_name = None;
        }
    }
    ui.horizontal(|ui| {
        let sel = app.ui.preset_sel.clone();
        if btn(
            ui,
            Btn::Ghost,
            None,
            "Delete",
            app.stored.presets.contains_key(&sel),
        )
        .clicked()
        {
            app.stored.presets.remove(&sel);
            app.ui.preset_sel.clear();
            msg = Some(("Deleted".into(), true));
        }
        if btn(ui, Btn::Ghost, Some(Icon::Download), "Export", true)
            .on_hover_text(
                "As a file – to keep or to share (also for the web version and the Stash plugin)",
            )
            .clicked()
        {
            let n = if sel.trim().is_empty() {
                "PMV settings".to_string()
            } else {
                sel.trim().to_string()
            };
            if let Some(p) = rfd::FileDialog::new()
                .set_file_name(format!(
                    "{}.pmvgen.json",
                    crate::util::safe_file_name(&n, 80)
                ))
                .save_file()
            {
                let j = settings::export_file(&n, &app.s);
                msg = Some(
                    match crate::util::write_atomic(
                        &p,
                        &serde_json::to_vec_pretty(&j).unwrap_or_default(),
                    ) {
                        Ok(_) => ("Exported".into(), true),
                        Err(e) => (format!("Export failed: {e}"), false),
                    },
                );
            }
        }
        if btn(ui, Btn::Ghost, Some(Icon::Upload), "Import", true).clicked() {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("Settings", &["json"])
                .pick_file()
            {
                msg = Some(
                    match std::fs::read(&p)
                        .map_err(anyhow::Error::from)
                        .and_then(|b| settings::import_file(&b))
                    {
                        Ok((n, o)) => {
                            app.s = app.s.with_preset(&o);
                            app.stored.presets.insert(n.clone(), o);
                            app.ui.preset_sel = n.clone();
                            (format!("Imported: {n}"), true)
                        }
                        Err(e) => (format!("Import failed: {e:#}"), false),
                    },
                );
            }
        }
    });
    if let Some((m, ok)) = msg {
        let _ = settings::save_stored(&app.stored);
        app.toast(m, ok);
    }
}
