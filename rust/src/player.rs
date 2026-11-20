// The song plays through the sound card (cpal); its position is the show's clock.
// Without a sound device the show still runs on a silent clock.

use crate::clipaudio::{soft_limit, ClipMix};
use crate::song::Pcm;
use crate::util::LockExt;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Shared {
    pcm: Arc<Pcm>,
    /// the clips' own sound (clip audio), mixed under the song
    mix: Option<Arc<ClipMix>>,
    pos: AtomicU64, // frames played
    playing: AtomicBool,
    vol: AtomicU32,
    cb: Mutex<(Instant, u64)>, // when the last block was handed over, and the position then
}

pub struct Player {
    _stream: Option<cpal::Stream>,
    sh: Arc<Shared>,
    // silent clock (no sound device)
    silent: Option<Mutex<(Option<Instant>, f64)>>,
    pub device_error: Option<String>,
}

/// The sample rate the sound card wants – or `fallback` without one (rendering on a machine without sound)
pub fn device_rate_or(fallback: u32) -> u32 {
    cpal::default_host()
        .default_output_device()
        .and_then(|d| d.default_output_config().ok())
        .map(|c| c.sample_rate().0)
        .filter(|&r| (8000..=384_000).contains(&r))
        .unwrap_or(fallback)
}

/// The sample rate the sound card wants (the song is decoded at it)
pub fn device_rate() -> u32 {
    cpal::default_host()
        .default_output_device()
        .and_then(|d| d.default_output_config().ok())
        .map(|c| c.sample_rate().0)
        .filter(|&r| (8000..=384_000).contains(&r))
        .unwrap_or(44100)
}

fn build<T: SizedSample + FromSample<f32>>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    sh: Arc<Shared>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let ch = cfg.channels as usize;
    // (a buffer for the clip sound, kept between calls – no allocation while playing once it is big enough)
    let mut clip: Vec<f32> = Vec::with_capacity(8192);
    dev.build_output_stream(
        cfg,
        move |out: &mut [T], _| {
            let frames = out.len() / ch.max(1);
            let playing = sh.playing.load(Ordering::Relaxed);
            let vol = f32::from_bits(sh.vol.load(Ordering::Relaxed));
            let start = sh.pos.load(Ordering::Relaxed);
            let mut pos = start as usize;
            let data = &sh.pcm.data;
            let total = data.len() / 2;
            clip.clear();
            clip.resize(frames * 2, 0.0);
            if playing {
                if let Some(m) = &sh.mix {
                    m.mix_into(&mut clip, start, true);
                }
            }
            for f in 0..frames {
                let (l, r) = if playing && pos < total {
                    let v = (
                        soft_limit(data[2 * pos] as f32 / 32768.0 * vol + clip[2 * f]),
                        soft_limit(data[2 * pos + 1] as f32 / 32768.0 * vol + clip[2 * f + 1]),
                    );
                    pos += 1;
                    v
                } else {
                    (0.0, 0.0)
                };
                for c in 0..ch {
                    let v = match (ch, c) {
                        (1, _) => (l + r) / 2.0,
                        (_, 0) => l,
                        (_, 1) => r,
                        _ => 0.0,
                    };
                    out[f * ch + c] = T::from_sample(v);
                }
            }
            if playing {
                sh.pos.store(pos as u64, Ordering::Relaxed);
                if let Ok(mut g) = sh.cb.lock() {
                    *g = (Instant::now(), pos as u64);
                }
            }
        },
        |e| eprintln!("[pmvgen] sound: {e}"),
        None,
    )
}

