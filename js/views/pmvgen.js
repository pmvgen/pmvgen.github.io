// PMV Generator: pick a song → detect beats → cut live to clips from your folders on every beat.
// Split-screen layouts (e.g. mirrored 3-way) change with the energy; the fields are re-cut in turn.
// Layouts and effects live in ../pmvfx.js.
// Optionally MediaRecorder records picture + song; the video can be downloaded.
// Everything runs in the browser – the clips are local files and are never uploaded.

import { esc, icon, toast, errorToast, fmtDuration, fmtBytes, fmtNum, store } from "../ui.js";
import * as lib from "../library.js";
import { fixWebmDuration } from "../webmfix.js";
import { analyzeSong, rescale, shift } from "../beats.js";
import { scanPmv } from "../pmvscan.js";
import { analyze, spotScore, matchDist } from "../pmvsmart.js";
import { folderPicker } from "./folderpick.js";
import { LAYOUTS, slotsFor, aspectOfGroup, Compositor } from "../pmvfx.js";

const DEFAULTS = {
  mode: "song", // song = your own song, tpl = use a PMV as template
  source: "scene", // scene = videos, image = images, both
  folders: [], // [{ id, path }] – empty = all folders
  shape: "all", // clip shape: all | portrait | landscape
  bestSpots: true, // best moments instead of random
  smartCrop: true, // crop follows what matters
  variety: true, // same clip not shortly after itself
  matchCut: true, // pick the best-matching clip at each cut
  cut: "auto",
  layouts: { full: true, kaleido: true, duo: true, trim: true, tri: true, quad: true },
  fx: { flash: true, zoom: true, shake: true, glitch: true, stutter: true, hue: false, rgb: true, echo: true, tunnel: true, invert: true, whip: true, zoomin: true, speed: true, voice: true, vhs: false, strobe: false, text: false, kenburns: true, lines: true },
  words: "",
  look: "none", // color look: none | warm | pink | cold | bw | noir | vivid
  lookEven: true, // even out clip brightness
  songVol: 100, // song volume (%)
  clipVol: 50, // clip audio volume (%)
  voiceMode: "drops", // clip audio: drops = only on drops, always = all the time
  intro: false,
  outro: false,
  title: "", // empty = song name
  format: "16:9",
  split: "cols",
  fit: "cover",
  quality: 720,
  record: true,
  tab: "cut", // last open style tab
};
const FX = {
  flash: ["Flash", "Bright flash on cuts and drops"],
  zoom: ["Zoom pulse", "The picture pumps on every beat"],
  whip: ["Transitions", "New clips whip into the field with motion blur"],
  zoomin: ["Zoom-in entry", "New clips zoom into the picture fast – alternating in and out"],
  speed: ["Speed ramps", "Slow motion in calm parts, faster when it gets loud, a burst on drops"],
  voice: ["Clip audio", "Mix in the clips' original audio – only on drops or all the time"],
  rgb: ["RGB split", "Red and cyan tear apart – really hard on drops"],
  echo: ["Echo", "The last frame lingers as a ghost (loud parts)"],
  tunnel: ["Tunnel", "On drops the picture sits inside itself three times"],
  invert: ["Negative", "Short negative on drops"],
  glitch: ["Glitch", "Picture stripes slip on drops"],
  stutter: ["Stutter", "Clips jump back on half beats – a classic PMV effect"],
  shake: ["Shake", "Shakes in loud parts"],
  hue: ["Color rush", "Colors rotate to the beat"],
  vhs: ["VHS", "Noise, scanlines and dark corners"],
  strobe: ["Strobe", "White flashes on half beats – careful if you are sensitive to light"],
  text: ["Text", "Your words slam into the picture on drops"],
  kenburns: ["Image drift", "Still images zoom and glide slowly (Ken Burns)"],
  lines: ["Glowing dividers", "The lines between fields flash pink to the beat"],
};
// Effects grouped by occasion – so every switch is where you'd look for it
const FX_GROUPS = [
  ["On cuts", "When a new clip comes in", ["whip", "zoomin", "flash"]],
  ["On the beat", "Run through the whole song", ["zoom", "shake", "speed", "stutter", "strobe"]],
  ["On drops", "The big moments", ["rgb", "glitch", "tunnel", "invert", "echo", "text"]],
];
const LOOK_FX = ["hue", "vhs", "kenburns", "lines"];
const LOOKS = [
  ["none", "Original"],
  ["warm", "Warm"],
  ["pink", "Pink"],
  ["cold", "Cold"],
  ["vivid", "Vivid"],
  ["bw", "Black & white"],
  ["noir", "Noir"],
];
// Moods: set layouts, cutting and effects in one go
const PRESETS = {
  classic: { name: "PMV classic", cut: "auto", layouts: ["full", "trim", "tri"], fx: ["flash", "zoom", "whip", "zoomin", "speed", "voice", "rgb", "glitch", "stutter", "shake", "kenburns", "lines"] },
  maximal: { name: "Maximal", cut: "1", layouts: ["full", "kaleido", "duo", "trim", "tri", "quad"], fx: ["flash", "zoom", "whip", "zoomin", "speed", "voice", "rgb", "echo", "tunnel", "invert", "glitch", "stutter", "shake", "hue", "text", "kenburns", "lines"] },
  hypno: { name: "Hypno", cut: "2", layouts: ["full", "kaleido", "trim"], fx: ["zoom", "speed", "echo", "tunnel", "hue", "vhs", "kenburns"] },
  clean: { name: "Clean", cut: "auto", layouts: ["full", "duo", "trim"], fx: ["zoom", "whip", "kenburns"] },
};
const CUTS = [
  ["auto", "Automatic", "Calm: every 4 beats · medium: every 2 · loud: every beat"],
  ["1", "Every beat", ""],
  ["2", "Every 2 beats", ""],
  ["4", "Every 4 beats", ""],
];
const TABS = [
  ["cut", "Cutting"],
  ["fx", "Effects"],
  ["look", "Look"],
  ["sound", "Sound"],
  ["out", "Output"],
];

// A row with a switch; path = "bestSpots" or "fx.flash"
const sw = (path, name, desc) =>
  `<label class="kb-pmvg-opt"><span><b>${esc(name)}</b>${desc ? `<small>${esc(desc)}</small>` : ""}</span>` +
  `<span class="kb-switch"><input type="checkbox" data-t="${path}"><i></i></span></label>`;
const fxSw = (k) => sw("fx." + k, FX[k][0], FX[k][1]);
const getPath = (S, p) => p.split(".").reduce((o, k) => (o ? o[k] : undefined), S);
function setPath(S, p, v) {
  const ks = p.split(".");
  const last = ks.pop();
  ks.reduce((o, k) => o[k], S)[last] = v;
}

