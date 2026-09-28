// PMV as template: analyze an existing PMV – music, cuts, layouts (split screens) and flashes.
// The video plays invisibly at several times the speed; every displayed frame is scaled down
// and measured. Effects like glitch or text are burned into the picture and can't be
// reversed – what's taken over is timing, structure and flashes.

import { analyzeSong } from "./beats.js";

const GW = 96; // measuring image for layout and cuts
const GH = 54;

// src: Blob (file or downloaded scene). onProgress(share, text). signal: AbortSignal
export async function scanPmv(src, onProgress, signal) {
  onProgress(0, "Reading music …");
  const buf = await src.arrayBuffer();
  let song;
  try {
    song = await analyzeSong(buf);
  } catch (e) {
    throw new Error("The browser can't read the audio track of this video.");
  }
  if (signal && signal.aborted) throw abortErr();
  onProgress(0.1, "Looking through the PMV …");
  const frames = await sampleFrames(src, (p) => onProgress(0.1 + 0.85 * p, "Looking through the PMV …"), signal);
  onProgress(0.97, "Evaluating …");
  const tpl = buildTemplate(frames, song);
  onProgress(1, "Done");
  return tpl;
}

// ---------- Sample frames ----------

async function sampleFrames(blob, onProgress, signal) {
  const url = URL.createObjectURL(blob);
  const v = document.createElement("video");
  v.muted = true;
  v.playsInline = true;
  v.preload = "auto";
  // Visible as far as the browser is concerned (otherwise it throttles), but practically invisible
  v.style.cssText = "position:fixed;left:0;top:0;width:2px;height:2px;opacity:.01;pointer-events:none";
  document.body.appendChild(v);
  const c = document.createElement("canvas");
  c.width = GW;
  c.height = GH;
  const g = c.getContext("2d", { willReadFrequently: true });
  const frames = [];
  let prevGray = null;
  try {
    v.src = url;
    await new Promise((res, rej) => {
      v.onloadedmetadata = res;
      v.onerror = () => rej(new Error("The browser can't play this video (format/codec)."));
    });
    const dur = v.duration;
    v.playbackRate = 3;
    await new Promise((res, rej) => {
      let lastT = -1;
      const onFrame = (now, meta) => {
        if (signal && signal.aborted) return rej(abortErr());
        const t = meta.mediaTime;
        if (t > lastT) {
          lastT = t;
          g.drawImage(v, 0, 0, GW, GH);
          const f = measure(g.getImageData(0, 0, GW, GH).data, prevGray);
          f.t = t;
          prevGray = f.gray;
          frames.push(f);
          if (frames.length > 1) frames[frames.length - 2].gray = null; // save memory
          onProgress(Math.min(1, t / dur));
        }
        if (!v.ended) v.requestVideoFrameCallback(onFrame);
      };
      v.onended = () => res();
      v.onerror = () => rej(new Error("Playback failed during the analysis."));
      v.requestVideoFrameCallback(onFrame);
      v.play().catch(rej);
    });
  } finally {
    v.pause();
    v.removeAttribute("src");
    v.load();
    v.remove();
    URL.revokeObjectURL(url);
  }
  if (frames.length < 10) throw new Error("Too few frames read – is this a video?");
  return frames;
}

