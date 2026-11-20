// Export: a session rendered frame by frame (no dropped frames, whatever the computer) and encoded by ffmpeg
// together with the sound – the song and the clips' own audio, mixed exactly like in the preview.
// The video is written under a temporary name and renamed when it is complete: a cancelled or failed export
// never leaves a broken file behind, nor replaces an older one.

use crate::clipaudio::{self, soft_limit, VoiceSpec};
use crate::enc;
use crate::ff;
use crate::media::Pick;
use crate::project::Session;
use crate::show::{self, BuildOpts, Pins};
use crate::util::LockExt;
use anyhow::{bail, Context, Result};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

#[derive(Clone, Default)]
pub struct Progress {
    pub frac: f32,
    pub msg: String,
    pub eta: Option<f64>,
    pub fails: Vec<String>,
    /// the encoder used (and a note if the chosen one wasn't possible)
    pub encoder: String,
}

/// What to render
#[derive(Clone)]
pub struct Job {
    pub out: PathBuf,
    pub pins: Pins,
    /// only a part (seconds of the show): from … to
    pub range: Option<(f64, f64)>,
}

fn set(prog: &Arc<Mutex<Progress>>, f: f32, m: &str, eta: Option<f64>) {
    let mut p = prog.lk();
    p.frac = f.clamp(0.0, 1.0);
    p.msg = m.to_string();
    p.eta = eta;
}

/// The file name a session's video gets (the codec decides the extension)
pub fn file_name(title: &str, codec: &str) -> String {
    format!(
        "{}.{}",
        crate::util::safe_file_name(&format!("PMV - {title}"), 100),
        enc::extension(codec)
    )
}

/// Render a session to a video file. Returns the file written (its extension fits the codec).
pub fn run(
    sess: &Session,
    job: &Job,
    prog: &Arc<Mutex<Progress>>,
    cancel: &Arc<AtomicBool>,
) -> Result<PathBuf> {
    ff::available()?;
    let s = &sess.settings;
    let out = job
        .out
        .with_extension(enc::extension(&s.codec))
        .to_path_buf();
    let dir = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if !dir.is_dir() {
        bail!("The folder {} doesn't exist", dir.display());
    }
    let stem = out
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "video".into());
    let tmp_video = dir.join(format!(".{stem}.partial.{}", enc::extension(&s.codec)));
    let tmp_audio = dir.join(format!(".{stem}.sound.wav"));
    // (whatever happens below: the temporary files go)
    struct Cleanup(Vec<PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    let _cleanup = Cleanup(vec![tmp_video.clone(), tmp_audio.clone()]);

    // 1. Every shot needs its clip (a session from "Generate" or a project has them all)
    if !sess.complete() {
        bail!("Some shots have no clip yet – generate the PMV first");
    }
    let (t_from, t_to) = match job.range {
        Some((a, b)) => (
            a.clamp(0.0, sess.plan.duration),
            b.clamp(0.0, sess.plan.duration),
        ),
        None => (0.0, sess.plan.duration),
    };
    if t_to - t_from < 0.1 {
        bail!("Nothing to render");
    }

    // 2. The sound: song + clip audio
    set(prog, 0.0, "Mixing the sound …", None);
    write_sound(sess, &tmp_audio, t_from, t_to, prog, cancel)?;
    if cancel.load(Ordering::SeqCst) {
        bail!("Cancelled");
    }

    // 3. The picture, encoded
    let (enc_name, note) = enc::choose(&s.codec, &s.encoder)
        .ok_or_else(|| anyhow::anyhow!("This ffmpeg has no encoder for {}", s.codec))?;
    let mut tries = vec![enc_name.clone()];
    if enc::is_hw(&enc_name) {
        // (a graphics card can refuse a size or a format only when it starts – then the processor does it)
        if let Some((cpu, _)) = enc::choose(&s.codec, "cpu") {
            tries.push(cpu);
        }
    }
    let mut last_err = None;
    for (attempt, name) in tries.iter().enumerate() {
        prog.lk().encoder = match (&note, attempt) {
            (Some(n), 0) => format!("{name} ({n})"),
            (_, a) if a > 0 => format!("{name} (the graphics card encoder failed)"),
            _ => name.clone(),
        };
        match render_video(
            sess,
            job,
            name,
            &tmp_audio,
            &tmp_video,
            (t_from, t_to),
            prog,
            cancel,
        ) {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(RenderErr::Early(e)) if attempt + 1 < tries.len() => {
                eprintln!(
                    "[pmvgen] encoder {name} failed at the start ({e}) – trying {}",
                    tries[attempt + 1]
                );
                last_err = Some(e);
            }
            Err(RenderErr::Early(e)) | Err(RenderErr::Late(e)) => {
                last_err = Some(e);
                break;
            }
        }
    }
    if cancel.load(Ordering::SeqCst) {
        bail!("Cancelled");
    }
    if let Some(e) = last_err {
        return Err(e);
    }
    std::fs::rename(&tmp_video, &out)
        .or_else(|_| std::fs::copy(&tmp_video, &out).map(|_| ()))
        .with_context(|| format!("saving {}", out.display()))?;

    // 4. Funscript next to the video
    if s.fs_on {
        let song = &sess.song;
        if let Ok(fs) = crate::funscript::build(
            &song.a.beats,
            &song.a.energy,
            song.a.bpm,
            sess.plan.duration,
            song.bars.as_deref(),
            s,
            &song.name,
        ) {
            let _ = crate::util::write_atomic(
                &out.with_extension("funscript"),
                &serde_json::to_vec(&fs).unwrap_or_default(),
            );
        }
    }
    set(prog, 1.0, "Done", None);
    Ok(out)
}

