// Plex: sign in the way Plex apps do (a PIN confirmed on plex.tv – no password passes through here),
// find your server, and then either follow what's playing (the generator becomes a visualizer for
// whatever Plex player you use) or take songs from your Plex playlists / music library.
// Everything talks straight from the browser to plex.tv and your server; the token stays in this browser.

import { analyzeFile } from "./beats.js";

// (overridable for tests)
const tv = () => window.PMVGEN_PLEX_TV || "https://plex.tv";
const app = () => window.PMVGEN_PLEX_APP || "https://app.plex.tv";
const KEY = "pmvgen.plex"; // { token, clientId, server: { name, uri, token } }

function load() {
  try {
    return JSON.parse(localStorage.getItem(KEY) || "{}");
  } catch (e) {
    return {};
  }
}
function save(v) {
  try {
    localStorage.setItem(KEY, JSON.stringify(v));
  } catch (e) { /* blocked – works for this visit only */ }
}
export const plexState = () => load();
export function plexForget() {
  const v = load();
  save({ clientId: v.clientId });
}
function clientId() {
  const v = load();
  if (!v.clientId) {
    v.clientId = "pmvgen-" + Math.random().toString(36).slice(2) + Date.now().toString(36);
    save(v);
  }
  return v.clientId;
}
// Plex parameters as query parameters – then the browser needs no extra permission round trip
const ident = () => `X-Plex-Product=PMV%20Generator&X-Plex-Client-Identifier=${encodeURIComponent(clientId())}&X-Plex-Version=1&X-Plex-Platform=Web`;
async function getJson(url, ms = 10000) {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), ms);
  try {
    const r = await fetch(url, { headers: { Accept: "application/json" }, signal: ctl.signal });
    if (r.status === 401) throw Object.assign(new Error("Plex says: not allowed (sign in again)"), { code: 401 });
    if (!r.ok) throw new Error(`Plex: HTTP ${r.status}`);
    return await r.json();
  } finally {
    clearTimeout(timer);
  }
}

// ---------- Sign in ----------
// Opens plex.tv in a small window; resolves with the token once you've confirmed there.
export async function plexSignIn(onWaiting) {
  // The window opens right away (still inside the click – otherwise popup blockers step in)
  const win = window.open("", "plexauth", "width=520,height=720");
  const r = await fetch(`${tv()}/api/v2/pins?strong=true&${ident()}`, { method: "POST", headers: { Accept: "application/json" } }).catch((e) => ({ ok: false, status: e.message }));
  if (!r.ok) {
    if (win) win.close();
    throw new Error(`plex.tv can't be reached (${r.status})`);
  }
  const pin = await r.json();
  const url = `${app()}/auth#?clientID=${encodeURIComponent(clientId())}&code=${encodeURIComponent(pin.code)}&context%5Bdevice%5D%5Bproduct%5D=PMV%20Generator`;
  if (win) win.location.href = url;
  onWaiting && onWaiting(url, !!win);
  const t0 = Date.now();
  while (Date.now() - t0 < 5 * 60 * 1000) {
    await new Promise((res) => setTimeout(res, 1500));
    const p = await getJson(`${tv()}/api/v2/pins/${pin.id}?${ident()}`).catch(() => null);
    if (p && p.authToken) {
      try {
        if (win && !win.closed) win.close();
      } catch (e) {}
      const v = load();
      v.token = p.authToken;
      save(v);
      return p.authToken;
    }
    if (win && win.closed && Date.now() - t0 > 3000) {
      // Window closed without confirming – look one last time
      const last = await getJson(`${tv()}/api/v2/pins/${pin.id}?${ident()}`).catch(() => null);
      if (last && last.authToken) {
        const v = load();
        v.token = last.authToken;
        save(v);
        return last.authToken;
      }
      throw new Error("Plex sign-in was closed");
    }
  }
  throw new Error("Plex sign-in took too long");
}

