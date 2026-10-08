// Interactive: scenes with a funscript play on The Handy – like classic Stash does it, with the same
// settings (Stash's interface settings: handyKey, funscriptOffset, useStashHostedFunscript).
// The Handy API v2 (handyfeeling.com): the script is handed to the device as a CSV (uploaded to the
// Handy's own script hosting, or fetched by the device from Stash), the clock is matched with the
// Handy server, then playback follows the video: play at a position, stop, again on seeking.

// (the web version has no Stash: the connection key and the sync offset are kept in this browser)
import { store } from "./ui.js";

const API = "https://www.handyfeeling.com/api/handy/v2/";
const UPLOAD = "https://www.handyfeeling.com/api/sync/upload?local=true";
const RESYNC = 60 * 60 * 1000; // match the clock again after an hour

let cfg = null; // { handyKey, funscriptOffset, useStashHostedFunscript }
export async function interactiveConfig(force) {
  if (cfg && !force) return cfg;
  const s = store.get("handy", {});
  cfg = { handyKey: s.handyKey || "", funscriptOffset: s.funscriptOffset || 0, useStashHostedFunscript: false };
  return cfg;
}
export async function saveInteractiveConfig(patch) {
  const keyChanged = "handyKey" in patch && patch.handyKey !== (cfg || {}).handyKey;
  store.set("handy", Object.assign(store.get("handy", {}), patch));
  cfg = Object.assign({}, cfg || {}, patch);
  if (keyChanged) handy = null; // a new key → a new connection
  return cfg;
}

export const samePath = (a, b) => !!a && !!b && a.replace(/\\/g, "/").toLowerCase() === b.replace(/\\/g, "/").toLowerCase();
// "Some Clip (hard) v2.funscript" → ["some", "clip", "hard", "v2"]
export const nameWords = (str) => String(str || "").toLowerCase().replace(/\.[a-z0-9]{2,9}$/, "").split(/[^\p{L}\p{N}]+/u).filter((w) => w.length > 1);

// A funscript → the CSV the Handy wants ("ms,position" per line; inverted and range turned into 0–100)
// (invert: the menu's switch – up becomes down, on top of what the script says)
export const handyPrefs = () => Object.assign({ invert: false }, store.get("handyPrefs", {}));
function toCsv(fs) {
  const acts = (fs && fs.actions) || [];
  if (!acts.length) throw new Error("The funscript has no movements");
  const range = (v, a, b, c, d) => ((v - a) * (d - c)) / (b - a) + c;
  const flip = (fs.inverted === true) !== handyPrefs().invert;
  return acts.reduce((out, a) => {
    let pos = a.pos;
    if (fs.range) pos = range(pos, 0, fs.range, 0, 100);
    if (flip) pos = 100 - pos;
    return `${out}${Math.round(a.at)},${Math.round(pos)}\r\n`;
  }, `#Created by Stash UI ${new Date().toUTCString()}\n`);
}

