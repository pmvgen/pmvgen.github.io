// Small helpers for display, formatting, messages and dialogs.

import { t, locale } from "./i18n.js";

export const esc = (s) =>
  String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

export const $ = (sel, root = document) => root.querySelector(sel);
export const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

const ICONS = {
  undo: '<path d="M9 7 4.5 11.5 9 16M5 11.5h9.5a5 5 0 0 1 0 10H11" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>',
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
  trophy: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M7.5 4.5h9v5a4.5 4.5 0 0 1-9 0z"/><path d="M7.5 6.5H4.5a3 3 0 0 0 3.2 3.4M16.5 6.5h3a3 3 0 0 1-3.2 3.4M12 14v3.5M8.5 20h7M9.5 17.5h5"/></g>',
  sliders: '<g fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M4 7h9M17 7h3M4 12h3M11 12h9M4 17h11M19 17h1"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="17" cy="17" r="2"/></g>',
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
  chart: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M4 20h16"/><path d="M7 16v-5M12 16V6M17 16v-8"/></g>',
  copies: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><rect x="8" y="8" width="12" height="12" rx="1.5"/><path d="M16 8V5.5A1.5 1.5 0 0 0 14.5 4h-9A1.5 1.5 0 0 0 4 5.5v9A1.5 1.5 0 0 0 5.5 16H8"/></g>',
  cast: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 17a3.5 3.5 0 0 1 3.5 3.5M3.5 13.5a7 7 0 0 1 7 7M3.5 10a10.5 10.5 0 0 1 10.5 10.5"/><path d="M3.5 7V5.5A1.5 1.5 0 0 1 5 4h14a1.5 1.5 0 0 1 1.5 1.5v13A1.5 1.5 0 0 1 19 20h-3"/></g>',
  pip: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="1.5"/></g><rect x="12" y="11.5" width="7" height="5.5" rx="1" fill="currentColor"/>',
  person: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="8" r="3.6"/><path d="M4.8 20c.6-3.9 3.5-6.2 7.2-6.2s6.6 2.3 7.2 6.2"/></g>',
  phone: '<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><rect x="7" y="3" width="10" height="18" rx="2"/><path d="M11 18h2" stroke-linecap="round"/></g>',
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
  return d.toLocaleDateString(locale(), { day: "numeric", month: "short", year: "numeric" });
}
export function fmtAgo(iso) {
  if (!iso) return "";
  const s = (Date.now() - new Date(iso)) / 1000;
  if (s < 90) return t("just now");
  if (s < 3600) return t("{n} min ago", { n: Math.round(s / 60) });
  if (s < 86400) return t("{n} h ago", { n: Math.round(s / 3600) });
  if (s < 86400 * 30) return t("{n} days ago", { n: Math.round(s / 86400) });
  return fmtDate(iso);
}
export const fmtNum = (n) => Number(n || 0).toLocaleString(locale());
// The unit words are translated too: plural(3, "scene", "scenes") → "3 scenes" / "3 个场景"
export const plural = (n, one, many) => `${fmtNum(n)} ${t(n === 1 ? one : many)}`;

// Inventory number like in a museum: S-12 (scene), I-40 (image), G-3 (gallery)
export const invNo = (kind, id) => `${{ scene: "S", image: "I", gallery: "G" }[kind]}-${id}`;

// ---------- Storage ----------

// Folders: "all" (navigation, home page, Folders page), "page" (only the Folders page) or "off"
export const folderMode = () => store.get("folderMode") || (store.get("railFolders", true) ? "all" : "page");

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

// act = { label, run }: a button in the toast (e.g. Undo) – the toast then stays a little longer
export function toast(msg, type, act) {
  const box = document.getElementById("toasts");
  const el = document.createElement("div");
  el.className = "kb-toast" + (type ? " is-" + type : "");
  el.textContent = msg;
  if (act) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "kb-toast-act";
    b.textContent = act.label;
    b.onclick = () => {
      el.remove();
      act.run();
    };
    el.appendChild(b);
  }
  box.appendChild(el);
  setTimeout(() => {
    el.classList.add("is-gone");
    setTimeout(() => el.remove(), 400);
  }, act ? 9000 : type === "error" ? 6000 : 3200);
}

export function errorToast(e, what) {
  console.error("[PMV Generator]", e);
  toast(`${what ? t(what) : t("Error")}: ${e.message || e}`, "error");
}

// ---------- Dialogs ----------

const overlayRoot = () => document.getElementById("overlay-root");

