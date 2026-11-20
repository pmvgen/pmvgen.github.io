// The look of the web version (app.css / stage.css / standalone.css): its colours (or another theme's), fonts,
// the "liquid glass" background and panels, and its components – step cards, segmented pills, option tiles with
// hatched switches, slider rows, collapsible sections, drop zones and line icons – drawn with egui, with the CSS
// transitions as animations.

use super::themes::{self, mix, rgba, Pal};
use eframe::egui::{
    self, emath::easing, epaint::RectShape, Align2, Color32, CornerRadius, FontFamily, FontId, Id,
    Margin, Pos2, Rect, Response, Sense, Shadow, Shape, Stroke, StrokeKind, TextStyle,
    TextureHandle, TextureId, Ui, Vec2,
};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

// ---------- Colours: the active palette ----------

/// The colours of the active theme (the web version's CSS variables)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub dark: bool,
    pub bg: Color32,
    pub bg2: Color32,
    pub bg3: Color32,
    pub base: Color32,
    pub line: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent2: Color32,
    pub accent_deep: Color32,
    pub second: Color32,
    pub shine: Color32,
    pub on_accent: Color32,
    pub paper: Color32,
    pub ok: Color32,
    pub danger: Color32,
    pub sechead_hover: Color32,
}
impl From<&Pal> for Colors {
    fn from(p: &Pal) -> Colors {
        Colors {
            dark: p.dark,
            bg: p.bg,
            bg2: p.bg2,
            bg3: p.bg3,
            base: p.base,
            line: p.line,
            text: p.text,
            text2: p.text2,
            faint: p.faint,
            accent: p.accent,
            accent2: p.accent2,
            accent_deep: p.accent_deep,
            second: p.second,
            shine: p.shine,
            on_accent: p.on_accent,
            // (--paper: the light "paper" offset under badges and the primary button)
            paper: if p.dark { p.text } else { p.bg3 },
            ok: p.ok,
            danger: p.danger,
            sechead_hover: p.sechead_hover,
        }
    }
}

thread_local! {
    static COL: Cell<Colors> = Cell::new(Colors::from(&themes::pmv()));
}

/// The active colours (the window is drawn on one thread – a copy is cheap)
pub fn c() -> Colors {
    COL.with(|c| c.get())
}

/// Make a palette the active one (and egui's widgets follow)
pub fn apply_palette(ctx: &egui::Context, p: &Pal) {
    COL.with(|c| c.set(Colors::from(p)));
    install_style(ctx);
}

/// The colour at `a` of its opacity (premultiplied, in gamma space – like CSS)
pub fn alpha(c: Color32, a: f32) -> Color32 {
    let a = a.clamp(0.0, 1.0);
    let m = |v: u8| (v as f32 * a).round() as u8;
    let [r, g, b, al] = c.to_array();
    Color32::from_rgba_premultiplied(m(r), m(g), m(b), m(al))
}
/// White at an opacity, as CSS rgba(255,255,255,a) blends
pub fn white(a: u8) -> Color32 {
    rgba(255, 255, 255, a)
}
/// The glass edge: rgba(255, 255, 255, .14) on dark themes, a dark line on light ones
pub fn glass_edge() -> Color32 {
    if c().dark {
        white(36)
    } else {
        rgba(0, 0, 0, 26)
    }
}

/// Liquid glass on (the switch at the top right) – read by every widget
static GLASS: AtomicBool = AtomicBool::new(true);
pub fn set_glass(on: bool) {
    GLASS.store(on, Ordering::Relaxed);
}
pub fn glass() -> bool {
    GLASS.load(Ordering::Relaxed)
}

// ---------- Animation ----------

/// 0 → 1 as `on` becomes true over `secs` (eased out, like the CSS transitions' --ease)
pub fn anim(ui: &Ui, id: Id, on: bool, secs: f32) -> f32 {
    easing::cubic_out(ui.ctx().animate_bool_with_time(id, on, secs))
}
/// A value that glides to its target
pub fn glide(ui: &Ui, id: Id, target: f32, secs: f32) -> f32 {
    ui.ctx().animate_value_with_time(id, target, secs)
}

// ---------- Fonts: the CSS font stacks, as the original renders them on this computer ----------

pub const DISPLAY: &str = "display";
pub const BOLD: &str = "bold";
pub const MEDIUM: &str = "medium";
pub const SEMIBOLD: &str = "semibold";

fn font_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        if let Some(w) = std::env::var_os("WINDIR") {
            v.push(PathBuf::from(w).join("Fonts"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(
                PathBuf::from(l)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    } else if cfg!(target_os = "macos") {
        v.push("/System/Library/Fonts".into());
        v.push("/System/Library/Fonts/Supplemental".into());
        v.push("/Library/Fonts".into());
        if let Some(h) = dirs::home_dir() {
            v.push(h.join("Library/Fonts"));
        }
    } else {
        v.push("/usr/share/fonts".into());
        v.push("/usr/local/share/fonts".into());
        if let Some(h) = dirs::home_dir() {
            v.push(h.join(".local/share/fonts"));
            v.push(h.join(".fonts"));
        }
    }
    v
}

/// All font files below the font folders (a few levels deep): lower-case name → path
fn font_files() -> std::collections::HashMap<String, PathBuf> {
    let mut out = std::collections::HashMap::new();
    let mut stack: Vec<(PathBuf, usize)> = font_dirs().into_iter().map(|d| (d, 0)).collect();
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
            } else if let Some(n) = p.file_name() {
                let n = n.to_string_lossy().to_lowercase();
                if n.ends_with(".ttf") || n.ends_with(".otf") {
                    out.entry(n).or_insert(p);
                }
            }
        }
    }
    out
}

fn first_font(
    files: &std::collections::HashMap<String, PathBuf>,
    names: &[&str],
) -> Option<Vec<u8>> {
    for n in names {
        if let Some(p) = files.get(&n.to_lowercase()) {
            if let Ok(b) = std::fs::read(p) {
                if ttf_parser::Face::parse(&b, 0).is_ok() {
                    return Some(b);
                }
            }
        }
    }
    None
}

/// The fonts the web version ends up with. --f-text (Bahnschrift, DIN Alternate, Segoe UI, …, system-ui) is
/// Bahnschrift on Windows and the system UI font – Noto Sans – on Linux; --f-display ("Bahnschrift
/// Condensed", Bahnschrift, "Arial Narrow", …) lands on Arial's metric twin on Linux: Liberation Sans Bold
/// (measured in Chrome on this kind of system).
pub fn install_fonts(ctx: &egui::Context) {
    let files = font_files();
    let text = first_font(
        &files,
        &[
            "bahnschrift.ttf",
            "DIN Alternate Bold.ttf",
            "segoeui.ttf",
            "NotoSans-Regular.ttf",
            "Ubuntu-R.ttf",
            "DejaVuSans.ttf",
            "LiberationSans-Regular.ttf",
        ],
    );
    let medium = first_font(
        &files,
        // (weight 500: the Medium face where there is one – else, like the browser, the Regular one)
        &[
            "bahnschrift.ttf",
            "DIN Alternate Bold.ttf",
            "segoeui.ttf",
            "NotoSans-Medium.ttf",
            "NotoSans-Regular.ttf",
            "DejaVuSans.ttf",
        ],
    );
    let semibold = first_font(
        &files,
        // (weight 600: SemiBold – else, like the browser, Bold)
        &[
            "bahnschrift.ttf",
            "seguisb.ttf",
            "NotoSans-SemiBold.ttf",
            "NotoSans-Bold.ttf",
            "DejaVuSans-Bold.ttf",
        ],
    );
    let bold = first_font(
        &files,
        &[
            "DIN Alternate Bold.ttf",
            "segoeuib.ttf",
            "NotoSans-Bold.ttf",
            "Ubuntu-B.ttf",
            "DejaVuSans-Bold.ttf",
            "LiberationSans-Bold.ttf",
            "bahnschrift.ttf",
        ],
    );
    let display = first_font(
        &files,
        &[
            "LiberationSans-Bold.ttf",
            "arialbd.ttf",
            "Arial Bold.ttf",
            "Arimo-Bold.ttf",
            "NotoSans-Bold.ttf",
            "DejaVuSans-Bold.ttf",
        ],
    );
    static ANTON: &[u8] = include_bytes!("../../assets/Anton-Regular.ttf");
    let mut defs = egui::FontDefinitions::default();
    let have_text = text.is_some();
    for (key, data) in [
        ("pmv-text", text),
        ("pmv-medium", medium),
        ("pmv-semibold", semibold),
        ("pmv-bold", bold),
        ("pmv-display", display),
    ] {
        let bytes = data.unwrap_or_else(|| ANTON.to_vec());
        defs.font_data.insert(
            key.to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
    }
    let base: Vec<String> = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let with = |first: &str| -> Vec<String> {
        let mut v = vec![first.to_string()];
        v.extend(base.iter().cloned());
        v
    };
    if have_text {
        defs.families
            .insert(FontFamily::Proportional, with("pmv-text"));
    }
    for (fam, key) in [
        (MEDIUM, "pmv-medium"),
        (SEMIBOLD, "pmv-semibold"),
        (BOLD, "pmv-bold"),
        (DISPLAY, "pmv-display"),
    ] {
        defs.families
            .insert(FontFamily::Name(fam.into()), with(key));
    }
    ctx.set_fonts(defs);
}

pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}
pub fn text(size: f32) -> FontId {
    FontId::proportional(size)
}

