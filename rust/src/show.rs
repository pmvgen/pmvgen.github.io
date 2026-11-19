// Making a show: the plan from a song (or a template) and the settings, the clips for its shots, and what
// draws it – shared by the live show, the preview, the timeline and the export.

use crate::clipaudio::{self, ClipMix};
use crate::library::{Item, Kind};
use crate::media::{Media, MediaOpts, PaceInfo, Pick};
use crate::plan::{self, Plan};
use crate::project::Session;
use crate::render::Renderer;
use crate::settings::{ratio_of, Settings};
use crate::song::Song;
use crate::template::Template;
use crate::util::LockExt;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Picture size for a quality (short side) and the format (width:height); both even
pub fn size_for(s: &Settings, short: f64) -> (i32, i32) {
    let r = ratio_of(&s.format).unwrap_or(16.0 / 9.0);
    let short = short.max(16.0);
    let even = |n: f64| ((n / 2.0).round() as i32 * 2).max(16);
    if r >= 1.0 {
        (even(short * r), even(short))
    } else {
        (even(short), even(short / r))
    }
}

/// Pinned moments in the form the media want them
pub type Pins = HashMap<PathBuf, Vec<f64>>;

/// Can a layout be used with these clips (per-layout clip shape: enough clips of that shape)?
fn layout_checker<'a>(s: &'a Settings, items: &'a [Item]) -> impl Fn(&str) -> bool + 'a {
    move |id: &str| -> bool {
        let rule = s.layout_shape.get(id).map(|x| x.as_str()).unwrap_or("all");
        if rule != "portrait" && rule != "landscape" {
            return true;
        }
        let n = items
            .iter()
            .filter(|i| {
                i.w > 0
                    && if rule == "portrait" {
                        i.h > i.w
                    } else {
                        i.w >= i.h
                    }
            })
            .count();
        n >= plan::layout(id).groups
    }
}

fn usable_images(images: Vec<Item>) -> Vec<Item> {
    images
        .into_iter()
        .filter(|i| i.kind == Kind::Image && i.w > 0)
        .collect()
}

/// The plan of a show (at the export's picture size)
pub fn make_plan(
    song: &Song,
    items: &[Item],
    images: &[Item],
    s: &Settings,
    template: Option<&Template>,
    seed: u64,
) -> Plan {
    let (w, h) = size_for(s, s.quality);
    let ok = layout_checker(s, items);
    plan::make(&plan::Input {
        beats: &song.a.beats,
        energy: &song.a.energy,
        duration: song.duration(),
        // (no bars for templates – their cuts come from the PMV)
        bars: if s.bars && template.is_none() {
            song.bars.as_deref()
        } else {
            None
        },
        s,
        w,
        h,
        n_images: images.len(),
        layout_ok: &ok,
        seed,
        template: template.map(|t| t.events.as_slice()),
    })
}

fn title_of(s: &Settings, song: &Song) -> String {
    if s.title.trim().is_empty() {
        song.name.clone()
    } else {
        s.title.trim().to_string()
    }
}

/// A new session whose clips are chosen as it plays (the live show)
pub fn live_session(
    song: Arc<Song>,
    items: &[Item],
    images: Vec<Item>,
    s: &Settings,
    template: Option<&Template>,
) -> anyhow::Result<Session> {
    if items.is_empty() {
        anyhow::bail!("No clips match the filters – add a folder with videos (or images) first");
    }
    let images = usable_images(images);
    let plan = make_plan(&song, items, &images, s, template, rand::random());
    let n = plan.shots.len();
    Ok(Session {
        title: title_of(s, &song),
        song,
        settings: s.clone(),
        plan: Arc::new(plan),
        picks: vec![None; n],
        images,
        template: template.map(|t| t.name.clone()),
    })
}

pub struct GenOpts<'a> {
    pub pins: Pins,
    pub template: Option<&'a Template>,
    pub seed: Option<u64>,
    /// progress(share 0–1, text)
    pub progress: &'a (dyn Fn(f32, String) + Sync),
    pub cancel: &'a AtomicBool,
}