enum RenderErr {
    /// before the first frames were taken – another encoder may work
    Early(anyhow::Error),
    Late(anyhow::Error),
}

#[allow(clippy::too_many_arguments)]
fn render_video(
    sess: &Session,
    job: &Job,
    enc_name: &str,
    audio: &Path,
    tmp_video: &Path,
    (t_from, t_to): (f64, f64),
    prog: &Arc<Mutex<Progress>>,
    cancel: &Arc<AtomicBool>,
) -> std::result::Result<(), RenderErr> {
    let s = sess.settings.clone();
    let fps = s.export_fps.max(1.0);
    let mut built = show::build(
        sess,
        BuildOpts {
            short: s.quality,
            export: true,
            fps: fps.max(24.0),
            pins: job.pins.clone(),
            items: Vec::new(),
        },
    );
    let plan = built.plan.clone();
    let media = built.media.clone();
    let (w, h) = (plan.w, plan.h);
    let first = (t_from * fps).round() as usize;
    let last = ((t_to * fps).ceil() as usize).max(first + 1);
    let total = last - first;
    let ea = enc::args(enc_name, &s.enc_speed, s.export_crf, w as u32, h as u32);
    let early = |e: anyhow::Error| RenderErr::Early(e);
    let mut c = ff::ffmpeg().map_err(early)?;
    c.args(["-y"]);
    c.args(&ea.pre_input);
    c.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
        .arg(format!("{w}x{h}"));
    c.arg("-r").arg(format!("{fps}")).args(["-i", "pipe:0"]);
    c.arg("-i").arg(audio);
    c.args(["-map", "0:v:0", "-map", "1:a:0"]);
    if let Some(f) = &ea.filter {
        c.arg("-vf").arg(f);
    }
    c.arg("-c:v").arg(enc_name);
    c.args(&ea.out);
    c.args(enc::audio_args(&s.codec));
    c.args(["-shortest"]);
    c.args(enc::container_args(&s.codec));
    c.arg(tmp_video);
    c.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = c.spawn().context("starting ffmpeg").map_err(early)?;
    let (Some(mut stdin), Some(mut errp)) = (child.stdin.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(early(anyhow::anyhow!("ffmpeg has no pipes")));
    };
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = errp.read_to_string(&mut s);
        s
    });

    // (preparing the first clips: from the first frame on)
    media.seek(first as f64 / fps);
    let t0 = Instant::now();
    let prof = std::env::var_os("PMVGEN_PROF").is_some();
    let (mut tt, mut tf, mut tw) = (0.0, 0.0, 0.0);
    let mut write_err: Option<(usize, std::io::Error)> = None;
    for i in 0..total {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let t = (first + i) as f64 / fps;
        let a = Instant::now();
        media.tick(t);
        let b = Instant::now();
        let frame = built.renderer.frame(t);
        let c2 = Instant::now();
        if let Err(e) = stdin.write_all(frame.data()) {
            write_err = Some((i, e));
            break;
        }
        if prof {
            tt += (b - a).as_secs_f64();
            tf += (c2 - b).as_secs_f64();
            tw += c2.elapsed().as_secs_f64();
        }
        if i % 10 == 0 {
            let f = (i + 1) as f32 / total as f32;
            let el = t0.elapsed().as_secs_f64();
            let eta = (i > 30).then(|| el / f as f64 - el);
            set(
                prog,
                f * 0.99,
                &format!(
                    "Rendering {} / {}",
                    crate::util::fmt_time(t - t_from),
                    crate::util::fmt_time(t_to - t_from)
                ),
                eta,
            );
            let fails: Vec<String> = media
                .failures
                .lk()
                .iter()
                .map(|f| format!("{}: {}", f.name, f.why))
                .collect();
            prog.lk().fails = fails;
        }
    }
    drop(stdin);
    if prof {
        eprintln!(
            "[prof] tick {tt:.1} s, frames {tf:.1} s, write {tw:.1} s, draw {:.1} s",
            built.renderer.draw_time
        );
    }
    media.stop();
    let status = child.wait();
    let err = err_t.join().unwrap_or_default();
    if cancel.load(Ordering::SeqCst) {
        return Err(RenderErr::Late(anyhow::anyhow!("Cancelled")));
    }
    let ok = status.as_ref().map(|s| s.success()).unwrap_or(false);
    if !ok || write_err.is_some() {
        let last = err
            .lines()
            .rfind(|l| !l.trim().is_empty())
            .unwrap_or("")
            .to_string();
        let e = anyhow::anyhow!(
            "The video couldn't be written{}",
            if last.is_empty() {
                String::new()
            } else {
                format!(": {last}")
            }
        );
        // (failing within the first frames = the encoder refused – another one may work)
        return Err(match write_err {
            Some((i, _)) if i < 10 => RenderErr::Early(e),
            None if !ok => RenderErr::Early(e),
            _ => RenderErr::Late(e),
        });
    }
    Ok(())
}

