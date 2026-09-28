// PMV Generator: layouts (split screens) and effects for the canvas.
// The generator decides WHEN something happens (beats, energy, drops) – this file decides HOW it looks.

// level = how "full" a layout feels; the generator picks by energy (calm → 1, drop → 4)
export const LAYOUTS = {
  full: { name: "Fullscreen", groups: 1, level: 1, hint: "One clip over everything" },
  kaleido: { name: "Kaleidoscope", groups: 1, level: 2, hint: "One clip, mirrored four times" },
  duo: { name: "2-way", groups: 2, level: 2, hint: "Two clips side by side" },
  trim: { name: "3-way mirrored", groups: 2, level: 3, hint: "Left and right the same clip mirrored, a different one in the middle" },
  tri: { name: "3-way", groups: 3, level: 3, hint: "Three different clips" },
  quad: { name: "4-way", groups: 4, level: 4, hint: "Four fields" },
};

// Fields of a layout: rectangle, group (which clip) and mirroring.
// dir "cols" = side by side (columns), "rows" = stacked (rows).
export function slotsFor(id, W, H, dir = "cols") {
  const wide = dir !== "rows";
  const split = (n, groups, mirror) =>
    groups.map((g, i) => {
      const a = Math.round((i * (wide ? W : H)) / n);
      const b = Math.round(((i + 1) * (wide ? W : H)) / n);
      return wide ? { x: a, y: 0, w: b - a, h: H, g, fx: !!mirror[i], fy: false } : { x: 0, y: a, w: W, h: b - a, g, fx: !!mirror[i], fy: false };
    });
  const quad = (groupsOf, mirrorOf) =>
    [0, 1, 2, 3].map((i) => {
      const x0 = Math.round(((i % 2) * W) / 2);
      const y0 = Math.round((Math.floor(i / 2) * H) / 2);
      return { x: x0, y: y0, w: Math.round(W / 2), h: Math.round(H / 2), g: groupsOf(i), fx: mirrorOf(i)[0], fy: mirrorOf(i)[1] };
    });
  switch (id) {
    case "duo":
      return split(2, [0, 1], [0, 0]);
    case "trim":
      return split(3, [0, 1, 0], [0, 0, 1]);
    case "tri":
      return split(3, [0, 1, 2], [0, 0, 0]);
    case "quad":
      return quad((i) => i, () => [false, false]);
    case "kaleido":
      return quad(() => 0, (i) => [i % 2 === 1, i >= 2]);
    default:
      return [{ x: 0, y: 0, w: W, h: H, g: 0, fx: false, fy: false }];
  }
}

export const aspectOfGroup = (slots, g) => {
  const s = slots.find((x) => x.g === g) || slots[0];
  return s.w / s.h;
};

// Color looks: a filter on each clip + a tint over the whole picture (soft-light)
export const LOOKS = {
  none: { filter: "", tint: null },
  warm: { filter: "sepia(.22) saturate(1.2) contrast(1.04)", tint: "rgba(255, 140, 60, .22)" },
  pink: { filter: "saturate(1.15) contrast(1.05)", tint: "rgba(255, 62, 138, .3)" },
  cold: { filter: "saturate(.85) contrast(1.06)", tint: "rgba(60, 140, 255, .26)" },
  vivid: { filter: "saturate(1.7) contrast(1.12)", tint: null },
  bw: { filter: "grayscale(1) contrast(1.15)", tint: null },
  noir: { filter: "grayscale(1) contrast(1.5) brightness(.88)", tint: null, vignette: true },
};

export class Compositor {
  constructor(canvas, S) {
    this.c = canvas;
    this.g = canvas.getContext("2d");
    this.W = canvas.width;
    this.H = canvas.height;
    this.S = S;
    this.fx = S.fx;
    this.t = { flash: -9, flashA: 0, flashColor: "#fff", shake: -9, glitch: -9, rgb: -9, rgbAmt: 0, rgbDur: 0.1, invert: -9, tunnel: -9, tunnelDur: 0.5, strobe: -9, text: -9, word: "" };
    this.words = String(S.words || "").split(/[,;\n]+/).map((w) => w.trim()).filter(Boolean);
    this.look = LOOKS[S.look] || LOOKS.none;
    this.title = ""; // intro/outro – set by the generator
    this.duration = 0;
    this.credits = () => "";
    const off = (w, h) => {
      const c = document.createElement("canvas");
      c.width = w;
      c.height = h;
      return c;
    };
    this.prev = off(this.W, this.H); // for echo
    this.red = off(this.W, this.H); // for RGB split
    this.cyan = off(this.W, this.H);
    this.grain = makeGrain(off(256, 256));
    this.lines = makeScanlines(off(4, 4));
  }

