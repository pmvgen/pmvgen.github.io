// End to end, with real (generated) media: generate a PMV, save and open it as a project, swap clips, render it
// twice (the same frames and sound, bit for bit), and read a rendered PMV back as a template.
// Needs ffmpeg; skipped without it (or with PMVGEN_SKIP_E2E set).

use crate::export;
use crate::library::{self, Kind};
use crate::project;
use crate::settings::Settings;
use crate::show;
use crate::song;
use crate::util::LockExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn ffmpeg_ok() -> bool {
    std::env::var_os("PMVGEN_SKIP_E2E").is_none() && crate::ff::available().is_ok()
}

fn run(args: &[&str]) {
    let st = Command::new(crate::ff::ffmpeg_path().expect("ffmpeg"))
        .args(["-nostdin", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .expect("ffmpeg runs");
    assert!(st.success(), "ffmpeg {args:?}");
}

/// The decoded frames' and sound's hashes of a video (what a player shows and plays)
fn hashes(p: &Path, stream: &str) -> Vec<String> {
    let out = Command::new(crate::ff::ffmpeg_path().expect("ffmpeg"))
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(p)
        .args(["-map", stream, "-f", "framemd5", "-"])
        .output()
        .expect("ffmpeg runs");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.rsplit(',').next().unwrap_or("").trim().to_string())
        .collect()
}

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn media(dir: &Path) -> (PathBuf, PathBuf) {
    let clips = dir.join("clips");
    std::fs::create_dir_all(&clips).unwrap();
    let song = dir.join("song.wav");
    // 16 s at 120 BPM: quiet, then loud from 6 s (a drop)
    run(&[
        "-f",
        "lavfi",
        "-i",
        "aevalsrc='(0.2+0.7*gte(t,6))*sin(2*PI*60*t)*exp(-25*mod(t,0.5))':s=44100:d=16",
        song.to_str().unwrap(),
    ]);
    for (i, f) in [(1, 300), (2, 500), (3, 700)] {
        run(&[
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc2=size=320x180:rate=25:d=30,hue=h={}", i * 90),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={f}:sample_rate=44100:duration=30"),
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            clips.join(format!("c{i}.mp4")).to_str().unwrap(),
        ]);
    }
    (song, clips)
}

#[test]
fn generate_save_swap_render_template() {
    if !ffmpeg_ok() {
        eprintln!("ffmpeg not found – end-to-end test skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("pmvgen-e2e-{}", std::process::id()));
    let _tmp = Tmp(dir.clone());
    std::fs::create_dir_all(&dir).unwrap();
    let (song_path, clips) = media(&dir);
    let cancel = Arc::new(AtomicBool::new(false));

    // the song and the library
    let song = song::load(&song_path, 0.0, None, 44100, &|_| {}, &cancel).expect("song");
    assert!((song.a.bpm - 120.0).abs() < 3.0, "bpm {}", song.a.bpm);
    let lib: library::Shared = Default::default();
    library::add_folder(&lib, &clips);
    let busy = Arc::new(AtomicUsize::new(0));
    library::probe_all(&lib, &busy);
    while busy.load(Ordering::SeqCst) > 0 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let mut s = Settings {
        quality: 144.0,
        export_fps: 24.0,
        codec: "h264".into(),
        encoder: "cpu".into(),
        enc_speed: "fast".into(),
        voice_mode: "always".into(),
        intro: true,
        ..Default::default()
    };
    s.fx.voice = true;
    s.fx.speed = true;
    let items = lib.lk().matching(&s);
    assert_eq!(items.len(), 3);

    // generate: every shot gets a clip
    let song = Arc::new(song);
    let sess = show::generate(
        song.clone(),
        items.clone(),
        vec![],
        &s,
        show::GenOpts {
            pins: Default::default(),
            template: None,
            seed: Some(11),
            progress: &|_, _| {},
            cancel: &cancel,
        },
    )
    .expect("generate");
    assert!(sess.complete());
    assert!(sess
        .picks
        .iter()
        .flatten()
        .all(|p| p.kind == Kind::Video && p.start >= 0.0));

    // the project file: the same plan and clips after saving and opening
    let pfile = dir.join("p.pmvproj");
    sess.save(&pfile).expect("save");
    let pf = project::read(&pfile).expect("read");
    assert!(pf.picks.iter().flatten().all(|p| p.path.is_absolute()));
    let opened = project::open(pf, 44100, &|_| {}, &cancel).expect("open");
    assert_eq!(opened.picks, sess.picks);
    assert_eq!(opened.plan.shots.len(), sess.plan.shots.len());

    // swap two shots automatically and one at random: those change, the others don't
    let shots = [1usize, 3];
    let ch = show::swap_auto(
        &opened,
        &shots,
        items.clone(),
        Default::default(),
        &|_| {},
        &cancel,
    );
    assert_eq!(ch.len(), 2);
    let r = show::swap_random(&opened, &[5], &items, &|_| {}, &cancel).expect("random");
    assert_eq!(r.len(), 1);
    let mut edited = opened.clone();
    for (i, p) in ch.into_iter().chain(r) {
        edited.picks[i] = Some(p);
    }
    for (i, (a, b)) in edited.picks.iter().zip(&sess.picks).enumerate() {
        if ![1, 3, 5].contains(&i) {
            assert_eq!(a, b, "shot {i} must stay");
        }
    }

    // render twice: the same frames and the same sound
    let prog = Arc::new(Mutex::new(export::Progress::default()));
    let job = |name: &str| export::Job {
        out: dir.join(name),
        pins: Default::default(),
        range: Some((0.0, 10.0)),
    };
    let a = export::run(&edited, &job("a"), &prog, &cancel).expect("render a");
    let b = export::run(&edited, &job("b"), &prog, &cancel).expect("render b");
    assert_eq!(a.extension().unwrap(), "mp4");
    let (va, vb) = (hashes(&a, "0:v"), hashes(&b, "0:v"));
    assert_eq!(va.len(), 240, "10 s at 24 fps");
    assert_eq!(va, vb, "the same frames");
    assert_eq!(hashes(&a, "0:a"), hashes(&b, "0:a"), "the same sound");
    // no temporary files left
    let left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with('.'))
        .collect();
    assert!(left.is_empty(), "{left:?}");

    // a cancelled render leaves nothing behind
    let stop = Arc::new(AtomicBool::new(true));
    assert!(export::run(&edited, &job("c"), &prog, &stop).is_err());
    assert!(!dir.join("c.mp4").exists());

    // the rendered PMV as a template: its cuts are found
    let (_, tpl) = crate::template::scan(
        &a,
        44100,
        crate::template::Kind::Classic,
        &|_, _| {},
        &cancel,
    )
    .expect("template");
    assert!(tpl.stats.cuts >= 3, "{:?}", tpl.stats);
}