/// Write the show's sound to a WAV file (stereo float at the song's rate): the song at its volume plus the
/// clips' audio at theirs. Streamed in one-second pieces – clip voices are rendered by a few workers just
/// ahead of where the writing is, so memory stays small also for hour-long mixes.
pub fn write_sound(
    sess: &Session,
    path: &Path,
    t_from: f64,
    t_to: f64,
    prog: &Arc<Mutex<Progress>>,
    cancel: &AtomicBool,
) -> Result<()> {
    let s = &sess.settings;
    let pcm = &sess.song.pcm;
    let rate = pcm.rate;
    let song_vol = (s.song_vol / 100.0).clamp(0.0, 1.0) as f32;
    let clip_vol = (s.clip_vol / 100.0).clamp(0.0, 1.0) as f32;
    let first = (t_from * rate as f64).round() as u64;
    let last = ((t_to * rate as f64).round() as u64).max(first + 1);
    let frames = last - first;
    let plan = sess.plan.clone();
    let mut specs: Vec<VoiceSpec> = if clip_vol > 0.0 {
        clipaudio::specs(&plan, s, Some(&sess.picks))
    } else {
        Vec::new()
    };
    specs.retain(|v| v.t1 > t_from && v.t0 < t_to);
    specs.sort_by(|a, b| a.t0.total_cmp(&b.t0));
    let specs = Arc::new(specs);
    let picks: Arc<Vec<Option<Pick>>> = Arc::new(sess.picks.clone());

    // workers: render voices in order, at most AHEAD past the one being written
    const AHEAD: usize = 24;
    let results: Arc<Vec<Mutex<Option<Option<clipaudio::Voice>>>>> =
        Arc::new((0..specs.len()).map(|_| Mutex::new(None)).collect());
    let next = Arc::new(AtomicUsize::new(0));
    let consumed = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let cv = Arc::new((Mutex::new(()), Condvar::new()));
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 6);
    let mut handles = Vec::new();
    for _ in 0..workers.min(specs.len().max(1)) {
        let (specs, picks, plan, results, next, consumed, stop, cv) = (
            specs.clone(),
            picks.clone(),
            plan.clone(),
            results.clone(),
            next.clone(),
            consumed.clone(),
            stop.clone(),
            cv.clone(),
        );
        handles.push(std::thread::spawn(move || loop {
            let i = next.fetch_add(1, Ordering::SeqCst);
            if i >= specs.len() {
                return;
            }
            // don't run too far ahead of the writer
            while i > consumed.load(Ordering::SeqCst) + AHEAD && !stop.load(Ordering::SeqCst) {
                let g = cv.0.lk();
                drop(crate::util::wait_timeout(
                    &cv.1,
                    g,
                    std::time::Duration::from_millis(50),
                ));
            }
            if stop.load(Ordering::SeqCst) {
                return;
            }
            let v = picks
                .get(specs[i].shot)
                .and_then(|p| p.as_ref())
                .and_then(|p| clipaudio::render(&specs[i], p, &plan, rate));
            *results[i].lk() = Some(v);
            let _g = cv.0.lk();
            cv.1.notify_all();
        }));
    }
    let result = (|| -> Result<()> {
        let f =
            std::fs::File::create(path).with_context(|| format!("writing {}", path.display()))?;
        let mut wr = BufWriter::with_capacity(1 << 20, f);
        write_wav_header(&mut wr, rate, frames)?;
        let mut active: Vec<clipaudio::Voice> = Vec::new();
        let mut taken = 0usize; // specs moved into `active`
        let chunk = rate as u64;
        let mut pos = first;
        let mut buf: Vec<f32> = Vec::with_capacity(chunk as usize * 2);
        let mut bytes: Vec<u8> = Vec::with_capacity(chunk as usize * 8);
        while pos < last {
            if cancel.load(Ordering::SeqCst) {
                bail!("Cancelled");
            }
            let n = chunk.min(last - pos);
            let end_t = (pos + n) as f64 / rate as f64;
            // the voices that start before this piece ends
            while taken < specs.len() && specs[taken].t0 < end_t {
                let v = loop {
                    if let Some(v) = results[taken].lk().take() {
                        break v;
                    }
                    let g = cv.0.lk();
                    drop(crate::util::wait_timeout(
                        &cv.1,
                        g,
                        std::time::Duration::from_millis(50),
                    ));
                    if cancel.load(Ordering::SeqCst) {
                        bail!("Cancelled");
                    }
                };
                if let Some(v) = v {
                    active.push(v);
                }
                taken += 1;
                consumed.store(taken, Ordering::SeqCst);
                let _g = cv.0.lk();
                cv.1.notify_all();
            }
            buf.clear();
            buf.resize(n as usize * 2, 0.0);
            // song
            let total = pcm.frames() as u64;
            for k in 0..n {
                let f = pos + k;
                if f < total {
                    let i = (f * 2) as usize;
                    buf[2 * k as usize] = pcm.data[i] as f32 / 32768.0 * song_vol;
                    buf[2 * k as usize + 1] = pcm.data[i + 1] as f32 / 32768.0 * song_vol;
                }
            }
            // clips
            for v in &active {
                let (a, b) = (v.start.max(pos), (v.start + v.frames()).min(pos + n));
                for f in a..b {
                    let si = ((f - v.start) * 2) as usize;
                    let oi = ((f - pos) * 2) as usize;
                    buf[oi] += v.data[si] * clip_vol;
                    buf[oi + 1] += v.data[si + 1] * clip_vol;
                }
            }
            active.retain(|v| v.start + v.frames() > pos + n);
            bytes.clear();
            for x in &buf {
                bytes.extend_from_slice(&soft_limit(*x).to_le_bytes());
            }
            wr.write_all(&bytes)?;
            pos += n;
            set(
                prog,
                0.0,
                &format!(
                    "Mixing the sound … {} %",
                    ((pos - first) as f64 / frames as f64 * 100.0).round()
                ),
                None,
            );
        }
        wr.flush()?;
        Ok(())
    })();
    stop.store(true, Ordering::SeqCst);
    {
        let _g = cv.0.lk();
        cv.1.notify_all();
    }
    for h in handles {
        let _ = h.join();
    }
    result
}