// Your servers, each with the connection that answers (on a https page only https ones work)
export async function plexServers() {
  const v = load();
  if (!v.token) throw new Error("Not signed in to Plex");
  const list = await getJson(`${tv()}/api/v2/resources?includeHttps=1&includeRelay=1&${ident()}&X-Plex-Token=${encodeURIComponent(v.token)}`);
  return (Array.isArray(list) ? list : []).filter((r) => String(r.provides || "").includes("server"));
}
export async function plexConnect(server) {
  const https = location.protocol === "https:";
  const conns = (server.connections || []).filter((c) => !https || c.protocol === "https");
  // Local first, relay last – the first one that answers wins
  conns.sort((a, b) => (b.local ? 1 : 0) - (a.local ? 1 : 0) || (a.relay ? 1 : 0) - (b.relay ? 1 : 0));
  const token = server.accessToken || load().token;
  const tries = conns.map((c) => getJson(`${c.uri}/identity?X-Plex-Token=${encodeURIComponent(token)}`, 5000).then(() => c.uri));
  const uri = await Promise.any(tries).catch(() => null);
  if (!uri) throw new Error(https ? "Your Plex server can't be reached over https from here – try the Stash plugin on your PC, or check Remote Access in Plex" : "Your Plex server can't be reached");
  const v = load();
  v.server = { name: server.name, uri, token };
  save(v);
  return v.server;
}

// ---------- Talking to your server ----------
export class PlexClient {
  constructor(server) {
    this.uri = server.uri;
    this.token = server.token;
    this.name = server.name;
  }
  url(path, extra = "") {
    return `${this.uri}${path}${path.includes("?") ? "&" : "?"}X-Plex-Token=${encodeURIComponent(this.token)}${extra ? "&" + extra : ""}`;
  }
  get(path) {
    return getJson(this.url(path)).then((d) => d.MediaContainer || {});
  }
  // What's playing right now – music only
  async nowPlaying(preferPlayer) {
    const mc = await this.get("/status/sessions");
    const at = performance.now();
    const tracks = (mc.Metadata || []).filter((m) => m.type === "track");
    const m = tracks.find((x) => preferPlayer && x.Player && x.Player.machineIdentifier === preferPlayer) || tracks.find((x) => x.Player && x.Player.state === "playing") || tracks[0];
    if (!m) return null;
    return {
      key: String(m.ratingKey),
      title: m.title,
      artist: m.grandparentTitle || m.originalTitle || "",
      album: m.parentTitle || "",
      duration: (m.duration || 0) / 1000,
      pos: (m.viewOffset || 0) / 1000,
      playing: !m.Player || m.Player.state !== "paused",
      player: m.Player ? m.Player.machineIdentifier : "",
      playerName: m.Player ? m.Player.title || m.Player.product || "" : "",
      meta: m,
      at,
    };
  }
  async playlists() {
    const mc = await this.get("/playlists?playlistType=audio");
    return (mc.Metadata || []).map((p) => ({ key: String(p.ratingKey), title: p.title, count: p.leafCount || 0 }));
  }
  async playlistTracks(key) {
    const mc = await this.get(`/playlists/${key}/items`);
    return (mc.Metadata || []).filter((m) => m.type === "track");
  }
  // "All my music, shuffled": random tracks from every music library
  async shuffleAll(n = 200) {
    const secs = ((await this.get("/library/sections")).Directory || []).filter((d) => d.type === "artist");
    const all = [];
    for (const s of secs) {
      const mc = await this.get(`/library/sections/${s.key}/all?type=10&sort=random&X-Plex-Container-Start=0&X-Plex-Container-Size=${n}`);
      all.push(...(mc.Metadata || []));
    }
    return all;
  }
  // A track's sound, analyzed: the original file, or an MP3 from Plex when the browser can't read it
  async song(m) {
    const name = [m.title, m.grandparentTitle || m.originalTitle].filter(Boolean).join(" – ");
    const part = ((m.Media || [])[0] || {}).Part || [];
    let buf = null;
    if (part[0] && part[0].key) {
      try {
        const r = await fetch(this.url(part[0].key));
        if (r.ok) return Object.assign(await analyzeFile(await r.blob()), { name });
      } catch (e) {
        buf = e;
      }
    }
    const mp3 = this.url("/music/:/transcode/universal/start.mp3", `path=${encodeURIComponent("/library/metadata/" + m.ratingKey)}&mediaIndex=0&partIndex=0&protocol=http&directPlay=0&directStream=0&session=${Date.now().toString(36)}&${ident()}`);
    const r = await fetch(mp3);
    if (!r.ok) throw buf || new Error(`Plex couldn't deliver “${name}” (HTTP ${r.status})`);
    return Object.assign(await analyzeFile(await r.blob()), { name });
  }
}