/// egui's own widgets in the theme's colours
pub fn install_style(ctx: &egui::Context) {
    let k = c();
    ctx.style_mut(|s| {
        let v = &mut s.visuals;
        *v = if k.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.override_text_color = Some(k.text);
        v.panel_fill = k.bg;
        v.window_fill = k.bg2;
        v.window_stroke = Stroke::new(1.0_f32, k.bg3);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(10);
        v.extreme_bg_color = k.base;
        v.faint_bg_color = k.bg2;
        v.code_bg_color = k.base;
        v.hyperlink_color = k.accent;
        v.selection.bg_fill = alpha(k.accent, 0.55);
        v.selection.stroke = Stroke::new(1.0_f32, k.accent);
        v.text_cursor.stroke = Stroke::new(2.0_f32, k.accent);
        let w = &mut v.widgets;
        let r = CornerRadius::same(4);
        w.noninteractive.bg_fill = k.bg2;
        w.noninteractive.weak_bg_fill = k.bg2;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, k.bg3);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, k.text2);
        w.noninteractive.corner_radius = r;
        // (bg_fill is also the rail of sliders – it has to show on the page)
        w.inactive.bg_fill = k.bg3;
        w.inactive.weak_bg_fill = k.bg2;
        w.inactive.bg_stroke = Stroke::new(1.0_f32, k.bg3);
        w.inactive.fg_stroke = Stroke::new(1.0_f32, k.text);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = k.bg3;
        w.hovered.weak_bg_fill = k.bg3;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, k.faint);
        w.hovered.fg_stroke = Stroke::new(1.0_f32, k.text);
        w.hovered.corner_radius = r;
        w.active.bg_fill = k.accent;
        w.active.weak_bg_fill = k.accent;
        w.active.bg_stroke = Stroke::new(1.0_f32, k.accent);
        w.active.fg_stroke = Stroke::new(1.0_f32, k.on_accent);
        w.active.corner_radius = r;
        w.open.bg_fill = k.bg3;
        w.open.weak_bg_fill = k.bg3;
        w.open.bg_stroke = Stroke::new(1.0_f32, k.accent);
        w.open.corner_radius = r;
        v.popup_shadow = Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(if k.dark { 110 } else { 40 }),
        };
        v.window_shadow = v.popup_shadow;
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.combo_height = 320.0;
        s.text_styles = [
            (TextStyle::Heading, display(24.0)),
            (TextStyle::Body, text(15.0)),
            (TextStyle::Button, medium(14.0)),
            (TextStyle::Small, text(12.5)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        s.animation_time = 0.18;
    });
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}

// ---------- The page background and the glass's backdrop ----------

fn px(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// The page background as pictures (rendered once per window size and theme): the base colour, in glass mode
/// three soft colour glows, and the halftone dots fading out towards the top right (body::before). The second
/// picture is what glass panels show through (backdrop-filter: blur(22px) saturate(170%)): the glows without
/// the dots, a little more saturated.
pub fn background_images(w: u32, h: u32, glass: bool) -> (egui::ColorImage, egui::ColorImage) {
    use tiny_skia::{
        Color, FillRule, GradientStop, Paint, PathBuilder, Pixmap, Point, RadialGradient,
        SpreadMode, Transform,
    };
    let k = c();
    let (w, h) = (w.clamp(16, 8192), h.clamp(16, 8192));
    let Some(mut pm) = Pixmap::new(w, h) else {
        let one = egui::ColorImage::new([1, 1], k.bg);
        return (one.clone(), one);
    };
    pm.fill(Color::from_rgba8(k.bg.r(), k.bg.g(), k.bg.b(), 255));
    let (wf, hf) = (w as f32, h as f32);
    let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, wf, hf);
    let glow = |pm: &mut Pixmap, cx: f32, cy: f32, r: f32, c: [u8; 3], a: f32| {
        let Some(sh) = RadialGradient::new(
            Point::from_xy(cx, cy),
            Point::from_xy(cx, cy),
            r,
            vec![
                GradientStop::new(0.0, Color::from_rgba8(c[0], c[1], c[2], (a * 255.0) as u8)),
                GradientStop::new(0.7, Color::from_rgba8(c[0], c[1], c[2], 0)),
                GradientStop::new(1.0, Color::from_rgba8(c[0], c[1], c[2], 0)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            return;
        };
        let p = Paint {
            shader: sh,
            anti_alias: true,
            ..Default::default()
        };
        if let Some(r) = rect {
            pm.fill_rect(r, &p, Transform::identity(), None);
        }
    };
    if glass {
        // radial-gradient(40vw at 12% 18%, pink 34%) · (45vw at 88% 12%, violet 26%) · (50vw at 70% 92%, pink 22% + shine 10%)
        let s = if k.dark { 1.0 } else { 0.55 };
        glow(
            &mut pm,
            wf * 0.12,
            hf * 0.18,
            wf * 0.40,
            px(k.accent),
            0.34 * s,
        );
        glow(
            &mut pm,
            wf * 0.88,
            hf * 0.12,
            wf * 0.45,
            px(k.second),
            0.26 * s,
        );
        glow(
            &mut pm,
            wf * 0.70,
            hf * 0.92,
            wf * 0.50,
            px(mix(k.accent, k.shine, 0.3)),
            0.30 * s,
        );
    }
    // the backdrop: blurred glows, more saturated, no dots
    let back = {
        let mut b = pm.clone();
        for px in b.data_mut().as_chunks_mut::<4>().0 {
            let (r, g, bl) = (px[0] as f32, px[1] as f32, px[2] as f32);
            let l = 0.299 * r + 0.587 * g + 0.114 * bl;
            let sat = |v: f32| (l + (v - l) * 1.7).clamp(0.0, 255.0) as u8;
            px[0] = sat(r);
            px[1] = sat(g);
            px[2] = sat(bl);
        }
        egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], b.data())
    };
    // halftone: dots every 7 px (radius ~1.4), accent at 22 %, masked by an ellipse at the top right
    let dots_a = if glass { 0.35 } else { 1.0 };
    let mut pb = PathBuilder::new();
    let (ex, ey) = (wf * 0.7, hf * 0.6);
    let mut y = 3.5f32;
    while y < hf {
        let mut x = 3.5f32;
        while x < wf {
            let dx = (x - wf) / ex;
            let dy = y / ey;
            let d = (dx * dx + dy * dy).sqrt();
            if d < 0.7 {
                let m = 1.0 - d / 0.7;
                pb.push_circle(x, y, 1.4 * m.sqrt().max(0.25));
            }
            x += 7.0;
        }
        y += 7.0;
    }
    if let Some(path) = pb.finish() {
        let mut p = Paint::default();
        p.set_color_rgba8(
            k.accent.r(),
            k.accent.g(),
            k.accent.b(),
            (0.22 * dots_a * 255.0) as u8,
        );
        p.anti_alias = true;
        pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
    (
        egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pm.data()),
        back,
    )
}

thread_local! {
    /// the backdrop of the glass this frame: its texture and the screen rect it covers
    static BACKDROP: Cell<Option<(TextureId, Rect)>> = const { Cell::new(None) };
    static GRADIENTS: RefCell<Option<Gradients>> = const { RefCell::new(None) };
}

/// Keeps the background pictures in step with the window size, the theme and the glass switch
#[derive(Default)]
pub struct Background {
    tex: Option<(TextureHandle, TextureHandle)>,
    key: (u32, u32, bool, Option<Colors>),
}
impl Background {
    pub fn paint(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
        let ppp = ctx.pixels_per_point();
        // (half the pixels is plenty for soft glows; the dots stay crisp enough)
        let scale = (ppp * 0.5).max(0.5);
        let (w, h) = (
            (rect.width() * scale).round().max(16.0) as u32,
            (rect.height() * scale).round().max(16.0) as u32,
        );
        let key = (w / 32, h / 32, glass(), Some(c()));
        if self.tex.is_none() || key != self.key {
            let (img, back) = background_images(w, h, glass());
            let opts = egui::TextureOptions::LINEAR;
            match &mut self.tex {
                Some((a, b)) => {
                    a.set(img, opts);
                    b.set(back, opts);
                }
                None => {
                    self.tex = Some((
                        ctx.load_texture("pmv-background", img, opts),
                        ctx.load_texture("pmv-backdrop", back, opts),
                    ))
                }
            }
            self.key = key;
        }
        if let Some((t, b)) = &self.tex {
            painter.image(
                t.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            BACKDROP.with(|c| c.set(Some((b.id(), rect))));
        }
    }
}

/// White gradient textures (tinted when drawn): the option tiles' fade (90deg, colour → transparent at 70 %)
/// and the glass shine (135deg: .13 → .03 at 38 % → 0 at 60 % → .05)
struct Gradients {
    fade: TextureHandle,
    shine: TextureHandle,
}
fn gradients(ctx: &egui::Context) -> (TextureId, TextureId) {
    GRADIENTS.with(|g| {
        let mut g = g.borrow_mut();
        let gr = g.get_or_insert_with(|| {
            let n = 128usize;
            let fade: Vec<Color32> = (0..n)
                .map(|i| {
                    let t = i as f32 / (n - 1) as f32;
                    white(((1.0 - t / 0.7).max(0.0) * 255.0) as u8)
                })
                .collect();
            let m = 64usize;
            let mut shine = Vec::with_capacity(m * m);
            for y in 0..m {
                for x in 0..m {
                    let t = (x + y) as f32 / (2 * (m - 1)) as f32;
                    let a = if t < 0.38 {
                        0.13 + (0.03 - 0.13) * t / 0.38
                    } else if t < 0.6 {
                        0.03 * (1.0 - (t - 0.38) / 0.22)
                    } else {
                        0.05 * (t - 0.6) / 0.4
                    };
                    shine.push(white((a * 255.0) as u8));
                }
            }
            let opts = egui::TextureOptions::LINEAR;
            Gradients {
                fade: ctx.load_texture(
                    "pmv-fade",
                    egui::ColorImage {
                        size: [n, 1],
                        pixels: fade,
                    },
                    opts,
                ),
                shine: ctx.load_texture(
                    "pmv-shine",
                    egui::ColorImage {
                        size: [m, m],
                        pixels: shine,
                    },
                    opts,
                ),
            }
        });
        (gr.fade.id(), gr.shine.id())
    })
}

const FULL_UV: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));

