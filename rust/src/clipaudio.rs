// Clip audio: the clips' own sound mixed in under the song – "only on drops" (the main clip fades in for four
// beats on every drop, like the voice-overs of real PMVs) or "always" (every clip on screen, together about as
// loud as the clip volume). Ports voice() / syncVoices() of the web version.
//
// The sound follows the picture exactly: for every moment of the show the clip's position comes from the same
// clock as its frames (speed ramps, stutter, motion on the beat, the loop near its end). Where the clip plays
// faster or slower the sound is time-stretched (WSOLA) so voices keep their pitch – the browser does the same.

use crate::ff;
use crate::library::Kind;
use crate::media::Pick;
use crate::plan::Plan;
use crate::settings::Settings;
use crate::util::LockExt;
use std::io::Read;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// One stretch of a clip's sound
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSpec {
    pub shot: usize,
    pub t0: f64,
    pub t1: f64,
    pub kind: VoiceKind,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceKind {
    /// a drop: in over 50 ms, held, out over the last 0.2 s of `dur` (four beats)
    Drop { dur: f64 },
    /// always on while the clip is on screen, at this share of the clip volume
    Always { level: f64 },
}

/// The stretches of clip sound a plan has. picks: known clips (an image has no sound; "always" shares the
/// volume between the videos on screen)
pub fn specs(plan: &Plan, s: &Settings, picks: Option<&[Option<Pick>]>) -> Vec<VoiceSpec> {
    if !s.fx.voice {
        return Vec::new();
    }
    let is_video = |shot: usize| -> bool {
        picks
            .and_then(|p| p.get(shot))
            .and_then(|x| x.as_ref())
            .is_none_or(|x| x.kind == Kind::Video)
    };
    let mut out = Vec::new();
    if s.voice_mode == "always" {
        for sh in &plan.shots {
            if !is_video(sh.id) || sh.t_off <= sh.t_on {
                continue;
            }
            // the videos on screen when it comes on
            let (_, cur, _) = plan.shots_at(sh.t_on);
            let n = cur
                .iter()
                .flatten()
                .filter(|&&i| is_video(i))
                .count()
                .max(1);
            out.push(VoiceSpec {
                shot: sh.id,
                t0: sh.t_on,
                t1: sh.t_off.min(plan.duration),
                kind: VoiceKind::Always {
                    level: 1.0 / (n as f64).sqrt(),
                },
            });
        }
    } else {
        for &(t, len) in &plan.drops {
            // the clip in the largest field right after the drop's cuts
            let pi = plan.phase_at(t);
            let ph = &plan.phases[pi];
            let Some(big) = ph.slots.iter().max_by_key(|s| s.w as i64 * s.h as i64) else {
                continue;
            };
            let Some(shot) = plan.shot_in(big.g, t) else {
                continue;
            };
            if !is_video(shot) {
                continue;
            }
            let dur = (len * 4.0).max(0.3);
            let end = (t + dur).min(plan.shots[shot].t_off).min(plan.duration);
            if end - t > 0.05 {
                out.push(VoiceSpec {
                    shot,
                    t0: t,
                    t1: end,
                    kind: VoiceKind::Drop { dur },
                });
            }
        }
    }
    out
}

/// Rendered clip sound: stereo f32 at the song's rate, from `start` (frames of the show)
#[derive(Debug)]
pub struct Voice {
    pub shot: usize,
    pub start: u64,
    pub data: Vec<f32>,
}
impl Voice {
    pub fn frames(&self) -> u64 {
        (self.data.len() / 2) as u64
    }
}

/// Decode a stretch of a file's sound: stereo f32 at `rate` from `from` seconds, `secs` long
fn decode(path: &std::path::Path, from: f64, secs: f64, rate: u32) -> Vec<f32> {
    let Ok(mut c) = ff::ffmpeg() else {
        return Vec::new();
    };
    c.arg("-ss").arg(format!("{:.4}", from.max(0.0)));
    c.arg("-t").arg(format!("{:.4}", secs.max(0.05)));
    c.arg("-i").arg(path);
    c.args(["-vn", "-sn", "-dn", "-ac", "2", "-ar"])
        .arg(rate.to_string());
    c.args(["-f", "f32le", "pipe:1"]);
    let Ok(mut ch) = c.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if let Some(mut o) = ch.stdout.take() {
        let _ = o.read_to_end(&mut bytes);
    }
    let _ = ch.wait();
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| {
            let v = f32::from_le_bytes(*b);
            if v.is_finite() {
                v.clamp(-4.0, 4.0)
            } else {
                0.0
            }
        })
        .collect()
}

const N: usize = 1024; // WSOLA frame (~21 ms at 48 kHz)
const HOP: usize = N / 2;
const SEARCH: i64 = 256;

