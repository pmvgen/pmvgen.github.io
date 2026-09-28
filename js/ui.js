// Small helpers for display, formatting, messages and dialogs.

export const esc = (s) =>
  String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

export const $ = (sel, root = document) => root.querySelector(sel);
export const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

const ICONS = {
  home: '<path d="M4 11.5 12 5l8 6.5V20h-5.5v-5h-5v5H4z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>',
  folder: '<path d="M3.5 7a1.5 1.5 0 0 1 1.5-1.5h4.3l2 2.2H19a1.5 1.5 0 0 1 1.5 1.5v8.3a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>',
  film: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><rect x="3.5" y="5" width="17" height="14" rx="1.5"/><path d="M8 5v14M16 5v14M3.5 9.5H8M3.5 14.5H8M16 9.5h4.5M16 14.5h4.5"/></g>',
  image: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="15" rx="1.5"/><path d="m4 17 5-5 4 4 3-3 4 4"/></g><circle cx="15.5" cy="9" r="1.5" fill="currentColor"/>',
  book: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M4 5.5h6a2 2 0 0 1 2 2V19a1.6 1.6 0 0 0-1.6-1.6H4z"/><path d="M20 5.5h-6a2 2 0 0 0-2 2V19a1.6 1.6 0 0 1 1.6-1.6H20z"/></g>',
  tag: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M3.5 12.2V4.5a1 1 0 0 1 1-1h7.7l8.3 8.3a1 1 0 0 1 0 1.4l-7 7a1 1 0 0 1-1.4 0z"/></g><circle cx="8" cy="8" r="1.5" fill="currentColor"/>',
  queue: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M4 6h11M4 11h11M4 16h7"/><path d="m15 14 5 3-5 3z" fill="currentColor"/></g>',
  history: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 12a8 8 0 1 0 2.4-5.7L4 8.5"/><path d="M4 4v4.5h4.5M12 8v4.5l3 2"/></g>',
  search: '<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5"/></g>',
  tasks: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6h11M9 12h11M9 18h11"/><path d="m3.5 6 1.3 1.3L7 5M3.5 12l1.3 1.3L7 11M3.5 18l1.3 1.3L7 17"/></g>',
  gear: '<g fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="12" cy="12" r="3"/><path d="M12 3.5v2.2M12 18.3v2.2M20.5 12h-2.2M5.7 12H3.5M18 6l-1.6 1.6M7.6 16.4 6 18M18 18l-1.6-1.6M7.6 7.6 6 6" stroke-linecap="round"/></g>',
  plug: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3.5v4M15 3.5v4M6.5 7.5h11V11a5.5 5.5 0 0 1-11 0zM12 16.5v4"/></g>',
  bolt: '<path d="M13 2.5 4.8 13.5H11L10 21.5l8.2-11H12z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>',
  download: '<path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 19.5h14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>',
  camera: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M4 8.5h3l1.5-2.5h7L17 8.5h3v10H4z"/><circle cx="12" cy="13" r="3.2"/></g>',
  tv: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" stroke-linecap="round"><rect x="6.5" y="3.5" width="11" height="17" rx="2"/><path d="m11 10 3 2-3 2z" fill="currentColor"/></g>',
  feed: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><rect x="5" y="3.5" width="14" height="7.5" rx="1.2"/><rect x="5" y="13" width="14" height="7.5" rx="1.2"/></g>',
  door: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" stroke-linecap="round"><path d="M13 4H6v16h7M10 12h10M17 9l3 3-3 3"/></g>',
  menu: '<path d="M4 7h16M4 12h16M4 17h16" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>',
  close: '<path d="M6 6l12 12M18 6 6 18" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>',
  play: '<path d="M7 4.5v15l12.5-7.5z" fill="currentColor"/>',
  pause: '<path d="M7 4.5h3.5v15H7zM13.5 4.5H17v15h-3.5z" fill="currentColor"/>',
  next: '<path d="M5 5v14l10-7zM16.5 5H19v14h-2.5z" fill="currentColor"/>',
  prev: '<path d="M19 5v14L9 12zM7.5 5H5v14h2.5z" fill="currentColor"/>',
  shuffle: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h3.5c4.5 0 5.5 10 10 10H20M4 17h3.5c1.8 0 3-1.6 4-3.5M20 7h-2.5c-1.8 0-3 1.6-4 3.5M17.5 4.5 20 7l-2.5 2.5M17.5 14.5 20 17l-2.5 2.5"/></g>',
  repeat: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 11V9a3 3 0 0 1 3-3h12M16.5 3.5 19 6l-2.5 2.5M20 13v2a3 3 0 0 1-3 3H5M7.5 20.5 5 18l2.5-2.5"/></g>',
  volume: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/></g>',
  mute: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="m16 9.5 5 5M21 9.5l-5 5"/></g>',
  expand: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>',
  edit: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/></g>',
  trash: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13"/></g>',
  plus: '<path d="M12 5v14M5 12h14" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>',
  drop: '<path d="M12 3.5c3 4 6 7.2 6 10.5a6 6 0 0 1-12 0c0-3.3 3-6.5 6-10.5z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>',
  back: '<path d="M14.5 5.5 8 12l6.5 6.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>',
  fwd: '<path d="M9.5 5.5 16 12l-6.5 6.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>',
  select: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><rect x="4" y="4" width="16" height="16" rx="2"/><path d="m8 12 3 3 5-6" stroke-linecap="round"/></g>',
  filter: '<path d="M4 5h16l-6.2 7.5V19l-3.6-1.8v-4.7z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>',
  zoomin: '<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5M10.5 8v5M8 10.5h5"/></g>',
  slides: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><rect x="3.5" y="5.5" width="17" height="13" rx="1.5"/><path d="m10 9.5 4.5 2.5-4.5 2.5z" fill="currentColor"/></g>',
  info: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.5M12 7.8v.2"/></g>',
  logs: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3.5h9l3.5 3.5v13.5H6z"/><path d="M9 11h6M9 14.5h6M9 18h4"/></g>',
  heart: '<path d="M12 20s-7-4.3-8.9-8.7C1.7 8.1 3.7 4.6 7.1 4.6c2 0 3.6 1.2 4.9 3 1.3-1.8 2.9-3 4.9-3 3.4 0 5.4 3.5 4 6.7C19 15.7 12 20 12 20z" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>',
  music: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M9 17.5V5.5l10-2v12"/><circle cx="6.5" cy="17.5" r="2.5"/><circle cx="16.5" cy="15.5" r="2.5"/></g>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="1.5" fill="currentColor"/>',
};
export const icon = (name) => `<svg class="kb-ic" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name] || ""}</svg>`;

