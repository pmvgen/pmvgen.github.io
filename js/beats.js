// Beat detection for the PMV Generator and Media Storm – runs entirely in the browser (Web Audio).
// 1. Onset curve: energy rises in the bass (kick drum) and in the whole signal.
// 2. Tempo: autocorrelation of the onset curve, weighted around 120 BPM (against octave errors).
// 3. Beats: dynamic programming after D. Ellis (2007) – follows small tempo fluctuations.
// 4. Energy per beat (0–1): this gives the calm parts and the drops.

const HOP = 512;

export async function analyzeSong(arrayBuffer) {
  return analyzeBuffer(await new OfflineAudioContext(1, 1, 44100).decodeAudioData(arrayBuffer));
}

// Only a part of a decoded song (seconds) – e.g. the music from a longer video
export function sliceBuffer(buf, start, end) {
  const sr = buf.sampleRate;
  const a = Math.max(0, Math.floor(start * sr));
  const b = Math.min(buf.length, Math.ceil((end || buf.duration) * sr));
  const out = new AudioBuffer({ length: Math.max(1, b - a), numberOfChannels: buf.numberOfChannels, sampleRate: sr });
  for (let c = 0; c < buf.numberOfChannels; c++) out.copyToChannel(buf.getChannelData(c).subarray(a, b), c);
  return out;
}

export async function analyzeBuffer(buf) {
  const sr = buf.sampleRate;
  const fps = sr / HOP;
  const mono = mixdown(buf);
  const low = await lowpass(buf, 160);
  const n = Math.floor(mono.length / HOP);
  const onset = onsetCurve(frameEnergy(low, n), frameEnergy(mono, n), fps);
  const period = estimatePeriod(onset, fps);
  const beats = trackBeats(onset, period).map((f) => (f * HOP) / sr);
  return {
    buffer: buf,
    duration: buf.duration,
    bpm: (60 * fps) / period,
    beats,
    energy: beatEnergy(mono, sr, beats),
    peaks: overview(mono, 800),
  };
}

// Halve/double the tempo afterwards (if the detection is off by an octave)
export function rescale(song, factor) {
  const b = song.beats;
  let beats;
  if (factor === 2) {
    beats = [];
    for (let i = 0; i < b.length; i++) {
      beats.push(b[i]);
      if (i + 1 < b.length) beats.push((b[i] + b[i + 1]) / 2);
    }
  } else beats = b.filter((_, i) => i % 2 === 0);
  return Object.assign({}, song, { beats, bpm: song.bpm * factor, energy: resampleEnergy(song.energy, beats.length) });
}

// Shift the beat grid (seconds), e.g. when the cuts come slightly too early
export function shift(song, sec) {
  return Object.assign({}, song, { beats: song.beats.map((t) => t + sec).filter((t) => t >= 0 && t < song.duration) });
}

// ---------- Building blocks ----------

function mixdown(buf) {
  const out = new Float32Array(buf.length);
  for (let c = 0; c < buf.numberOfChannels; c++) {
    const d = buf.getChannelData(c);
    for (let i = 0; i < d.length; i++) out[i] += d[i] / buf.numberOfChannels;
  }
  return out;
}

async function lowpass(buf, freq) {
  const ctx = new OfflineAudioContext(1, buf.length, buf.sampleRate);
  const src = ctx.createBufferSource();
  src.buffer = buf;
  const f = ctx.createBiquadFilter();
  f.type = "lowpass";
  f.frequency.value = freq;
  src.connect(f).connect(ctx.destination);
  src.start();
  return (await ctx.startRendering()).getChannelData(0);
}

function frameEnergy(x, n) {
  const e = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    let s = 0;
    for (let j = i * HOP, end = j + HOP; j < end; j++) s += x[j] * x[j];
    e[i] = s;
  }
  return e;
}

function onsetCurve(eLow, eAll, fps) {
  const n = eLow.length;
  const raw = new Float32Array(n);
  for (let i = 1; i < n; i++) {
    const dl = Math.log(eLow[i] + 1e-6) - Math.log(eLow[i - 1] + 1e-6);
    const da = Math.log(eAll[i] + 1e-6) - Math.log(eAll[i - 1] + 1e-6);
    raw[i] = 0.65 * Math.max(0, dl) + 0.35 * Math.max(0, da);
  }
  // Subtract the moving average (only real peaks count) and normalize
  const w = Math.max(1, Math.round(fps * 0.4));
  const out = new Float32Array(n);
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc += raw[i];
    if (i >= w) acc -= raw[i - w];
    out[i] = Math.max(0, raw[i] - acc / Math.min(i + 1, w));
  }
  let mean = 0;
  for (let i = 0; i < n; i++) mean += out[i];
  mean /= n || 1;
  let sd = 0;
  for (let i = 0; i < n; i++) sd += (out[i] - mean) ** 2;
  sd = Math.sqrt(sd / (n || 1)) || 1;
  for (let i = 0; i < n; i++) out[i] /= sd;
  return out;
}