/// Generate a PMV: the plan and a clip for every shot (nothing is drawn yet)
pub fn generate(
    song: Arc<Song>,
    items: Vec<Item>,
    images: Vec<Item>,
    s: &Settings,
    o: GenOpts,
) -> anyhow::Result<Session> {
    if items.is_empty() {
        anyhow::bail!("No clips match the filters – add a folder with videos (or images) first");
    }
    crate::ff::available()?;
    let images = usable_images(images);
    (o.progress)(0.0, "Planning the cuts …".into());
    let seed = o.seed.unwrap_or_else(rand::random);
    let plan = Arc::new(make_plan(&song, &items, &images, s, o.template, seed));
    let media = Media::new(
        s,
        plan.clone(),
        items,
        images.clone(),
        MediaOpts {
            export: false,
            decode: false,
            fps: 30.0,
            picks: None,
            pins: o.pins,
            pace: Some(pace_of(&song)),
            bpm: song.a.bpm,
        },
    );
    let picks = media.resolve(
        &|done, total| {
            (o.progress)(
                0.02 + 0.98 * done as f32 / total.max(1) as f32,
                format!("Choosing clips … {done} / {total}"),
            )
        },
        o.cancel,
    );
    if o.cancel.load(Ordering::SeqCst) {
        anyhow::bail!("Cancelled");
    }
    if picks.iter().all(|p| p.is_none()) {
        let why = media
            .failures
            .lk()
            .iter()
            .take(3)
            .map(|f| format!("{}: {}", f.name, f.why))
            .collect::<Vec<_>>()
            .join("; ");
        anyhow::bail!(
            "None of the clips could be used{}",
            if why.is_empty() {
                String::new()
            } else {
                format!(" ({why})")
            }
        );
    }
    Ok(Session {
        title: title_of(s, &song),
        song,
        settings: s.clone(),
        plan,
        picks,
        images,
        template: o.template.map(|t| t.name.clone()),
    })
}

pub fn pace_of(song: &Song) -> Arc<PaceInfo> {
    Arc::new(PaceInfo {
        beats: song.a.beats.clone(),
        energy: song.a.energy.clone(),
        pace: song.a.pace.clone(),
    })
}

/// What draws a session
pub struct Built {
    pub plan: Arc<Plan>,
    pub media: Arc<Media>,
    pub renderer: Renderer,
}

pub struct BuildOpts {
    /// short side of the picture
    pub short: f64,
    /// a file: every frame waits for its clips
    pub export: bool,
    pub fps: f64,
    pub pins: Pins,
    /// items to choose from for shots without a clip (a live show)
    pub items: Vec<Item>,
}

/// Get a session ready to draw (at another size than the export's if asked)
pub fn build(sess: &Session, o: BuildOpts) -> Built {
    let (w, h) = size_for(&sess.settings, o.short);
    let plan = if (w, h) == (sess.plan.w, sess.plan.h) {
        sess.plan.clone()
    } else {
        Arc::new(sess.plan.resized(w, h))
    };
    let media = Media::new(
        &sess.settings,
        plan.clone(),
        o.items,
        sess.images.clone(),
        MediaOpts {
            export: o.export,
            decode: true,
            fps: o.fps,
            picks: Some(sess.picks.clone()),
            pins: o.pins,
            pace: Some(pace_of(&sess.song)),
            bpm: sess.song.a.bpm,
        },
    );
    let renderer = Renderer::new(
        plan.clone(),
        media.clone(),
        sess.title.clone(),
        sess.song.a.bpm,
        o.export,
    );
    media.warm_pictures(w as u32, h as u32);
    Built {
        plan,
        media,
        renderer,
    }
}

/// Clip audio for a playing show: each shot's sound is rendered when it gets its clip (a few seconds ahead)
pub fn hook_clip_audio(media: &Arc<Media>, mix: &Arc<ClipMix>, rate: u32) {
    let s = &media.s;
    if !s.fx.voice {
        media.set_hook(None);
        return;
    }
    let plan = media.plan.clone();
    let known = media.picks();
    let all_known = known.iter().all(|p| p.is_some());
    let specs = clipaudio::specs(&plan, s, all_known.then_some(known.as_slice()));
    let mut by_shot: HashMap<usize, Vec<clipaudio::VoiceSpec>> = HashMap::new();
    for v in specs {
        by_shot.entry(v.shot).or_default().push(v);
    }
    let by_shot = Arc::new(by_shot);
    let mix = mix.clone();
    media.set_hook(Some(Arc::new(move |shot: usize, pick: &Pick| {
        let Some(list) = by_shot.get(&shot) else {
            return;
        };
        if pick.kind != Kind::Video {
            return;
        }
        let (list, pick, plan, mix) = (list.clone(), pick.clone(), plan.clone(), mix.clone());
        std::thread::spawn(move || {
            for spec in &list {
                if let Some(v) = clipaudio::render(spec, &pick, &plan, rate) {
                    mix.add(v);
                }
            }
        });
    })));
}