export function render(main) {
  const S = Object.assign({}, DEFAULTS, store.get("pmvgen", {}));
  S.fx = Object.assign({}, DEFAULTS.fx, S.fx);
  S.layouts = Object.assign({}, DEFAULTS.layouts, S.layouts);
  if (!Array.isArray(S.folders)) S.folders = [];
  const save = () => store.set("pmvgen", S);
  let song = null; // beat detection result + name
  let run = null; // running generator
  let alive = true;

  main.innerHTML = `
    <header class="kb-head"><div class="kb-head-title">
      <h1 class="kb-h1">PMV Generator</h1>
      <p class="kb-sub">Pick a song and a folder of clips – on every beat it cuts to the next one. Live, and optionally recorded as a video. Everything runs in your browser: your files are never uploaded.</p>
    </div></header>
    <div class="kb-pmvg">
      <div class="kb-pmvg-main">
      <section class="kb-card kb-pmvg-song">
        <h2><span class="kb-pmvg-no">1</span>Music</h2>
        <div class="kb-seg" data-seg="mode"><button type="button" data-v="song">Your song</button><button type="button" data-v="tpl">PMV as template</button></div>
        <div data-tplpane hidden>
          <p class="kb-hint">An existing PMV is analyzed: music, every cut, the layouts (split screens) and flashes are taken over – only the content comes from your clips. Effects like glitch or text are burned into the picture; instead, your effects run at the same moments.</p>
          <div data-tplpick>
            <div class="kb-pmvg-row">
              <label class="kb-search kb-pmvg-tplq">${icon("search")}<input class="kb-field" type="search" data-tplq placeholder="Search a PMV among your clips …"></label>
              <button class="kb-btn" type="button" data-tplfile>${icon("film")}Choose file</button>
              <input type="file" accept="video/*,.mp4,.webm,.mkv,.mov" data-tplinput hidden>
            </div>
            <div class="kb-pmvg-tplist" data-tplist></div>
          </div>
          <div class="kb-pmvg-tplprog" data-tplprog hidden>
            <b data-tplname></b><span data-tpltext></span>
            <div class="kb-job-bar"><i data-tplbar style="width:0%"></i></div>
            <button class="kb-btn is-ghost" type="button" data-tplcancel>Cancel</button>
          </div>
          <div data-tplinfo hidden>
            <div class="kb-pmvg-songhead"><b data-tplname2></b><span data-tplstats></span></div>
            <canvas class="kb-pmvg-wave" data-tplwave width="1200" height="120"></canvas>
            <div class="kb-chips kb-pmvg-legend" data-tpllegend></div>
            <div class="kb-card-acts"><span class="kb-spacer"></span><button class="kb-btn is-ghost" type="button" data-tplother>Other template</button></div>
          </div>
        </div>
        <div data-songpane>
        <label class="kb-pmvg-drop" data-drop>
          <input type="file" accept="audio/*,.mp3,.m4a,.wav,.ogg,.flac,.opus" data-file hidden>
          ${icon("music")}<b>Drop a song here</b><small>or click · MP3, M4A, WAV, OGG, FLAC</small>
        </label>
        <div data-songinfo hidden>
          <div class="kb-pmvg-songhead"><b data-songname></b><span data-bpm></span></div>
          <canvas class="kb-pmvg-wave" data-wave width="1200" height="120"></canvas>
          <div class="kb-card-acts">
            <button class="kb-btn" data-tempo="0.5" title="If detected twice as fast">½ tempo</button>
            <button class="kb-btn" data-tempo="2" title="If detected half as fast">2× tempo</button>
            <button class="kb-btn is-ghost" data-nudge="-0.02" title="Cuts 20 ms earlier">Earlier</button>
            <button class="kb-btn is-ghost" data-nudge="0.02" title="Cuts 20 ms later">Later</button>
            <span class="kb-spacer"></span>
            <button class="kb-btn is-ghost" data-other>Other song</button>
          </div>
        </div>
        </div>
      </section>

      <section class="kb-card kb-pmvg-clips" data-clips>
        <h2><span class="kb-pmvg-no">2</span>Clips</h2>
        <div class="kb-pmvg-drop kb-pmvg-libdrop" data-libdrop role="button" tabindex="0">
          ${icon("folder")}<b>Drop folders with videos and images here</b>
          <small>or click to choose a folder · <button type="button" class="kb-pmvg-link" data-addfiles>pick single files</button> · nothing is uploaded</small>
        </div>
        <div class="kb-pmvg-libbar" data-libbar hidden>
          ${icon("folder")}<span data-libsum></span><span class="kb-spacer"></span>
          <button class="kb-btn" type="button" data-addfolder>${icon("plus")}Add folder</button>
          <button class="kb-btn is-ghost" type="button" data-addfiles>Add files</button>
          <button class="kb-btn is-ghost" type="button" data-libclear>Remove all</button>
        </div>
        <input type="file" webkitdirectory multiple hidden data-folderinput>
        <input type="file" multiple accept="video/*,image/*,.mkv" hidden data-filesinput>
        <div class="kb-pmvg-cols">
          <div class="kb-pmvg-block">
            <span class="kb-lab-t">What</span>
            <div class="kb-seg" data-seg="source"><button type="button" data-v="scene">Videos</button><button type="button" data-v="image">Images</button><button type="button" data-v="both">Both</button></div>
            <span class="kb-lab-t">Clip shape</span>
            <div class="kb-seg" data-seg="shape"><button type="button" data-v="all">All</button><button type="button" data-v="portrait">Portrait only</button><button type="button" data-v="landscape">Landscape only</button></div>
          </div>
          <div class="kb-pmvg-block">
            <span class="kb-lab-t">Folders <small>– including subfolders</small></span>
            <div class="kb-pmvg-folders" data-folders></div>
          </div>
        </div>
        <p class="kb-hint kb-pmvg-count" data-count></p>
        <span class="kb-lab-t">Clip selection</span>
        <div class="kb-pmvg-opts">
          ${sw("bestSpots", "Best moments instead of random", "Looks at several spots per video (motion, skin, contrast) and takes the best one")}
          ${sw("smartCrop", "Smart crop", "The crop follows what matters in the clip instead of sticking to the center")}
          ${sw("matchCut", "Match cuts", "At each cut, the clip that best matches the previous one in color, brightness and composition comes next")}
          ${sw("variety", "Variety", "The same clip doesn't come up again shortly after")}
        </div>
      </section>

      <section class="kb-card">
        <h2><span class="kb-pmvg-no">3</span>Style</h2>
        <p class="kb-hint kb-pmvg-tip" data-tplnote hidden>Template active: cuts, layouts and flashes come from the PMV. Here you choose the effects that are added at those moments.</p>
        <div class="kb-pmvg-presets">
          <span class="kb-lab-t">Mood <small>– sets cutting, layouts and effects in one go</small></span>
          <div class="kb-chips" data-presets>${Object.entries(PRESETS).map(([k, p]) => `<button type="button" class="kb-chip kb-pmvg-preset" data-preset="${k}">${p.name}</button>`).join("")}</div>
        </div>
        <div class="kb-pmvg-tabs" role="tablist">${TABS.map(([k, l]) => `<button type="button" role="tab" data-tab="${k}">${l}<small data-tabsum="${k}"></small></button>`).join("")}</div>

        <div class="kb-pmvg-pane" data-pane="cut">
          <span class="kb-lab-t">Cutting</span>
          <div class="kb-seg" data-seg="cut">${CUTS.map(([v, l, t]) => `<button type="button" data-v="${v}" title="${esc(t)}">${l}</button>`).join("")}</div>
          <span class="kb-lab-t">Layouts <small>– change to the beat, the louder the more fields</small></span>
          <div class="kb-chips kb-pmvg-layouts" data-layouts>${Object.entries(LAYOUTS).map(([k, l]) => `<button type="button" class="kb-chip" data-l="${k}" title="${esc(l.hint)}">${layoutIcon(k)}${l.name}</button>`).join("")}</div>
          <span class="kb-lab-t">Fields in 2-/3-way layouts</span>
          <div class="kb-seg" data-seg="split"><button type="button" data-v="cols" title="Columns – also in portrait format">side by side</button><button type="button" data-v="rows" title="Rows">stacked</button></div>
          <p class="kb-hint kb-pmvg-tip" data-tip hidden></p>
        </div>

        <div class="kb-pmvg-pane" data-pane="fx" hidden>
          ${FX_GROUPS.map(([name, sub, keys]) => `
            <div class="kb-pmvg-group">
              <div class="kb-pmvg-grouphead"><b>${name}</b>${sub ? `<small>${sub}</small>` : ""}<span class="kb-spacer"></span>
                <button type="button" class="kb-btn is-ghost kb-pmvg-all" data-all="${keys.join(",")}">All on</button></div>
              <div class="kb-pmvg-opts">${keys.map(fxSw).join("")}</div>
              ${keys.includes("text") ? `<input class="kb-field kb-pmvg-words" data-words placeholder="Words for “Text”, comma separated – e.g. DROP, MORE, YES" value="${esc(S.words)}">` : ""}
            </div>`).join("")}
        </div>

        <div class="kb-pmvg-pane" data-pane="look" hidden>
          <span class="kb-lab-t">Color look <small>– all clips in the same color mood</small></span>
          <div class="kb-seg kb-pmvg-looks" data-seg="look">${LOOKS.map(([v, l]) => `<button type="button" data-v="${v}"><i class="kb-pmvg-lookdot is-${v}"></i>${l}</button>`).join("")}</div>
          <div class="kb-pmvg-opts">
            ${sw("lookEven", "Even out brightness", "Clips that are too dark get brightened, too bright ones toned down – looks all of a piece")}
            ${LOOK_FX.map(fxSw).join("")}
          </div>
          <span class="kb-lab-t">Picture</span>
          <div class="kb-pmvg-row">
            <div class="kb-seg" data-seg="format"><button type="button" data-v="16:9">16:9 landscape</button><button type="button" data-v="9:16">9:16 portrait</button></div>
            <div class="kb-seg" data-seg="fit"><button type="button" data-v="cover" title="Picture fills everything, edges are cropped">Fill</button><button type="button" data-v="contain" title="Whole picture, rest blurred">Fit</button></div>
          </div>
        </div>

        <div class="kb-pmvg-pane" data-pane="sound" hidden>
          <span class="kb-lab-t">Volume</span>
          <div class="kb-pmvg-sound">
            <label class="kb-pmvg-range"><span>${icon("music")}Song</span><input type="range" min="0" max="100" step="5" data-r="songVol" aria-label="Song volume"><output data-ro="songVol"></output></label>
            <label class="kb-pmvg-range" data-clipvol><span>${icon("film")}Clips</span><input type="range" min="0" max="100" step="5" data-r="clipVol" aria-label="Clip volume"><output data-ro="clipVol"></output></label>
          </div>
          <div class="kb-pmvg-opts">${fxSw("voice")}</div>
          <span class="kb-lab-t" data-voicewhen>Clip audio plays</span>
          <div class="kb-seg" data-seg="voiceMode"><button type="button" data-v="drops" title="Fade in briefly on drops only – like the voice-overs in real PMVs">Only on drops</button><button type="button" data-v="always" title="The clips can be heard all the time, under the song">Always</button></div>
          <p class="kb-hint" data-voicehint></p>
          <p class="kb-hint">Both end up in the recording exactly like this. With several clips at once (split screen) they share the clip volume.</p>
        </div>

        <div class="kb-pmvg-pane" data-pane="out" hidden>
          <div class="kb-pmvg-opts">
            ${sw("intro", "Intro", "Title card at the start: your title slams in, comic-SFX style")}
            ${sw("outro", "Outro", "Credits at the end: the picture fades dark, title and number of clips")}
          </div>
          <input class="kb-field" data-title placeholder="Title for intro/outro – empty = song name" value="${esc(S.title)}">
          <div class="kb-pmvg-opts">
            ${sw("record", "Record", "Saves the result as a video file (WebM) you can download")}
          </div>
          <span class="kb-lab-t">Recording quality</span>
          <div class="kb-seg" data-seg="quality"><button type="button" data-v="720">720p</button><button type="button" data-v="1080">1080p</button></div>
        </div>
      </section>
      </div>

      <aside class="kb-card kb-pmvg-go">
        <h2><span class="kb-pmvg-no">4</span>Go</h2>
        <ul class="kb-pmvg-sum" data-sum></ul>
        <button class="kb-btn is-primary kb-pmvg-start" data-start disabled>${icon("bolt")}Pick a song first</button>
        <p class="kb-hint">Keys while it runs: Space pause · F fullscreen · Esc stop</p>
      </aside>
    </div>`;
  const $ = (s) => main.querySelector(s);

  // ---------- Show settings ----------

  function paintSegs() {
    main.querySelectorAll("[data-seg]").forEach((seg) => {
      seg.querySelectorAll("[data-v]").forEach((b) => b.classList.toggle("is-on", String(S[seg.dataset.seg]) === b.dataset.v));
    });
    main.querySelectorAll("[data-layouts] [data-l]").forEach((b) => b.classList.toggle("is-on", !!S.layouts[b.dataset.l]));
    main.querySelectorAll("[data-t]").forEach((c) => (c.checked = !!getPath(S, c.dataset.t)));
    main.querySelectorAll("[data-all]").forEach((b) => {
      const on = b.dataset.all.split(",").every((k) => S.fx[k]);
      b.textContent = on ? "All off" : "All on";
    });
    const words = $("[data-words]");
    if (words) words.hidden = !S.fx.text;
    $("[data-title]").hidden = !S.intro && !S.outro;
    main.querySelectorAll("[data-tab]").forEach((b) => {
      const on = b.dataset.tab === S.tab;
      b.classList.toggle("is-on", on);
      b.setAttribute("aria-selected", on);
    });
    main.querySelectorAll("[data-pane]").forEach((p) => (p.hidden = p.dataset.pane !== S.tab));
    main.querySelectorAll("[data-r]").forEach((r) => {
      r.value = S[r.dataset.r];
      r.style.setProperty("--p", S[r.dataset.r] + "%");
    });
    main.querySelectorAll("[data-ro]").forEach((o) => (o.textContent = S[o.dataset.ro] + " %"));
    // Without clip audio, clip volume and "when" have no effect
    ["[data-clipvol]", "[data-seg=voiceMode]", "[data-voicewhen]"].forEach((q) => $(q).classList.toggle("is-dim", !S.fx.voice));
    $("[data-voicehint]").textContent = !S.fx.voice
      ? "Clip audio is off – only the song can be heard."
      : S.voiceMode === "always"
      ? "The clips play audibly under the song."
      : "On every drop, the original audio of the largest clip fades in for a few beats.";
    paintSummary();
  }

  // Summary: on the tabs and in the Go card
  function paintSummary() {
    const fxOn = Object.keys(FX).filter((k) => S.fx[k] && !LOOK_FX.includes(k) && k !== "voice").length;
    const lays = Object.keys(LAYOUTS).filter((k) => S.layouts[k]).length;
    const look = LOOKS.find(([v]) => v === S.look)[1];
    const tabSum = {
      cut: `${CUTS.find(([v]) => v === S.cut)[1]} · ${lays} ${lays === 1 ? "layout" : "layouts"}`,
      fx: `${fxOn} on`,
      look: `${look} · ${S.format}`,
      sound: `Song ${S.songVol} · Clips ${S.fx.voice ? S.clipVol : "off"}`,
      out: [S.intro && "Intro", S.outro && "Outro", S.record && "Recording"].filter(Boolean).join(" · ") || "live only",
    };
    main.querySelectorAll("[data-tabsum]").forEach((s) => (s.textContent = tabSum[s.dataset.tabsum]));
    const clipOpts = [S.bestSpots && "best moments", S.smartCrop && "smart crop", S.matchCut && "match cuts", S.variety && "variety"].filter(Boolean);
    const src = { scene: "Videos", image: "Images", both: "Videos + images" }[S.source];
    const where = S.folders.length ? `from ${S.folders.length === 1 ? "1 folder" : S.folders.length + " folders"}` : "from all folders";
    $("[data-sum]").innerHTML = [
      `<li><b>Clips</b>${esc(src)} ${esc(where)}${S.shape !== "all" ? ` · ${S.shape} only` : ""}</li>`,
      `<li><b>Selection</b>${clipOpts.length ? esc(clipOpts.join(", ")) : "random"}</li>`,
      `<li><b>Cutting</b>${esc(tabSum.cut)}</li>`,
      `<li><b>Effects</b>${fxOn} on${S.look !== "none" ? " · look " + esc(look) : ""}</li>`,
      `<li><b>Sound</b>Song ${S.songVol} % · clips ${S.fx.voice ? `${S.clipVol} %, ${S.voiceMode === "always" ? "always" : "only on drops"}` : "off"}</li>`,
      `<li><b>Output</b>${esc(S.format)} · ${esc(tabSum.out)}${S.record ? " · " + S.quality + "p" : ""}</li>`,
    ].join("");
  }
  paintSegs();

  // Tip: three full-size portrait clips side by side only work in landscape format
  function paintTip() {
    const tip = main.querySelector("[data-tip]");
    let t = "";
    if (S.format === "9:16" && S.split === "cols") t = "In portrait format, 3 fields side by side get very narrow. For three full-size portrait clips side by side: format “16:9 landscape” (Look tab) – each column is then almost exactly 9:16.";
    else if (S.format === "16:9" && S.split === "cols" && S.shape === "landscape") t = "In 3-way layouts the columns are almost portrait – with “Landscape only” a lot gets cropped. “All” or “Portrait only” fits better.";
    tip.textContent = t;
    tip.hidden = !t;
  }
  paintTip();
  main.addEventListener("click", (e) => {
    const b = e.target.closest("[data-seg] [data-v]");
    if (b) {
      const key = b.closest("[data-seg]").dataset.seg;
      S[key] = key === "quality" ? Number(b.dataset.v) : b.dataset.v;
      save();
      paintSegs();
      if (key === "source" || key === "shape") updateCount();
      paintTip();
    }
    const tab = e.target.closest("[data-tab]");
    if (tab) {
      S.tab = tab.dataset.tab;
      save();
      paintSegs();
    }
    const all = e.target.closest("[data-all]");
    if (all) {
      const keys = all.dataset.all.split(",");
      const on = !keys.every((k) => S.fx[k]);
      keys.forEach((k) => (S.fx[k] = on && (k !== "strobe" || !!S.fx.strobe))); // strobe only on purpose, one by one
      save();
      paintSegs();
    }
    const l = e.target.closest("[data-layouts] [data-l]");
    if (l) {
      S.layouts[l.dataset.l] = !S.layouts[l.dataset.l];
      if (!Object.values(S.layouts).some(Boolean)) S.layouts.full = true; // at least one
      save();
      paintSegs();
    }
    const pr = e.target.closest("[data-preset]");
    if (pr) {
      const P = PRESETS[pr.dataset.preset];
      S.cut = P.cut;
      Object.keys(S.layouts).forEach((k) => (S.layouts[k] = P.layouts.includes(k)));
      Object.keys(S.fx).forEach((k) => (S.fx[k] = P.fx.includes(k) && (k !== "text" || !!S.words.trim())));
      save();
      paintSegs();
      toast(`Mood “${P.name}”`, "ok");
    }
  });
  $("[data-words]").addEventListener("input", (e) => {
    S.words = e.target.value;
    save();
  });
  main.addEventListener("input", (e) => {
    const r = e.target.closest("[data-r]");
    if (!r) return;
    S[r.dataset.r] = Number(r.value);
    save();
    paintSegs();
  });
  $("[data-title]").addEventListener("input", (e) => {
    S.title = e.target.value;
    save();
  });
  main.addEventListener("change", (e) => {
    const c = e.target.closest("[data-t]");
    if (!c) return;
    setPath(S, c.dataset.t, c.checked);
    save();
    paintSegs();
    if (c.dataset.t === "fx.text" && S.fx.text) $("[data-words]").focus();
  });
  folderPicker($("[data-folders]"), {
    selected: S.folders,
    onChange: (list) => {
      S.folders = list;
      save();
      paintSummary();
      updateCount();
    },
  });

  // ---------- Your clips ----------

  const folderInput = $("[data-folderinput]");
  const filesInput = $("[data-filesinput]");
  const addFrom = (list) => {
    const n = lib.addFiles(list);
    if (!n) toast(list.length ? "No new videos or images found there" : "That folder is empty", "error");
    else toast(`${fmtNum(n)} ${n === 1 ? "clip" : "clips"} added`, "ok");
  };
  folderInput.onchange = () => {
    addFrom(lib.fromInput(folderInput.files));
    folderInput.value = "";
  };
  filesInput.onchange = () => {
    addFrom(lib.fromInput(filesInput.files));
    filesInput.value = "";
  };
  const libDrop = $("[data-libdrop]");
  libDrop.addEventListener("click", (e) => !e.target.closest("[data-addfiles]") && folderInput.click());
  libDrop.addEventListener("keydown", (e) => {
    if (e.target !== libDrop || (e.key !== "Enter" && e.key !== " ")) return;
    e.preventDefault();
    folderInput.click();
  });
  main.querySelectorAll("[data-addfiles]").forEach((b) => (b.onclick = () => filesInput.click()));
  $("[data-addfolder]").onclick = () => folderInput.click();
  $("[data-libclear]").onclick = () => lib.clear();
  // The whole card takes dropped folders and files
  const clips = $("[data-clips]");
  clips.addEventListener("dragover", (e) => {
    e.preventDefault();
    clips.classList.add("is-over");
  });
  clips.addEventListener("dragleave", (e) => !clips.contains(e.relatedTarget) && clips.classList.remove("is-over"));
  clips.addEventListener("drop", async (e) => {
    e.preventDefault();
    clips.classList.remove("is-over");
    try {
      addFrom(await lib.fromDrop(e.dataTransfer));
    } catch (err) {
      errorToast(err, "Clips");
    }
  });

  function paintLib() {
    const all = lib.all();
    const ok = all.filter((it) => !it.bad);
    const v = ok.filter((it) => it.kind === "video").length;
    const i = ok.length - v;
    const folders = new Set(all.map((it) => it.folder.split("/")[0])).size;
    $("[data-libdrop]").hidden = !!all.length;
    $("[data-libbar]").hidden = !all.length;
    const bad = all.length - ok.length;
    $("[data-libsum]").textContent =
      `${fmtNum(v)} ${v === 1 ? "video" : "videos"} · ${fmtNum(i)} ${i === 1 ? "image" : "images"} from ${folders === 1 ? "1 folder" : folders + " folders"}` +
      (bad ? ` · ${fmtNum(bad)} can't be played here` : "");
  }

  // How many clips match?
  function updateCount() {
    const el = $("[data-count]");
    if (!lib.all().length) {
      el.textContent = "";
      el.classList.remove("is-warn");
      return;
    }
    const n = lib.count(S);
    const kinds = S.source === "both" ? ["scene", "image"] : [S.source];
    const total = kinds.reduce((a, k) => a + (k === "scene" ? n.videos : n.images), 0);
    el.textContent =
      kinds.map((k) => (k === "scene" ? `${fmtNum(n.videos)} ${n.videos === 1 ? "video" : "videos"}` : `${fmtNum(n.images)} ${n.images === 1 ? "image" : "images"}`)).join(" + ") +
      " match" +
      (n.pending ? ` · checking the size of ${fmtNum(n.pending)} …` : "") +
      (total === 0 ? " – loosen the filters" : total < 8 ? " – rather few, clips will repeat" : "");
    el.classList.toggle("is-warn", total === 0);
  }
  const onLibrary = () => {
    if (!alive) return;
    paintLib();
    updateCount();
    paintStart();
    if (S.mode === "tpl" && !tpl && !scanAbort) listTemplates($("[data-tplq]").value.trim());
  };
  const onProbe = () => {
    if (!alive) return;
    paintLib();
    updateCount();
  };
  window.addEventListener("pmv:library", onLibrary);
  window.addEventListener("pmv:probe", onProbe);
  paintLib();
  updateCount();

  // ---------- Song ----------

  const drop = $("[data-drop]");
  const file = $("[data-file]");
  file.onchange = () => file.files[0] && loadSong(file.files[0]);
  drop.addEventListener("dragover", (e) => {
    e.preventDefault();
    drop.classList.add("is-over");
  });
  drop.addEventListener("dragleave", () => drop.classList.remove("is-over"));
  drop.addEventListener("drop", (e) => {
    e.preventDefault();
    drop.classList.remove("is-over");
    const f = [...e.dataTransfer.files].find((x) => x.type.startsWith("audio/") || /\.(mp3|m4a|wav|ogg|flac|opus|aac)$/i.test(x.name));
    if (f) loadSong(f);
    else toast("That's not an audio file", "error");
  });
  $("[data-other]").onclick = () => file.click();

  async function loadSong(f) {
    drop.classList.add("is-busy");
    drop.querySelector("b").textContent = "Detecting beats …";
    try {
      const r = await analyzeSong(await f.arrayBuffer());
      if (!alive) return;
      if (r.beats.length < 8) throw new Error("Too few beats detected – is this a song with a rhythm?");
      song = Object.assign(r, { name: f.name.replace(/\.[^.]+$/, "") });
      paintSong();
    } catch (err) {
      errorToast(err.name === "EncodingError" ? new Error("The browser can't read this audio file") : err, "Song");
    } finally {
      drop.classList.remove("is-busy");
      drop.querySelector("b").textContent = "Drop a song here";
      file.value = "";
    }
  }

  function paintSong() {
    $("[data-drop]").hidden = true;
    $("[data-songinfo]").hidden = false;
    $("[data-songname]").textContent = song.name;
    $("[data-bpm]").textContent = `${Math.round(song.bpm)} BPM · ${fmtDuration(song.duration)} · ${song.beats.length} Beats`;
    drawWave($("[data-wave]"), song);
    paintStart();
  }
  main.querySelectorAll("[data-tempo]").forEach((b) => (b.onclick = () => song && ((song = Object.assign(rescale(song, Number(b.dataset.tempo)), { name: song.name })), paintSong())));
  main.querySelectorAll("[data-nudge]").forEach((b) => (b.onclick = () => song && ((song = Object.assign(shift(song, Number(b.dataset.nudge)), { name: song.name })), paintSong())));

  $("[data-start]").onclick = () => (S.mode === "tpl" ? tpl : song) && startRun();

  function paintStart() {
    const music = S.mode === "tpl" ? !!tpl : !!song;
    const clips = lib.all().length > 0;
    const b = $("[data-start]");
    b.disabled = !music || !clips;
    b.innerHTML = `${icon("bolt")}${!music ? (S.mode === "tpl" ? "Pick a PMV first" : "Pick a song first") : !clips ? "Add clips first" : S.mode === "tpl" ? "Rebuild with my clips" : "Start"}`;
  }

  // ---------- PMV as template ----------

  let tpl = null;
  let scanAbort = null;
  function paintMode() {
    const t = S.mode === "tpl";
    $("[data-tplpane]").hidden = !t;
    $("[data-songpane]").hidden = t;
    $("[data-tplnote]").hidden = !t;
    main.querySelectorAll("[data-layouts], [data-seg=cut], [data-presets]").forEach((el) => el.classList.toggle("is-dim", t));
    if (t && !tpl && !scanAbort) listTemplates("");
    paintStart();
  }
  main.querySelector("[data-seg=mode]").addEventListener("click", () => setTimeout(paintMode));

  // Your videos as templates: names with "PMV" first
  function listTemplates(q) {
    const box = $("[data-tplist]");
    const ql = q.toLowerCase();
    const pmv = (it) => (/pmv/i.test(it.name) ? 0 : 1);
    const list = lib
      .videos()
      .filter((it) => !ql || it.path.toLowerCase().includes(ql))
      .sort((a, b) => pmv(a) - pmv(b) || a.name.localeCompare(b.name, undefined, { numeric: true }))
      .slice(0, 12);
    box.innerHTML = list.length
      ? list
          .map((it) => {
            const name = it.name.replace(/\.[^.]+$/, "");
            return `<button type="button" class="kb-pmvg-tpl" data-id="${esc(it.id)}" data-name="${esc(name)}" title="${esc(it.path)}">
              <i class="kb-pmvg-tplic">${icon("film")}</i>
              <span><b>${esc(name)}</b><small>${esc(it.dur ? fmtDuration(it.dur) : it.folder)}</small></span></button>`;
          })
          .join("")
      : `<p class="kb-hint">${lib.all().length ? "Nothing found." : "Add your clips in step 2 – or choose a video file."}</p>`;
  }
  let qTimer;
  $("[data-tplq]").addEventListener("input", (e) => {
    clearTimeout(qTimer);
    qTimer = setTimeout(() => listTemplates(e.target.value.trim()), 150);
  });
  $("[data-tplist]").addEventListener("click", (e) => {
    const b = e.target.closest("[data-id]");
    const it = b && lib.all().find((x) => x.id === b.dataset.id);
    if (it) scanFrom(b.dataset.name, async () => it.file);
  });
  $("[data-tplfile]").onclick = () => $("[data-tplinput]").click();
  $("[data-tplinput]").onchange = (e) => {
    const f = e.target.files[0];
    e.target.value = "";
    if (f) scanFrom(f.name.replace(/\.[^.]+$/, ""), async () => f);
  };
  $("[data-tplcancel]").onclick = () => scanAbort && scanAbort.abort();
  $("[data-tplother]").onclick = () => {
    tpl = null;
    $("[data-tplinfo]").hidden = true;
    $("[data-tplpick]").hidden = false;
    paintStart();
  };

  function progress(p, text) {
    $("[data-tplbar]").style.width = `${Math.round(p * 100)}%`;
    $("[data-tpltext]").textContent = text;
  }

  async function scanFrom(name, getBlob) {
    if (scanAbort) return;
    scanAbort = new AbortController();
    $("[data-tplpick]").hidden = true;
    $("[data-tplinfo]").hidden = true;
    $("[data-tplprog]").hidden = false;
    $("[data-tplname]").textContent = name;
    progress(0, "Loading …");
    try {
      const blob = await getBlob();
      const r = await scanPmv(blob, (p, text) => progress(0.1 + 0.9 * p, text), scanAbort.signal);
      if (!alive) return;
      if (r.song.beats.length < 8) throw new Error("Too few beats detected – does the video have music?");
      r.song.name = name;
      tpl = Object.assign(r, { name });
      $("[data-tplinfo]").pmvTemplate = () => tpl; // for tests
      paintTpl();
    } catch (err) {
      if (err.name !== "AbortError") errorToast(err, "Template");
      $("[data-tplpick]").hidden = false;
    } finally {
      scanAbort = null;
      $("[data-tplprog]").hidden = true;
    }
  }

  function paintTpl() {
    $("[data-tplinfo]").hidden = false;
    $("[data-tplname2]").textContent = tpl.name;
    const st = tpl.stats;
    $("[data-tplstats]").textContent = `${Math.round(tpl.song.bpm)} BPM · ${fmtDuration(tpl.song.duration)} · ${st.cuts} cuts · ${st.layoutChanges} layout changes · ${st.flashes} flashes`;
    drawTimeline($("[data-tplwave]"), tpl);
    const total = Object.values(st.layoutTime).reduce((a, b) => a + b, 0) || 1;
    $("[data-tpllegend]").innerHTML = Object.entries(st.layoutTime)
      .sort((a, b) => b[1] - a[1])
      .map(([k, sec]) => `<span class="kb-chip"><i class="kb-pmvg-sw" style="background:${LAYOUT_COLORS[k]}"></i>${esc((LAYOUTS[k] || { name: k }).name)} ${Math.round((sec / total) * 100)} %</span>`)
      .join("");
    paintStart();
  }
  paintMode();

  // ---------- Run ----------

  function startRun() {
    run = new Generator(song, JSON.parse(JSON.stringify(S)), onRunClosed, S.mode === "tpl" ? tpl : null);
  }
  // The sound controls in the running show save themselves – take the settings over here afterwards
  function onRunClosed(g) {
    run = g || null;
    if (g || !alive) return;
    const saved = store.get("pmvgen", {});
    ["songVol", "clipVol", "voiceMode"].forEach((k) => saved[k] != null && (S[k] = saved[k]));
    if (saved.fx) S.fx.voice = !!saved.fx.voice;
    paintSegs();
  }

  return () => {
    alive = false;
    window.removeEventListener("pmv:library", onLibrary);
    window.removeEventListener("pmv:probe", onProbe);
    if (run) run.close();
  };
}