class Handy {
  constructor(key) {
    this.key = key;
    this.state = "idle"; // idle | connecting | syncing | uploading | ready | error
    this.error = "";
    this.script = ""; // the funscript path that's on the device
    this.playing = false;
    this.listeners = new Set();
    const saved = store.get("handyClock", null);
    this.offset = saved && saved.key === key ? saved.offset : 0;
    this.syncedAt = saved && saved.key === key ? saved.at : 0;
  }
  set(state, error) {
    this.state = state;
    this.error = error || "";
    this.listeners.forEach((f) => f(this));
  }
  async req(method, path, body) {
    const r = await fetch(API + path, {
      method,
      headers: Object.assign({ "X-Connection-Key": this.key }, body ? { "Content-Type": "application/json" } : {}),
      body: body ? JSON.stringify(body) : undefined,
    });
    let j = {};
    try {
      j = await r.json();
    } catch (e) { /* empty answer */ }
    if (!r.ok || j.error) throw new Error((j.error && (j.error.message || j.error.name)) || `Handy server: ${r.status}`);
    return j;
  }
  // The difference between this computer's clock and the Handy server's (30 trips, outliers left out)
  async syncClock() {
    this.set("syncing");
    await this.req("GET", "servertime");
    const offs = [];
    for (let i = 0; i < 20; i++) {
      const a = Date.now();
      const { serverTime } = await this.req("GET", "servertime");
      const b = Date.now();
      offs.push(serverTime + (b - a) / 2 - b);
    }
    const mean = offs.reduce((x, y) => x + y, 0) / offs.length;
    const sd = Math.sqrt(offs.reduce((x, y) => x + (y - mean) ** 2, 0) / offs.length);
    const good = offs.filter((o) => Math.abs(o - mean) <= sd);
    this.offset = (good.length ? good : offs).reduce((x, y) => x + y, 0) / (good.length || offs.length);
    this.syncedAt = Date.now();
    store.set("handyClock", { key: this.key, offset: this.offset, at: this.syncedAt });
  }
  async connect(force) {
    this.set("connecting");
    const c = await this.req("GET", "connected");
    if (!c.connected) throw new Error("The Handy isn't online – is it switched on and connected to Wi-Fi?");
    const info = await this.req("GET", "info");
    this.info = info; // { model, hwVersion, fwVersion, fwStatus, branch }
    if (info.fwStatus === 1) throw new Error("The Handy needs a firmware update first");
    this.off = false;
    if (force || !this.syncedAt || Date.now() - this.syncedAt > RESYNC) await this.syncClock();
  }
  // Hand the scene's script to the device
  // (variant: a chosen variant's funscript, parsed – it isn't Stash's own, so it always goes the upload way)
  async load(funscriptPath, apiKey, variant) {
    // (no "already loaded" shortcut: a scene's funscript keeps its address – /scene/ID/funscript –
    // also when another file was chosen for it, so the script is always sent fresh)
    this.set("uploading");
    let url;
    if (cfg.useStashHostedFunscript && !variant) {
      // the device fetches it from Stash itself (only works when Stash can be reached from the internet)
      const u = new URL(funscriptPath.replace("/funscript", "/interactive_csv"), location.href);
      if (apiKey === undefined) apiKey = "";
      if (apiKey) u.searchParams.set("apikey", apiKey);
      u.searchParams.set("v", Date.now()); // a new address each time – the device doesn't reuse an old copy
      url = u.toString();
    } else {
      const fs = variant || (await (await fetch(funscriptPath, { credentials: "same-origin", cache: "no-store" })).json());
      const fd = new FormData();
      fd.append("syncFile", new File([toCsv(fs)], `${Math.round(Math.random() * 1e8)}.csv`), "script.csv");
      const up = await (await fetch(UPLOAD, { method: "POST", body: fd })).json();
      if (!up.url) throw new Error("The script couldn't be uploaded to the Handy server");
      url = up.url;
    }
    await this.req("PUT", "mode", { mode: 1 }); // HSSP: synced script playback
    const setup = await this.req("PUT", "hssp/setup", { url: encodeURI(url) });
    if (setup.result !== 0 && setup.result !== 1) throw new Error("The Handy couldn't load the script");
    await this.req("GET", "status").catch(() => {});
    this.script = funscriptPath;
    this.playing = false;
    this.set("ready");
  }
  async play(sec, force) {
    if (this.state !== "ready") return;
    if (force) this.from = null;
    // already playing from there (the start and the "playing" event come together) → nothing to do
    const now = performance.now();
    if (this.playing && this.from && Math.abs(this.from.sec + (now - this.from.at) / 1000 - sec) < 0.3) return;
    this.from = { sec, at: now };
    await this.req("PUT", "hssp/play", { estimatedServerTime: Math.round(Date.now() + this.offset), startTime: Math.max(0, Math.round(sec * 1000 + (cfg.funscriptOffset || 0))) });
    this.playing = true;
  }
  async stop() {
    if (this.state !== "ready" || !this.playing) return;
    this.playing = false;
    this.from = null;
    await this.req("PUT", "hssp/stop", {});
  }
  // Stroke: the part of the slide that's used (0–100)
  async getStroke() {
    const s = await this.req("GET", "slide");
    return { min: s.min ?? 0, max: s.max ?? 100 };
  }
  async setStroke(min, max) {
    await this.req("PUT", "slide", { min: Math.round(Math.min(min, max)), max: Math.round(Math.max(min, max)) });
  }
  // Disconnect: stop and leave the device alone until "Connect" (the key stays)
  async disconnect() {
    await this.stop().catch(() => {});
    this.off = true;
    this.script = "";
    this.set("idle");
  }
  async loop(on) {
    if (this.state === "ready") await this.req("PUT", "hssp/loop", { activated: !!on }).catch(() => {});
  }
}

let handy = null;
// The device for the configured key (one for the whole app), or null without a key
export async function getHandy() {
  const c = await interactiveConfig();
  if (!c.handyKey) return null;
  if (!handy || handy.key !== c.handyKey) handy = new Handy(c.handyKey);
  return handy;
}

// A test from the settings: online? firmware? clock → a message
export async function testHandy() {
  const h = await getHandy();
  if (!h) throw new Error("Enter the connection key first");
  await h.connect(true);
  h.set(h.script ? "ready" : "idle");
  return Math.round(h.offset);
}

