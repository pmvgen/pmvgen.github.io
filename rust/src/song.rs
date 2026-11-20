// A song: decoded once by ffmpeg into stereo PCM for playing, and boiled down to beats while it is read.

use crate::bars::{analyze_bars, Bars};
use crate::beats::{analyze_long, analyze_short, Analysis, FrameSink, LONG, LSR, SR};
use crate::ff;
use anyhow::{bail, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Pcm {
    pub rate: u32,
    pub data: Vec<i16>, // interleaved stereo
}
impl Pcm {
    pub fn frames(&self) -> usize {
        self.data.len() / 2
    }
}

#[derive(Clone)]
pub struct Song {
    pub path: PathBuf,
    pub name: String,
    pub from: f64,       // only a part of the file: start …
    pub to: Option<f64>, // … and end (seconds in the file)
    pub pcm: Arc<Pcm>,
    pub base: Analysis, // as detected
    pub a: Analysis,    // after ½ / 2× tempo and Earlier / Later
    pub tempo: f64,     // 1, 0.5, 2, …
    pub shift_ms: f64,
    pub bars: Option<Arc<Bars>>,
    /// the song as the analysis heard it: mono, 44.1 kHz (not kept for long mixes)
    pub mono: Option<Arc<Vec<f32>>>,
}

pub fn song_name(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Song".into())
}

fn input_args(c: &mut std::process::Command, path: &Path, from: f64, to: Option<f64>) {
    if from > 0.0 {
        c.arg("-ss").arg(format!("{from:.3}"));
    }
    if let Some(t) = to {
        c.arg("-t").arg(format!("{:.3}", (t - from).max(0.05)));
    }
    c.arg("-i").arg(path);
}

enum Heard {
    Mono(Vec<f32>),
    Frames(FrameSink),
}

/// The analysis stream: like the browser – the song at 44.1 kHz (mixes over 45 min: 22.05 kHz, boiled down to frames
/// right away), channels averaged
fn hear(
    path: &Path,
    from: f64,
    to: Option<f64>,
    long: bool,
    channels: u32,
    cancel: &AtomicBool,
) -> Result<Heard> {
    let mut c = ff::ffmpeg()?;
    input_args(&mut c, path, from, to);
    let rate = if long { LSR } else { SR };
    // (the file's own channels – ffmpeg's up/down-mixing would change the levels)
    let ch = channels.clamp(1, 32) as usize;
    c.args(["-vn", "-sn", "-dn", "-ac"])
        .arg(ch.to_string())
        .arg("-ar")
        .arg(format!("{}", rate as u32))
        .args(["-f", "f32le", "pipe:1"]);
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let mut out = child.stdout.take().unwrap();
    let mut buf = vec![0u8; 1 << 16];
    let mut carry: Vec<u8> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    let mut sink = FrameSink::long();
    let mut piece: Vec<f32> = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("cancelled");
        }
        let n = out.read(&mut buf)?;
        if n == 0 {
            break;
        }
        carry.extend_from_slice(&buf[..n]);
        let fb = 4 * ch;
        let whole = carry.len() / fb * fb;
        piece.clear();
        for fr in carry[..whole].chunks_exact(fb) {
            // (the browser's mixdown: each channel / channels, added up)
            let mut v = 0.0f32;
            for c in fr.as_chunks::<4>().0 {
                v += f32::from_le_bytes([c[0], c[1], c[2], c[3]]) / ch as f32;
            }
            piece.push(v);
        }
        carry.drain(..whole);
        if long {
            sink.push(&piece);
        } else {
            mono.extend_from_slice(&piece);
        }
    }
    let _ = child.wait();
    Ok(if long {
        Heard::Frames(sink)
    } else {
        Heard::Mono(mono)
    })
}