const LAYOUT_COLORS = { full: "#6d3a63", kaleido: "#b48cff", duo: "#8fe3ff", trim: "#ff3e8a", tri: "#ff9ec4", quad: "#ffd166" };

// Timeline of a template: sections colored by layout, cuts on top, flashes at the bottom
function drawTimeline(canvas, tpl) {
  const c = canvas.getContext("2d");
  const { width: W, height: H } = canvas;
  const d = tpl.song.duration || 1;
  c.clearRect(0, 0, W, H);
  tpl.segs.forEach((sg) => {
    c.fillStyle = LAYOUT_COLORS[sg.layout] || "#555";
    c.globalAlpha = 0.85;
    c.fillRect((sg.t / d) * W, 22, Math.max(1, ((sg.end - sg.t) / d) * W - 1), H - 44);
  });
  c.globalAlpha = 1;
  tpl.events.forEach((ev) => {
    const x = (ev.t / d) * W;
    if (ev.type === "flash") {
      c.fillStyle = ev.color;
      c.fillRect(x - 1, H - 18, 3, 18);
    } else {
      c.fillStyle = ev.type === "layout" ? "#fbeff4" : "rgba(251, 239, 244, .55)";
      c.fillRect(x, 0, ev.type === "layout" ? 2 : 1, ev.type === "layout" ? 20 : 12);
    }
  });
}