  // ---------- Triggers (from the generator) ----------

  flash(t, a, color, force) {
    if (!this.fx.flash && !force) return;
    this.t.flash = t;
    this.t.flashA = a;
    this.t.flashColor = color;
  }
  shake(t, dur) {
    if (this.fx.shake) this.t.shake = t + dur;
  }
  glitch(t, dur) {
    if (this.fx.glitch) this.t.glitch = t + dur;
  }
  rgb(t, amount, dur) {
    if (!this.fx.rgb) return;
    this.t.rgb = t;
    this.t.rgbAmt = amount;
    this.t.rgbDur = dur;
  }
  invert(t, dur) {
    if (this.fx.invert) this.t.invert = t + dur;
  }
  tunnel(t, dur) {
    if (!this.fx.tunnel) return;
    this.t.tunnel = t;
    this.t.tunnelDur = dur;
  }
  strobe(t) {
    if (this.fx.strobe) this.t.strobe = t;
  }
  text(t) {
    if (!this.fx.text || !this.words.length) return;
    this.t.text = t;
    this.t.word = this.words[Math.floor(Math.random() * this.words.length)];
  }

  // ---------- One frame ----------
  // st: { t, slots, groups (media per group), cutT (per group), energy, beatT, beatAmt, stutterT }

  draw(st) {
    const { g, W, H, fx } = this;
    const t = st.t;
    const e = st.energy;
    g.save();
    g.fillStyle = "#000";
    g.fillRect(0, 0, W, H);

    let dx = 0;
    let dy = 0;
    if (t < this.t.shake) {
      const a = 6 + 16 * e;
      dx = (Math.random() - 0.5) * a;
      dy = (Math.random() - 0.5) * a;
    }
    const hue = fx.hue ? `hue-rotate(${Math.round((t * (30 + 110 * e)) % 360)}deg) saturate(1.3)` : "";
    const pulse = fx.zoom ? (st.beatAmt || 0) * Math.exp(-(t - st.beatT) * 9) : 0;

    st.slots.forEach((s, i) => {
      const m = st.groups[s.g];
      if (!m || !m.w || !m.h) return;
      const since = t - (st.cutT[s.g] || 0);
      let zoom = 1 + pulse;
      if (m.kind === "image" && fx.kenburns) zoom += 0.07 * Math.min(1, since / 4);
      let ox = dx;
      let oy = dy;
      let filter = hue;
      if (this.look.filter) filter += " " + this.look.filter;
      // Even out brightness: every clip towards a medium brightness
      if (this.S.lookEven && m.sig && m.sig.lum != null) filter += ` brightness(${Math.max(0.8, Math.min(1.4, 118 / Math.max(20, m.sig.lum))).toFixed(2)})`;
      // Transition: the new clip whips into the field with motion blur
      if (fx.whip && since < 0.16) {
        const p = 1 - since / 0.16;
        const dir = (s.g + st.cutCount) % 2 ? 1 : -1;
        if (s.w >= s.h * 0.9) ox += dir * p * p * s.w * 0.6;
        else oy += dir * p * p * s.h * 0.6;
        filter += ` blur(${(p * 10).toFixed(1)}px)`;
      }
      // Zoom-in entry: the new clip shoots into the field – alternating from big (in) and from small (out)
      if (fx.zoomin && since < 0.3) {
        const p = since / 0.3;
        const ease = 1 - Math.pow(1 - p, 3);
        const from = (m.zoomDir || 1) > 0 ? 1.75 : 0.45;
        zoom *= from + (1 - from) * ease;
        if (from < 1 && p < 0.35) filter += ` brightness(${(1 + 0.6 * (1 - p / 0.35)).toFixed(2)})`; // short flare when zooming out
      }
      if (this.S.fit === "contain") drawIn(g, m, s, "cover", 1.15, 0, 0, (filter + " blur(22px) brightness(.45)").trim());
      drawIn(g, m, s, this.S.fit, zoom, ox, oy, filter.trim(), fx.kenburns);
    });

    // Color look: tint over everything, noir with dark corners
    if (this.look.tint) {
      g.globalCompositeOperation = "soft-light";
      g.fillStyle = this.look.tint;
      g.fillRect(0, 0, W, H);
      g.globalCompositeOperation = "source-over";
    }
    if (this.look.vignette) vignette(g, W, H, 0.6);

    // Dividers between fields, glowing to the beat
    if (st.slots.length > 1) {
      const glow = Math.exp(-(t - st.beatT) * 7);
      g.fillStyle = "#0a0309";
      st.slots.forEach((s) => {
        if (s.x > 0) g.fillRect(s.x - 2, s.y, 4, s.h);
        if (s.y > 0) g.fillRect(s.x, s.y - 2, s.w, 4);
      });
      if (fx.lines && glow > 0.05) {
        g.fillStyle = `rgba(255, 62, 138, ${0.9 * glow})`;
        st.slots.forEach((s) => {
          if (s.x > 0) g.fillRect(s.x - 1, s.y, 2, s.h);
          if (s.y > 0) g.fillRect(s.x, s.y - 1, s.w, 2);
        });
      }
    }

    // Echo: the last frame lingers as a slightly enlarged ghost
    if (fx.echo && e > 0.45) {
      g.globalAlpha = 0.16 + 0.2 * e; // not too strong: the echo records itself too
      g.drawImage(this.prev, -W * 0.02, -H * 0.02, W * 1.04, H * 1.04);
      g.globalAlpha = 1;
    }

    // Tunnel: the picture sits shrunk inside itself
    const tp = (t - this.t.tunnel) / this.t.tunnelDur;
    if (tp >= 0 && tp < 1) {
      const grow = 1 - tp;
      for (const f of [0.74, 0.52, 0.34]) {
        const s = f + (1 - f) * 0.12 * tp;
        const w = W * s;
        const h = H * s;
        g.globalAlpha = 0.9 * grow + 0.1;
        g.drawImage(this.c, (W - w) / 2, (H - h) / 2, w, h);
        g.strokeStyle = "#ff3e8a";
        g.lineWidth = 3;
        g.strokeRect((W - w) / 2, (H - h) / 2, w, h);
      }
      g.globalAlpha = 1;
    }

    // Glitch: stripes slip
    if (t < this.t.glitch) {
      const n = 5 + Math.floor(Math.random() * 6);
      for (let i = 0; i < n; i++) {
        const y = Math.random() * H;
        const h = 4 + Math.random() * H * 0.09;
        g.drawImage(this.c, 0, y, W, h, (Math.random() - 0.5) * W * 0.14, y, W, h);
      }
      g.globalCompositeOperation = "screen";
      g.fillStyle = "rgba(255, 62, 138, .16)";
      g.fillRect(0, 0, W, H);
      g.globalCompositeOperation = "source-over";
    }

    // RGB split: red and cyan drift apart
    const rp = (t - this.t.rgb) / this.t.rgbDur;
    if (rp >= 0 && rp < 1) this.split(this.t.rgbAmt * (1 - rp) * (1 - rp) + 1);

    // Negative
    if (t < this.t.invert) {
      g.globalCompositeOperation = "difference";
      g.fillStyle = "#fff";
      g.fillRect(0, 0, W, H);
      g.globalCompositeOperation = "source-over";
    }

    // Flash
    const fa = this.t.flashA * Math.exp(-(t - this.t.flash) * 9);
    if (fa > 0.01) {
      g.globalAlpha = fa;
      g.fillStyle = this.t.flashColor;
      g.fillRect(0, 0, W, H);
      g.globalAlpha = 1;
    }

    // Strobe: short white
    if (t - this.t.strobe >= 0 && t - this.t.strobe < 0.045) {
      g.fillStyle = "rgba(255, 255, 255, .92)";
      g.fillRect(0, 0, W, H);
    }

    // Text
    const tt = t - this.t.text;
    if (tt >= 0 && tt < 0.42 && this.t.word) this.drawWord(this.t.word, tt);

    // VHS: noise, scanlines, dark corners
    if (fx.vhs) {
      g.globalAlpha = 0.1;
      const ox = Math.floor(Math.random() * 256);
      const oy = Math.floor(Math.random() * 256);
      g.fillStyle = g.createPattern(this.grain, "repeat");
      g.translate(-ox, -oy);
      g.fillRect(ox, oy, W, H);
      g.translate(ox, oy);
      g.globalAlpha = 1;
      g.fillStyle = g.createPattern(this.lines, "repeat");
      g.fillRect(0, 0, W, H);
      vignette(g, W, H, 0.55);
    }
    g.restore();

    // Intro and outro lie on top of everything
    if (this.S.intro && t < INTRO) this.drawIntro(t);
    if (this.S.outro && this.duration > 12 && t > this.duration - OUTRO) this.drawOutro(t - (this.duration - OUTRO));

    if (fx.echo) {
      const p = this.prev.getContext("2d");
      p.clearRect(0, 0, W, H);
      p.drawImage(this.c, 0, 0);
    }
  }

