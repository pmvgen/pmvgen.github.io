// The live show: the picture fullscreen, a bar at the bottom (hides after 2.5 s without moving the mouse),
// the clip info (I) and "Change the moment" (M) – like the web version's show.

use super::stage::Stage;
use super::theme::*;
use super::App;
use crate::library::Kind;
use crate::settings;
use crate::util::{fmt_at, fmt_time};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct Live {
    pub stage: Stage,
    order: Vec<usize>,
    pos: usize,
    /// set when the next song should start (the app does that)
    pub next_song: Option<(Vec<usize>, usize)>,
    /// started from the studio: Esc goes back there
    pub from_studio: bool,
    last_move: Instant,
    bar_hidden: bool,
    info: bool,
    pub moments: Option<Moments>,
    ended: Option<Instant>,
}

/// The "Change the moment" panel's state: was the show playing, and where each clip was taken from first
#[derive(Default)]
pub struct Moments {
    was_playing: bool,
    pub orig: HashMap<usize, f64>,
    /// the slider's value while it is dragged (applied when let go)
    drag: HashMap<usize, f64>,
}

impl Live {
    pub fn new(stage: Stage, order: Vec<usize>, pos: usize) -> Live {
        Live {
            stage,
            order,
            pos,
            next_song: None,
            from_studio: false,
            last_move: Instant::now(),
            bar_hidden: false,
            info: false,
            moments: None,
            ended: None,
        }
    }

    fn toggle_moments(&mut self) {
        if let Some(m) = self.moments.take() {
            if m.was_playing {
                self.stage.play();
            }
        } else {
            let was = self.stage.playing() || self.stage.want_play;
            self.stage.pause();
            self.moments = Some(Moments {
                was_playing: was,
                ..Default::default()
            });
        }
    }

