// PMV Generator – native rewrite of pmvgen.github.io (2.38.10) in Rust.
// Pick a song and a folder of clips – on every beat it cuts to the next one.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod align;
mod app;
mod bars;
mod beats;
mod clipaudio;
mod compositor;
#[cfg(test)]
mod difftest;
#[cfg(test)]
mod e2e;
mod enc;
mod export;
mod ff;
mod funscript;
mod library;
mod media;
mod motion;
mod plan;
mod player;
mod project;
mod raster;
mod render;
mod settings;
mod shots;
mod show;
mod smart;
mod song;
mod template;
mod text;
mod util;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use util::LockExt;

const HELP: &str = "PMV Generator 2.38.10 (native)

  pmvgen [FILES / FOLDERS / PROJECT]   open the app (songs, clip folders and a .pmvproj given are opened)

  pmvgen --export --song FILE --clips DIR [--clips DIR …] --out FILE
         [--template PMV [--exact [--music-start TIME]]] [--settings FILE.pmvgen.json] [--from SEC] [--to SEC] [--seed N]
         [--save-project FILE.pmvproj] [OUTPUT OPTIONS]
                                       generate a PMV and render it (no window)
  pmvgen --generate --song FILE --clips DIR [--clips DIR …] --save-project FILE.pmvproj [...]
                                       only generate (choose the clips) and save the project
  pmvgen --render PROJECT.pmvproj --out FILE [--part FROM-TO] [OUTPUT OPTIONS]
                                       render a saved project again – the same video
  pmvgen --analyze FILE                print tempo and beats of a song (JSON)
  pmvgen --encoders                    list the video encoders that work on this computer
  pmvgen --shots VIDEO [--from SEC] [--to SEC]
                                       list where the video cuts to another scene
  pmvgen --align PMV --clips SONG      where the song file plays in the PMV's soundtrack
  (--template: the PMV's cuts, layouts and flashes on the music's beats, with its music. With --exact every cut
   exactly where the PMV has it, frame by frame; --song is then the same song as a file, lined up with the
   PMV's soundtrack, and --music-start where in the PMV the music – and the new PMV – begins, e.g. 0:05.300)

OUTPUT OPTIONS
  --codec h264|h265|av1|web   --encoder auto|cpu|nvenc|qsv|amf|vaapi|mf   --preset fast|balanced|small
  --quality 144|240|360|480|720|1080|1440|2160   --fps 24|25|30|50|60   --crf 12-40
  --format 16:9|9:16|1:1|21:9|32:9|…  (not for --render)   --font FILE.ttf
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // no options: the app (paths given are opened in it – songs, clip folders / files, a project)
    if args.iter().all(|a| !a.starts_with("--") && a != "-h") {
        let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
        if let Err(e) = app::run(paths) {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return;
    }
    let code = match cli(&args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

#[derive(Default)]
struct Cli {
    mode: String,
    song: Option<PathBuf>,
    clips: Vec<PathBuf>,
    out: Option<PathBuf>,
    settings: Option<PathBuf>,
    template: Option<PathBuf>,
    project: Option<PathBuf>,
    save_project: Option<PathBuf>,
    from: f64,
    to: Option<f64>,
    music_start: Option<f64>,
    exact: bool,
    seed: Option<u64>,
    part: Option<(f64, f64)>,
    overrides: Vec<(String, String)>,
}

fn parse_args(args: &[String]) -> anyhow::Result<Cli> {
    let mut c = Cli::default();
    let mut i = 0;
    let val = |i: usize| {
        args.get(i + 1)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{} needs a value", args[i]))
    };
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--help" | "-h" => {
                c.mode = "help".into();
                return Ok(c);
            }
            "--export" | "--generate" | "--encoders" => c.mode = a[2..].to_string(),
            "--analyze" | "--render" | "--shots" | "--shots-dump" | "--align" => {
                c.mode = a[2..].to_string();
                let v = PathBuf::from(val(i)?);
                if a != "--render" {
                    c.song = Some(v);
                } else {
                    c.project = Some(v);
                }
                i += 1;
            }
            "--song" | "--out" | "--settings" | "--template" | "--save-project" | "--clips" => {
                let v = PathBuf::from(val(i)?);
                match a {
                    "--song" => c.song = Some(v),
                    "--out" => c.out = Some(v),
                    "--settings" => c.settings = Some(v),
                    "--template" => c.template = Some(v),
                    "--save-project" => c.save_project = Some(v),
                    _ => c.clips.push(v),
                }
                i += 1;
            }
            "--from" => {
                c.from = val(i)?.parse()?;
                i += 1;
            }
            "--exact" => c.exact = true,
            "--music-start" => {
                let v = val(i)?;
                c.music_start =
                    Some(util::parse_time(&v).ok_or_else(|| anyhow::anyhow!("bad time {v}"))?);
                i += 1;
            }
            "--to" => {
                c.to = Some(val(i)?.parse()?);
                i += 1;
            }
            "--seed" => {
                c.seed = Some(val(i)?.parse()?);
                i += 1;
            }
            "--part" => {
                let v = val(i)?;
                let (x, y) = v
                    .split_once('-')
                    .ok_or_else(|| anyhow::anyhow!("--part needs FROM-TO, e.g. 10-40"))?;
                let (x, y) = (
                    util::parse_time(x).ok_or_else(|| anyhow::anyhow!("bad time {x}"))?,
                    util::parse_time(y).ok_or_else(|| anyhow::anyhow!("bad time {y}"))?,
                );
                c.part = Some((x, y));
                i += 1;
            }
            "--codec" | "--encoder" | "--preset" | "--quality" | "--fps" | "--crf" | "--format"
            | "--font" => {
                c.overrides.push((a[2..].to_string(), val(i)?));
                i += 1;
            }
            x => anyhow::bail!("unknown option {x}\n\n{HELP}"),
        }
        i += 1;
    }
    Ok(c)
}