  // Intro: the title slams in on a pink hatched bar, the picture below darkened; fades out after ~3 s
  drawIntro(t) {
    const { g, W, H } = this;
    const out = t > INTRO - 0.6 ? (INTRO - t) / 0.6 : 1; // fade out
    g.save();
    g.globalAlpha = Math.max(0, out);
    g.fillStyle = `rgba(10, 3, 9, ${t < 0.25 ? 1 - t * 1.6 : 0.6})`;
    g.fillRect(0, 0, W, H);
    // The bar slides in from the left
    const bar = Math.min(1, t / 0.35);
    const bh = Math.min(W, H) * 0.3;
    g.translate(W / 2, H / 2);
    g.rotate(-0.08);
    g.fillStyle = "#ff3e8a";
    g.fillRect(-W * 0.75, -bh / 2, W * 1.5 * (1 - Math.pow(1 - bar, 3)), bh);
    g.fillStyle = "rgba(35, 15, 31, .35)";
    for (let x = -W * 0.75; x < W * 0.75 * (2 * bar - 1) + W * 0.02; x += 18) {
      g.beginPath();
      g.moveTo(x, -bh / 2);
      g.lineTo(x + 8, -bh / 2);
      g.lineTo(x + 8 - bh * 0.4, bh / 2);
      g.lineTo(x - bh * 0.4, bh / 2);
      g.fill();
    }
    g.restore();
    if (t > 0.2) {
      g.save();
      g.globalAlpha = Math.max(0, out);
      this.drawWord(this.title || "PMV", Math.min(0.29, (t - 0.2) * 0.5), 0.3, true);
      g.restore();
    }
  }