// Confirmation with an optional checkbox. Returns { ok, checked }.
export function confirmDialog({ title, text, ok = t("OK"), danger = false, checkbox }) {
  return new Promise((resolve) => {
    const wrap = document.createElement("div");
    wrap.innerHTML = `
      <div class="kb-scrim kb-dialog-scrim"></div>
      <div class="kb-dialog" role="dialog" aria-modal="true" aria-label="${esc(title)}">
        <h2>${esc(title)}</h2>
        ${text ? `<p>${esc(text)}</p>` : ""}
        ${checkbox ? `<label class="kb-check"><input type="checkbox" data-c>${esc(checkbox)}</label>` : ""}
        <div class="kb-actions">
          <button class="kb-btn" data-no>${t("Cancel")}</button>
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
export function promptDialog({ title, label, value = "", ok = t("OK") }) {
  return new Promise((resolve) => {
    const wrap = document.createElement("div");
    wrap.innerHTML = `
      <div class="kb-scrim kb-dialog-scrim"></div>
      <form class="kb-dialog" role="dialog" aria-modal="true" aria-label="${esc(title)}">
        <h2>${esc(title)}</h2>
        <label class="kb-form-row"><span class="kb-dialog-label">${esc(label || "")}</span><input class="kb-field kb-dialog-input" value="${esc(value)}" required></label>
        <div class="kb-actions">
          <button type="button" class="kb-btn" data-no>${t("Cancel")}</button>
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
      <div class="kb-drawer-head"><h2>${esc(title)}</h2><button class="kb-btn is-icon is-ghost" data-close aria-label="${t("Close")}">${icon("close")}</button></div>
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

// Little particles flying out of an element (O counter drops, favorite hearts). kind: "drop" | "heart"
export function burst(el, kind = "drop", count = 11) {
  if (!el || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const r = el.getBoundingClientRect();
  let cx = r.left + r.width / 2;
  let cy = r.top + r.height / 2;
  // Button hidden (e.g. the info panel is tucked away in fullscreen, key O): from the middle of the screen
  if (cx < 0 || cx > innerWidth || cy < 0 || cy > innerHeight || !r.width) {
    cx = innerWidth / 2;
    cy = innerHeight / 2;
  }
  for (let i = 0; i < count; i++) {
    const p = document.createElement("i");
    p.className = "kb-particle is-" + kind;
    const size = kind === "heart" ? 9 + Math.random() * 7 : 4 + Math.random() * 5;
    p.style.cssText = `left:${cx}px;top:${cy}px;width:${size}px;height:${size}px`;
    (document.fullscreenElement || document.body).appendChild(p); // in fullscreen only that element is visible
    // Mostly upwards, spread to the sides; drops fall a little at the end
    const a = -Math.PI / 2 + (Math.random() - 0.5) * Math.PI * (kind === "heart" ? 0.9 : 1.5);
    const d = (kind === "heart" ? 34 : 26) + Math.random() * 34;
    const x = Math.cos(a) * d;
    const y = Math.sin(a) * d;
    const rot = kind === "drop" ? (Math.atan2(y, x) * 180) / Math.PI + 225 : (Math.random() - 0.5) * 50;
    p.animate(
      [
        { transform: `translate(-50%, -50%) rotate(${rot}deg) scale(.3)`, opacity: 1 },
        { transform: `translate(calc(-50% + ${x * 0.75}px), calc(-50% + ${y * 0.75}px)) rotate(${rot}deg) scale(1)`, opacity: 1, offset: 0.55 },
        { transform: `translate(calc(-50% + ${x}px), calc(-50% + ${y + (kind === "drop" ? 14 : -12)}px)) rotate(${rot}deg) scale(.6)`, opacity: 0 },
      ],
      { duration: 650 + Math.random() * 350, easing: "cubic-bezier(.2,.8,.3,1)", delay: Math.random() * 60 }
    ).onfinish = () => p.remove();
  }
}

// A quick "boing" on a button
export function pop(el, scale = 1.25) {
  if (!el || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  el.animate([{ transform: "scale(1)" }, { transform: `scale(${scale})` }, { transform: "scale(.94)" }, { transform: "scale(1)" }], { duration: 420, easing: "cubic-bezier(.3,1.6,.5,1)" });
}

// ---------- Rating: the system chosen in Stash (classic Stash → Settings → Interface → Editing) ----------
// Stash keeps every rating as 1–100; it's shown as stars (whole, half, quarter or tenth) or as 0.0–10.0.
let RS = (() => {
  const v = store.get("ratingSystem", null);
  return v && v.type ? v : { type: "stars", step: 1 };
})();
export const ratingSystem = () => RS;
export function setRatingSystem(opts) {
  const o = opts || {};
  const next = { type: o.type === "decimal" ? "decimal" : "stars", step: { full: 1, half: 0.5, quarter: 0.25, tenth: 0.1 }[o.starPrecision] || 1 };
  const changed = next.type !== RS.type || next.step !== RS.step;
  RS = next;
  store.set("ratingSystem", RS);
  return changed;
}
const round = (v, step) => Math.round(Math.round(v / step) * step * 100) / 100;
// rating100 → what's shown: stars 0–5 (in the chosen steps) or 0.0–10.0
export const ratingValue = (r) => (!r ? 0 : RS.type === "decimal" ? Math.round(r) / 10 : round(r / 20, RS.step));
const fmtVal = (v) => (RS.type === "decimal" ? v.toFixed(1) : String(v));
// Short text: "★★★★", "★ 3.5" or "7.5"
export function ratingText(r) {
  if (!r) return "";
  const v = ratingValue(r);
  return RS.type === "decimal" ? fmtVal(v) : RS.step === 1 ? "★".repeat(v) : "★ " + fmtVal(v);
}
export const ratingToast = (r) => (r ? t("Rating: {r}", { r: RS.type === "decimal" ? fmtVal(ratingValue(r)) + " / 10" : ratingText(r) }) : t("Rating removed"));

export function starsHtml(rating100, interactive) {
  if (RS.type === "decimal") {
    const v = ratingValue(rating100);
    return interactive
      ? `<span class="kb-stars kb-rate-dec" role="group" aria-label="${t("Rating")}"><input class="kb-field" type="number" min="0" max="10" step="0.1" inputmode="decimal" data-ratedec value="${v ? v.toFixed(1) : ""}" placeholder="–" aria-label="${t("Rating")} (0–10)"><small>/ 10</small></span>`
      : `<span class="kb-stars kb-rate-dec"><b>${v ? fmtVal(v) : "–"}</b><small>/ 10</small></span>`;
  }
  const v = ratingValue(rating100);
  return (
    `<span class="kb-stars${RS.step < 1 ? " is-fine" : ""}"${interactive ? ` role="group" aria-label="${t("Rating")}"` : ""}>` +
    [1, 2, 3, 4, 5]
      .map((i) => {
        // a partly filled star (half, quarter, tenth)
        const fill = Math.max(0, Math.min(1, v - (i - 1)));
        const cls = fill >= 1 ? "is-on" : fill > 0 ? "is-part" : "";
        const style = fill > 0 && fill < 1 ? ` style="--fill:${Math.round(fill * 100)}%"` : "";
        return interactive ? `<button type="button" data-star="${i}" class="${cls}"${style} aria-label="${t("{n} stars", { n: i })}">★</button>` : `<span class="${cls}"${style}>★</span>`;
      })
      .join("") +
    "</span>"
  );
}
// A click on the stars → the new rating100; null = removed (the same value again); undefined = not a star.
// With half/quarter/tenth stars, where on the star you click counts.
export function ratingClick(e, current100) {
  const b = e.target.closest("[data-star]");
  if (!b) return undefined;
  const i = Number(b.dataset.star);
  let v = i;
  if (RS.step < 1) {
    const r = b.getBoundingClientRect();
    const frac = r.width ? Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) : 1;
    v = round(i - 1 + Math.max(RS.step, Math.ceil(frac / RS.step - 1e-6) * RS.step), RS.step);
  }
  return ratingValue(current100) === v ? null : Math.round(v * 20);
}
// Half/quarter/tenth stars: hovering shows exactly what a click would set
function starAt(e) {
  const b = e.target.closest && e.target.closest(".kb-stars.is-fine [data-star]");
  if (!b) return null;
  const r = b.getBoundingClientRect();
  const frac = r.width ? Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) : 1;
  return { b, v: round(Number(b.dataset.star) - 1 + Math.max(RS.step, Math.ceil(frac / RS.step - 1e-6) * RS.step), RS.step) };
}
if (typeof document !== "undefined") {
  document.addEventListener("mousemove", (e) => {
    const h = starAt(e);
    if (!h) return;
    h.b.parentElement.querySelectorAll("[data-star]").forEach((s, i) => {
      s.classList.add("is-hov");
      s.style.setProperty("--hfill", Math.round(Math.max(0, Math.min(1, h.v - i)) * 100) + "%");
    });
  });
  document.addEventListener("mouseout", (e) => {
    const g = e.target.closest && e.target.closest(".kb-stars.is-fine");
    if (g && !g.contains(e.relatedTarget)) g.querySelectorAll(".is-hov").forEach((s) => s.classList.remove("is-hov"));
  });
}
// The decimal field → rating100 (empty or 0 = removed; undefined = not a number, leave it)
export function ratingFromInput(el) {
  if (el.validity && el.validity.badInput) return undefined;
  const v = Math.max(0, Math.min(10, parseFloat(String(el.value).replace(",", ".")) || 0));
  return v ? Math.max(1, Math.round(v * 10)) : null;
}
// "Rating from" filter: the steps of the chosen system → [value, label]; and the matching rating100 limit
export const ratingFilterSteps = () => (RS.type === "decimal" ? [1, 2, 3, 4, 5, 6, 7, 8, 9].map((n) => [n, "≥ " + n]) : [1, 2, 3, 4, 5].map((n) => [n, "★".repeat(n)]));
export const ratingFilterMin = (n) => (RS.type === "decimal" ? n * 10 : n * 20) - 1;

// Debounce
export function debounce(fn, ms) {
  let t;
  return (...a) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...a), ms);
  };
}

export const seed = () => "random_" + Math.floor(Math.random() * 1e8);