/// A rounded rect as a triangle fan with texture coordinates spread over it: `uv` maps to `rect`. (egui's own
/// textured RectShape samples one corner of the texture when the corners are rounded.) The fan is inset half a
/// pixel – the border drawn over it keeps the edge smooth.
pub fn textured_rrect(
    rect: Rect,
    radius: impl Into<CornerRadius>,
    tex: TextureId,
    uv: Rect,
    tint: Color32,
) -> Shape {
    let r = rect.shrink(0.5);
    let crr: CornerRadius = radius.into();
    let lim = r.width().min(r.height()) / 2.0;
    let rad_of = |v: u8| (v as f32 - 0.5).clamp(0.0, lim);
    let mut mesh = egui::Mesh::with_texture(tex);
    let to_uv = |p: Pos2| {
        Pos2::new(
            uv.left() + (p.x - rect.left()) / rect.width().max(1e-3) * uv.width(),
            uv.top() + (p.y - rect.top()) / rect.height().max(1e-3) * uv.height(),
        )
    };
    let center = r.center();
    mesh.vertices.push(egui::epaint::Vertex {
        pos: center,
        uv: to_uv(center),
        color: tint,
    });
    let (ne, se, sw, nw) = (
        rad_of(crr.ne),
        rad_of(crr.se),
        rad_of(crr.sw),
        rad_of(crr.nw),
    );
    let corners = [
        (
            Pos2::new(r.right() - ne, r.top() + ne),
            -std::f32::consts::FRAC_PI_2,
            ne,
        ),
        (Pos2::new(r.right() - se, r.bottom() - se), 0.0, se),
        (
            Pos2::new(r.left() + sw, r.bottom() - sw),
            std::f32::consts::FRAC_PI_2,
            sw,
        ),
        (
            Pos2::new(r.left() + nw, r.top() + nw),
            std::f32::consts::PI,
            nw,
        ),
    ];
    for (c0, a0, rad) in corners {
        let segs = if rad > 0.5 { 8 } else { 0 };
        for i in 0..=segs {
            let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / segs.max(1) as f32;
            let p = Pos2::new(c0.x + rad * a.cos(), c0.y + rad * a.sin());
            mesh.vertices.push(egui::epaint::Vertex {
                pos: p,
                uv: to_uv(p),
                color: tint,
            });
        }
    }
    let n = mesh.vertices.len() as u32;
    for i in 1..n {
        let j = if i + 1 < n { i + 1 } else { 1 };
        mesh.add_triangle(0, i, j);
    }
    Shape::mesh(mesh)
}

/// A rounded rect filled with a texture (anti-aliased, the corners exact)
fn textured(
    painter: &egui::Painter,
    rect: Rect,
    cr: CornerRadius,
    tex: TextureId,
    uv: Rect,
    tint: Color32,
) {
    painter.add(textured_rrect(rect, cr, tex, uv, tint));
}

/// The rounded "inset 0 1px 0 rgba(255,255,255,a)" highlight: a hairline along the top edge that follows the
/// corners and fades out down their curve
pub fn inset_highlight(painter: &egui::Painter, rect: Rect, cr: f32, a: f32) {
    if !c().dark {
        return;
    }
    let band = Rect::from_min_max(
        rect.min,
        Pos2::new(rect.right(), rect.top() + (cr * 0.55).max(2.0)),
    );
    painter
        .with_clip_rect(band.intersect(painter.clip_rect()))
        .rect_stroke(
            rect.shrink(0.5),
            cr - 0.5,
            Stroke::new(1.0_f32, white((a * 255.0) as u8)),
            StrokeKind::Inside,
        );
}

/// A glass panel's body: what lies behind it, blurred (backdrop-filter), its tint and the shine.
/// Without glass: a plain fill. (The edge and the inset highlight are painted over the content.)
pub struct Panel {
    pub radius: f32,
    pub fill: Color32,
    pub shine: bool,
    pub backdrop: bool,
}

/// The accent fade over a checked option tile (linear-gradient 90deg, accent → transparent at 70 %), clipped to
/// the tile's rounded corners
pub fn fade_fill(painter: &egui::Painter, rect: Rect, radius: f32, color: Color32) {
    let (fade, _) = gradients(painter.ctx());
    textured(
        painter,
        rect,
        CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8),
        fade,
        FULL_UV,
        color,
    );
}

// ---------- Painting helpers ----------

