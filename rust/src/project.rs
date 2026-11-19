// A generated PMV: the song, the settings, the plan (every cut, layout, effect and speed ramp) and the clip of
// every shot (file, moment, measurement). In memory a `Session`; on disk a project file (.pmvproj, JSON) from
// which the same PMV is rendered again exactly – as long as the song and the clips are the same files.

use crate::library::{Item, Kind};
use crate::media::Pick;
use crate::plan::Plan;
use crate::settings::Settings;
use crate::song::{self, Song};
use crate::util::write_atomic;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub const EXT: &str = "pmvproj";
const VERSION: u32 = 1;

/// A generated PMV in memory
#[derive(Clone)]
pub struct Session {
    pub song: Arc<Song>,
    pub settings: Settings,
    /// at the export's picture size
    pub plan: Arc<Plan>,
    /// one per shot; None = not chosen yet (a live show chooses as it goes)
    pub picks: Vec<Option<Pick>>,
    /// the pictures the image flash / overlay show (the plan refers to them by number)
    pub images: Vec<Item>,
    /// PMV as template: its name
    pub template: Option<String>,
    /// the title cards' title
    pub title: String,
}

impl Session {
    pub fn complete(&self) -> bool {
        self.picks.len() == self.plan.shots.len() && self.picks.iter().all(|p| p.is_some())
    }

