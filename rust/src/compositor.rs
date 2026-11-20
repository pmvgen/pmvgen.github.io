// Layouts and effects on the picture – a port of pmvfx.js (Compositor). The generator decides WHEN
// something happens; this decides HOW it looks. Drawn on the CPU with tiny-skia, the same for the live
// show and for the exported video.

use crate::media::{fit_for, SNAP};
use crate::plan::{Slot, SCROLL};
use crate::raster;
use crate::settings::Settings;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tiny_skia::{
    BlendMode, Color, FillRule, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke,
    Transform,
};

pub const INTRO: f64 = 3.2;
pub const OUTRO: f64 = 4.5;
const PINK: [u8; 3] = [255, 62, 138];

/// PMVGEN_PROF=2: how long each stage of a frame takes (printed every 30 frames)
fn prof_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("PMVGEN_PROF")
            .map(|v| v == "2")
            .unwrap_or(false)
    })
}
struct Prof {
    t: std::time::Instant,
    parts: Vec<(&'static str, f64)>,
}
impl Prof {
    fn new() -> Option<Prof> {
        prof_on().then(|| Prof {
            t: std::time::Instant::now(),
            parts: Vec::new(),
        })
    }
    fn mark(p: &mut Option<Prof>, name: &'static str) {
        if let Some(p) = p {
            p.parts.push((name, p.t.elapsed().as_secs_f64() * 1000.0));
            p.t = std::time::Instant::now();
        }
    }
}

// ---------- Color filters (CSS filter functions as one affine color matrix) ----------

#[derive(Clone, Copy, Debug)]
pub struct Cm([f32; 12]); // rows r, g, b: [m0 m1 m2 offset]
impl Cm {
    pub const ID: Cm = Cm([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    fn lin(m: [f32; 9]) -> Cm {
        Cm([
            m[0], m[1], m[2], 0.0, m[3], m[4], m[5], 0.0, m[6], m[7], m[8], 0.0,
        ])
    }
    /// self then o (o ∘ self)
    pub fn then(self, o: Cm) -> Cm {
        let a = self.0;
        let b = o.0;
        let mut r = [0f32; 12];
        for row in 0..3 {
            for col in 0..3 {
                r[row * 4 + col] = (0..3).map(|k| b[row * 4 + k] * a[k * 4 + col]).sum();
            }
            r[row * 4 + 3] =
                (0..3).map(|k| b[row * 4 + k] * a[k * 4 + 3]).sum::<f32>() + b[row * 4 + 3];
        }
        Cm(r)
    }
    pub fn is_id(&self) -> bool {
        self.0
            .iter()
            .zip(Cm::ID.0.iter())
            .all(|(a, b)| (a - b).abs() < 1e-4)
    }
    pub fn grayscale(a: f32) -> Cm {
        let a = 1.0 - a.clamp(0.0, 1.0);
        Cm::lin([
            0.2126 + 0.7874 * a,
            0.7152 - 0.7152 * a,
            0.0722 - 0.0722 * a,
            0.2126 - 0.2126 * a,
            0.7152 + 0.2848 * a,
            0.0722 - 0.0722 * a,
            0.2126 - 0.2126 * a,
            0.7152 - 0.7152 * a,
            0.0722 + 0.9278 * a,
        ])
    }
    pub fn sepia(a: f32) -> Cm {
        let a = 1.0 - a.clamp(0.0, 1.0);
        Cm::lin([
            0.393 + 0.607 * a,
            0.769 - 0.769 * a,
            0.189 - 0.189 * a,
            0.349 - 0.349 * a,
            0.686 + 0.314 * a,
            0.168 - 0.168 * a,
            0.272 - 0.272 * a,
            0.534 - 0.534 * a,
            0.131 + 0.869 * a,
        ])
    }
    pub fn saturate(s: f32) -> Cm {
        Cm::lin([
            0.213 + 0.787 * s,
            0.715 - 0.715 * s,
            0.072 - 0.072 * s,
            0.213 - 0.213 * s,
            0.715 + 0.285 * s,
            0.072 - 0.072 * s,
            0.213 - 0.213 * s,
            0.715 - 0.715 * s,
            0.072 + 0.928 * s,
        ])
    }
    pub fn hue_rotate(deg: f32) -> Cm {
        let (s, c) = deg.to_radians().sin_cos();
        Cm::lin([
            0.213 + c * 0.787 - s * 0.213,
            0.715 - c * 0.715 - s * 0.715,
            0.072 - c * 0.072 + s * 0.928,
            0.213 - c * 0.213 + s * 0.143,
            0.715 + c * 0.285 + s * 0.140,
            0.072 - c * 0.072 - s * 0.283,
            0.213 - c * 0.213 - s * 0.787,
            0.715 - c * 0.715 + s * 0.715,
            0.072 + c * 0.928 + s * 0.072,
        ])
    }
    pub fn brightness(b: f32) -> Cm {
        Cm::lin([b, 0.0, 0.0, 0.0, b, 0.0, 0.0, 0.0, b])
    }
    pub fn contrast(c: f32) -> Cm {
        let o = 0.5 - 0.5 * c;
        Cm([c, 0.0, 0.0, o, 0.0, c, 0.0, o, 0.0, 0.0, c, o])
    }
    pub fn fixed(&self) -> raster::CmFixed {
        let mut m = [0i32; 12];
        for (i, v) in self.0.iter().enumerate() {
            m[i] = if i % 4 == 3 {
                (v * 255.0 * 1024.0) as i32
            } else {
                (v * 1024.0) as i32
            };
        }
        m
    }
    #[allow(dead_code)]
    fn apply(&self, src: &Pixmap) -> Pixmap {
        let mut out = src.clone();
        // fixed point: matrix × 1024, offset in 0–255 × 1024
        let m: Vec<i32> = self
            .0
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if i % 4 == 3 {
                    (v * 255.0 * 1024.0) as i32
                } else {
                    (v * 1024.0) as i32
                }
            })
            .collect();
        let row = out.width() as usize * 4;
        raster::par_rows(out.data_mut(), row, |_, data| {
            for px in data.as_chunks_mut::<4>().0 {
                let (r, g, b, a) = (px[0] as i32, px[1] as i32, px[2] as i32, px[3] as i32);
                let f = |i: usize| {
                    ((m[i] * r + m[i + 1] * g + m[i + 2] * b + (m[i + 3] * a) / 255) >> 10)
                        .clamp(0, a)
                };
                let (nr, ng, nb) = (f(0), f(4), f(8));
                px[0] = nr as u8;
                px[1] = ng as u8;
                px[2] = nb as u8;
            }
        });
        out
    }
}

/// The look's filter for each clip at strength a (0–1), its tint over everything, vignette
fn look_of(id: &str, a: f32, custom: [u8; 3]) -> (Cm, Option<[f32; 4]>, bool) {
    match id {
        "warm" => (
            Cm::sepia(0.22 * a)
                .then(Cm::saturate(1.0 + 0.2 * a))
                .then(Cm::contrast(1.0 + 0.04 * a)),
            Some([255.0, 140.0, 60.0, 0.22]),
            false,
        ),
        "pink" => (
            Cm::saturate(1.0 + 0.15 * a).then(Cm::contrast(1.0 + 0.05 * a)),
            Some([255.0, 62.0, 138.0, 0.3]),
            false,
        ),
        "cold" => (
            Cm::saturate(1.0 - 0.15 * a).then(Cm::contrast(1.0 + 0.06 * a)),
            Some([60.0, 140.0, 255.0, 0.26]),
            false,
        ),
        "vivid" => (
            Cm::saturate(1.0 + 0.7 * a).then(Cm::contrast(1.0 + 0.12 * a)),
            None,
            false,
        ),
        "bw" => (
            Cm::grayscale(a).then(Cm::contrast(1.0 + 0.15 * a)),
            None,
            false,
        ),
        "noir" => (
            Cm::grayscale(a)
                .then(Cm::contrast(1.0 + 0.5 * a))
                .then(Cm::brightness(1.0 - 0.12 * a)),
            None,
            true,
        ),
        "custom" => (
            Cm::ID,
            Some([custom[0] as f32, custom[1] as f32, custom[2] as f32, 0.34]),
            false,
        ),
        _ => (Cm::ID, None, false),
    }
}
pub fn hex_rgb(h: &str) -> [u8; 3] {
    let s = h.trim().trim_start_matches('#');
    let n = if s.len() == 6 {
        u32::from_str_radix(s, 16).unwrap_or(0xff4d94)
    } else {
        0xff4d94
    };
    [(n >> 16) as u8, (n >> 8) as u8, n as u8]
}

// ---------- What one frame shows ----------

#[derive(Clone)]
pub struct Tile<'a> {
    pub pix: &'a Pixmap,
    pub w: f64, // natural size of the clip
    pub h: f64,
    pub image: bool,
    pub since: f64,
    pub focus: Option<(f64, f64)>,
    pub lum: Option<f64>,
    pub kb: f64,
    pub zoom_dir: i32,
}