  // Outro: the picture darkens, title and details, the last second black
  drawOutro(p) {
    const { g, W, H } = this;
    const k = Math.min(1, p / (OUTRO - 1));
    g.save();
    g.fillStyle = `rgba(10, 3, 9, ${0.25 + 0.75 * k})`;
    g.fillRect(0, 0, W, H);
    g.globalAlpha = p < 0.4 ? p / 0.4 : p > OUTRO - 1.2 ? Math.max(0, (OUTRO - 0.3 - p) / 0.9) : 1;
    g.textAlign = "center";
    g.textBaseline = "middle";
    const size = Math.min(W, H) * 0.12;
    g.font = `700 ${size}px "Bahnschrift Condensed", Bahnschrift, "Arial Narrow", Impact, sans-serif`;
    g.fillStyle = "#fbeff4";
    const title = String(this.title || "PMV").toUpperCase();
    const mw = g.measureText(title).width;
    if (mw > W * 0.86) g.font = `700 ${(size * W * 0.86) / mw}px "Bahnschrift Condensed", Bahnschrift, "Arial Narrow", Impact, sans-serif`;
    g.fillText(title, W / 2, H / 2 - size * 0.3);
    g.fillStyle = "#ff3e8a";
    g.fillRect(W / 2 - size * 1.2, H / 2 + size * 0.35, size * 2.4, Math.max(3, size * 0.06));
    g.font = `600 ${size * 0.28}px Bahnschrift, "Segoe UI", sans-serif`;
    g.fillStyle = "rgba(251, 239, 244, .75)";
    g.fillText(this.credits(), W / 2, H / 2 + size * 0.8);
    g.restore();
  }

