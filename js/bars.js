// Bars and phrases from the beats: which beat starts a bar (the "one") and which bar starts a new
// phrase (4 bars). The beat tracker only knows beats – "beat 0" isn't necessarily the one, and a
// song that starts with a pickup or a short intro breaks the "every 4th beat" guess.
//
// How: every beat gets an accent (how much stronger its bass hit and its overall hit are than what
// comes just before it, compared with its neighbours). The bar's "one" is the position among 4 whose
// beats have the largest accent sum – checked in windows of 8 bars, and only moved when another
// position is clearly better (songs with a break or a changed pattern). The same one level up:
// bars with the largest accent and the biggest rise in loudness start a phrase.

const PER = 4; // 4/4 – what PMV music almost always is

// Per beat: bass hit + overall hit, relative to the beats around it (about 0 = ordinary, > 0 = accent)
function accents(buf, beats) {
  const sr = buf.sampleRate;
  const chs = [];
  for (let c = 0; c < buf.numberOfChannels; c++) chs.push(buf.getChannelData(c));
  const n = beats.length;
  const lowE = new Float32Array(n * 2); // [pre, hit] per beat, bass
  const allE = new Float32Array(n * 2);
  const a = 1 - Math.exp((-2 * Math.PI * 150) / sr); // one-pole lowpass at about 150 Hz
  const span = (t0, t1) => {
    const i0 = Math.max(0, Math.floor(t0 * sr));
    const i1 = Math.min(buf.length, Math.max(i0 + 1, Math.floor(t1 * sr)));
    let lo = 0;
    let al = 0;
    let y = 0;
    const step = 2; // every second sample is plenty
    for (let i = i0; i < i1; i += step) {
      let x = 0;
      for (const d of chs) x += d[i];
      x /= chs.length;
      y += a * step * (x - y);
      lo += y * y;
      al += x * x;
    }
    const m = Math.max(1, (i1 - i0) / step);
    return [lo / m, al / m];
  };
  for (let k = 0; k < n; k++) {
    const t = beats[k];
    const len = (beats[k + 1] || t + 0.5) - t;
    const pre = span(t - Math.min(0.3, 0.4 * len), t - 0.02);
    const hit = span(t - 0.02, t + Math.min(0.15, 0.4 * len));
    lowE[2 * k] = pre[0];
    lowE[2 * k + 1] = hit[0];
    allE[2 * k] = pre[1];
    allE[2 * k + 1] = hit[1];
  }
  const e = 1e-9;
  const raw = new Float32Array(n);
  for (let k = 0; k < n; k++) {
    const jl = Math.max(0, Math.log(lowE[2 * k + 1] + e) - Math.log(lowE[2 * k] + e));
    const ja = Math.max(0, Math.log(allE[2 * k + 1] + e) - Math.log(allE[2 * k] + e));
    raw[k] = 0.6 * jl + 0.4 * ja + 0.25 * Math.log(lowE[2 * k + 1] + e);
  }
  return relative(raw, 8);
}

// Value minus the average of its surroundings (±r), in units of the local spread
function relative(v, r) {
  const n = v.length;
  const out = new Float32Array(n);
  for (let k = 0; k < n; k++) {
    let s = 0;
    let c = 0;
    for (let j = Math.max(0, k - r); j <= Math.min(n - 1, k + r); j++) (s += v[j]), c++;
    const mean = s / c;
    let q = 0;
    for (let j = Math.max(0, k - r); j <= Math.min(n - 1, k + r); j++) q += (v[j] - mean) ** 2;
    out[k] = (v[k] - mean) / (Math.sqrt(q / c) || 1);
  }
  return out;
}

// Which of the PER positions holds the "one" – per element: globally the best, moved in a window only
// when another is clearly better there (margin in accent units, summed over the window)
export function phases(vals, per, win, hop, margin) {
  const n = vals.length;
  const sums = (a, b) => {
    const s = new Float64Array(per);
    for (let k = a; k < b; k++) s[k % per] += vals[k];
    return s;
  };
  const best = (s) => s.reduce((bi, x, i) => (x > s[bi] ? i : bi), 0);
  const out = new Uint8Array(n);
  let cur = best(sums(0, n));
  for (let h = 0; h * hop < n; h++) {
    const a = Math.max(0, h * hop - Math.round((win - hop) / 2));
    const b = Math.min(n, a + win);
    const s = sums(a, b);
    const bi = best(s);
    if (bi !== cur && s[bi] - s[cur] > margin) cur = bi;
    for (let k = h * hop; k < Math.min(n, (h + 1) * hop); k++) out[k] = cur;
  }
  return out;
}

// → { downbeat[k] (starts a bar), pos[k] (0–3 in the bar), phrase[k] (starts a 4-bar phrase) } or null
export function analyzeBars(song) {
  const buf = song && song.buffer;
  const beats = song && song.beats;
  if (!buf || !beats || beats.length < 4 * PER) return null;
  let acc;
  try {
    acc = accents(buf, beats);
  } catch (e) {
    return null;
  }
  const n = beats.length;
  const ph = phases(acc, PER, 8 * PER, PER, 2.5);
  const pos = new Uint8Array(n);
  const downbeat = new Uint8Array(n);
  for (let k = 0; k < n; k++) {
    pos[k] = (((k - ph[k]) % PER) + PER) % PER;
    downbeat[k] = pos[k] === 0 ? 1 : 0;
  }
  // Phrases: per bar the accent of its "one" plus how much louder the bar is than the one before
  const bars = [];
  for (let k = 0; k < n; k++) if (downbeat[k]) bars.push(k);
  const energy = song.energy || [];
  const barVal = bars.map((k, b) => {
    const end = bars[b + 1] ?? n;
    const mean = (a, z) => {
      let s = 0;
      let c = 0;
      for (let j = a; j < z; j++) (s += energy[j] || 0), c++;
      return c ? s / c : 0;
    };
    const rise = b ? mean(k, end) - mean(bars[b - 1], k) : 0;
    return acc[k] + 4 * Math.abs(rise); // (a drop in loudness starts a phrase as well)
  });
  const phrase = new Uint8Array(n);
  if (bars.length >= 8) {
    const pp = phases(barVal, 4, 16, 4, 3);
    bars.forEach((k, b) => {
      if ((((b - pp[b]) % 4) + 4) % 4 === 0) phrase[k] = 1;
    });
  } else bars.forEach((k, b) => b % 4 === 0 && (phrase[k] = 1));
  return { downbeat, pos, phrase };
}
