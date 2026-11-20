import fs from "node:fs";
import { File } from "node:buffer";
const { readWav, songFromFrames } = await import("./orig/beats_x.js");
// a 46-minute mix: 124 BPM, then 128, then 122 (mono 16-bit, 22.05 kHz)
const sr = 22050, secs = 46 * 60, n = sr * secs;
const buf = Buffer.alloc(44 + n * 2);
buf.write("RIFF", 0); buf.writeUInt32LE(36 + n * 2, 4); buf.write("WAVE", 8); buf.write("fmt ", 12);
buf.writeUInt32LE(16, 16); buf.writeUInt16LE(1, 20); buf.writeUInt16LE(1, 22); buf.writeUInt32LE(sr, 24);
buf.writeUInt32LE(sr * 2, 28); buf.writeUInt16LE(2, 32); buf.writeUInt16LE(16, 34); buf.write("data", 36); buf.writeUInt32LE(n * 2, 40);
let phase = 0, seed = 7;
const rnd = () => ((seed = (seed * 1103515245 + 12345) >>> 0) / 4294967296);
for (let i = 0; i < n; i++) {
  const t = i / sr;
  const bpm = t < 900 ? 124 : t < 1900 ? 128 : 122;
  phase = (phase + bpm / 60 / sr) % 1;
  const ph = phase * 60 / bpm;
  const loud = Math.floor(t / 64) % 3 === 1;
  let v = ph < 0.25 ? 0.7 * Math.sin(2 * Math.PI * (50 + 80 * Math.exp(-ph * 30)) * ph) * Math.exp(-ph * 9) : 0;
  const hp = ((phase + 0.5) % 1) * 60 / bpm;
  if (loud && hp < 0.05) v += 0.3 * (rnd() - 0.5) * Math.exp(-hp * 90);
  buf.writeInt16LE(Math.round(Math.max(-1, Math.min(1, v)) * 32767), 44 + i * 2);
}
fs.writeFileSync("songs/mix46.wav", buf);
const file = new File([buf], "mix46.wav");
const frames = await readWav(file, secs, () => {});
const song = songFromFrames(frames);
fs.writeFileSync("fixtures_long.json", JSON.stringify({ file: "songs/mix46.wav", bpm: song.bpm, beats: song.beats, energy: song.energy, pace: song.pace, duration: song.duration }));
console.log("long:", song.bpm.toFixed(3), song.beats.length, "beats", song.duration.toFixed(1), "s");
