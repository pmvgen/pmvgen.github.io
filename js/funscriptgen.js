// A funscript built from the song: the beats become strokes (position 0 = bottom, 100 = top), the energy of the song
// decides how fast and how big. Options o: pace (auto | slow | normal | fast), size (auto | small | medium | large | full),
// where (low | mid | high), style (sharp | smooth), rests (calm parts get slow, gentle strokes), accent (bigger stroke on the
// first beat of a bar and on drops), speed (the fastest the device may be asked to move, position units per second).
// The times are the song's own (seconds from its start) – the same clock the PMV Generator plays the song with.

export const FS_DEFAULTS = { fsOn: false, fsPace: "auto", fsSize: "auto", fsWhere: "mid", fsStyle: "sharp", fsRests: true, fsAccent: true, fsSpeed: 300 };

const HALF = { small: 12, medium: 25, large: 38, full: 50 }; // half the height of a stroke
const CENTER = { low: 32, mid: 50, high: 68 };
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

export function buildFunscript(song, bars, S) {
  const o = Object.assign({}, FS_DEFAULTS, S || {});
  const beats = song.beats || [];
  const energy = song.energy || [];
  const dur = song.duration || (beats.length ? beats[beats.length - 1] + 1 : 0);
  if (!beats.length || !dur) throw new Error("the song has no beats to build a script from");
  const beatLen = song.beatLen || 60 / (song.bpm || 120);
  const center = o.fsSize === "full" ? 50 : CENTER[o.fsWhere] || 50;
  const raw = [[0, center]];
  const at = (t) => beats[t] != null ? beats[t] : beats[beats.length - 1] + (t - beats.length + 1) * beatLen;
  const calm = (k) => {
    let s = 0;
    for (let j = 0; j < 4; j++) s += energy[k + j] != null ? energy[k + j] : 0.5;
    return s / 4 < 0.18;
  };
  let skip = 0; // a slow stroke covers several beats: no new one starts before
  for (let k = 0; k < beats.length; k++) {
    if (k < skip) continue;
    const t = beats[k];
    const e = energy[k] != null ? energy[k] : 0.5;
    const drop = e - (energy[k - 4] || 0) > 0.35 && e > 0.6;
    const bar = bars ? !!(bars.downbeat && bars.downbeat[k]) : k % 4 === 0;
    const rest = o.fsRests && calm(k);
    // strokes per beat: 0.25 = one stroke over four beats … 2 = two strokes in one beat
    let rate = { slow: 0.5, normal: 1, fast: 2 }[o.fsPace] || (e < 0.3 ? 0.5 : e < 0.62 ? 1 : 2);
    if (o.fsPace === "auto" && drop) rate = 2;
    if (rest) rate = Math.min(rate, 0.25);
    // how big: auto follows the energy
    let amp = o.fsSize === "auto" ? 0.35 + 0.65 * e : 1;
    const half = o.fsSize === "auto" ? HALF.large : HALF[o.fsSize] || HALF.medium;
    if (rest) amp *= 0.6;
    if (o.fsAccent) amp *= drop ? 1.3 : bar ? 1.12 : 1;
    const h = clamp(half * amp, 4, 50);
    const lo = clamp(center - h, 0, 100);
    const hi = clamp(center + h, 0, 100);
    if (rate >= 1) {
      const n = Math.round(rate);
      const len = at(k + 1) - t;
      for (let s = 0; s < n; s++) {
        const ts = t + (len * s) / n;
        raw.push([ts, lo], [ts + len / (2 * n), hi]);
      }
    } else {
      const span = Math.round(1 / rate); // beats per stroke
      raw.push([t, lo], [at(k + span / 2), hi]);
      skip = k + span;
    }
  }
  raw.push([dur, center]);

  // The device can only move so fast: a stroke that would be quicker is made smaller
  const vmax = clamp(Number(o.fsSpeed) || 300, 100, 600);
  const acts = [];
  for (const [t, p] of raw) {
    const prev = acts[acts.length - 1];
    if (prev && t <= prev[0] + 0.02) continue;
    let pos = clamp(p, 0, 100);
    if (prev) {
      const maxD = vmax * (t - prev[0]);
      if (Math.abs(pos - prev[1]) > maxD) pos = prev[1] + Math.sign(pos - prev[1]) * maxD;
    }
    acts.push([t, pos]);
  }

  // smooth: round the corners (a point every 50 ms along a cosine between two points)
  let out = acts;
  if (o.fsStyle === "smooth") {
    out = [];
    for (let i = 0; i < acts.length; i++) {
      out.push(acts[i]);
      const next = acts[i + 1];
      if (!next || next[0] - acts[i][0] <= 0.12) continue;
      const n = Math.floor((next[0] - acts[i][0]) / 0.05);
      for (let j = 1; j < n; j++) {
        const f = j / n;
        out.push([acts[i][0] + (next[0] - acts[i][0]) * f, acts[i][1] + (next[1] - acts[i][1]) * (0.5 - 0.5 * Math.cos(Math.PI * f))]);
      }
    }
  }
  return {
    version: "1.0",
    inverted: false,
    range: 100,
    actions: out.map(([t, p]) => ({ at: Math.round(t * 1000), pos: Math.round(clamp(p, 0, 100)) })),
    metadata: { creator: "PMV Generator", title: song.name || "", description: `pace ${o.fsPace}, size ${o.fsSize}, ${o.fsWhere}, ${o.fsStyle}` },
  };
}
