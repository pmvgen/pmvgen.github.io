// Video encoders for the export: H.264, H.265, AV1 and a web format (VP9 + Opus in WebM), on the graphics
// card (NVIDIA NVENC, Intel Quick Sync, AMD AMF, VA-API on Linux, Media Foundation on Windows) or on the
// processor, each with three presets: fastest, balanced and smallest file.
//
// ffmpeg lists every encoder it was built with, also those whose hardware isn't there – so each one is tried
// on a tiny test video before it is used (once per run, cached).

use crate::ff;
use crate::util::LockExt;
use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Hardware kinds in the order "Automatic" tries them
pub const HW_ORDER: [&str; 5] = ["nvenc", "qsv", "amf", "vaapi", "mf"];

/// The ffmpeg encoders for a codec and an encoder kind, best first
pub fn names(codec: &str, hw: &str) -> &'static [&'static str] {
    match (codec, hw) {
        ("h264", "nvenc") => &["h264_nvenc"],
        ("h264", "qsv") => &["h264_qsv"],
        ("h264", "amf") => &["h264_amf"],
        ("h264", "vaapi") => &["h264_vaapi"],
        ("h264", "mf") => &["h264_mf"],
        ("h264", _) => &["libx264", "libopenh264"],
        ("h265", "nvenc") => &["hevc_nvenc"],
        ("h265", "qsv") => &["hevc_qsv"],
        ("h265", "amf") => &["hevc_amf"],
        ("h265", "vaapi") => &["hevc_vaapi"],
        ("h265", "mf") => &["hevc_mf"],
        ("h265", _) => &["libx265"],
        ("av1", "nvenc") => &["av1_nvenc"],
        ("av1", "qsv") => &["av1_qsv"],
        ("av1", "amf") => &["av1_amf"],
        ("av1", "vaapi") => &["av1_vaapi"],
        ("av1", "mf") => &[],
        ("av1", _) => &["libsvtav1", "libaom-av1", "librav1e"],
        ("web", "qsv") => &["vp9_qsv"],
        ("web", "vaapi") => &["vp9_vaapi"],
        ("web", "nvenc" | "amf" | "mf") => &[],
        ("web", _) => &["libvpx-vp9"],
        _ => &[],
    }
}

pub fn is_hw(name: &str) -> bool {
    ["_nvenc", "_qsv", "_amf", "_vaapi", "_mf"]
        .iter()
        .any(|s| name.ends_with(s))
}

/// File type of a codec's video
pub fn extension(codec: &str) -> &'static str {
    if codec == "web" {
        "webm"
    } else {
        "mp4"
    }
}

/// What ffmpeg was built with
fn listed() -> &'static HashSet<String> {
    static L: OnceLock<HashSet<String>> = OnceLock::new();
    L.get_or_init(|| {
        let out = ff::ffmpeg()
            .and_then(|mut c| {
                c.arg("-encoders");
                Ok(c.output()?)
            })
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        // lines like " V....D libx264   libx264 H.264 ..." (V = video, A = audio)
        out.lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                let flags = it.next()?;
                let name = it.next()?;
                (flags.len() == 6 && (flags.starts_with('V') || flags.starts_with('A')))
                    .then(|| name.to_string())
            })
            .collect()
    })
}

/// The render device for VA-API (Linux)
fn vaapi_device() -> Option<String> {
    static D: OnceLock<Option<String>> = OnceLock::new();
    D.get_or_init(|| {
        let mut l: Vec<String> = std::fs::read_dir("/dev/dri")
            .ok()?
            .flatten()
            .map(|e| e.path().to_string_lossy().to_string())
            .filter(|p| p.contains("renderD"))
            .collect();
        l.sort();
        l.into_iter().next()
    })
    .clone()
}

/// Run a command with a time limit; true if it ended well
fn run_limited(mut c: std::process::Command, limit: Duration) -> bool {
    let Ok(mut ch) = c.stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    let t0 = Instant::now();
    loop {
        match ch.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if t0.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = ch.kill();
                let _ = ch.wait();
                return false;
            }
        }
    }
}

