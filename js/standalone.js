// Entry point of the PMV Generator web app: terms first, then the app.
import { gate, showTerms } from "./gate.js";
import { render } from "./views/pmvgen.js";

document.querySelector("[data-terms]").addEventListener("click", (e) => {
  e.preventDefault();
  showTerms();
});
gate().then(() => render(document.getElementById("main")));