/// Output options on top of a settings set (checked: an unknown value is an error, not a silent default)
fn apply_overrides(
    s: &mut settings::Settings,
    o: &[(String, String)],
    project: bool,
) -> anyhow::Result<()> {
    use settings::*;
    for (k, v) in o {
        match k.as_str() {
            "codec" if CODECS.iter().any(|c| c.0 == v) => s.codec = v.clone(),
            "encoder" if ENCODERS.iter().any(|c| c.0 == v) => s.encoder = v.clone(),
            "preset" if ENC_SPEEDS.iter().any(|c| c.0 == v) => s.enc_speed = v.clone(),
            "quality" if QUALITIES.iter().any(|q| v.parse() == Ok(q.0)) => s.quality = v.parse()?,
            "fps" if FPS_CHOICES.iter().any(|q| v.parse() == Ok(*q)) => s.export_fps = v.parse()?,
            "crf" => s.export_crf = v.parse::<f64>()?.clamp(12.0, 40.0),
            "format" if project => anyhow::bail!("a project's format is part of its timeline"),
            "format" if RATIOS.iter().any(|c| c.0 == v) => s.format = v.clone(),
            "font" => {
                text::Font::load(std::path::Path::new(v))?;
                s.font = v.clone();
            }
            _ => anyhow::bail!("--{k} {v}: not a possible value\n\n{HELP}"),
        }
    }
    Ok(())
}