/// Decode and analyse. progress: 0–1; cancel stops it.
pub fn load(
    path: &Path,
    from: f64,
    to: Option<f64>,
    rate: u32,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> Result<Song> {
    let probe = ff::probe(path).unwrap_or_default();
    let total = probe.dur;
    let channels = if probe.channels > 0 {
        probe.channels
    } else {
        2
    };
    let span = match to {
        Some(t) => (t - from).max(0.0),
        None => (total - from).max(0.0),
    };
    let long = span > LONG;
    // the analysis runs alongside the decoding for playing
    let heard = std::thread::scope(|sc| -> Result<(Vec<i16>, Heard)> {
        let h = sc.spawn(|| hear(path, from, to, long, channels, cancel));
        let mut c = ff::ffmpeg()?;
        input_args(&mut c, path, from, to);
        c.args(["-vn", "-sn", "-dn", "-ac", "2", "-ar"])
            .arg(rate.to_string());
        c.args(["-f", "s16le", "pipe:1"]);
        let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let mut out = child.stdout.take().unwrap();
        let err_pipe = child.stderr.take().unwrap();
        let err_thread = std::thread::spawn(move || {
            let mut s = String::new();
            let mut e = err_pipe;
            let _ = e.read_to_string(&mut s);
            s
        });
        let mut data: Vec<i16> =
            Vec::with_capacity(((span.max(1.0) * rate as f64) as usize * 2).min(1 << 31));
        let mut buf = vec![0u8; 1 << 16];
        let mut carry: Vec<u8> = Vec::new();
        let mut last_p = -1.0f32;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                bail!("cancelled");
            }
            let n = out.read(&mut buf)?;
            if n == 0 {
                break;
            }
            carry.extend_from_slice(&buf[..n]);
            let whole = carry.len() / 2 * 2;
            for ch in carry[..whole].as_chunks::<2>().0 {
                data.push(i16::from_le_bytes([ch[0], ch[1]]));
            }
            carry.drain(..whole);
            if span > 0.0 {
                let p = ((data.len() / 2) as f64 / rate as f64 / span).min(1.0) as f32;
                if p - last_p > 0.01 {
                    last_p = p;
                    progress(p * 0.85);
                }
            }
        }
        let status = child.wait()?;
        let err = err_thread.join().unwrap_or_default();
        if data.len() < (rate as usize) {
            if !status.success() || data.is_empty() {
                bail!(
                    "No sound could be read from this file{}",
                    err.lines()
                        .next()
                        .map(|l| format!(" ({l})"))
                        .unwrap_or_default()
                );
            }
            bail!("This song is too short");
        }
        let heard = h
            .join()
            .map_err(|_| anyhow::anyhow!("the analysis stopped"))??;
        Ok((data, heard))
    })?;
    let (data, heard) = heard;
    let pcm = Arc::new(Pcm { rate, data });
    progress(0.9);
    let (a, mono) = match heard {
        Heard::Mono(m) => (analyze_short(&m), Some(Arc::new(m))),
        Heard::Frames(f) => (analyze_long(&f), None),
    };
    if a.beats.len() < 4 {
        bail!("No beats found in this song");
    }
    progress(0.97);
    let mut song = Song {
        path: std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
        name: song_name(path),
        from,
        to,
        pcm,
        base: a.clone(),
        a,
        tempo: 1.0,
        shift_ms: 0.0,
        bars: None,
        mono,
    };
    song.refresh_bars();
    progress(1.0);
    Ok(song)
}

impl Song {
    pub fn refresh_bars(&mut self) {
        self.bars = match (&self.mono, self.a.long) {
            (Some(m), false) => analyze_bars(m, SR, &self.a.beats, &self.a.energy).map(Arc::new),
            _ => None,
        };
    }
    /// ½ tempo / 2× tempo
    pub fn rescale(&mut self, factor: f64) {
        self.a = self.a.rescale(factor);
        self.tempo *= factor;
        self.refresh_bars();
    }
    /// Earlier / Later (ms)
    pub fn nudge(&mut self, ms: f64) {
        self.a = self.a.shift(ms / 1000.0);
        self.shift_ms += ms;
        self.refresh_bars();
    }
    pub fn reset_grid(&mut self) {
        self.a = self.base.clone();
        self.tempo = 1.0;
        self.shift_ms = 0.0;
        self.refresh_bars();
    }
    pub fn duration(&self) -> f64 {
        self.pcm.frames() as f64 / self.pcm.rate as f64
    }
}
