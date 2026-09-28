# PMV Generator – web version

Pick a song and a folder of clips – on every beat it cuts to the next one: split-screen layouts like real PMVs, effects on cuts, beats and drops, beat-synced speed ramps and clip audio. It runs live in the browser and can record the result as a video. It can also analyze an existing PMV and rebuild it with your own clips.

**Open it:** https://pmvgen.github.io

This is the standalone web version of the PMV Generator from [stash-pmv-plugins](https://github.com/anonym88312/stash-pmv-plugins) – it doesn't need Stash or any server.

## Privacy

Everything happens in your browser. Songs and clips are opened straight from your disk and are **never uploaded** – the page is plain static HTML and JavaScript without any backend, tracking or analytics. Your settings are remembered in your browser's local storage.

## Using it

1. **Music**: drop or choose a song (MP3, M4A, WAV, OGG, FLAC). Tempo and beats are detected in the browser. If the tempo is off, **½ tempo** / **2× tempo** help, and **Earlier** / **Later** shift the cuts by 20 ms. Or use **PMV as template** (see below).
2. **Clips**: drop one or more folders onto the card, or click it to choose a folder (subfolders are included). Single files work too. Supported: MP4, WebM, MOV, MKV (whatever your browser can play) and JPG, PNG, WebP, GIF, AVIF.
   - **What**: videos, images or both · **Clip shape**: all, portrait only, landscape only.
   - **Folders**: limit the clips to some of the added folders.
   - **Clip selection**: best moments instead of random spots, smart crop, match cuts, variety.
3. **Style**: mood presets (*PMV classic*, *Maximal*, *Hypno*, *Clean*) and five tabs – cutting and layouts (fullscreen, kaleidoscope, 2-way, 3-way mirrored, 3-way, 4-way), effects on cuts / beats / drops, color look and format (16:9 or 9:16), song and clip volume, intro/outro and recording quality (720p/1080p).
4. **Go**: runs as a fullscreen show (Space pause, F fullscreen, Esc stop). With **Record** you get a WebM video to download – the duration is written into the file right in the browser, so players show the length and can seek.

The clips you add are kept only while the page is open – after a reload, add the folder again (your folder choice and all settings are remembered).

Tip: three full-size portrait clips side by side = format **16:9** + layout 3-way + “Portrait only”.

## PMV as template

Switch step 1 to **PMV as template**, then pick one of your added videos (names containing “PMV” come first) or choose a video file. The analysis runs in the browser at about three times real-time speed and takes over the music, every cut, the layouts (split screens) and flashes. **Rebuild with my clips** plays the original music with the same cuts, layouts and flashes – only with your clips.

## Requirements

A current Chrome, Edge or Firefox on a desktop computer. Choosing whole folders isn't supported by every mobile browser – pick single files there.

## Run it yourself

It's a static site – any web server works, for example:

```
python -m http.server 8000
```

then open http://localhost:8000. (Opening `index.html` directly from disk doesn't work, because browsers block JavaScript modules on `file://`.)

## License

MIT – see [LICENSE](LICENSE).
