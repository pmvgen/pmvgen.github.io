// Entry screen: terms of use that have to be accepted before the app loads.
// Accepted once per browser (per terms version); the "Terms" link in the top bar shows them again.

const VERSION = "1";
const KEY = "pmvgen.terms";

const TERMS = `
  <h2 id="kb-gate-title">Before you start</h2>
  <p class="kb-gate-lead">This is a tool for making music edits from <b>your own</b> media. By entering you confirm that:</p>
  <ol class="kb-gate-list">
    <li><b>You are 18 or older</b> (or of legal age where you live).</li>
    <li><b>You only use media you own or have permission to use</b> – videos, images and music.</li>
    <li><b>You never use content showing anyone under 18.</b></li>
    <li><b>You never use footage of real people without their consent</b> – in particular, you don't create or share sexual content of anyone who hasn't agreed to it.</li>
    <li><b>You follow the law</b> where you live, including copyright and privacy law.</li>
  </ol>
  <p class="kb-gate-note">Everything runs on your device: nothing is uploaded, and the author can't see, store or control what you make. <b>You alone are responsible for what you create and share with this tool.</b> The author doesn't condone any misuse and accepts no liability for it. The software is provided “as is”, without warranty of any kind (MIT license).</p>
`;

const accepted = () => {
  try {
    return localStorage.getItem(KEY) === VERSION;
  } catch (e) {
    return false;
  }
};

function dialog(readOnly) {
  return new Promise((resolve) => {
    const box = document.createElement("div");
    box.className = "kb-gate";
    box.innerHTML = `
      <div class="kb-gate-card" role="dialog" aria-modal="true" aria-labelledby="kb-gate-title">
        ${TERMS}
        <div class="kb-gate-acts">
          ${readOnly
            ? `<button type="button" class="kb-btn is-primary" data-gate="close">Close</button>`
            : `<button type="button" class="kb-btn is-primary" data-gate="agree">I agree – enter</button>
               <a class="kb-btn is-ghost" href="https://www.google.com" rel="noreferrer">Leave</a>`}
        </div>
      </div>`;
    document.body.appendChild(box);
    document.body.classList.add("kb-noscroll");
    const main = document.getElementById("main");
    if (main) main.inert = true;
    const btn = box.querySelector("[data-gate]");
    btn.focus();
    const done = () => {
      box.remove();
      document.body.classList.remove("kb-noscroll");
      if (main) main.inert = false;
      document.removeEventListener("keydown", onKey);
      resolve();
    };
    // Keep the keyboard inside the dialog; Esc only closes the read-only view
    const onKey = (e) => {
      if (e.key === "Escape" && readOnly) done();
      if (e.key !== "Tab") return;
      const f = [...box.querySelectorAll("button, a")];
      const i = f.indexOf(document.activeElement);
      e.preventDefault();
      f[(i + (e.shiftKey ? -1 : 1) + f.length) % f.length].focus();
    };
    document.addEventListener("keydown", onKey);
    btn.onclick = () => {
      if (!readOnly) {
        try {
          localStorage.setItem(KEY, VERSION);
        } catch (e) { /* storage blocked – they'll see it again next time */ }
      }
      done();
    };
  });
}

// Resolves once the terms are accepted (right away if they were before)
export const gate = () => (accepted() ? Promise.resolve() : dialog(false));
export const showTerms = () => dialog(true);
