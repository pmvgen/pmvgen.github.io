// Colour themes: the web version's own ("PMV"), the system's (GTK on Linux – read from the theme's gtk.css –,
// Fluent with the accent colour on Windows 11), and popular editor / desktop themes (Catppuccin, Dracula, Nord,
// Sweet, Tokyo Night, Gruvbox, Rosé Pine …). Every colour of the window comes from the active palette.

use eframe::egui::Color32;
#[cfg(not(windows))]
use std::path::PathBuf;

/// A palette – the web version's CSS variables, for any theme
#[derive(Clone, Debug, PartialEq)]
pub struct Pal {
    pub name: String,
    pub dark: bool,
    /// page background (--bg)
    pub bg: Color32,
    /// cards (--bg-2)
    pub bg2: Color32,
    /// borders, hovered things (--bg-3)
    pub bg3: Color32,
    /// fields, tiles (--base)
    pub base: Color32,
    pub line: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub faint: Color32,
    /// the accent (--pink)
    pub accent: Color32,
    pub accent2: Color32,
    pub accent_deep: Color32,
    /// the second colour of the glows (--violet)
    pub second: Color32,
    pub shine: Color32,
    /// text on the accent (--ink)
    pub on_accent: Color32,
    pub ok: Color32,
    pub danger: Color32,
    pub sechead_hover: Color32,
}

/// A colour with alpha, premultiplied in gamma (sRGB) space – how browsers blend, and how egui blends.
/// (egui's own from_rgba_unmultiplied premultiplies in linear light: light colours come out too bright.)
pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    let k = a as f32 / 255.0;
    let m = |v: u8| (v as f32 * k).round() as u8;
    Color32::from_rgba_premultiplied(m(r), m(g), m(b), a)
}

pub fn hex(s: &str) -> Color32 {
    let s = s.trim().trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    match s.len() {
        3 => {
            let q = |i: usize| u8::from_str_radix(&s[i..i + 1].repeat(2), 16).unwrap_or(0);
            Color32::from_rgb(q(0), q(1), q(2))
        }
        8 => rgba(p(0), p(2), p(4), p(6)),
        _ => Color32::from_rgb(p(0), p(2), p(4)),
    }
}

/// a + (b − a)·t on the premultiplied sRGB channels (CSS color-mix in srgb)
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let (x, y) = (a.to_array(), b.to_array());
    let l = |i: usize| (x[i] as f32 + (y[i] as f32 - x[i] as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(0), l(1), l(2), l(3))
}

