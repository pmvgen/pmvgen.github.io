# PMV Generator – native (Rust)

A native desktop companion to [pmvgen.github.io](https://github.com/pmvgen/pmvgen.github.io) (a rewrite of web
version 2.38.10) – local only, for people who want to work on their own computer.
Pick a song and a folder of clips – on every beat it cuts to the next one: split-screen layouts, effects on cuts,
beats and drops, speed ramps, clip audio, a live fullscreen show – and, beyond the web version, a studio to preview
and edit a generated PMV, project files that render the same video again, and fast hardware-encoded exports.

Everything runs locally. Nothing is uploaded.

## Run it

**Linux:** `sudo apt install ffmpeg`, then run `pmvgen`.
**Windows:** put `ffmpeg.exe` and `ffprobe.exe` next to `pmvgen.exe` (or in an `ffmpeg` folder beside it), or
install ffmpeg (`winget install Gyan.FFmpeg`).

Open files directly: `pmvgen song.mp3 ~/Videos/clips` – or a project: `pmvgen my.pmvproj`. You can also drop
songs, clip folders and projects onto the window.

## The window

It is laid out like the web version: **1 Music · 2 Clips · 3 Style · 4 Go**, with the same options, labels and
hints, the same colours and the *Liquid glass* look (switch at the top right). Beside it, a **theme**: the
original, your **system's** (GTK theme on Linux, accent and light / dark on Windows 11), Fluent, Adwaita,
Catppuccin (4), Dracula, Nord, Sweet, Tokyo Night, Gruvbox, Rosé Pine, One Dark, Solarized, Everforest, Kanagawa,
Monokai, GitHub and more. The window scales itself to its size and to the screen; **Ctrl +** / **Ctrl −** zoom
further, **Ctrl 0** resets.

1. **Music** – *Your song*: songs (MP3, M4A, WAV, OGG, FLAC, … or a video's music), tempo and beats found at once,
   **½ / 2× tempo**, **Earlier / Later** (20 ms), **Only use … Cut**. Several songs play one after another.
   *PMV as template*: an existing PMV is analysed – music, every cut, the layouts and the flashes are taken over;
   only the content comes from your clips. Two ways: **Follow the beat** (the web version's: cuts on the music's
   beats, the PMV's music) and **Exact cuts** – every frame of the PMV is looked at and every cut is placed exactly
   where its maker put it, whatever the beat; the music is the PMV's own or **the same song as a file** (where it
   plays in the PMV is found by itself, to a fraction of a frame), and **where the music starts** can be set as a
   time or a frame.
2. **Clips** – folders (subfolders included) or single files; videos, images or both; portrait / landscape; which
   folders. Clip selection: best moments, **clip speed follows the music**, **motion on the beat**, **clean cuts**
   (every frame around a clip is checked for a cut to another scene – see *Shot detection* below),
   smart crop, skip the start / end of each video – and **per video** its own skip of the start / end. Clip order:
   match cuts, follow the scenes' timeline, variety.
3. **Style** – moods and the sections Cutting, Effects, Pictures, Color & look, Picture & frame (**any format from
   1:1 to 32:9, or portrait 4:5 to 9:16**), Sound (**clip audio** with its own volume – only on drops or always),
   Funscript and Output (title cards with **your own font**, export quality **144p to 4K**, frame rate, **H.264 /
   H.265 / AV1 / Web (VP9)**, the encoder – **NVIDIA, Intel, AMD, VA-API, Media Foundation or the processor** – and
   the preset **Fastest / Balanced / Smallest file**).
4. **Go** – **Start the show** (live, fullscreen; clips are chosen as it plays) or **Generate, preview & edit**.

### The live show

Space pause · F fullscreen · H hide the bar · I clip info · **M change the moment** · N / P next / previous song ·
Esc stop. *Change the moment* pauses and gives every clip on screen a slider to take it from another moment of its
video; **Pin this moment** makes clips of that video often start there (kept between runs).

### The studio (Generate, preview & edit)

Every clip of the PMV is chosen first; then you see it before anything is rendered:

- **Preview** with sound (song + clip audio), play / pause, jump anywhere.
- **Timeline**: the layouts, every shot in its field with a thumbnail, drops. Click to select (Ctrl+click adds,
  Shift+click a range, drag a box, Ctrl+A all), Ctrl+wheel zooms, the wheel scrolls.
- **Swap** the selected shots' clips: **Auto** (like the generator: best moments, match cuts, the music's pace),
  **Random**, or **by hand** (a clip and its moment) – taken from the library, one folder, or the clips the PMV
  already uses. **Change the moment** of the selected clips. **Undo / redo** (Ctrl+Z / Ctrl+Y).
