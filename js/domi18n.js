// Translates a page as it is shown: text, tooltips and placeholders whose English wording is in the
// dictionary are swapped – also whatever appears later (messages, the live show, dialogs). This way
// the PMV Generator's code stays as it is. { texts: {English: translation}, patterns: [[regex, out]] }

const ATTRS = ["title", "placeholder", "aria-label"];
const SKIP = new Set(["SCRIPT", "STYLE", "TEXTAREA", "CODE", "PRE"]);
// Names from the library stay as they are: folder rows and chosen folders, tag chips, found PMVs
const DATA = "[data-noi18n], [data-fid], [data-fchips] .kb-chip, .kb-chip[data-id], .kb-pmvg-tpl";

export function startDomTranslation(root, { texts, patterns }) {
  const exact = new Map(Object.entries(texts));
  const pats = patterns.map(([re, out]) => [new RegExp("^" + re + "$"), out]);
  // Parts without lowercase words (numbers, "16:9", "720p", "VHS") stay as they are
  const neutral = (p) => !/[a-z]{2,}/.test(p);
  const tr = (raw, depth = 0) => {
    const s = raw.replace(/\s+/g, " ").trim();
    if (!s || !/[A-Za-z]/.test(s) || depth > 4) return null;
    if (exact.has(s)) return exact.get(s);
    for (const [re, out] of pats) {
      const m = s.match(re);
      if (m) return out.replace(/\$(\d)/g, (x, i) => (m[+i] == null ? "" : tr(m[+i], depth + 1) ?? m[+i]));
    }
    // Lists: "Automatic · 6 layouts", "best moments, smart crop" – each part on its own
    for (const [sep, join] of [[" · ", " · "], [", ", "、"]]) {
      if (!s.includes(sep)) continue;
      const parts = s.split(sep).map((p) => tr(p, depth + 1) ?? (neutral(p) ? p : null));
      if (parts.every((p) => p != null)) return parts.join(join);
    }
    return null;
  };
  const doText = (n) => {
    const p = n.parentElement;
    if (!p || SKIP.has(p.tagName) || p.closest(DATA)) return;
    const v = n.nodeValue;
    const out = tr(v);
    if (out != null && out !== v.trim()) n.nodeValue = v.match(/^\s*/)[0] + out + v.match(/\s*$/)[0];
  };
  const doEl = (el) => {
    if (el.matches && el.matches(DATA)) return; // its own tooltip is a name (e.g. a folder path)
    for (const a of ATTRS) {
      const v = el.getAttribute && el.getAttribute(a);
      if (v) {
        const out = tr(v);
        if (out != null && out !== v) el.setAttribute(a, out);
      }
    }
  };
  const walk = (node) => {
    if (node.nodeType === 3) return doText(node);
    if (node.nodeType !== 1 || SKIP.has(node.tagName)) return;
    doEl(node);
    const w = document.createTreeWalker(node, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
    for (let n = w.nextNode(); n; n = w.nextNode()) n.nodeType === 3 ? doText(n) : doEl(n);
  };
  walk(root);
  const mo = new MutationObserver((list) => {
    for (const m of list) {
      if (m.type === "characterData") doText(m.target);
      else if (m.type === "attributes") doEl(m.target);
      else m.addedNodes.forEach(walk);
    }
  });
  mo.observe(root, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ATTRS });
  return () => mo.disconnect();
}