/// The hatching (repeating-linear-gradient(-60deg, accent 0 2px, transparent 2px 5px)) inside a rect
pub fn hatch(painter: &egui::Painter, rect: Rect, color: Color32) {
    let p = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let step = 5.0;
    let slope = (60f32).to_radians().tan();
    let h = rect.height();
    let mut x = rect.left() - h / slope - step;
    while x < rect.right() + step {
        let a = Pos2::new(x, rect.bottom());
        let b = Pos2::new(x + h / slope, rect.top());
        p.line_segment([a, b], Stroke::new(2.0_f32, color));
        x += step;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Music,
    Folder,
    Film,
    Bolt,
    Sliders,
    Plus,
    Download,
    Upload,
    Eye,
    Check,
    Edit,
    Play,
    Pause,
    Chevron,
    ChevronUp,
    Heart,
    Close,
    Pin,
    Save,
    Info,
    Expand,
    Prev,
    Next,
    Undo,
    Dice,
}

/// A line icon (24×24 design, 2 px strokes) in `rect`
pub fn icon(painter: &egui::Painter, rect: Rect, which: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - Vec2::splat(12.0 * s);
    let p = |x: f32, y: f32| Pos2::new(o.x + x * s, o.y + y * s);
    let st = Stroke::new((2.0 * s).max(1.2), color);
    let line = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        painter.add(Shape::line(v, st));
    };
    let closed = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        painter.add(Shape::closed_line(v, st));
    };
    let filled = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        painter.add(Shape::convex_polygon(v, color, Stroke::NONE));
    };
    match which {
        Icon::Music => {
            line(&[(9.0, 18.0), (9.0, 5.0), (20.0, 3.0), (20.0, 16.0)]);
            painter.circle_stroke(p(6.5, 18.0), 2.6 * s, st);
            painter.circle_stroke(p(17.5, 16.0), 2.6 * s, st);
        }
        Icon::Folder => closed(&[
            (3.0, 6.0),
            (9.0, 6.0),
            (11.0, 8.0),
            (21.0, 8.0),
            (21.0, 19.0),
            (3.0, 19.0),
        ]),
        Icon::Film => {
            painter.rect_stroke(
                Rect::from_min_max(p(3.0, 4.0), p(21.0, 20.0)),
                CornerRadius::same((2.0 * s) as u8),
                st,
                StrokeKind::Middle,
            );
            line(&[(8.0, 4.0), (8.0, 20.0)]);
            line(&[(16.0, 4.0), (16.0, 20.0)]);
            line(&[(3.0, 12.0), (21.0, 12.0)]);
        }
        Icon::Bolt => filled(&[
            (13.0, 2.0),
            (4.0, 14.0),
            (11.0, 14.0),
            (10.0, 22.0),
            (20.0, 9.0),
            (13.0, 9.0),
        ]),
        Icon::Sliders => {
            line(&[(4.0, 7.0), (20.0, 7.0)]);
            line(&[(4.0, 17.0), (20.0, 17.0)]);
            painter.circle_filled(p(9.0, 7.0), 2.4 * s, color);
            painter.circle_filled(p(15.0, 17.0), 2.4 * s, color);
        }
        Icon::Plus => {
            line(&[(12.0, 5.0), (12.0, 19.0)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        Icon::Download => {
            line(&[(12.0, 4.0), (12.0, 15.0)]);
            line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]);
            line(&[(5.0, 20.0), (19.0, 20.0)]);
        }
        Icon::Upload => {
            line(&[(12.0, 15.0), (12.0, 4.0)]);
            line(&[(7.0, 9.0), (12.0, 4.0), (17.0, 9.0)]);
            line(&[(5.0, 20.0), (19.0, 20.0)]);
        }
        Icon::Eye => {
            line(&[
                (2.0, 12.0),
                (7.0, 6.5),
                (12.0, 5.0),
                (17.0, 6.5),
                (22.0, 12.0),
                (17.0, 17.5),
                (12.0, 19.0),
                (7.0, 17.5),
                (2.0, 12.0),
            ]);
            painter.circle_stroke(p(12.0, 12.0), 3.0 * s, st);
        }
        Icon::Check => line(&[(5.0, 12.5), (9.5, 17.0), (19.0, 7.5)]),
        Icon::Edit => {
            closed(&[
                (4.0, 20.0),
                (5.0, 15.0),
                (16.0, 4.0),
                (20.0, 8.0),
                (9.0, 19.0),
            ]);
            line(&[(13.5, 6.5), (17.5, 10.5)]);
        }
        Icon::Play => filled(&[(7.0, 4.0), (20.0, 12.0), (7.0, 20.0)]),
        Icon::Pause => {
            painter.rect_filled(Rect::from_min_max(p(6.0, 4.0), p(10.0, 20.0)), 1.0, color);
            painter.rect_filled(Rect::from_min_max(p(14.0, 4.0), p(18.0, 20.0)), 1.0, color);
        }
        Icon::Chevron => line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
        Icon::ChevronUp => line(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)]),
        Icon::Heart => {
            // M16 26s-8-4.9-10.2-9.9C4.2 12.5 6.4 8.5 10.3 8.5c2.3 0 4.1 1.4 5.7 3.4 1.5-2 3.3-3.4 5.6-3.4 3.9 0 6.2 4 4.6 7.6C24 21.1 16 26 16 26z (32 grid)
            let k = 24.0 / 32.0;
            let pts: Vec<Pos2> = [
                (16.0, 26.0),
                (10.5, 22.0),
                (5.8, 16.1),
                (5.0, 12.0),
                (7.0, 9.4),
                (10.3, 8.5),
                (13.6, 9.6),
                (16.0, 11.9),
                (18.3, 9.6),
                (21.6, 8.5),
                (25.0, 9.4),
                (26.9, 12.0),
                (26.2, 16.1),
                (21.5, 22.0),
            ]
            .iter()
            .map(|&(x, y)| p(x * k, y * k))
            .collect();
            // (a heart isn't convex: two halves)
            let mid = p(16.0 * k, 26.0 * k);
            let left: Vec<Pos2> = pts[..8].to_vec();
            let mut right: Vec<Pos2> = pts[7..].to_vec();
            right.push(mid);
            painter.add(Shape::convex_polygon(left, color, Stroke::NONE));
            painter.add(Shape::convex_polygon(right, color, Stroke::NONE));
            painter.add(Shape::convex_polygon(
                vec![p(5.8 * k, 16.1 * k), pts[7], p(26.2 * k, 16.1 * k), mid],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Pin => {
            closed(&[
                (9.0, 3.0),
                (15.0, 3.0),
                (14.0, 9.0),
                (18.0, 13.0),
                (6.0, 13.0),
                (10.0, 9.0),
            ]);
            line(&[(12.0, 13.0), (12.0, 21.0)]);
        }
        Icon::Save => {
            closed(&[
                (4.0, 4.0),
                (17.0, 4.0),
                (20.0, 7.0),
                (20.0, 20.0),
                (4.0, 20.0),
            ]);
            line(&[(8.0, 4.0), (8.0, 9.0), (15.0, 9.0), (15.0, 4.0)]);
            painter.rect_stroke(
                Rect::from_min_max(p(7.0, 13.0), p(17.0, 20.0)),
                0.0,
                st,
                StrokeKind::Middle,
            );
        }
        Icon::Info => {
            painter.circle_stroke(p(12.0, 12.0), 9.0 * s, st);
            line(&[(12.0, 11.0), (12.0, 17.0)]);
            painter.circle_filled(p(12.0, 7.5), 1.3 * s, color);
        }
        Icon::Expand => {
            line(&[(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)]);
            line(&[(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)]);
            line(&[(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)]);
            line(&[(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]);
        }
        Icon::Prev => {
            filled(&[(18.0, 5.0), (8.0, 12.0), (18.0, 19.0)]);
            line(&[(6.0, 5.0), (6.0, 19.0)]);
        }
        Icon::Next => {
            filled(&[(6.0, 5.0), (16.0, 12.0), (6.0, 19.0)]);
            line(&[(18.0, 5.0), (18.0, 19.0)]);
        }
        Icon::Undo => {
            line(&[(9.0, 5.0), (4.0, 10.0), (9.0, 15.0)]);
            line(&[
                (4.0, 10.0),
                (15.0, 10.0),
                (19.0, 13.0),
                (19.0, 17.0),
                (16.0, 20.0),
                (11.0, 20.0),
            ]);
        }
        Icon::Dice => {
            painter.rect_stroke(
                Rect::from_min_max(p(4.0, 4.0), p(20.0, 20.0)),
                CornerRadius::same((3.0 * s) as u8),
                st,
                StrokeKind::Middle,
            );
            for (x, y) in [
                (8.5, 8.5),
                (15.5, 15.5),
                (12.0, 12.0),
                (15.5, 8.5),
                (8.5, 15.5),
            ] {
                painter.circle_filled(p(x, y), 1.4 * s, color);
            }
        }
    }
}

/// Layout pictogram (layoutIcon of the web version: 24×15)
pub fn layout_icon(painter: &egui::Painter, rect: Rect, id: &str, color: Color32) {
    let s = (rect.width() / 24.0).min(rect.height() / 15.0);
    let o = rect.center() - Vec2::new(12.0 * s, 7.5 * s);
    let r = |x: f32, y: f32, w: f32, h: f32, on: bool| {
        let rr = Rect::from_min_size(Pos2::new(o.x + x * s, o.y + y * s), Vec2::new(w * s, h * s));
        if on {
            painter.rect_filled(rr, s, color);
        } else {
            painter.rect_stroke(rr, s, Stroke::new(1.2 * s, color), StrokeKind::Inside);
        }
    };
    match id {
        "kaleido" => {
            r(1.0, 1.0, 10.5, 6.0, true);
            r(12.5, 1.0, 10.5, 6.0, true);
            r(1.0, 8.0, 10.5, 6.0, true);
            r(12.5, 8.0, 10.5, 6.0, true);
        }
        "duo" => {
            r(1.0, 1.0, 10.5, 13.0, true);
            r(12.5, 1.0, 10.5, 13.0, false);
        }
        "trim" => {
            r(1.0, 1.0, 6.7, 13.0, true);
            r(8.7, 1.0, 6.6, 13.0, false);
            r(16.3, 1.0, 6.7, 13.0, true);
        }
        "tri" => {
            r(1.0, 1.0, 6.7, 13.0, true);
            r(8.7, 1.0, 6.6, 13.0, false);
            r(16.3, 1.0, 6.7, 13.0, false);
        }
        "quad" => {
            r(1.0, 1.0, 10.5, 6.0, true);
            r(12.5, 1.0, 10.5, 6.0, false);
            r(1.0, 8.0, 10.5, 6.0, false);
            r(12.5, 8.0, 10.5, 6.0, true);
        }
        _ => r(1.0, 1.0, 22.0, 13.0, true),
    }
}

// ---------- Components ----------

/// Corner radius helper
fn cr(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}

/// The surface of option tiles, slider rows, sections and drop zones (.kb-pmvg-opt …; glass: 14 px, see-through)
fn tile_style(hovered: f32) -> (f32, Color32, Color32) {
    let k = c();
    if glass() {
        (
            14.0,
            alpha(k.base, 0.42 * 0.9),
            mix(glass_edge(), k.faint, hovered),
        )
    } else {
        (5.0, k.base, mix(k.bg3, k.faint, hovered))
    }
}

/// A card (.kb-card): glass – blurred backdrop, 42 % tint, shine, light edge, 18 px corners, a soft shadow
pub fn card<R>(ui: &mut Ui, step: Option<&str>, title: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    let k = c();
    let g = glass();
    let slot = ui.painter().add(Shape::Noop);
    let resp = egui::Frame::new()
        // (padding 20 22 18 plus the 1 px border – CSS counts it outside the padding)
        .inner_margin(Margin {
            left: 23,
            right: 23,
            top: 21,
            bottom: 19,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 12.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if let Some(n) = step {
                    step_badge(ui, n);
                }
                ui.label(egui::RichText::new(title).font(display(25.0)).color(k.text));
            });
            body(ui)
        });
    let rect = resp.response.rect;
    let radius = if g { 18.0 } else { 6.0 };
    let mut shapes: Vec<Shape> = Vec::new();
    if g {
        shapes.push(Shape::Rect(
            Shadow {
                offset: [0, 12],
                blur: 34,
                spread: 0,
                color: Color32::from_black_alpha(if k.dark { 82 } else { 26 }),
            }
            .as_shape(rect, cr(radius)),
        ));
    }
    // (backdrop, tint and shine under the content; edge and highlight over it)
    let p = Panel {
        radius,
        fill: if g { alpha(k.bg2, 0.42) } else { k.bg2 },
        shine: g,
        backdrop: g,
    };
    shapes.extend(panel_shapes(ui.ctx(), rect, &p));
    ui.painter().set(slot, Shape::Vec(shapes));
    ui.painter().rect_stroke(
        rect,
        cr(radius),
        Stroke::new(1.0_f32, if g { glass_edge() } else { k.bg3 }),
        StrokeKind::Inside,
    );
    if g {
        inset_highlight(ui.painter(), rect, radius, 0.22);
    }
    resp.inner
}

/// The shapes of a panel's body (backdrop, fill, shine) – for painting under content added later
pub fn panel_shapes(ctx: &egui::Context, rect: Rect, p: &Panel) -> Vec<Shape> {
    let r = cr(p.radius);
    let mut v = Vec::new();
    if p.backdrop {
        if let Some((tex, screen)) = BACKDROP.with(|c| c.get()) {
            let uv = Rect::from_min_max(
                Pos2::new(
                    (rect.left() - screen.left()) / screen.width(),
                    (rect.top() - screen.top()) / screen.height(),
                ),
                Pos2::new(
                    (rect.right() - screen.left()) / screen.width(),
                    (rect.bottom() - screen.top()) / screen.height(),
                ),
            );
            v.push(textured_rrect(rect, r, tex, uv, Color32::WHITE));
        }
    }
    v.push(Shape::Rect(RectShape::filled(rect, r, p.fill)));
    if p.shine {
        let (_, shine) = gradients(ctx);
        v.push(textured_rrect(rect, r, shine, FULL_UV, Color32::WHITE));
    }
    v
}

/// The numbered step badge (.kb-pmvg-no): 30 px, accent, a paper shadow 2 px down-right
pub fn step_badge(ui: &mut Ui, n: &str) {
    let k = c();
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect.translate(Vec2::splat(2.0)), 4.0, k.paper);
    p.rect_filled(rect, 4.0, k.accent);
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        n,
        display(18.0),
        k.on_accent,
    );
}

/// Field label (.kb-lab-t, 14 px) with an optional small note (12 px, faint)
pub fn lab(ui: &mut Ui, title: &str, small: &str) {
    // (one text block of exactly its own height – egui rows would pad it to the controls' 30 px)
    let k = c();
    let mut job = egui::text::LayoutJob::default();
    job.append(
        title,
        0.0,
        egui::TextFormat {
            font_id: text(14.0),
            color: k.text2,
            ..Default::default()
        },
    );
    if !small.is_empty() {
        job.append(
            &format!(" – {small}"),
            0.0,
            egui::TextFormat {
                font_id: text(12.0),
                color: k.faint,
                valign: egui::Align::BOTTOM,
                ..Default::default()
            },
        );
    }
    job.wrap.max_width = ui.available_width();
    ui.label(job);
}

/// A hint (.kb-hint: 13 px, faint)
pub fn hint(ui: &mut Ui, t: &str) -> Response {
    ui.add(egui::Label::new(egui::RichText::new(t).font(text(13.0)).color(c().faint)).wrap())
}

/// A tip box (.kb-pmvg-tip): accent line on the left, an accent tint
pub fn tip(ui: &mut Ui, t: &str) {
    let k = c();
    let resp = egui::Frame::new()
        .fill(alpha(k.accent, 0.08))
        .corner_radius(3)
        .inner_margin(Margin {
            left: 13,
            right: 10,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(t).font(text(13.0)).color(k.text2)).wrap());
        });
    let r = resp.response.rect;
    ui.painter()
        .with_clip_rect(Rect::from_min_size(r.min, Vec2::new(3.0, r.height())))
        .rect_filled(r, 3.0, k.accent);
}