// ---------- Formatting ----------

export function fmtDuration(sec) {
  if (!sec && sec !== 0) return "";
  sec = Math.round(sec);
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${m}:${String(s).padStart(2, "0")}`;
}
export function fmtBytes(b) {
  if (!b) return "";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (b >= 1024 && i < u.length - 1) {
    b /= 1024;
    i++;
  }
  return `${b.toFixed(b >= 10 || i === 0 ? 0 : 1)} ${u[i]}`;
}
export function fmtRes(w, h) {
  const p = Math.min(w || 0, h || 0);
  if (!p) return "";
  if (p >= 2100) return "4K";
  if (p >= 1400) return "1440p";
  if (p >= 1000) return "1080p";
  if (p >= 700) return "720p";
  if (p >= 460) return "480p";
  return p + "p";
}
export function fmtDate(iso) {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d)) return iso;
  return d.toLocaleDateString("en-US", { day: "numeric", month: "short", year: "numeric" });
}
export function fmtAgo(iso) {
  if (!iso) return "";
  const s = (Date.now() - new Date(iso)) / 1000;
  if (s < 90) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 30) return `${Math.round(s / 86400)} days ago`;
  return fmtDate(iso);
}
export const fmtNum = (n) => Number(n || 0).toLocaleString("en-US");
export const plural = (n, one, many) => `${fmtNum(n)} ${n === 1 ? one : many}`;

// Inventory number like in a museum: S-12 (scene), I-40 (image), G-3 (gallery)
export const invNo = (kind, id) => `${{ scene: "S", image: "I", gallery: "G" }[kind]}-${id}`;

// ---------- Storage ----------

export const store = {
  get(key, fallback) {
    try {
      const v = localStorage.getItem("pmvgen." + key);
      return v == null ? fallback : JSON.parse(v);
    } catch (e) {
      return fallback;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem("pmvgen." + key, JSON.stringify(value));
    } catch (e) { /* full or blocked */ }
  },
};

// ---------- Messages ----------

export function toast(msg, type) {
  const box = document.getElementById("toasts");
  const el = document.createElement("div");
  el.className = "kb-toast" + (type ? " is-" + type : "");
  el.textContent = msg;
  box.appendChild(el);
  setTimeout(() => {
    el.classList.add("is-gone");
    setTimeout(() => el.remove(), 400);
  }, type === "error" ? 6000 : 3200);
}

export function errorToast(e, what) {
  console.error("[PMV Generator]", e);
  toast(`${what || "Error"}: ${e.message || e}`, "error");
}

// ---------- Dialogs ----------

const overlayRoot = () => document.getElementById("overlay-root");

// Confirmation with an optional checkbox. Returns { ok, checked }.
export function confirmDialog({ title, text, ok = "OK", danger = false, checkbox }) {
  return new Promise((resolve) => {
    const wrap = document.createElement("div");
    wrap.innerHTML = `
      <div class="kb-scrim kb-dialog-scrim"></div>
      <div class="kb-dialog" role="dialog" aria-modal="true" aria-label="${esc(title)}">
        <h2>${esc(title)}</h2>
        ${text ? `<p>${esc(text)}</p>` : ""}
        ${checkbox ? `<label class="kb-check"><input type="checkbox" data-c>${esc(checkbox)}</label>` : ""}
        <div class="kb-actions">
          <button class="kb-btn" data-no>Cancel</button>
          <button class="kb-btn ${danger ? "is-danger" : "is-primary"}" data-yes>${esc(ok)}</button>
        </div>
      </div>`;
    overlayRoot().appendChild(wrap);
    const done = (okv) => {
      const c = wrap.querySelector("[data-c]");
      wrap.remove();
      document.removeEventListener("keydown", onKey, true);
      resolve({ ok: okv, checked: !!(c && c.checked) });
    };
    const onKey = (e) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        done(false);
      }
    };
    document.addEventListener("keydown", onKey, true);
    wrap.querySelector("[data-no]").onclick = () => done(false);
    wrap.querySelector(".kb-scrim").onclick = () => done(false);
    wrap.querySelector("[data-yes]").onclick = () => done(true);
    wrap.querySelector("[data-yes]").focus();
  });
}

// Input with its own dialog instead of prompt(). Returns the text or null.
export function promptDialog({ title, label, value = "", ok = "OK" }) {
  return new Promise((resolve) => {
    const wrap = document.createElement("div");
    wrap.innerHTML = `
      <div class="kb-scrim kb-dialog-scrim"></div>
      <form class="kb-dialog" role="dialog" aria-modal="true" aria-label="${esc(title)}">
        <h2>${esc(title)}</h2>
        <label class="kb-form-row"><span class="kb-dialog-label">${esc(label || "")}</span><input class="kb-field kb-dialog-input" value="${esc(value)}" required></label>
        <div class="kb-actions">
          <button type="button" class="kb-btn" data-no>Cancel</button>
          <button type="submit" class="kb-btn is-primary">${esc(ok)}</button>
        </div>
      </form>`;
    overlayRoot().appendChild(wrap);
    const input = wrap.querySelector("input");
    const done = (v) => {
      wrap.remove();
      document.removeEventListener("keydown", onKey, true);
      resolve(v);
    };
    const onKey = (e) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        done(null);
      }
    };
    document.addEventListener("keydown", onKey, true);
    wrap.querySelector("[data-no]").onclick = () => done(null);
    wrap.querySelector(".kb-scrim").onclick = () => done(null);
    wrap.querySelector("form").onsubmit = (e) => {
      e.preventDefault();
      done(input.value.trim() || null);
    };
    input.focus();
    input.select();
  });
}

// Drawer from the right. render(body) fills the content; returns close().
export function openDrawer({ title, body, foot, onClose }) {
  const wrap = document.createElement("div");
  wrap.innerHTML = `
    <div class="kb-scrim"></div>
    <aside class="kb-drawer" role="dialog" aria-modal="true" aria-label="${esc(title)}">
      <div class="kb-drawer-head"><h2>${esc(title)}</h2><button class="kb-btn is-icon is-ghost" data-close aria-label="Close">${icon("close")}</button></div>
      <div class="kb-drawer-body">${body || ""}</div>
      ${foot ? `<div class="kb-drawer-foot">${foot}</div>` : ""}
    </aside>`;
  overlayRoot().appendChild(wrap);
  const close = () => {
    wrap.remove();
    document.removeEventListener("keydown", onKey, true);
    onClose && onClose();
  };
  const onKey = (e) => {
    if (e.key === "Escape" && !overlayRoot().querySelector(".kb-dialog")) {
      e.stopPropagation();
      close();
    }
  };
  document.addEventListener("keydown", onKey, true);
  wrap.querySelector(".kb-scrim").onclick = close;
  wrap.querySelector("[data-close]").onclick = close;
  return { el: wrap.querySelector(".kb-drawer"), close };
}

export function starsHtml(rating100, interactive) {
  const n = Math.round((rating100 || 0) / 20);
  return (
    `<span class="kb-stars"${interactive ? ' role="group" aria-label="Rating"' : ""}>` +
    [1, 2, 3, 4, 5]
      .map((i) => (interactive ? `<button type="button" data-star="${i}" class="${i <= n ? "is-on" : ""}" aria-label="${i} stars">★</button>` : `<span class="${i <= n ? "is-on" : ""}">★</span>`))
      .join("") +
    "</span>"
  );
}

// Debounce
export function debounce(fn, ms) {
  let t;
  return (...a) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...a), ms);
  };
}

export const seed = () => "random_" + Math.floor(Math.random() * 1e8);