// Key figures of a measuring image: brightness, color, difference to the previous frame (overall and per region),
// edges at typical divider positions and mirror similarity.
function measure(px, prev) {
  const n = GW * GH;
  const gray = new Float32Array(n);
  let sum = 0;
  let r = 0;
  let gr = 0;
  let b = 0;
  for (let i = 0, j = 0; i < n; i++, j += 4) {
    const y = 0.299 * px[j] + 0.587 * px[j + 1] + 0.114 * px[j + 2];
    gray[i] = y;
    sum += y;
    r += px[j];
    gr += px[j + 1];
    b += px[j + 2];
  }
  const luma = sum / n / 255;
  let varSum = 0;
  for (let i = 0; i < n; i++) varSum += (gray[i] - luma * 255) ** 2;
  const std = Math.sqrt(varSum / n);

  // Difference to the previous frame, per region (thirds, halves, quarters, row thirds)
  const diff = { all: 0, c3: [0, 0, 0], c2: [0, 0], q4: [0, 0, 0, 0], r3: [0, 0, 0], r2: [0, 0] };
  if (prev) {
    const cnt = { c3: [0, 0, 0], c2: [0, 0], q4: [0, 0, 0, 0], r3: [0, 0, 0], r2: [0, 0] };
    for (let y = 0; y < GH; y++) {
      for (let x = 0; x < GW; x++) {
        const i = y * GW + x;
        const d = Math.abs(gray[i] - prev[i]) / 255;
        diff.all += d;
        const c3 = Math.min(2, Math.floor((x * 3) / GW));
        const c2 = x < GW / 2 ? 0 : 1;
        const r3 = Math.min(2, Math.floor((y * 3) / GH));
        const r2 = y < GH / 2 ? 0 : 1;
        diff.c3[c3] += d;
        cnt.c3[c3]++;
        diff.c2[c2] += d;
        cnt.c2[c2]++;
        diff.q4[r2 * 2 + c2] += d;
        cnt.q4[r2 * 2 + c2]++;
        diff.r3[r3] += d;
        cnt.r3[r3]++;
        diff.r2[r2] += d;
        cnt.r2[r2]++;
      }
    }
    diff.all /= n;
    for (const k of ["c3", "c2", "q4", "r3", "r2"]) diff[k] = diff[k].map((s, i) => s / cnt[k][i]);
  }

  // Dividers: a real border between two clips runs through almost every row (or column).
  // We measure in how many rows the jump across the border is clearly bigger than the
  // differences right next to it – edges within the picture (bodies, furniture) aren't enough for that.
  const colCover = (at) => {
    const x0 = Math.round(at * GW);
    let best = 0;
    for (let c = x0 - 1; c <= x0 + 1; c++) {
      let hit = 0;
      for (let y = 0; y < GH; y++) {
        const row = y * GW;
        const across = Math.abs(gray[row + c + 1] - gray[row + c - 2]);
        const left = Math.abs(gray[row + c - 2] - gray[row + c - 5]);
        const right = Math.abs(gray[row + c + 4] - gray[row + c + 1]);
        if (across > 2 * Math.max(left, right) + 10) hit++;
      }
      best = Math.max(best, hit / GH);
    }
    return best;
  };
  const rowCover = (at) => {
    const y0 = Math.round(at * GH);
    let best = 0;
    for (let r = y0 - 1; r <= y0 + 1; r++) {
      let hit = 0;
      for (let x = 0; x < GW; x++) {
        const across = Math.abs(gray[(r + 1) * GW + x] - gray[(r - 2) * GW + x]);
        const up = Math.abs(gray[(r - 2) * GW + x] - gray[(r - 5) * GW + x]);
        const down = Math.abs(gray[(r + 4) * GW + x] - gray[(r + 1) * GW + x]);
        if (across > 2 * Math.max(up, down) + 10) hit++;
      }
      best = Math.max(best, hit / GW);
    }
    return best;
  };
  const edges = { c13: colCover(1 / 3), c23: colCover(2 / 3), c12: colCover(1 / 2), r12: rowCover(1 / 2), r13: rowCover(1 / 3), r23: rowCover(2 / 3) };

  // Mirror similarity (0 = identically mirrored), relative to the contrast
  const mirror = (x0, x1, y0, y1, fx, fy) => {
    let s = 0;
    let c = 0;
    for (let y = y0; y < y1; y++) {
      for (let x = x0; x < x1; x++) {
        const mx = fx ? GW - 1 - x : x;
        const my = fy ? GH - 1 - y : y;
        s += Math.abs(gray[y * GW + x] - gray[my * GW + mx]);
        c++;
      }
    }
    return s / c / (std + 8);
  };
  const third = Math.floor(GW / 3);
  const mir = {
    lr3: mirror(0, third - 2, 0, GH, true, false), // left third vs. mirrored right third
    lr2: mirror(0, GW / 2 - 2, 0, GH, true, false),
    tb2: mirror(0, GW, 0, GH / 2 - 2, false, true),
    tb3: mirror(0, GW, 0, Math.floor(GH / 3) - 2, false, true),
  };
  return { gray, luma, color: [r / n, gr / n, b / n], std, diff, edges, mir, layout: classify(edges, mir, std) };
}