/// A group's head (.kb-pmvg-grouphead): hatch mark, title (18 px display), note
pub fn group_head(ui: &mut Ui, title: &str, small: &str) {
    group_head_btn(ui, title, small, None);
}

/// A group's head with a small button on the right (.kb-pmvg-all: 26 px, 12.5 px). → the button was clicked
pub fn group_head_btn(ui: &mut Ui, title: &str, small: &str, button: Option<&str>) -> bool {
    let k = c();
    if ui.cursor().top() > ui.min_rect().top() + 0.5 {
        ui.add_space(8.0);
    }
    // one block of exactly its own height (b: 18px/1; the hatch sits on the baseline, 8 px before the title)
    let mut job = egui::text::LayoutJob::default();
    job.append(
        title,
        0.0,
        egui::TextFormat {
            font_id: display(18.0),
            color: k.text,
            line_height: Some(18.0),
            ..Default::default()
        },
    );
    if !small.is_empty() {
        job.append(
            small,
            10.0,
            egui::TextFormat {
                font_id: text(12.5),
                color: k.faint,
                line_height: Some(18.0),
                valign: egui::Align::BOTTOM,
                ..Default::default()
            },
        );
    }
    let w = ui.available_width();
    let b_gal = button.map(|b| {
        ui.painter()
            .layout_no_wrap(b.to_string(), medium(12.5), Color32::PLACEHOLDER)
    });
    let b_w = b_gal.as_ref().map_or(0.0, |g| g.size().x + 20.0 + 10.0);
    job.wrap.max_width = w - 20.0 - b_w;
    let gal = ui.painter().layout_job(job);
    let h = gal.size().y.max(if button.is_some() { 26.0 } else { 0.0 });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let top = rect.center().y - gal.size().y / 2.0;
    hatch(
        ui.painter(),
        Rect::from_min_size(
            Pos2::new(rect.left(), top + 14.5 - 8.0),
            Vec2::new(12.0, 8.0),
        ),
        k.accent,
    );
    ui.painter()
        .galley(Pos2::new(rect.left() + 20.0, top), gal, k.text);
    let Some(b_gal) = b_gal else { return false };
    let br = Rect::from_min_size(
        Pos2::new(rect.right() - b_w + 10.0, rect.center().y - 13.0),
        Vec2::new(b_w - 10.0, 26.0),
    );
    let resp = ui.interact(br, ui.id().with(("ghb", title)), Sense::click());
    paint_btn(ui, br, &resp, Btn::Normal, None, b_gal, true, 14.0);
    resp.clicked()
}

/// Segmented pills (.kb-seg): the selection glides to the item clicked. Returns true when changed.
pub fn seg<T: PartialEq + Clone>(ui: &mut Ui, value: &mut T, opts: &[(T, &str, &str)]) -> bool {
    seg_impl(ui, value, opts, false)
}

/// A seg stretched to the full width, its buttons on the left (a .kb-seg straight inside a .kb-pmvg-group)
pub fn seg_full<T: PartialEq + Clone>(
    ui: &mut Ui,
    value: &mut T,
    opts: &[(T, &str, &str)],
) -> bool {
    seg_impl(ui, value, opts, true)
}

fn seg_impl<T: PartialEq + Clone>(
    ui: &mut Ui,
    value: &mut T,
    opts: &[(T, &str, &str)],
    full: bool,
) -> bool {
    let k = c();
    let g = glass();
    let mut changed = false;
    let id = ui.next_auto_id().with("seg");
    let frame_slot = ui.painter().add(Shape::Noop);
    let ind_slot = ui.painter().add(Shape::Noop);
    let inner = egui::Frame::new()
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            if full {
                ui.set_min_width(ui.available_width());
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                let mut sel_rect = None;
                for (i, (v, label, tooltip)) in opts.iter().enumerate() {
                    let on = *value == *v;
                    let galley = ui.painter().layout_no_wrap(
                        label.to_string(),
                        medium(13.0),
                        Color32::PLACEHOLDER,
                    );
                    let size = Vec2::new(galley.size().x + 24.0, 30.0);
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                    let h = anim(ui, id.with(("h", i)), resp.hovered() && !on, 0.15);
                    let o = anim(ui, id.with(("o", i)), on, 0.18);
                    let r = if g { 255.0 } else { 2.0 };
                    if h > 0.0 {
                        ui.painter()
                            .rect_filled(rect, cr(r), alpha(k.bg2, h * (1.0 - o)));
                    }
                    if on {
                        sel_rect = Some(rect);
                    }
                    let col = mix(mix(k.text2, k.text, h), k.on_accent, o);
                    ui.painter()
                        .galley(rect.center() - galley.size() / 2.0, galley, col);
                    let resp = if tooltip.is_empty() {
                        resp
                    } else {
                        resp.on_hover_text(*tooltip)
                    };
                    if resp.clicked() && !on {
                        *value = v.clone();
                        changed = true;
                    }
                }
                sel_rect
            })
            .inner
        });
    let outer = inner.response.rect;
    // the container
    let rad = if g { 255.0 } else { 4.0 };
    let mut fs = vec![Shape::Rect(RectShape::filled(
        outer,
        cr(rad),
        if g { alpha(k.bg3, 0.38) } else { k.base },
    ))];
    if g {
        let (_, shine) = gradients(ui.ctx());
        fs.push(textured_rrect(
            outer,
            cr(rad),
            shine,
            FULL_UV,
            Color32::WHITE,
        ));
    }
    fs.push(Shape::Rect(RectShape::stroke(
        outer,
        cr(rad),
        Stroke::new(1.0_f32, if g { glass_edge() } else { k.bg3 }),
        StrokeKind::Inside,
    )));
    ui.painter().set(frame_slot, Shape::Vec(fs));
    if g {
        inset_highlight(ui.painter(), outer, outer.height() / 2.0, 0.16);
    }
    // the selection, gliding
    if let Some(r) = inner.inner {
        // (relative to the container, so scrolling doesn't make it trail behind)
        let x = glide(ui, id.with("x"), r.left() - outer.left(), 0.2);
        let y = glide(ui, id.with("y"), r.top() - outer.top(), 0.2);
        let w = glide(ui, id.with("w"), r.width(), 0.2);
        let rr = Rect::from_min_size(outer.min + Vec2::new(x, y), Vec2::new(w, r.height()));
        ui.painter().set(
            ind_slot,
            Shape::Rect(RectShape::filled(
                rr,
                cr(if g { 255.0 } else { 2.0 }),
                k.accent,
            )),
        );
    }
    changed
}

/// The switch (.kb-switch): 38×20, 3 px corners; on: hatched over the deep accent, the knob slides right
pub fn switch_paint(ui: &Ui, id: Id, rect: Rect, on: bool, hovered: bool) {
    let k = c();
    let t = anim(ui, id.with("sw"), on, 0.2);
    let h = anim(ui, id.with("swh"), hovered, 0.15);
    let p = ui.painter();
    p.rect_filled(rect, 3.0, mix(k.base, k.accent_deep, t));
    if t > 0.01 {
        let mut ph = p.clone();
        ph.multiply_opacity(t);
        hatch(&ph, rect.shrink(1.0), k.accent);
    }
    p.rect_stroke(
        rect,
        3.0,
        Stroke::new(
            1.0_f32,
            mix(mix(k.bg3, k.faint, h * (1.0 - t)), k.accent, t),
        ),
        StrokeKind::Inside,
    );
    let knob = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0 + 18.0 * t, rect.top() + 4.0),
        Vec2::splat(12.0),
    );
    p.rect_filled(knob, 2.0, mix(k.faint, k.paper, t));
}

pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(38.0, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    switch_paint(ui, resp.id, rect, *on, resp.hovered());
    resp
}

/// An option tile with a switch (.kb-pmvg-opt) – the whole tile toggles. Returns true when changed.
pub fn opt(ui: &mut Ui, on: &mut bool, title: &str, desc: &str) -> bool {
    let k = c();
    let width = ui.available_width();
    let g = glass();
    let pad = Vec2::new(12.0, 8.0);
    let text_w = (width - pad.x * 2.0 - 38.0 - 12.0).max(60.0);
    let t_gal = ui
        .painter()
        .layout(title.to_string(), semibold(14.0), k.text, text_w);
    let d_gal = (!desc.is_empty()).then(|| {
        ui.painter()
            .layout(desc.to_string(), text(12.5), k.faint, text_w)
    });
    let content_h = t_gal.size().y + d_gal.as_ref().map(|g| g.size().y + 2.0).unwrap_or(0.0);
    let natural = (content_h + pad.y * 2.0).max(52.0);
    let h = natural.max(GRID_ROW.with(|g| {
        let (min, nat) = g.get();
        g.set((min, nat.max(natural)));
        min
    }));
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, h), Sense::click());
    let hov = anim(ui, resp.id.with("h"), resp.hovered(), 0.15);
    let t = anim(ui, resp.id.with("on"), *on, 0.2);
    let (radius, fill, edge) = tile_style(hov);
    let p = ui.painter();
    p.rect_filled(rect, cr(radius), fill);
    if t > 0.0 {
        fade_fill(
            p,
            rect,
            radius,
            alpha(k.accent, (if g { 0.16 } else { 0.09 }) * t),
        );
    }
    let edge_on = if g {
        alpha(k.accent, 0.6)
    } else {
        k.accent_deep
    };
    p.rect_stroke(
        rect,
        cr(radius),
        Stroke::new(1.0_f32, mix(edge, edge_on, t)),
        StrokeKind::Inside,
    );
    if g {
        inset_highlight(p, rect, radius, 0.12);
    }
    let mut y = rect.center().y - content_h / 2.0;
    let tx = rect.left() + pad.x;
    let th = t_gal.size().y;
    p.galley(Pos2::new(tx, y), t_gal, k.text);
    y += th + 2.0;
    if let Some(dg) = d_gal {
        p.galley(Pos2::new(tx, y), dg, k.faint);
    }
    let sw = Rect::from_min_size(
        Pos2::new(rect.right() - pad.x - 38.0, rect.center().y - 10.0),
        Vec2::new(38.0, 20.0),
    );
    switch_paint(ui, resp.id, sw, *on, resp.hovered());
    if resp.clicked() {
        *on = !*on;
        return true;
    }
    false
}

