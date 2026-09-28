// Local clip library: the videos and images you pick or drop – instead of a Stash library.
// Nothing is uploaded: files are played straight from disk through object URLs.
// Width, height and duration are read in the background (needed for "Portrait only" etc.).

const VIDEO = /\.(mp4|m4v|webm|mkv|mov|ogv)$/i;
const IMAGE = /\.(jpe?g|png|webp|gif|avif|bmp)$/i;
const LOOSE = "Single files";

const items = []; // { id, kind: "video"|"image", file, name, path, folder, w, h, dur, url, probed, bad }
const byKey = new Set();
let nextId = 1;

// pmv:library = files added/removed (folder tree changes), pmv:probe = sizes measured (counts change)
const changed = () => window.dispatchEvent(new Event("pmv:library"));
const measured = () => window.dispatchEvent(new Event("pmv:probe"));

export const kindOf = (f) => (VIDEO.test(f.name) || f.type.startsWith("video/") ? "video" : IMAGE.test(f.name) || f.type.startsWith("image/") ? "image" : null);

// files: [{ file, path }] – path relative to the picked/dropped folder, or just the file name
export function addFiles(list) {
  let added = 0;
  for (const { file, path } of list) {
    const kind = kindOf(file);
    if (!kind) continue;
    const p = String(path || file.name).replace(/\\/g, "/").replace(/^\/+/, "");
    const key = p + "|" + file.size + "|" + file.lastModified;
    if (byKey.has(key)) continue;
    byKey.add(key);
    const cut = p.lastIndexOf("/");
    items.push({ id: String(nextId++), kind, file, name: file.name, path: p, folder: cut > 0 ? p.slice(0, cut) : LOOSE });
    added++;
  }
  if (added) {
    folderCache = null;
    changed();
    probeAll();
  }
  return added;
}

export function clear() {
  items.forEach((it) => it.url && URL.revokeObjectURL(it.url));
  items.length = 0;
  byKey.clear();
  folderCache = null;
  changed();
}

export const all = () => items;
export const videos = () => items.filter((it) => it.kind === "video" && !it.bad);

export function urlOf(it) {
  if (!it.url) it.url = URL.createObjectURL(it.file);
  return it.url;
}

// ---------- Filters (same meaning as the Stash filters before) ----------

const inFolder = (it, folders) => !folders.length || folders.some((f) => it.folder === f.id || it.folder.startsWith(f.id + "/"));
function shapeOk(it, shape) {
  if (shape === "all" || !it.w) return true; // not measured yet → decided when it's prepared
  return shape === "portrait" ? it.h > it.w : it.w > it.h;
}
const kindOk = (it, source) => source === "both" || (source === "scene" ? it.kind === "video" : it.kind === "image");
const longEnough = (it) => it.kind !== "video" || !it.dur || it.dur > 4;

export function matching(S) {
  const folders = S.folders || [];
  return items.filter((it) => !it.bad && kindOk(it, S.source) && inFolder(it, folders) && shapeOk(it, S.shape) && longEnough(it));
}

// Does a measured clip fit? (after loading, with its real size)
export const fits = (it, S) => shapeOk(it, S.shape) && longEnough(it);

export function count(S) {
  const list = matching(S);
  return {
    videos: list.filter((it) => it.kind === "video").length,
    images: list.filter((it) => it.kind === "image").length,
    pending: S.shape !== "all" ? list.filter((it) => !it.probed).length : 0,
  };
}

// ---------- Folder tree (for the folder picker) ----------