fn write_wav_header(w: &mut impl Write, rate: u32, frames: u64) -> Result<()> {
    let data = frames * 2 * 4;
    // (over 4 GB a WAV can't say its size: it says the most it can, ffmpeg reads on to the end)
    let data32 = data.min(u32::MAX as u64 - 50) as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(data32 + 50).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&18u32.to_le_bytes())?;
    w.write_all(&3u16.to_le_bytes())?; // IEEE float
    w.write_all(&2u16.to_le_bytes())?;
    w.write_all(&rate.to_le_bytes())?;
    w.write_all(&(rate * 8).to_le_bytes())?;
    w.write_all(&8u16.to_le_bytes())?;
    w.write_all(&32u16.to_le_bytes())?;
    w.write_all(&0u16.to_le_bytes())?;
    w.write_all(b"fact")?;
    w.write_all(&4u32.to_le_bytes())?;
    w.write_all(&(frames.min(u32::MAX as u64) as u32).to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data32.to_le_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wav_header_is_58_bytes() {
        let mut v = Vec::new();
        write_wav_header(&mut v, 48000, 100).unwrap();
        assert_eq!(v.len(), 58);
        assert_eq!(&v[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(v[4..8].try_into().unwrap()), 800 + 50);
        assert_eq!(u32::from_le_bytes(v[54..58].try_into().unwrap()), 800);
        assert_eq!(file_name("a/b", "web"), "PMV - a_b.webm");
        assert_eq!(file_name("x", "h264"), "PMV - x.mp4");
    }
}