    pub fn to_file(&self) -> ProjectFile {
        // (absolute paths: the project opens from anywhere)
        let abs = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
        let mut picks = self.picks.clone();
        for p in picks.iter_mut().flatten() {
            p.path = abs(&p.path);
        }
        ProjectFile {
            pmvgen_project: VERSION,
            app: format!("pmvgen {}", env!("CARGO_PKG_VERSION")),
            song: SongRef {
                path: abs(&self.song.path),
                name: self.song.name.clone(),
                from: self.song.from,
                to: self.song.to,
                tempo: self.song.tempo,
                shift_ms: self.song.shift_ms,
                duration: self.song.duration(),
            },
            settings: self.settings.clone(),
            plan: (*self.plan).clone(),
            picks,
            images: self
                .images
                .iter()
                .map(|i| ImageRef {
                    path: abs(&i.path),
                    w: i.w,
                    h: i.h,
                })
                .collect(),
            template: self.template.clone(),
            title: self.title.clone(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.to_file())?;
        write_atomic(path, &bytes).with_context(|| format!("writing {}", path.display()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SongRef {
    pub path: PathBuf,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub from: f64,
    #[serde(default)]
    pub to: Option<f64>,
    /// ½ / 2× tempo as chosen
    #[serde(default = "one")]
    pub tempo: f64,
    /// Earlier / Later
    #[serde(default)]
    pub shift_ms: f64,
    /// to notice a different file under the same name
    #[serde(default)]
    pub duration: f64,
}
fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageRef {
    pub path: PathBuf,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub pmvgen_project: u32,
    #[serde(default)]
    pub app: String,
    pub song: SongRef,
    pub settings: Settings,
    pub plan: Plan,
    pub picks: Vec<Option<Pick>>,
    #[serde(default)]
    pub images: Vec<ImageRef>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub title: String,
}

/// Read and check a project file. Paths in it that aren't absolute count from the project's folder; a file
/// that isn't where the project says, but lies in the project's folder under its name, is taken from there.
pub fn read(path: &Path) -> Result<ProjectFile> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut pf = parse(&bytes)?;
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let fix = |p: &mut PathBuf| {
        if p.is_relative() {
            *p = dir.join(&*p);
        }
        if !p.is_file() {
            if let Some(n) = p.file_name() {
                let near = dir.join(n);
                if near.is_file() {
                    *p = near;
                }
            }
        }
    };
    fix(&mut pf.song.path);
    for p in pf.picks.iter_mut().flatten() {
        fix(&mut p.path);
    }
    for i in pf.images.iter_mut() {
        fix(&mut i.path);
    }
    Ok(pf)
}

pub fn parse(bytes: &[u8]) -> Result<ProjectFile> {
    let mut pf: ProjectFile =
        serde_json::from_slice(bytes).context("This isn't a PMV Generator project file")?;
    if pf.pmvgen_project == 0 || pf.pmvgen_project > VERSION {
        bail!(
            "This project was made with a newer version of PMV Generator (format {})",
            pf.pmvgen_project
        );
    }
    pf.settings.sanitize();
    pf.plan.finish();
    pf.plan
        .validate()
        .context("The project's timeline is damaged")?;
    if pf.picks.len() != pf.plan.shots.len() {
        bail!(
            "The project's timeline is damaged: {} shots but {} clips",
            pf.plan.shots.len(),
            pf.picks.len()
        );
    }
    for (i, p) in pf.picks.iter().flatten().enumerate() {
        if !(p.start.is_finite() && p.start >= 0.0 && p.tempo() > 0.0 && p.tempo() < 4.0) {
            bail!("The project's clip {i} is damaged");
        }
    }
    let n_img = pf.images.len();
    for e in &pf.plan.fx {
        if let crate::plan::Fx::PicFlash { pic, .. } | crate::plan::Fx::PicOver { pic, .. } = e.fx {
            if pic >= n_img {
                bail!("The project's picture list is damaged");
            }
        }
    }
    Ok(pf)
}

/// Files the project needs that aren't there (the song first)
pub fn missing_files(pf: &ProjectFile) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut check = |p: &Path| {
        if !p.is_file() && !out.iter().any(|x| x == p) {
            out.push(p.to_path_buf());
        }
    };
    check(&pf.song.path);
    for p in pf.picks.iter().flatten() {
        check(&p.path);
    }
    for i in &pf.images {
        check(&i.path);
    }
    out
}

/// Open a project: the song is read again (its sound for playing, its beats for the funscript); the
/// timeline and the clips come from the file
pub fn open(
    pf: ProjectFile,
    rate: u32,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> Result<Session> {
    let missing = missing_files(&pf);
    if !missing.is_empty() {
        let list: Vec<String> = missing
            .iter()
            .take(5)
            .map(|p| p.display().to_string())
            .collect();
        bail!(
            "{} file{} of this project {} missing: {}{}",
            missing.len(),
            if missing.len() == 1 { "" } else { "s" },
            if missing.len() == 1 { "is" } else { "are" },
            list.join(", "),
            if missing.len() > 5 { " …" } else { "" }
        );
    }
    let mut s = song::load(
        &pf.song.path,
        pf.song.from,
        pf.song.to,
        rate,
        progress,
        cancel,
    )?;
    if (pf.song.tempo - 1.0).abs() > 1e-9 {
        s.rescale(pf.song.tempo);
    }
    if pf.song.shift_ms != 0.0 {
        s.nudge(pf.song.shift_ms);
    }
    let dur = s.duration();
    if (dur - pf.plan.duration).abs() > 0.25 {
        bail!(
            "The song {} isn't the one this project was made with (it is {:.1} s long, the project {:.1} s)",
            pf.song.path.display(),
            dur,
            pf.plan.duration
        );
    }
    let images = pf
        .images
        .iter()
        .enumerate()
        .map(|(i, r)| Item {
            id: 1_000_000 + i,
            kind: Kind::Image,
            path: r.path.clone(),
            name: r
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            root: r.path.parent().map(Path::to_path_buf).unwrap_or_default(),
            w: r.w,
            h: r.h,
            dur: 0.0,
            probed: true,
            bad: None,
            skip: Default::default(),
        })
        .collect();
    Ok(Session {
        song: Arc::new(s),
        settings: pf.settings,
        plan: Arc::new(pf.plan),
        picks: pf.picks,
        images,
        template: pf.template,
        title: pf.title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{make, Input};

    fn file() -> ProjectFile {
        let beats: Vec<f64> = (0..80).map(|i| i as f64 * 0.5).collect();
        let energy = vec![0.7; 80];
        let st = Settings::default();
        let plan = make(&Input {
            beats: &beats,
            energy: &energy,
            duration: 40.0,
            bars: None,
            s: &st,
            w: 1280,
            h: 720,
            n_images: 0,
            layout_ok: &|_| true,
            seed: 9,
            template: None,
        });
        let pick = Pick {
            path: "/nonexistent/clip.mp4".into(),
            name: "clip.mp4".into(),
            kind: Kind::Video,
            w: 1920,
            h: 1080,
            dur: 100.0,
            start: 20.0,
            end_at: Some(99.6),
            kb: 0.0,
            beat: Some(1.04),
            speed: None,
            motion: Some(0.2),
            sig: None,
            pinned: false,
            manual: true,
            clean: None,
        };
        ProjectFile {
            pmvgen_project: 1,
            app: "test".into(),
            song: SongRef {
                path: "/nonexistent/song.mp3".into(),
                name: "song".into(),
                from: 0.0,
                to: None,
                tempo: 1.0,
                shift_ms: 0.0,
                duration: 40.0,
            },
            settings: st,
            picks: vec![Some(pick); plan.shots.len()],
            plan,
            images: vec![],
            template: None,
            title: "T".into(),
        }
    }

    #[test]
    fn floats_survive_the_file_exactly() {
        // (the same project must draw the same pictures and sound: every number comes back bit for bit)
        let mut pf = file();
        let mut x = 0.1f64;
        for p in pf.picks.iter_mut().flatten() {
            x = (x * 7.000_000_123_456).fract() + 0.123_456_789_012_345_6;
            p.start = x * 97.3;
        }
        let back = parse(&serde_json::to_vec(&pf).unwrap()).unwrap();
        for (a, b) in back.picks.iter().zip(&pf.picks) {
            assert_eq!(
                a.as_ref().unwrap().start.to_bits(),
                b.as_ref().unwrap().start.to_bits()
            );
        }
        for (a, b) in back.plan.shots.iter().zip(&pf.plan.shots) {
            assert_eq!(a.t_on.to_bits(), b.t_on.to_bits());
            assert_eq!(a.until.to_bits(), b.until.to_bits());
        }
        for (a, b) in back.plan.rates.iter().zip(&pf.plan.rates) {
            assert_eq!(a.0.to_bits(), b.0.to_bits());
        }
    }

    #[test]
    fn round_trip_and_checks() {
        let pf = file();
        let bytes = serde_json::to_vec(&pf).unwrap();
        let back = parse(&bytes).unwrap();
        assert_eq!(back.picks, pf.picks);
        assert_eq!(back.plan.shots.len(), pf.plan.shots.len());
        assert_eq!(back.plan.rate_integral(30.0), pf.plan.rate_integral(30.0));
        // both files are missing, listed once each
        let m = missing_files(&back);
        assert_eq!(m.len(), 2);
        assert!(open(back, 44100, &|_| {}, &AtomicBool::new(false)).is_err());

        // damaged files are refused, not drawn
        let mut bad = pf.clone();
        bad.picks.pop();
        assert!(parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        let mut bad = pf.clone();
        bad.plan.shots[3].phase = 999;
        assert!(parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        let mut bad = pf;
        bad.pmvgen_project = 99;
        assert!(parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        assert!(parse(b"{}").is_err());
        assert!(parse(b"garbage").is_err());
    }
}