pub fn luminance(c: Color32) -> f32 {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

/// Dark or light text, whichever reads better on c
pub fn readable_on(c: Color32, dark: Color32, light: Color32) -> Color32 {
    if luminance(c) > 0.42 {
        dark
    } else {
        light
    }
}

/// The few colours a theme needs; the rest follows from them
#[allow(clippy::too_many_arguments)]
fn make(
    name: &str,
    dark: bool,
    bg: &str,
    bg2: &str,
    bg3: &str,
    base: &str,
    text: &str,
    text2: &str,
    faint: &str,
    accent: &str,
    accent2: &str,
    second: &str,
    ok: &str,
    danger: &str,
) -> Pal {
    let (bg, bg2, bg3, base) = (hex(bg), hex(bg2), hex(bg3), hex(base));
    let (text, accent) = (hex(text), hex(accent));
    Pal {
        name: name.into(),
        dark,
        bg,
        bg2,
        bg3,
        base,
        line: mix(bg2, bg3, 0.6),
        text,
        text2: hex(text2),
        faint: hex(faint),
        accent,
        accent2: hex(accent2),
        accent_deep: mix(accent, bg, if dark { 0.55 } else { 0.3 }),
        second: hex(second),
        shine: mix(hex(second), Color32::WHITE, 0.4),
        on_accent: readable_on(
            accent,
            if dark { bg } else { hex("#1b1b1f") },
            Color32::WHITE,
        ),
        ok: hex(ok),
        danger: hex(danger),
        sechead_hover: mix(bg3, text, 0.07),
    }
}

/// The web version's look
pub fn pmv() -> Pal {
    let mut p = make(
        "PMV", true, "#230f1f", "#34172e", "#4a2342", "#1a0a17", "#fbeff4", "#d6bccd", "#a07a96",
        "#ff3e8a", "#ff6ea8", "#b48cff", "#5ee39a", "#ff5a5a",
    );
    p.line = hex("#45203d");
    p.accent_deep = hex("#7a1d4a");
    p.shine = hex("#8fe3ff");
    p.on_accent = hex("#230f1f");
    p.sechead_hover = hex("#5a2b50");
    p
}

/// (id, name) of every theme, in the order of the menu
pub const THEMES: [(&str, &str); 25] = [
    ("pmv", "PMV (original)"),
    ("system", "System"),
    ("catppuccin-mocha", "Catppuccin Mocha"),
    ("catppuccin-macchiato", "Catppuccin Macchiato"),
    ("catppuccin-frappe", "Catppuccin Frappé"),
    ("catppuccin-latte", "Catppuccin Latte"),
    ("dracula", "Dracula"),
    ("nord", "Nord"),
    ("sweet", "Sweet"),
    ("sweet-mars", "Sweet Mars"),
    ("tokyo-night", "Tokyo Night"),
    ("gruvbox", "Gruvbox Dark"),
    ("rose-pine", "Rosé Pine"),
    ("one-dark", "One Dark"),
    ("solarized-dark", "Solarized Dark"),
    ("solarized-light", "Solarized Light"),
    ("everforest", "Everforest"),
    ("kanagawa", "Kanagawa"),
    ("monokai", "Monokai Pro"),
    ("github-dark", "GitHub Dark"),
    ("github-light", "GitHub Light"),
    ("fluent-dark", "Fluent Dark (Windows 11)"),
    ("fluent-light", "Fluent Light (Windows 11)"),
    ("adwaita-dark", "Adwaita Dark"),
    ("adwaita-light", "Adwaita Light"),
];

pub fn by_id(id: &str) -> Pal {
    let m = make;
    match id {
        "system" => system().unwrap_or_else(pmv),
        "catppuccin-mocha" => m(
            "Catppuccin Mocha",
            true,
            "#1e1e2e",
            "#313244",
            "#45475a",
            "#181825",
            "#cdd6f4",
            "#bac2de",
            "#7f849c",
            "#f5c2e7",
            "#cba6f7",
            "#b4befe",
            "#a6e3a1",
            "#f38ba8",
        ),
        "catppuccin-macchiato" => m(
            "Catppuccin Macchiato",
            true,
            "#24273a",
            "#363a4f",
            "#494d64",
            "#1e2030",
            "#cad3f5",
            "#b8c0e0",
            "#8087a2",
            "#f5bde6",
            "#c6a0f6",
            "#b7bdf8",
            "#a6da95",
            "#ed8796",
        ),
        "catppuccin-frappe" => m(
            "Catppuccin Frappé",
            true,
            "#303446",
            "#414559",
            "#51576d",
            "#292c3c",
            "#c6d0f5",
            "#b5bfe2",
            "#838ba7",
            "#f4b8e4",
            "#ca9ee6",
            "#babbf1",
            "#a6d189",
            "#e78284",
        ),
        "catppuccin-latte" => m(
            "Catppuccin Latte",
            false,
            "#e6e9ef",
            "#eff1f5",
            "#ccd0da",
            "#f7f8fa",
            "#4c4f69",
            "#5c5f77",
            "#8c8fa1",
            "#ea76cb",
            "#8839ef",
            "#7287fd",
            "#40a02b",
            "#d20f39",
        ),
        "dracula" => m(
            "Dracula", true, "#282a36", "#343746", "#44475a", "#21222c", "#f8f8f2", "#e2e2dc",
            "#6272a4", "#ff79c6", "#ff92d0", "#bd93f9", "#50fa7b", "#ff5555",
        ),
        "nord" => m(
            "Nord", true, "#2e3440", "#3b4252", "#4c566a", "#272c36", "#eceff4", "#d8dee9",
            "#7b88a1", "#88c0d0", "#8fbcbb", "#b48ead", "#a3be8c", "#bf616a",
        ),
        "sweet" => m(
            "Sweet", true, "#161925", "#1e2132", "#2c3047", "#0f111a", "#e6e8ee", "#c3c7d1",
            "#6d707b", "#c50ed2", "#e04ef0", "#6d44de", "#71f79f", "#ed254e",
        ),
        "sweet-mars" => m(
            "Sweet Mars",
            true,
            "#1a1e21",
            "#22282c",
            "#323a3f",
            "#14181a",
            "#e1e2e3",
            "#b6b7b9",
            "#686b6d",
            "#f67e7d",
            "#ff9e8f",
            "#ac87d4",
            "#00c1e4",
            "#f46665",
        ),
        "tokyo-night" => m(
            "Tokyo Night",
            true,
            "#1a1b26",
            "#24283b",
            "#414868",
            "#16161e",
            "#c0caf5",
            "#a9b1d6",
            "#565f89",
            "#7aa2f7",
            "#bb9af7",
            "#bb9af7",
            "#9ece6a",
            "#f7768e",
        ),
        "gruvbox" => m(
            "Gruvbox Dark",
            true,
            "#282828",
            "#3c3836",
            "#504945",
            "#1d2021",
            "#ebdbb2",
            "#d5c4a1",
            "#928374",
            "#fe8019",
            "#fabd2f",
            "#d3869b",
            "#b8bb26",
            "#fb4934",
        ),
        "rose-pine" => m(
            "Rosé Pine",
            true,
            "#191724",
            "#1f1d2e",
            "#403d52",
            "#16141f",
            "#e0def4",
            "#c5c2dd",
            "#6e6a86",
            "#eb6f92",
            "#ebbcba",
            "#c4a7e7",
            "#9ccfd8",
            "#eb6f92",
        ),
        "one-dark" => m(
            "One Dark", true, "#282c34", "#2f343e", "#3e4451", "#21252b", "#dcdfe4", "#abb2bf",
            "#5c6370", "#61afef", "#82c3ff", "#c678dd", "#98c379", "#e06c75",
        ),
        "solarized-dark" => m(
            "Solarized Dark",
            true,
            "#002b36",
            "#073642",
            "#0f4a57",
            "#00212b",
            "#eee8d5",
            "#93a1a1",
            "#586e75",
            "#d33682",
            "#e85fa3",
            "#6c71c4",
            "#859900",
            "#dc322f",
        ),
        "solarized-light" => m(
            "Solarized Light",
            false,
            "#eee8d5",
            "#fdf6e3",
            "#dcd5bf",
            "#fffbef",
            "#073642",
            "#586e75",
            "#93a1a1",
            "#d33682",
            "#268bd2",
            "#6c71c4",
            "#859900",
            "#dc322f",
        ),
        "everforest" => m(
            "Everforest",
            true,
            "#2d353b",
            "#343f44",
            "#475258",
            "#232a2e",
            "#d3c6aa",
            "#bdb39a",
            "#859289",
            "#a7c080",
            "#83c092",
            "#d699b6",
            "#a7c080",
            "#e67e80",
        ),
        "kanagawa" => m(
            "Kanagawa", true, "#1f1f28", "#2a2a37", "#54546d", "#16161d", "#dcd7ba", "#c8c093",
            "#727169", "#d27e99", "#e46876", "#957fb8", "#98bb6c", "#e46876",
        ),
        "monokai" => m(
            "Monokai Pro",
            true,
            "#2d2a2e",
            "#363337",
            "#5b595c",
            "#221f22",
            "#fcfcfa",
            "#c1c0c0",
            "#727072",
            "#ff6188",
            "#fc9867",
            "#ab9df2",
            "#a9dc76",
            "#ff6188",
        ),
        "github-dark" => m(
            "GitHub Dark",
            true,
            "#0d1117",
            "#161b22",
            "#30363d",
            "#010409",
            "#e6edf3",
            "#c9d1d9",
            "#7d8590",
            "#2f81f7",
            "#58a6ff",
            "#bc8cff",
            "#3fb950",
            "#f85149",
        ),
        "github-light" => m(
            "GitHub Light",
            false,
            "#f6f8fa",
            "#ffffff",
            "#d0d7de",
            "#ffffff",
            "#1f2328",
            "#424a53",
            "#656d76",
            "#0969da",
            "#218bff",
            "#8250df",
            "#1a7f37",
            "#cf222e",
        ),
        "fluent-dark" => fluent(true, None),
        "fluent-light" => fluent(false, None),
        "adwaita-dark" => adwaita(true, None),
        "adwaita-light" => adwaita(false, None),
        _ => pmv(),
    }
}

fn fluent(dark: bool, accent: Option<Color32>) -> Pal {
    let mut p = if dark {
        make(
            "Fluent Dark",
            true,
            "#202020",
            "#2b2b2b",
            "#3d3d3d",
            "#1c1c1c",
            "#ffffff",
            "#d1d1d1",
            "#9d9d9d",
            "#60cdff",
            "#99ebff",
            "#b4a0ff",
            "#6ccb5f",
            "#ff99a4",
        )
    } else {
        make(
            "Fluent Light",
            false,
            "#f3f3f3",
            "#fbfbfb",
            "#e0e0e0",
            "#ffffff",
            "#1b1b1b",
            "#5c5c5c",
            "#8a8a8a",
            "#005fb8",
            "#0067c0",
            "#7160e8",
            "#0f7b0f",
            "#c42b1c",
        )
    };
    if let Some(a) = accent {
        p = with_accent(p, a);
    }
    p
}

fn adwaita(dark: bool, accent: Option<Color32>) -> Pal {
    let mut p = if dark {
        make(
            "Adwaita Dark",
            true,
            "#242424",
            "#303030",
            "#464646",
            "#1e1e1e",
            "#ffffff",
            "#deddda",
            "#9a9996",
            "#3584e4",
            "#62a0ea",
            "#9141ac",
            "#33d17a",
            "#e01b24",
        )
    } else {
        make(
            "Adwaita Light",
            false,
            "#f6f5f4",
            "#ffffff",
            "#d6d4d1",
            "#ffffff",
            "#1f1f1f",
            "#5e5c64",
            "#77767b",
            "#3584e4",
            "#1c71d8",
            "#9141ac",
            "#26a269",
            "#c01c28",
        )
    };
    if let Some(a) = accent {
        p = with_accent(p, a);
    }
    p
}

fn with_accent(mut p: Pal, a: Color32) -> Pal {
    p.accent = a;
    p.accent2 = mix(a, Color32::WHITE, 0.25);
    p.accent_deep = mix(a, p.bg, if p.dark { 0.55 } else { 0.3 });
    p.on_accent = readable_on(a, Color32::from_rgb(0x1b, 0x1b, 0x1f), Color32::WHITE);
    p
}

// ---------- the system's theme ----------

/// The desktop's colours: Windows 11 (light / dark and the accent colour), or the GTK theme on Linux
pub fn system() -> Option<Pal> {
    #[cfg(windows)]
    {
        windows_theme()
    }
    #[cfg(not(windows))]
    {
        gtk_theme()
    }
}

#[cfg(windows)]
fn reg_dword(key: &str, value: &str) -> Option<u32> {
    let out = crate::ff::command(std::path::Path::new("reg"))
        .args(["query", key, "/v", value])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let tok = s
        .split_whitespace()
        .skip_while(|t| *t != "REG_DWORD")
        .nth(1)?;
    u32::from_str_radix(tok.trim_start_matches("0x"), 16).ok()
}

#[cfg(windows)]
fn windows_theme() -> Option<Pal> {
    let light = reg_dword(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "AppsUseLightTheme",
    )
    .map(|v| v == 1)
    .unwrap_or(false);
    // AccentColor is 0xAABBGGRR
    let accent = reg_dword(r"HKCU\Software\Microsoft\Windows\DWM", "AccentColor").map(|v| {
        Color32::from_rgb(
            (v & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            ((v >> 16) & 0xff) as u8,
        )
    });
    let mut p = fluent(!light, accent);
    p.name = "System (Windows)".into();
    Some(p)
}

#[cfg(not(windows))]
fn gsetting(schema: &str, key: &str) -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout)
        .trim()
        .trim_matches('\'')
        .to_string();
    (!s.is_empty() && out.status.success()).then_some(s)
}