function layoutIcon(id) {
  const r = (x, y, w, h, on) => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="1" fill="${on ? "currentColor" : "none"}" stroke="currentColor" stroke-width="1.2"/>`;
  const shapes = {
    full: r(1, 1, 22, 13, 1),
    kaleido: r(1, 1, 10.5, 6, 1) + r(12.5, 1, 10.5, 6, 1) + r(1, 8, 10.5, 6, 1) + r(12.5, 8, 10.5, 6, 1),
    duo: r(1, 1, 10.5, 13, 1) + r(12.5, 1, 10.5, 13, 0),
    trim: r(1, 1, 6.7, 13, 1) + r(8.7, 1, 6.6, 13, 0) + r(16.3, 1, 6.7, 13, 1),
    tri: r(1, 1, 6.7, 13, 1) + r(8.7, 1, 6.6, 13, 0) + r(16.3, 1, 6.7, 13, 0),
    quad: r(1, 1, 10.5, 6, 1) + r(12.5, 1, 10.5, 6, 0) + r(1, 8, 10.5, 6, 0) + r(12.5, 8, 10.5, 6, 1),
  };
  return `<svg class="kb-pmvg-lay" viewBox="0 0 24 15" aria-hidden="true">${shapes[id]}</svg>`;
}

function drawWave(canvas, song) {
  const c = canvas.getContext("2d");
  const { width: W, height: H } = canvas;
  c.clearRect(0, 0, W, H);
  const css = getComputedStyle(document.documentElement);
  const pink = css.getPropertyValue("--pink").trim() || "#ff3e8a";
  const faint = css.getPropertyValue("--bg-3").trim() || "#4a2342";
  // Energy as a background band: loud = strong pink
  song.beats.forEach((t, k) => {
    const x = (t / song.duration) * W;
    const w = (((song.beats[k + 1] || song.duration) - t) / song.duration) * W + 1;
    c.fillStyle = `rgba(255, 62, 138, ${0.05 + 0.35 * song.energy[k] ** 2})`;
    c.fillRect(x, 0, w, H);
  });
  c.fillStyle = faint;
  const peaks = song.peaks;
  let max = 0;
  for (const p of peaks) max = Math.max(max, p);
  for (let i = 0; i < peaks.length; i++) {
    const h = (peaks[i] / (max || 1)) * (H - 10);
    c.fillStyle = pink;
    c.globalAlpha = 0.85;
    c.fillRect((i / peaks.length) * W, (H - h) / 2, Math.max(1, W / peaks.length - 0.5), h);
  }
  c.globalAlpha = 1;
  // Bar lines (every 4th beat)
  c.fillStyle = "rgba(251, 239, 244, .35)";
  song.beats.forEach((t, k) => k % 4 === 0 && c.fillRect((t / song.duration) * W, 0, 1, 8));
}

// ==========================================================================
// Generator: prepare clips, cut to the beat, draw, record
// ==========================================================================

class Generator {
  // tpl (optional): template from pmvscan.js – then its cuts, layouts and flashes drive the show
  constructor(song, S, onClose, tpl) {
    this.song = tpl ? tpl.song : song;
    this.S = S;
    this.onClose = onClose;
    this.tpl = tpl || null;
    this.ti = 0;
    this.log = []; // sequence (cuts, layouts) – readable on the stage element for tests
    this.dir = S.split;
    const long = S.quality === 1080 ? 1920 : 1280;
    const short = S.quality === 1080 ? 1080 : 720;
    [this.W, this.H] = S.format === "9:16" ? [short, long] : [long, short];
    this.sources = [];
    this.srcIdx = 0;
    this.ready = [];
    this.preparing = 0;
    this.bad = 0;
    this.recent = []; // recently shown clips (variety)
    this.used = new Set();
    this.shown = new Set(); // all shown clips (credits)
    this.layouts = Object.keys(LAYOUTS).filter((k) => S.layouts[k]);
    if (!this.layouts.length) this.layouts = ["full"];
    this.layout = null;
    this.slots = [];
    this.groups = []; // media per group (field or mirrored pair of fields)
    this.cutT = [];
    this.nextGroup = 0;
    this.layoutBeat = 0;
    this.layoutHold = 8;
    this.lastDrop = -99;
    this.bi = 0;
    this.lastCut = -99;
    this.cuts = 0;
    this.events = []; // half beats: stutter, strobe
    this.st = { beatT: -9, beatAmt: 0, energy: 0 };
    this.paused = false;
    this.done = false;
    lib.pauseProbing(true);
    this.mount();
    this.comp = new Compositor(this.canvas, S);
    this.comp.title = String(S.title || "").trim() || this.song.name;
    this.comp.duration = this.song.duration;
    this.comp.credits = () => `${this.shown.size} Clips · ${Math.round(this.song.bpm)} BPM`;
    this.start().catch((e) => this.fail(e));
  }
  mount() {
    const el = document.createElement("div");
    el.className = "kb-overlay-host";
    el.innerHTML = `
      <div class="kb-pmvg-stage" tabindex="-1">
        <canvas class="kb-pmvg-canvas" width="${this.W}" height="${this.H}"></canvas>
        <div class="kb-pmvg-pool" aria-hidden="true"></div>
        <div class="kb-pmvg-hud">
          ${this.S.record ? '<span class="kb-pmvg-rec" title="Recording">REC</span>' : ""}
          <b>${esc(this.song.name)}</b><span data-h="bpm">${Math.round(this.song.bpm)} BPM</span><span data-h="time">0:00 / ${fmtDuration(this.song.duration)}</span><span data-h="cuts">0 cuts</span>
          <span class="kb-spacer"></span>
          <span class="kb-pmvg-hudvol" title="Song volume">${icon("music")}<input type="range" min="0" max="100" step="5" data-vol="songVol" value="${this.S.songVol ?? 100}" aria-label="Song volume"></span>
          <span class="kb-pmvg-hudvol" title="Clip volume">${icon("film")}<input type="range" min="0" max="100" step="5" data-vol="clipVol" value="${this.S.clipVol ?? 50}" aria-label="Clip volume"></span>
          <button class="kb-btn is-ghost kb-pmvg-hudmode" data-act="voicemode" title="Clip audio: off → only on drops → always"></button>
          <button class="kb-btn is-icon is-ghost" data-act="pause" title="Pause (Space)">${icon("pause")}</button>
          <button class="kb-btn is-icon is-ghost" data-act="full" title="Fullscreen (F)">${icon("expand")}</button>
          <button class="kb-btn is-icon is-ghost" data-act="stop" title="Stop (Esc)">${icon("stop")}</button>
        </div>
        <div class="kb-pmvg-bar"><i data-h="bar"></i></div>
        <div class="kb-pmvg-msg" data-h="msg">Preparing clips …</div>
        <div class="kb-pmvg-end" data-h="end" hidden></div>
      </div>`;
    document.getElementById("overlay-root").appendChild(el);
    document.body.classList.add("kb-noscroll");
    this.el = el;
    this.stage = el.querySelector(".kb-pmvg-stage");
    this.stage.pmvLog = () => this.log;
    this.stage.pmvGen = () => this; // for tests
    this.canvas = el.querySelector("canvas");
    this.g = this.canvas.getContext("2d");
    this.pool = el.querySelector(".kb-pmvg-pool");
    this.h = (k) => el.querySelector(`[data-h="${k}"]`);
    this.stage.focus();
    el.addEventListener("click", (e) => {
      const b = e.target.closest("[data-act]");
      if (!b) return;
      const a = b.dataset.act;
      if (a === "pause") this.togglePause();
      if (a === "voicemode") this.cycleVoiceMode();
      if (a === "full") this.fullscreen();
      if (a === "stop") this.done ? this.close() : this.finish(true);
    });
    this.onKey = (e) => {
      if (e.key === "Escape") this.done ? this.close() : this.finish(true);
      else if (e.key === " " && !this.done) this.togglePause();
      else if (e.key === "f" || e.key === "F") this.fullscreen();
      else return;
      e.preventDefault();
    };
    document.addEventListener("keydown", this.onKey);
    el.addEventListener("input", (e) => {
      const r = e.target.closest("[data-vol]");
      if (r) this.setVolume(r.dataset.vol, Number(r.value));
    });
    this.paintSound();
  }

  // ---------- Adjust sound live (top bar) ----------

  paintSound() {
    const S = this.S;
    const b = this.el.querySelector('[data-act="voicemode"]');
    b.textContent = !S.fx.voice ? "Clip audio off" : S.voiceMode === "always" ? "Clip audio always" : "Clip audio on drops";
    b.classList.toggle("is-on", !!S.fx.voice);
    this.el.querySelectorAll("[data-vol]").forEach((r) => {
      r.style.setProperty("--p", r.value + "%");
      if (r.dataset.vol === "clipVol") r.closest(".kb-pmvg-hudvol").classList.toggle("is-dim", !S.fx.voice);
    });
  }

  // Remember for the next show, too
  persistSound() {
    const saved = store.get("pmvgen", {});
    saved.songVol = this.S.songVol;
    saved.clipVol = this.S.clipVol;
    saved.voiceMode = this.S.voiceMode;
    saved.fx = Object.assign({}, saved.fx, { voice: !!this.S.fx.voice });
    store.set("pmvgen", saved);
  }

  setVolume(key, v) {
    this.S[key] = v;
    if (key === "songVol" && this.songGain) this.songGain.gain.setTargetAtTime(v / 100, this.ac.currentTime, 0.05);
    if (key === "clipVol") this.syncVoices();
    this.paintSound();
    this.persistSound();
  }

  cycleVoiceMode() {
    const S = this.S;
    if (!S.fx.voice) {
      S.fx.voice = true;
      S.voiceMode = "drops";
    } else if (S.voiceMode === "drops") S.voiceMode = "always";
    else S.fx.voice = false;
    // No longer "always": fade out the running clip audio
    if ((!S.fx.voice || S.voiceMode !== "always") && this.ac) {
      const t = this.ac.currentTime;
      this.videos().forEach((m) => m.voice && m.voice.gain.gain.setTargetAtTime(0, t, 0.08));
    }
    this.syncVoices();
    this.paintSound();
    this.persistSound();
  }

  fullscreen() {
    if (document.fullscreenElement) document.exitFullscreen();
    else this.stage.requestFullscreen && this.stage.requestFullscreen().catch(() => {});
  }

  say(t) {
    const m = this.h("msg");
    m.textContent = t || "";
    m.hidden = !t;
  }

  fail(e) {
    console.error("[PMV Generator]", e);
    this.say("");
    this.stopEverything();
    this.showEnd(null, e.message || String(e));
  }

  // ---------- Clips ----------

  async fetchSources() {
    const kinds = this.S.source === "both" ? ["video", "image"] : [this.S.source === "scene" ? "video" : "image"];
    const found = lib.matching(this.S);
    // A new random order each round; videos and images alternate
    const lists = kinds.map((k) =>
      shuffle(found.filter((it) => it.kind === k)).map((it) => ({ kind: it.kind, id: it.id, key: it.id, url: lib.urlOf(it), dur: it.dur || 0, item: it }))
    );
    const mixed = [];
    for (let i = 0; lists.some((l) => i < l.length); i++) lists.forEach((l) => i < l.length && mixed.push(l[i]));
    if (!mixed.length) throw new Error(lib.all().length ? "No matching clips found – loosen the filters." : "No clips yet – add a folder with videos or images.");
    this.sources = mixed;
    this.srcIdx = 0;
  }

  async nextSource() {
    // Several clips are prepared in parallel – fetch new ones only once
    for (let tries = 0; ; tries++) {
      while (this.srcIdx >= this.sources.length) {
        this.fetching = this.fetching || this.fetchSources().finally(() => (this.fetching = null));
        await this.fetching;
      }
      const s = this.sources[this.srcIdx++];
      // Variety: skip what was just shown or is being prepared (with a small selection, take it eventually)
      if (!this.S.variety || tries >= this.sources.length || !this.isRecent(s)) return s;
    }
  }

  // Variety: the same clip not within the last 24
  isRecent(s) {
    return this.used.has(s.key) || this.ready.some((m) => m.key === s.key);
  }
  remember(m) {
    this.shown.add(m.key);
    this.recent.push(m);
    if (this.recent.length > 24) this.recent.shift();
    this.used = new Set(this.recent.map((x) => x.key));
  }
  varietyPenalty(m) {
    if (!this.S.variety) return 0;
    return this.used.has(m.key) ? 3 : 0;
  }

  fillPool() {
    // Enough supply for a layout change with four new fields
    // (match cuts need a bit more choice)
    while (!this.done && this.ready.length + this.preparing < (this.S.matchCut ? 8 : 6)) {
      this.preparing++;
      this.prepareOne()
        .then((m) => {
          if (this.done) return this.release(m);
          this.ready.push(m);
          this.bad = 0;
        })
        .catch((e) => !e.skip && this.bad++) // "doesn't fit the filters" is not an error
        .finally(() => {
          this.preparing--;
          if (!this.done && this.bad < 12) setTimeout(() => this.fillPool(), this.bad ? 200 : 0);
        });
    }
  }

  async prepareOne() {
    const s = await this.nextSource();
    if (s.kind === "image") {
      const img = new Image();
      img.decoding = "async";
      img.src = s.url;
      try {
        await withTimeout(img.decode(), 8000);
      } catch (e) {
        if (e.name === "EncodingError") s.item.bad = s.item.probed = true; // the browser can't read it – leave it out
        throw e;
      }
      measured(s.item, img.naturalWidth, img.naturalHeight, 0, this.S);
      const m = { kind: "image", el: img, w: img.naturalWidth, h: img.naturalHeight, id: s.id, key: s.key, kb: Math.random() < 0.5 ? 1 : -1 };
      m.sig = analyze(img, m.w, m.h);
      if (this.S.smartCrop) m.focus = m.sig.focus;
      return m;
    }
    const v = document.createElement("video");
    v.muted = true;
    v.playsInline = true;
    v.preload = "auto";
    v.loop = true;
    this.pool.appendChild(v);
    try {
      v.src = s.url;
      await withTimeout(once(v, "loadedmetadata"), 8000);
      const dur = isFinite(v.duration) && v.duration > 0 ? v.duration : s.dur;
      measured(s.item, v.videoWidth, v.videoHeight, dur, this.S);
      const rand = () => (dur > 10 ? dur * 0.08 + Math.random() * Math.max(0, dur * 0.84 - 4) : 0);
      const seek = async (t) => {
        v.currentTime = t;
        await withTimeout(once(v, "seeked"), 8000);
        if (v.readyState < 2) await withTimeout(once(v, "loadeddata"), 8000);
      };
      let start = rand();
      let info = null;
      if (this.S.bestSpots && dur > 10) {
        // Look at several random spots; two frames each for motion
        // (when the supply runs low, only two candidates – that's faster)
        const n = this.ready.length < 2 ? 2 : 4;
        const cands = Array.from({ length: n }, () => ({ t: rand(), bonus: 0 }));
        let best = -1;
        for (const c of cands) {
          await seek(c.t);
          const a = analyze(v, v.videoWidth, v.videoHeight);
          await seek(Math.min(dur - 0.1, c.t + 0.35));
          const b = analyze(v, v.videoWidth, v.videoHeight);
          const sc = spotScore(a, b, c.bonus);
          if (sc > best) {
            best = sc;
            start = c.t;
            info = a;
          }
        }
      }
      await seek(start);
      const m = { kind: "video", el: v, w: v.videoWidth, h: v.videoHeight, id: s.id, key: s.key, start };
      m.sig = info || analyze(v, m.w, m.h);
      if (this.S.smartCrop) {
        m.focus = Object.assign({}, m.sig.focus);
        m.focusTarget = Object.assign({}, m.focus);
      }
      return m;
    } catch (e) {
      if (e.unplayable) s.item.bad = s.item.probed = true; // the browser can't play it – leave it out
      this.release({ kind: "video", el: v });
      throw e;
    }
  }

  release(m) {
    if (!m || m.kind !== "video") return;
    if (m.voice) {
      try {
        m.voice.gain.disconnect();
        m.voice.src.disconnect();
      } catch (e) { /* already disconnected */ }
      m.voice = null;
    }
    m.el.pause();
    m.el.removeAttribute("src");
    m.el.load();
    m.el.remove();
  }

  // Take the best of the ready clips: shape fits the field (portrait into a narrow field etc.),
  // match cut: looks like the clip that is leaving (out), variety: not the same clip again
  takeMedia(aspect, out) {
    if (!this.ready.length) return null;
    let outSig = null;
    if (this.S.matchCut && out) outSig = out.kind === "video" && out.el.readyState >= 2 ? analyze(out.el, out.w, out.h) : out.sig;
    let best = 0;
    let score = Infinity;
    this.ready.slice(0, this.S.matchCut ? 8 : 5).forEach((m, i) => {
      let s = Math.abs(Math.log(m.w / m.h / aspect));
      if (outSig) s += 1.5 * matchDist(outSig, m.sig);
      s += this.varietyPenalty(m);
      if (s < score) {
        score = s;
        best = i;
      }
    });
    const m = this.ready.splice(best, 1)[0];
    this.remember(m);
    m.shownAt = performance.now();
    m.zoomDir = (this.zoomFlip = !this.zoomFlip) ? 1 : -1; // zoom-in entry: alternating in/out
    if (m.kind === "video") {
      m.el.playbackRate = this.rate || 1;
      m.el.play().catch(() => {});
    }
    return m;
  }

  dropUnused(old) {
    old.forEach((m) => m && !this.groups.includes(m) && this.release(m));
  }

  cutGroup(gi, t) {
    const old = this.groups[gi];
    const m = this.takeMedia(aspectOfGroup(this.slots, gi), old);
    if (!m) return false; // nothing ready yet → the field keeps running
    this.groups[gi] = m;
    this.cutT[gi] = t;
    this.dropUnused([old]);
    this.cuts++;
    this.log.push({ t, type: "cut", group: gi });
    this.fillPool();
    this.syncVoices();
    return true;
  }

  setLayout(id, t, dir) {
    if (dir) this.dir = dir;
    const old = this.groups.slice();
    this.layout = id;
    this.slots = slotsFor(id, this.W, this.H, this.dir);
    const n = LAYOUTS[id].groups;
    this.groups = [];
    for (let gi = 0; gi < n; gi++) {
      // New clips for all fields; if some are missing, old ones keep running
      const prev = old[gi % Math.max(1, old.length)] || null;
      this.groups[gi] = this.takeMedia(aspectOfGroup(this.slots, gi), prev) || prev;
      this.cutT[gi] = t;
    }
    this.cutT.length = n;
    this.nextGroup = 0;
    this.dropUnused(old);
    this.cuts++;
    this.log.push({ t, type: "layout", layout: id, dir: this.dir });
    this.fillPool();
    this.syncVoices();
  }

  // How "full" the picture should be: calm 1 (fullscreen) … drop 4 (many fields)
  // Loud parts: mostly 3-way (mirrored ↔ different), drops and now and then 4-way
  levelFor(e, drop) {
    if (drop) return 4;
    return e > 0.6 ? 3 : e > 0.35 ? 2 : 1;
  }

  pickLayout(e, drop) {
    let want = this.levelFor(e, drop);
    if (want === 3 && e > 0.8 && Math.random() < 0.2) want = 4; // a 4-way now and then
    const dist = (k) => Math.abs(LAYOUTS[k].level - want);
    // Never the same layout again – otherwise the next best match (e.g. 4-way ↔ mirrored 3-way)
    const cands = this.layouts.filter((k) => k !== this.layout || this.layouts.length === 1);
    const min = Math.min(...cands.map(dist));
    const pool = cands.filter((k) => dist(k) === min);
    return pool[Math.floor(Math.random() * pool.length)];
  }
  async start() {
    this.fillPool();
    // Wait until the first clips are ready (max. 20 s)
    const t0 = performance.now();
    while (this.ready.length < 3 && !this.done) {
      if (this.bad >= 12 || performance.now() - t0 > 20000) {
        if (this.ready.length) break;
        throw new Error("No clips could be played. Choose other filters – the browser may not play videos in exotic formats directly.");
      }
      await sleep(100);
    }
    if (this.done) return;
    this.say("");
    this.ac = new AudioContext();
    const gain = this.ac.createGain();
    gain.connect(this.ac.destination);
    this.src = this.ac.createBufferSource();
    this.src.buffer = this.song.buffer;
    this.songGain = this.ac.createGain();
    this.songGain.gain.value = (this.S.songVol ?? 100) / 100;
    this.src.connect(this.songGain).connect(gain);
    this.master = gain; // song, clip audio and recording go through this
    this.src.onended = () => !this.paused && this.finish(false);
    if (this.S.record) this.startRecorder(gain);
    this.startAt = this.ac.currentTime + 0.12;
    this.src.start(this.startAt);
    const first = this.tpl && this.tpl.events.find((ev) => ev.type === "layout");
    if (first) this.setLayout(first.layout, 0, first.dir);
    else this.setLayout(this.layouts.includes("full") ? "full" : this.layouts[0], 0);
    this.loop = this.loop.bind(this);
    this.raf = requestAnimationFrame(this.loop);
  }

  startRecorder(gain) {
    const dest = this.ac.createMediaStreamDestination();
    gain.connect(dest);
    const stream = new MediaStream([...this.canvas.captureStream(30).getVideoTracks(), ...dest.stream.getAudioTracks()]);
    const mime = ["video/webm;codecs=vp9,opus", "video/webm;codecs=vp8,opus", "video/webm"].find((m) => window.MediaRecorder && MediaRecorder.isTypeSupported(m));
    if (!mime) {
      toast("This browser can't record – running without recording", "error");
      return;
    }
    this.chunks = [];
    this.mime = mime;
    this.rec = new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: this.S.quality === 1080 ? 12e6 : 7e6 });
    this.rec.ondataavailable = (e) => e.data.size && this.chunks.push(e.data);
    this.rec.start(1000);
  }

  now() {
    return this.ac ? this.ac.currentTime - this.startAt : 0;
  }

  loop() {
    if (this.done) return;
    const t = this.now();
    const beats = this.song.beats;
    while (this.bi < beats.length && beats[this.bi] <= t) this.onBeat(this.bi++, t);
    if (this.tpl) {
      const evs = this.tpl.events;
      while (this.ti < evs.length && evs[this.ti].t <= t) this.applyEvent(evs[this.ti++], t);
    }
    while (this.events.length && this.events[0].t <= t) {
      const ev = this.events.shift();
      if (ev.type === "stutter") this.stutter(t);
      if (ev.type === "strobe") this.comp.strobe(t);
    }
    this.trackFocus();
    this.st.t = t;
    this.st.slots = this.slots;
    this.st.groups = this.groups;
    this.st.cutT = this.cutT;
    this.st.cutCount = this.cuts;
    this.comp.draw(this.st);
    if (!this.hudT || performance.now() - this.hudT > 250) {
      this.hudT = performance.now();
      this.h("time").textContent = `${fmtDuration(Math.max(0, t))} / ${fmtDuration(this.song.duration)}`;
      this.h("cuts").textContent = `${this.cuts} cuts · ${LAYOUTS[this.layout] ? LAYOUTS[this.layout].name : ""}`;
      this.h("bar").style.width = `${Math.min(100, (t / this.song.duration) * 100)}%`;
    }
    this.raf = requestAnimationFrame(this.loop);
  }

  // Direction: decide what happens on each beat
  onBeat(k, t) {
    const S = this.S;
    const beats = this.song.beats;
    const e = this.song.energy[k] || 0;
    const prev = this.song.energy[k - 4] || 0;
    // A drop counts once – otherwise the signal stays up for several beats in a row
    const drop = e - prev > 0.35 && e > 0.6 && k - this.lastDrop >= 16;
    if (drop) this.lastDrop = k;
    const bar = k % 4 === 0;
    const len = (beats[k + 1] || t + 0.5) - beats[k];
    const comp = this.comp;
    this.st.energy = e;
    this.st.beatT = t;
    this.st.beatAmt = 0.03 + 0.07 * e + (drop ? 0.1 : 0);

    // Change layout: on drops right away; otherwise every 2 bars if the mood has changed
    // or – in loud parts – for variety. Calm parts stay calm.
    const want = this.levelFor(e, drop);
    const moodChanged = LAYOUTS[this.layout].level !== want && this.layouts.some((x) => LAYOUTS[x].level === want || want > 1);
    const due = bar && k - this.layoutBeat >= this.layoutHold && (moodChanged || (want >= 3 && Math.random() < 0.6));
    if (this.tpl) {
      // Cuts and layouts come from the template (applyEvent)
    } else if (this.layouts.length > 1 && (drop || due)) {
      this.setLayout(this.pickLayout(e, drop), t);
      this.layoutBeat = k;
      this.layoutHold = drop ? 4 : 8; // after a drop, move on after just one bar
      this.lastCut = k;
      comp.flash(t, drop ? 0.9 : 0.35, drop ? "#ff3e8a" : "#fff");
    } else {
      // Within the layout: re-cut the fields in turn
      const every = S.cut === "auto" ? (e > 0.72 ? 1 : e > 0.4 ? 2 : 4) : Number(S.cut);
      if (k - this.lastCut >= every || (every === 4 && bar && k - this.lastCut >= 2)) {
        const n = LAYOUTS[this.layout].groups;
        const gi = this.nextGroup % n;
        if (this.cutGroup(gi, t)) {
          this.nextGroup = gi + 1;
          this.lastCut = k;
          if (e > 0.5) comp.flash(t, 0.15 + 0.3 * e, bar ? "#ff3e8a" : "#fff");
        }
      }
    }

    if (S.fx.speed) this.setRate(drop ? 2 : e < 0.35 ? 0.6 : e < 0.7 ? 1 : 1.3, drop ? len * 2 : 0);
    if (S.fx.voice && S.voiceMode !== "always" && drop) this.voice(len * 4, (S.clipVol ?? 50) / 100);
    if (e > 0.7) comp.shake(t, 0.1 + 0.1 * e);
    if (drop || (e > 0.85 && bar)) comp.glitch(t, drop ? 0.3 : 0.14);
    if (drop) comp.rgb(t, 26, 0.45);
    else if (e > 0.62 && (bar || k === this.lastCut)) comp.rgb(t, 6 + 12 * e, 0.14);
    if (drop) comp.tunnel(t, Math.min(1.2, len * 2));
    if (drop || (e > 0.85 && k % 8 === 0)) comp.invert(t, len / 4);
    if (drop || (bar && e > 0.6 && Math.random() < 0.45)) comp.text(t);
    // Half beats in loud parts: stutter and strobe
    if (beats[k + 1] && e > 0.8 && !drop) {
      const mid = (beats[k] + beats[k + 1]) / 2;
      if (S.fx.stutter && k % 2 === 1) this.events.push({ t: mid, type: "stutter" });
      if (S.fx.strobe) this.events.push({ t: mid, type: "strobe" });
      this.events.sort((a, b) => a.t - b.t);
    }
    if (S.fx.strobe && e > 0.85) comp.strobe(t);
  }

  // Speed ramps: playback rate of all running clips; a burst (hold) lasts briefly
  setRate(rate, hold) {
    const now = performance.now();
    if (!hold && this.rateHold && now < this.rateHold) return;
    this.rateHold = hold ? now + hold * 1000 : 0;
    this.rate = rate;
    this.videos().forEach((m) => (m.el.playbackRate = rate));
  }

  // Hook a clip's audio into the mix (once per clip); goes through master, so into the recording too
  ensureVoice(m) {
    if (!m || m.kind !== "video" || !this.ac || !this.master) return null;
    if (!m.voice) {
      try {
        const src = this.ac.createMediaElementSource(m.el);
        const gain = this.ac.createGain();
        gain.gain.value = 0;
        src.connect(gain).connect(this.master);
        m.voice = { src, gain };
        m.el.muted = false; // audible only through the gain here from now on
      } catch (e) {
        console.warn("[PMV Generator] Clip audio", e);
        return null;
      }
    }
    return m.voice;
  }

  // Clip audio "always": all visible clips audible, together about as loud as set
  syncVoices() {
    const S = this.S;
    if (!S.fx.voice || S.voiceMode !== "always" || !this.ac) return;
    const vids = this.videos();
    const level = (S.clipVol ?? 50) / 100 / Math.sqrt(Math.max(1, vids.length));
    const t = this.ac.currentTime;
    vids.forEach((m) => {
      const v = this.ensureVoice(m);
      if (v) v.gain.gain.setTargetAtTime(level, t, 0.06);
    });
  }

  // Clip audio on drops: briefly fade in the original audio of the main clip (largest field)
  voice(dur, level) {
    if (!this.ac || !this.master) return;
    if (!this.slots || !this.slots.length) return;
    const big = this.slots.reduce((a, b) => (b.w * b.h > a.w * a.h ? b : a));
    const m = big && this.groups[big.g];
    if (!this.ensureVoice(m)) return;
    try {
      const g = m.voice.gain.gain;
      const t = this.ac.currentTime;
      g.cancelScheduledValues(t);
      g.setValueAtTime(g.value, t);
      g.linearRampToValueAtTime(level, t + 0.05);
      g.setValueAtTime(level, t + Math.max(0.1, dur - 0.2));
      g.linearRampToValueAtTime(0, t + dur);
    } catch (e) {
      console.warn("[PMV Generator] Clip audio", e);
    }
  }

  // Smart crop: re-measure the focus of running clips every 0.4 s and follow it smoothly
  // The same measurement also keeps brightness/color up to date (even out brightness)
  trackFocus() {
    const S = this.S;
    if (!S.smartCrop && !S.lookEven) return;
    const now = performance.now();
    if (!this.focusT || now - this.focusT > 400) {
      this.focusT = now;
      this.videos().forEach((m) => {
        if (m.el.readyState < 2) return;
        const a = analyze(m.el, m.w, m.h);
        if (S.smartCrop) m.focusTarget = a.focus;
        if (a.lum != null && m.sig && m.sig.lum != null) m.sig.lum += (a.lum - m.sig.lum) * 0.3;
      });
    }
    if (!S.smartCrop) return;
    this.videos().forEach((m) => {
      if (!m.focus || !m.focusTarget) return;
      m.focus.x += (m.focusTarget.x - m.focus.x) * 0.06;
      m.focus.y += (m.focusTarget.y - m.focus.y) * 0.06;
    });
  }

  // Run a template event
  applyEvent(ev, t) {
    const comp = this.comp;
    const e = this.st.energy || 0;
    if (ev.type === "flash") {
      comp.flash(t, Math.max(0.35, ev.strength), ev.color, true);
      return;
    }
    if (ev.type === "layout") {
      this.setLayout(LAYOUTS[ev.layout] ? ev.layout : "full", t, ev.dir);
      if (e > 0.6) comp.rgb(t, 8 + 14 * e, 0.16);
      if (e > 0.8) comp.glitch(t, 0.14);
    } else if (ev.groups === "all") {
      this.setLayout(this.layout, t, this.dir);
    } else {
      const n = LAYOUTS[this.layout].groups;
      const gs = (ev.groups.length ? ev.groups : [this.nextGroup % n]).filter((g) => g < n);
      gs.forEach((g) => this.cutGroup(g, t));
      this.nextGroup = (gs[gs.length - 1] || 0) + 1;
    }
    if (e > 0.65) comp.rgb(t, 5 + 10 * e, 0.12);
  }

  // Stutter: all running clips jump back briefly
  stutter(t) {
    new Set(this.groups).forEach((m, i) => {
      if (!m || m.kind !== "video") return;
      const since = t - (this.cutT[this.groups.indexOf(m)] || t);
      m.el.currentTime = m.start + Math.max(0, since * 0.25);
    });
    this.st.beatT = t;
    this.st.beatAmt = 0.05;
  }
  videos() {
    return [...new Set(this.groups)].filter((m) => m && m.kind === "video");
  }

  togglePause() {
    if (!this.ac || this.done) return;
    this.paused = !this.paused;
    const b = this.el.querySelector('[data-act="pause"]');
    b.innerHTML = icon(this.paused ? "play" : "pause");
    if (this.paused) {
      this.ac.suspend();
      this.videos().forEach((m) => m.el.pause());
      if (this.rec && this.rec.state === "recording") this.rec.pause();
      this.say("Paused – Space continues");
    } else {
      this.ac.resume();
      this.videos().forEach((m) => m.el.play().catch(() => {}));
      if (this.rec && this.rec.state === "paused") this.rec.resume();
      this.say("");
    }
  }

  stopEverything() {
    this.done = true;
    cancelAnimationFrame(this.raf);
    try {
      if (this.src) this.src.onended = null;
      if (this.src) this.src.stop();
    } catch (e) { /* already stopped */ }
    new Set(this.groups).forEach((m) => this.release(m));
    this.groups = [];
    this.ready.forEach((m) => this.release(m));
    this.ready = [];
  }
  async finish(early) {
    if (this.done) return;
    const t = this.now();
    this.stopEverything();
    this.length = Math.min(t, this.song.duration);
    let blob = null;
    if (this.rec && this.rec.state !== "inactive") {
      this.say("Finishing the video …");
      await new Promise((r) => {
        this.rec.onstop = r;
        this.rec.stop();
      });
      blob = await fixWebmDuration(new Blob(this.chunks, { type: this.mime.split(";")[0] }), this.length * 1000);
      this.say("");
    }
    if (this.ac) this.ac.close().catch(() => {});
    this.showEnd(blob, null, early);
  }

  showEnd(blob, error, early) {
    const end = this.h("end");
    this.blob = blob;
    const url = blob ? URL.createObjectURL(blob) : null;
    this.blobUrl = url;
    end.hidden = false;
    end.innerHTML = error
      ? `<div class="kb-pmvg-endcard"><h2>That didn't work</h2><p>${esc(error)}</p><div class="kb-card-acts"><button class="kb-btn" data-end="close">Back</button></div></div>`
      : `<div class="kb-pmvg-endcard">
          <h2>${early ? "Stopped" : "Done!"}</h2>
          <p>${fmtDuration(this.length || 0)} · ${this.cuts} cuts · ${Math.round(this.song.bpm)} BPM${blob ? ` · ${fmtBytes(blob.size)}` : ""}</p>
          ${url ? `<video class="kb-pmvg-result" src="${url}" controls playsinline></video>` : ""}
          <div class="kb-card-acts">
            ${blob ? `<a class="kb-btn is-primary" data-end="file" href="${url}" download="${esc(fileName(this.song.name))}.webm">${icon("download")}Download</a>` : ""}
            <button class="kb-btn" data-end="again">${icon("shuffle")}Again, reshuffled</button>
            <button class="kb-btn is-ghost" data-end="close">Close</button>
          </div>
          ${blob ? `<p class="kb-hint">WebM video – plays in the browser, VLC and most players.</p>` : ""}
        </div>`;
    end.onclick = (e) => {
      const b = e.target.closest("[data-end]");
      if (!b) return;
      const a = b.dataset.end;
      if (a === "close") this.close();
      if (a === "again") {
        const { song, S, onClose, tpl } = this;
        this.close();
        onClose(new Generator(song, S, onClose, tpl));
      }
    };
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    if (!this.done) this.stopEverything();
    if (this.rec && this.rec.state !== "inactive") this.rec.stop();
    if (this.ac && this.ac.state !== "closed") this.ac.close().catch(() => {});
    if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
    document.removeEventListener("keydown", this.onKey);
    document.body.classList.remove("kb-noscroll");
    if (this.blobUrl) setTimeout(() => URL.revokeObjectURL(this.blobUrl), 60000);
    this.el.remove();
    lib.pauseProbing(false);
    this.onClose(null);
  }
}

// ---------- Helpers ----------


const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const once = (el, ev) =>
  new Promise((res, rej) => {
    el.addEventListener(ev, res, { once: true });
    el.addEventListener("error", () => rej(Object.assign(new Error("File can't be played"), { unplayable: true })), { once: true });
  });
function withTimeout(p, ms) {
  return Promise.race([p, new Promise((_, rej) => setTimeout(() => rej(new Error("Timed out")), ms))]);
}
const fileName = (s) => ("PMV - " + s).replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").slice(0, 100);

function shuffle(a) {
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

// Remember the real size of a clip; if it doesn't fit the filters (e.g. landscape with "Portrait only"), skip it
function measured(it, w, h, dur, S) {
  if (!it.probed) Object.assign(it, { w, h, dur, probed: true });
  if (!lib.fits(it, S)) throw Object.assign(new Error("Doesn't fit the filters"), { skip: true });
}
