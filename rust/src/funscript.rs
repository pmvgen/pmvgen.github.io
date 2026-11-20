// A funscript built from the song – a port of funscriptgen.js. The beats become strokes (0 = bottom,
// 100 = top), the energy of the song decides how fast and how big.

use crate::bars::Bars;
use crate::settings::Settings;

fn clamp(v: f64, a: f64, b: f64) -> f64 {
    v.max(a).min(b)
}

pub fn build(
    beats: &[f64],
    energy: &[f64],
    bpm: f64,
    duration: f64,
    bars: Option<&Bars>,
    s: &Settings,
    title: &str,
) -> anyhow::Result<serde_json::Value> {
    let dur = if duration > 0.0 {
        duration
    } else {
        beats.last().map(|b| b + 1.0).unwrap_or(0.0)
    };
    if beats.is_empty() || dur <= 0.0 {
        anyhow::bail!("the song has no beats to build a script from");
    }
    let beat_len = 60.0 / if bpm > 0.0 { bpm } else { 120.0 };
    let center = if s.fs_size == "full" {
        50.0
    } else {
        match s.fs_where.as_str() {
            "low" => 32.0,
            "high" => 68.0,
            _ => 50.0,
        }
    };
    let half_of = |k: &str| match k {
        "small" => 12.0,
        "medium" => 25.0,
        "large" => 38.0,
        "full" => 50.0,
        _ => 25.0,
    };
    let mut raw: Vec<(f64, f64)> = vec![(0.0, center)];
    let at = |t: usize| -> f64 {
        if t < beats.len() {
            beats[t]
        } else {
            beats[beats.len() - 1] + (t as f64 - beats.len() as f64 + 1.0) * beat_len
        }
    };
    let calm = |k: usize| {
        let mut s = 0.0;
        for j in 0..4 {
            s += energy.get(k + j).copied().unwrap_or(0.5);
        }
        s / 4.0 < 0.18
    };
    let mut skip = 0;
    for (k, &t) in beats.iter().enumerate() {
        if k < skip {
            continue;
        }
        let e = energy.get(k).copied().unwrap_or(0.5);
        let prev = if k >= 4 {
            energy.get(k - 4).copied().unwrap_or(0.0)
        } else {
            0.0
        };
        let drop = e - prev > 0.35 && e > 0.6;
        let bar = match bars {
            Some(b) => b.downbeat.get(k).copied().unwrap_or(false),
            None => k % 4 == 0,
        };
        let rest = s.fs_rests && calm(k);
        let mut rate = match s.fs_pace.as_str() {
            "slow" => 0.5,
            "normal" => 1.0,
            "fast" => 2.0,
            _ => {
                if e < 0.3 {
                    0.5
                } else if e < 0.62 {
                    1.0
                } else {
                    2.0
                }
            }
        };
        if s.fs_pace == "auto" && drop {
            rate = 2.0;
        }
        if rest {
            rate = f64::min(rate, 0.25);
        }
        let mut amp = if s.fs_size == "auto" {
            0.35 + 0.65 * e
        } else {
            1.0
        };
        let half = if s.fs_size == "auto" {
            half_of("large")
        } else {
            half_of(&s.fs_size)
        };
        if rest {
            amp *= 0.6;
        }
        if s.fs_accent {
            amp *= if drop {
                1.3
            } else if bar {
                1.12
            } else {
                1.0
            };
        }
        let h = clamp(half * amp, 4.0, 50.0);
        let lo = clamp(center - h, 0.0, 100.0);
        let hi = clamp(center + h, 0.0, 100.0);
        if rate >= 1.0 {
            let n = rate.round() as usize;
            let len = at(k + 1) - t;
            for st in 0..n {
                let ts = t + len * st as f64 / n as f64;
                raw.push((ts, lo));
                raw.push((ts + len / (2.0 * n as f64), hi));
            }
        } else {
            let span = (1.0 / rate).round() as usize;
            raw.push((t, lo));
            raw.push((at(k + span / 2), hi));
            skip = k + span;
        }
    }
    raw.push((dur, center));

    // The device can only move so fast: a stroke that would be quicker is made smaller
    let vmax = clamp(s.fs_speed, 100.0, 600.0);
    let mut acts: Vec<(f64, f64)> = Vec::new();
    for (t, p) in raw {
        if let Some(&(pt, pp)) = acts.last() {
            if t <= pt + 0.02 {
                continue;
            }
            let mut pos = clamp(p, 0.0, 100.0);
            let max_d = vmax * (t - pt);
            if (pos - pp).abs() > max_d {
                pos = pp + (pos - pp).signum() * max_d;
            }
            acts.push((t, pos));
        } else {
            acts.push((t, clamp(p, 0.0, 100.0)));
        }
    }
    let mut out = acts.clone();
    if s.fs_style == "smooth" {
        out.clear();
        for i in 0..acts.len() {
            out.push(acts[i]);
            if i + 1 >= acts.len() || acts[i + 1].0 - acts[i].0 <= 0.12 {
                continue;
            }
            let next = acts[i + 1];
            let n = ((next.0 - acts[i].0) / 0.05).floor() as usize;
            for j in 1..n {
                let f = j as f64 / n as f64;
                out.push((
                    acts[i].0 + (next.0 - acts[i].0) * f,
                    acts[i].1
                        + (next.1 - acts[i].1) * (0.5 - 0.5 * (std::f64::consts::PI * f).cos()),
                ));
            }
        }
    }
    let actions: Vec<serde_json::Value> = out
        .iter()
        .map(|(t, p)| serde_json::json!({ "at": (t * 1000.0).round() as i64, "pos": clamp(*p, 0.0, 100.0).round() as i64 }))
        .collect();
    Ok(serde_json::json!({
        "version": "1.0",
        "inverted": false,
        "range": 100,
        "actions": actions,
        "metadata": { "creator": "PMV Generator", "title": title, "description": format!("pace {}, size {}, {}, {}", s.fs_pace, s.fs_size, s.fs_where, s.fs_style) }
    }))
}