- **Save project**: a `.pmvproj` file with every cut, effect and clip. Opening it again renders **exactly the same
  video** – the same frames and the same sound – as long as the song and the clips are the same files.
- **Export video**, with progress; nothing is left behind when it is cancelled or fails.

The look (colour, edges, seams, fit, film effects), sound, title cards, font and output settings can still be changed
after generating; the cuts, layouts and triggered effects stay as generated.

## Shot detection

*Clean cuts* and *Exact cuts* need to tell a cut to another scene from motion, camera moves, flashes and effects.
Every frame is looked at by two detectors that cover each other's blind spots:

- **Classical measures** on a small picture: how much of it can't be found again nearby after compensating for
  motion and brightness, colour change, edge change – and a light-independent **structure comparison** that tells
  a lighting or colour effect from a new picture. A picture that comes back within 0.2 s was a flash or a glitch.
- **TransNetV2** (Souček & Lokoč, MIT licence), a neural network made for shot boundaries, run in pure Rust
  ([tract](https://github.com/sonos/tract)) from `models/transnetv2.onnx`
  ([ONNX export](https://huggingface.co/elya5/transnetv2) of the [original weights](https://github.com/soCzech/TransNetV2)).

`pmvgen --shots VIDEO` lists what it finds.

## Command line

```
pmvgen --export --song song.mp3 --clips DIR [--clips DIR …] --out FILE [--template PMV] [--settings my.pmvgen.json]
       [--from SEC] [--to SEC] [--seed N] [--save-project p.pmvproj] [OUTPUT OPTIONS]
pmvgen --generate --song song.mp3 --clips DIR --save-project p.pmvproj
pmvgen --render p.pmvproj --out FILE [--part 0:30-1:10] [OUTPUT OPTIONS]
pmvgen --export --template PMV --exact [--song song.flac] [--music-start 0:05.300] --clips DIR --out FILE
                                   # exact cuts: every cut where the PMV has it; the song file is lined up by itself
pmvgen --analyze song.mp3          # tempo and beats as JSON
pmvgen --encoders                  # which encoders work on this computer
pmvgen --shots VIDEO               # where a video cuts to another scene
pmvgen --align PMV --clips SONG    # where a song file plays in a PMV's soundtrack

OUTPUT OPTIONS: --codec h264|h265|av1|web  --encoder auto|cpu|nvenc|qsv|amf|vaapi|mf  --preset fast|balanced|small
                --quality 144…2160  --fps 24|25|30|50|60  --crf 12–40  --format 16:9|9:16|1:1|21:9|32:9|…  --font FILE
```

Every graphics card encoder is test-run once before it is used (ffmpeg lists encoders even when their hardware is
missing); if it still refuses a size or format when the export starts, the processor takes over by itself.

## What is the same as the web version

The parts that decide what happens were ported line by line and **checked against the original JavaScript** (the
original modules run in Node; `tests/diff`): tempo, every beat, energy, bars and phrases, ½× / 2× tempo, funscripts,
layouts and the picture analysis. The direction (when to cut, which layout, effects, speed ramps), clip selection,
the template analysis (pmvscan), clip speed / motion on the beat and the drawing (pmvfx) follow the original's logic.

## What is different

- **Recording → Export / studio.** The web version records the live show in real time (WebM). Here a PMV is rendered
  frame by frame to a video file – every frame is there, however slow the computer – and can be previewed and edited
  first.
- The template's beat snapping does what the web version's comment says (its code never moves an event).
- Clip audio keeps its pitch through speed ramps (time-stretched), like the browser does.
- **Not in this version:** Plex, mouse control, live control of The Handy (a `.funscript` is saved with the video),
  the Chinese translation, *New clips for every song* (every song gets new clips here).

## Building

```
cargo build --release                 # Linux: needs libgtk-3-dev, libasound2-dev
cargo test --release                  # unit tests + an end-to-end test with generated media (needs ffmpeg)
cargo clippy --release --all-targets -- -D warnings
python3 tools_weaken_glibc.py target/release/pmvgen   # optional: lets the binary start on glibc < 2.39
```

Windows (cross-compiled from Linux): `sudo apt install mingw-w64`, `rustup target add x86_64-pc-windows-gnu`,
`cargo build --release --target x86_64-pc-windows-gnu`.

Settings live in `~/.config/pmvgen/settings.json` (Windows: `%APPDATA%\pmvgen\settings.json`).

## Licenses

MIT (like the original – see `assets/ORIGINAL-LICENSE.txt`). Font: Anton (SIL Open Font License,
`assets/Anton-OFL.txt`). Shot detection model: TransNetV2 (MIT).