/// Does this encoder really work here? (a few frames of a test picture; cached)
pub fn works(name: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lk().get(name) {
        return *v;
    }
    let ok = listed().contains(name) && {
        match ff::ffmpeg() {
            Ok(mut c) => {
                let a = args(name, "balanced", 23.0, 256, 144);
                c.args(&a.pre_input);
                c.args([
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=256x144:rate=30:duration=0.3",
                ]);
                if let Some(f) = &a.filter {
                    c.arg("-vf").arg(f);
                }
                c.arg("-c:v").arg(name);
                c.args(&a.out);
                c.args(["-frames:v", "8", "-f", "null", "-"]);
                run_limited(c, Duration::from_secs(15))
            }
            Err(_) => false,
        }
    };
    cache.lk().insert(name.to_string(), ok);
    ok
}

/// The encoder to use for a codec: the chosen kind if it works, "auto" = the first graphics card encoder that
/// works, else the processor's. → (encoder, a note when the choice wasn't possible)
pub fn choose(codec: &str, pref: &str) -> Option<(String, Option<String>)> {
    let first = |hw: &str| {
        names(codec, hw)
            .iter()
            .find(|n| works(n))
            .map(|n| n.to_string())
    };
    let cpu = || first("cpu");
    match pref {
        "cpu" => cpu().map(|c| (c, None)),
        "auto" => {
            for hw in HW_ORDER {
                if hw == "mf" && !cfg!(windows) || hw == "vaapi" && !cfg!(target_os = "linux") {
                    continue;
                }
                if let Some(n) = first(hw) {
                    return Some((n, None));
                }
            }
            cpu().map(|c| (c, None))
        }
        hw => match first(hw) {
            Some(n) => Some((n, None)),
            None => cpu().map(|c| {
                (
                    c,
                    Some(format!(
                        "{} isn't available for {} here – the processor encodes instead",
                        crate::settings::ENCODERS
                            .iter()
                            .find(|e| e.0 == hw)
                            .map(|e| e.1)
                            .unwrap_or(hw),
                        crate::settings::CODECS
                            .iter()
                            .find(|e| e.0 == codec)
                            .map(|e| e.1)
                            .unwrap_or(codec)
                    )),
                )
            }),
        },
    }
}

/// Which encoder kinds work for which codecs here (for the window – slow the first time: test encodes)
pub fn survey() -> Vec<(&'static str, &'static str, String)> {
    let mut out = Vec::new();
    for (codec, _) in crate::settings::CODECS {
        for hw in HW_ORDER.iter().chain(&["cpu"]) {
            if let Some(n) = names(codec, hw).iter().find(|n| works(n)) {
                out.push((codec, *hw, n.to_string()));
            }
        }
    }
    out
}

/// ffmpeg arguments for an encoder
#[derive(Debug, Default, Clone, PartialEq)]
pub struct EncArgs {
    /// before the inputs (e.g. the VA-API device)
    pub pre_input: Vec<String>,
    /// a video filter at the end (upload to the graphics card)
    pub filter: Option<String>,
    /// after -c:v NAME
    pub out: Vec<String>,
}

