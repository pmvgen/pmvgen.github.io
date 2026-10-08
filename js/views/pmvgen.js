// PMV Generator: pick a song → detect beats → cut live to clips from your folders on every beat.
// Split-screen layouts (e.g. mirrored 3-way) change with the energy; the fields are re-cut in turn.
// Layouts and effects live in ../pmvfx.js.
// Optionally MediaRecorder records picture + song; the video can be downloaded.
// Everything runs in the browser – the clips are local files and are never uploaded.

import { esc, icon, toast, errorToast, fmtDuration, fmtBytes, fmtNum, store, promptDialog, confirmDialog } from "../ui.js";
import * as lib from "../library.js";
import { fixWebmDuration } from "../webmfix.js";
import { buildFunscript, FS_DEFAULTS } from "../funscriptgen.js";
import { attachHandy, getHandy, interactiveConfig, saveInteractiveConfig } from "../interactive.js";
import { analyzeFile, analyzeBuffer, sliceBuffer, songFromFrames, rescale, shift } from "../beats.js";
import { scanPmv } from "../pmvscan.js";
import { analyzeAsync, spotScore, matchDist, isCut } from "../pmvsmart.js";
import { analyzeBars } from "../bars.js";
import { folderPicker } from "./folderpick.js";
import { LAYOUTS, slotsFor, aspectOfGroup, Compositor } from "../pmvfx.js";
import { Playlist, fileTrack, filesFromDrop, isSongFile } from "../music.js";
import { plexState, plexForget, plexSignIn, plexServers, plexConnect, PlexClient, plexTracks, PlexFollow } from "../plex.js";