impl Player {
    pub fn new(pcm: Arc<Pcm>, volume: f32, mix: Option<Arc<ClipMix>>) -> Player {
        let sh = Arc::new(Shared {
            pcm,
            mix,
            pos: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            vol: AtomicU32::new(volume.to_bits()),
            cb: Mutex::new((Instant::now(), 0)),
        });
        let host = cpal::default_host();
        let res = (|| -> Result<cpal::Stream, String> {
            let dev = host.default_output_device().ok_or("no sound device")?;
            let sup = dev.default_output_config().map_err(|e| e.to_string())?;
            let mut cfg: cpal::StreamConfig = sup.config();
            cfg.sample_rate = cpal::SampleRate(sh.pcm.rate);
            let s = match sup.sample_format() {
                cpal::SampleFormat::F32 => build::<f32>(&dev, &cfg, sh.clone()),
                cpal::SampleFormat::I16 => build::<i16>(&dev, &cfg, sh.clone()),
                cpal::SampleFormat::U16 => build::<u16>(&dev, &cfg, sh.clone()),
                cpal::SampleFormat::I32 => build::<i32>(&dev, &cfg, sh.clone()),
                cpal::SampleFormat::F64 => build::<f64>(&dev, &cfg, sh.clone()),
                f => return Err(format!("unsupported sample format {f:?}")),
            }
            .map_err(|e| e.to_string())?;
            s.play().map_err(|e| e.to_string())?;
            Ok(s)
        })();
        match res {
            Ok(s) => Player {
                _stream: Some(s),
                sh,
                silent: None,
                device_error: None,
            },
            Err(e) => Player {
                _stream: None,
                sh,
                silent: Some(Mutex::new((None, 0.0))),
                device_error: Some(e),
            },
        }
    }

    pub fn duration(&self) -> f64 {
        self.sh.pcm.frames() as f64 / self.sh.pcm.rate as f64
    }

    /// Song position in seconds (smoothed between the sound card's blocks)
    pub fn time(&self) -> f64 {
        if let Some(s) = &self.silent {
            let g = s.lk();
            return match g.0 {
                Some(t0) => g.1 + t0.elapsed().as_secs_f64(),
                None => g.1,
            };
        }
        let rate = self.sh.pcm.rate as f64;
        if !self.sh.playing.load(Ordering::Relaxed) {
            return self.sh.pos.load(Ordering::Relaxed) as f64 / rate;
        }
        let (at, pos) = *self.sh.cb.lk();
        // (the block handed over last is heard over the next ~blocks; never run past 50 ms ahead)
        pos as f64 / rate + at.elapsed().as_secs_f64().min(0.05)
    }

    pub fn playing(&self) -> bool {
        match &self.silent {
            Some(s) => s.lk().0.is_some(),
            None => self.sh.playing.load(Ordering::Relaxed),
        }
    }

    pub fn play(&self) {
        if let Some(s) = &self.silent {
            let mut g = s.lk();
            if g.0.is_none() {
                g.0 = Some(Instant::now());
            }
            return;
        }
        *self.sh.cb.lk() = (Instant::now(), self.sh.pos.load(Ordering::Relaxed));
        self.sh.playing.store(true, Ordering::Relaxed);
    }
    pub fn pause(&self) {
        if let Some(s) = &self.silent {
            let mut g = s.lk();
            if let Some(t0) = g.0.take() {
                g.1 += t0.elapsed().as_secs_f64();
            }
            return;
        }
        let t = self.time();
        self.sh.playing.store(false, Ordering::Relaxed);
        self.sh
            .pos
            .store((t * self.sh.pcm.rate as f64) as u64, Ordering::Relaxed);
    }
    pub fn ended(&self) -> bool {
        self.time() >= self.duration() - 0.01
    }

    /// Jump to t seconds (playing or paused)
    pub fn seek(&self, t: f64) {
        let t = t.clamp(0.0, self.duration());
        if let Some(s) = &self.silent {
            let mut g = s.lk();
            g.1 = t;
            if g.0.is_some() {
                g.0 = Some(Instant::now());
            }
            return;
        }
        let f = (t * self.sh.pcm.rate as f64) as u64;
        self.sh.pos.store(f, Ordering::Relaxed);
        *self.sh.cb.lk() = (Instant::now(), f);
    }

    /// Song volume (0–1)
    pub fn set_volume(&self, v: f32) {
        self.sh
            .vol
            .store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}