/// `crf`: quality on the x264 scale (lower = better, 12–40); speed: fast | balanced | small
pub fn args(name: &str, speed: &str, crf: f64, w: u32, h: u32) -> EncArgs {
    // (graphics card encoders aim at a quality, not a size: for "smallest file" their better tools are spent on
    // size – a slightly higher number at about the same look)
    let c = (crf
        + if is_hw(name) && speed == "small" {
            3.0
        } else {
            0.0
        })
    .clamp(0.0, 51.0);
    let q = |add: f64, max: f64| format!("{}", (c + add).round().clamp(0.0, max) as i64);
    let pick = |fast: &str, bal: &str, small: &str| -> String {
        match speed {
            "fast" => fast,
            "small" => small,
            _ => bal,
        }
        .to_string()
    };
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut a = EncArgs::default();
    let px = (w as f64 * h as f64).max(1.0);
    match name {
        "libx264" => {
            a.out = s(&["-preset"]);
            a.out.push(pick("veryfast", "medium", "slower"));
            a.out.extend(s(&["-crf"]));
            a.out.push(q(0.0, 51.0));
            a.out
                .extend(s(&["-pix_fmt", "yuv420p", "-profile:v", "high"]));
        }
        "libopenh264" => {
            // (bitrate only) ~ 0.1 bits per pixel at 30 fps, less for "small"
            let bpp = match speed {
                "small" => 0.06,
                "fast" => 0.12,
                _ => 0.09,
            } * (28.0 / c.max(12.0));
            a.out = s(&["-b:v"]);
            a.out
                .push(format!("{}k", (px * 30.0 * bpp / 1000.0).round() as i64));
            a.out.extend(s(&["-pix_fmt", "yuv420p"]));
        }
        "libx265" => {
            a.out = s(&["-preset"]);
            a.out.push(pick("veryfast", "medium", "slow"));
            a.out.extend(s(&["-crf"]));
            a.out.push(q(4.0, 51.0));
            a.out.extend(s(&[
                "-pix_fmt",
                "yuv420p",
                "-tag:v",
                "hvc1",
                "-x265-params",
                "log-level=error",
            ]));
        }
        "libsvtav1" => {
            a.out = s(&["-preset"]);
            a.out.push(pick("10", "7", "4"));
            a.out.extend(s(&["-crf"]));
            a.out.push(format!(
                "{}",
                (c * 1.35 + 4.0).round().clamp(1.0, 63.0) as i64
            ));
            a.out.extend(s(&["-pix_fmt", "yuv420p"]));
        }
        "libaom-av1" => {
            a.out = s(&["-cpu-used"]);
            a.out.push(pick("8", "6", "4"));
            a.out.extend(s(&["-row-mt", "1", "-b:v", "0", "-crf"]));
            a.out.push(format!(
                "{}",
                (c * 1.35 + 4.0).round().clamp(1.0, 63.0) as i64
            ));
            a.out.extend(s(&["-pix_fmt", "yuv420p"]));
        }
        "librav1e" => {
            a.out = s(&["-speed"]);
            a.out.push(pick("10", "6", "4"));
            a.out.extend(s(&["-qp"]));
            a.out
                .push(format!("{}", (c * 5.0).round().clamp(0.0, 255.0) as i64));
            a.out.extend(s(&["-pix_fmt", "yuv420p"]));
        }
        "libvpx-vp9" => {
            a.out = s(&["-b:v", "0", "-crf"]);
            a.out.push(q(11.0, 63.0));
            a.out.extend(s(&["-deadline", "good", "-cpu-used"]));
            a.out.push(pick("5", "3", "1"));
            a.out.extend(s(&[
                "-row-mt",
                "1",
                "-tile-columns",
                "2",
                "-pix_fmt",
                "yuv420p",
            ]));
        }
        n if n.ends_with("_nvenc") => {
            // (NVENC's CQ makes bigger files than x264's CRF at the same number – shifted to about match)
            let add = if n.starts_with("hevc") {
                6.0
            } else if n.starts_with("av1") {
                10.0
            } else {
                4.0
            };
            a.out = s(&["-preset"]);
            a.out.push(pick("p2", "p5", "p7"));
            a.out.extend(s(&["-tune", "hq", "-rc", "vbr", "-cq"]));
            a.out.push(q(add, 51.0));
            a.out.extend(s(&["-b:v", "0", "-spatial-aq", "1"]));
            // smaller files: look ahead, two passes per frame, B-frames as references (H.264 / H.265)
            match speed {
                "small" => {
                    a.out
                        .extend(s(&["-rc-lookahead", "32", "-multipass", "fullres"]));
                    if !n.starts_with("av1") {
                        a.out.extend(s(&["-bf", "3", "-b_ref_mode", "middle"]));
                    }
                }
                "balanced" => a.out.extend(s(&["-rc-lookahead", "16"])),
                _ => {}
            }
            a.out.extend(s(&["-pix_fmt", "yuv420p"]));
        }
        n if n.ends_with("_qsv") => {
            let add = match n {
                "hevc_qsv" => 3.0,
                "av1_qsv" => 8.0,
                "vp9_qsv" => 10.0,
                _ => 0.0,
            };
            a.out = s(&["-preset"]);
            a.out.push(pick("veryfast", "medium", "veryslow"));
            a.out.extend(s(&["-global_quality"]));
            a.out.push(q(add, 51.0));
            a.out.extend(s(&["-pix_fmt", "nv12"]));
        }
        n if n.ends_with("_amf") => {
            let add = if n.starts_with("hevc") || n.starts_with("av1") {
                2.0
            } else {
                0.0
            };
            a.out = s(&["-quality"]);
            a.out.push(pick("speed", "balanced", "quality"));
            a.out.extend(s(&["-rc", "cqp", "-qp_i"]));
            a.out.push(q(add, 51.0));
            a.out.extend(s(&["-qp_p"]));
            a.out.push(q(add + 2.0, 51.0));
            a.out.extend(s(&["-pix_fmt", "nv12"]));
        }
        n if n.ends_with("_vaapi") => {
            let add = match n {
                "hevc_vaapi" => 3.0,
                "av1_vaapi" => 8.0,
                "vp9_vaapi" => 10.0,
                _ => 0.0,
            };
            if let Some(d) = vaapi_device() {
                a.pre_input = vec!["-vaapi_device".into(), d];
            }
            a.filter = Some("format=nv12,hwupload".into());
            a.out = s(&["-rc_mode", "CQP", "-qp"]);
            a.out
                .push(q(add, if n == "vp9_vaapi" { 255.0 } else { 51.0 }));
            a.out.extend(s(&["-compression_level"]));
            a.out.push(pick("7", "4", "1"));
        }
        n if n.ends_with("_mf") => {
            a.out = s(&["-rate_control", "quality", "-quality"]);
            a.out.push(format!(
                "{}",
                (100.0 - c * 2.0).round().clamp(1.0, 100.0) as i64
            ));
            a.out.extend(s(&["-hw_encoding", "1", "-pix_fmt", "nv12"]));
        }
        _ => {
            a.out = s(&["-q:v", "3", "-pix_fmt", "yuv420p"]);
        }
    }
    a
}