// "1:23", "1:02:03", "83" → seconds; "" → 0
function parseTime(v) {
  v = String(v || "").trim().replace(",", ".");
  if (!v) return 0;
  const parts = v.split(":").map(Number);
  if (parts.some((n) => !Number.isFinite(n) || n < 0)) return NaN;
  return parts.reduce((a, n) => a * 60 + n, 0);
}
function fmtTime(sec) {
  sec = Math.max(0, Math.round(sec || 0));
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const x = String(sec % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${x}` : `${m}:${x}`;
}

const DEFAULTS = {
  ...FS_DEFAULTS, // the funscript for The Handy (fsOn, fsPace, fsSize, fsWhere, fsStyle, fsRests, fsAccent, fsSpeed)
  glass: true, // liquid glass look (switch at the top right)
  mode: "song", // song = your own song(s), plex = music from Plex, tpl = use a PMV as template
  shuffle: false, // several songs: shuffled
  plexWhat: "follow", // follow = visualize what plays in Plex, list = a Plex playlist, all = all music shuffled
  plexList: "", // chosen Plex playlist
  plexSync: 0, // following Plex: cuts shifted by this (seconds)
  plexSyncs: {}, // … remembered per Plex player (phone over Bluetooth ≠ PC)
  source: "scene", // scene = videos, image = images, both
  folders: [], // [{ id, path }] – empty = all folders
  shape: "all", // clip shape: all | portrait | landscape
  layoutShape: {}, // clip shape per layout (layout id → all | portrait | landscape), missing = all
  bestSpots: true, // best moments instead of random
  cleanCuts: true, // a clip's start has no scene change in the first seconds (it would cut by itself)
  smartCrop: true, // crop follows what matters
  variety: true, // same clip not shortly after itself
  sequenced: false, // clips come from the part of their scene that matches how far the song is (start → start, end → end)
  matchCut: true, // pick the best-matching clip at each cut
  cut: "auto",
  bars: true, // cuts on the bar's beats, layouts change where a phrase starts (not just "every 4th beat")
  reveal: false, // the opening: the clip sits small in the middle (rounded corners), grows until the first drop, then the layouts start
  scroll: false, // in 3-way layouts the middle clip stays longer while the side clips scroll up or down like a feed
  layouts: { full: true, kaleido: true, duo: true, trim: true, tri: true, quad: true },
  // Effects: a calm start – zoom-in entry, flash, zoom pulse and RGB split; the rest is opt-in
  fx: { flash: true, zoom: true, shake: false, glitch: false, stutter: false, hue: false, rgb: true, echo: false, tunnel: false, invert: false, whip: false, zoomin: true, speed: false, voice: true, vhs: false, strobe: false, text: false, kenburns: true, lines: true },
  words: "",
  look: "none", // color look: none | warm | pink | cold | bw | noir | vivid | custom
  lookAmt: 100, // strength of the look (%)
  lookColor: "#ff4d94", // the tint of the "custom" look
  bright: 0, // smooth extra brightness (%)
  soft: false, // soft seams: the fields of a split screen blend into each other
  softAmt: 50, // how wide the blend is (%)
  edge: "off", // rim of the picture: off | blur | motion | lens
  edgeAmt: 50, // how strong / how far in (%)
  smooth: true, // clips scaled smoothly (less pixelated)
  pulseAmt: 100, // zoom pulse strength (%)
  pulseOn: "beat", // zoom pulse on: beat | 2 (every other beat) | bar
  pace: "normal", // automatic cuts: slow | normal | fast
  lookEven: true, // even out clip brightness
  songVol: 100, // song volume (%)
  clipVol: 50, // clip audio volume (%)
  voiceMode: "drops", // clip audio: drops = only on drops, always = all the time
  intro: false,
  outro: false,
  title: "", // empty = song name
  format: "16:9",
  split: "cols",
  fit: "contain", // contain = whole clip, blurred border (less confusing than cropping); cover = fill
  quality: 720,
  record: true,
  collapsed: {}, // style sections the user closed (all open by default)
  v: 2, // settings version
};
const FX = {
  flash: ["Flash", "Bright flash on cuts and drops"],
  zoom: ["Zoom pulse", "The picture pumps on every beat"],
  whip: ["Transitions", "New clips whip into the field with motion blur"],
  zoomin: ["Zoom-in entry", "New clips zoom into the picture fast – always zooming in, alternating strong and soft"],
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
  ["custom", "Own color"],
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
const SECTIONS = [
  ["cut", "Cutting", "When to cut and which split screens"],
  ["fx", "Effects", "What happens on cuts, beats and drops"],
  ["look", "Look & picture", "Colors, format and how clips fill the frame"],
  ["sound", "Sound", "Song and clip volume"],
  ["fs", "Funscript", "A script for The Handy that follows the song"],
  ["out", "Output", "Intro, outro and recording"],
];
const CHEVRON = `<svg class="kb-pmvg-chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m6 9 6 6 6-6" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

// A row with a switch; path = "bestSpots" or "fx.flash"
const sw = (path, name, desc) =>
  `<label class="kb-pmvg-opt"><span><b>${esc(name)}</b>${desc ? `<small>${esc(desc)}</small>` : ""}</span>` +
  `<span class="kb-switch"><input type="checkbox" data-t="${path}"><i></i></span></label>`;
const fxSw = (k) => sw("fx." + k, FX[k][0], FX[k][1]);
// Header of a style section: the whole bar opens/closes it
function secHead(key) {
  const [, name, sub] = SECTIONS.find(([k]) => k === key);
  return `<button type="button" class="kb-pmvg-sechead" data-toggle="${key}" aria-controls="pmvg-pane-${key}">
    <span class="kb-pmvg-sectitle"><b>${name}</b><small data-tabsum="${key}"></small></span>
    <span class="kb-pmvg-secsub">${sub}</span>${CHEVRON}</button>`;
}
const getPath = (S, p) => p.split(".").reduce((o, k) => (o ? o[k] : undefined), S);
function setPath(S, p, v) {
  const ks = p.split(".");
  const last = ks.pop();
  ks.reduce((o, k) => o[k], S)[last] = v;
}

export function render(main) {
  const stored = store.get("pmvgen", {});
  const S = Object.assign({}, DEFAULTS, stored);
  S.fx = Object.assign({}, DEFAULTS.fx, S.fx);
  S.layouts = Object.assign({}, DEFAULTS.layouts, S.layouts);
  S.collapsed = Object.assign({}, S.collapsed);
  // Settings from before version 2 had "Fill" as default – people took the cropping for a bug
  if (!stored.v) S.fit = "contain";
  if (!Array.isArray(S.folders)) S.folders = [];
  const save = () => store.set("pmvgen", S);
  let song = null; // beat detection result + name
  let run = null; // running generator
  let alive = true;
  // Liquid glass: switch at the top right, on by default
  const setGlass = () => document.documentElement.classList.toggle("kb-glass", S.glass !== false);
  setGlass();
  main.addEventListener("change", (e) => {
    if (!e.target.matches("[data-pglass]")) return;
    S.glass = e.target.checked;
    save();
    setGlass();
  });

  main.innerHTML = `
    <header class="kb-head"><div class="kb-head-title">
      <h1 class="kb-h1">PMV Generator</h1>
      <p class="kb-sub">Pick a song and a folder of clips – on every beat it cuts to the next one. Live, and optionally recorded as a video. Everything runs in your browser: your files are never uploaded.</p>
    </div>
    <div class="kb-head-tools"><label class="kb-theme-inline"><span class="kb-switch"><input type="checkbox" data-pglass${S.glass !== false ? " checked" : ""}><i></i></span>Liquid glass</label></div></header>
    <div class="kb-pmvg">
      <div class="kb-pmvg-main">
      <section class="kb-card kb-pmvg-song">
        <h2><span class="kb-pmvg-no">1</span>Music</h2>
        <div class="kb-seg" data-seg="mode"><button type="button" data-v="song">Your song</button><button type="button" data-v="plex">Plex</button><button type="button" data-v="tpl">PMV as template</button></div>
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
        <div data-plexpane hidden>
          <div data-plexsign>
            <p class="kb-hint">Sign in with your Plex account – then the generator becomes a visualizer for the music you play in Plex (Plexamp, your phone, the web …), or it plays your Plex playlists. You confirm on plex.tv; no password passes through here, and the key stays in this browser.</p>
            <div class="kb-card-acts"><button class="kb-btn is-primary" type="button" data-plexin>${icon("plug")}Sign in with Plex</button></div>
            <p class="kb-hint" data-plexwait hidden></p>
          </div>
          <div data-plexservers hidden>
            <span class="kb-lab-t">Which server?</span>
            <div class="kb-pmvg-tplist" data-plexsrv></div>
          </div>
          <div data-plexok hidden>
            <div class="kb-pmvg-songhead"><b data-plexname></b><span class="kb-spacer"></span><button class="kb-btn is-ghost" type="button" data-plexother>Other server</button><button class="kb-btn is-ghost" type="button" data-plexbye>Sign out</button></div>
            <div class="kb-seg" data-seg="plexWhat"><button type="button" data-v="follow">Follow what's playing</button><button type="button" data-v="list">A playlist</button><button type="button" data-v="all">Shuffle all</button></div>
            <div data-plexfollow>
              <p class="kb-hint">Play music in any Plex app – Plexamp, your phone, the web … The show follows it: next song, pause, seeking. The sound comes from your Plex player; here you only get the pictures.</p>
              <div class="kb-pmvg-np" data-plexnp></div>
            </div>
            <div data-plexlists hidden>
              <div class="kb-pmvg-tplist" data-plexpl></div>
              ${sw("shuffle", "Shuffle", "")}
            </div>
            <p class="kb-hint" data-plexall hidden>Random songs from all your music libraries, one after another.</p>
          </div>
        </div>
        <div data-songpane>
        <label class="kb-pmvg-drop" data-drop>
          <input type="file" accept="audio/*,video/*,.mp3,.m4a,.wav,.ogg,.flac,.opus,.mp4,.webm,.mov,.m4v" data-file multiple hidden>
          ${icon("music")}<b>Drop a song here</b><small>or click · MP3, M4A, WAV, OGG, FLAC – or a video, then its music is used · several songs or a folder: they play one after another</small>
        </label>
        <div data-songinfo hidden>
          <div class="kb-pmvg-songhead"><b data-songname></b><span data-bpm></span></div>
          <canvas class="kb-pmvg-wave" data-wave width="1200" height="120"></canvas>
          <div class="kb-pmvg-listinfo" data-listinfo hidden>
            <p class="kb-hint" data-listn></p>
            ${sw("shuffle", "Shuffle", "")}
          </div>
          <div class="kb-card-acts">
            <button class="kb-btn" data-tempo="0.5" title="If detected twice as fast">½ tempo</button>
            <button class="kb-btn" data-tempo="2" title="If detected half as fast">2× tempo</button>
            <button class="kb-btn is-ghost" data-nudge="-0.02" title="Cuts 20 ms earlier">Earlier</button>
            <button class="kb-btn is-ghost" data-nudge="0.02" title="Cuts 20 ms later">Later</button>
            <span class="kb-spacer"></span>
            <button class="kb-btn is-ghost" data-other>Other song</button>
          </div>
          <div class="kb-pmvg-trim">
            <span>Only use</span>
            <input class="kb-field" data-tfrom value="0:00" aria-label="From">
            <span>–</span>
            <input class="kb-field" data-tto aria-label="To">
            <button class="kb-btn" type="button" data-trim>Cut</button>
            <button class="kb-btn is-ghost" type="button" data-untrim hidden>Whole song again</button>
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
          ${sw("cleanCuts", "Clean cuts", "With best moments: a clip starts where its scene runs on for the next few seconds – no hidden cut inside the clip that jumps to another scene by itself")}
          ${sw("smartCrop", "Smart crop", "The crop follows what matters in the clip instead of sticking to the center")}
          ${sw("matchCut", "Match cuts", "At each cut, the clip that best matches the previous one in color, brightness and composition comes next")}
          ${sw("sequenced", "Follow the scenes' timeline", "A clip comes from the part of its scene that matches how far the song is: the start of the song uses the beginnings of the scenes, the end of the song their endings (a song of known length only)")}
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
        <div class="kb-pmvg-sechead-all"><span class="kb-lab-t">Settings</span><button type="button" class="kb-btn is-ghost" data-collapseall></button></div>

        <div class="kb-pmvg-sec" data-sec="cut">
          ${secHead("cut")}
          <div class="kb-pmvg-pane" id="pmvg-pane-cut" data-pane="cut">
            <span class="kb-lab-t">When to cut</span>
            <div class="kb-seg" data-seg="cut">${CUTS.map(([v, l, t]) => `<button type="button" data-v="${v}" title="${esc(t)}">${l}</button>`).join("")}</div>
            <div data-pacebox>
              <span class="kb-lab-t">Pace <small>– how fast “Automatic” cuts</small></span>
              <div class="kb-seg" data-seg="pace"><button type="button" data-v="slow" title="Calm: every 8 beats · medium: every 4 · loud: every 2">Slow</button><button type="button" data-v="normal" title="Calm: every 4 beats · medium: every 2 · loud: every beat">Normal</button><button type="button" data-v="fast" title="Calm: every 2 beats · medium and loud: every beat">Fast</button></div>
            </div>
            <div class="kb-pmvg-opts">
              ${sw("reveal", "Reveal opening", "The first clip sits small in the middle with rounded corners and slowly grows – at the first drop the picture opens up into the layouts (songs with a known length; not with templates)")}
              ${sw("scroll", "Scrolling sides", "In 3-way layouts the middle clip stays longer while the clips at the sides scroll up or down, like swiping through a feed (needs the 3-way layouts)")}
              ${sw("bars", "Bars and phrases", "Finds the \"one\" of each bar and where a phrase begins: cuts land on the strong beats, split screens change at the start of a phrase")}
            </div>
            <span class="kb-lab-t">Layouts <small>– change to the beat, the louder the more fields</small></span>
            <div class="kb-chips kb-pmvg-layouts" data-layouts>${Object.entries(LAYOUTS).map(([k, l]) => `<button type="button" class="kb-chip" data-l="${k}" title="${esc(l.hint)}">${layoutIcon(k)}${l.name}</button>`).join("")}</div>
            <span class="kb-lab-t">Clip shape per layout <small>– e.g. landscape clips only in full screen, portrait only in 3-way (set “Clip shape” in What to “All”)</small></span>
            <div class="kb-pmvg-lshape" data-lshape>${Object.entries(LAYOUTS).map(([k, l]) => `<label class="kb-pmvg-lsrow" data-lsrow="${k}"><span>${layoutIcon(k)}${l.name}</span><select class="kb-field" data-ls="${k}"><option value="all">All</option><option value="landscape">Landscape only</option><option value="portrait">Portrait only</option></select></label>`).join("")}</div>
            <span class="kb-lab-t">Fields in 2-/3-way layouts</span>
            <div class="kb-seg" data-seg="split"><button type="button" data-v="cols" title="Columns – also in portrait format">side by side</button><button type="button" data-v="rows" title="Rows">stacked</button></div>
            <p class="kb-hint kb-pmvg-tip" data-tip hidden></p>
          </div>
        </div>

        <div class="kb-pmvg-sec" data-sec="fx">
          ${secHead("fx")}
          <div class="kb-pmvg-pane" id="pmvg-pane-fx" data-pane="fx">
            <div class="kb-pmvg-group" data-pulsebox>
              <div class="kb-pmvg-grouphead"><b>Zoom pulse</b><small>How strong and on which beats the picture pumps</small></div>
              <div class="kb-pmvg-sound">
                <label class="kb-pmvg-range"><span>${icon("sliders")}Strength</span><input type="range" min="0" max="100" step="5" data-r="pulseAmt" aria-label="Zoom pulse strength"><output data-ro="pulseAmt"></output></label>
              </div>
              <div class="kb-seg" data-seg="pulseOn"><button type="button" data-v="beat" title="On every beat">Every beat</button><button type="button" data-v="2" title="On every other beat">Every 2nd</button><button type="button" data-v="bar" title="Only on the first beat of each bar">Each bar</button></div>
            </div>
            ${FX_GROUPS.map(([name, sub, keys]) => `
              <div class="kb-pmvg-group">
                <div class="kb-pmvg-grouphead"><b>${name}</b>${sub ? `<small>${sub}</small>` : ""}<span class="kb-spacer"></span>
                  <button type="button" class="kb-btn is-ghost kb-pmvg-all" data-all="${keys.join(",")}">All on</button></div>
                <div class="kb-pmvg-opts">${keys.map(fxSw).join("")}</div>
                ${keys.includes("text") ? `<input class="kb-field kb-pmvg-words" data-words placeholder="Words for “Text”, comma separated – e.g. DROP, MORE, YES" value="${esc(S.words)}">` : ""}
              </div>`).join("")}
          </div>
        </div>

        <div class="kb-pmvg-sec" data-sec="look">
          ${secHead("look")}
          <div class="kb-pmvg-pane" id="pmvg-pane-look" data-pane="look">
            <span class="kb-lab-t">Color look <small>– all clips in the same color mood</small></span>
            <div class="kb-seg kb-pmvg-looks" data-seg="look">${LOOKS.map(([v, l]) => `<button type="button" data-v="${v}"><i class="kb-pmvg-lookdot is-${v}"></i>${l}</button>`).join("")}</div>
            <div class="kb-pmvg-sound" data-lookbox>
              <label class="kb-pmvg-range"><span>${icon("sliders")}Strength</span><input type="range" min="0" max="100" step="5" data-r="lookAmt" aria-label="Strength of the color look"><output data-ro="lookAmt"></output></label>
              <label class="kb-pmvg-range" data-colorrow><span>${icon("image")}Color</span><input type="color" class="kb-pmvg-color" data-sel="lookColor" aria-label="Color of your own look"></label>
            </div>
            <span class="kb-lab-t">Brighter <small>– lifts the mid-tones smoothly, without burning out the highlights</small></span>
            <div class="kb-pmvg-sound">
              <label class="kb-pmvg-range"><span>${icon("eye")}Brightness</span><input type="range" min="0" max="100" step="5" data-r="bright" aria-label="Brightness"><output data-ro="bright"></output></label>
            </div>
            <span class="kb-lab-t">Soft seams <small>– the line between the clips of a split screen is soft instead of sharp</small></span>
            <div class="kb-pmvg-opts">
              ${sw("soft", "Soften the seams", "No sharp lines between the fields: the seam is smeared softly (the clips don't overlap)")}
            </div>
            <div class="kb-pmvg-sound" data-softbox>
              <label class="kb-pmvg-range"><span>${icon("sliders")}Softness</span><input type="range" min="0" max="100" step="5" data-r="softAmt" aria-label="How soft the seams are"><output data-ro="softAmt"></output></label>
            </div>
            <span class="kb-lab-t">Rim of the picture <small>– only the edges, the middle stays sharp</small></span>
            <div class="kb-seg" data-seg="edge"><button type="button" data-v="off">Off</button><button type="button" data-v="blur" title="Soft blur towards the edges">Blur</button><button type="button" data-v="motion" title="Light motion blur: streaks sideways at the left and right edge, up and down at the top and bottom">Motion</button><button type="button" data-v="lens" title="The edges look bent outwards, like through a lens">Lens</button></div>
            <div class="kb-pmvg-sound" data-edgebox>
              <label class="kb-pmvg-range"><span>${icon("sliders")}Strength</span><input type="range" min="0" max="100" step="5" data-r="edgeAmt" aria-label="Strength of the rim effect"><output data-ro="edgeAmt"></output></label>
            </div>
            <div class="kb-pmvg-opts">
              ${sw("smooth", "Smooth scaling", "Clips are scaled with the best quality – less pixelated and jagged, a bit more work for the computer")}
              ${sw("lookEven", "Even out brightness", "Clips that are too dark get brightened, too bright ones toned down – looks all of a piece")}
              ${LOOK_FX.map(fxSw).join("")}
            </div>
            <span class="kb-lab-t">Picture <small>– “Fit” shows the whole clip, “Fill” crops it to fill the frame</small></span>
            <div class="kb-pmvg-row">
              <div class="kb-seg" data-seg="format"><button type="button" data-v="16:9">16:9 landscape</button><button type="button" data-v="9:16">9:16 portrait</button></div>
              <div class="kb-seg" data-seg="fit"><button type="button" data-v="contain" title="Whole picture, rest blurred">Fit</button><button type="button" data-v="cover" title="Picture fills everything, edges are cropped">Fill</button></div>
            </div>
          </div>
        </div>

        <div class="kb-pmvg-sec" data-sec="sound">
          ${secHead("sound")}
          <div class="kb-pmvg-pane" id="pmvg-pane-sound" data-pane="sound">
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
        </div>

        <div class="kb-pmvg-sec" data-sec="fs">
          ${secHead("fs")}
          <div class="kb-pmvg-pane" id="pmvg-pane-fs" data-pane="fs">
            <div class="kb-pmvg-opts">${sw("fsOn", "Play a funscript with the song", "Built from the song's beats and energy and played on The Handy together with the PMV (connection key: Stash UI → Settings → Interactive). Not with Plex or a live app – only with your own song files. After the show you can save it as a .funscript")}</div>
            <div data-fsbox>
              <span class="kb-lab-t">Handy connection key <small>– from handyfeeling.com / the Handy app; kept in this browser only</small></span>
              <input class="kb-field" type="text" data-handykey placeholder="Connection key" autocomplete="off" spellcheck="false" aria-label="Handy connection key">
              <span class="kb-lab-t">Pace <small>– strokes per beat</small></span>
              <div class="kb-seg" data-seg="fsPace"><button type="button" data-v="auto" title="Calm parts slow, loud parts and drops fast">Follow the song</button><button type="button" data-v="slow" title="One stroke every 2 beats">Slow</button><button type="button" data-v="normal" title="One stroke per beat">Normal</button><button type="button" data-v="fast" title="Two strokes per beat">Fast</button></div>
              <span class="kb-lab-t">Stroke size</span>
              <div class="kb-seg" data-seg="fsSize"><button type="button" data-v="auto" title="Louder = bigger strokes">Follow the song</button><button type="button" data-v="small">Small</button><button type="button" data-v="medium">Medium</button><button type="button" data-v="large">Large</button><button type="button" data-v="full" title="The whole length, 0–100">Full</button></div>
              <span class="kb-lab-t">Where on the stroke</span>
              <div class="kb-seg" data-seg="fsWhere"><button type="button" data-v="low" title="Strokes in the lower part">Low</button><button type="button" data-v="mid">Middle</button><button type="button" data-v="high" title="Strokes in the upper part">High</button></div>
              <span class="kb-lab-t">Style</span>
              <div class="kb-seg" data-seg="fsStyle"><button type="button" data-v="sharp" title="Straight lines from beat to beat">Sharp</button><button type="button" data-v="smooth" title="Rounded, flowing strokes">Smooth</button></div>
              <div class="kb-pmvg-opts">${sw("fsRests", "Gentle in calm parts", "Quiet passages get slow, small strokes")}${sw("fsAccent", "Accents", "A bigger stroke on the first beat of each bar and on drops")}</div>
              <div class="kb-pmvg-sound"><label class="kb-pmvg-range"><span>${icon("sliders")}Top speed</span><input type="range" min="100" max="600" step="25" data-r="fsSpeed" aria-label="Top speed of the script"><output data-ro="fsSpeed"></output></label></div>
              <p class="kb-hint">Top speed limits how fast the device is asked to move (units per second) – fast strokes get smaller instead. The script starts with the song; if it runs early or late, Stash UI's sync offset (Settings → Interactive) applies.</p>
            </div>
          </div>
        </div>

        <div class="kb-pmvg-sec" data-sec="out">
          ${secHead("out")}
          <div class="kb-pmvg-pane" id="pmvg-pane-out" data-pane="out">
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
        </div>
      </section>
      </div>

      <aside class="kb-card kb-pmvg-go">
        <h2><span class="kb-pmvg-no">4</span>Go</h2>
        <ul class="kb-pmvg-sum" data-sum></ul>
        <button class="kb-btn is-primary kb-pmvg-start" data-start disabled>${icon("bolt")}Pick a song first</button>
        <p class="kb-hint">Keys while it runs: Space pause · F fullscreen · H hide the bar · I clip info · Esc stop</p>
        <div class="kb-pmvg-presets">
          <span class="kb-lab-t">My settings</span>
          <div class="kb-pmvg-row">
            <select class="kb-field" data-mypreset aria-label="Saved settings"></select>
            <button class="kb-btn" type="button" data-psave title="Save everything about clips, cutting, effects, look and sound under a name">Save …</button>
          </div>
          <div class="kb-pmvg-row">
            <button class="kb-btn is-ghost" type="button" data-pdel>Delete</button>
            <button class="kb-btn is-ghost" type="button" data-pexport title="As a file – to keep or to share">${icon("download")}Export</button>
            <button class="kb-btn is-ghost" type="button" data-pimport>Import</button>
            <input type="file" accept=".json,application/json" data-pfile hidden>
          </div>
        </div>
      </aside>
    </div>`;
  const $ = (s) => main.querySelector(s);

  // ---------- Show settings ----------

  function paintSegs() {
    main.querySelectorAll("[data-seg]").forEach((seg) => {
      seg.querySelectorAll("[data-v]").forEach((b) => b.classList.toggle("is-on", String(S[seg.dataset.seg]) === b.dataset.v));
    });
    main.querySelectorAll("[data-layouts] [data-l]").forEach((b) => b.classList.toggle("is-on", !!S.layouts[b.dataset.l]));
    main.querySelectorAll("[data-lsrow]").forEach((r) => {
      r.hidden = !S.layouts[r.dataset.lsrow];
      r.querySelector("select").value = (S.layoutShape || {})[r.dataset.lsrow] || "all";
    });
    $("[data-pacebox]").hidden = S.cut !== "auto";
    $("[data-colorrow]").hidden = S.look !== "custom";
    $("[data-lookbox]").hidden = S.look === "none";
    $("[data-edgebox]").hidden = S.edge === "off";
    $("[data-softbox]").hidden = !S.soft;
    $("[data-pulsebox]").hidden = !S.fx.zoom;
    $("[data-fsbox]").hidden = !S.fsOn;
    main.querySelectorAll("[data-t]").forEach((c) => (c.checked = !!getPath(S, c.dataset.t)));
    main.querySelectorAll("[data-all]").forEach((b) => {
      const on = b.dataset.all.split(",").every((k) => S.fx[k]);
      b.textContent = on ? "All off" : "All on";
    });
    const words = $("[data-words]");
    if (words) words.hidden = !S.fx.text;
    $("[data-title]").hidden = !S.intro && !S.outro;
    main.querySelectorAll("[data-sec]").forEach((sec) => {
      const open = !S.collapsed[sec.dataset.sec];
      sec.classList.toggle("is-open", open);
      sec.querySelector("[data-toggle]").setAttribute("aria-expanded", open);
      sec.querySelector("[data-pane]").hidden = !open;
    });
    const anyOpen = SECTIONS.some(([k]) => !S.collapsed[k]);
    $("[data-collapseall]").innerHTML = `${CHEVRON}${anyOpen ? "Collapse all" : "Expand all"}`;
    $("[data-collapseall]").classList.toggle("is-closed", !anyOpen);
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

  // Summary: on the section headers and in the Go card
  function paintSummary() {
    const fxOn = Object.keys(FX).filter((k) => S.fx[k] && !LOOK_FX.includes(k) && k !== "voice").length;
    const lays = Object.keys(LAYOUTS).filter((k) => S.layouts[k]).length;
    const look = LOOKS.find(([v]) => v === S.look)[1];
    const tabSum = {
      cut: `${CUTS.find(([v]) => v === S.cut)[1]} · ${lays} ${lays === 1 ? "layout" : "layouts"}`,
      fx: `${fxOn} on`,
      look: `${look} · ${S.format}`,
      sound: `Song ${S.songVol} · Clips ${S.fx.voice ? S.clipVol : "off"}`,
      fs: S.fsOn ? `${S.fsPace === "auto" ? "follows the song" : S.fsPace} · ${S.fsSize === "auto" ? "auto size" : S.fsSize}` : "off",
      out: [S.intro && "Intro", S.outro && "Outro", S.record && "Recording"].filter(Boolean).join(" · ") || "live only",
    };
    main.querySelectorAll("[data-tabsum]").forEach((s) => (s.textContent = tabSum[s.dataset.tabsum]));
    const clipOpts = [S.bestSpots && "best moments", S.cleanCuts && "clean cuts", S.smartCrop && "smart crop", S.matchCut && "match cuts", S.sequenced && "scene timeline", S.variety && "variety"].filter(Boolean);
    const src = { scene: "Videos", image: "Images", both: "Videos + images" }[S.source];
    const where = S.folders.length ? `from ${S.folders.length === 1 ? "1 folder" : S.folders.length + " folders"}` : "from all folders";
    $("[data-sum]").innerHTML = [
      `<li><b>Clips</b>${esc(src)} ${esc(where)}${S.shape !== "all" ? ` · ${S.shape} only` : ""}</li>`,
      `<li><b>Selection</b>${clipOpts.length ? esc(clipOpts.join(", ")) : "random"}</li>`,
      `<li><b>Cutting</b>${esc(tabSum.cut)}</li>`,
      `<li><b>Effects</b>${fxOn} on${S.look !== "none" ? " · look " + esc(look) : ""}</li>`,
      `<li><b>Sound</b>Song ${S.songVol} % · clips ${S.fx.voice ? `${S.clipVol} %, ${S.voiceMode === "always" ? "always" : "only on drops"}` : "off"}</li>`,
      S.fsOn ? `<li><b>Funscript</b>${esc(tabSum.fs)}</li>` : "",
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
    const tog = e.target.closest("[data-toggle]");
    if (tog) {
      S.collapsed[tog.dataset.toggle] = !S.collapsed[tog.dataset.toggle];
      save();
      paintSegs();
    }
    if (e.target.closest("[data-collapseall]")) {
      const close = SECTIONS.some(([k]) => !S.collapsed[k]);
      S.collapsed = close ? Object.fromEntries(SECTIONS.map(([k]) => [k, true])) : {};
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
  main.addEventListener("change", (e) => {
    const sel = e.target.closest("[data-ls]");
    if (!sel) return;
    S.layoutShape = Object.assign({}, S.layoutShape);
    if (sel.value === "all") delete S.layoutShape[sel.dataset.ls];
    else S.layoutShape[sel.dataset.ls] = sel.value;
    save();
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
  // The Handy's connection key (the web version keeps it in this browser)
  interactiveConfig().then((c) => {
    const f = main.querySelector("[data-handykey]");
    if (!f) return;
    f.value = c.handyKey || "";
    f.addEventListener("change", () => saveInteractiveConfig({ handyKey: f.value.trim() }));
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
  const fresh = (q) => {
    const old = $(q);
    const el = old.cloneNode(false);
    old.replaceWith(el);
    return el;
  };
  const mountFolders = () => folderPicker(fresh("[data-folders]"), {
    selected: S.folders,
    onChange: (list) => {
      S.folders = list;
      save();
      paintSummary();
      updateCount();
    },
  });
  mountFolders();

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
  file.onchange = () => {
    const fs = [...file.files];
    if (fs.length > 1) loadList(fs);
    else if (fs[0]) loadSong(fs[0]);
  };
  drop.addEventListener("dragover", (e) => {
    e.preventDefault();
    drop.classList.add("is-over");
  });
  drop.addEventListener("dragleave", () => drop.classList.remove("is-over"));
  drop.addEventListener("drop", async (e) => {
    e.preventDefault();
    drop.classList.remove("is-over");
    const all = await filesFromDrop(e.dataTransfer);
    const songs = all.filter(isSongFile);
    if (songs.length > 1) return loadList(songs);
    const f = songs[0] || all.find((x) => /^video\//.test(x.type) || /\.(mp4|webm|mov|m4v)$/i.test(x.name));
    if (f) loadSong(f);
    else toast("That's not an audio or video file", "error");
  });
  $("[data-other]").onclick = () => file.click();

  let songFull = null; // the whole decoded song, so "Cut" can be changed again
  let playlist = null; // several songs (music.js)
  async function loadList(files) {
    const songs = files.filter(isSongFile).sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
    if (songs.length < 2) return songs[0] ? loadSong(songs[0]) : toast("There are no songs in there", "error");
    drop.classList.add("is-busy");
    drop.querySelector("b").textContent = "Detecting beats …";
    try {
      const list = new Playlist(songs.map(fileTrack), { shuffle: !!S.shuffle });
      const first = await list.first();
      if (!alive) return;
      playlist = list;
      song = first;
      songFull = null;
      paintSong();
    } catch (err) {
      errorToast(err.name === "EncodingError" ? new Error("The browser can't read the sound of the first song") : err, "Songs");
    } finally {
      drop.classList.remove("is-busy");
      drop.querySelector("b").textContent = "Drop a song here";
      file.value = "";
    }
  }
  async function loadSong(f) {
    // A video file is decoded completely in the browser – beyond ~1.5 GB that runs out of memory
    if (/^video\//.test(f.type) && f.size > 1.5e9) return toast("This video is too big to read in the browser – cut the part with the song out first", "error");
    drop.classList.add("is-busy");
    const label = drop.querySelector("b");
    label.textContent = "Detecting beats …";
    try {
      // Long mixes are read piece by piece – that takes a moment, so show how far it is
      const r = await analyzeFile(f, (p) => (label.textContent = `Reading a long song … ${Math.round(p * 100)} %`));
      if (!alive) return;
      if (r.beats.length < 8) throw new Error("Too few beats detected – is this a song with a rhythm?");
      if (songFull && songFull.media) URL.revokeObjectURL(songFull.media);
      song = Object.assign(r, { name: f.name.replace(/\.[^.]+$/, "") });
      songFull = r.long ? { frames: r.frames, media: r.media, duration: r.duration, name: song.name } : { buffer: r.buffer, name: song.name };
      playlist = null;
      $("[data-tfrom]").value = "0:00";
      $("[data-tto]").value = fmtTime(r.duration);
      $("[data-untrim]").hidden = true;
      paintSong();
    } catch (err) {
      errorToast(err.name === "EncodingError" ? new Error("The browser can't read the sound of this file") : err, "Song");
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
    drawWave($("[data-wave]"), song, songBars(S, song));
    // Several songs: tempo and cutting are per song – they're detected anew for each one
    $("[data-listinfo]").hidden = !playlist;
    if (playlist) $("[data-listn]").textContent = `${playlist.size} songs – the show goes from one to the next (N / P skip). This is the first one.`;
    main.querySelectorAll("[data-songinfo] [data-tempo], [data-songinfo] [data-nudge], .kb-pmvg-trim").forEach((el) => (el.hidden = !!playlist));
    paintStart();
  }
  // Only a part of the song: cut the decoded sound and detect the beats again
  $("[data-trim]").onclick = async () => {
    if (!songFull) return;
    const full = songFull.buffer ? songFull.buffer.duration : songFull.duration;
    const a = parseTime($("[data-tfrom]").value);
    const b = parseTime($("[data-tto]").value) || full;
    if (!(a >= 0) || !(b > a) || a >= full) return toast("From/to don't fit – e.g. 0:45 to 3:30", "error");
    try {
      // A long song: from its stored curves; it then plays from "from" to "to" in the file
      const r = songFull.frames
        ? Object.assign(songFromFrames(songFull.frames, a, Math.min(b, full)), { media: songFull.media, offset: a, end: Math.min(b, full) })
        : await analyzeBuffer(sliceBuffer(songFull.buffer, a, Math.min(b, full)));
      if (r.beats.length < 8) throw new Error("Too few beats in this part");
      song = Object.assign(r, { name: songFull.name });
      $("[data-untrim]").hidden = false;
      paintSong();
    } catch (err) {
      errorToast(err, "Song");
    }
  };
  $("[data-untrim]").onclick = async () => {
    if (!songFull) return;
    const r = songFull.frames ? Object.assign(songFromFrames(songFull.frames), { media: songFull.media }) : await analyzeBuffer(songFull.buffer);
    song = Object.assign(r, { name: songFull.name });
    $("[data-tfrom]").value = "0:00";
    $("[data-tto]").value = fmtTime(r.duration);
    $("[data-untrim]").hidden = true;
    paintSong();
  };

  main.querySelectorAll("[data-tempo]").forEach((b) => (b.onclick = () => song && ((song = Object.assign(rescale(song, Number(b.dataset.tempo)), { name: song.name })), paintSong())));
  main.querySelectorAll("[data-nudge]").forEach((b) => (b.onclick = () => song && ((song = Object.assign(shift(song, Number(b.dataset.nudge)), { name: song.name })), paintSong())));

  $("[data-start]").onclick = () => {
    if (waiting) return (waiting.cancelled = true);
    if (canStart() && lib.all().length) startRun();
  };

  const canStart = () => (S.mode === "tpl" ? !!tpl : S.mode === "plex" ? !!plex && (S.plexWhat !== "list" || !!S.plexList) : !!song);
  function paintStart() {
    if (starting) return;
    const ok = canStart();
    const clips = lib.all().length > 0;
    const b = $("[data-start]");
    b.disabled = !ok || !clips;
    const label =
      S.mode === "tpl"
        ? ok ? "Rebuild with my clips" : "Pick a PMV first"
        : S.mode === "plex"
          ? !plex ? "Connect Plex first" : !ok ? "Pick a playlist first" : S.plexWhat === "follow" ? "Start – follow Plex" : "Start"
          : ok ? "Start" : "Pick a song first";
    b.innerHTML = `${icon("bolt")}${ok && !clips ? "Add clips first" : label}`;
  }

  // ---------- My settings (presets) ----------
  // Everything about clips, cutting, effects, look and sound – not the music source or the title.
  // Kept in this browser; Export/Import moves them as a file.
  const NOT_IN_PRESET = ["collapsed", "glass", "v", "mode", "title", "shuffle", "plexWhat", "plexList", "plexSync", "plexSyncs", "liveApp"];
  const presets = () => store.get("presets", {});
  function writePresets(all) {
    store.set("presets", all);
    if (JSON.stringify(store.get("presets", null)) !== JSON.stringify(all)) throw new Error("The browser didn't keep it (storage full or blocked) – use Export instead");
  }
  const snapshot = () => {
    const o = JSON.parse(JSON.stringify(S));
    NOT_IN_PRESET.forEach((k) => delete o[k]);
    return o;
  };
  function paintPresets(sel) {
    const all = presets();
    const names = Object.keys(all).sort((a, b) => a.localeCompare(b));
    const box = $("[data-mypreset]");
    box.innerHTML = `<option value="">${names.length ? "Load saved settings …" : "Nothing saved yet"}</option>` + names.map((n) => `<option value="${esc(n)}"${n === sel ? " selected" : ""}>${esc(n)}</option>`).join("");
    $("[data-pdel]").disabled = !sel;
  }
  function applyPreset(o) {
    const keep = {};
    NOT_IN_PRESET.forEach((k) => (keep[k] = S[k]));
    const base = JSON.parse(JSON.stringify(DEFAULTS));
    Object.assign(S, base, o, keep);
    S.fx = Object.assign({}, DEFAULTS.fx, o.fx);
    S.layouts = Object.assign({}, DEFAULTS.layouts, o.layouts);
    // values this version doesn't know (older, edited or plugin files) → the default
    if (!CUTS.some(([v]) => v === S.cut)) S.cut = DEFAULTS.cut;
    if (!LOOKS.some(([v]) => v === S.look)) S.look = DEFAULTS.look;
    if (!Object.values(S.layouts).some(Boolean)) S.layouts = Object.assign({}, DEFAULTS.layouts);
    if (!Array.isArray(S.folders)) S.folders = [];
    save();
    // Everything on the page from S again
    const words = $("[data-words]");
    if (words) words.value = S.words || "";
    mountFolders();
    paintSegs();
    paintSummary();
    paintTip();
    updateCount();
    paintMode();
  }
  paintPresets("");
  $("[data-mypreset]").addEventListener("change", (e) => {
    const name = e.target.value;
    $("[data-pdel]").disabled = !name;
    if (!name) return;
    applyPreset(presets()[name] || {});
    toast(`Loaded: ${name}`, "ok");
  });
  $("[data-psave]").onclick = async () => {
    const cur = $("[data-mypreset]").value;
    const name = (await promptDialog({ title: "Save settings", label: "Clips, cutting, effects, look and sound – under this name (the same name replaces it)", value: cur, ok: "Save" })) || "";
    if (!name.trim()) return;
    try {
      writePresets(Object.assign({}, presets(), { [name.trim()]: snapshot() }));
      paintPresets(name.trim());
      toast(`Saved: ${name.trim()}`, "ok");
    } catch (err) {
      errorToast(err, "Save");
    }
  };
  $("[data-pdel]").onclick = async () => {
    const name = $("[data-mypreset]").value;
    if (!name || !(await confirmDialog({ title: `Delete “${name}”?`, text: "Only the saved settings – nothing else.", ok: "Delete", danger: true })).ok) return;
    const all = Object.assign({}, presets());
    delete all[name];
    try {
      writePresets(all);
      paintPresets("");
    } catch (err) {
      errorToast(err, "Delete");
    }
  };
  $("[data-pexport]").onclick = () => {
    const name = $("[data-mypreset]").value || "PMV settings";
    const blob = new Blob([JSON.stringify({ pmvGenerator: 1, name, settings: snapshot() }, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_") + ".pmvgen.json";
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 30000);
  };
  $("[data-pimport]").onclick = () => $("[data-pfile]").click();
  $("[data-pfile]").onchange = async (e) => {
    const f = e.target.files[0];
    e.target.value = "";
    if (!f) return;
    try {
      const d = JSON.parse(await f.text());
      const o = d && d.settings && typeof d.settings === "object" ? d.settings : null;
      if (!o) throw new Error("That's not a PMV Generator settings file");
      const name = String(d.name || f.name.replace(/(\.pmvgen)?\.json$/i, "")).slice(0, 60) || "Imported";
      applyPreset(o);
      try {
        writePresets(Object.assign({}, presets(), { [name]: o }));
        paintPresets(name);
        toast(`Imported and loaded: ${name}`, "ok");
      } catch (err) {
        toast(`Loaded: ${name} (couldn't keep it in the list: ${err.message || err})`, "error");
      }
    } catch (err) {
      errorToast(err, "Import");
    }
  };

  // ---------- Plex ----------

  let plex = null; // PlexClient of the chosen server
  let pickServer = false;
  let srvBusy = false;
  let plLists = null;
  let npTimer = 0;
  let waiting = null; // PlexFollow waiting for something to play (the start button cancels)
  let starting = false;
  const plexGone = () => {
    plexForget();
    plex = null;
    plLists = null;
    toast("Plex wants you to sign in again", "error");
    paintPlex();
  };
  function paintPlex() {
    const st = plexState();
    if (!plex && st.token && st.server && !pickServer) plex = new PlexClient(st.server);
    $("[data-plexsign]").hidden = !!st.token;
    $("[data-plexok]").hidden = !plex;
    $("[data-plexservers]").hidden = !st.token || !!plex;
    if (st.token && !plex) showServers();
    if (plex) {
      $("[data-plexname]").textContent = "Plex: " + plex.name;
      $("[data-plexfollow]").hidden = S.plexWhat !== "follow";
      $("[data-plexlists]").hidden = S.plexWhat !== "list";
      $("[data-plexall]").hidden = S.plexWhat !== "all";
      if (S.plexWhat === "list") listPlaylists();
    }
    pollNp();
    paintStart();
  }
  async function showServers() {
    if (srvBusy) return;
    srvBusy = true;
    const box = $("[data-plexsrv]");
    box.innerHTML = `<div class="kb-loading">Looking for your servers …</div>`;
    try {
      const list = await plexServers();
      if (!alive) return;
      box.pmvServers = list;
      if (list.length === 1 && !pickServer) return useServer(list[0]);
      box.innerHTML = list.length
        ? list.map((x, i) => `<button type="button" class="kb-pmvg-tpl" data-srv="${i}"><span><b>${esc(x.name)}</b><small>${x.owned ? "Your server" : "Shared with you"}${x.presence === false ? " · offline" : ""}</small></span></button>`).join("")
        : `<p class="kb-hint">No Plex server on this account.</p>`;
    } catch (err) {
      if (err.code === 401) return plexGone();
      box.innerHTML = `<p class="kb-hint">${esc(err.message)}</p>`;
    } finally {
      srvBusy = false;
    }
  }
  async function useServer(x) {
    const box = $("[data-plexsrv]");
    box.innerHTML = `<div class="kb-loading">Connecting to ${esc(x.name)} …</div>`;
    try {
      plex = new PlexClient(await plexConnect(x));
      pickServer = false;
      plLists = null;
    } catch (err) {
      box.innerHTML = `<p class="kb-hint">${esc(err.message)}</p><div class="kb-card-acts"><button class="kb-btn" type="button" data-plexother>Try again</button></div>`;
      return;
    }
    paintPlex();
  }
  $("[data-plexsrv]").addEventListener("click", (e) => {
    const b = e.target.closest("[data-srv]");
    if (b) useServer($("[data-plexsrv]").pmvServers[Number(b.dataset.srv)]);
  });
  $("[data-plexin]").onclick = async () => {
    const b = $("[data-plexin]");
    const wait = $("[data-plexwait]");
    b.disabled = true;
    try {
      await plexSignIn((url, opened) => {
        wait.hidden = false;
        wait.innerHTML = opened ? "Confirm in the Plex window …" : `Your browser blocked the window – <a href="${esc(url)}" target="_blank" rel="noopener">open the Plex sign-in</a> and confirm there.`;
      });
      paintPlex();
    } catch (err) {
      errorToast(err, "Plex");
    } finally {
      b.disabled = false;
      wait.hidden = true;
    }
  };
  $("[data-plexbye]").onclick = () => {
    plexForget();
    plex = null;
    plLists = null;
    paintPlex();
  };
  main.addEventListener("click", (e) => {
    if (!e.target.closest("[data-plexother]")) return;
    plex = null;
    plLists = null;
    pickServer = true;
    paintPlex();
  });
  main.querySelector("[data-seg=plexWhat]").addEventListener("click", () => setTimeout(paintPlex));
  async function listPlaylists() {
    const box = $("[data-plexpl]");
    if (plLists) return paintPlaylists();
    box.innerHTML = `<div class="kb-loading">Loading your playlists …</div>`;
    try {
      plLists = await plex.playlists();
      if (alive) paintPlaylists();
    } catch (err) {
      if (err.code === 401) return plexGone();
      box.innerHTML = `<p class="kb-hint">${esc(err.message)}</p>`;
    }
  }
  function paintPlaylists() {
    $("[data-plexpl]").innerHTML = plLists.length
      ? plLists.map((x) => `<button type="button" class="kb-pmvg-tpl${x.key === S.plexList ? " is-on" : ""}" data-pl="${esc(x.key)}"><span><b>${esc(x.title)}</b><small>${x.count} songs</small></span></button>`).join("")
      : `<p class="kb-hint">No music playlists on this server.</p>`;
  }
  $("[data-plexpl]").addEventListener("click", (e) => {
    const b = e.target.closest("[data-pl]");
    if (!b) return;
    S.plexList = b.dataset.pl;
    save();
    paintPlaylists();
    paintStart();
  });
  // What's playing – shown while you're on the Plex pane
  function pollNp() {
    clearTimeout(npTimer);
    if (!alive || !plex || S.mode !== "plex" || S.plexWhat !== "follow" || run || starting) return;
    const el = $("[data-plexnp]");
    plex
      .nowPlaying()
      .then(
        (np) => {
          if (!alive || starting) return;
          el.innerHTML = np
            ? `${icon(np.playing ? "play" : "pause")}<span><b>${esc(np.title)}</b><small>${esc([np.artist, np.playerName].filter(Boolean).join(" · "))}</small></span>`
            : `<span class="kb-hint">Nothing is playing on Plex right now – start a song there.</span>`;
        },
        (err) => {
          if (err.code === 401) return plexGone();
          el.innerHTML = `<span class="kb-hint">${esc(err.message)}</span>`;
        }
      )
      .finally(() => {
        clearTimeout(npTimer);
        npTimer = setTimeout(pollNp, 3000);
      });
  }
  // Several songs: shuffle on/off right away
  main.addEventListener("change", (e) => {
    if (!e.target.matches("[data-t=shuffle]")) return;
    setTimeout(() => {
      if (playlist) playlist.setShuffle(!!S.shuffle);
      main.querySelectorAll("[data-t=shuffle]").forEach((c) => (c.checked = !!S.shuffle));
    });
  });

  // ---------- PMV as template ----------

  let tpl = null;
  let scanAbort = null;
  function paintMode() {
    const t = S.mode === "tpl";
    $("[data-tplpane]").hidden = !t;
    $("[data-songpane]").hidden = S.mode !== "song";
    $("[data-plexpane]").hidden = S.mode !== "plex";
    if (S.mode === "plex") paintPlex();
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

  async function startRun() {
    if (starting) return;
    const opts = () => JSON.parse(JSON.stringify(S));
    if (S.mode === "tpl") return (run = new Generator(song, opts(), onRunClosed, tpl));
    if (S.mode === "song" && !playlist) return (run = new Generator(song, opts(), onRunClosed, null));
    starting = true;
    const b = $("[data-start]");
    const say = (t) => (b.textContent = t);
    try {
      if (S.mode === "song") {
        // The playlist starts at the song it's at
        say("Loading …");
        run = new Generator(await playlist.song(playlist.pos), opts(), onRunClosed, null, { type: "list", list: playlist });
      } else if (S.plexWhat === "follow") {
        const follow = new PlexFollow(plex);
        waiting = follow;
        say("Cancel");
        const np = $("[data-plexnp]");
        const got = await follow.first((t) => (np.textContent = t));
        waiting = null;
        if (!alive) return;
        // No recording: the sound plays in Plex, not here
        run = new Generator(got.song, Object.assign(opts(), { record: false }), onRunClosed, null, { type: "follow", follow, np: got.np });
      } else {
        b.disabled = true;
        say("Loading the songs …");
        const tracks = S.plexWhat === "list" ? await plex.playlistTracks(S.plexList) : await plex.shuffleAll();
        if (!tracks.length) throw new Error("There are no songs in there");
        const list = new Playlist(plexTracks(plex, tracks), { shuffle: S.plexWhat === "all" || !!S.shuffle });
        say("Detecting beats …");
        const first = await list.first();
        if (!alive) return;
        run = new Generator(first, opts(), onRunClosed, null, { type: "list", list });
      }
    } catch (err) {
      if (err.code === 401) plexGone();
      else if (!(waiting && waiting.cancelled)) errorToast(err, S.mode === "plex" ? "Plex" : "Songs");
    } finally {
      waiting = null;
      starting = false;
      paintStart();
      pollNp();
    }
  }
  // The sound controls in the running show save themselves – take the settings over here afterwards
  function onRunClosed(g) {
    run = g || null;
    if (g || !alive) return;
    const saved = store.get("pmvgen", {});
    ["songVol", "clipVol", "voiceMode"].forEach((k) => saved[k] != null && (S[k] = saved[k]));
    if (saved.fx) S.fx.voice = !!saved.fx.voice;
    if (saved.plexSync != null) S.plexSync = saved.plexSync;
    if (saved.plexSyncs) S.plexSyncs = saved.plexSyncs;
    paintSegs();
    pollNp();
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

// Bars and phrases of a song, worked out once per song (none for very long files)
const barsCache = new WeakMap();
function songBars(S, song) {
  if (!S.bars || !song || song.long) return null;
  if (!barsCache.has(song)) {
    let r = null;
    try {
      r = analyzeBars(song);
    } catch (e) {
      console.warn("[PMV Generator] bars", e);
    }
    barsCache.set(song, r);
  }
  return barsCache.get(song);
}

function drawWave(canvas, song, bars = null) {
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
  song.beats.forEach((t, k) => (bars ? bars.downbeat[k] : k % 4 === 0) && c.fillRect((t / song.duration) * W, 0, 1, 8));
}

// ==========================================================================
// Generator: prepare clips, cut to the beat, draw, record
// ==========================================================================

// Following Plex: a position report is about this much older than it looks when it arrives (seconds)
const LEAD = 0.1;

class Generator {
  // tpl (optional): template from pmvscan.js – then its cuts, layouts and flashes drive the show.
  // music (optional): { type: "list", list: Playlist } – song after song – or
  // { type: "follow", follow: PlexFollow, np } – no sound of its own, it follows what Plex plays.
  constructor(song, S, onClose, tpl, music) {
    this.song = tpl ? tpl.song : song;
    this.music = tpl ? null : music || null;
    this.S = S;
    this.setBars();
    this.onClose = onClose;
    this.tpl = tpl || null;
    this.ti = 0;
    this.log = []; // sequence (cuts, layouts) – readable on the stage element for tests
    this.dir = S.split;
    [this.W, this.H] = this.sizeFor();
    this.prepShape = { landscape: 0, portrait: 0 }; // clips being prepared per shape (clip shape per layout)
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
    this.leave = {}; // scroll cuts: group → { m: the clip on its way out, t, dir }
    this.scrollDir = 0; // -1 up, 1 down: the sides scroll in this layout phase (0 = no)
    this.sideN = 0;
    this.revealing = false;
    this.reveal = S.reveal && !this.tpl && !this.music && this.song && !this.song.live && Array.isArray(this.song.energy) && this.song.beats && this.song.beats.length > 16 ? this.makeReveal() : null;
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
    this.comp.duration = this.music ? 0 : this.song.duration;
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
          ${this.S.record ? '<span class="kb-pmvg-rec" title="Recording in real time – let the show run to the end, stopping early ends the video there">REC</span>' : ""}
          ${this.music && this.music.type === "follow" ? '<span class="kb-pmvg-plex" title="Follows what plays on Plex">Plex</span>' : ""}
          <b data-h="name">${esc(this.song.name)}</b>${this.music && this.music.type === "list" ? `<span data-h="track"></span>` : ""}<span data-h="bpm">${Math.round(this.song.bpm)} BPM</span><span data-h="time">0:00 / ${fmtDuration(this.song.duration)}</span><span data-h="cuts">0 cuts</span>
          <span class="kb-spacer"></span>
          ${this.music && this.music.type === "follow" ? "" : `<span class="kb-pmvg-hudvol" title="Song volume">${icon("music")}<input type="range" min="0" max="100" step="5" data-vol="songVol" value="${this.S.songVol ?? 100}" aria-label="Song volume"></span>`}
          <span class="kb-pmvg-hudvol" title="Clip volume">${icon("film")}<input type="range" min="0" max="100" step="5" data-vol="clipVol" value="${this.S.clipVol ?? 50}" aria-label="Clip volume"></span>
          <button class="kb-btn is-ghost kb-pmvg-hudmode" data-act="voicemode" title="Clip audio: off → only on drops → always"></button>
          ${
            this.music && this.music.type === "list"
              ? `<button class="kb-btn is-icon is-ghost" data-act="prev" title="Previous song (P)">${icon("prev")}</button><button class="kb-btn is-icon is-ghost" data-act="next" title="Next song (N)">${icon("next")}</button><button class="kb-btn is-icon is-ghost kb-toggle${this.music.list.shuffle ? " is-on" : ""}" data-act="shuffle" title="Shuffle">${icon("shuffle")}</button>`
              : ""
          }
          ${
            this.music && this.music.type === "follow"
              ? `<span class="kb-pmvg-sync" title="If the cuts come too early or too late: shift them ([ / ])"><button class="kb-btn is-ghost" data-act="sync-">−</button><span data-h="sync"></span><button class="kb-btn is-ghost" data-act="sync+">+</button><button class="kb-btn is-ghost" data-act="tap" title="Tap along to the beat you hear (T) – the sync sets itself">Tap</button></span>`
              : `<button class="kb-btn is-icon is-ghost" data-act="pause" title="Pause (Space)">${icon("pause")}</button>`
          }
          <button class="kb-btn is-icon is-ghost" data-act="info" title="Which clips are on screen (I)">${icon("info")}</button>
          <button class="kb-btn is-icon is-ghost" data-act="hidebar" title="Hide this bar (H) – H brings it back">${icon("close")}</button>
          <button class="kb-btn is-icon is-ghost" data-act="full" title="Fullscreen (F)">${icon("expand")}</button>
          <button class="kb-btn is-icon is-ghost" data-act="stop" title="Stop (Esc)">${icon("stop")}</button>
        </div>
        <div class="kb-pmvg-bar"><i data-h="bar"></i></div>
        <div class="kb-pmvg-info" data-h="info" hidden></div>
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
      if (a === "next") this.skip(1);
      if (a === "prev") this.skip(-1);
      if (a === "shuffle") {
        this.music.list.setShuffle(!this.music.list.shuffle);
        b.classList.toggle("is-on", this.music.list.shuffle);
        this.paintTrack();
      }
      if (a === "sync-" || a === "sync+") this.nudge(a === "sync+" ? 0.05 : -0.05);
      if (a === "tap") this.tap();
      if (a === "voicemode") this.cycleVoiceMode();
      if (a === "full") this.fullscreen();
      if (a === "info") this.toggleInfo();
      if (a === "hidebar") this.toggleBar();
      if (a === "stop") this.done ? this.close() : this.finish(true);
    });
    this.onKey = (e) => {
      if (e.key === "Escape") this.done ? this.close() : this.finish(true);
      else if (e.key === " " && !this.done && !(this.music && this.music.type === "follow")) this.togglePause();
      else if ((e.key === "n" || e.key === "N") && this.music && this.music.type === "list") this.skip(1);
      else if ((e.key === "p" || e.key === "P") && this.music && this.music.type === "list") this.skip(-1);
      else if ((e.key === "t" || e.key === "T") && this.ext && !e.repeat) this.tap();
      else if ((e.key === "[" || e.key === "]") && this.ext) this.nudge(e.key === "]" ? 0.05 : -0.05);
      else if (e.key === "f" || e.key === "F") this.fullscreen();
      else if (e.key === "i" || e.key === "I") this.toggleInfo();
      else if (e.key === "h" || e.key === "H") this.toggleBar();
      else return;
      e.preventDefault();
    };
    document.addEventListener("keydown", this.onKey);
    el.addEventListener("input", (e) => {
      const r = e.target.closest("[data-vol]");
      if (r) this.setVolume(r.dataset.vol, Number(r.value));
    });
    this.paintSound();
    // The bar goes away after 2.5 s without the mouse moving (the cursor too) – H hides it for good
    const wake = () => {
      this.stage.classList.remove("is-idle");
      clearTimeout(this.idleT);
      const sleep = () => {
        if (this.el.querySelector(".kb-pmvg-hud:hover")) this.idleT = setTimeout(sleep, 1000);
        else this.stage.classList.add("is-idle");
      };
      this.idleT = setTimeout(sleep, 2500);
    };
    el.addEventListener("mousemove", wake);
    el.addEventListener("pointerdown", wake);
    wake();
    const saved = store.get("pmvgen", {});
    this.stage.classList.toggle("is-nohud", !!saved.hudOff);
    if (saved.clipInfo) this.toggleInfo(true);
  }

  toggleBar() {
    const off = !this.stage.classList.contains("is-nohud");
    this.stage.classList.toggle("is-nohud", off);
    const saved = store.get("pmvgen", {});
    saved.hudOff = off;
    store.set("pmvgen", saved);
    if (off) toast("Bar hidden – H shows it again", "ok");
  }

  // Clip info: what's on screen (name, file, size, state) and what had to be skipped, and why
  toggleInfo(on) {
    const box = this.h("info");
    const show = on != null ? on : box.hidden;
    box.hidden = !show;
    const saved = store.get("pmvgen", {});
    saved.clipInfo = show;
    store.set("pmvgen", saved);
    if (show) this.paintInfo();
  }
  paintInfo() {
    const box = this.h("info");
    if (box.hidden) return;
    const state = (m) => {
      if (m.kind === "image") return ["image", false];
      const v = m.el;
      if (v.error) return [mediaError(v.error), true];
      if (!v.videoWidth) return ["no picture – the browser can't decode this video", true];
      if (v.readyState < 2) return ["loading …", false];
      if (m.live && m.live.lum != null && m.live.lum < 6) return ["black picture", true];
      return [v.paused ? "paused" : "playing", false];
    };
    const row = (m, i) => {
      const [st, bad] = state(m);
      const name = m.name || m.key;
      return `<div><b>${i + 1}</b> ${esc(name)}${m.file && m.file !== name ? ` <small>${esc(m.file)}</small>` : ""} <small>${m.w}×${m.h}</small> <span class="${bad ? "is-bad" : ""}">${esc(st)}</span></div>`;
    };
    const fails = (this.failed || []).slice(-5).reverse();
    box.innerHTML =
      [...new Set(this.groups)].filter(Boolean).map(row).join("") +
      (fails.length ? `<div class="kb-pmvg-infohead">Skipped</div>` + fails.map((f) => `<div class="is-bad">${esc(f.name)} <small>${esc(f.file || "")}</small> – ${esc(f.why)}${f.n > 1 ? ` <small>×${f.n}</small>` : ""}</div>`).join("") : "");
  }
  // A clip that failed: listed once (with a count) and not tried again in this show
  noteFail(s, e) {
    const why = e.why || e.message || String(e);
    this.failed = this.failed || [];
    const known = this.failed.find((f) => f.key === s.key);
    if (known) {
      known.n++;
      known.why = why;
      this.failed.push(this.failed.splice(this.failed.indexOf(known), 1)[0]);
    } else this.failed.push({ key: s.key, name: s.name || s.key, file: s.file, why, n: 1 });
    if (this.failed.length > 20) this.failed.shift();
    (this.badKeys = this.badKeys || new Set()).add(s.key);
    if (!known) console.warn(`[PMV Generator] clip skipped: ${s.name || s.key} – ${why}`);
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
    this.showEnd(null, e.message || String(e), false, e.details, e.slow);
  }

  // ---------- Clips ----------

  async fetchSources() {
    const kinds = this.S.source === "both" ? ["video", "image"] : [this.S.source === "scene" ? "video" : "image"];
    const found = lib.matching(this.S);
    // A new random order each round; videos and images alternate
    const lists = kinds.map((k) =>
      shuffle(found.filter((it) => it.kind === k)).map((it) => {
        const file = String(it.path || (it.file && it.file.name) || "").split(/[\\/]/).pop();
        return { kind: it.kind, id: it.id, key: it.id, url: lib.urlOf(it), dur: it.dur || 0, item: it, name: file.replace(/\.[^.]+$/, "") || it.id, file: it.path || file };
      })
    );
    const mixed = [];
    for (let i = 0; lists.some((l) => i < l.length); i++) lists.forEach((l) => i < l.length && mixed.push(l[i]));
    if (!mixed.length) throw new Error(lib.all().length ? "No matching clips found – loosen the filters." : "No clips yet – add a folder with videos or images.");
    this.sources = mixed;
    this.srcIdx = 0;
  }

  async nextSource(st = 0, want = null) {
    // Several clips are prepared in parallel – fetch new ones only once
    for (let tries = 0; ; tries++) {
      while (this.srcIdx >= this.sources.length) {
        this.fetching = this.fetching || this.fetchSources().finally(() => (this.fetching = null));
        await this.fetching;
      }
      // A shape is short (per-layout clip shape): take a source of that shape that is already known
      if (want) {
        const fits = (x) => (x.w || (x.item && x.item.w)) && (x.h || (x.item && x.item.h)) && !(this.badKeys && this.badKeys.has(x.key)) && (want === "portrait" ? (x.h || x.item.h) > (x.w || x.item.w) : (x.w || x.item.w) >= (x.h || x.item.h));
        // (new ones first, then not shown recently; with a small selection a repeat is better than the wrong shape)
        let k = this.sources.findIndex((x, n) => n >= this.srcIdx && fits(x) && !this.isRecent(x));
        if (k < 0) k = this.sources.findIndex((x) => fits(x) && !this.isRecent(x));
        if (k < 0) k = this.sources.findIndex(fits);
        if (k >= 0) return this.sources[k];
      }
      const s = this.sources[this.srcIdx++];
      if (this.badKeys && this.badKeys.has(s.key) && tries < this.sources.length) continue; // failed before
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
    // At most 3 at once: each one decodes and seeks its video – with 4K several at once choke the decoder
    while (!this.done && this.preparing < 3) {
      const want = this.shapeShort();
      const cap = this.S.matchCut ? 8 : 6;
      const have = this.ready.length + this.preparing;
      if (have >= cap && !(want && have < (this.S.matchCut ? 14 : 12))) break;
      this.preparing++;
      if (want) this.prepShape[want]++;
      this.prepareOne(want)
        .then((m) => {
          if (this.done) return this.release(m);
          this.ready.push(m);
          this.bad = 0;
        })
        .catch((e) => !e.skip && this.bad++) // "doesn't fit the filters" is not an error
        .finally(() => {
          this.preparing--;
          if (want) this.prepShape[want]--;
          if (!this.done && this.bad < 12) setTimeout(() => this.fillPool(), this.bad ? 200 : 0);
        });
    }
  }

  // Per-layout clip shape: which shape (landscape / portrait) the ready clips are short of (null = none)
  shapeShort() {
    const rules = this.S.layoutShape || {};
    const need = [...new Set(this.layouts.map((k) => rules[k]).filter((v) => v === "landscape" || v === "portrait"))];
    if (!need.length) return null;
    const have = (sh) => this.ready.filter((m) => (sh === "portrait" ? m.h > m.w : m.w >= m.h)).length + this.prepShape[sh];
    need.sort((a, b) => have(a) - have(b));
    return have(need[0]) < 5 ? need[0] : null;
  }

  async prepareOne(want = null) {
    const s = await this.nextSource(0, want);
    try {
      return await this.prepareFrom(s);
    } catch (e) {
      if (!e.skip) this.noteFail(s, e);
      throw e;
    }
  }

  async prepareFrom(s) {
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
      const m = { kind: "image", el: img, w: img.naturalWidth, h: img.naturalHeight, id: s.id, key: s.key, kb: Math.random() < 0.5 ? 1 : -1, name: s.name, file: s.file };
      m.sig = await analyzeAsync(img, m.w, m.h);
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
      // Opens but no picture (e.g. a codec the browser can't decode): skip it – else it's a black field
      if (!v.videoWidth || !v.videoHeight) throw Object.assign(new Error("no picture"), { why: "no picture – the browser can't decode this video (codec?)", unplayable: true });
      const dur = isFinite(v.duration) && v.duration > 0 ? v.duration : s.dur;
      measured(s.item, v.videoWidth, v.videoHeight, dur, this.S);
      // Follow the scenes' timeline: how far the song is = where in the scene the clip is taken from (± a window)
      let seq = null;
      if (this.S.sequenced && dur > 10 && this.song && this.song.duration > 0) {
        const prog = Math.min(1, Math.max(0, this.now() / this.song.duration));
        const half = Math.max(6, dur * 0.12);
        const lo = Math.max(0, Math.min(prog * dur - half, dur - 6));
        const hi = Math.min(dur - 3, Math.max(lo, prog * dur + half));
        seq = { lo, hi };
      }
      const inSeq = (t) => !seq || (t >= seq.lo && t <= seq.hi);
      const rand = () => (seq ? seq.lo + Math.random() * Math.max(0, seq.hi - seq.lo) : dur > 10 ? dur * 0.08 + Math.random() * Math.max(0, dur * 0.84 - 4) : 0);
      const seek = async (t) => {
        v.currentTime = t;
        await withTimeout(once(v, "seeked"), 8000);
        if (v.readyState < 2) await withTimeout(once(v, "loadeddata"), 8000);
      };
      let start = rand();
      let info = null;
      if (this.S.bestSpots && dur > 10) {
        // Look at several random spots; two frames each for motion. Big videos (above 1440p) and a
        // low supply look at fewer – every seek costs, and 4K seeks are slow.
        const big = v.videoWidth * v.videoHeight > 2560 * 1440;
        const urgent = this.ready.length < 2 || big;
        const n = urgent ? 2 : 4;
        const cands = Array.from({ length: n }, () => ({ t: rand(), bonus: 0 }));
        // Clean cuts: a clip plays for a few beats from its start – a scene change in that stretch would
        // jump to another scene by itself. Spots with one are passed over (up to a few more are tried).
        const clean = !!this.S.cleanCuts && !urgent;
        const span = clean ? Math.max(1.5, Math.min(4, this.song && this.song.bpm ? (4 * 60) / this.song.bpm : 2.5)) : 0;
        if (clean) for (let i = 0; i < 4; i++) cands.push({ t: rand(), bonus: 0, extra: true });
        let best = -Infinity;
        let anyClean = false;
        for (const c of cands) {
          if (c.extra && anyClean) break; // extra spots only while nothing clean is found
          const t0 = clean ? Math.min(c.t, Math.max(0, dur - span - 0.5)) : c.t;
          await seek(t0);
          const a = await analyzeAsync(v, v.videoWidth, v.videoHeight);
          await seek(Math.min(dur - 0.1, t0 + 0.35));
          const b = await analyzeAsync(v, v.videoWidth, v.videoHeight);
          let cut = clean && isCut(a, b);
          if (clean && !cut) {
            // a few more looks along the stretch
            let prev = b;
            for (const f of [0.45, 0.8]) {
              await seek(Math.min(dur - 0.1, t0 + span * f));
              const x = await analyzeAsync(v, v.videoWidth, v.videoHeight);
              if (isCut(prev, x)) {
                cut = true;
                break;
              }
              prev = x;
            }
          }
          if (clean && !cut) anyClean = true;
          // motion counts, but a "motion" that is a cut isn't one
          const sc = cut ? spotScore(a, a, c.bonus) - 5 : spotScore(a, b, c.bonus);
          if (sc > best) {
            best = sc;
            start = t0;
            info = a;
          }
        }
      }
      await seek(start);
      const m = { kind: "video", el: v, w: v.videoWidth, h: v.videoHeight, id: s.id, key: s.key, start, name: s.name, file: s.file };
      m.sig = info || (await analyzeAsync(v, m.w, m.h));
      if (this.S.smartCrop) {
        m.focus = Object.assign({}, m.sig.focus);
        m.focusTarget = Object.assign({}, m.focus);
      }
      return m;
    } catch (e) {
      if (!e.why) e.why = v.error ? mediaError(v.error) : e.message === "Timed out" ? "didn't load within 8 s" : e.message;
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
  // match cut: looks like the clip that is leaving (out), variety: not the same scene/performer
  takeMedia(aspect, out, strict = false) {
    if (!this.ready.length) return null;
    let outSig = null;
    if (this.S.matchCut && out) outSig = out.live || out.sig; // (measured in the background, see trackFocus)
    let best = 0;
    let score = Infinity;
    let cand = this.ready.map((m, i) => i);
    // Shape per layout: only landscape / portrait clips in this layout (none ready → whatever there is)
    const want = (this.S.layoutShape || {})[this.layout];
    if (want === "landscape" || want === "portrait") {
      const fit = cand.filter((i) => (want === "portrait" ? this.ready[i].h > this.ready[i].w : this.ready[i].w >= this.ready[i].h));
      if (fit.length) cand = fit;
      else if (strict) return null; // (a single field: better keep the clip than show the wrong shape)
    }
    cand.slice(0, this.S.matchCut ? 8 : 5).forEach((i) => {
      const m = this.ready[i];
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
    m.zoomDir = (this.zoomFlip = !this.zoomFlip) ? 1 : -1; // zoom-in entry: alternating strong/soft (always zooming in)
    if (m.kind === "video") {
      m.el.playbackRate = this.rate || 1;
      m.el.play().catch(() => {});
      this.watchFrames(m);
    }
    return m;
  }

  // A clip on screen got a new picture → draw soon (see loop)
  watchFrames(m) {
    const v = m.el;
    if (m.vfc || !v.requestVideoFrameCallback) return;
    m.vfc = true;
    const cb = () => {
      if (this.done || !this.groups.includes(m)) return (m.vfc = false);
      this.freshFrame = true;
      v.requestVideoFrameCallback(cb);
    };
    v.requestVideoFrameCallback(cb);
  }

  dropUnused(old) {
    old.forEach((m) => m && !this.groups.includes(m) && this.release(m));
  }

  cutGroup(gi, t, opt) {
    const old = this.groups[gi];
    const m = this.takeMedia(aspectOfGroup(this.slots, gi), old, true);
    if (!m) return false; // nothing ready yet → the field keeps running
    this.groups[gi] = m;
    this.cutT[gi] = t;
    if (opt && opt.scroll && old && old !== m) {
      // the old clip scrolls out: it keeps playing a moment longer
      const prev = this.leave[gi];
      this.leave[gi] = { m: old, t, dir: opt.scroll };
      if (prev && prev.m !== old) this.dropUnused([prev.m]);
      setTimeout(() => !this.groups.includes(old) && this.release(old), 600);
    } else {
      delete this.leave[gi];
      this.dropUnused([old]);
    }
    this.cuts++;
    this.log.push({ t, type: "cut", group: gi, key: m.key, scroll: (opt && opt.scroll) || 0 });
    this.fillPool();
    this.syncVoices();
    return true;
  }

  setLayout(id, t, dir) {
    if (dir) this.dir = dir;
    Object.values(this.leave).forEach((lv) => lv.m && this.release(lv.m)); // (scrolling clips that are still on their way out)
    this.leave = {};
    const old = this.groups.slice();
    this.layout = id;
    this.slots = slotsFor(id, this.W, this.H, this.dir);
    const n = LAYOUTS[id].groups;
    this.groups = [];
    for (let gi = 0; gi < n; gi++) {
      // New clips for all fields; if some are missing, old ones keep running
      const prev = old[gi % Math.max(1, old.length)] || null;
      this.groups[gi] = this.takeMedia(aspectOfGroup(this.slots, gi), prev, !!prev) || prev;
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

  layoutReady(id) {
    const rule = (this.S.layoutShape || {})[id];
    if (rule !== "landscape" && rule !== "portrait") return true;
    const fit = this.ready.filter((m) => (rule === "portrait" ? m.h > m.w : m.w >= m.h)).length;
    return fit >= LAYOUTS[id].groups;
  }

  pickLayout(e, drop) {
    let want = this.levelFor(e, drop);
    if (want === 3 && e > 0.8 && Math.random() < 0.2) want = 4; // a 4-way now and then
    const dist = (k) => Math.abs(LAYOUTS[k].level - want);
    // Never the same layout again – otherwise the next best match (e.g. 4-way ↔ mirrored 3-way)
    // (a layout with a clip-shape rule only opens when enough clips of that shape are ready)
    const cands = this.layouts.filter((k) => (k !== this.layout || this.layouts.length === 1) && this.layoutReady(k));
    if (!cands.length) return null;
    const min = Math.min(...cands.map(dist));
    const pool = cands.filter((k) => dist(k) === min);
    return pool[Math.floor(Math.random() * pool.length)];
  }
  // Reveal opening: where it ends – the first drop (not in the first 5 s, and within a minute), else after 32 beats
  makeReveal() {
    const { beats, energy } = this.song;
    let last = -99;
    let endK = Math.min(32, beats.length - 1);
    for (let k = 0; k < beats.length && beats[k] < 75; k++) {
      const e = energy[k] || 0;
      const prev = energy[k - 4] || 0;
      if (e - prev > 0.35 && e > 0.6 && k - last >= 16) {
        last = k;
        if (beats[k] >= 5) {
          endK = k;
          break;
        }
      }
    }
    return { endK, endT: Math.max(2, beats[endK]), done: false };
  }

  async start() {
    this.fillPool();
    // Wait until the first clips are ready (max. 20 s)
    const t0 = performance.now();
    while (this.ready.length < 3 && !this.done) {
      if (this.bad >= 12 || performance.now() - t0 > 20000) {
        if (this.ready.length) break;
        throw Object.assign(new Error("No clips could be played."), { details: (this.failed || []).slice(-8).reverse(), slow: !(this.failed || []).length });
      }
      await sleep(100);
    }
    if (this.done) return;
    this.say("");
    this.ac = new AudioContext();
    const gain = this.ac.createGain();
    gain.connect(this.ac.destination);
    this.songGain = this.ac.createGain();
    this.songGain.gain.value = (this.S.songVol ?? 100) / 100;
    this.songGain.connect(gain);
    this.master = gain; // song, clip audio and recording go through this
    if (this.S.record) this.startRecorder(gain);
    if (this.music && this.music.type === "follow") {
      // No sound of our own: the clock is Plex's position (estimated between its reports)
      const np = this.music.np;
      this.ext = { pos: np.pos + LEAD, perf: np.at || performance.now(), playing: true };
      this.usePlayer(np.player);
      this.extHeld = !np.playing;
      this.rebase();
      this.vOff = -this.pos(); // the show starts at 0 (intro), wherever the song is
      this.music.follow.attach(this);
      if (this.extHeld) this.sayPlex("Paused on Plex");
      this.paintSync();
      if (!this.extHeld) this.say("Cuts early or late? Tap T along to the beat – or use − / +");
      setTimeout(() => this.h("msg").textContent.startsWith("Cuts early") && this.say(""), 6000);
    } else {
      this.playSource(0);
      this.rebase();
      this.fsStart();
    }
    this.paintTrack();
    const first = this.tpl && this.tpl.events.find((ev) => ev.type === "layout");
    if (first) this.setLayout(first.layout, 0, first.dir);
    else if (this.reveal) {
      this.revealing = true;
      this.setLayout("full", 0);
    } else this.setLayout(this.layouts.includes("full") ? "full" : this.layouts[0], 0);
    this.loop = this.loop.bind(this);
    this.raf = requestAnimationFrame(this.loop);
  }

  // ---------- Funscript: built from the song, played on The Handy together with the show ----------

  // A funscript for the current song (null when it can't be made); also kept for "Save funscript" at the end
  fsBuild() {
    this.fs = null;
    if (!this.S.fsOn || !this.song || !this.song.beats || this.song.live) return null;
    try {
      this.fs = buildFunscript(this.song, this.bars, this.S);
    } catch (e) {
      console.warn("[PMV Generator] funscript", e);
    }
    return this.fs;
  }
  async fsStart() {
    if (!this.S.fsOn || this.tpl || this.live || (this.music && this.music.type === "follow")) return;
    const fs = this.fsBuild();
    if (!fs) return;
    this.fsClock = Object.assign(new EventTarget(), {
      show: this,
      get currentTime() {
        return Math.max(0, this.show.pos());
      },
      get paused() {
        return !!(this.show.paused || this.show.done);
      },
    });
    let told = "";
    this.fsHandy = attachHandy(this.fsClock, { paths: { funscript: "pmv" } }, {
      getVariant: async () => this.fs,
      onState: (h) => {
        if (!h) return;
        const st = h.state + (h.error || "");
        if (st === told) return;
        told = st;
        if (h.state === "ready") toast("The Handy is ready – the funscript plays with the song", "ok");
        else if (h.state === "error") toast("The Handy: " + h.error, "error");
      },
    });
    getHandy().then((h) => h || toast("No Handy connection key – enter it under Funscript", "error")).catch(() => {});
  }
  fsEvent(type) {
    if (this.fsClock) this.fsClock.dispatchEvent(new Event(type));
  }
  saveFunscript() {
    if (!this.fs) return;
    const blob = new Blob([JSON.stringify(this.fs)], { type: "application/json" });
    const a = Object.assign(document.createElement("a"), { href: URL.createObjectURL(blob), download: `${fileName(this.song.name)}.funscript` });
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 4000);
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

  // Two clocks: pos() = where the song is (beats), now() = show time for the pictures. They're the same
  // for a single song; with a playlist or Plex the song position jumps (next song, seeking) while
  // the show time keeps running smoothly.
  pos() {
    if (this.song.media && this.mediaEl) return this.mediaEl.currentTime - (this.song.offset || 0);
    if (this.ext) {
      const e = this.ext;
      return e.pos + (e.playing && !this.extHeld ? (performance.now() - e.perf) / 1000 : 0) + (this.S.plexSync || 0);
    }
    return this.ac ? this.ac.currentTime - this.startAt : 0;
  }
  now() {
    return this.pos() + (this.vOff || 0);
  }
  // The song position jumped: the show time goes on from t0 (the show time before the jump),
  // the beats are picked up from the new position
  rebase(t0) {
    const p = this.pos();
    this.vOff = (t0 != null ? t0 : p) - p;
    const k = this.song.beats.findIndex((b) => b > p);
    this.bi = k < 0 ? this.song.beats.length : k;
    this.lastCut = this.bi - 1;
    this.layoutBeat = this.bi;
    this.events = [];
  }

  // The song's sound from position `at` (seconds); at its end: the next song, or the end of the show
  playSource(at) {
    if (this.src) {
      this.src.onended = null;
      try {
        this.src.stop();
      } catch (e) { /* not started yet */ }
      this.src = null;
    }
    if (this.song.media) return this.playMedia(at);
    if (this.mediaEl) this.mediaEl.pause();
    const src = this.ac.createBufferSource();
    src.buffer = this.song.buffer;
    src.connect(this.songGain);
    this.src = src;
    const when = this.ac.currentTime + 0.1;
    src.start(when, at);
    this.startAt = when - at;
    src.onended = () => {
      if (this.src !== src || this.paused || this.done) return;
      if (this.music && this.music.type === "list") this.skip(1, true);
      else this.finish(false);
    };
  }

  // A long song: streamed from its file (from song.offset to song.end); the clock is the element's time
  playMedia(at) {
    if (!this.mediaEl) {
      this.mediaEl = new Audio();
      this.mediaEl.preload = "auto";
      this.ac.createMediaElementSource(this.mediaEl).connect(this.songGain);
    }
    const el = this.mediaEl;
    if (el.src !== this.song.media) el.src = this.song.media;
    el.currentTime = (this.song.offset || 0) + at;
    this.mediaOver = false;
    if (!this.paused) el.play().catch((e) => this.fail(e));
  }
  // Checked every frame: the long song (or its part) is over → next song / end
  checkMedia() {
    const el = this.mediaEl;
    if (!el || !this.song.media || this.mediaOver || this.paused || this.done) return;
    if (el.ended || (this.song.end && el.currentTime >= this.song.end)) {
      this.mediaOver = true;
      if (this.music && this.music.type === "list") this.skip(1, true);
      else this.finish(false);
    }
  }

  // Another song in the running show (call rebase() once its position is set)
  setSong(song) {
    this.song = song;
    this.setBars();
    if (this.fsHandy && this.fsBuild()) this.fsHandy.setVariant(this.fs); // playlist: the new song gets its own script
    this.lastDrop = -99;
    this.h("name").textContent = song.name;
    this.h("bpm").textContent = `${Math.round(song.bpm)} BPM`;
    this.log.push({ t: this.now(), type: "song", name: song.name });
  }
  paintTrack() {
    const el = this.h("track");
    if (el && this.music && this.music.type === "list") el.textContent = `${this.music.list.pos + 1}/${this.music.list.size}`;
  }

  // Playlist: next/previous song (auto = the last one ended)
  async skip(dir, auto) {
    if (!this.music || this.music.type !== "list" || this.skipping || this.done || !this.ac) return;
    this.skipping = true;
    const list = this.music.list;
    try {
      for (let tries = 0; tries < Math.min(5, list.size); tries++) {
        this.say("Loading the next song …");
        let next = null;
        try {
          next = await list.step(dir);
        } catch (e) {
          toast(`Can't play “${list.current().name}” – skipped`, "error");
          continue;
        }
        if (this.done) return;
        if (!next) return this.finish(false);
        const t0 = this.now();
        this.setSong(next);
        this.playSource(0);
        this.rebase(t0);
        this.say(this.paused ? "Paused – Space continues" : "");
        this.paintTrack();
        if (!this.paused) this.comp.text(this.now());
        return;
      }
      this.say("These songs can't be played");
    } finally {
      this.skipping = false;
    }
  }

  // Plex: news from the server (see plex.js PlexFollow). Positions come a bit late – +0.25 s.
  follow(u) {
    if (this.done || !this.ext) return;
    const t0 = this.now();
    if (u.error) return this.sayPlex("Plex: " + u.error);
    if (u.loading) return this.sayPlex("Next song: " + u.loading + " …");
    if (u.idle) {
      this.setHeld(true);
      return this.sayPlex("Nothing is playing on Plex");
    }
    if (u.player) this.usePlayer(u.player);
    const at = (p) => ({ pos: p + LEAD, perf: performance.now(), playing: true });
    if (u.song) {
      this.setSong(u.song);
      this.ext = at(u.pos);
      this.setHeld(!u.playing);
      this.rebase(t0);
      this.sayPlex(u.playing ? "" : "Paused on Plex");
      return;
    }
    if (u.playing === !!this.extHeld) {
      // Plex paused or goes on
      if (u.playing && u.fresh) this.ext = at(u.pos);
      this.setHeld(!u.playing);
      this.rebase(t0);
      this.sayPlex(u.playing ? "" : "Paused on Plex");
      return;
    }
    // A fresh report is a lower bound: the song is at least there now (the report is already a bit
    // old when it arrives). Behind it → catch up; far ahead of it → seeking back (or the player stalled).
    // Otherwise the estimate stays – so it keeps the best of all reports instead of jumping around.
    const est = this.pos() - (this.S.plexSync || 0);
    const lb = u.pos + LEAD;
    if (u.fresh && u.playing && (lb > est + 0.02 || lb < est - 1.2)) {
      this.ext = at(u.pos);
      this.rebase(t0);
    }
    if (u.playing) this.sayPlex("");
  }
  // Plex's own messages (paused, nothing playing …) – clearing them leaves other messages alone
  sayPlex(t) {
    if (!t && !this.plexSaid) return;
    this.plexSaid = !!t;
    this.say(t);
  }
  // Plex paused: freeze the clock and the clips
  setHeld(held) {
    if (!!held === !!this.extHeld) return;
    const p = this.pos() - (this.S.plexSync || 0);
    this.extHeld = !!held;
    this.ext = { pos: held ? p : this.ext.pos, perf: performance.now(), playing: true };
    this.videos().forEach((m) => (held ? m.el.pause() : m.el.play().catch(() => {})));
  }
  nudge(d) {
    this.setSync((this.S.plexSync || 0) + d);
  }
  setSync(v) {
    const t0 = this.now();
    this.S.plexSync = Math.max(-5, Math.min(5, Math.round(v * 100) / 100));
    const saved = store.get("pmvgen", {});
    saved.plexSync = this.S.plexSync;
    if (this.syncPlayer) saved.plexSyncs = Object.assign({}, saved.plexSyncs, { [this.syncPlayer]: this.S.plexSync });
    store.set("pmvgen", saved);
    this.rebase(t0);
    this.paintSync();
  }
  // Each Plex player keeps its own offset (Bluetooth speakers are late, a PC isn't)
  usePlayer(id) {
    if (!id || id === this.syncPlayer) return;
    this.syncPlayer = id;
    const own = (store.get("pmvgen", {}).plexSyncs || {})[id];
    if (own == null) return;
    const t0 = this.now();
    this.S.plexSync = own;
    if (this.ext) this.rebase(t0);
    this.paintSync();
  }
  // Tap sync: tap along to the beat you hear; after 4 taps the offset is set (tap on to refine)
  tap() {
    const beats = this.song.beats;
    if (this.extHeld || beats.length < 2) return;
    const ms = performance.now();
    if (!this.taps || ms - this.tapMs > 2500) this.taps = [];
    this.tapMs = ms;
    const x = this.pos();
    const k = Math.max(1, Math.min(beats.length - 1, beats.findIndex((b) => b > x)));
    const b = Math.abs(beats[k] - x) < Math.abs(x - beats[k - 1]) ? beats[k] : beats[k - 1];
    this.taps.push(x - b); // > 0: the cuts run ahead of what you hear
    this.stage.classList.remove("is-tap");
    void this.stage.offsetWidth;
    this.stage.classList.add("is-tap");
    if (this.taps.length < 4) return this.say(`Tap along to the beat … ${this.taps.length}/4`);
    const sorted = this.taps.slice().sort((a, c) => a - c);
    const med = (sorted[1] + sorted[2]) / 2; // middle two of four: a stray tap doesn't count
    this.taps = [];
    this.setSync((this.S.plexSync || 0) - med);
    this.say(`Sync set: ${this.h("sync").textContent} – tap on to refine`);
    clearTimeout(this.tapSayT);
    this.tapSayT = setTimeout(() => this.h("msg").textContent.startsWith("Sync set") && this.say(""), 2500);
  }
  paintSync() {
    const el = this.h("sync");
    if (el) el.textContent = `${this.S.plexSync > 0 ? "+" : ""}${Math.round((this.S.plexSync || 0) * 1000)} ms`;
  }

  loop() {
    if (this.done) return;
    this.checkMedia();
    const p = this.pos();
    const t = p + (this.vOff || 0);
    const beats = this.song.beats;
    while (this.bi < beats.length && beats[this.bi] <= p) this.onBeat(this.bi++, t);
    if (this.tpl) {
      const evs = this.tpl.events;
      while (this.ti < evs.length && evs[this.ti].t <= t) this.applyEvent(evs[this.ti++], t);
    }
    while (this.events.length && this.events[0].t <= p) {
      const ev = this.events.shift();
      if (ev.type === "stutter") this.stutter(t);
      if (ev.type === "strobe") this.comp.strobe(t);
    }
    // Draw when a clip on screen has a new picture (every clip frame shows up, evenly – also on 144/240 Hz
    // screens, where a fixed ~60 per second skipped and bunched clip frames), at most every 4 ms;
    // without a new picture ~60 times a second for the effects.
    // (beats and cuts above are checked on every frame, so the timing stays exact)
    const ms = performance.now();
    const since = ms - (this.drawMs || -1e9);
    if (since >= 15 || (this.freshFrame && since >= 4)) {
      this.drawMs = ms;
      this.freshFrame = false;
      this.trackFocus();
      this.st.t = t;
      this.st.slots = this.slots;
      this.st.groups = this.groups;
      this.st.cutT = this.cutT;
      this.st.cutCount = this.cuts;
      this.st.reveal = this.revealing ? { p: Math.max(0, Math.min(1, t / this.reveal.endT)) } : null;
      this.st.leave = this.leave;
      this.comp.draw(this.st);
    }
    if (!this.hudT || performance.now() - this.hudT > 250) {
      this.hudT = performance.now();
      this.h("time").textContent = `${fmtDuration(Math.max(0, p))} / ${fmtDuration(this.song.duration)}`;
      this.h("cuts").textContent = `${this.cuts} cuts · ${LAYOUTS[this.layout] ? LAYOUTS[this.layout].name : ""}`;
      this.h("bar").style.width = `${Math.max(0, Math.min(100, (p / this.song.duration) * 100))}%`;
    }
    this.raf = requestAnimationFrame(this.loop);
  }

  // Direction: decide what happens on each beat
  // Bars and phrases of the song (bars.js) – none for templates or very long files: then every 4th beat counts
  setBars() {
    this.bars = this.tpl ? null : songBars(this.S, this.song);
  }

  // How hard beat k pumps the picture: louder = more; "every 2nd" / "each bar" leave out the others
  pulseFor(k, e, bar, drop) {
    const on = this.S.pulseOn;
    if (!drop && ((on === "2" && k % 2) || (on === "bar" && !bar))) return 0;
    return 0.03 + 0.07 * e + (drop ? 0.1 : 0);
  }

  // Direction: decide what happens on each beat
  onBeat(k, t) {
    const S = this.S;
    const beats = this.song.beats;
    const e = this.song.energy[k] || 0;
    const prev = this.song.energy[k - 4] || 0;
    // A drop counts once – otherwise the signal stays up for several beats in a row
    let drop = e - prev > 0.35 && e > 0.6 && k - this.lastDrop >= 16;
    // Reveal opening: ends at its beat – that counts as the drop (the picture opens up)
    let revealEnds = false;
    if (this.revealing && k >= this.reveal.endK) {
      revealEnds = true;
      this.revealing = false;
      this.reveal.done = true;
      drop = true;
    }
    if (drop) this.lastDrop = k;
    const B = this.bars;
    const bar = B ? !!B.downbeat[k] : k % 4 === 0;
    const len = (beats[k + 1] || beats[k] + 0.5) - beats[k];
    const comp = this.comp;
    this.st.energy = e;
    // The zoom pulse sits exactly on the beat (its time from the song, not from when it was noticed); the next beat is
    // known too, so the pulse can ease in just before it
    this.st.beatT = beats[k] != null ? beats[k] + (this.vOff || 0) : t;
    this.st.beatAmt = this.pulseFor(k, e, bar, drop);
    this.st.nextT = beats[k + 1] != null ? beats[k + 1] + (this.vOff || 0) : null;
    this.st.nextAmt = beats[k + 1] != null ? this.pulseFor(k + 1, this.song.energy[k + 1] || 0, B ? !!B.downbeat[k + 1] : (k + 1) % 4 === 0, false) : 0;

    // Change layout: on drops right away; otherwise every 2 bars if the mood has changed
    // or – in loud parts – for variety. Calm parts stay calm.
    const want = this.levelFor(e, drop);
    const moodChanged = LAYOUTS[this.layout].level !== want && this.layouts.some((x) => LAYOUTS[x].level === want || want > 1);
    // With bars: a layout change waits for the start of a phrase (or, when the mood has changed, at most a bar later than 6 bars)
    const due = B
      ? ((B.phrase[k] && k - this.layoutBeat >= this.layoutHold && (moodChanged || (want >= 3 && Math.random() < 0.6))) || (bar && moodChanged && k - this.layoutBeat >= 24))
      : bar && k - this.layoutBeat >= this.layoutHold && (moodChanged || (want >= 3 && Math.random() < 0.6));
    let nextLayout = null;
    if (this.tpl) {
      // Cuts and layouts come from the template (applyEvent)
    } else if (this.layouts.length > 1 && (drop || due) && !this.revealing && (nextLayout = this.pickLayout(e, drop))) {
      this.setLayout(nextLayout, t);
      this.layoutBeat = k;
      this.centerK = k;
      // Scrolling sides: in a 3-way layout (not on a drop, not in the loudest parts) the middle clip stays and the sides scroll
      this.scrollDir = S.scroll && !drop && e <= 0.9 && (this.layout === "tri" || this.layout === "trim") && Math.random() < 0.75 ? (Math.random() < 0.5 ? -1 : 1) : 0;
      this.sideN = 0;
      this.layoutHold = drop ? 4 : 8; // after a drop, move on after just one bar
      this.lastCut = k;
      comp.flash(t, drop ? 0.9 : 0.35, drop ? "#ff3e8a" : "#fff");
    } else {
      // Within the layout: re-cut the fields in turn
      if (revealEnds && this.layouts.length < 2) comp.flash(t, 0.9, "#ff3e8a"); // (a single layout: the window just opens up)
      const PACE = { slow: [8, 4, 2], normal: [4, 2, 1], fast: [2, 1, 1] }[S.pace] || [4, 2, 1]; // calm · medium · loud
      let every = S.cut === "auto" ? (e > 0.72 ? PACE[2] : e > 0.4 ? PACE[1] : PACE[0]) : Number(S.cut);
      if (this.scrollDir) every = Math.max(2, every); // scrolling is slower than cutting
      // With bars the cuts sit on the bar's grid: every beat, beats 1 and 3, or only the "one" – not "N beats after the last cut"
      const onGrid = B && 4 % every === 0 ? B.pos[k] % every === 0 && k - this.lastCut >= Math.min(every, 2) : k - this.lastCut >= every;
      if (this.revealing) {
        // Reveal opening: one clip only, it just grows – no cuts until the drop
      } else if (this.scrollDir && (this.layout === "tri" || this.layout === "trim")) {
        // The middle clip (group 1) runs for four bars; the sides (groups 0 and 2) scroll in turn
        if (bar && k - (this.centerK ?? this.layoutBeat) >= 16) {
          if (this.cutGroup(1, t)) {
            this.centerK = k;
            this.lastCut = k;
          }
        } else if (onGrid) {
          const sides = this.layout === "tri" ? [0, 2] : [0];
          if (this.cutGroup(sides[this.sideN++ % sides.length], t, { scroll: this.scrollDir })) this.lastCut = k;
        }
      } else if (onGrid || (every === 4 && bar && k - this.lastCut >= 2)) {
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
    const now = performance.now();
    if (!this.infoT || now - this.infoT > 500) {
      this.infoT = now;
      this.paintInfo();
    }
    if (!this.focusT || now - this.focusT > 400) {
      this.focusT = now;
      this.videos().forEach((m) => {
        if (m.el.error) {
          this.noteFail(m, { why: mediaError(m.el.error) });
          this.groups.forEach((g, gi) => g === m && this.cutGroup(gi, this.now()));
          return;
        }
        if (m.el.readyState < 2 || m.measuring) return;
        // also without smart crop: match cuts and the clip info (black picture) use it
        m.measuring = true;
        analyzeAsync(m.el, m.w, m.h)
          .then((a) => {
            if (!a.gray) return;
            m.live = a;
            if (S.smartCrop) m.focusTarget = a.focus;
            if (a.lum != null && m.sig && m.sig.lum != null) m.sig.lum += (a.lum - m.sig.lum) * 0.3;
          })
          .finally(() => (m.measuring = false));
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
      this.fsEvent("pause");
      if (this.mediaEl && this.song.media) this.mediaEl.pause();
      this.videos().forEach((m) => m.el.pause());
      if (this.rec && this.rec.state === "recording") this.rec.pause();
      this.say("Paused – Space continues");
    } else {
      this.ac.resume();
      this.fsEvent("playing");
      if (this.mediaEl && this.song.media) this.mediaEl.play().catch(() => {});
      this.videos().forEach((m) => m.el.play().catch(() => {}));
      if (this.rec && this.rec.state === "paused") this.rec.resume();
      this.say("");
    }
  }

  stopEverything() {
    this.done = true;
    if (this.fsHandy) {
      this.fsHandy.stop();
      this.fsHandy = null;
    }
    if (this.music && this.music.follow) this.music.follow.detach();
    cancelAnimationFrame(this.raf);
    try {
      if (this.src) this.src.onended = null;
      if (this.src) this.src.stop();
    } catch (e) { /* already stopped */ }
    if (this.mediaEl) {
      this.mediaEl.pause();
      this.mediaEl.removeAttribute("src");
      this.mediaEl.load();
    }
    new Set(this.groups).forEach((m) => this.release(m));
    this.groups = [];
    this.ready.forEach((m) => this.release(m));
    this.ready = [];
  }
  async finish(early) {
    if (this.done) return;
    const t = this.now();
    this.stopEverything();
    this.length = this.music ? t : Math.min(t, this.song.duration);
    let blob = null;
    if (this.rec && this.rec.state !== "inactive") {
      this.say("Finishing the video …");
      await new Promise((r) => {
        this.rec.onstop = r;
        this.rec.stop();
      });
      blob = await fixWebmDuration(new Blob(this.chunks, { type: this.mime.split(";")[0] }), this.length * 1000);
      this.say("");
      // Browsers stop drawing hidden tabs – then nothing was recorded
      if (!blob.size) {
        blob = null;
        this.emptyRec = true;
      }
    }
    if (this.ac) this.ac.close().catch(() => {});
    this.showEnd(blob, null, early);
  }

  // The clips that were skipped (name, file, why) – so you can see which ones don't work
  failListHtml(list) {
    return `<ul class="kb-pmvg-faillist">${list.map((f) => `<li><b>${esc(f.name)}</b>${f.file && f.file !== f.name ? ` <small>${esc(f.file)}</small>` : ""} – ${esc(f.why)}${f.n > 1 ? ` <small>×${f.n}</small>` : ""}</li>`).join("")}</ul>`;
  }

  showEnd(blob, error, early, details, slow) {
    const end = this.h("end");
    this.blob = blob;
    const url = blob ? URL.createObjectURL(blob) : null;
    this.blobUrl = url;
    end.hidden = false;
    end.innerHTML = error
      ? `<div class="kb-pmvg-endcard"><h2>That didn't work</h2><p>${esc(error)}</p>
          ${details && details.length ? `<p class="kb-hint">These clips were tried and skipped:</p>${this.failListHtml(details)}` : ""}
          ${slow ? `<p class="kb-hint">Nothing was ready within 20 seconds – the clips load too slowly. Try a lower resolution (“Resolution up to”) or fewer clips from a slow drive.</p>` : ""}
          ${details && details.length ? `<p class="kb-hint">Often it is the video's codec (HEVC / H.265, AV1 …): the browser can't decode it, so the clip stays black or is skipped. Re-encoding such files to H.264 helps – and generating the previews and sprites in Stash (Tasks → Generate) speeds clips up.</p>` : `<p class="kb-hint">Choose other filters – the browser may not play videos in exotic formats directly.</p>`}
          <div class="kb-card-acts"><button class="kb-btn" data-end="close">Back</button></div></div>`
      : `<div class="kb-pmvg-endcard">
          <h2>${early ? "Stopped" : "Done!"}</h2>
          <p>${fmtDuration(this.length || 0)} · ${this.cuts} cuts · ${Math.round(this.song.bpm)} BPM${blob ? ` · ${fmtBytes(blob.size)}` : ""}</p>
          ${early && blob && this.song.duration > 0 && this.length < this.song.duration - 2 ? `<p class="kb-hint">Stopped early – the video ends at ${fmtDuration(this.length || 0)} of ${fmtDuration(this.song.duration)}. The recording runs in real time, so let the show play to the end for the whole track.</p>` : ""}
          ${this.failed && this.failed.length ? `<details class="kb-pmvg-skipped"><summary>${this.failed.length} ${this.failed.length === 1 ? "clip was" : "clips were"} skipped</summary>${this.failListHtml(this.failed.slice().reverse())}</details>` : ""}
          ${url ? `<video class="kb-pmvg-result" src="${url}" controls playsinline></video>` : ""}
          <div class="kb-card-acts">
            ${blob ? `<a class="kb-btn is-primary" data-end="file" href="${url}" download="${esc(fileName(this.song.name))}.webm">${icon("download")}Download</a>` : ""}
            ${this.fs ? `<button class="kb-btn" data-end="fs" title="The funscript that played with this song">${icon("download")}Save funscript</button>` : ""}
            <button class="kb-btn" data-end="again">${icon("shuffle")}Again, reshuffled</button>
            <button class="kb-btn is-ghost" data-end="close">Close</button>
          </div>
          ${blob ? `<p class="kb-hint">WebM video – plays in the browser, VLC and most players.</p>` : ""}
          ${this.emptyRec ? `<p class="kb-hint">Nothing was recorded – keep this tab visible while the show runs, browsers pause hidden tabs.</p>` : ""}
        </div>`;
    end.onclick = (e) => {
      const b = e.target.closest("[data-end]");
      if (!b) return;
      const a = b.dataset.end;
      if (a === "close") this.close();
      if (a === "again") {
        const { song, S, onClose, tpl, music } = this;
        this.close();
        onClose(new Generator(song, S, onClose, tpl, music));
      }
      if (a === "fs") this.saveFunscript();
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
const mediaError = (err) =>
  ({ 1: "loading was aborted", 2: "network error while loading", 3: "can't be decoded (broken file or codec)", 4: "format not supported by the browser" })[err && err.code] || "error";

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