/// Option tiles in a responsive grid (repeat(auto-fill, minmax(270px, 1fr)), 6 px gaps); a row's tiles are as
/// tall as its tallest (CSS grid stretch)
pub fn opts_grid(ui: &mut Ui, n: usize, mut each: impl FnMut(&mut Ui, usize)) {
    if n == 0 {
        return;
    }
    let w = ui.available_width();
    // (auto-fill keeps empty tracks: a lone tile is still one column wide)
    let cols = ((w + 6.0) / (270.0 + 6.0)).floor().max(1.0) as usize;
    let cell = (w - 6.0 * (cols as f32 - 1.0)) / cols as f32;
    let rows = n.div_ceil(cols);
    let gid = ui.next_auto_id().with(("grid", cols));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for r in 0..rows {
            // a row is as tall as its tallest tile's own height (measured last frame)
            let rid = gid.with(r);
            let min = ui.data(|d| d.get_temp::<f32>(rid)).unwrap_or(0.0);
            let outer = GRID_ROW.with(|g| g.replace((min, 0.0)));
            ui.horizontal_top(|ui| {
                for c in 0..cols {
                    let i = r * cols + c;
                    if i >= n {
                        break;
                    }
                    ui.allocate_ui_with_layout(
                        Vec2::new(cell, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(cell);
                            each(ui, i)
                        },
                    );
                }
            });
            let (_, nat) = GRID_ROW.with(|g| g.replace(outer));
            if (nat - min).abs() > 0.5 {
                ui.data_mut(|d| d.insert_temp(rid, nat));
                ui.ctx().request_discard("tile row height");
            }
        }
    });
}

thread_local! {
    /// The option-grid row being laid out: (height its tiles stretch to, tallest natural height seen)
    static GRID_ROW: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
}

/// A slider row (.kb-pmvg-range): icon + label (90 px) | slider (8 px track, 18 px thumb) | value (52 px).
/// Returns true when changed.
#[allow(clippy::too_many_arguments)]
pub fn range(
    ui: &mut Ui,
    ic: Icon,
    label: &str,
    value: &mut f64,
    min: f64,
    max: f64,
    step: f64,
    fmt: &dyn Fn(f64) -> String,
) -> bool {
    let k = c();
    let width = ui.available_width();
    let g = glass();
    let (label_w, val_w, gap, padx) = (90.0, 52.0, 12.0, 14.0);
    // (font: 600 14px/1 – lines exactly 14 px apart)
    let mut job = egui::text::LayoutJob::single_section(
        label.to_string(),
        egui::TextFormat {
            font_id: semibold(14.0),
            color: k.text,
            line_height: Some(14.0),
            ..Default::default()
        },
    );
    // (CSS only breaks between words: a long word overflows the column instead of being split)
    let widest = label
        .split_whitespace()
        .map(|w| {
            ui.painter()
                .layout_no_wrap(w.to_string(), semibold(14.0), k.text)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    job.wrap.max_width = (label_w - 25.0_f32).max(widest + 0.5);
    let label_gal = ui.painter().layout_job(job);
    let h = (label_gal.size().y + 18.0).max(52.0);
    let (rect, resp_all) = ui.allocate_exact_size(Vec2::new(width, h), Sense::hover());
    let hov = anim(ui, resp_all.id.with("h"), resp_all.hovered(), 0.15);
    let (radius, fill, edge) = tile_style(hov * 0.0);
    let p = ui.painter().clone();
    p.rect_filled(rect, cr(radius), fill);
    p.rect_stroke(
        rect,
        cr(radius),
        Stroke::new(1.0_f32, edge),
        StrokeKind::Inside,
    );
    if g {
        inset_highlight(&p, rect, radius, 0.12);
    }
    let ir = Rect::from_center_size(
        Pos2::new(rect.left() + padx + 8.5, rect.center().y),
        Vec2::splat(17.0),
    );
    icon(&p, ir, ic, k.accent);
    p.galley(
        Pos2::new(ir.right() + 8.0, rect.center().y - label_gal.size().y / 2.0),
        label_gal,
        k.text,
    );
    let tr = Rect::from_min_max(
        Pos2::new(rect.left() + padx + label_w + gap, rect.center().y - 9.0),
        Pos2::new(rect.right() - padx - val_w - gap, rect.center().y + 9.0),
    );
    let id = ui.id().with(("range", label));
    let resp = ui.interact(tr.expand2(Vec2::new(9.0, 8.0)), id, Sense::click_and_drag());
    let mut changed = false;
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on() {
            // (the thumb's centre runs from 9 px in to 9 px before the end, as in a browser)
            let f =
                ((pos.x - tr.left() - 9.0) / (tr.width() - 18.0).max(1.0)).clamp(0.0, 1.0) as f64;
            let mut v = min + f * (max - min);
            if step > 0.0 {
                v = (v / step).round() * step;
            }
            v = v.clamp(min, max);
            if (v - *value).abs() > 1e-9 {
                *value = v;
                changed = true;
            }
        }
    }
    if resp.hovered() {
        let (l, r) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        let st = if step > 0.0 {
            step
        } else {
            (max - min) / 100.0
        };
        if l || r {
            let v = (*value + if r { st } else { -st }).clamp(min, max);
            if v != *value {
                *value = v;
                changed = true;
            }
        }
    }
    let target = if max > min {
        ((*value - min) / (max - min)).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    // (the thumb follows a drag at once, a click or arrow key glides)
    let f = if resp.dragged() {
        ui.ctx().animate_value_with_time(id.with("f"), target, 0.0)
    } else {
        glide(ui, id.with("f"), target, 0.12)
    };
    let line = Rect::from_center_size(tr.center(), Vec2::new(tr.width(), 8.0));
    p.rect_filled(line, 4.0, k.bg3);
    let knob = Pos2::new(
        line.left() + 9.0 + (line.width() - 18.0) * f,
        line.center().y,
    );
    if f > 0.0 {
        p.with_clip_rect(Rect::from_min_max(line.min, Pos2::new(knob.x, line.max.y)))
            .rect_filled(line, 4.0, k.accent);
    }
    let hk = anim(ui, id.with("hk"), resp.hovered() || resp.dragged(), 0.15);
    if hk > 0.0 {
        p.circle_filled(knob, 9.0 + 5.0 * hk, alpha(k.accent, 0.18 * hk));
    }
    p.circle_filled(knob, 9.0, k.paper);
    p.circle_stroke(knob, 7.5, Stroke::new(3.0_f32, k.accent));
    p.text(
        Pos2::new(rect.right() - padx, rect.center().y),
        Align2::RIGHT_CENTER,
        fmt(*value),
        semibold(14.0),
        k.accent,
    );
    changed
}

/// Two slider rows side by side when there is room (.kb-pmvg-sound)
pub fn pair(ui: &mut Ui, a: impl FnOnce(&mut Ui), b: impl FnOnce(&mut Ui)) {
    let w = ui.available_width();
    if w < 560.0 {
        a(ui);
        b(ui);
    } else {
        ui.columns(2, |c| {
            a(&mut c[0]);
            b(&mut c[1]);
        });
    }
}

/// A chevron (22 px) pointing down, turned by `turn` (0 = down, 1 = up)
fn chevron(painter: &egui::Painter, center: Pos2, size: f32, turn: f32, color: Color32) {
    let s = size / 24.0;
    let ang = std::f32::consts::PI * turn;
    let (sn, cs) = ang.sin_cos();
    let pts: Vec<Pos2> = [(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]
        .iter()
        .map(|&(x, y)| {
            let (dx, dy) = ((x - 12.0) * s, (y - 12.0) * s);
            Pos2::new(center.x + dx * cs - dy * sn, center.y + dx * sn + dy * cs)
        })
        .collect();
    painter.add(Shape::line(pts, Stroke::new(2.2 * s, color)));
}

/// A style section (.kb-pmvg-sec): the header bar (title, summary, subtitle, chevron) opens and closes it –
/// animated; the accent bar on the left follows the rounded corner
pub fn section(
    ui: &mut Ui,
    title: &str,
    summary: &str,
    sub: &str,
    open: &mut bool,
    body: impl FnOnce(&mut Ui),
) {
    let k = c();
    let g = glass();
    let id = ui.make_persistent_id(("sec", title));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, *open);
    if state.is_open() != *open {
        state.set_open(*open);
    }
    let openness = state.openness(ui.ctx());
    let radius = if g { 14.0 } else { 6.0 };
    let slot = ui.painter().add(Shape::Noop);
    let resp = egui::Frame::new().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 60.0), Sense::click());
        let hov = anim(ui, id.with("h"), resp.hovered(), 0.15);
        let p = ui.painter();
        // the header: rounded with the section's top corners (and its bottom ones while closed)
        let bottom = (radius * (1.0 - openness)).round() as u8;
        let hr = CornerRadius {
            nw: radius as u8,
            ne: radius as u8,
            sw: bottom,
            se: bottom,
        };
        let bg = if g {
            alpha(k.bg3, 0.45 + 0.25 * hov)
        } else {
            mix(k.bg3, k.sechead_hover, hov)
        };
        p.rect_filled(rect, hr, bg);
        if g {
            let (_, shine) = gradients(ui.ctx());
            p.add(textured_rrect(rect, hr, shine, FULL_UV, Color32::WHITE));
        }
        // the 4 px bar: the header's own rounded shape, cut to its left 4 px – so it curves with the corners
        let bar = mix(k.faint, k.accent, openness);
        p.with_clip_rect(Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height())))
            .rect_filled(rect, hr, bar);
        p.text(
            Pos2::new(rect.left() + 16.0, rect.top() + 12.0),
            Align2::LEFT_TOP,
            title,
            display(19.0),
            k.text,
        );
        p.text(
            Pos2::new(rect.left() + 16.0, rect.top() + 36.0),
            Align2::LEFT_TOP,
            summary,
            medium(12.5),
            k.accent2,
        );
        let chev_c = Pos2::new(rect.right() - 14.0 - 11.0, rect.center().y);
        if w > 560.0 {
            p.text(
                Pos2::new(chev_c.x - 11.0 - 14.0, rect.center().y),
                Align2::RIGHT_CENTER,
                sub,
                text(13.0),
                k.text2,
            );
        }
        chevron(p, chev_c, 22.0, openness, k.accent);
        if resp.clicked() {
            *open = !*open;
            state.set_open(*open);
        }
        state.store(ui.ctx());
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, *open);
        state.show_body_unindented(ui, |ui| {
            egui::Frame::new()
                // (padding 14 16 16 inside the section's 1 px border)
                .inner_margin(Margin {
                    left: 17,
                    right: 17,
                    top: 14,
                    bottom: 17,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // (the pane's things are in groups: 8 px apart inside one, 10 + 6 between two)
                    ui.spacing_mut().item_spacing.y = 8.0;
                    body(ui)
                });
        });
    });
    let rect = resp.response.rect;
    ui.painter().set(
        slot,
        Shape::Rect(RectShape::filled(
            rect,
            cr(radius),
            if g { alpha(k.base, 0.42 * 0.9) } else { k.base },
        )),
    );
    ui.painter().rect_stroke(
        rect,
        cr(radius),
        Stroke::new(1.0_f32, if g { glass_edge() } else { k.bg3 }),
        StrokeKind::Inside,
    );
    if g {
        inset_highlight(ui.painter(), rect, radius, 0.12);
    }
}

