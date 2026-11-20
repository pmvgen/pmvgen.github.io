// The clip library: videos and images from the folders you add (subfolders included).
// Size and duration are read in the background with ffprobe (needed for "Portrait only" etc.).

use crate::ff;
use crate::settings::{ClipSkip, Settings};
use crate::util::LockExt;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Video,
    Image,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: usize,
    pub kind: Kind,
    pub path: PathBuf,
    pub name: String,
    pub root: PathBuf,
    pub w: u32,
    pub h: u32,
    pub dur: f64,
    pub probed: bool,
    pub bad: Option<String>,
    /// this video's own skip of its start / end (None = the setting for all videos)
    pub skip: ClipSkip,
}

pub const VIDEO: [&str; 12] = [
    "mp4", "m4v", "webm", "mkv", "mov", "ogv", "avi", "wmv", "flv", "ts", "mts", "mpg",
];
pub const IMAGE: [&str; 7] = ["jpg", "jpeg", "png", "webp", "gif", "avif", "bmp"];

pub fn kind_of(p: &Path) -> Option<Kind> {
    let e = p.extension()?.to_string_lossy().to_lowercase();
    if VIDEO.contains(&e.as_str()) {
        Some(Kind::Video)
    } else if IMAGE.contains(&e.as_str()) {
        Some(Kind::Image)
    } else {
        None
    }
}

#[derive(Default)]
pub struct Library {
    pub items: Vec<Item>,
    pub roots: Vec<PathBuf>,
    pub off: HashSet<PathBuf>, // added folders that are switched off
    seen: HashSet<PathBuf>,
    next: usize,
    /// per-video skips (file path → seconds), applied to the items as they are added
    pub skips: std::collections::HashMap<String, ClipSkip>,
}

pub type Shared = Arc<Mutex<Library>>;

impl Library {
    fn add_file(&mut self, p: PathBuf, root: &Path) -> bool {
        let Some(kind) = kind_of(&p) else {
            return false;
        };
        if !self.seen.insert(p.clone()) {
            return false;
        }
        self.next += 1;
        let p_key = p.to_string_lossy().to_string();
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        self.items.push(Item {
            id: self.next,
            kind,
            path: p,
            name,
            root: root.to_path_buf(),
            w: 0,
            h: 0,
            dur: 0.0,
            probed: false,
            bad: None,
            skip: self.skips.get(&p_key).copied().unwrap_or_default(),
        });
        true
    }
    pub fn remove_root(&mut self, root: &Path) {
        self.roots.retain(|r| r != root);
        self.off.remove(root);
        let gone: Vec<PathBuf> = self
            .items
            .iter()
            .filter(|i| i.root == root)
            .map(|i| i.path.clone())
            .collect();
        for g in gone {
            self.seen.remove(&g);
        }
        self.items.retain(|i| i.root != root);
    }

    pub fn matching(&self, s: &Settings) -> Vec<Item> {
        self.items
            .iter()
            .filter(|it| fits_filters(it, s) && !self.off.contains(&it.root))
            .cloned()
            .collect()
    }
    pub fn images(&self) -> Vec<Item> {
        self.items
            .iter()
            .filter(|it| it.kind == Kind::Image && it.bad.is_none() && !self.off.contains(&it.root))
            .cloned()
            .collect()
    }
    /// Set one video's own skips (and remember them for when it is added again)
    pub fn set_skip(&mut self, path: &Path, skip: ClipSkip) {
        let key = path.to_string_lossy().to_string();
        if skip.is_empty() {
            self.skips.remove(&key);
        } else {
            self.skips.insert(key, skip);
        }
        for it in self.items.iter_mut().filter(|i| i.path == path) {
            it.skip = skip;
        }
    }
    pub fn videos(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(|i| i.kind == Kind::Video && i.bad.is_none())
    }
    pub fn count(&self, s: &Settings) -> (usize, usize, usize) {
        let list = self.matching(s);
        let v = list.iter().filter(|i| i.kind == Kind::Video).count();
        let im = list.len() - v;
        let pending = self.items.iter().filter(|i| !i.probed).count();
        (v, im, pending)
    }
}

fn kind_ok(it: &Item, source: &str) -> bool {
    match source {
        "both" => true,
        "image" => it.kind == Kind::Image,
        _ => it.kind == Kind::Video,
    }
}
pub fn shape_ok(w: u32, h: u32, shape: &str) -> bool {
    if shape == "all" || w == 0 {
        return true; // not measured yet → decided when it's prepared
    }
    if shape == "portrait" {
        h > w
    } else {
        w > h
    }
}
pub fn fits_filters(it: &Item, s: &Settings) -> bool {
    it.bad.is_none()
        && kind_ok(it, &s.source)
        && shape_ok(it.w, it.h, &s.shape)
        && (it.kind != Kind::Video || !it.probed || it.dur > 4.0)
}

/// Walk a folder (and its subfolders) and add every clip; returns how many were added
pub fn add_folder(lib: &Shared, root: &Path) -> usize {
    // (absolute paths: projects and pins refer to them from anywhere)
    let root_abs = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let root = root_abs.as_path();
    let mut stack = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                // (hidden folders are skipped)
                if !e.file_name().to_string_lossy().starts_with('.') {
                    stack.push(p);
                }
            } else if ft.is_file() || ft.is_symlink() {
                found.push(p);
            }
        }
    }
    found.sort();
    let mut l = lib.lk();
    if !l.roots.iter().any(|r| r == root) {
        l.roots.push(root.to_path_buf());
    }
    let mut n = 0;
    for p in found {
        if l.add_file(p, root) {
            n += 1;
        }
    }
    n
}

pub fn add_files(lib: &Shared, files: &[PathBuf]) -> usize {
    let root = PathBuf::from("(single files)");
    let mut l = lib.lk();
    if !l.roots.iter().any(|r| r == &root) {
        l.roots.push(root.clone());
    }
    files
        .iter()
        .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()))
        .filter(|p| l.add_file(p.clone(), &root))
        .count()
}

/// Probe every unprobed item in the background (a few at a time)
pub fn probe_all(lib: &Shared, busy: &Arc<AtomicUsize>) {
    if busy.load(Ordering::SeqCst) > 0 {
        return;
    }
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 6);
    for _ in 0..workers {
        let lib = lib.clone();
        let busy = busy.clone();
        busy.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            loop {
                // take the next unprobed one (marking it so no other worker takes it)
                let job = {
                    let mut l = lib.lk();
                    l.items.iter_mut().find(|i| !i.probed).map(|i| {
                        i.probed = true;
                        (i.id, i.path.clone(), i.kind)
                    })
                };
                let Some((id, path, kind)) = job else { break };
                let res = probe_item(&path, kind);
                let mut l = lib.lk();
                if let Some(it) = l.items.iter_mut().find(|i| i.id == id) {
                    match res {
                        Ok((w, h, d)) => {
                            it.w = w;
                            it.h = h;
                            it.dur = d;
                        }
                        Err(e) => it.bad = Some(e),
                    }
                }
            }
            busy.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

pub fn probe_item(path: &Path, kind: Kind) -> Result<(u32, u32, f64), String> {
    match ff::probe(path) {
        Ok(p) if p.has_video && p.w > 0 && p.h > 0 => {
            if kind == Kind::Video && p.dur <= 0.0 {
                return Err("no duration – can't be played".into());
            }
            Ok((p.w, p.h, p.dur))
        }
        Ok(_) => Err("no picture in this file".into()),
        Err(e) => Err(e.to_string()),
    }
}