let folderCache = null;
export function loadFolders() {
  if (folderCache) return folderCache;
  const nodes = new Map();
  const node = (path) => {
    if (nodes.has(path)) return nodes.get(path);
    const cut = path.lastIndexOf("/");
    const n = { id: path, path, name: cut >= 0 ? path.slice(cut + 1) : path, parent: cut > 0 ? path.slice(0, cut) : null, kids: [], img: 0, vid: 0 };
    nodes.set(path, n);
    if (n.parent) node(n.parent);
    return n;
  };
  items.forEach((it) => !it.bad && node(it.folder)[it.kind === "video" ? "vid" : "img"]++);
  for (const n of nodes.values()) if (n.parent) nodes.get(n.parent).kids.push(n);
  const total = (n) => {
    n.timg = n.img;
    n.tvid = n.vid;
    n.kids.forEach((k) => {
      total(k);
      n.timg += k.timg;
      n.tvid += k.tvid;
    });
  };
  const roots = [...nodes.values()].filter((n) => !n.parent);
  roots.forEach(total);
  const cmp = (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
  const sortRec = (n) => {
    n.kids.sort(cmp);
    n.kids.forEach(sortRec);
  };
  roots.forEach(sortRec);
  roots.sort(cmp);
  folderCache = Promise.resolve({ nodes, roots });
  return folderCache;
}

// ---------- Measure in the background ----------

let probing = false;
let treeDirty = false;
let paused = false;
// While a show runs, measuring waits – the show needs the disk and decoder more
export function pauseProbing(on) {
  paused = on;
  if (!on) probeAll();
}

async function probeAll() {
  if (probing) return;
  probing = true;
  let lastNote = performance.now();
  try {
    for (;;) {
      if (paused) return;
      const todo = items.filter((it) => !it.probed).slice(0, 3);
      if (!todo.length) return;
      await Promise.all(todo.map(probe));
      if (performance.now() - lastNote > 700) {
        lastNote = performance.now();
        measured();
      }
    }
  } finally {
    probing = false;
    measured();
    // Files the browser can't open drop out of the folder counts
    if (treeDirty) {
      treeDirty = false;
      folderCache = null;
      changed();
    }
  }
}

export async function probe(it) {
  if (it.probed) return it;
  try {
    if (it.kind === "image") {
      const bmp = await withTimeout(createImageBitmap(it.file), 8000);
      it.w = bmp.width;
      it.h = bmp.height;
      bmp.close();
    } else {
      const v = document.createElement("video");
      v.muted = true;
      v.preload = "metadata";
      try {
        v.src = urlOf(it);
        await withTimeout(
          new Promise((res, rej) => {
            v.onloadedmetadata = res;
            v.onerror = () => rej(new Error("unplayable"));
          }),
          8000
        );
        it.w = v.videoWidth;
        it.h = v.videoHeight;
        it.dur = isFinite(v.duration) ? v.duration : 0;
        if (!it.w || !it.h) it.bad = true; // audio only
      } finally {
        v.removeAttribute("src");
        v.load();
      }
    }
  } catch (e) {
    it.bad = true; // the browser can't open it – leave it out
    treeDirty = true;
  }
  it.probed = true;
  return it;
}

function withTimeout(p, ms) {
  return Promise.race([p, new Promise((_, rej) => setTimeout(() => rej(new Error("Timed out")), ms))]);
}

// ---------- Reading picked / dropped folders ----------

// <input type="file" webkitdirectory> or <input multiple>
export const fromInput = (fileList) => [...fileList].map((file) => ({ file, path: file.webkitRelativePath || file.name }));

// Drag & drop: walk dropped folders recursively
export async function fromDrop(dataTransfer) {
  const entries = [...dataTransfer.items].map((i) => i.webkitGetAsEntry && i.webkitGetAsEntry()).filter(Boolean);
  if (!entries.length) return fromInput(dataTransfer.files);
  const out = [];
  const walk = async (entry) => {
    if (entry.isFile) {
      const file = await new Promise((res, rej) => entry.file(res, rej));
      out.push({ file, path: entry.fullPath });
    } else if (entry.isDirectory) {
      const reader = entry.createReader();
      for (;;) {
        const batch = await new Promise((res, rej) => reader.readEntries(res, rej));
        if (!batch.length) break;
        for (const e of batch) await walk(e);
      }
    }
  };
  for (const e of entries) await walk(e);
  return out;
}