/// Button kinds (.kb-btn, .is-primary, .is-ghost)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Btn {
    Normal,
    Primary,
    Ghost,
}

/// A button with an optional icon. Disabled: the whole button at 45 % (opacity .45).
pub fn btn(ui: &mut Ui, kind: Btn, ic: Option<Icon>, label: &str, enabled: bool) -> Response {
    let font = if kind == Btn::Primary {
        bold(14.0)
    } else {
        medium(14.0)
    };
    let gal = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, Color32::PLACEHOLDER);
    let icon_w = if ic.is_some() {
        if label.is_empty() {
            16.0
        } else {
            24.0
        }
    } else {
        0.0
    };
    let w = gal.size().x + icon_w + 28.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 36.0), sense);
    paint_btn(ui, rect, &resp, kind, ic, gal, enabled, 16.0);
    resp
}

/// A button as wide as the space (the Go card's start button: 52 px, 17 px text, 22 px icon)
pub fn wide_btn(ui: &mut Ui, kind: Btn, ic: Option<Icon>, label: &str, enabled: bool) -> Response {
    let (font, h, isz) = if kind == Btn::Primary {
        (bold(17.0), 52.0, 22.0)
    } else {
        (medium(14.5), 40.0, 16.0)
    };
    let gal = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, Color32::PLACEHOLDER);
    let w = ui.available_width() - 4.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), sense);
    paint_btn(ui, rect, &resp, kind, ic, gal, enabled, isz);
    resp
}

#[allow(clippy::too_many_arguments)]
fn paint_btn(
    ui: &Ui,
    rect: Rect,
    resp: &Response,
    kind: Btn,
    ic: Option<Icon>,
    gal: std::sync::Arc<egui::Galley>,
    enabled: bool,
    icon_size: f32,
) {
    let k = c();
    let g = glass();
    let mut p = ui.painter().clone();
    if !enabled {
        p.multiply_opacity(0.45);
    }
    let hov = anim(ui, resp.id.with("h"), enabled && resp.hovered(), 0.15);
    let press = anim(
        ui,
        resp.id.with("p"),
        enabled && resp.is_pointer_button_down_on(),
        0.08,
    );
    let radius = if g { rect.height() / 2.0 } else { 4.0 };
    // primary: lifts 1 px on hover (its paper shadow grows), back down when pressed
    let lift = if kind == Btn::Primary && !g {
        -hov * (1.0 - press)
    } else {
        0.0
    };
    let r = rect.translate(Vec2::splat(lift));
    match kind {
        Btn::Primary => {
            if g {
                // inset highlight .45 and a soft accent glow 0 6px 20px (40 %)
                p.add(Shape::Rect(
                    RectShape::filled(
                        r.translate(Vec2::new(0.0, 6.0)),
                        cr(radius),
                        alpha(k.accent, 0.4),
                    )
                    .with_blur_width(20.0),
                ));
            } else {
                p.rect_filled(r.translate(Vec2::splat(3.0 + hov)), cr(radius), k.paper);
            }
            p.rect_filled(r, cr(radius), mix(k.accent, k.accent2, hov));
            if g {
                inset_highlight(&p, r, radius, 0.45);
            }
        }
        Btn::Ghost => {
            if hov > 0.0 {
                p.rect_filled(
                    r,
                    cr(radius),
                    alpha(if g { alpha(k.bg3, 0.5) } else { k.bg2 }, hov),
                );
            }
        }
        Btn::Normal => {
            let fill = if g {
                alpha(k.bg3, 0.38 + 0.3 * hov)
            } else {
                mix(k.bg2, k.bg3, hov)
            };
            p.rect_filled(r, cr(radius), fill);
            if g {
                let (_, shine) = gradients(ui.ctx());
                p.add(textured_rrect(
                    r,
                    cr(radius),
                    shine,
                    FULL_UV,
                    Color32::WHITE,
                ));
            }
            let edge = if g {
                mix(glass_edge(), k.faint, hov)
            } else {
                mix(k.bg3, k.faint, hov)
            };
            p.rect_stroke(
                r,
                cr(radius),
                Stroke::new(1.0_f32, edge),
                StrokeKind::Inside,
            );
            if g {
                inset_highlight(&p, r, radius, 0.16);
            }
        }
    }
    let fg = match kind {
        Btn::Primary => k.on_accent,
        Btn::Ghost => mix(k.text2, k.text, hov),
        Btn::Normal => k.text,
    };
    let gap = if gal.size().x > 0.0 { 8.0 } else { 0.0 };
    let icon_w = if ic.is_some() { icon_size + gap } else { 0.0 };
    let total = gal.size().x + icon_w;
    let x0 = r.center().x - total / 2.0;
    if let Some(i) = ic {
        let ir = Rect::from_center_size(
            Pos2::new(x0 + icon_size / 2.0, r.center().y),
            Vec2::splat(icon_size),
        );
        icon(&p, ir, i, fg);
    }
    p.galley(
        Pos2::new(x0 + icon_w, r.center().y - gal.size().y / 2.0),
        gal,
        fg,
    );
}

/// A chip (.kb-chip): violet tint with a violet bar on the left (glass: a capsule of tinted glass); on: accent
pub fn chip(ui: &mut Ui, label: &str, on: bool, layout: Option<&str>) -> Response {
    let k = c();
    let g = glass();
    let gal = ui
        .painter()
        .layout_no_wrap(label.to_string(), text(13.0), Color32::PLACEHOLDER);
    let lw = if layout.is_some() { 28.0 } else { 0.0 };
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(gal.size().x + 21.0 + lw, 28.0), Sense::click());
    let hov = anim(ui, resp.id.with("h"), resp.hovered(), 0.15);
    let t = anim(ui, resp.id.with("on"), on, 0.18);
    let p = ui.painter();
    let radius = if g { 14.0 } else { 3.0 };
    let off = if g {
        alpha(k.bg3, 0.38 + 0.3 * hov)
    } else {
        alpha(k.second, 0.16 + 0.12 * hov)
    };
    p.rect_filled(rect, cr(radius), mix(off, k.accent, t));
    if g {
        if t < 1.0 {
            let (_, shine) = gradients(ui.ctx());
            p.add(textured_rrect(
                rect,
                cr(radius),
                shine,
                FULL_UV,
                Color32::WHITE,
            ));
            p.rect_stroke(
                rect,
                cr(radius),
                Stroke::new(1.0_f32, alpha(glass_edge(), 1.0 - t)),
                StrokeKind::Inside,
            );
            inset_highlight(p, rect, radius, 0.16 * (1.0 - t));
        }
    } else if t < 1.0 {
        p.with_clip_rect(Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())))
            .rect_filled(rect, 3.0, alpha(k.second, 1.0 - t));
    }
    let fg = mix(k.text, k.on_accent, t);
    let mut x = rect.left() + 11.0;
    if let Some(id) = layout {
        layout_icon(
            p,
            Rect::from_min_size(Pos2::new(x, rect.center().y - 7.0), Vec2::new(22.0, 14.0)),
            id,
            alpha(fg, 0.9),
        );
        x += lw;
    }
    p.galley(Pos2::new(x, rect.center().y - gal.size().y / 2.0), gal, fg);
    resp
}

/// A text field (.kb-field): 36 px, the accent ring when focused
pub fn field(ui: &mut Ui, value: &mut String, placeholder: &str, width: f32) -> Response {
    let k = c();
    let width = width.min(ui.available_width()).max(60.0);
    let g = glass();
    let slot = ui.painter().add(Shape::Noop);
    let r = egui::Frame::new()
        .inner_margin(Margin::symmetric(11, 9))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(egui::RichText::new(placeholder).color(k.faint))
                    .font(text(14.0))
                    .text_color(k.text)
                    .frame(false)
                    .desired_width(width - 22.0),
            )
        });
    let rect = r.response.rect;
    let te = r.inner;
    let focus = anim(ui, te.id.with("f"), te.has_focus(), 0.15);
    let hov = anim(ui, te.id.with("h"), te.hovered(), 0.15);
    let radius = if g { 10.0 } else { 4.0 };
    let edge = mix(
        mix(if g { glass_edge() } else { k.bg3 }, k.faint, hov),
        k.accent,
        focus,
    );
    let mut shapes = vec![];
    if focus > 0.0 {
        shapes.push(Shape::Rect(RectShape::filled(
            rect.expand(3.0),
            cr(radius + 3.0),
            alpha(k.accent, 0.18 * focus),
        )));
    }
    shapes.push(Shape::Rect(RectShape::filled(
        rect,
        cr(radius),
        if g { alpha(k.base, 0.45) } else { k.base },
    )));
    shapes.push(Shape::Rect(RectShape::stroke(
        rect,
        cr(radius),
        Stroke::new(1.0_f32, edge),
        StrokeKind::Inside,
    )));
    ui.painter().set(slot, Shape::Vec(shapes));
    te
}