/// The GTK theme's name (GTK_THEME, Cinnamon, GNOME, or settings.ini)
#[cfg(not(windows))]
fn gtk_theme_name() -> Option<String> {
    if let Ok(t) = std::env::var("GTK_THEME") {
        return Some(t.split(':').next().unwrap_or("").to_string());
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();
    if desktop.contains("cinnamon") {
        if let Some(t) = gsetting("org.cinnamon.desktop.interface", "gtk-theme") {
            return Some(t);
        }
    }
    if let Some(t) = gsetting("org.gnome.desktop.interface", "gtk-theme") {
        return Some(t);
    }
    let ini = dirs::config_dir()?.join("gtk-3.0").join("settings.ini");
    std::fs::read_to_string(ini).ok()?.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "gtk-theme-name").then(|| v.trim().to_string())
    })
}

#[cfg(not(windows))]
fn theme_css(name: &str, dark: bool) -> Option<String> {
    let mut dirs_: Vec<PathBuf> = Vec::new();
    if let Some(h) = dirs::home_dir() {
        dirs_.push(h.join(".themes"));
        dirs_.push(h.join(".local/share/themes"));
    }
    dirs_.push("/usr/share/themes".into());
    dirs_.push("/usr/local/share/themes".into());
    for d in dirs_ {
        let g = d.join(name).join("gtk-3.0");
        let order = if dark {
            ["gtk-dark.css", "gtk.css"]
        } else {
            ["gtk.css", "gtk-dark.css"]
        };
        for f in order {
            if let Ok(s) = std::fs::read_to_string(g.join(f)) {
                if s.contains("@define-color") {
                    return Some(s);
                }
            }
        }
    }
    None
}