function autocorr(o, lag) {
  let s = 0;
  const n = o.length - lag;
  for (let i = 0; i < n; i++) s += o[i] * o[i + lag];
  return n > 0 ? s / n : 0;
}

function estimatePeriod(o, fps) {
  const lagMin = Math.floor((60 * fps) / 200);
  const lagMax = Math.ceil((60 * fps) / 60);
  const score = (l) => {
    const bpm = (60 * fps) / l;
    const w = Math.exp(-0.5 * (Math.log2(bpm / 120) / 0.9) ** 2);
    return w * (autocorr(o, l) + 0.5 * autocorr(o, 2 * l));
  };
  let best = lagMin;
  let bestS = -Infinity;
  const s = {};
  for (let l = lagMin - 1; l <= lagMax + 1; l++) s[l] = score(l);
  for (let l = lagMin; l <= lagMax; l++) {
    if (s[l] > bestS) {
      bestS = s[l];
      best = l;
    }
  }
  // Parabola through the neighbors → period with decimals
  const a = s[best - 1];
  const b = s[best];
  const c = s[best + 1];
  const d = a - 2 * b + c;
  return best + (d < 0 ? (0.5 * (a - c)) / d : 0);
}

function trackBeats(o, period) {
  const n = o.length;
  const alpha = 100;
  const score = new Float32Array(n);
  const back = new Int32Array(n).fill(-1);
  const lo = Math.round(2 * period);
  const hi = Math.max(1, Math.round(period / 2));
  for (let i = 0; i < n; i++) {
    let best = -Infinity;
    let arg = -1;
    for (let j = Math.max(0, i - lo); j <= i - hi; j++) {
      const v = score[j] - alpha * Math.log((i - j) / period) ** 2;
      if (v > best) {
        best = v;
        arg = j;
      }
    }
    score[i] = o[i] + (arg >= 0 && best > 0 ? best : 0);
    back[i] = arg >= 0 && best > 0 ? arg : -1;
  }
  // Find the best end in the last bar and trace back
  let end = n - 1;
  for (let i = Math.max(0, n - Math.round(period)); i < n; i++) if (score[i] > score[end]) end = i;
  const beats = [];
  for (let i = end; i >= 0; i = back[i]) beats.push(i);
  beats.reverse();
  // Fill gaps at the start (quiet intro) with the grid
  const first = beats[0] || 0;
  const pre = [];
  for (let f = first - period; f >= 0; f -= period) pre.unshift(Math.round(f));
  return pre.concat(beats);
}

function beatEnergy(mono, sr, beats) {
  const rms = beats.map((t, k) => {
    const a = Math.floor(t * sr);
    const b = Math.min(mono.length, Math.floor((beats[k + 1] || t + 0.5) * sr));
    let s = 0;
    for (let i = a; i < b; i++) s += mono[i] * mono[i];
    return Math.sqrt(s / Math.max(1, b - a));
  });
  const sorted = rms.slice().sort((x, y) => x - y);
  const q = (p) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] || 0;
  const lo = q(0.1);
  const span = q(0.95) - lo || 1;
  const e = rms.map((r) => Math.min(1, Math.max(0, (r - lo) / span)));
  // Smooth over one bar so the mood doesn't flicker
  return e.map((_, k) => {
    let s = 0;
    let c = 0;
    for (let j = k - 2; j <= k + 1; j++) if (j >= 0 && j < e.length) (s += e[j]), c++;
    return s / c;
  });
}

function resampleEnergy(e, len) {
  return Array.from({ length: len }, (_, i) => e[Math.min(e.length - 1, Math.floor((i * e.length) / len))] || 0);
}

function overview(x, buckets) {
  const out = new Float32Array(buckets);
  const size = Math.max(1, Math.floor(x.length / buckets));
  for (let b = 0; b < buckets; b++) {
    let m = 0;
    for (let i = b * size, end = Math.min(x.length, i + size); i < end; i++) m = Math.max(m, Math.abs(x[i]));
    out[b] = m;
  }
  return out;
}
