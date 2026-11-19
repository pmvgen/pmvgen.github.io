# Differential test against the original JavaScript

The approach of rust-anything: record what the original does, replay the same inputs through the port, compare.

```
mkdir work && cd work
npm install node-web-audio-api                     # the browser's Web Audio for Node
mkdir orig songs
cp <pmvgen.github.io>/js/{beats,bars,funscriptgen,pmvfx,pmvsmart}.js orig/
cp orig/pmvsmart.js orig/pmvsmart_x.js && echo 'export { measure };' >> orig/pmvsmart_x.js
cp orig/beats.js orig/beats_x.js && echo 'export { readWav, FrameSink };' >> orig/beats_x.js
node ../record.mjs && node ../record_long.mjs       # → fixtures.json, fixtures_long.json, songs/*.wav
cd <pmvgen-rs> && PMVGEN_FIXTURES=<work> cargo test --release diff_ -- --nocapture
```

The original modules run unmodified (the two `_x` copies only add an `export` line for functions the
original keeps private). Recorded: 6 generated songs (90–174 BPM, tempo drift, quiet intro, pickup beat),
a 46-minute DJ mix (the long-file path), ½× / 2× tempo, shifting, 4 funscript variants per song, all
layouts at 3 sizes × 2 directions, and the picture analysis on 40 images. Everything matches: tempo to
1e-6, every beat time to 1e-9 s, energy / pace to 1e-6, bars and phrases and funscripts exactly.