    /// One frame of the show. → true when it ends (Esc / stop)
    pub fn ui(&mut self, ctx: &egui::Context, app: &mut App) -> bool {
        let mut stop = false;
        let (space, f, esc, h, i, m, n, p, moved) = ctx.input(|inp| {
            (
                inp.key_pressed(egui::Key::Space),
                inp.key_pressed(egui::Key::F),
                inp.key_pressed(egui::Key::Escape),
                inp.key_pressed(egui::Key::H),
                inp.key_pressed(egui::Key::I),
                inp.key_pressed(egui::Key::M),
                inp.key_pressed(egui::Key::N),
                inp.key_pressed(egui::Key::P),
                inp.pointer.delta() != Vec2::ZERO || inp.pointer.any_click(),
            )
        });
        let typing = ctx.wants_keyboard_input();
        if moved {
            self.last_move = Instant::now();
        }
        if !typing {
            if space {
                if self.moments.is_some() {
                    self.toggle_moments();
                } else {
                    self.stage.toggle();
                }
            }
            if f {
                let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
            }
            if h {
                self.bar_hidden = !self.bar_hidden;
            }
            if i {
                self.info = !self.info;
            }
            if m {
                self.toggle_moments();
            }
            if n && self.pos + 1 < self.order.len() {
                self.next_song = Some((self.order.clone(), self.pos + 1));
            }
            if p && self.pos > 0 {
                self.next_song = Some((self.order.clone(), self.pos - 1));
            }
        }
        if esc {
            if self.moments.is_some() {
                self.toggle_moments();
            } else {
                stop = true;
            }
        }
        self.stage.update(ctx);
        // the end of the song: the next one, or the end card
        if self.stage.is_ready() && self.stage.player.ended() && self.moments.is_none() {
            let at = *self.ended.get_or_insert_with(Instant::now);
            if self.pos + 1 < self.order.len() && at.elapsed() > Duration::from_millis(600) {
                self.next_song = Some((self.order.clone(), self.pos + 1));
            }
        } else {
            self.ended = None;
        }
        if self.stage.playing() || !self.stage.is_ready() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let ready = self.stage.is_ready();
        let show_bar = !self.bar_hidden
            && (self.last_move.elapsed() < Duration::from_millis(2500)
                || std::env::var_os("PMVGEN_SHOT").is_some()
                || !ready
                || !self.stage.playing()
                || self.moments.is_some());
        if !show_bar {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        } else {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::BLACK))
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                if let Some(tx) = &self.stage.tex {
                    let sz = tx.size_vec2();
                    let k = (avail.width() / sz.x).min(avail.height() / sz.y);
                    let r = Rect::from_center_size(avail.center(), sz * k);
                    ui.painter().image(
                        tx.id(),
                        r,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                if !ready {
                    ui.painter().text(
                        avail.center(),
                        Align2::CENTER_CENTER,
                        "Getting the first clips ready …",
                        display(26.0),
                        c().text,
                    );
                }
                if self.info {
                    self.info_overlay(ui, avail);
                }
                if ready
                    && self.stage.player.ended()
                    && self.pos + 1 >= self.order.len()
                    && self.moments.is_none()
                {
                    if let Some(a) = self.end_card(ui, avail) {
                        match a {
                            0 => {
                                self.stage.seek(0.0);
                                self.stage.want_play = true;
                            }
                            1 => {
                                let sess = self.stage.session();
                                if sess.complete() {
                                    app.studio = None;
                                    app.studio =
                                        Some(Box::new(super::studio::Studio::new(sess, app)));
                                    stop = true;
                                    self.from_studio = true;
                                } else {
                                    app.toast(
                                        "Not every clip was chosen – generate the PMV to edit it",
                                        false,
                                    );
                                }
                            }
                            _ => stop = true,
                        }
                    }
                }
            });
        if show_bar {
            if let Some(a) = self.bar(ctx, app) {
                match a {
                    BarAct::Stop => stop = true,
                    BarAct::Moments => self.toggle_moments(),
                }
            }
        }
        if self.moments.is_some() {
            let mut close = false;
            egui::Area::new(egui::Id::new("moments"))
                .anchor(Align2::RIGHT_TOP, Vec2::new(-16.0, 16.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let t = self.stage.time();
                    let shots = self.stage.on_screen(t);
                    panel_frame().show(ui, |ui| {
                        ui.set_width(380.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Change the moments").font(display(22.0)).color(c().text));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if btn(ui, Btn::Ghost, Some(Icon::Close), "", true).clicked() {
                                    close = true;
                                }
                            });
                        });
                        hint(ui, "Move the slider (or use the buttons) to take a clip from another moment of its video. Pin a moment you like: clips of that video then often start there.");
                        let m = self.moments.as_mut().expect("moments open");
                        egui::ScrollArea::vertical().max_height(ui.ctx().screen_rect().height() * 0.7).show(ui, |ui| {
                            moments_ui(ui, &mut self.stage, app, &shots, m);
                        });
                    });
                });
            if close {
                self.toggle_moments();
            }
        }
        stop
    }

    fn info_overlay(&self, ui: &mut Ui, avail: Rect) {
        let (fields, fails) = self.stage.info();
        let (w, h) = self.stage.size;
        let mut lines = vec![format!(
            "{:.0} fps · {}×{} · {}",
            self.stage.fps.2,
            w,
            h,
            fmt_time(self.stage.time())
        )];
        for (g, name, st) in fields {
            lines.push(format!("Field {g}: {name} – {st}"));
        }
        if !fails.is_empty() {
            lines.push("Left out:".into());
            for f in fails.iter().take(8) {
                lines.push(format!("  {f}"));
            }
        }
        let text_s = lines.join("\n");
        let galley = ui
            .painter()
            .layout(text_s, text(13.5), c().text, avail.width() * 0.6);
        let r = Rect::from_min_size(
            avail.min + Vec2::new(16.0, 16.0),
            galley.size() + Vec2::new(24.0, 20.0),
        );
        ui.painter()
            .rect_filled(r, 10.0, Color32::from_black_alpha(190));
        ui.painter()
            .galley(r.min + Vec2::new(12.0, 10.0), galley, c().text);
    }

    /// The end card: play again, edit, back. → the button pressed
    fn end_card(&self, ui: &mut Ui, avail: Rect) -> Option<u8> {
        let mut out = None;
        let r = Rect::from_center_size(avail.center(), Vec2::new(380.0, 170.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
            panel_frame().show(ui, |ui| {
                ui.set_width(340.0);
                ui.label(
                    egui::RichText::new("The end")
                        .font(display(26.0))
                        .color(c().text),
                );
                hint(
                    ui,
                    "Play it again, edit this PMV (swap clips, save, export) or go back.",
                );
                ui.horizontal_wrapped(|ui| {
                    if btn(ui, Btn::Primary, Some(Icon::Play), "Again", true).clicked() {
                        out = Some(0);
                    }
                    if btn(ui, Btn::Normal, Some(Icon::Edit), "Edit", true).clicked() {
                        out = Some(1);
                    }
                    if btn(ui, Btn::Ghost, None, "Back", true).clicked() {
                        out = Some(2);
                    }
                });
            });
        });
        out
    }

    fn bar(&mut self, ctx: &egui::Context, app: &mut App) -> Option<BarAct> {
        let mut act = None;
        egui::Area::new(egui::Id::new("hud"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                panel_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let playing = self.stage.playing() || self.stage.want_play;
                        if icon_btn(
                            ui,
                            if playing { Icon::Pause } else { Icon::Play },
                            "Pause / play (Space)",
                        )
                        .clicked()
                        {
                            self.stage.toggle();
                        }
                        if self.order.len() > 1 {
                            if icon_btn(ui, Icon::Prev, "Previous song (P)").clicked()
                                && self.pos > 0
                            {
                                self.next_song = Some((self.order.clone(), self.pos - 1));
                            }
                            if icon_btn(ui, Icon::Next, "Next song (N)").clicked()
                                && self.pos + 1 < self.order.len()
                            {
                                self.next_song = Some((self.order.clone(), self.pos + 1));
                            }
                        }
                        let t = self.stage.time();
                        let d = self.stage.duration();
                        ui.label(
                            egui::RichText::new(format!("{} / {}", fmt_time(t), fmt_time(d)))
                                .font(medium(14.0))
                                .color(c().text),
                        );
                        // progress
                        let (r, _) = ui.allocate_exact_size(Vec2::new(140.0, 6.0), Sense::hover());
                        ui.painter().rect_filled(r, 3.0, c().bg3);
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                r.min,
                                Vec2::new(
                                    r.width() * (t / d.max(0.01)).clamp(0.0, 1.0) as f32,
                                    r.height(),
                                ),
                            ),
                            3.0,
                            c().accent,
                        );
                        let plan = &self.stage.sess.plan;
                        let ph = &plan.phases[plan.phase_at(t)];
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {} cuts · {}",
                                self.stage.sess.song.name,
                                plan.cuts_before(t),
                                crate::plan::layout(&ph.layout).name
                            ))
                            .font(text(13.0))
                            .color(c().text2),
                        );
                        ui.separator();
                        let mut sv = self.stage.sess.settings.song_vol;
                        let mut cv = self.stage.sess.settings.clip_vol;
                        let mut ch = false;
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        icon(ui.painter(), r, Icon::Music, c().accent);
                        ch |= ui
                            .add(
                                egui::Slider::new(&mut sv, 0.0..=100.0)
                                    .show_value(false)
                                    .step_by(5.0),
                            )
                            .on_hover_text("Song volume")
                            .changed();
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        icon(
                            ui.painter(),
                            r,
                            Icon::Film,
                            if self.stage.sess.settings.fx.voice {
                                c().accent
                            } else {
                                c().faint
                            },
                        );
                        ch |= ui
                            .add_enabled(
                                self.stage.sess.settings.fx.voice,
                                egui::Slider::new(&mut cv, 0.0..=100.0)
                                    .show_value(false)
                                    .step_by(5.0),
                            )
                            .on_hover_text("Clip volume")
                            .changed();
                        if ch {
                            self.stage.set_volumes(sv, cv);
                            app.s.song_vol = sv;
                            app.s.clip_vol = cv;
                        }
                        ui.separator();
                        if icon_btn(ui, Icon::Edit, "Change the moment a clip is taken from (M)")
                            .clicked()
                        {
                            act = Some(BarAct::Moments);
                        }
                        if icon_btn(ui, Icon::Info, "Clip info (I)").clicked() {
                            self.info = !self.info;
                        }
                        if icon_btn(ui, Icon::Expand, "Fullscreen (F)").clicked() {
                            let fs = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
                        }
                        if icon_btn(ui, Icon::Close, "Stop (Esc)").clicked() {
                            act = Some(BarAct::Stop);
                        }
                    });
                });
            });
        act
    }
}