pub struct Leave<'a> {
    pub tile: Tile<'a>,
    pub t: f64,
    pub dir: i32,
}

#[derive(Default, Clone)]
pub struct Triggers {
    pub flash: Option<(f64, f64, [u8; 3])>,
    pub shake_until: f64,
    pub glitch_until: f64,
    pub rgb: Option<(f64, f64, f64)>, // t, amount, dur
    pub invert_until: f64,
    pub tunnel: Option<(f64, f64)>,
    pub strobe: Option<f64>,
    pub text: Option<(f64, String)>,
}

pub struct FrameIn<'a> {
    pub t: f64,
    pub slots: &'a [Slot],
    pub groups: Vec<Option<Tile<'a>>>,
    pub leave: Vec<Option<Leave<'a>>>,
    pub energy: f64,
    pub beat_t: f64,
    pub beat_amt: f64,
    pub next_t: Option<f64>,
    pub next_amt: f64,
    pub cut_count: usize,
    pub reveal: Option<f64>,
    pub trig: Triggers,
    pub pic_flash: Option<(&'a Pixmap, f64, f64, f64, f64)>, // pix, natural w, h, t0, t1
    pub pic_over: Option<(&'a Pixmap, f64, f64, f64, f64, f64)>, // + alpha
    pub song_start: f64,
    pub duration: f64,
    pub title: String,
    pub credits: String,
}

pub struct Compositor {
    pub w: u32,
    pub h: u32,
    k: f32, // pixel scale against the web version's picture (1 at the chosen quality)
    s: Settings,
    pub canvas: Pixmap,
    prev: Option<Pixmap>,
    grain: Pixmap,
    rng: StdRng,
    look: Cm,
    tint: Option<[f32; 4]>,
    vignette_look: bool,
    bright: f32,
    pulse_amt: f64,
    rv_asp: Option<f64>,
    prof_n: usize,
    maps: std::collections::HashMap<(&'static str, u32), Vec<u8>>,
    prof_acc: std::collections::HashMap<&'static str, f64>,
    font: crate::text::Font,
    /// the plan's seed: every frame's chance (shake, glitch stripes, VHS noise) comes from it and the frame's
    /// time, so a project draws the same pictures every time – also when it starts in the middle
    seed: u64,
}

fn color(rgb: [u8; 3], a: f32) -> Color {
    Color::from_rgba8(
        rgb[0],
        rgb[1],
        rgb[2],
        (a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}
fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Option<Rect> {
    Rect::from_xywh(x, y, w.max(0.0), h.max(0.0)).filter(|r| r.width() > 0.0 && r.height() > 0.0)
}
fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.max(0.0).min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Box blur (separable, passes ≈ gaussian) of an RGBA pixmap
fn box_blur(p: &mut Pixmap, radius: usize, passes: usize) {
    if radius == 0 {
        return;
    }
    let (w, h) = (p.width() as usize, p.height() as usize);
    let mut tmp = vec![0u8; w * h * 4];
    for _ in 0..passes {
        for horizontal in [true, false] {
            let src = p.data().to_vec();
            let (n, lines) = if horizontal { (w, h) } else { (h, w) };
            for line in 0..lines {
                let idx = |i: usize| {
                    if horizontal {
                        (line * w + i) * 4
                    } else {
                        (i * w + line) * 4
                    }
                };
                let mut acc = [0u32; 4];
                let r = radius as isize;
                let cnt = (2 * radius + 1) as u32;
                for i in -r..=r {
                    let j = i.clamp(0, n as isize - 1) as usize;
                    for c in 0..4 {
                        acc[c] += src[idx(j) + c] as u32;
                    }
                }
                for i in 0..n {
                    let o = idx(i);
                    for c in 0..4 {
                        tmp[o + c] = (acc[c] / cnt) as u8;
                    }
                    let add = (i as isize + r + 1).clamp(0, n as isize - 1) as usize;
                    let sub = (i as isize - r).clamp(0, n as isize - 1) as usize;
                    for c in 0..4 {
                        acc[c] = acc[c] + src[idx(add) + c] as u32 - src[idx(sub) + c] as u32;
                    }
                }
            }
            p.data_mut().copy_from_slice(&tmp);
        }
    }
}

/// Area-average shrink to (dw, dh)
fn shrink(src: &Pixmap, dw: u32, dh: u32) -> Pixmap {
    let mut out = Pixmap::new(dw.max(1), dh.max(1)).unwrap();
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let (dw, dh) = (out.width() as usize, out.height() as usize);
    let s = src.data();
    let o = out.data_mut();
    for y in 0..dh {
        let y0 = y * sh / dh;
        let y1 = ((y + 1) * sh / dh).max(y0 + 1).min(sh);
        for x in 0..dw {
            let x0 = x * sw / dw;
            let x1 = ((x + 1) * sw / dw).max(x0 + 1).min(sw);
            let mut acc = [0u32; 4];
            // (big blocks: every other pixel is plenty)
            let step = if (x1 - x0) * (y1 - y0) > 64 { 2 } else { 1 };
            let mut n = 0;
            let mut yy = y0;
            while yy < y1 {
                let mut xx = x0;
                while xx < x1 {
                    let i = (yy * sw + xx) * 4;
                    for c in 0..4 {
                        acc[c] += s[i + c] as u32;
                    }
                    n += 1;
                    xx += step;
                }
                yy += step;
            }
            let i = (y * dw + x) * 4;
            for c in 0..4 {
                o[i + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    out
}

fn sample_bilinear(s: &[u8], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let mut out = [0f32; 4];
    for (c, o) in out.iter_mut().enumerate() {
        let a = s[(y0 * w + x0) * 4 + c] as f32;
        let b = s[(y0 * w + x1) * 4 + c] as f32;
        let cc = s[(y1 * w + x0) * 4 + c] as f32;
        let d = s[(y1 * w + x1) * 4 + c] as f32;
        *o = (a + (b - a) * fx) * (1.0 - fy) + (cc + (d - cc) * fx) * fy;
    }
    out
}

#[derive(Clone, Copy)]
struct Ctx {
    smooth: bool,
    smart_crop: bool,
    k: f32,
}

/// Draw a clip into its field (rect fx0, fy0, sw, sh of dst): fill or fit, mirroring, zoom, offset, smart crop, image drift
#[allow(clippy::too_many_arguments)]
fn draw_in(
    cx: Ctx,
    dst: &mut Pixmap,
    org: (i32, i32),
    src: &Pixmap,
    cm: Option<&raster::CmFixed>,
    m: &Tile,
    sw: f32,
    sh: f32,
    fx: bool,
    fy: bool,
    fit: &str,
    zoom: f64,
    ox: f64,
    oy: f64,
    drift: bool,
    oy_extra: f32,
    alpha: f32,
) {
    let (mw, mh) = (m.w.max(1.0), m.h.max(1.0));
    let r = if fit == "contain" {
        (sw as f64 / mw).min(sh as f64 / mh)
    } else {
        (sw as f64 / mw).max(sh as f64 / mh)
    };
    let w = mw * r * zoom;
    let h = mh * r * zoom;
    let mut sx = 0.0;
    let mut sy = 0.0;
    if let Some(f) = m.focus {
        if fit != "contain" && cx.smart_crop {
            let rx = ((w - sw as f64) / 2.0).max(0.0);
            let ry = ((h - sh as f64) / 2.0).max(0.0);
            sx = ((0.5 - f.0) * w).clamp(-rx, rx);
            sy = ((0.5 - f.1) * h).clamp(-ry, ry);
        }
    }
    let mut kbx = 0.0;
    if m.image && m.kb != 0.0 && drift {
        kbx = m.kb * 18.0 * cx.k as f64 * (m.since / 4.0).min(1.0);
    }
    let t = Transform::from_translate(
        org.0 as f32 + sw / 2.0 + ox as f32,
        org.1 as f32 + sh / 2.0 + oy as f32 + oy_extra,
    )
    .pre_scale(if fx { -1.0 } else { 1.0 }, if fy { -1.0 } else { 1.0 })
    .pre_translate((kbx + sx - w / 2.0) as f32, (sy - h / 2.0) as f32)
    .pre_scale(
        (w / src.width() as f64) as f32,
        (h / src.height() as f64) as f32,
    );
    let clip = (org.0, org.1, org.0 + sw as i32, org.1 + sh as i32);
    raster::draw_affine_cm(dst, src, t, clip, alpha, cx.smooth, cm);
}

/// Blurred, darkened border behind a fitted clip (tiny copy, blurred, scaled up)
fn backdrop(dst: &mut Pixmap, org: (i32, i32), src: &Pixmap, sw: f32, sh: f32, oy: f32) {
    let k = 16.0;
    let bw = (sw / k).ceil().max(4.0) as u32;
    let bh = (sh / k).ceil().max(4.0) as u32;
    // cover the tiny canvas ×1.15, centred
    let (iw, ih) = (src.width() as f32, src.height() as f32);
    let r = (bw as f32 / iw).max(bh as f32 / ih) * 1.15;
    let (cw, ch) = ((iw * r).ceil() as u32, (ih * r).ceil() as u32);
    let small = shrink(src, cw.max(1), ch.max(1));
    let mut c = Pixmap::new(bw, bh).unwrap();
    let ox = (bw as i32 - cw as i32) / 2;
    let oy0 = (bh as i32 - ch as i32) / 2;
    raster::blit(&mut c, &small, ox, oy0);
    box_blur(&mut c, 1, 2);
    // darkened (black at .55 over it)
    for px in c.data_mut().as_chunks_mut::<4>().0 {
        for v in px.iter_mut().take(3) {
            *v = (*v as u32 * 115 / 255) as u8;
        }
        px[3] = (px[3] as u32 * 115 / 255 + 140).min(255) as u8;
    }
    let t = Transform::from_translate(org.0 as f32, org.1 as f32 + oy)
        .pre_scale(sw / (bw as f32 - 2.0), sh / (bh as f32 - 2.0))
        .pre_translate(-1.0, -1.0);
    let clip = (org.0, org.1, org.0 + sw as i32, org.1 + sh as i32);
    raster::draw_affine(dst, &c, t, clip, 1.0, true);
}

impl Compositor {
    fn ctx(&self) -> Ctx {
        Ctx {
            smooth: self.s.smooth,
            smart_crop: self.s.smart_crop,
            k: self.k,
        }
    }

    pub fn new(w: u32, h: u32, s: &Settings, seed: u64) -> Compositor {
        let q_short = if s.quality >= 1080.0 { 1080.0 } else { 720.0 };
        let k = (w.min(h) as f32 / q_short).max(0.1);
        let la = (s.look_amt / 100.0).clamp(0.0, 1.0) as f32;
        let (look, tint0, vig) = look_of(&s.look, la, hex_rgb(&s.look_color));
        let tint = tint0.map(|t| [t[0], t[1], t[2], t[3] * la]);
        let mut rng = StdRng::seed_from_u64(seed ^ 0x5eed_9a1e);
        let mut grain = Pixmap::new(256, 256).unwrap();
        for px in grain.data_mut().as_chunks_mut::<4>().0 {
            let v = rng.gen::<u8>();
            px[0] = v;
            px[1] = v;
            px[2] = v;
            px[3] = 255;
        }
        Compositor {
            w,
            h,
            k,
            s: s.clone(),
            canvas: Pixmap::new(w, h).unwrap(),
            prev: None,
            grain,
            rng,
            look,
            tint,
            vignette_look: vig,
            bright: (s.bright / 100.0).clamp(0.0, 1.0) as f32,
            pulse_amt: (s.pulse_amt / 100.0).clamp(0.0, 1.5),
            rv_asp: None,
            prof_n: 0,
            maps: Default::default(),
            prof_acc: Default::default(),
            font: crate::text::Font::from_setting(&s.font),
            seed,
        }
    }

    fn fill(&mut self, c: Color, mode: BlendMode) {
        let mut p = paint(c);
        p.blend_mode = mode;
        let r = Rect::from_xywh(0.0, 0.0, self.w as f32, self.h as f32).unwrap();
        self.canvas.fill_rect(r, &p, Transform::identity(), None);
    }

    /// Dark corners (radial: clear inside 35 % of the short side, `a` black at 75 % of the long side) – the map is cached
    fn vignette(&mut self, a: f32) {
        let key = ("vig", (a * 1000.0) as u32);
        let (w, h) = (self.w as usize, self.h as usize);
        let map = self.maps.entry(key).or_insert_with(|| {
            let r0 = w.min(h) as f32 * 0.35;
            let r1 = w.max(h) as f32 * 0.75;
            let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
            let mut m = vec![0u8; w * h];
            for y in 0..h {
                for x in 0..w {
                    let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                    let al = if d <= r0 {
                        0.0
                    } else {
                        ((d - r0) / (r1 - r0)).min(1.0) * a
                    };
                    m[y * w + x] = (al * 255.0).round() as u8;
                }
            }
            m
        });
        let map = &*map;
        raster::par_rows(self.canvas.data_mut(), w * 4, |first, rows| {
            for (ry, row) in rows.chunks_exact_mut(w * 4).enumerate() {
                let mrow = &map[(first + ry) * w..(first + ry + 1) * w];
                for (px, &al) in row.as_chunks_mut::<4>().0.iter_mut().zip(mrow) {
                    if al > 0 {
                        let keep = 255 - al as u32;
                        px[0] = ((px[0] as u32 * keep + 127) / 255) as u8;
                        px[1] = ((px[1] as u32 * keep + 127) / 255) as u8;
                        px[2] = ((px[2] as u32 * keep + 127) / 255) as u8;
                    }
                }
            }
        });
    }

    pub fn draw(&mut self, st: &FrameIn) -> &Pixmap {
        let (wf, hf) = (self.w as f32, self.h as f32);
        let t = st.t;
        let e = st.energy;
        let fxs = self.s.fx.clone();
        let k = self.k;
        self.rng = StdRng::seed_from_u64(
            self.seed ^ ((t * 1000.0).round() as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        );
        self.canvas.fill(Color::BLACK);
        let mut prof = Prof::new();

        let mut dx = 0.0;
        let mut dy = 0.0;
        if t < st.trig.shake_until {
            let a = (6.0 + 16.0 * e) * k as f64;
            dx = (self.rng.gen::<f64>() - 0.5) * a;
            dy = (self.rng.gen::<f64>() - 0.5) * a;
        }
        let hue = if fxs.hue {
            Some(
                Cm::hue_rotate(((t * (30.0 + 110.0 * e)) % 360.0).round() as f32)
                    .then(Cm::saturate(1.3)),
            )
        } else {
            None
        };
        // Zoom pulse: eases in just before the beat so the peak lands on it, then fades out softly
        let mut pulse = 0.0;
        if fxs.zoom && self.pulse_amt > 0.0 {
            let after = st.beat_amt * (-(t - st.beat_t).max(0.0) * 5.5).exp();
            let ease = |tt: Option<f64>, amt: f64| {
                let d = tt.map(|x| x - t).unwrap_or(9.0);
                let x = if (0.0..0.11).contains(&d) {
                    1.0 - d / 0.11
                } else {
                    0.0
                };
                amt * x * x * (3.0 - 2.0 * x)
            };
            let before = ease(st.next_t, st.next_amt).max(if st.beat_t > t {
                ease(Some(st.beat_t), st.beat_amt)
            } else {
                0.0
            });
            pulse = after.max(before) * self.pulse_amt;
        }

        Prof::mark(&mut prof, "start");
        // Reveal opening: the clip sits in the middle as a small rounded window and grows until the drop
        let mut slots: Vec<Slot> = st.slots.to_vec();
        let reveal = st.reveal.is_some() && slots.len() == 1;
        let mut reveal_win: Option<(Slot, f32)> = None;
        if let (true, Some(p)) = (reveal, st.reveal) {
            let m0 = st.groups.get(slots[0].g).and_then(|x| x.as_ref());
            let kk = 0.34 + 0.56 * ((1.0 - (std::f64::consts::PI * p).cos()) / 2.0);
            let want = m0
                .map(|m| (m.w / m.h).clamp(0.4, 2.4))
                .unwrap_or(wf as f64 / hf as f64);
            let asp = match self.rv_asp {
                None => want,
                Some(a) => a + (want - a) * 0.2,
            };
            self.rv_asp = Some(asp);
            let mut w = wf as f64 * kk;
            let mut h = hf as f64 * kk;
            if w / h > asp {
                w = h * asp;
            } else {
                h = w / asp;
            }
            let (w, h) = (w.round() as i32, h.round() as i32);
            let win = Slot {
                x: ((wf as i32 - w) / 2),
                y: ((hf as i32 - h) / 2),
                w,
                h,
                g: slots[0].g,
                fx: false,
                fy: false,
            };
            let rad = w.min(h) as f32 * 0.07;
            // soft glow around the window, breathing with the beat
            let beat = (-(t - st.beat_t) * 6.0).exp() as f32;
            let blur = wf.min(hf) * (0.045 + 0.035 * beat);
            let n = 10;
            for i in (1..=n).rev() {
                let g = blur * i as f32 / n as f32;
                let a = 0.6 * (1.0 - i as f32 / (n as f32 + 1.0)).powi(2) * 0.35;
                if let Some(pth) = round_rect(
                    win.x as f32 - g,
                    win.y as f32 - g,
                    win.w as f32 + 2.0 * g,
                    win.h as f32 + 2.0 * g,
                    rad + g,
                ) {
                    self.canvas.fill_path(
                        &pth,
                        &paint(color([255, 196, 224], a)),
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            if let Some(pth) =
                round_rect(win.x as f32, win.y as f32, win.w as f32, win.h as f32, rad)
            {
                self.canvas.fill_path(
                    &pth,
                    &paint(Color::BLACK),
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            slots = vec![win];
            reveal_win = Some((win, rad));
        } else {
            self.rv_asp = None;
        }
        Prof::mark(&mut prof, "reveal");
        let fit_m = if reveal {
            "cover".to_string()
        } else {
            self.s.fit.clone()
        };
        let soft_on = self.s.soft && !reveal && slots.len() > 1;

        // per group: the clip with its filter applied (once, also when it shows in several fields)
        let base_cm = {
            let mut c = Cm::ID;
            if let Some(h) = hue {
                c = c.then(h);
            }
            c = c.then(self.look);
            if self.bright > 0.0 {
                c = c.then(Cm::brightness(1.0 + 0.1 * self.bright));
            }
            c
        };
        let cm_for = |m: &Tile| -> Cm {
            let mut c = base_cm;
            if self.s.look_even {
                if let Some(l) = m.lum {
                    c = c.then(Cm::brightness((118.0 / l.max(20.0)).clamp(0.8, 1.4) as f32));
                }
            }
            c
        };
        let cms: Vec<Option<raster::CmFixed>> = st
            .groups
            .iter()
            .map(|g| {
                g.as_ref()
                    .map(&cm_for)
                    .filter(|c| !c.is_id())
                    .map(|c| c.fixed())
            })
            .collect();

        for s in slots.iter() {
            let Some(m) = st.groups.get(s.g).and_then(|x| x.as_ref()) else {
                continue;
            };
            let src = m.pix;
            let cm = cms[s.g].as_ref();
            let since = m.since;
            let (sw, sh) = (s.w.max(1) as f32, s.h.max(1) as f32);
            let fit_here = if fit_m == "contain" {
                fit_for("contain", sw as f64, sh as f64, m.w, m.h)
            } else {
                "cover"
            };
            let lv = st.leave.get(s.g).and_then(|x| x.as_ref());
            let sp = lv.map(|l| (t - l.t) / SCROLL).unwrap_or(1.0);
            let scrolling = lv.is_some() && (0.0..1.0).contains(&sp);
            let mut zoom = 1.0 + pulse;
            if m.image && fxs.kenburns {
                zoom += 0.07 * (since / 4.0).min(1.0);
            }
            let mut ox = dx;
            let mut oy = dy;
            let mut blur_px = 0.0f32;
            if fxs.whip && since < 0.16 && !scrolling {
                let p = 1.0 - since / 0.16;
                let dir = if (s.g + st.cut_count) % 2 == 1 {
                    1.0
                } else {
                    -1.0
                };
                if sw >= sh * 0.9 {
                    ox += dir * p * p * sw as f64 * 0.6;
                } else {
                    oy += dir * p * p * sh as f64 * 0.6;
                }
                blur_px = (p * 10.0) as f32 * k;
            }
            if fxs.zoomin && since < 0.3 && !scrolling {
                let p = since / 0.3;
                let ease = 1.0 - (1.0 - p).powi(3);
                let from = if m.zoom_dir > 0 { 1.75 } else { 1.35 };
                zoom *= from + (1.0 - from) * ease;
            }
            let cx = self.ctx();
            let direct = !scrolling && blur_px < 1.0 && reveal_win.is_none();
            if direct {
                let org = (s.x, s.y);
                if fit_here == "contain" {
                    backdrop(&mut self.canvas, org, m.pix, sw, sh, 0.0);
                }
                draw_in(
                    cx,
                    &mut self.canvas,
                    org,
                    src,
                    cm,
                    m,
                    sw,
                    sh,
                    s.fx,
                    s.fy,
                    fit_here,
                    zoom,
                    ox,
                    oy,
                    fxs.kenburns,
                    0.0,
                    1.0,
                );
                continue;
            }
            let mut field = Pixmap::new(sw as u32, sh as u32).unwrap();
            if scrolling {
                let l = lv.unwrap();
                let ease = 1.0 - (1.0 - sp).powi(3);
                // the old clip moves out, the new one follows right behind it
                let old_src = &l.tile;
                let off_old = l.dir as f64 * sh as f64 * ease;
                let off_new = l.dir as f64 * sh as f64 * (ease - 1.0);
                let old_fit = if fit_m == "contain" {
                    fit_for("contain", sw as f64, sh as f64, old_src.w, old_src.h)
                } else {
                    "cover"
                };
                if old_fit == "contain" {
                    backdrop(&mut field, (0, 0), old_src.pix, sw, sh, off_old as f32);
                }
                let old_cm = cm_for(old_src);
                let old_fx = if old_cm.is_id() {
                    None
                } else {
                    Some(old_cm.fixed())
                };
                draw_in(
                    cx,
                    &mut field,
                    (0, 0),
                    old_src.pix,
                    old_fx.as_ref(),
                    old_src,
                    sw,
                    sh,
                    s.fx,
                    s.fy,
                    old_fit,
                    1.0,
                    ox,
                    oy,
                    fxs.kenburns,
                    off_old as f32,
                    1.0,
                );
                if fit_here == "contain" {
                    backdrop(&mut field, (0, 0), m.pix, sw, sh, off_new as f32);
                }
                draw_in(
                    cx,
                    &mut field,
                    (0, 0),
                    src,
                    cm,
                    m,
                    sw,
                    sh,
                    s.fx,
                    s.fy,
                    fit_here,
                    zoom,
                    ox,
                    oy,
                    fxs.kenburns,
                    off_new as f32,
                    1.0,
                );
            } else {
                if fit_here == "contain" {
                    backdrop(&mut field, (0, 0), m.pix, sw, sh, 0.0);
                }
                draw_in(
                    cx,
                    &mut field,
                    (0, 0),
                    src,
                    cm,
                    m,
                    sw,
                    sh,
                    s.fx,
                    s.fy,
                    fit_here,
                    zoom,
                    ox,
                    oy,
                    fxs.kenburns,
                    0.0,
                    1.0,
                );
                if blur_px >= 1.0 {
                    let r = (blur_px * 0.9).round() as usize;
                    box_blur(&mut field, r.max(1), 2);
                }
            }
            if let Some((win, rad)) = reveal_win {
                let mut mask = Mask::new(self.w, self.h).unwrap();
                if let Some(pth) =
                    round_rect(win.x as f32, win.y as f32, win.w as f32, win.h as f32, rad)
                {
                    mask.fill_path(&pth, FillRule::Winding, true, Transform::identity());
                }
                self.canvas.draw_pixmap(
                    s.x,
                    s.y,
                    field.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    Some(&mask),
                );
            } else {
                raster::blit(&mut self.canvas, &field, s.x, s.y);
            }
        }
        Prof::mark(&mut prof, "fields");
        if soft_on {
            self.soft_seams(&slots);
        }
        if let Some((win, rad)) = reveal_win {
            if let Some(pth) =
                round_rect(win.x as f32, win.y as f32, win.w as f32, win.h as f32, rad)
            {
                let st2 = Stroke {
                    width: 2.0 * k,
                    ..Default::default()
                };
                self.canvas.stroke_path(
                    &pth,
                    &paint(Color::from_rgba(1.0, 1.0, 1.0, 0.2).unwrap()),
                    &st2,
                    Transform::identity(),
                    None,
                );
            }
        }

        Prof::mark(&mut prof, "soft");
        // Pictures: an overlay lies over the clips, see-through; a flash picture covers them for a moment
        if let Some((pix, pw, ph, t0, t1, al)) = st.pic_over {
            if t >= t0 && t < t1 {
                let alpha = al * ((t - t0) / 0.15).min((t1 - t) / 0.15).clamp(0.0, 1.0);
                self.draw_pic(pix, pw, ph, false, 1.0 + pulse * 0.6, dx, dy, alpha as f32);
            }
        }
        let mut pic_on = false;
        if let Some((pix, pw, ph, t0, t1)) = st.pic_flash {
            if t >= t0 && t < t1 {
                pic_on = true;
                self.draw_pic(pix, pw, ph, true, 1.0 + pulse, dx, dy, 1.0);
            }
        }

        Prof::mark(&mut prof, "pictures");
        // Color look: tint over everything, noir with dark corners
        if let Some(tn) = self.tint {
            self.fill(
                Color::from_rgba(
                    tn[0] / 255.0,
                    tn[1] / 255.0,
                    tn[2] / 255.0,
                    tn[3].clamp(0.0, 1.0),
                )
                .unwrap(),
                BlendMode::SoftLight,
            );
        }
        if self.bright > 0.0 {
            self.fill(
                Color::from_rgba(1.0, 1.0, 1.0, (0.4 * self.bright).clamp(0.0, 1.0)).unwrap(),
                BlendMode::SoftLight,
            );
        }
        if self.vignette_look {
            self.vignette(0.6);
        }
        Prof::mark(&mut prof, "look");
        if self.s.edge != "off" {
            let a = (self.s.edge_amt / 100.0).clamp(0.0, 1.0) as f32;
            let kind = self.s.edge.clone();
            self.edge_soft(&kind, a);
        }

        Prof::mark(&mut prof, "edge");
        // Dividers between fields, glowing to the beat
        if st.slots.len() > 1 && !soft_on && !pic_on && !reveal {
            let dw = ((self.s.div_w.max(0.0) * self.w as f64 / 1280.0).round() as f32).max(1.0);
            let h2 = dw / 2.0;
            let glow = (-(t - st.beat_t) * 7.0).exp() as f32;
            let draw_lines = |c: Color, canvas: &mut Pixmap| {
                for s in st.slots {
                    if s.x > 0 {
                        if let Some(r) = rect(s.x as f32 - h2, s.y as f32, dw, s.h as f32) {
                            canvas.fill_rect(r, &paint(c), Transform::identity(), None);
                        }
                    }
                    if s.y > 0 {
                        if let Some(r) = rect(s.x as f32, s.y as f32 - h2, s.w as f32, dw) {
                            canvas.fill_rect(r, &paint(c), Transform::identity(), None);
                        }
                    }
                }
            };
            draw_lines(Color::from_rgba8(10, 3, 9, 255), &mut self.canvas);
            if fxs.lines && glow > 0.05 {
                draw_lines(color(PINK, 0.9 * glow), &mut self.canvas);
            }
        }

        Prof::mark(&mut prof, "dividers");
        // Echo: the last frame lingers as a slightly enlarged ghost
        if fxs.echo && e > 0.45 {
            if let Some(prev) = &self.prev {
                let tr = Transform::from_translate(-wf * 0.02, -hf * 0.02).pre_scale(1.04, 1.04);
                raster::draw_affine(
                    &mut self.canvas,
                    prev,
                    tr,
                    (0, 0, i32::MAX, i32::MAX),
                    (0.16 + 0.2 * e) as f32,
                    true,
                );
            }
        }

        Prof::mark(&mut prof, "echo");
        // Tunnel: the picture sits shrunk inside itself
        if let Some((tt, dur)) = st.trig.tunnel {
            let tp = (t - tt) / dur.max(0.01);
            if (0.0..1.0).contains(&tp) {
                let grow = 1.0 - tp;
                for f in [0.74, 0.52, 0.34] {
                    let sc = (f + (1.0 - f) * 0.12 * tp) as f32;
                    let (w, h) = (wf * sc, hf * sc);
                    let snap = self.canvas.clone();
                    let tr =
                        Transform::from_translate((wf - w) / 2.0, (hf - h) / 2.0).pre_scale(sc, sc);
                    raster::draw_affine(
                        &mut self.canvas,
                        &snap,
                        tr,
                        (0, 0, i32::MAX, i32::MAX),
                        (0.9 * grow + 0.1) as f32,
                        true,
                    );
                    if let Some(r) = rect((wf - w) / 2.0, (hf - h) / 2.0, w, h) {
                        let pth = PathBuilder::from_rect(r);
                        let mut p = paint(color(PINK, (0.9 * grow + 0.1) as f32));
                        p.anti_alias = true;
                        self.canvas.stroke_path(
                            &pth,
                            &p,
                            &Stroke {
                                width: 3.0 * k,
                                ..Default::default()
                            },
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
        }

        Prof::mark(&mut prof, "tunnel");
        // Glitch: stripes slip
        if t < st.trig.glitch_until {
            let n = 5 + self.rng.gen_range(0..6);
            let snap = self.canvas.clone();
            let (w, h) = (self.w as usize, self.h as usize);
            for _ in 0..n {
                let y = (self.rng.gen::<f32>() * hf) as usize;
                let sh = (4.0 * k + self.rng.gen::<f32>() * hf * 0.09) as usize;
                let off = ((self.rng.gen::<f32>() - 0.5) * wf * 0.14) as isize;
                let src = snap.data();
                let dst = self.canvas.data_mut();
                for yy in y..(y + sh).min(h) {
                    for x in 0..w {
                        let sx = x as isize - off;
                        if sx < 0 || sx >= w as isize {
                            continue;
                        }
                        let di = (yy * w + x) * 4;
                        let si = (yy * w + sx as usize) * 4;
                        dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
                    }
                }
            }
            self.fill(color(PINK, 0.16), BlendMode::Screen);
        }

        Prof::mark(&mut prof, "glitch");
        // RGB split: red and cyan drift apart
        if let Some((rt, amt, dur)) = st.trig.rgb {
            let rp = (t - rt) / dur.max(0.01);
            if (0.0..1.0).contains(&rp) {
                let a = ((amt * (1.0 - rp) * (1.0 - rp) + 1.0) * k as f64).round() as isize;
                self.split(a);
            }
        }

        Prof::mark(&mut prof, "rgb");
        // Negative
        if t < st.trig.invert_until {
            for px in self.canvas.data_mut().as_chunks_mut::<4>().0 {
                px[0] = px[3] - px[0];
                px[1] = px[3] - px[1];
                px[2] = px[3] - px[2];
            }
        }

        // Flash
        if let Some((ft, fa0, fc)) = st.trig.flash {
            let fa = fa0 * (-(t - ft) * 9.0).exp();
            if fa > 0.01 && t >= ft {
                self.fill(color(fc, fa as f32), BlendMode::SourceOver);
            }
        }
        // Strobe: short white
        if let Some(stt) = st.trig.strobe {
            if t - stt >= 0.0 && t - stt < 0.045 {
                self.fill(
                    Color::from_rgba(1.0, 1.0, 1.0, 0.92).unwrap(),
                    BlendMode::SourceOver,
                );
            }
        }
        // Text
        if let Some((tt0, word)) = &st.trig.text {
            let tt = t - tt0;
            if (0.0..0.42).contains(&tt) && !word.is_empty() {
                self.draw_word(word, tt, 0.34, false, 1.0);
            }
        }
        Prof::mark(&mut prof, "flash/text");
        // VHS: noise, scanlines, dark corners
        if fxs.vhs {
            // noise at 10 % (a 256² tile at a random offset), every other pair of lines 22 % darker, dark corners
            let ox = self.rng.gen_range(0..256usize);
            let oy = self.rng.gen_range(0..256usize);
            let w = self.w as usize;
            let grain = self.grain.data();
            raster::par_rows(self.canvas.data_mut(), w * 4, |first, rows| {
                for (ry, row) in rows.chunks_exact_mut(w * 4).enumerate() {
                    let y = first + ry;
                    let gy = (y + oy) % 256;
                    let dark = y % 4 < 2;
                    for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        let g = grain[(gy * 256 + (x + ox) % 256) * 4] as u32;
                        for c in px.iter_mut().take(3) {
                            let mut v = (*c as u32 * 230 + g * 25 + 127) / 255;
                            if dark {
                                v = v * 199 / 255;
                            }
                            *c = v as u8;
                        }
                    }
                }
            });
            self.vignette(0.55);
        }

        Prof::mark(&mut prof, "vhs");
        // Intro and outro lie on top of everything
        let rt = t - st.song_start;
        if self.s.intro && (0.0..INTRO).contains(&rt) {
            self.draw_intro(rt, &st.title);
        }
        if self.s.outro && st.duration > 12.0 && rt > st.duration - OUTRO {
            self.draw_outro(rt - (st.duration - OUTRO), &st.title, &st.credits);
        }
        Prof::mark(&mut prof, "intro/outro");
        if let Some(p) = prof {
            self.prof_n += 1;
            for (k, v) in p.parts {
                *self.prof_acc.entry(k).or_insert(0.0) += v;
            }
            if self.prof_n.is_multiple_of(30) {
                let mut v: Vec<_> = self
                    .prof_acc
                    .iter()
                    .map(|(k, x)| (*k, x / self.prof_n as f64))
                    .filter(|x| x.1 > 0.05)
                    .collect();
                v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                eprintln!(
                    "[prof] ms/frame: {}",
                    v.iter()
                        .map(|(k, x)| format!("{k} {x:.1}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        if fxs.echo {
            match &mut self.prev {
                Some(p) if p.width() == self.w && p.height() == self.h => {
                    p.data_mut().copy_from_slice(self.canvas.data())
                }
                _ => self.prev = Some(self.canvas.clone()),
            }
        }
        &self.canvas
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_pic(
        &mut self,
        pix: &Pixmap,
        mw: f64,
        mh: f64,
        backdrop_on: bool,
        zoom: f64,
        ox: f64,
        oy: f64,
        alpha: f32,
    ) {
        if alpha <= 0.0 {
            return;
        }
        let (wf, hf) = (self.w as f32, self.h as f32);
        let r = (wf as f64 / hf as f64 / (mw / mh)).max(mw / mh / (wf as f64 / hf as f64));
        let fit = if r <= SNAP { "cover" } else { "contain" };
        let m = Tile {
            pix,
            w: mw,
            h: mh,
            image: true,
            since: 0.0,
            focus: None,
            lum: None,
            kb: 0.0,
            zoom_dir: 1,
        };
        let cx = self.ctx();
        if alpha >= 0.999 {
            if backdrop_on && fit == "contain" {
                backdrop(&mut self.canvas, (0, 0), pix, wf, hf, 0.0);
            }
            draw_in(
                cx,
                &mut self.canvas,
                (0, 0),
                pix,
                None,
                &m,
                wf,
                hf,
                false,
                false,
                fit,
                zoom,
                ox,
                oy,
                false,
                0.0,
                1.0,
            );
        } else {
            let mut field = Pixmap::new(self.w, self.h).unwrap();
            if backdrop_on && fit == "contain" {
                backdrop(&mut field, (0, 0), pix, wf, hf, 0.0);
            }
            draw_in(
                cx,
                &mut field,
                (0, 0),
                pix,
                None,
                &m,
                wf,
                hf,
                false,
                false,
                fit,
                zoom,
                ox,
                oy,
                false,
                0.0,
                1.0,
            );
            raster::draw_affine(
                &mut self.canvas,
                &field,
                Transform::identity(),
                (0, 0, i32::MAX, i32::MAX),
                alpha.clamp(0.0, 1.0),
                false,
            );
        }
    }

    // Soft seams: a band across each seam is smeared and blended in – strongest right at the seam
    fn soft_seams(&mut self, slots: &[Slot]) {
        let a = (self.s.soft_amt / 100.0).clamp(0.0, 1.0);
        let kk = 0.004 + 0.026 * a;
        let min_w = slots.iter().map(|s| s.w).min().unwrap_or(0) as f64;
        let min_h = slots.iter().map(|s| s.h).min().unwrap_or(0) as f64;
        let fh = (kk * self.w as f64).min(0.08 * min_w).round() as i32;
        let fv = (kk * self.h as f64).min(0.08 * min_h).round() as i32;
        let mut done = std::collections::HashSet::new();
        for s in slots {
            if s.x > 1 && fh >= 2 && done.insert(('v', s.x, s.y)) {
                self.smear(s.x - fh, s.y, 2 * fh, s.h, true);
            }
            if s.y > 1 && fv >= 2 && done.insert(('h', s.y, s.x)) {
                self.smear(s.x, s.y - fv, s.w, 2 * fv, false);
            }
        }
    }
    fn smear(&mut self, x0: i32, y0: i32, rw: i32, rh: i32, axis_x: bool) {
        let (w, h) = (self.w as i32, self.h as i32);
        let along = if axis_x { rw } else { rh };
        let texel = ((along as f64 / 6.0).round() as i32).max(2);
        let cells = ((along as f64 / texel as f64).ceil() as i32).max(2);
        let snap = self.canvas.data().to_vec();
        let data = self.canvas.data_mut();
        let lines = if axis_x { rh } else { rw };
        for li in 0..lines {
            // block averages across the seam
            let mut cell = vec![[0f32; 3]; cells as usize];
            for (ci, c) in cell.iter_mut().enumerate() {
                let a = ci as i32 * along / cells;
                let b = ((ci as i32 + 1) * along / cells).max(a + 1);
                let mut acc = [0f32; 3];
                let mut n = 0.0;
                for j in a..b {
                    let (px, py) = if axis_x {
                        (x0 + j, y0 + li)
                    } else {
                        (x0 + li, y0 + j)
                    };
                    if px < 0 || py < 0 || px >= w || py >= h {
                        continue;
                    }
                    let i = ((py * w + px) * 4) as usize;
                    for (q, v) in acc.iter_mut().enumerate() {
                        *v += snap[i + q] as f32;
                    }
                    n += 1.0;
                }
                if n > 0.0 {
                    *c = [acc[0] / n, acc[1] / n, acc[2] / n];
                }
            }
            for j in 0..along {
                let (px, py) = if axis_x {
                    (x0 + j, y0 + li)
                } else {
                    (x0 + li, y0 + j)
                };
                if px < 0 || py < 0 || px >= w || py >= h {
                    continue;
                }
                let p = (j as f32 + 0.5) / along as f32;
                let v = (std::f32::consts::PI * p).sin();
                let wgt = v * v;
                // smeared value: the blocks scaled back up smoothly
                let u = (p * cells as f32 - 0.5).clamp(0.0, (cells - 1) as f32);
                let c0 = u.floor() as usize;
                let c1 = (c0 + 1).min(cells as usize - 1);
                let f = u - c0 as f32;
                let i = ((py * w + px) * 4) as usize;
                for q in 0..3 {
                    let sm = cell[c0][q] + (cell[c1][q] - cell[c0][q]) * f;
                    data[i + q] = (snap[i + q] as f32 * (1.0 - wgt) + sm * wgt)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    // Rim effect: a blurred copy (the picture shrunk and scaled back up) that only shows towards the edges.
    // blur = soft, motion = streaks sideways (left/right) and up/down (top/bottom), lens = bent outwards
    fn edge_soft(&mut self, kind: &str, a: f32) {
        if a <= 0.0 {
            return;
        }
        let (w, h) = (self.w as usize, self.h as usize);
        let f = 1.0 / (2.0 + 7.0 * a);
        let sw = ((w as f32 * f).round() as u32).max(8);
        let sh = ((h as f32 * f).round() as u32).max(8);
        let small = shrink(&self.canvas, sw, sh);
        let (swu, shu) = (sw as usize, sh as usize);
        // the small picture scaled up to the full size (zoomed about the middle for the lens)
        let full = |src: &Pixmap, zoom: f32| -> Pixmap {
            let mut big = Pixmap::new(w as u32, h as u32).unwrap();
            let t = Transform::from_translate(w as f32 / 2.0, h as f32 / 2.0)
                .pre_scale(zoom, zoom)
                .pre_translate(-(w as f32) / 2.0, -(h as f32) / 2.0)
                .pre_scale(w as f32 / sw as f32, h as f32 / sh as f32);
            raster::draw_affine(&mut big, src, t, (0, 0, w as i32, h as i32), 1.0, true);
            big
        };
        // blend `big` in with a weight per pixel (0–255)
        let blend =
            |canvas: &mut Pixmap, big: &Pixmap, wt: &(dyn Fn(usize, usize) -> u32 + Sync)| {
                let bd = big.data();
                raster::par_rows(canvas.data_mut(), w * 4, |first, rows| {
                    for (ry, row) in rows.chunks_exact_mut(w * 4).enumerate() {
                        let y = first + ry;
                        let brow = &bd[y * w * 4..(y + 1) * w * 4];
                        for x in 0..w {
                            let k = wt(x, y);
                            if k == 0 {
                                continue;
                            }
                            let i = x * 4;
                            for q in 0..3 {
                                row[i + q] =
                                    ((row[i + q] as u32 * (255 - k) + brow[i + q] as u32 * k + 127)
                                        / 255) as u8;
                            }
                        }
                    }
                });
            };
        // 1 at the edges, 0 in the middle stretch (stronger = more of the rim)
        let kreach = 0.5 + 0.5 * a;
        let p1 = (0.5 - 0.5 * (1.0 - kreach) - 0.12).max(0.02);
        let p2 = (0.5 + 0.5 * (1.0 - kreach) + 0.12).min(0.98);
        let lin = |u: f32| -> f32 {
            if u <= p1 {
                1.0 - u / p1
            } else if u >= p2 {
                (u - p2) / (1.0 - p2)
            } else {
                0.0
            }
        };
        if kind == "motion" {
            // 6 copies at 1/3 each cover 1 − (2/3)⁶ of it; the copies' average is done on the small picture
            let cov = 1.0 - (2.0f32 / 3.0).powi(6);
            for horizontal in [true, false] {
                let n = 6;
                let reach = if horizontal {
                    w as f32 * 0.03 * (0.4 + a) * swu as f32 / w as f32
                } else {
                    h as f32 * 0.03 * (0.4 + a) * shu as f32 / h as f32
                };
                let sd = small.data();
                let mut streak = Pixmap::new(sw, sh).unwrap();
                let out = streak.data_mut();
                for y in 0..shu {
                    for x in 0..swu {
                        let mut acc = [0f32; 4];
                        for i in 0..n {
                            let o = (i as f32 / (n - 1) as f32 - 0.5) * 2.0 * reach;
                            let (sx, sy) = if horizontal {
                                (x as f32 - o, y as f32)
                            } else {
                                (x as f32, y as f32 - o)
                            };
                            let v = sample_bilinear(sd, swu, shu, sx, sy);
                            for q in 0..4 {
                                acc[q] += v[q] / n as f32;
                            }
                        }
                        let i = (y * swu + x) * 4;
                        for q in 0..4 {
                            out[i + q] = acc[q].round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
                let big = full(&streak, 1.0);
                let weights: Vec<u32> = if horizontal {
                    (0..w)
                        .map(|x| {
                            ((lin((x as f32 + 0.5) / w as f32) * cov).clamp(0.0, 1.0) * 255.0)
                                .round() as u32
                        })
                        .collect()
                } else {
                    (0..h)
                        .map(|y| {
                            ((lin((y as f32 + 0.5) / h as f32) * cov).clamp(0.0, 1.0) * 255.0)
                                .round() as u32
                        })
                        .collect()
                };
                if horizontal {
                    blend(&mut self.canvas, &big, &|x, _| weights[x]);
                } else {
                    blend(&mut self.canvas, &big, &|_, y| weights[y]);
                }
            }
            return;
        }
        // blur / lens: radial – nothing inside 35 % of the way out, up to .55–.95 at the corners (the map is cached)
        let key = ("edge", (a * 1000.0) as u32);
        let map = self.maps.entry(key).or_insert_with(|| {
            let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
            let r0 = w.min(h) as f32 * 0.3;
            let r1 = ((w * w + h * h) as f32).sqrt() * 0.5;
            let mut m = vec![0u8; w * h];
            for y in 0..h {
                for x in 0..w {
                    let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                    let u = ((d - r0) / (r1 - r0)).clamp(0.0, 1.0);
                    let v = if u <= 0.35 {
                        0.0
                    } else {
                        (u - 0.35) / 0.65 * (0.55 + 0.4 * a)
                    };
                    m[y * w + x] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
            m
        });
        let map = map.clone();
        let big = full(
            &small,
            if kind == "lens" {
                1.0 + 0.05 * a + 0.02
            } else {
                1.0
            },
        );
        blend(&mut self.canvas, &big, &|x, y| map[y * w + x] as u32);
    }

    fn split(&mut self, a: isize) {
        if a == 0 {
            return;
        }
        let (w, h) = (self.w as isize, self.h as usize);
        let _ = h;
        let snap = self.canvas.data().to_vec();
        let rb = w as usize * 4;
        raster::par_rows(self.canvas.data_mut(), rb, |first, rows| {
            for (ry, out) in rows.chunks_exact_mut(rb).enumerate() {
                let row = (first + ry) * rb;
                for x in 0..w {
                    let i = x as usize * 4;
                    let rx = x + a; // red drawn at −a: this pixel shows red from x + a
                    let cx = x - a; // cyan drawn at +a
                    out[i] = if rx >= 0 && rx < w {
                        snap[row + rx as usize * 4]
                    } else {
                        0
                    };
                    if cx >= 0 && cx < w {
                        out[i + 1] = snap[row + cx as usize * 4 + 1];
                        out[i + 2] = snap[row + cx as usize * 4 + 2];
                    } else {
                        out[i + 1] = 0;
                        out[i + 2] = 0;
                    }
                    out[i + 3] = 255;
                }
            }
        });
    }

    fn draw_word(&mut self, word: &str, tt: f64, scale: f32, hold: bool, alpha: f32) {
        let (wf, hf) = (self.w as f32, self.h as f32);
        let txt = word.to_uppercase();
        let mut size = wf.min(hf) * scale;
        let max_w = wf * 0.86;
        let mw = self.font.measure(&txt, size);
        if mw > max_w {
            size *= max_w / mw;
        }
        let punch = 1.0 + 0.35 * (-tt * 18.0).exp() as f32;
        let fade = if hold {
            1.0
        } else if tt > 0.3 {
            1.0 - ((tt - 0.3) / 0.12) as f32
        } else {
            1.0
        };
        let a = (alpha * fade).clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let Some(pth) = self.font.path(&txt, size) else {
            return;
        };
        let base = Transform::from_translate(wf / 2.0, hf / 2.0)
            .pre_concat(Transform::from_rotate(-0.1f32.to_degrees()))
            .pre_concat(Transform::from_row(1.0, 0.0, -0.18, 1.0, 0.0, 0.0))
            .pre_scale(punch, punch);
        self.canvas.fill_path(
            &pth,
            &paint(color(PINK, a)),
            FillRule::Winding,
            base.pre_translate(size * 0.05, size * 0.05),
            None,
        );
        let stroke = Stroke {
            width: size * 0.1,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };
        self.canvas
            .stroke_path(&pth, &paint(color([35, 15, 31], a)), &stroke, base, None);
        self.canvas.fill_path(
            &pth,
            &paint(color([251, 239, 244], a)),
            FillRule::Winding,
            base,
            None,
        );
    }

    // Intro: the title slams in on a pink hatched bar, the picture below darkened; fades out after ~3 s
    fn draw_intro(&mut self, t: f64, title: &str) {
        let (wf, hf) = (self.w as f32, self.h as f32);
        let out = (if t > INTRO - 0.6 {
            (INTRO - t) / 0.6
        } else {
            1.0
        })
        .max(0.0) as f32;
        let dark = if t < 0.25 { 1.0 - t as f32 * 1.6 } else { 0.6 };
        self.fill(color([10, 3, 9], dark * out), BlendMode::SourceOver);
        let bar = (t / 0.35).min(1.0) as f32;
        let bh = wf.min(hf) * 0.3;
        let tr = Transform::from_translate(wf / 2.0, hf / 2.0)
            .pre_concat(Transform::from_rotate(-0.08f32.to_degrees()));
        if let Some(r) = rect(
            -wf * 0.75,
            -bh / 2.0,
            wf * 1.5 * (1.0 - (1.0 - bar).powi(3)),
            bh,
        ) {
            self.canvas.fill_rect(r, &paint(color(PINK, out)), tr, None);
        }
        let step = 18.0 * self.k;
        let mut x = -wf * 0.75;
        let mut pb = PathBuilder::new();
        while x < wf * 0.75 * (2.0 * bar - 1.0) + wf * 0.02 {
            pb.move_to(x, -bh / 2.0);
            pb.line_to(x + 8.0 * self.k, -bh / 2.0);
            pb.line_to(x + 8.0 * self.k - bh * 0.4, bh / 2.0);
            pb.line_to(x - bh * 0.4, bh / 2.0);
            pb.close();
            x += step;
        }
        if let Some(pth) = pb.finish() {
            self.canvas.fill_path(
                &pth,
                &paint(color([35, 15, 31], 0.35 * out)),
                FillRule::Winding,
                tr,
                None,
            );
        }
        if t > 0.2 {
            let ttl = if title.trim().is_empty() {
                "PMV"
            } else {
                title
            };
            self.draw_word(ttl, ((t - 0.2) * 0.5).min(0.29), 0.3, true, out);
        }
    }

    // Outro: the picture darkens, title and details, the last second black
    fn draw_outro(&mut self, p: f64, title: &str, credits: &str) {
        let (wf, hf) = (self.w as f32, self.h as f32);
        let kk = (p / (OUTRO - 1.0)).min(1.0) as f32;
        self.fill(color([10, 3, 9], 0.25 + 0.75 * kk), BlendMode::SourceOver);
        let a = (if p < 0.4 {
            p / 0.4
        } else if p > OUTRO - 1.2 {
            ((OUTRO - 0.3 - p) / 0.9).max(0.0)
        } else {
            1.0
        }) as f32;
        let size0 = wf.min(hf) * 0.12;
        let ttl = (if title.trim().is_empty() {
            "PMV"
        } else {
            title
        })
        .to_uppercase();
        let mut size = size0;
        let mw = self.font.measure(&ttl, size);
        if mw > wf * 0.86 {
            size = size * wf * 0.86 / mw;
        }
        if let Some(pth) = self.font.path(&ttl, size) {
            self.canvas.fill_path(
                &pth,
                &paint(color([251, 239, 244], a)),
                FillRule::Winding,
                Transform::from_translate(wf / 2.0, hf / 2.0 - size0 * 0.3),
                None,
            );
        }
        if let Some(r) = rect(
            wf / 2.0 - size0 * 1.2,
            hf / 2.0 + size0 * 0.35,
            size0 * 2.4,
            (size0 * 0.06).max(3.0),
        ) {
            self.canvas
                .fill_rect(r, &paint(color(PINK, a)), Transform::identity(), None);
        }
        if let Some(pth) = self.font.path(credits, size0 * 0.28) {
            self.canvas.fill_path(
                &pth,
                &paint(color([251, 239, 244], 0.75 * a)),
                FillRule::Winding,
                Transform::from_translate(wf / 2.0, hf / 2.0 + size0 * 0.8),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_matrix_matches_css() {
        // grayscale(1) of pure red = 0.2126 gray
        let c = Cm::grayscale(1.0);
        let r = c.0[0];
        assert!((r - 0.2126).abs() < 1e-4);
        // contrast(2) maps 0.25 → 0
        let c = Cm::contrast(2.0);
        assert!((c.0[0] * 0.25 + c.0[3]).abs() < 1e-5);
        // composing: brightness 2 then contrast 1 = brightness 2
        let m = Cm::brightness(2.0).then(Cm::ID);
        assert!((m.0[0] - 2.0).abs() < 1e-6);
        // order: brightness(0.5) then contrast(2): x → 2·(0.5x) − 0.5
        let m = Cm::brightness(0.5).then(Cm::contrast(2.0));
        assert!((m.0[0] - 1.0).abs() < 1e-6 && (m.0[3] + 0.5).abs() < 1e-6);
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    #[test]
    #[ignore]
    fn bench_draw() {
        let mut s = Settings::default();
        if std::env::var("BENCH_HEAVY").is_ok() {
            s.edge = "motion".into();
            s.fx.vhs = true;
            s.look = "noir".into();
        }
        let mut c = Compositor::new(1280, 720, &s, 1);
        let mut pix = Pixmap::new(1280, 720).unwrap();
        for (i, v) in pix.data_mut().iter_mut().enumerate() {
            *v = if i % 4 == 3 { 255 } else { (i * 7 % 251) as u8 };
        }
        let slots = crate::plan::slots_for("full", 1280, 720, "cols");
        for (name, w, h) in [
            ("cover 16:9", 1280.0, 720.0),
            ("contain portrait", 405.0, 720.0),
        ] {
            let p = if w < 1000.0 {
                Pixmap::new(405, 720).unwrap()
            } else {
                pix.clone()
            };
            let t0 = std::time::Instant::now();
            for i in 0..50 {
                let tile = Tile {
                    pix: &p,
                    w,
                    h,
                    image: false,
                    since: 1.0,
                    focus: Some((0.5, 0.5)),
                    lum: Some(100.0),
                    kb: 0.0,
                    zoom_dir: 1,
                };
                let st = FrameIn {
                    t: i as f64 * 0.033,
                    slots: &slots,
                    groups: vec![Some(tile)],
                    leave: vec![None],
                    energy: 0.5,
                    beat_t: 0.0,
                    beat_amt: 0.08,
                    next_t: Some(0.5),
                    next_amt: 0.08,
                    cut_count: 0,
                    reveal: None,
                    trig: Triggers::default(),
                    pic_flash: None,
                    pic_over: None,
                    song_start: 0.0,
                    duration: 100.0,
                    title: String::new(),
                    credits: String::new(),
                };
                c.draw(&st);
            }
            eprintln!(
                "{name}: {:.1} ms/frame",
                t0.elapsed().as_secs_f64() * 1000.0 / 50.0
            );
        }
    }
}