/// Render a stretch of clip sound. None: no sound (an image, a video without audio, nothing to play)
pub fn render(spec: &VoiceSpec, pick: &Pick, plan: &Plan, rate: u32) -> Option<Voice> {
    if pick.kind != Kind::Video || spec.t1 <= spec.t0 {
        return None;
    }
    let shot = plan.shots.get(spec.shot)?;
    let r = rate as f64;
    let frames = ((spec.t1 - spec.t0) * r).round() as usize;
    if frames < 16 {
        return None;
    }
    let ft = |t: f64| pick.file_time(plan, shot, t);
    // the part of the file the stretch plays
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut t = spec.t0;
    while t <= spec.t1 + 0.011 {
        let x = ft(t.min(spec.t1));
        lo = lo.min(x);
        hi = hi.max(x);
        t += 0.01;
    }
    let from = (lo - 0.1).max(0.0);
    let src = decode(&pick.path, from, hi - from + 0.2, rate);
    let src_frames = src.len() / 2;
    if src_frames < N {
        return None;
    }
    // WSOLA: frames of the output (hop HOP, Hann window – overlapping windows add up to 1) are taken from where
    // the clock says the clip is, shifted a little (± SEARCH) to continue the waveform of the frame before
    let window: Vec<f32> = (0..N)
        .map(|n| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / N as f32).cos())
        .collect();
    let mut out = vec![0f32; frames * 2];
    let mut nat: Option<i64> = None; // where the previous frame's natural continuation starts (src frames)
    let max_x = (src_frames - N) as i64;
    let mono = |i: i64| -> f32 {
        let i = i.clamp(0, src_frames as i64 - 1) as usize;
        src[2 * i] + src[2 * i + 1]
    };
    // frame m covers output samples o..o+N (o = (m−1)·HOP: one frame before the start, so every sample has
    // two windows on it); output sample i is show time t0 + i/rate
    let mut m = 0usize;
    while m * HOP < frames + HOP {
        let o = m as i64 * HOP as i64 - HOP as i64;
        let tc = spec.t0 + (o + (N / 2) as i64) as f64 / r;
        let want = ((ft(tc.clamp(spec.t0, spec.t1)) - from) * r).round() as i64 - (N / 2) as i64;
        let want = want.clamp(0, max_x);
        let x = match nat {
            // plays on at normal speed: just continue
            Some(n) if (want - n).abs() <= 1 => n.clamp(0, max_x),
            // faster / slower: the spot near the wanted one that fits the continuation best
            Some(n) if (want - n).abs() < 2 * N as i64 && n <= max_x => {
                let score = |x: i64| -> f32 {
                    let mut s = 0.0f32;
                    let mut k = 0;
                    while k < HOP {
                        s += mono(x + k as i64) * mono(n + k as i64);
                        k += 4;
                    }
                    s
                };
                let (lo_x, hi_x) = ((want - SEARCH).max(0), (want + SEARCH).min(max_x));
                let mut best = want;
                let mut bs = f32::NEG_INFINITY;
                let mut x = lo_x;
                while x <= hi_x {
                    let s = score(x);
                    if s > bs {
                        bs = s;
                        best = x;
                    }
                    x += 2;
                }
                for x in [best - 1, best + 1] {
                    if x >= lo_x && x <= hi_x && score(x) > bs {
                        bs = score(x);
                        best = x;
                    }
                }
                best
            }
            // a jump (stutter, loop, the start): from there, the overlap crossfades
            _ => want,
        };
        for (k, w) in window.iter().enumerate() {
            let oi = o + k as i64;
            if oi < 0 {
                continue;
            }
            let oi = oi as usize;
            if oi >= frames {
                break;
            }
            let si = (x as usize + k).min(src_frames - 1);
            out[2 * oi] += src[2 * si] * w;
            out[2 * oi + 1] += src[2 * si + 1] * w;
        }
        nat = Some(x + HOP as i64);
        m += 1;
    }
    let mut data = out;
    // envelope
    let cut_out = (spec.t1 - shot.t_off).abs() < 1e-6 || (spec.t1 - plan.duration).abs() < 1e-6;
    for (i, fr) in data.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let tt = i as f64 / r;
        let left = (frames - i) as f64 / r;
        let mut g = match spec.kind {
            VoiceKind::Drop { dur } => {
                let a = (tt / 0.05).min(1.0);
                let rel = (dur - tt).max(0.0);
                a * (rel / 0.2).min(1.0)
            }
            VoiceKind::Always { level } => level * (1.0 - (-tt / 0.06).exp()),
        };
        if cut_out || matches!(spec.kind, VoiceKind::Always { .. }) {
            g *= (left / 0.015).min(1.0);
        }
        fr[0] *= g as f32;
        fr[1] *= g as f32;
    }
    Some(Voice {
        shot: spec.shot,
        start: (spec.t0 * r).round() as u64,
        data,
    })
}