// The player: follows the video while a scene with a script is open. Returns a stop function.
// onState(h) is told every change (for the status in the player bar).
export function attachHandy(video, scene, { apiKey, onState, loop, getVariant } = {}) {
  let h = null;
  let alive = true;
  let variant = null; // the chosen variant's funscript (parsed), or null: the scene's own
  let busy = Promise.resolve();
  // A hiccup ("device timeout", "isn't online") must not end the show for the Handy: try again a few times – first the
  // command, then connect again, load the script again and go on from where the video is
  let recovering = false;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function recover(why) {
    if (recovering || !alive || !h || h.off) return;
    recovering = true;
    try {
      for (let i = 0; i < 12 && alive && !h.off; i++) {
        h.set("connecting", "");
        await sleep(Math.min(2500 + i * 1500, 9000));
        try {
          await h.connect();
          await h.load(scene.paths.funscript, apiKey, variant);
          if (loop && loop()) await h.loop(true);
          if (!video.paused) await h.play(video.currentTime, true);
          return;
        } catch (e) {
          h.error = e.message || why;
        }
      }
      if (alive) h.set("error", h.error || why);
    } finally {
      recovering = false;
    }
  }
  const queue = (fn) =>
    (busy = busy
      .then(async () => {
        if (!alive || !h || recovering) return;
        try {
          await fn();
        } catch (e) {
          // once more right away (the request itself may just have been lost), then the full recovery
          try {
            await sleep(700);
            if (alive && h && h.state === "ready") await fn();
          } catch (e2) {
            recover(e2.message || e.message);
          }
        }
      })
      .catch(() => {}));
  const tell = (x) => alive && onState && onState(x);
  (async () => {
    h = await getHandy();
    if (!h || !alive) return tell(null);
    h.listeners.add(tell);
    if (h.off) return tell(h); // disconnected in the menu – stays so until "Connect"
    try {
      if (getVariant) variant = await getVariant().catch(() => null);
      if (!alive) return;
      if (h.state !== "ready") await h.connect();
      await h.load(scene.paths.funscript, apiKey, variant);
      if (loop && loop()) await h.loop(true);
      if (!video.paused) await h.play(video.currentTime);
    } catch (e) {
      h.set("error", e.message);
      recover(e.message);
    }
  })();
  let waitT = 0;
  const onPlay = () => (clearTimeout(waitT), queue(() => h.play(video.currentTime)));
  const onStop = () => queue(() => h.stop());
  const onWait = () => {
    clearTimeout(waitT);
    waitT = setTimeout(onStop, 900); // a short stall of the picture doesn't stop the device
  };
  const onSeeked = () => queue(() => (video.paused ? h.stop() : h.play(video.currentTime)));
  video.addEventListener("playing", onPlay);
  video.addEventListener("pause", onStop);
  video.addEventListener("waiting", onWait);
  video.addEventListener("seeking", onStop);
  video.addEventListener("seeked", onSeeked);
  video.addEventListener("ratechange", onSeeked); // (the Handy plays at 1×; it simply follows again)
  return {
    // clicking the status: connect again, match the clock, load the script again
    async retry() {
      if (!h) return;
      h.script = "";
      try {
        await h.connect(true);
        await h.load(scene.paths.funscript, apiKey, variant);
        if (!video.paused) await h.play(video.currentTime);
      } catch (e) {
        h.set("error", e.message);
      }
    },
    setLoop: (on) => queue(() => h.loop(on)),
    get device() {
      return h;
    },
    // the sync offset changed (menu): saved in Stash a moment later, applied right away
    setOffset(ms) {
      cfg.funscriptOffset = Math.round(ms);
      clearTimeout(this.saveT);
      this.saveT = setTimeout(() => saveInteractiveConfig({ funscriptOffset: cfg.funscriptOffset }).catch(() => {}), 800);
      if (h && !video.paused) queue(() => h.play(video.currentTime, true));
    },
    // invert switched: the script goes to the device again (turned around)
    async reload() {
      if (!h) return;
      h.script = "";
      await queue(async () => {
        await h.load(scene.paths.funscript, apiKey, variant);
        if (!video.paused) await h.play(video.currentTime, true);
      });
    },
    // another variant (parsed funscript; null: the scene's own): the Handy loads it and goes on from where the video is
    async setVariant(fs) {
      variant = fs || null;
      await this.reload();
    },
    async disconnect() {
      if (h) await h.disconnect();
    },
    stop() {
      alive = false;
      video.removeEventListener("playing", onPlay);
      video.removeEventListener("pause", onStop);
      clearTimeout(waitT);
      video.removeEventListener("waiting", onWait);
      video.removeEventListener("seeking", onStop);
      video.removeEventListener("seeked", onSeeked);
      video.removeEventListener("ratechange", onSeeked);
      if (h) {
        h.listeners.delete(tell);
        h.stop().catch(() => {});
      }
    },
  };
}