enum BarAct {
    Stop,
    Moments,
}

/// A floating panel (HUD, moments) – glass or solid
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(if glass() {
            super::themes::rgba(0x34, 0x17, 0x2e, 225)
        } else {
            c().bg2
        })
        .stroke(Stroke::new(
            1.0_f32,
            if glass() { glass_edge() } else { c().bg3 },
        ))
        .corner_radius(if glass() { 18 } else { 8 })
        .inner_margin(egui::Margin::symmetric(14, 10))
        .shadow(egui::Shadow {
            offset: [0, 10],
            blur: 30,
            spread: 0,
            color: Color32::from_black_alpha(140),
        })
}

pub fn icon_btn(ui: &mut Ui, ic: Icon, tip: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(r, if glass() { 255 } else { 4 }, c().bg3);
    }
    icon(
        ui.painter(),
        r.shrink(9.0),
        ic,
        if resp.hovered() { c().text } else { c().text2 },
    );
    resp.on_hover_text(tip)
}

/// "Change the moment" for some shots: a slider over each clip's video, ±1 s, back to the picked moment, and
/// pinning (clips of that video then often start at a pinned moment)
pub fn moments_ui(
    ui: &mut Ui,
    stage: &mut Stage,
    app: &mut App,
    shots: &[(usize, usize)],
    m: &mut Moments,
) {
    if shots.is_empty() {
        hint(ui, "No clip on screen yet.");
        return;
    }
    let mut changes: Vec<(usize, crate::media::Pick)> = Vec::new();
    let mut pins_changed = false;
    for &(field, shot) in shots {
        let Some(pick) = stage.pick(shot) else {
            continue;
        };
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{field}"))
                    .font(display(18.0))
                    .color(c().accent),
            );
            ui.label(
                egui::RichText::new(&pick.name)
                    .font(bold(14.0))
                    .color(c().text),
            )
            .on_hover_text(pick.path.to_string_lossy());
        });
        if pick.kind == Kind::Image {
            hint(ui, "A picture – no moment to change");
            continue;
        }
        m.orig.entry(shot).or_insert(pick.start);
        let dur = pick.dur.max(1.0);
        let mut v = *m.drag.get(&shot).unwrap_or(&pick.start);
        ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(80.0);
        let resp = ui.add(
            egui::Slider::new(&mut v, 0.0..=(dur - 0.6).max(0.0))
                .show_value(false)
                .trailing_fill(true)
                .clamping(egui::SliderClamping::Always),
        );
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} of {}", fmt_at(v), fmt_at(dur)))
                    .font(medium(13.0))
                    .color(c().accent),
            );
            if btn(ui, Btn::Ghost, None, "−1 s", true).clicked() {
                v = (v - 1.0).max(0.0);
                changes.push((shot, pick.with_start(v)));
            }
            if btn(ui, Btn::Ghost, None, "+1 s", true).clicked() {
                v = (v + 1.0).min(dur - 0.6);
                changes.push((shot, pick.with_start(v)));
            }
        });
        if resp.dragged() {
            m.drag.insert(shot, v);
        }
        if resp.drag_stopped() || (resp.clicked() && !resp.dragged()) {
            m.drag.remove(&shot);
            changes.push((shot, pick.with_start(v)));
        }
        let key = pick.path.to_string_lossy().to_string();
        let pins = app.stored.pins.get(&key).cloned().unwrap_or_default();
        let pinned = pins.iter().any(|x| (x - pick.start).abs() < 0.25);
        ui.horizontal_wrapped(|ui| {
            let label = if pinned {
                "Pinned ✓"
            } else {
                "Pin this moment"
            };
            if btn(
                ui,
                if pinned { Btn::Primary } else { Btn::Normal },
                Some(Icon::Pin),
                label,
                true,
            )
            .clicked()
            {
                let mut l = pins.clone();
                if let Some(i) = l.iter().position(|x| (x - pick.start).abs() < 0.25) {
                    l.remove(i);
                } else {
                    l.push(pick.start);
                }
                settings::set_pins(&mut app.stored, &key, &l);
                pins_changed = true;
            }
            if let Some(o) = m.orig.get(&shot) {
                if (o - pick.start).abs() > 0.05
                    && btn(
                        ui,
                        Btn::Ghost,
                        Some(Icon::Undo),
                        "Back to the picked moment",
                        true,
                    )
                    .clicked()
                {
                    changes.push((shot, pick.with_start(*o)));
                }
            }
        });
        if !pins.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("Pinned:")
                        .font(text(12.5))
                        .color(c().faint),
                );
                for x in &pins {
                    if chip(ui, &fmt_at(*x), false, None)
                        .on_hover_text("Take the clip from here")
                        .clicked()
                    {
                        changes.push((shot, pick.with_start(*x)));
                    }
                    if x_btn(ui, "Remove the pin").clicked() {
                        let l: Vec<f64> = pins.iter().copied().filter(|y| y != x).collect();
                        settings::set_pins(&mut app.stored, &key, &l);
                        pins_changed = true;
                    }
                }
            });
        }
    }
    if pins_changed {
        let _ = settings::save_stored(&app.stored);
    }
    if !changes.is_empty() {
        // (measured again in the background: match cuts / smart crop of later picks use it)
        stage.set_picks(&changes);
        for (shot, p) in changes {
            let tx = stage_remeasure_sender();
            std::thread::spawn(move || {
                let p2 = p.remeasured();
                let _ = tx.send((shot, p2));
            });
        }
    }
    // remeasured picks that came back
    while let Ok((shot, p)) = stage_remeasure_receiver().try_recv() {
        if stage
            .pick(shot)
            .is_some_and(|cur| cur.path == p.path && (cur.start - p.start).abs() < 1e-6)
        {
            stage.sess.picks[shot] = Some(p);
        }
    }
}

// (a channel for picks measured again in the background)
type Remeasured = (usize, crate::media::Pick);
type RemeasureChannel = (
    std::sync::Mutex<std::sync::mpsc::Sender<Remeasured>>,
    std::sync::Mutex<std::sync::mpsc::Receiver<Remeasured>>,
);
fn channel() -> &'static RemeasureChannel {
    static C: std::sync::OnceLock<RemeasureChannel> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        (std::sync::Mutex::new(tx), std::sync::Mutex::new(rx))
    })
}
fn stage_remeasure_sender() -> std::sync::mpsc::Sender<Remeasured> {
    use crate::util::LockExt;
    channel().0.lk().clone()
}
struct RxGuard;
impl RxGuard {
    fn try_recv(&self) -> Result<Remeasured, std::sync::mpsc::TryRecvError> {
        use crate::util::LockExt;
        channel().1.lk().try_recv()
    }
}
fn stage_remeasure_receiver() -> RxGuard {
    RxGuard
}
