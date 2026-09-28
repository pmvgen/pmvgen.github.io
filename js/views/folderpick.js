// Folder picker: chosen folders as chips, below a searchable list of all folders you added
// (indented, with the number of videos/images). Subfolders are always included.

import { esc, fmtNum } from "../ui.js";
import { loadFolders } from "../library.js";

export function folderPicker(host, opts) {
  let sel = [...(opts.selected || [])]; // [{ id, path }]
  let rows = [];
  host.innerHTML = `<div class="kb-chips" data-fchips></div>
    <input class="kb-field" type="search" placeholder="Search folders …" autocomplete="off" spellcheck="false" aria-label="Search folders">
    <div class="kb-fpick" role="listbox" aria-multiselectable="true"></div>`;
  const chips = host.querySelector("[data-fchips]");
  const input = host.querySelector("input");
  const list = host.querySelector(".kb-fpick");
  const baseName = (p) => String(p).split(/[\\/]/).filter(Boolean).pop() || p;

  function renderChips() {
    chips.innerHTML = sel.length
      ? sel.map((f) => `<span class="kb-chip is-on" title="${esc(f.path)}">${esc(baseName(f.path))}<button type="button" data-rm="${esc(f.id)}" aria-label="Remove">×</button></span>`).join("")
      : `<span class="kb-hint">All folders</span>`;
  }

  function renderList() {
    const q = input.value.trim().toLowerCase();
    const on = new Set(sel.map((f) => f.id));
    const shown = (q ? rows.filter((r) => r.path.toLowerCase().includes(q)) : rows).slice(0, 400);
    list.innerHTML = shown.length
      ? shown
          .map((r) => {
            const n = [r.tvid ? fmtNum(r.tvid) + " V" : "", r.timg ? fmtNum(r.timg) + " I" : ""].filter(Boolean).join(" · ");
            return `<button type="button" role="option" aria-selected="${on.has(r.id)}" class="${on.has(r.id) ? "is-on" : ""}" data-fid="${esc(r.id)}" style="--d:${q ? 0 : r.depth}" title="${esc(r.path)}">
              <i class="kb-fpick-box"></i><span>${esc(q ? r.path : r.name)}</span><small>${n}</small></button>`;
          })
          .join("")
      : `<div class="kb-hint">${rows.length ? "No folder matches" : "Add clips above – their folders show up here"}</div>`;
  }

  const emit = () => opts.onChange && opts.onChange(sel.map((f) => ({ ...f })));
  function toggle(id) {
    const r = rows.find((x) => x.id === id);
    if (sel.some((f) => f.id === id)) sel = sel.filter((f) => f.id !== id);
    else if (r) sel.push({ id: r.id, path: r.path });
    renderChips();
    renderList();
    emit();
  }

  list.addEventListener("click", (e) => {
    const b = e.target.closest("[data-fid]");
    if (b) toggle(b.dataset.fid);
  });
  chips.addEventListener("click", (e) => {
    const b = e.target.closest("[data-rm]");
    if (!b) return;
    sel = sel.filter((f) => f.id !== b.dataset.rm);
    renderChips();
    renderList();
    emit();
  });
  input.addEventListener("input", renderList);

  // Build the list again whenever clips are added or removed
  function load() {
    loadFolders().then((tree) => {
      rows = [];
      const walk = (n, d) => {
        rows.push({ id: n.id, path: n.path, name: n.name, depth: d, tvid: n.tvid, timg: n.timg });
        n.kids.forEach((k) => walk(k, d + 1));
      };
      tree.roots.forEach((r) => walk(r, 0));
      // Quietly remove chosen folders that aren't there (kept while nothing is added yet,
      // so the choice survives adding the same folder again after a reload)
      const known = new Set(rows.map((r) => r.id));
      if (rows.length && sel.some((f) => !known.has(f.id))) {
        sel = sel.filter((f) => known.has(f.id));
        renderChips();
        emit();
      }
      renderList();
    });
  }
  renderChips();
  load();
  window.addEventListener("pmv:library", load);
}