  split(a) {
    const { g, W, H } = this;
    for (const [cv, color] of [[this.red, "#f00"], [this.cyan, "#0ff"]]) {
      const x = cv.getContext("2d");
      x.globalCompositeOperation = "source-over";
      x.drawImage(this.c, 0, 0);
      x.globalCompositeOperation = "multiply";
      x.fillStyle = color;
      x.fillRect(0, 0, W, H);
    }
    g.fillStyle = "#000";
    g.fillRect(0, 0, W, H);
    g.globalCompositeOperation = "lighter";
    g.drawImage(this.red, -a, 0);
    g.drawImage(this.cyan, a, 0);
    g.globalCompositeOperation = "source-over";
  }

  drawWord(word, tt, scale, hold) {
    const { g, W, H } = this;
    const text = word.toUpperCase();
    let size = Math.min(W, H) * (scale || 0.34);
    g.save();
    g.font = `700 ${size}px "Bahnschrift Condensed", Bahnschrift, "Arial Narrow", Impact, sans-serif`;
    const maxW = W * 0.86;
    const mw = g.measureText(text).width;
    if (mw > maxW) {
      size *= maxW / mw;
      g.font = `700 ${size}px "Bahnschrift Condensed", Bahnschrift, "Arial Narrow", Impact, sans-serif`;
    }
    const punch = 1 + 0.35 * Math.exp(-tt * 18);
    const fade = hold ? 1 : tt > 0.3 ? 1 - (tt - 0.3) / 0.12 : 1;
    g.globalAlpha *= Math.max(0, fade);
    g.translate(W / 2, H / 2);
    g.rotate(-0.1);
    g.transform(1, 0, -0.18, 1, 0, 0); // slanted like a sound effect
    g.scale(punch, punch);
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.lineJoin = "round";
    g.fillStyle = "#ff3e8a";
    g.fillText(text, size * 0.05, size * 0.05);
    g.lineWidth = size * 0.1;
    g.strokeStyle = "#230f1f";
    g.strokeText(text, 0, 0);
    g.fillStyle = "#fbeff4";
    g.fillText(text, 0, 0);
    g.restore();
  }
}

// Draw media into a field (fill or fit), with mirroring, zoom and offset
const INTRO = 3.2; // seconds
const OUTRO = 4.5;

function vignette(g, W, H, a) {
  const v = g.createRadialGradient(W / 2, H / 2, Math.min(W, H) * 0.35, W / 2, H / 2, Math.max(W, H) * 0.75);
  v.addColorStop(0, "rgba(0,0,0,0)");
  v.addColorStop(1, `rgba(0,0,0,${a})`);
  g.fillStyle = v;
  g.fillRect(0, 0, W, H);
}

function drawIn(g, m, s, fit, zoom, ox, oy, filter, drift = true) {
  const r = fit === "contain" ? Math.min(s.w / m.w, s.h / m.h) : Math.max(s.w / m.w, s.h / m.h);
  const w = m.w * r * zoom;
  const h = m.h * r * zoom;
  g.save();
  g.beginPath();
  g.rect(s.x, s.y, s.w, s.h);
  g.clip();
  g.translate(s.x + s.w / 2 + ox, s.y + s.h / 2 + oy);
  g.scale(s.fx ? -1 : 1, s.fy ? -1 : 1);
  if (filter) g.filter = filter;
  if (m.kind === "image" && m.kb && drift) g.translate(m.kb * 18 * Math.min(1, (performance.now() - m.shownAt) / 4000), 0);
  // Smart crop: shift the picture so that its focus sits as central as possible –
  // only so far that the field stays filled (mirroring works automatically through scale())
  let sx = 0;
  let sy = 0;
  if (m.focus && fit !== "contain") {
    const rx = Math.max(0, (w - s.w) / 2);
    const ry = Math.max(0, (h - s.h) / 2);
    sx = Math.max(-rx, Math.min(rx, (0.5 - m.focus.x) * w));
    sy = Math.max(-ry, Math.min(ry, (0.5 - m.focus.y) * h));
  }
  g.drawImage(m.el, -w / 2 + sx, -h / 2 + sy, w, h);
  g.restore();
}

function makeGrain(c) {
  const x = c.getContext("2d");
  const img = x.createImageData(c.width, c.height);
  for (let i = 0; i < img.data.length; i += 4) {
    const v = Math.random() * 255;
    img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
    img.data[i + 3] = 255;
  }
  x.putImageData(img, 0, 0);
  return c;
}

function makeScanlines(c) {
  const x = c.getContext("2d");
  x.fillStyle = "rgba(0, 0, 0, .22)";
  x.fillRect(0, 0, 4, 2);
  return c;
}