/// The clip voices playing right now – mixed into the song by the player (and the export)
pub struct ClipMix {
    voices: Mutex<Vec<Arc<Voice>>>,
    vol: AtomicU32,
}
impl ClipMix {
    pub fn new(vol: f32) -> Arc<ClipMix> {
        Arc::new(ClipMix {
            voices: Mutex::new(Vec::new()),
            vol: AtomicU32::new(vol.to_bits()),
        })
    }
    pub fn set_volume(&self, v: f32) {
        self.vol
            .store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.vol.load(Ordering::Relaxed))
    }
    /// A rendered voice (one of the same shot before it is replaced)
    pub fn add(&self, v: Voice) {
        let mut l = self.voices.lk();
        l.retain(|x| x.shot != v.shot);
        l.push(Arc::new(v));
    }
    pub fn clear(&self) {
        self.voices.lk().clear();
    }
    /// Forget voices that ended before `frame`
    pub fn prune(&self, frame: u64) {
        self.voices.lk().retain(|v| v.start + v.frames() > frame);
    }
    /// Add the voices to stereo samples `out` that start at show frame `at` (realtime: never waits for the
    /// lock – a block without clip sound is better than a stutter)
    pub fn mix_into(&self, out: &mut [f32], at: u64, realtime: bool) {
        let g = if realtime {
            match self.voices.try_lock() {
                Ok(g) => g,
                Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => return,
            }
        } else {
            self.voices.lk()
        };
        let vol = self.volume();
        if vol <= 0.0 {
            return;
        }
        let n = (out.len() / 2) as u64;
        for v in g.iter() {
            let (a, b) = (v.start.max(at), (v.start + v.frames()).min(at + n));
            if a >= b {
                continue;
            }
            for f in a..b {
                let si = ((f - v.start) * 2) as usize;
                let oi = ((f - at) * 2) as usize;
                out[oi] += v.data[si] * vol;
                out[oi + 1] += v.data[si + 1] * vol;
            }
        }
    }
}

/// Keeps a sum in range without a hard clip: linear up to 0.9, then a soft knee to 1
pub fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.9 {
        x
    } else {
        let over = a - 0.9;
        (0.9 + 0.1 * (over / 0.1).tanh()).copysign(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{make, Input};

    fn plan() -> Plan {
        let beats: Vec<f64> = (0..200).map(|i| i as f64 * 0.5).collect();
        let energy: Vec<f64> = (0..200)
            .map(|i| if (i / 32) % 2 == 0 { 0.2 } else { 0.9 })
            .collect();
        let st = Settings::default();
        make(&Input {
            beats: &beats,
            energy: &energy,
            duration: 100.0,
            bars: None,
            s: &st,
            w: 1280,
            h: 720,
            n_images: 0,
            layout_ok: &|_| true,
            seed: 3,
            template: None,
        })
    }

    #[test]
    fn drop_and_always_specs() {
        let p = plan();
        let mut s = Settings::default();
        s.fx.voice = true;
        s.voice_mode = "drops".into();
        let d = specs(&p, &s, None);
        assert!(!d.is_empty());
        for v in &d {
            assert!(v.t1 > v.t0 && v.t1 <= p.shots[v.shot].t_off + 1e-9);
            assert!(matches!(v.kind, VoiceKind::Drop { .. }));
            assert!(p.shots[v.shot].t_on <= v.t0 + 1e-9);
        }
        s.voice_mode = "always".into();
        let a = specs(&p, &s, None);
        assert_eq!(a.len(), p.shots.iter().filter(|x| x.t_off > x.t_on).count());
        s.fx.voice = false;
        assert!(specs(&p, &s, None).is_empty());
    }

    #[test]
    fn mix_adds_and_prunes() {
        let m = ClipMix::new(0.5);
        m.add(Voice {
            shot: 1,
            start: 10,
            data: vec![1.0; 20],
        });
        let mut out = vec![0f32; 40];
        m.mix_into(&mut out, 0, false);
        assert_eq!(out[19], 0.0);
        assert_eq!(out[20], 0.5);
        assert_eq!(out[39], 0.5); // (frames 10..19)
        let mut out2 = vec![0f32; 40];
        m.mix_into(&mut out2, 5, false);
        assert_eq!(out2[10], 0.5);
        m.prune(100);
        let mut out3 = vec![0f32; 40];
        m.mix_into(&mut out3, 0, false);
        assert!(out3.iter().all(|v| *v == 0.0));
        assert!((soft_limit(5.0) - 1.0).abs() < 1e-3);
        assert_eq!(soft_limit(0.5), 0.5);
        assert!(soft_limit(-0.95) < -0.9);
    }
}
