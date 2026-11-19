// Text as filled / stroked outlines for the title cards and the words. Built-in: Anton, a condensed display
// face (the web version asks for Bahnschrift Condensed / Impact); any TTF / OTF can be chosen instead.
// A character the chosen font doesn't have comes from Anton. Glyph outlines from ttf-parser become tiny-skia
// paths.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::sync::Arc;
use tiny_skia::{Path as SkPath, PathBuilder};

static BUILTIN: &[u8] = include_bytes!("../assets/Anton-Regular.ttf");

/// A font: the file's bytes (parsed when used – ttf-parser only reads tables, that is cheap)
#[derive(Clone)]
pub struct Font {
    data: Arc<Vec<u8>>,
    index: u32,
    pub name: String,
}

impl Default for Font {
    fn default() -> Self {
        Font::builtin()
    }
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Font({})", self.name)
    }
}

impl Font {
    pub fn builtin() -> Font {
        Font {
            data: Arc::new(BUILTIN.to_vec()),
            index: 0,
            name: "Anton (built-in)".into(),
        }
    }

    /// A font file (TTF, OTF, or the first font of a TTC collection)
    pub fn load(path: &Path) -> Result<Font> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        if data.len() > 64 << 20 {
            bail!("this font file is too big");
        }
        let face = ttf_parser::Face::parse(&data, 0)
            .map_err(|e| anyhow::anyhow!("not a font this program can read ({e})"))?;
        if face.glyph_index('A').is_none() && face.glyph_index('a').is_none() {
            bail!("this font has no Latin letters");
        }
        let name = face
            .names()
            .into_iter()
            .find(|n| n.name_id == ttf_parser::name_id::FULL_NAME && n.is_unicode())
            .and_then(|n| n.to_string())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Font".into())
            });
        Ok(Font {
            data: Arc::new(data),
            index: 0,
            name,
        })
    }

    /// The font of a setting: the file, or the built-in one when it is empty or can't be read
    pub fn from_setting(path: &str) -> Font {
        if path.trim().is_empty() {
            return Font::builtin();
        }
        match Font::load(Path::new(path)) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[pmvgen] font {path}: {e:#} – using the built-in one");
                Font::builtin()
            }
        }
    }

    fn face(&self) -> Option<ttf_parser::Face<'_>> {
        ttf_parser::Face::parse(&self.data, self.index).ok()
    }
}

/// The face and glyph a character is drawn with: the font's, else Anton's, else a '?'
fn glyph<'a>(
    main: Option<&ttf_parser::Face<'a>>,
    fallback: Option<&ttf_parser::Face<'static>>,
    c: char,
) -> Option<(bool, ttf_parser::GlyphId)> {
    if let Some(g) = main.and_then(|f| f.glyph_index(c)) {
        return Some((true, g));
    }
    if let Some(g) = fallback.and_then(|f| f.glyph_index(c)) {
        return Some((false, g));
    }
    main.and_then(|f| f.glyph_index('?'))
        .map(|g| (true, g))
        .or_else(|| {
            fallback
                .and_then(|f| f.glyph_index('?'))
                .map(|g| (false, g))
        })
}

fn builtin_face() -> Option<&'static ttf_parser::Face<'static>> {
    static FACE: std::sync::OnceLock<Option<ttf_parser::Face<'static>>> =
        std::sync::OnceLock::new();
    FACE.get_or_init(|| ttf_parser::Face::parse(BUILTIN, 0).ok())
        .as_ref()
}

struct Builder<'a> {
    pb: &'a mut PathBuilder,
    x: f32,
    s: f32,
    dy: f32,
}
impl ttf_parser::OutlineBuilder for Builder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.x + x * self.s, -y * self.s + self.dy);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.x + x * self.s, -y * self.s + self.dy);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(
            self.x + x1 * self.s,
            -y1 * self.s + self.dy,
            self.x + x * self.s,
            -y * self.s + self.dy,
        );
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.x + x1 * self.s,
            -y1 * self.s + self.dy,
            self.x + x2 * self.s,
            -y2 * self.s + self.dy,
            self.x + x * self.s,
            -y * self.s + self.dy,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

impl Font {
    /// Width of a text at a font size (px)
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        let main = self.face();
        let fb = builtin_face();
        if main.is_none() && fb.is_none() {
            return text.chars().count() as f32 * size * 0.45;
        }
        text.chars()
            .map(|c| match glyph(main.as_ref(), fb, c) {
                Some((true, g)) => {
                    let f = main.as_ref().expect("main face");
                    f.glyph_hor_advance(g).unwrap_or(0) as f32 * size / f.units_per_em() as f32
                }
                Some((false, g)) => {
                    let f = fb.expect("fallback face");
                    f.glyph_hor_advance(g).unwrap_or(0) as f32 * size / f.units_per_em() as f32
                }
                None => size * 0.45,
            })
            .sum()
    }

    /// The outline of a text centred on (0, 0) (canvas textAlign center, textBaseline middle)
    pub fn path(&self, text: &str, size: f32) -> Option<SkPath> {
        let main = self.face();
        let fb = builtin_face();
        let w = self.measure(text, size);
        // "middle": the middle of the em box – between ascender and descender (of the main font)
        let mid = |f: &ttf_parser::Face| {
            (f.ascender() as f32 + f.descender() as f32) / 2.0 * size / f.units_per_em() as f32
        };
        let main_mid = main.as_ref().map(mid).or_else(|| fb.map(mid))?;
        let mut pb = PathBuilder::new();
        let mut x = -w / 2.0;
        for c in text.chars() {
            let (face, g): (&ttf_parser::Face, _) = match glyph(main.as_ref(), fb, c) {
                Some((true, g)) => (main.as_ref()?, g),
                Some((false, g)) => (fb?, g),
                None => {
                    x += size * 0.45;
                    continue;
                }
            };
            let s = size / face.units_per_em() as f32;
            // (all glyphs – also fallback ones – sit on one baseline; the line is centred below)
            let mut b = Builder {
                pb: &mut pb,
                x,
                s,
                dy: 0.0,
            };
            face.outline_glyph(g, &mut b);
            x += face.glyph_hor_advance(g).unwrap_or(0) as f32 * s;
        }
        pb.finish()?
            .transform(tiny_skia::Transform::from_translate(0.0, main_mid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_draws_and_measures() {
        let f = Font::builtin();
        let w = f.measure("HELLO", 100.0);
        assert!(w > 100.0 && w < 400.0, "{w}");
        let p = f.path("HELLO", 100.0).expect("outline");
        let b = p.bounds();
        // centred
        assert!((b.left() + b.right()).abs() < 10.0, "{b:?}");
        assert!((b.top() + b.bottom()).abs() < 30.0, "{b:?}");
        // characters Anton doesn't have don't break it
        assert!(f.path("✓ ok", 50.0).is_some());
        assert!(f.path("", 50.0).is_none());
    }
    #[test]
    fn bad_font_files() {
        let dir = std::env::temp_dir().join(format!("pmvgen-font-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.ttf");
        std::fs::write(&p, b"not a font").unwrap();
        assert!(Font::load(&p).is_err());
        assert!(Font::load(&dir.join("missing.ttf")).is_err());
        assert_eq!(
            Font::from_setting(p.to_str().unwrap()).name,
            "Anton (built-in)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
