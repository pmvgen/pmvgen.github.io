// ffmpeg / ffprobe: finding them, probing files, building commands.
// Every picture and sound goes through ffmpeg (it plays whatever the browser could, and more).

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

static FFMPEG: OnceLock<Option<PathBuf>> = OnceLock::new();
static FFPROBE: OnceLock<Option<PathBuf>> = OnceLock::new();

#[cfg(windows)]
const EXE: &str = ".exe";
#[cfg(not(windows))]
const EXE: &str = "";

fn locate(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{EXE}");
    // 1. next to the program (the bundled copy), also in a "ffmpeg" or "bin" folder beside it
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for sub in ["", "ffmpeg", "bin", "ffmpeg/bin"] {
                let p = if sub.is_empty() {
                    dir.join(&file)
                } else {
                    dir.join(sub).join(&file)
                };
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    // 2. on the PATH
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(&file);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn ffmpeg_path() -> Option<&'static PathBuf> {
    FFMPEG.get_or_init(|| locate("ffmpeg")).as_ref()
}
pub fn ffprobe_path() -> Option<&'static PathBuf> {
    FFPROBE.get_or_init(|| locate("ffprobe")).as_ref()
}

pub fn available() -> Result<()> {
    if ffmpeg_path().is_none() {
        bail!("ffmpeg was not found. Put ffmpeg{EXE} and ffprobe{EXE} next to this program, or install ffmpeg (Linux: sudo apt install ffmpeg).");
    }
    if ffprobe_path().is_none() {
        bail!("ffprobe was not found. Put ffprobe{EXE} next to this program, or install ffmpeg (Linux: sudo apt install ffmpeg).");
    }
    Ok(())
}

/// A command that never opens a console window on Windows.
pub fn command(prog: &Path) -> Command {
    let mut c = Command::new(prog);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.stdin(Stdio::null());
    c
}

pub fn ffmpeg() -> Result<Command> {
    let p = ffmpeg_path().ok_or_else(|| anyhow!("ffmpeg not found"))?;
    let mut c = command(p);
    c.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
    Ok(c)
}

#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub w: u32,
    pub h: u32,
    pub dur: f64,
    pub has_video: bool,
    pub has_audio: bool,
    pub channels: u32, // of the first audio stream
    pub still: bool,   // a single picture (image file or one-frame stream)
    /// frames per second of the video stream (0 when unknown)
    pub fps: f64,
}

/// Size (after rotation), duration, streams. Fails for files ffmpeg can't read.
pub fn probe(path: &Path) -> Result<Probe> {
    let p = ffprobe_path().ok_or_else(|| anyhow!("ffprobe not found"))?;
    let out = command(p)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("running ffprobe")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "can't be read: {}",
            err.lines().next().unwrap_or("unknown error")
        );
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).context("ffprobe output")?;
    let mut pr = Probe::default();
    let fdur = v["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok());
    let fmt_name = v["format"]["format_name"].as_str().unwrap_or("");
    if let Some(streams) = v["streams"].as_array() {
        for s in streams {
            let kind = s["codec_type"].as_str().unwrap_or("");
            if kind == "video" && !pr.has_video {
                // attached pictures (album covers) are not a video
                if s["disposition"]["attached_pic"].as_i64() == Some(1) {
                    continue;
                }
                let mut w = s["width"].as_u64().unwrap_or(0) as u32;
                let mut h = s["height"].as_u64().unwrap_or(0) as u32;
                if w == 0 || h == 0 {
                    continue;
                }
                let rate = |k: &str| {
                    let v = s[k].as_str()?;
                    let (a, b) = v.split_once('/').unwrap_or((v, "1"));
                    let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
                    (a > 0.0 && b > 0.0)
                        .then(|| a / b)
                        .filter(|f| (1.0..=1000.0).contains(f))
                };
                pr.fps = rate("avg_frame_rate")
                    .or_else(|| rate("r_frame_rate"))
                    .unwrap_or(0.0);
                // display aspect (non-square pixels)
                if let Some(sar) = s["sample_aspect_ratio"].as_str() {
                    let mut it = sar.split(':');
                    if let (Some(a), Some(b)) = (it.next(), it.next()) {
                        if let (Ok(a), Ok(b)) = (a.parse::<f64>(), b.parse::<f64>()) {
                            if a > 0.0 && b > 0.0 && (a - b).abs() > 0.01 {
                                w = ((w as f64) * a / b).round().max(2.0) as u32;
                            }
                        }
                    }
                }
                // rotation (phone videos): ffmpeg turns the picture upright, so the size is swapped
                let mut rot = s["tags"]["rotate"]
                    .as_str()
                    .and_then(|x| x.parse::<i64>().ok())
                    .unwrap_or(0);
                if let Some(sd) = s["side_data_list"].as_array() {
                    for d in sd {
                        if let Some(r) = d["rotation"].as_f64() {
                            rot = r.round() as i64;
                        } else if let Some(r) = d["rotation"].as_i64() {
                            rot = r;
                        }
                    }
                }
                if rot.rem_euclid(180) == 90 {
                    std::mem::swap(&mut w, &mut h);
                }
                pr.w = w;
                pr.h = h;
                pr.has_video = true;
                let frames = s["nb_frames"].as_str().and_then(|x| x.parse::<u64>().ok());
                let codec = s["codec_name"].as_str().unwrap_or("");
                pr.still = fmt_name.contains("image2")
                    || fmt_name.ends_with("_pipe")
                    || frames == Some(1)
                    || matches!(codec, "mjpeg" | "png" | "webp" | "bmp")
                        && fdur.is_none_or(|d| d < 0.1);
                if let Some(d) = s["duration"].as_str().and_then(|x| x.parse::<f64>().ok()) {
                    pr.dur = d;
                }
            } else if kind == "audio" && !pr.has_audio {
                pr.has_audio = true;
                pr.channels = s["channels"].as_u64().unwrap_or(2) as u32;
            }
        }
    }
    if let Some(d) = fdur {
        if d > 0.0 && (pr.dur <= 0.0 || pr.has_audio) {
            pr.dur = d;
        }
    }
    Ok(pr)
}

/// Read exactly `buf.len()` bytes, false at the end of the stream
pub fn read_full(r: &mut impl std::io::Read, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut got = 0;
    while got < buf.len() {
        let n = r.read(&mut buf[got..])?;
        if n == 0 {
            return Ok(false);
        }
        got += n;
    }
    Ok(true)
}

/// Decode a picture (image file – of an animated GIF / WebP the first frame – or one video frame at `at`
/// seconds) to straight RGBA at w×h
pub fn decode_rgba(path: &Path, at: Option<f64>, w: u32, h: u32) -> Result<Vec<u8>> {
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        bail!("no picture: size {w}×{h}");
    }
    let mut c = ffmpeg()?;
    if let Some(t) = at {
        c.arg("-ss").arg(format!("{:.3}", t.max(0.0)));
    }
    c.arg("-i").arg(path);
    c.args(["-frames:v", "1", "-an", "-sn", "-dn", "-vf"]);
    c.arg(format!("scale={w}:{h}:flags=bicubic,format=rgba"));
    c.args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
    let out = c.stdout(Stdio::piped()).stderr(Stdio::piped()).output()?;
    let need = (w * h * 4) as usize;
    if out.stdout.len() < need {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "no picture: {}",
            err.lines().next().unwrap_or("ffmpeg gave no frame")
        );
    }
    Ok(out.stdout[..need].to_vec())
}
