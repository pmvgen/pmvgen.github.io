// Translations. The English text itself is the key: t("Scenes") – a missing translation simply
// shows the English text. Placeholders: t("{n} selected", { n: 3 }).
// Add a language: a file in ./locales/ that exports { "English text": "translation", … }
// and an entry in LANGS below.

export const LANGS = [
  ["en", "English"],
  ["zh-CN", "简体中文"],
];
const FILES = { "zh-CN": () => import("./locales/zh-CN.js") };
const KEY = "pmvgen.lang"; // "auto" (like Stash) or a code from LANGS

let dict = null;
export let lang = "en";

export function t(text, params) {
  let out = (dict && dict[text]) || text;
  if (params) out = out.replace(/\{(\w+)\}/g, (m, k) => (params[k] != null ? params[k] : m));
  return out;
}

// For Intl / toLocaleString
export const locale = () => (lang === "en" ? "en-US" : lang);

export function chosen() {
  try {
    return localStorage.getItem(KEY) || "auto";
  } catch (e) {
    return "auto";
  }
}
export function choose(code) {
  try {
    localStorage.setItem(KEY, code);
  } catch (e) { /* blocked – only for this page view */ }
}

// Stash's language setting (e.g. "zh-CN", "en-GB") → one of ours; Traditional Chinese etc. stay English for now
export function match(code) {
  const c = String(code || "").toLowerCase();
  if (/^zh[-_](cn|sg|hans)/.test(c) || c === "zh") return "zh-CN";
  return "en";
}

// Load the language before the first render. stashLanguage: Stash's interface language (for "auto").
export async function initLang(stashLanguage) {
  const pick = chosen();
  const code = pick === "auto" ? match(stashLanguage || navigator.language) : LANGS.some(([c]) => c === pick) ? pick : "en";
  dict = null;
  lang = "en";
  if (FILES[code]) {
    try {
      dict = (await FILES[code]()).default;
      lang = code;
    } catch (e) {
      console.error("[Stash UI] Couldn't load the translation", code, e);
    }
  }
  document.documentElement.lang = lang;
  return lang;
}
