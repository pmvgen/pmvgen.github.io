// Several songs in a row: the show keeps going from one to the next (in order or shuffled). Each song
// is analyzed only when it's about to play – the next one already while the current one runs.
// A track is { name, sub?, load: () => Promise<song> } (song = analyzeSong() result + name).

import { analyzeSong } from "./beats.js";

export const AUDIO_RE = /\.(mp3|m4a|aac|wav|ogg|oga|opus|flac|webm)$/i;

// Songs from dropped/chosen files and folders (folders are read with their subfolders)
export async function filesFromDrop(dataTransfer) {
  const out = [];
  const items = [...(dataTransfer.items || [])].map((i) => (i.webkitGetAsEntry ? i.webkitGetAsEntry() : null)).filter(Boolean);
  if (!items.length) return [...(dataTransfer.files || [])];
  const walk = async (e) => {
    if (e.isFile) out.push(await new Promise((res, rej) => e.file(res, rej)));
    else if (e.isDirectory) {
      const reader = e.createReader();
      for (;;) {
        const batch = await new Promise((res, rej) => reader.readEntries(res, rej));
        if (!batch.length) break;
        for (const x of batch) await walk(x);
      }
    }
  };
  for (const e of items) await walk(e);
  return out;
}

export const isSongFile = (f) => /^audio\//.test(f.type) || AUDIO_RE.test(f.name);

export function fileTrack(f) {
  const name = f.name.replace(/\.[^.]+$/, "");
  return {
    name,
    load: async () => {
      const r = await analyzeSong(await f.arrayBuffer());
      return Object.assign(r, { name });
    },
  };
}

export class Playlist {
  constructor(tracks, { shuffle = false, repeat = true } = {}) {
    this.tracks = tracks;
    this.repeat = repeat;
    this.order = tracks.map((_, i) => i);
    this.pos = 0;
    this.cache = new Map(); // track index → Promise<song>
    this.setShuffle(shuffle, true);
  }
  setShuffle(on, fresh) {
    this.shuffle = on;
    const cur = fresh ? null : this.order[this.pos];
    this.order = this.tracks.map((_, i) => i);
    if (on) {
      for (let i = this.order.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [this.order[i], this.order[j]] = [this.order[j], this.order[i]];
      }
      // The song that plays stays the current one
      if (cur != null) this.order.splice(this.order.indexOf(cur), 1), this.order.unshift(cur);
    }
    this.pos = 0;
  }
  get size() {
    return this.tracks.length;
  }
  current() {
    return this.tracks[this.order[this.pos]];
  }
  // A song, analyzed (once); failures are remembered so a broken file is skipped
  song(k) {
    const idx = this.order[k];
    if (!this.cache.has(idx)) this.cache.set(idx, this.tracks[idx].load());
    return this.cache.get(idx);
  }
  // Drop analyses we won't need soon (they hold the whole decoded sound)
  trim() {
    const keep = new Set([this.pos - 1, this.pos, this.pos + 1].map((k) => this.order[(k + this.size) % this.size]));
    [...this.cache.keys()].forEach((i) => keep.has(i) || this.cache.delete(i));
  }
  async load(k) {
    const s = await this.song(k);
    if (this.size > 1) this.song((k + 1) % this.size).catch(() => {}); // the next one in the background
    return s;
  }
  async first() {
    return this.load(this.pos);
  }
  hasNext() {
    return this.repeat || this.pos < this.size - 1;
  }
  async step(dir) {
    if (dir > 0 && !this.hasNext()) return null;
    this.pos = (this.pos + dir + this.size) % this.size;
    this.trim();
    return this.load(this.pos);
  }
}