// Layout of a frame from dividers and mirroring.
// Thresholds calibrated on a recorded PMV with a known sequence:
// mirrored thirds differ by 0.01–0.03 (fullscreen/3-way ≥ 0.17); in a 3-way layout at least
// one border usually runs through > 45 % of the rows (fullscreen < 31 % in 90 % of cases).
function classify(e, m, std) {
  if (std < 6) return null; // (almost) a single color, e.g. a flash – not meaningful
  if (m.lr2 < 0.08 && m.tb2 < 0.08) return { id: "kaleido", dir: "cols" };
  if (m.lr3 < 0.08) return { id: "trim", dir: "cols" };
  if (m.tb3 < 0.08 && Math.max(e.r13, e.r23) > 0.3) return { id: "trim", dir: "rows" };
  const cols3 = Math.max(e.c13, e.c23) > 0.45 || e.c13 + e.c23 > 0.8;
  const rows3 = Math.max(e.r13, e.r23) > 0.45 || e.r13 + e.r23 > 0.8;
  if (e.c12 > 0.45 && e.r12 > 0.45) return { id: "quad", dir: "cols" };
  if (cols3 && e.c12 < 0.3) return { id: "tri", dir: "cols" };
  if (rows3 && e.r12 < 0.3) return { id: "tri", dir: "rows" };
  if (e.c12 > 0.5) return { id: "duo", dir: "cols" };
  if (e.r12 > 0.5) return { id: "duo", dir: "rows" };
  return { id: "full", dir: "cols" };
}

// ---------- Build the template ----------

const REGIONS = {
  full: { cols: [["all"]], rows: [["all"]] },
  kaleido: { cols: [["all"]], rows: [["all"]] },
  duo: { cols: [["c2", 0], ["c2", 1]], rows: [["r2", 0], ["r2", 1]] },
  trim: { cols: [["c3", 0, 2], ["c3", 1]], rows: [["r3", 0, 2], ["r3", 1]] },
  tri: { cols: [["c3", 0], ["c3", 1], ["c3", 2]], rows: [["r3", 0], ["r3", 1], ["r3", 2]] },
  quad: { cols: [["q4", 0], ["q4", 1], ["q4", 2], ["q4", 3]], rows: [["q4", 0], ["q4", 1], ["q4", 2], ["q4", 3]] },
};

function regionDiff(f, spec) {
  if (spec[0] === "all") return f.diff.all;
  const arr = f.diff[spec[0]];
  const idx = spec.slice(1);
  return Math.max(...idx.map((i) => arr[i]));
}