/// Prints progress lines every second while `f` runs
fn with_progress<T>(prog: &Arc<Mutex<export::Progress>>, f: impl FnOnce() -> T) -> T {
    let done = Arc::new(AtomicBool::new(false));
    let (d2, p2) = (done.clone(), prog.clone());
    let printer = std::thread::spawn(move || {
        let mut last = String::new();
        while !d2.load(Ordering::SeqCst) {
            let p = p2.lk().clone();
            let line = format!("{:5.1} %  {}", p.frac * 100.0, p.msg);
            if line != last && !p.msg.is_empty() {
                eprintln!("{line}");
                last = line;
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
    });
    let r = f();
    done.store(true, Ordering::SeqCst);
    let _ = printer.join();
    r
}

fn cli(args: &[String]) -> anyhow::Result<()> {
    let c = parse_args(args)?;
    match c.mode.as_str() {
        "help" => {
            print!("{HELP}");
            return Ok(());
        }
        "export" | "generate" | "render" | "analyze" | "encoders" | "shots" | "shots-dump"
        | "align" => {}
        _ => anyhow::bail!("nothing to do\n\n{HELP}"),
    }
    ff::available()?;
    if c.mode == "encoders" {
        println!("Codec  Encoder kind  ffmpeg encoder");
        for (codec, hw, name) in enc::survey() {
            println!("{codec:<6} {hw:<13} {name}");
        }
        return Ok(());
    }
    if c.mode == "align" {
        let pmv = c.song.clone().unwrap_or_default();
        let song = c
            .clips
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("--align PMV needs --clips SONG"))?;
        let f = align::find(&pmv, &song, &AtomicBool::new(false))?;
        println!(
            "The song starts at {:.4} s of the PMV (clarity {:.1}, pieces agreeing {:.0} %)",
            f.offset,
            f.clarity,
            f.agree * 100.0
        );
        return Ok(());
    }
    if c.mode == "shots" || c.mode == "shots-dump" {
        let v = c.song.clone().unwrap_or_default();
        let secs = c.to.map(|t| t - c.from);
        if c.mode == "shots" {
            print!(
                "{}",
                shots::report(&v, c.from, secs, shots::NetUse::Always)?
            );
            println!("--- hybrid (clip spots):");
            print!(
                "{}",
                shots::report(&v, c.from, secs, shots::NetUse::Doubtful)?
            );
        } else {
            print!("{}", shots::dump(&v, c.from, secs)?);
        }
        return Ok(());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let prog = Arc::new(Mutex::new(export::Progress::default()));
    let stored = settings::load_stored();
    let pins: show::Pins = stored
        .pins
        .iter()
        .map(|(k, v)| (PathBuf::from(k), v.clone()))
        .collect();

    if c.mode == "render" {
        let path = c.project.clone().unwrap_or_default();
        let pf = project::read(&path)?;
        eprintln!("Opening {} …", path.display());
        let mut sess = project::open(pf, player::device_rate_or(48000), &|_| {}, &cancel)?;
        apply_overrides(&mut sess.settings, &c.overrides, true)?;
        let out = c.out.ok_or_else(|| anyhow::anyhow!("--out is missing"))?;
        let t0 = std::time::Instant::now();
        let job = export::Job {
            out,
            pins,
            range: c.part,
        };
        let r = with_progress(&prog, || export::run(&sess, &job, &prog, &cancel))?;
        eprintln!(
            "Wrote {} in {:.1} s ({})",
            r.display(),
            t0.elapsed().as_secs_f64(),
            prog.lk().encoder
        );
        return Ok(());
    }

    let song_path = c
        .song
        .clone()
        .or_else(|| c.template.clone())
        .ok_or_else(|| anyhow::anyhow!("--song is missing"))?;
    let rate = player::device_rate_or(48000);
    let (song, tpl) = if let Some(t) = &c.template {
        let kind = if c.exact {
            template::Kind::Exact
        } else {
            template::Kind::Classic
        };
        if kind == template::Kind::Classic && (c.song.is_some() || c.music_start.is_some()) {
            anyhow::bail!("--song and --music-start with a template need --exact (the classic template uses the PMV's music)");
        }
        eprintln!("Analysing the template {} …", t.display());
        let (s, raw) = template::scan(t, rate, kind, &|_, _| {}, &cancel)?;
        let (s, tpl) = if kind == template::Kind::Classic {
            (s, raw)
        } else {
            let music = match &c.song {
                Some(sp) => {
                    let f = align::find(t, sp, &cancel)?;
                    eprintln!(
                        "The song plays from {:.3} s of the PMV (clarity {:.1}, {:.0} % of the PMV matches)",
                        f.offset,
                        f.clarity,
                        f.agree * 100.0
                    );
                    template::Music::File {
                        path: sp.clone(),
                        at: f.offset,
                    }
                }
                None => template::Music::Pmv,
            };
            let start = c.music_start.unwrap_or(match &music {
                template::Music::File { at, .. } => at.max(0.0),
                template::Music::Pmv => 0.0,
            });
            if music == template::Music::Pmv && start == 0.0 {
                let tpl = raw.shifted(0.0, s.duration());
                (s, tpl)
            } else {
                template::with_music(t, &raw, &music, start, rate, &|_| {}, &cancel)?
            }
        };
        eprintln!(
            "Template: {} cuts, {} layout changes, {} flashes",
            tpl.stats.cuts, tpl.stats.layout_changes, tpl.stats.flashes
        );
        (s, Some(tpl))
    } else {
        eprintln!("Reading {} …", song_path.display());
        (
            song::load(&song_path, c.from, c.to, rate, &|_| {}, &cancel)?,
            None,
        )
    };
    if c.mode == "analyze" {
        let j = serde_json::json!({
            "bpm": song.a.bpm, "duration": song.duration(), "beats": song.a.beats, "energy": song.a.energy,
            "downbeats": song.bars.as_ref().map(|b| b.downbeat.iter().enumerate().filter(|(_, d)| **d).map(|(k, _)| k).collect::<Vec<_>>()),
        });
        println!("{}", serde_json::to_string_pretty(&j)?);
        return Ok(());
    }
    let mut s = settings::Settings::default();
    if let Some(f) = &c.settings {
        let (_, o) = settings::import_file(&std::fs::read(f)?)?;
        s = s.with_preset(&o);
    }
    apply_overrides(&mut s, &c.overrides, false)?;
    let lib: library::Shared = Default::default();
    {
        let mut l = lib.lk();
        for (k, v) in &stored.clip_skips {
            l.skips.insert(k.clone(), *v);
        }
    }
    for p in &c.clips {
        if p.is_dir() {
            library::add_folder(&lib, p);
        } else {
            library::add_files(&lib, std::slice::from_ref(p));
        }
    }
    // probe everything first (the size decides the shape filters)
    let busy = Arc::new(AtomicUsize::new(0));
    library::probe_all(&lib, &busy);
    while busy.load(Ordering::SeqCst) > 0 {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let (items, images) = {
        let l = lib.lk();
        for it in l.items.iter().filter(|i| i.bad.is_some()) {
            eprintln!(
                "skipped {}: {}",
                it.name,
                it.bad.clone().unwrap_or_default()
            );
        }
        (l.matching(&s), l.images())
    };
    eprintln!(
        "{} BPM, {:.1} s, {} beats; {} clips",
        song.a.bpm.round(),
        song.duration(),
        song.a.beats.len(),
        items.len()
    );
    let t0 = std::time::Instant::now();
    let song = Arc::new(song);
    let sess = with_progress(&prog, || {
        let p2 = prog.clone();
        show::generate(
            song.clone(),
            items,
            images,
            &s,
            show::GenOpts {
                pins: pins.clone(),
                template: tpl.as_ref(),
                seed: c.seed,
                progress: &move |f, m| {
                    let mut p = p2.lk();
                    p.frac = f;
                    p.msg = m;
                },
                cancel: &cancel,
            },
        )
    })?;
    eprintln!(
        "Generated {} shots in {:.1} s",
        sess.plan.shots.len(),
        t0.elapsed().as_secs_f64()
    );
    if let Some(p) = &c.save_project {
        sess.save(p)?;
        eprintln!("Saved the project {}", p.display());
    } else if c.mode == "generate" {
        anyhow::bail!("--save-project is missing");
    }
    if c.mode == "generate" {
        return Ok(());
    }
    let out = c.out.ok_or_else(|| anyhow::anyhow!("--out is missing"))?;
    let t0 = std::time::Instant::now();
    let job = export::Job {
        out,
        pins,
        range: c.part,
    };
    *prog.lk() = export::Progress::default();
    let r = with_progress(&prog, || export::run(&sess, &job, &prog, &cancel))?;
    for f in prog.lk().fails.iter() {
        eprintln!("clip left out – {f}");
    }
    eprintln!(
        "Wrote {} in {:.1} s ({})",
        r.display(),
        t0.elapsed().as_secs_f64(),
        prog.lk().encoder
    );
    Ok(())
}