// Plex tracks as playlist entries (see music.js)
export const plexTracks = (client, list) =>
  list.map((m) => ({ name: m.title, sub: m.grandparentTitle || "", load: () => client.song(m) }));

// ---------- Following what's playing ----------
// Asks the server twice a second; the generator gets the song (analyzed once per track), the position
// and whether it's playing. Plex players report their position every few seconds – in between the
// time runs on here, and it's corrected when a fresh report comes in.
export class PlexFollow {
  constructor(client) {
    this.client = client;
    this.player = null;
    this.trackKey = null;
    this.lastPos = null;
    this.loading = null;
    this.gen = null;
  }
  // The track that plays now, analyzed – to start the show with
  async first(onStatus) {
    const stop = () => {
      if (this.cancelled) throw Object.assign(new Error("Cancelled"), { cancelled: true });
    };
    for (;;) {
      stop();
      const np = await this.client.nowPlaying();
      stop();
      if (np) {
        this.player = np.player;
        onStatus && onStatus(`Loading “${np.title}” …`);
        const song = await this.client.song(np.meta);
        stop();
        this.trackKey = np.key;
        this.lastPos = np.pos;
        return { song, np };
      }
      onStatus && onStatus("Nothing is playing on Plex – start a song there");
      await new Promise((res) => setTimeout(res, 2000));
    }
  }
  // About twice a second, a little irregular: then some polls land right after a report – the
  // generator keeps the tightest one (see Generator.follow)
  attach(gen) {
    this.gen = gen;
    const next = () => {
      this.timer = setTimeout(() => this.tick().finally(() => this.gen && next()), 350 + Math.random() * 300);
    };
    next();
  }
  detach() {
    clearTimeout(this.timer);
    this.gen = null;
  }
  async tick() {
    if (!this.gen || this.busy) return;
    this.busy = true;
    try {
      const np = await this.client.nowPlaying(this.player);
      if (!this.gen) return;
      if (!np) return this.gen.follow({ playing: false, idle: true });
      if (np.player) this.player = np.player;
      if (np.key !== this.trackKey) {
        // Another song: analyze it, meanwhile the show keeps its rhythm
        if (this.loading === np.key) return;
        this.loading = np.key;
        this.gen.follow({ loading: `${np.title}${np.artist ? " – " + np.artist : ""}` });
        const song = await this.client.song(np.meta).catch(() => null);
        this.loading = null;
        if (!this.gen || !song) return;
        this.trackKey = np.key;
        this.lastPos = np.pos;
        const again = await this.client.nowPlaying(this.player).catch(() => np);
        this.gen.follow({ song, pos: again && again.key === np.key ? again.pos : np.pos, playing: np.playing, fresh: true, player: np.player });
        return;
      }
      // Same song: a changed position is a fresh report
      const fresh = np.pos !== this.lastPos;
      this.lastPos = np.pos;
      this.gen.follow({ pos: np.pos, playing: np.playing, fresh, player: np.player });
    } catch (e) {
      this.gen && this.gen.follow({ error: e.message });
    } finally {
      this.busy = false;
    }
  }
}
