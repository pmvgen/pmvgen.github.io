// PMV Generator: image analysis for the smart crop and for picking good moments.
// Measured on a tiny image (at most 64 px wide): skin share (simple RGB skin rule),
// contrast (edges) and – with a second frame – motion.

const MAX = 64;
let canvas = null;
let ctx = null;

function grab(el, w, h) {
  const sw = Math.max(8, Math.round(w >= h ? MAX : (MAX * w) / h));
  const sh = Math.max(8, Math.round(w >= h ? (MAX * h) / w : MAX));
  if (!canvas) {
    canvas = document.createElement("canvas");
    ctx = canvas.getContext("2d", { willReadFrequently: true });
  }
  canvas.width = sw;
  canvas.height = sh;
  ctx.drawImage(el, 0, 0, sw, sh);
  return { px: ctx.getImageData(0, 0, sw, sh).data, sw, sh };
}

const isSkin = (r, g, b) => r > 95 && g > 40 && b > 20 && r > g && r > b && r - g > 15 && Math.max(r, g, b) - Math.min(r, g, b) > 15;

// Key figures of an image: focus (0–1), skin share, contrast, grayscale image for motion
const NEUTRAL = { focus: { x: 0.5, y: 0.5 }, skin: 0, contrast: 0, gray: null, color: null, lum: null };

export function analyze(el, w, h) {
  let img;
  try {
    img = grab(el, w, h);
  } catch (e) {
    return NEUTRAL; // e.g. foreign source (tainted canvas) – then simply the center
  }
  const { px, sw, sh } = img;
  const gray = new Float32Array(sw * sh);
  let sr = 0;
  let sg = 0;
  let sb = 0;
  for (let i = 0, j = 0; i < gray.length; i++, j += 4) {
    gray[i] = 0.299 * px[j] + 0.587 * px[j + 1] + 0.114 * px[j + 2];
    sr += px[j];
    sg += px[j + 1];
    sb += px[j + 2];
  }
  const all = gray.length;
  let wx = 0;
  let wy = 0;
  let wsum = 0;
  let skin = 0;
  let edge = 0;
  for (let y = 1; y < sh - 1; y++) {
    for (let x = 1; x < sw - 1; x++) {
      const i = y * sw + x;
      const j = i * 4;
      const s = isSkin(px[j], px[j + 1], px[j + 2]) ? 1 : 0;
      const gxy = Math.abs(gray[i + 1] - gray[i - 1]) + Math.abs(gray[i + sw] - gray[i - sw]);
      const e = Math.min(1, gxy / 80);
      skin += s;
      edge += e;
      // Weight: skin counts most, plus edges; a slight preference for the center
      const cx = x / sw - 0.5;
      const cy = y / sh - 0.5;
      const prior = Math.exp(-(cx * cx + cy * cy) / 0.5);
      const wgt = (0.7 * s + 0.3 * e) * prior;
      wx += wgt * (x / sw);
      wy += wgt * (y / sh);
      wsum += wgt;
    }
  }
  const n = (sw - 2) * (sh - 2);
  return {
    focus: wsum > n * 0.02 ? { x: wx / wsum, y: wy / wsum } : { x: 0.5, y: 0.5 },
    skin: skin / n,
    contrast: edge / n,
    gray,
    color: { r: sr / all, g: sg / all, b: sb / all }, // average color (0–255)
    lum: (0.299 * sr + 0.587 * sg + 0.114 * sb) / all, // average brightness (0–255)
  };
}

// How well two frames fit together at a cut (0 = equal, larger = more different):
// color and brightness, plus where the subject sits – that's what makes a match cut
export function matchDist(a, b) {
  if (!a || !b || !a.color || !b.color) return 0.5;
  const dc = Math.hypot(a.color.r - b.color.r, a.color.g - b.color.g, a.color.b - b.color.b) / 441;
  const dl = Math.abs(a.lum - b.lum) / 255;
  const df = a.focus && b.focus ? Math.hypot(a.focus.x - b.focus.x, a.focus.y - b.focus.y) / 1.414 : 0.3;
  return 1.2 * dc + 0.8 * dl + 1.0 * df;
}

export function motion(a, b) {
  if (!a || !b || !a.gray || !b.gray || a.gray.length !== b.gray.length) return 0;
  let s = 0;
  for (let i = 0; i < a.gray.length; i++) s += Math.abs(a.gray[i] - b.gray[i]);
  return Math.min(1, s / a.gray.length / 40);
}

// How "good" a spot is: motion counts most, then skin, then contrast
export const spotScore = (a, b, bonus = 0) => 3 * motion(a, b) + 1.2 * Math.min(1, a.skin * 2.5) + 0.6 * Math.min(1, a.contrast * 3) + bonus;