/// Audio arguments for a codec's container
pub fn audio_args(codec: &str) -> Vec<String> {
    let l: &[&str] = if codec == "web" {
        if listed().contains("libopus") {
            &["-c:a", "libopus", "-b:a", "160k"]
        } else {
            &["-c:a", "libvorbis", "-q:a", "6"]
        }
    } else {
        &["-c:a", "aac", "-b:a", "192k"]
    };
    l.iter().map(|s| s.to_string()).collect()
}

/// Container arguments
pub fn container_args(codec: &str) -> Vec<String> {
    if codec == "web" {
        vec!["-f".into(), "webm".into()]
    } else {
        vec![
            "-movflags".into(),
            "+faststart".into(),
            "-f".into(),
            "mp4".into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_codec_has_a_processor_encoder() {
        for (c, _) in crate::settings::CODECS {
            assert!(!names(c, "cpu").is_empty(), "{c}");
        }
        assert_eq!(extension("web"), "webm");
        assert_eq!(extension("h265"), "mp4");
        assert!(is_hw("h264_nvenc") && !is_hw("libx264"));
    }
    #[test]
    fn presets_differ_and_quality_maps() {
        let fast = args("libx264", "fast", 20.0, 1280, 720);
        let small = args("libx264", "small", 20.0, 1280, 720);
        assert_ne!(fast, small);
        assert!(fast.out.windows(2).any(|w| w[0] == "-crf" && w[1] == "20"));
        let h = args("libx265", "balanced", 20.0, 1280, 720);
        assert!(h.out.windows(2).any(|w| w[0] == "-crf" && w[1] == "24"));
        let v = args("libvpx-vp9", "balanced", 20.0, 1280, 720);
        assert!(v.out.windows(2).any(|w| w[0] == "-crf" && w[1] == "31"));
        let a = args("h264_vaapi", "fast", 20.0, 1280, 720);
        assert_eq!(a.filter.as_deref(), Some("format=nv12,hwupload"));
        for n in [
            "h264_nvenc",
            "hevc_qsv",
            "av1_amf",
            "vp9_vaapi",
            "h264_mf",
            "libsvtav1",
            "libaom-av1",
            "librav1e",
            "libopenh264",
        ] {
            assert!(!args(n, "small", 30.0, 640, 360).out.is_empty(), "{n}");
        }
    }
}
