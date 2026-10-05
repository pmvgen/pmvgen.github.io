// PMV Generator: image analysis for the smart crop and for picking good moments.
// Measured on a tiny image (at most 64 px wide): skin share (simple RGB skin rule),
// contrast (edges) and – with a second frame – motion.
// analyzeAsync() lets the graphics card shrink the picture first (createImageBitmap): drawing a
// 4K video frame straight into a readable canvas copies the whole frame back into memory and
// blocks the page for 25–60 ms – shrunk first, it's about 1 ms.

const MAX = 64;
let canvas = null;
let ctx = null;

const tiny = (w, h) => [Math.max(8, Math.round(w >= h ? MAX : (MAX * w) / h)), Math.max(8, Math.round(w >= h ? (MAX * h) / w : MAX))];
function surface(sw, sh) {
  if (!canvas) {
    canvas = document.createElement("canvas");
    ctx = canvas.getContext("2d", { willReadFrequently: true });
  }
  canvas.width = sw;
  canvas.height = sh;
  return ctx;
}
function grab(el, w, h) {
  const [sw, sh] = tiny(w, h);
  surface(sw, sh).drawImage(el, 0, 0, sw, sh);
  return { px: ctx.getImageData(0, 0, sw, sh).data, sw, sh };
}
let bitmapOk = typeof createImageBitmap === "function";

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
  return measure(img);
}

// Reading the shrunk picture back still waits for the graphics card (with playing 4K videos up to
// ~50 ms) – a worker does that part, so the page keeps running. Built from this file's own
// measure() (a blob, so no extra file to keep in step).
let smart = null;
function worker() {
  if (smart !== null) return smart;
  smart = false;
  if (typeof Worker !== "function" || typeof OffscreenCanvas !== "function") return smart;
  try {
    const src = `const isSkin = ${isSkin};\nconst measure = ${measure};\nlet c = null, x = null;
onmessage = (e) => {
  const { id, bmp, sw, sh } = e.data;
  let r = null;
  try {
    if (!c) { c = new OffscreenCanvas(sw, sh); x = c.getContext("2d", { willReadFrequently: true }); }
    c.width = sw; c.height = sh;
    x.drawImage(bmp, 0, 0);
    r = measure({ px: x.getImageData(0, 0, sw, sh).data, sw, sh });
  } catch (err) { /* e.g. a foreign picture */ }
  bmp.close();
  postMessage({ id, r });
};`;
    const w = new Worker(URL.createObjectURL(new Blob([src], { type: "text/javascript" })));
    const waiting = new Map();
    let seq = 0;
    w.onmessage = (e) => {
      const done = waiting.get(e.data.id);
      waiting.delete(e.data.id);
      if (done) done(e.data.r || NEUTRAL);
    };
    w.onerror = () => {
      smart = false; // fall back to the page itself
      waiting.forEach((done) => done(NEUTRAL));
      waiting.clear();
    };
    smart = {
      run: (bmp, sw, sh) =>
        new Promise((done) => {
          const id = ++seq;
          waiting.set(id, done);
          w.postMessage({ id, bmp, sw, sh }, [bmp]);
        }),
    };
  } catch (e) {
    smart = false;
  }
  return smart;
}

// The same without blocking; region = [x, y, w, h] of el (e.g. one picture of a sprite sheet)
export async function analyzeAsync(el, w, h, region) {
  const [rw, rh] = region ? [region[2], region[3]] : [w, h];
  const [sw, sh] = tiny(rw, rh);
  if (bitmapOk) {
    try {
      const opts = { resizeWidth: sw, resizeHeight: sh, resizeQuality: "low" };
      const bmp = region ? await createImageBitmap(el, region[0], region[1], region[2], region[3], opts) : await createImageBitmap(el, opts);
      const w = worker();
      if (w) return await w.run(bmp, sw, sh);
      surface(sw, sh).drawImage(bmp, 0, 0);
      bmp.close();
      return measure({ px: ctx.getImageData(0, 0, sw, sh).data, sw, sh });
    } catch (e) {
      if (e.name === "SecurityError") return NEUTRAL;
      if (e.name === "TypeError" || e.name === "NotSupportedError") bitmapOk = false; // older browser – the old way
      else return NEUTRAL; // e.g. no frame yet
    }
  }
  try {
    const x = surface(sw, sh);
    if (region) x.drawImage(el, region[0], region[1], region[2], region[3], 0, 0, sw, sh);
    else x.drawImage(el, 0, 0, sw, sh);
    return measure({ px: x.getImageData(0, 0, sw, sh).data, sw, sh });
  } catch (e) {
    return NEUTRAL;
  }
}

// Many small pictures of one image (a sprite sheet): drawn and read once, each rect measured from memory.
// rects = [[x, y, w, h], …] in the image's pixels, all the same size
export function analyzeTiles(img, rects) {
  if (!rects.length) return [];
  const [tw, th] = tiny(rects[0][2], rects[0][3]);
  const k = tw / rects[0][2];
  const W = Math.ceil(img.naturalWidth * k);
  const H = Math.ceil(img.naturalHeight * k);
  let all;
  try {
    surface(W, H).drawImage(img, 0, 0, W, H);
    all = ctx.getImageData(0, 0, W, H).data;
  } catch (e) {
    return rects.map(() => NEUTRAL);
  }
  return rects.map(([x, y]) => {
    const ox = Math.min(W - tw, Math.round(x * k));
    const oy = Math.min(H - th, Math.round(y * k));
    const px = new Uint8ClampedArray(tw * th * 4);
    for (let row = 0; row < th; row++) px.set(all.subarray(((oy + row) * W + ox) * 4, ((oy + row) * W + ox + tw) * 4), row * tw * 4);
    return measure({ px, sw: tw, sh: th });
  });
}

function measure({ px, sw, sh }) {
  const gray = new Float32Array(sw * sh);
  let sr = 0;
  let sg = 0;
  let sb = 0;
  const hist = new Float32Array(16); // brightness distribution: a cut changes it, motion inside a scene hardly does
  for (let i = 0, j = 0; i < gray.length; i++, j += 4) {
    gray[i] = 0.299 * px[j] + 0.587 * px[j + 1] + 0.114 * px[j + 2];
    hist[gray[i] >> 4]++;
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
    hist: hist.map((x) => x / all),
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

// Did the scene change between two frames (a hard cut)? The picture differs a lot AND its brightness
// distribution / average color changed – fast movement inside one scene fails the second test.
export function isCut(a, b) {
  if (!a || !b || !a.hist || !b.hist) return false;
  let h = 0;
  for (let i = 0; i < 16; i++) h += Math.abs(a.hist[i] - b.hist[i]);
  h /= 2;
  const dc = Math.hypot(a.color.r - b.color.r, a.color.g - b.color.g, a.color.b - b.color.b) / 441;
  return motion(a, b) > 0.5 && (h > 0.35 || dc > 0.18);
}

// How "good" a spot is: motion counts most, then skin, then contrast
export const spotScore = (a, b, bonus = 0) => 3 * motion(a, b) + 1.2 * Math.min(1, a.skin * 2.5) + 0.6 * Math.min(1, a.contrast * 3) + bonus;
