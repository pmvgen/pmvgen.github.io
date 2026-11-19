// Records what the ORIGINAL pmvgen modules return (rust-anything's "record" step), for the Rust port to replay.
import * as wa from "node-web-audio-api";
import fs from "node:fs";
globalThis.OfflineAudioContext = wa.OfflineAudioContext;
globalThis.AudioBuffer = wa.AudioBuffer;
const { analyzeSong, rescale, shift } = await import("./orig/beats.js");
const { analyzeBars } = await import("./orig/bars.js");
const { buildFunscript } = await import("./orig/funscriptgen.js");
const { slotsFor } = await import("./orig/pmvfx.js");
const { measure } = await import("./orig/pmvsmart_x.js");

// deterministic noise
let seed = 12345;
const rnd = () => ((seed = (seed * 1103515245 + 12345) >>> 0) / 4294967296);

function wav(name, secs, gen) {
  const sr = 44100, n = Math.floor(secs * sr);
  const buf = Buffer.alloc(44 + n * 4);
  buf.write("RIFF", 0); buf.writeUInt32LE(36 + n * 4, 4); buf.write("WAVE", 8); buf.write("fmt ", 12);
  buf.writeUInt32LE(16, 16); buf.writeUInt16LE(1, 20); buf.writeUInt16LE(2, 22); buf.writeUInt32LE(sr, 24);
  buf.writeUInt32LE(sr * 4, 28); buf.writeUInt16LE(4, 32); buf.writeUInt16LE(16, 34); buf.write("data", 36); buf.writeUInt32LE(n * 4, 40);
  for (let i = 0; i < n; i++) {
    const [l, r] = gen(i / sr, i);
    buf.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(l * 32767))), 44 + i * 4);
    buf.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(r * 32767))), 46 + i * 4);
  }
  fs.writeFileSync(`songs/${name}.wav`, buf);
  return buf;
}
const kick = (ph) => (ph < 0.25 ? Math.sin(2 * Math.PI * (50 + 80 * Math.exp(-ph * 30)) * ph) * Math.exp(-ph * 9) : 0);
const hat = (ph) => (ph < 0.05 ? (rnd() - 0.5) * Math.exp(-ph * 90) : 0);

const songs = {
  house128: [40, (t) => { const b = 60 / 128, ph = t % b, loud = t > 14 && t < 30; const v = 0.8 * kick(ph) + (loud ? 0.5 * hat((t + b / 2) % b) + 0.2 * Math.sin(2 * Math.PI * 220 * t) : 0.03 * Math.sin(2 * Math.PI * 330 * t)); return [v, v * 0.9]; }],
  hiphop90: [35, (t) => { const b = 60 / 90, ph = t % b, k = Math.floor(t / b); const v = (k % 4 === 0 || k % 4 === 2 ? 0.9 : 0.25) * kick(ph) + 0.3 * hat((t + b * 0.66) % b) + 0.1 * Math.sin(2 * Math.PI * 110 * t); return [v, v]; }],
  dnb174: [30, (t) => { const b = 60 / 174, ph = t % b; const v = 0.7 * kick(ph) + 0.4 * hat((t + b / 2) % b) + (t > 12 ? 0.25 * (rnd() - 0.5) : 0); return [v, -v * 0.8 + 0.05]; }],
  drift: [45, (t) => { const bpm = 118 + 8 * t / 45, phase = (t * bpm / 60) % 1, ph = phase * 60 / bpm; const v = 0.8 * kick(ph) + 0.2 * hat(((phase + 0.5) % 1) * 60 / bpm); return [v, v]; }],
  quietintro: [40, (t) => { const b = 60 / 124, ph = t % b; const v = t < 9 ? 0.05 * Math.sin(2 * Math.PI * 440 * t) * Math.sin(2 * Math.PI * 0.5 * t) : 0.8 * kick(ph) + 0.25 * hat((t + b / 2) % b) + (t > 25 ? 0.3 * Math.sin(2 * Math.PI * 165 * t) : 0); return [v, v]; }],
  pickup: [36, (t) => { const b = 60 / 140, k = Math.floor((t - 0.3) / b), ph = ((t - 0.3) % b + b) % b; const acc = ((k % 4) + 4) % 4 === 1 ? 1 : 0.45; const v = t < 0.3 ? 0 : acc * kick(ph) + 0.2 * hat((t + b / 2) % b); return [v, v]; }],
};

const FS = [
  {},
  { fsPace: "fast", fsSize: "large", fsWhere: "high", fsStyle: "smooth" },
  { fsPace: "slow", fsSize: "small", fsWhere: "low", fsRests: false, fsAccent: false, fsSpeed: 150 },
  { fsPace: "auto", fsSize: "full", fsStyle: "sharp", fsSpeed: 600 },
];
const out = { songs: [], slots: [], measure: [] };
for (const [name, [secs, gen]] of Object.entries(songs)) {
  const bytes = wav(name, secs, gen);
  const ab = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const song = await analyzeSong(ab);
  const bars = analyzeBars(song);
  const r2 = rescale(song, 2), rh = rescale(song, 0.5), sh = shift(song, 0.04);
  const fsc = FS.map((o) => buildFunscript(song, bars, o).actions);
  out.songs.push({
    name, file: `songs/${name}.wav`, bpm: song.bpm, duration: song.duration, beats: song.beats, energy: song.energy, pace: song.pace,
    downbeat: bars && Array.from(bars.downbeat), pos: bars && Array.from(bars.pos), phrase: bars && Array.from(bars.phrase),
    rescale2: r2.beats, rescaleHalf: rh.beats, rescale2Energy: r2.energy, shift40: sh.beats, fs: FS, fsActions: fsc,
  });
  console.log(name, song.bpm.toFixed(2), song.beats.length, bars ? "bars" : "no bars");
}
for (const id of ["full", "kaleido", "duo", "trim", "tri", "quad"]) for (const [W, H] of [[1280, 720], [1081, 1921], [333, 77]]) for (const dir of ["cols", "rows"]) out.slots.push({ id, W, H, dir, slots: slotsFor(id, W, H, dir) });
for (let c = 0; c < 40; c++) {
  const sw = 8 + Math.floor(rnd() * 57), sh = 8 + Math.floor(rnd() * 57);
  const px = new Uint8ClampedArray(sw * sh * 4);
  const skinish = c % 3 === 0;
  for (let i = 0; i < px.length; i += 4) { px[i] = skinish ? 150 + rnd() * 100 : rnd() * 255; px[i + 1] = skinish ? 80 + rnd() * 60 : rnd() * 255; px[i + 2] = skinish ? 40 + rnd() * 60 : rnd() * 255; px[i + 3] = 255; }
  const m = measure({ px, sw, sh });
  out.measure.push({ sw, sh, px: Array.from(px), focus: m.focus, skin: m.skin, contrast: m.contrast, lum: m.lum, hist: Array.from(m.hist), color: m.color });
}
fs.writeFileSync("fixtures.json", JSON.stringify(out));
console.log("recorded", out.songs.length, "songs,", out.slots.length, "layouts,", out.measure.length, "pictures");