/// Colour values of GTK CSS: #hex, rgb()/rgba(), names, @references, shade(), alpha(), mix(), lighter(), darker()
struct GtkColors {
    defs: std::collections::HashMap<String, String>,
}
impl GtkColors {
    fn parse(css: &str) -> GtkColors {
        let mut defs = std::collections::HashMap::new();
        // (comments out)
        let mut clean = String::with_capacity(css.len());
        let mut rest = css;
        while let Some(i) = rest.find("/*") {
            clean.push_str(&rest[..i]);
            rest = rest[i..]
                .find("*/")
                .map(|j| &rest[i + j + 2..])
                .unwrap_or("");
        }
        clean.push_str(rest);
        for stmt in clean.split(';') {
            let s = stmt.trim();
            if let Some(r) = s.find("@define-color") {
                let mut it = s[r + 13..].trim().splitn(2, char::is_whitespace);
                if let (Some(k), Some(v)) = (it.next(), it.next()) {
                    // (the first definition wins – themes repeat theirs further down for documentation)
                    defs.entry(k.trim().to_string())
                        .or_insert_with(|| v.trim().to_string());
                }
            }
        }
        GtkColors { defs }
    }
    fn get(&self, name: &str) -> Option<Color32> {
        self.eval(self.defs.get(name)?, 0)
    }
    fn eval(&self, v: &str, depth: u32) -> Option<Color32> {
        if depth > 16 {
            return None;
        }
        let v = v.trim();
        if let Some(r) = v.strip_prefix('@') {
            return self.eval(self.defs.get(r)?, depth + 1);
        }
        if v.starts_with('#') {
            return Some(hex(v));
        }
        let (f, args) = match v.find('(') {
            Some(i) if v.ends_with(')') => (&v[..i], &v[i + 1..v.len() - 1]),
            _ => {
                return match v {
                    "black" => Some(Color32::BLACK),
                    "white" => Some(Color32::WHITE),
                    "transparent" => Some(Color32::TRANSPARENT),
                    _ => None,
                }
            }
        };
        // split the arguments at top-level commas
        let mut parts = Vec::new();
        let (mut d, mut start) = (0, 0);
        for (i, c) in args.char_indices() {
            match c {
                '(' => d += 1,
                ')' => d -= 1,
                ',' if d == 0 => {
                    parts.push(args[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(args[start..].trim());
        let num = |s: &str| s.trim().trim_end_matches('%').parse::<f32>().ok();
        match f.trim() {
            "rgb" | "rgba" => {
                let ch = |s: &str| -> Option<u8> {
                    let x = num(s)?;
                    Some(
                        if s.ends_with('%') {
                            (x * 2.55).round()
                        } else {
                            x.round()
                        }
                        .clamp(0.0, 255.0) as u8,
                    )
                };
                let a = parts.get(3).and_then(|s| num(s)).unwrap_or(1.0);
                Some(rgba(
                    ch(parts.first()?)?,
                    ch(parts.get(1)?)?,
                    ch(parts.get(2)?)?,
                    (a.clamp(0.0, 1.0) * 255.0) as u8,
                ))
            }
            "shade" => {
                let c = self.eval(parts.first()?, depth + 1)?;
                let k = num(parts.get(1)?)?;
                let sc = |x: u8| (x as f32 * k).round().clamp(0.0, c.a() as f32) as u8;
                Some(Color32::from_rgba_premultiplied(
                    sc(c.r()),
                    sc(c.g()),
                    sc(c.b()),
                    c.a(),
                ))
            }
            "lighter" => self.eval(&format!("shade({}, 1.3)", parts.first()?), depth + 1),
            "darker" => self.eval(&format!("shade({}, 0.7)", parts.first()?), depth + 1),
            "alpha" => {
                let c = self.eval(parts.first()?, depth + 1)?;
                let a = num(parts.get(1)?)?.clamp(0.0, 1.0);
                // (alpha of an opaque colour – the themes' colours are)
                let k = a * 255.0 / c.a().max(1) as f32;
                let m = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
                Some(Color32::from_rgba_premultiplied(
                    m(c.r()),
                    m(c.g()),
                    m(c.b()),
                    (a * 255.0) as u8,
                ))
            }
            "mix" => {
                let a = self.eval(parts.first()?, depth + 1)?;
                let b = self.eval(parts.get(1)?, depth + 1)?;
                Some(mix(a, b, num(parts.get(2)?)?))
            }
            _ => None,
        }
    }
}

/// A palette from a GTK theme's named colours
#[cfg_attr(windows, allow(dead_code))]
fn from_gtk(name: &str, css: &str) -> Option<Pal> {
    let g = GtkColors::parse(css);
    let bg = g.get("theme_bg_color").or_else(|| g.get("bg_color"))?;
    let fg = g.get("theme_fg_color").or_else(|| g.get("fg_color"))?;
    let base = g
        .get("theme_base_color")
        .or_else(|| g.get("base_color"))
        .unwrap_or(bg);
    let accent = g
        .get("theme_selected_bg_color")
        .or_else(|| g.get("selected_bg_color"))
        .or_else(|| g.get("accent_bg_color"))?;
    let dark = luminance(bg) < 0.3;
    let faint = g
        .get("insensitive_fg_color")
        .or_else(|| g.get("unfocused_fg_color"))
        .unwrap_or_else(|| mix(fg, bg, 0.45));
    // (the cards a touch apart from the page)
    let (card, edge) = if dark {
        (mix(bg, fg, 0.06), mix(bg, fg, 0.16))
    } else {
        (mix(bg, Color32::WHITE, 0.6), mix(bg, fg, 0.16))
    };
    let mut p = Pal {
        name: format!("System ({name})"),
        dark,
        bg,
        bg2: card,
        bg3: edge,
        base: if (luminance(base) - luminance(bg)).abs() < 0.002 {
            mix(bg, if dark { Color32::BLACK } else { Color32::WHITE }, 0.25)
        } else {
            base
        },
        line: mix(card, edge, 0.6),
        text: fg,
        text2: mix(fg, bg, 0.18),
        faint,
        accent,
        accent2: mix(accent, Color32::WHITE, 0.25),
        accent_deep: mix(accent, bg, if dark { 0.55 } else { 0.3 }),
        second: g
            .get("success_color")
            .unwrap_or_else(|| mix(accent, fg, 0.4)),
        shine: mix(accent, Color32::WHITE, 0.5),
        on_accent: g
            .get("theme_selected_fg_color")
            .unwrap_or_else(|| readable_on(accent, Color32::BLACK, Color32::WHITE)),
        ok: g.get("success_color").unwrap_or(hex("#33d17a")),
        danger: g.get("error_color").unwrap_or(hex("#e01b24")),
        sechead_hover: mix(edge, fg, 0.07),
    };
    // a second glow colour that isn't black
    if luminance(p.second) < 0.02 {
        p.second = p.accent2;
    }
    Some(p)
}

#[cfg(not(windows))]
fn gtk_theme() -> Option<Pal> {
    let name = gtk_theme_name()?;
    let prefer_dark = gsetting("org.gnome.desktop.interface", "color-scheme")
        .map(|v| v.contains("dark"))
        .unwrap_or(false)
        || name.to_lowercase().contains("dark");
    if let Some(css) = theme_css(&name, prefer_dark) {
        if let Some(p) = from_gtk(&name, &css) {
            return Some(p);
        }
    }
    // a theme built into GTK (Adwaita): its colours, with the desktop's accent colour if it sets one
    let accent = gsetting("org.gnome.desktop.interface", "accent-color").and_then(|a| {
        Some(hex(match a.as_str() {
            "blue" => "#3584e4",
            "teal" => "#2190a4",
            "green" => "#3a944a",
            "yellow" => "#c88800",
            "orange" => "#ed5b00",
            "red" => "#e62d42",
            "pink" => "#d56199",
            "purple" => "#9141ac",
            "slate" => "#6f8396",
            _ => return None,
        }))
    });
    let mut p = adwaita(prefer_dark, accent);
    p.name = format!("System ({name})");
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_theme_is_readable() {
        for (id, _) in THEMES {
            let p = by_id(id);
            let contrast = |a: Color32, b: Color32| {
                let (x, y) = (luminance(a), luminance(b));
                (x.max(y) + 0.05) / (x.min(y) + 0.05)
            };
            assert!(contrast(p.text, p.bg2) > 4.5, "{id}: text");
            assert!(contrast(p.on_accent, p.accent) > 2.2, "{id}: on accent");
            assert_eq!(p.dark, luminance(p.bg) < 0.3, "{id}: dark flag");
        }
    }
    #[test]
    fn gtk_css_colours() {
        let css = "/* x */ @define-color theme_bg_color #101013;\n@define-color theme_fg_color #F4F5F6;\n\
                   @define-color a rgba(255, 0, 0, 0.5);\n@define-color theme_selected_bg_color shade(@b, 0.5);\n\
                   @define-color b #0072ff;\n@define-color m mix(#000000, #ffffff, 0.5);\n@define-color al alpha(@b, 0.2);";
        let g = GtkColors::parse(css);
        assert_eq!(g.get("theme_bg_color"), Some(hex("#101013")));
        assert_eq!(g.get("a").unwrap().a(), 127);
        assert_eq!(g.get("a").unwrap().r(), 127, "premultiplied in gamma space");
        assert_eq!(
            g.get("theme_selected_bg_color"),
            Some(Color32::from_rgb(0, 57, 128))
        );
        assert_eq!(g.get("m"), Some(Color32::from_rgb(128, 128, 128)));
        assert_eq!(g.get("al").unwrap().a(), 51);
        let p = from_gtk("T", css).unwrap();
        assert!(p.dark);
        assert_eq!(p.text, hex("#F4F5F6"));
    }
    #[test]
    fn mixing_like_css() {
        // 50 % white over black, as color-mix(in srgb …) and the browser's blending give: mid grey 128
        let w = rgba(255, 255, 255, 128);
        assert_eq!(w.to_array(), [128, 128, 128, 128]);
        assert_eq!(
            mix(Color32::BLACK, Color32::WHITE, 0.5),
            Color32::from_rgb(128, 128, 128)
        );
    }
}