/// A drop zone (.kb-pmvg-drop): dashed border, icon, title and note (without glass: on halftone dots).
/// `over`: files are dragged over it; busy: it pulses. At least 150 px tall (.kb-pmvg-drop's min-height).
pub fn drop_zone(
    ui: &mut Ui,
    ic: Icon,
    title: &str,
    small: &str,
    over: bool,
    busy: bool,
) -> Response {
    drop_zone_link(ui, 150.0, ic, title, (small, "", ""), over, busy).0
}

/// A drop zone at least `min_h` tall whose note has a link in it (pre · link · post).
/// → (the zone, the link was clicked)
#[allow(clippy::too_many_arguments)]
pub fn drop_zone_link(
    ui: &mut Ui,
    min_h: f32,
    ic: Icon,
    title: &str,
    (pre, link, post): (&str, &str, &str),
    over: bool,
    busy: bool,
) -> (Response, bool) {
    let k = c();
    let w = ui.available_width();
    // (<small>: 12.5 px)
    let fs = 12.5;
    let small_gal = if link.is_empty() {
        ui.painter()
            .layout(pre.to_string(), text(fs), k.faint, w - 40.0)
    } else {
        let mut job = egui::text::LayoutJob::default();
        let f = |c: Color32, u: bool| egui::TextFormat {
            font_id: text(fs),
            color: c,
            underline: if u {
                Stroke::new(1.0_f32, c)
            } else {
                Stroke::NONE
            },
            ..Default::default()
        };
        job.append(pre, 0.0, f(k.faint, false));
        job.append(link, 0.0, f(k.accent, true));
        job.append(post, 0.0, f(k.faint, false));
        job.wrap.max_width = w - 40.0;
        ui.painter().layout_job(job)
    };
    // where the link is: the width of the text before it, on one line
    let link_span = (!link.is_empty()).then(|| {
        let a = ui
            .painter()
            .layout_no_wrap(pre.to_string(), text(fs), k.faint)
            .size()
            .x;
        let b = ui
            .painter()
            .layout_no_wrap(link.to_string(), text(fs), k.faint)
            .size()
            .x;
        (a, b)
    });
    // (padding 20 + icon 34 + gaps 6 + title 24 + note + border 2: 132 px with a one-line note)
    let h = min_h.max(96.0 + small_gal.size().y);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let hov = anim(ui, resp.id.with("h"), over || resp.hovered() || busy, 0.15);
    let p = ui.painter();
    let g = glass();
    let radius = if g { 14.0 } else { 6.0 };
    p.rect_filled(
        rect,
        cr(radius),
        if g { alpha(k.base, 0.42 * 0.9) } else { k.base },
    );
    if !g {
        let pc = p.with_clip_rect(rect.shrink(2.0));
        let col = alpha(k.accent, 0.22);
        let mut y = rect.top() + 3.5;
        while y < rect.bottom() {
            let mut x = rect.left() + 3.5;
            while x < rect.right() {
                pc.circle_filled(Pos2::new(x, y), 1.1, col);
                x += 7.0;
            }
            y += 7.0;
        }
    }
    let border = mix(
        if g {
            mix(glass_edge(), k.bg3, 0.5)
        } else {
            k.bg3
        },
        k.accent,
        hov,
    );
    if busy {
        p.rect_stroke(
            rect,
            cr(radius),
            Stroke::new(2.0_f32, border),
            StrokeKind::Inside,
        );
    } else {
        // a dashed rounded border (dashes along the straight edges and the corner arcs)
        let r = rect.shrink(1.0);
        let rad = (radius - 1.0).max(0.0);
        let mut pts = Vec::new();
        let corner = |cx: f32, cy: f32, a0: f32, pts: &mut Vec<Pos2>| {
            for i in 0..=6 {
                let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / 6.0;
                pts.push(Pos2::new(cx + rad * a.cos(), cy + rad * a.sin()));
            }
        };
        use std::f32::consts::PI;
        corner(r.right() - rad, r.top() + rad, -PI / 2.0, &mut pts);
        corner(r.right() - rad, r.bottom() - rad, 0.0, &mut pts);
        corner(r.left() + rad, r.bottom() - rad, PI / 2.0, &mut pts);
        corner(r.left() + rad, r.top() + rad, PI, &mut pts);
        pts.push(pts[0]);
        p.extend(Shape::dashed_line(
            &pts,
            Stroke::new(2.0_f32, border),
            6.0,
            4.0,
        ));
    }
    if g {
        inset_highlight(p, rect, radius, 0.12);
    }
    let t = ui.input(|i| i.time) as f32;
    let pulse = if busy {
        0.75 + 0.25 * (t * 6.0).sin()
    } else {
        1.0
    };
    let block = 34.0 + 6.0 + 26.0 + 6.0 + small_gal.size().y;
    let top = rect.center().y - block / 2.0 - 2.0;
    icon(
        p,
        Rect::from_center_size(Pos2::new(rect.center().x, top + 17.0), Vec2::splat(34.0)),
        ic,
        alpha(k.accent, pulse),
    );
    p.text(
        Pos2::new(rect.center().x, top + 40.0 + 13.0),
        Align2::CENTER_CENTER,
        title,
        display(22.0),
        k.text,
    );
    let gx = rect.center().x - small_gal.size().x / 2.0;
    let gh = small_gal.size().y;
    let one_line = small_gal.rows.len() == 1;
    p.galley(Pos2::new(gx, top + 72.0), small_gal, k.faint);
    if busy {
        ui.ctx().request_repaint();
    }
    let mut clicked = false;
    if let (Some((a, b)), true) = (link_span, one_line) {
        let lr = Rect::from_min_size(Pos2::new(gx + a, top + 72.0), Vec2::new(b, gh));
        let lresp = ui
            .interact(lr, resp.id.with("link"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        clicked = lresp.clicked();
    }
    (
        resp.on_hover_cursor(egui::CursorIcon::PointingHand),
        clicked,
    )
}

/// A small "remove" button (an ×, drawn – fonts don't all have the glyph)
pub fn x_btn(ui: &mut Ui, tip: &str) -> Response {
    let k = c();
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    let h = anim(ui, resp.id.with("h"), resp.hovered(), 0.12);
    if h > 0.0 {
        ui.painter().rect_filled(r, 4.0, alpha(k.bg3, h));
    }
    icon(
        ui.painter(),
        r.shrink(4.0),
        Icon::Close,
        mix(k.faint, k.text, h),
    );
    resp.on_hover_text(tip)
}

/// A select (ComboBox) in the field style
pub fn select<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    value: &mut T,
    opts: &[(T, String)],
    width: f32,
) -> bool {
    let k = c();
    let mut changed = false;
    let cur = opts
        .iter()
        .find(|o| o.0 == *value)
        .map(|o| o.1.clone())
        .unwrap_or_default();
    let g = glass();
    ui.scope(|ui| {
        let v = ui.visuals_mut();
        let rad = CornerRadius::same(if g { 10 } else { 4 });
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = rad;
            w.weak_bg_fill = if g { alpha(k.base, 0.45) } else { k.base };
            w.expansion = 0.0;
        }
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, if g { glass_edge() } else { k.bg3 });
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, k.faint);
        v.widgets.open.bg_stroke = Stroke::new(1.0_f32, k.accent);
        ui.spacing_mut().button_padding = Vec2::new(11.0, 8.0);
        egui::ComboBox::from_id_salt(id)
            .selected_text(egui::RichText::new(cur).font(text(14.0)).color(k.text))
            .width(width)
            .icon(move |ui, rect, _visuals, open, _above| {
                chevron(
                    ui.painter(),
                    rect.center(),
                    18.0,
                    if open { 1.0 } else { 0.0 },
                    c().accent,
                );
            })
            .show_ui(ui, |ui| {
                for (v, l) in opts {
                    if ui
                        .selectable_label(*value == *v, egui::RichText::new(l).font(text(14.0)))
                        .clicked()
                        && *value != *v
                    {
                        *value = v.clone();
                        changed = true;
                    }
                }
            });
    });
    changed
}

/// A progress bar (.kb-job-bar) – the fill glides
pub fn progress_bar(ui: &mut Ui, f: f32) {
    let k = c();
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 6.0), Sense::hover());
    let f = glide(ui, resp.id.with("f"), f.clamp(0.0, 1.0), 0.3);
    let p = ui.painter();
    p.rect_filled(rect, 3.0, k.bg3);
    if f > 0.0 {
        p.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(rect.width() * f, rect.height())),
            3.0,
            k.accent,
        );
    }
}

/// The summary list of the Go card (.kb-pmvg-sum): small caps label (78 px), value
pub fn sum_row(ui: &mut Ui, label: &str, value: &str) {
    let k = c();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &label.to_uppercase(),
            0.0,
            egui::TextFormat {
                font_id: medium(12.0),
                color: k.faint,
                extra_letter_spacing: 0.48,
                line_height: Some(21.6),
                ..Default::default()
            },
        );
        let (r, _) = ui.allocate_exact_size(Vec2::new(78.0, 18.0), Sense::hover());
        let gal = ui.painter().layout_job(job);
        ui.painter().galley(r.left_top(), gal, k.faint);
        ui.add(egui::Label::new(egui::RichText::new(value).font(text(13.5)).color(k.text2)).wrap());
    });
}

/// The extension of a path, lower case
pub fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backdrop_matches_the_page() {
        let (page, back) = background_images(320, 200, true);
        for (x, y) in [(10usize, 10usize), (160, 100), (300, 20), (50, 180)] {
            let a = page.pixels[y * 320 + x];
            let b = back.pixels[y * 320 + x];
            eprintln!("{x},{y}: page {:?} back {:?}", a.to_array(), b.to_array());
            for (p, q) in a.to_array().iter().zip(b.to_array()) {
                assert!((*p as i32 - q as i32).abs() < 30, "{x},{y}");
            }
        }
    }
}