function buildTemplate(frames, song) {
  // 1. Flashes: brightness jumps up sharply
  const flashes = [];
  for (let i = 1; i < frames.length; i++) {
    const f = frames[i];
    const p = frames[i - 1];
    if (f.luma - p.luma > 0.22 && f.luma > 0.55) {
      const [r, g, b] = f.color;
      const pink = r > 200 && b > 90 && g < r - 50;
      flashes.push({ t: f.t, strength: Math.min(1, (f.luma - p.luma) * 2), color: pink ? "#ff3e8a" : "#fff" });
      f.flash = true;
    }
  }
  // 2. Cuts: a big frame difference in at least one region, compared with the motion before
  const cutsIdx = [];
  const recent = [];
  for (let i = 1; i < frames.length; i++) {
    const f = frames[i];
    if (f.flash || frames[i - 1].flash) continue;
    const local = recent.length ? recent.slice().sort((a, b) => a - b)[Math.floor(recent.length / 2)] : 0.02;
    const regional = Math.max(f.diff.all, ...f.diff.c3, ...f.diff.c2, ...f.diff.q4, ...f.diff.r3);
    const thr = Math.max(0.09, local * 3.2);
    if (regional > thr && (!cutsIdx.length || f.t - frames[cutsIdx[cutsIdx.length - 1]].t > 0.15)) cutsIdx.push(i);
    recent.push(f.diff.all);
    if (recent.length > 12) recent.shift();
  }
  // 3. Sections between cuts: layout by majority of the frames in the middle
  const bounds = [0, ...cutsIdx, frames.length];
  const segs = [];
  for (let s = 0; s + 1 < bounds.length; s++) {
    const a = bounds[s];
    const b = bounds[s + 1];
    const lo = a + Math.floor((b - a) * 0.2);
    const hi = Math.max(lo + 1, b - Math.floor((b - a) * 0.2));
    const votes = {};
    for (let i = lo; i < hi; i++) {
      const L = frames[i].layout;
      if (!L) continue;
      const key = L.id + "|" + L.dir;
      votes[key] = (votes[key] || 0) + 1;
    }
    const best = Object.entries(votes).sort((x, y) => y[1] - x[1])[0];
    const prevSeg = segs[segs.length - 1];
    let [id, dir] = best ? best[0].split("|") : prevSeg ? [prevSeg.layout, prevSeg.dir] : ["full", "cols"];
    // Very short sections (1–3 frames) are too uncertain for a layout change → keep the layout
    if (prevSeg && b - a < 4) [id, dir] = [prevSeg.layout, prevSeg.dir];
    segs.push({ t: frames[a].t, i: a, layout: id, dir });
  }
  // 4. Events: layout change or cut of individual fields
  const events = [];
  segs.forEach((sg, k) => {
    const prev = segs[k - 1];
    if (!prev || prev.layout !== sg.layout || prev.dir !== sg.dir) {
      events.push({ t: sg.t, type: "layout", layout: sg.layout, dir: sg.dir });
      return;
    }
    const f = frames[sg.i];
    const specs = REGIONS[sg.layout][sg.dir];
    const ds = specs.map((sp) => regionDiff(f, sp));
    const top = Math.max(...ds);
    const groups = ds.map((d, gi) => (d > Math.max(0.07, top * 0.45) ? gi : -1)).filter((gi) => gi >= 0);
    events.push({ t: sg.t, type: "cut", groups: groups.length === specs.length ? "all" : groups });
  });
  flashes.forEach((fl) => events.push(Object.assign({ type: "flash" }, fl)));
  // 5. Snap to the beat (the sampling is coarser than the music)
  const beats = song.beats;
  let bi = 0;
  events.forEach((ev) => {
    while (bi + 1 < beats.length && beats[bi + 1] <= ev.t) bi++;
    const cands = [beats[bi], beats[bi + 1]].filter((x) => x != null);
    const near = cands.reduce((a, b) => (Math.abs(b - ev.t) < Math.abs(a - ev.t) ? b : a), ev.t);
    if (Math.abs(near - ev.t) < 0.08) ev.t = near;
  });
  events.sort((a, b) => a.t - b.t || (a.type === "layout" ? -1 : 1));

  const layoutTime = {};
  segs.forEach((sg, k) => {
    const end = segs[k + 1] ? segs[k + 1].t : song.duration;
    layoutTime[sg.layout] = (layoutTime[sg.layout] || 0) + (end - sg.t);
  });
  return {
    song,
    events,
    segs: segs.map((s, k) => ({ t: s.t, end: segs[k + 1] ? segs[k + 1].t : song.duration, layout: s.layout, dir: s.dir })),
    stats: {
      cuts: events.filter((e) => e.type !== "flash").length,
      layoutChanges: events.filter((e) => e.type === "layout").length,
      flashes: flashes.length,
      layoutTime,
      frames: frames.length,
    },
  };
}

function abortErr() {
  const e = new Error("Aborted");
  e.name = "AbortError";
  return e;
}

// For tests: measurement of a single frame
export { measure as measureFrame, GW as MEASURE_W, GH as MEASURE_H, sampleFrames, buildTemplate };
