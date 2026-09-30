// Entry point of the PMV Generator web app: terms first, then the app – in Chinese when the browser
// is set to it (the page is translated as it's shown; the code stays in English).
import { gate, showTerms } from "./gate.js";
import { initLang } from "./i18n.js";
import { startDomTranslation } from "./domi18n.js";
import { render } from "./views/pmvgen.js";

document.querySelector("[data-terms]").addEventListener("click", (e) => {
  e.preventDefault();
  showTerms();
});
gate().then(async () => {
  const lang = await initLang("");
  if (lang === "zh-CN") {
    try {
      startDomTranslation(document.body, (await import("./locales/pmv-zh-CN.js")).default);
    } catch (e) {
      console.error("[PMV Generator] translation", e);
    }
  }
  render(document.getElementById("main"));
});