/// New clips for some shots, chosen like the generator does (best moments, match cuts against their
/// neighbours, the music's pace …) from `sources`; the other shots keep theirs
pub fn swap_auto(
    sess: &Session,
    shots: &[usize],
    sources: Vec<Item>,
    pins: Pins,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> Vec<(usize, Pick)> {
    let mut picks = sess.picks.clone();
    for &i in shots {
        if let Some(p) = picks.get_mut(i) {
            *p = None;
        }
    }
    let m = Media::new(
        &sess.settings,
        sess.plan.clone(),
        sources,
        sess.images.clone(),
        MediaOpts {
            export: false,
            decode: false,
            fps: 30.0,
            picks: Some(picks),
            pins,
            pace: Some(pace_of(&sess.song)),
            bpm: sess.song.a.bpm,
        },
    );
    let all = m.resolve(&|d, t| progress(d as f32 / t.max(1) as f32), cancel);
    shots
        .iter()
        .filter_map(|&i| all.get(i).cloned().flatten().map(|p| (i, p)))
        .collect()
}

/// New clips for some shots: any clip of `sources`, from a random moment in its allowed range
pub fn swap_random(
    sess: &Session,
    shots: &[usize],
    sources: &[Item],
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> anyhow::Result<Vec<(usize, Pick)>> {
    use rand::seq::SliceRandom;
    let mut out = Vec::new();
    let mut rng = rand::thread_rng();
    for (k, &i) in shots.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            anyhow::bail!("Cancelled");
        }
        progress(k as f32 / shots.len().max(1) as f32);
        let Some(it) = sources.choose(&mut rng) else {
            break;
        };
        let mut it = it.clone();
        if it.w == 0 {
            if let Ok((w, h, d)) = crate::library::probe_item(&it.path, it.kind) {
                it.w = w;
                it.h = h;
                it.dur = d;
            }
        }
        if it.w > 0 {
            out.push((i, crate::media::random_pick(&it, &sess.settings)));
        }
    }
    Ok(out)
}

/// Before playing from t: wait until the clips on screen are ready (at most until `deadline`)
pub fn preroll(media: &Arc<Media>, t: f64, deadline: Instant) -> bool {
    let plan = media.plan.clone();
    media.seek(t);
    let (_, cur, _) = plan.shots_at(t);
    let all_ready = || {
        let a = media.assigned.lk();
        cur.iter().all(|id| {
            id.and_then(|i| a.get(&i))
                .map(|x| x.frames.any() || x.frames.done.load(Ordering::SeqCst))
                .unwrap_or(false)
        })
    };
    while Instant::now() < deadline {
        media.tick(t);
        let (ready, _) = media.ready_count();
        let known = media.picks().iter().all(|p| p.is_some());
        if all_ready() && (known || ready >= 2) {
            return true;
        }
        if all_ready() && Instant::now() + Duration::from_secs(14) > deadline {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    all_ready()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_for_every_format() {
        let mut s = Settings::default();
        for (f, _) in crate::settings::RATIOS {
            s.format = f.to_string();
            for (q, _) in crate::settings::QUALITIES {
                let (w, h) = size_for(&s, q);
                assert!(w % 2 == 0 && h % 2 == 0, "{f} {q}");
                assert_eq!(w.min(h) as f64, (q / 2.0).round() * 2.0, "{f} {q}");
                let r = ratio_of(f).unwrap();
                assert!(
                    ((w as f64 / h as f64) - r).abs() / r < 0.02,
                    "{f} {q}: {w}×{h}"
                );
            }
        }
        s.format = "16:9".into();
        assert_eq!(size_for(&s, 720.0), (1280, 720));
        assert_eq!(size_for(&s, 2160.0), (3840, 2160));
        s.format = "9:16".into();
        assert_eq!(size_for(&s, 1080.0), (1080, 1920));
        s.format = "32:9".into();
        assert_eq!(size_for(&s, 1080.0), (3840, 1080));
    }
}
