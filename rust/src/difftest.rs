// Replays the cases recorded from the original JavaScript (tests/record.mjs → fixtures.json) against the port.
// Run: PMVGEN_FIXTURES=/path/to/dir cargo test --release diff_ -- --nocapture

use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn fixtures() -> Option<(PathBuf, Value)> {
    let dir = PathBuf::from(std::env::var("PMVGEN_FIXTURES").ok()?);
    let v: Value = serde_json::from_slice(&std::fs::read(dir.join("fixtures.json")).ok()?).ok()?;
    Some((dir, v))
}
fn f64s(v: &Value) -> Vec<f64> {
    v.as_array()
        .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect())
        .unwrap_or_default()
}
fn close(name: &str, a: &[f64], b: &[f64], tol: f64) -> usize {
    assert_eq!(
        a.len(),
        b.len(),
        "{name}: length {} (Rust) vs {} (JS)",
        a.len(),
        b.len()
    );
    let bad = a
        .iter()
        .zip(b)
        .filter(|(x, y)| (*x - *y).abs() > tol * (1.0 + y.abs()))
        .count();
    if bad > 0 {
        let i = a
            .iter()
            .zip(b)
            .position(|(x, y)| (*x - *y).abs() > tol * (1.0 + y.abs()))
            .unwrap();
        eprintln!(
            "  {name}: {bad} of {} differ (first at {i}: {} vs {})",
            a.len(),
            a[i],
            b[i]
        );
    }
    bad
}

#[test]
fn diff_songs() {
    let Some((dir, fx)) = fixtures() else {
        eprintln!("PMVGEN_FIXTURES not set – skipped");
        return;
    };
    let mut total_bad = 0;
    for c in fx["songs"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let s = crate::song::load(
            &dir.join(c["file"].as_str().unwrap()),
            0.0,
            None,
            44100,
            &|_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        let mut bad = 0;
        let bpm = c["bpm"].as_f64().unwrap();
        if (s.a.bpm - bpm).abs() > 1e-6 {
            eprintln!("  bpm {} vs {}", s.a.bpm, bpm);
            bad += 1;
        }
        bad += close("beats", &s.a.beats, &f64s(&c["beats"]), 1e-9);
        bad += close("energy", &s.a.energy, &f64s(&c["energy"]), 1e-6);
        bad += close("pace", &s.a.pace, &f64s(&c["pace"]), 1e-6);
        if let Some(b) = &s.bars {
            let db: Vec<f64> = b.downbeat.iter().map(|&x| x as u8 as f64).collect();
            let pos: Vec<f64> = b.pos.iter().map(|&x| x as f64).collect();
            let ph: Vec<f64> = b.phrase.iter().map(|&x| x as u8 as f64).collect();
            bad += close("downbeat", &db, &f64s(&c["downbeat"]), 0.0);
            bad += close("pos", &pos, &f64s(&c["pos"]), 0.0);
            bad += close("phrase", &ph, &f64s(&c["phrase"]), 0.0);
        } else if !c["downbeat"].is_null() {
            eprintln!("  no bars in Rust");
            bad += 1;
        }
        let r2 = s.a.rescale(2.0);
        bad += close("rescale 2×", &r2.beats, &f64s(&c["rescale2"]), 1e-9);
        bad += close(
            "rescale 2× energy",
            &r2.energy,
            &f64s(&c["rescale2Energy"]),
            1e-6,
        );
        bad += close(
            "rescale ½",
            &s.a.rescale(0.5).beats,
            &f64s(&c["rescaleHalf"]),
            1e-9,
        );
        bad += close(
            "shift +40 ms",
            &s.a.shift(0.04).beats,
            &f64s(&c["shift40"]),
            1e-9,
        );
        // funscripts
        for (i, o) in c["fs"].as_array().unwrap().iter().enumerate() {
            let st = crate::settings::Settings::default().with_preset(o);
            let fs = crate::funscript::build(
                &s.a.beats,
                &s.a.energy,
                s.a.bpm,
                s.a.duration,
                s.bars.as_deref(),
                &st,
                "",
            )
            .unwrap();
            let got: Vec<f64> = fs["actions"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|a| [a["at"].as_f64().unwrap(), a["pos"].as_f64().unwrap()])
                .collect();
            let want: Vec<f64> = c["fsActions"][i]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|a| [a["at"].as_f64().unwrap(), a["pos"].as_f64().unwrap()])
                .collect();
            bad += close(&format!("funscript {i}"), &got, &want, 0.0);
        }
        eprintln!(
            "{name}: {:.2} BPM, {} beats – {}",
            s.a.bpm,
            s.a.beats.len(),
            if bad == 0 {
                "all match".to_string()
            } else {
                format!("{bad} mismatches")
            }
        );
        total_bad += bad;
    }
    assert_eq!(total_bad, 0, "mismatches against the original");
}

#[test]
fn diff_layouts_and_pictures() {
    let Some((_, fx)) = fixtures() else { return };
    let mut n = 0;
    for c in fx["slots"].as_array().unwrap() {
        let got = crate::plan::slots_for(
            c["id"].as_str().unwrap(),
            c["W"].as_i64().unwrap() as i32,
            c["H"].as_i64().unwrap() as i32,
            c["dir"].as_str().unwrap(),
        );
        let want = c["slots"].as_array().unwrap();
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert_eq!(
                (g.x, g.y, g.w, g.h, g.g, g.fx, g.fy),
                (
                    w["x"].as_i64().unwrap() as i32,
                    w["y"].as_i64().unwrap() as i32,
                    w["w"].as_i64().unwrap() as i32,
                    w["h"].as_i64().unwrap() as i32,
                    w["g"].as_u64().unwrap() as usize,
                    w["fx"].as_bool().unwrap(),
                    w["fy"].as_bool().unwrap()
                ),
                "{c}"
            );
        }
        n += 1;
    }
    for c in fx["measure"].as_array().unwrap() {
        let px: Vec<u8> = c["px"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u8)
            .collect();
        let m = crate::smart::measure(
            &px,
            c["sw"].as_u64().unwrap() as usize,
            c["sh"].as_u64().unwrap() as usize,
            4,
        );
        let want = [
            c["focus"]["x"].as_f64().unwrap(),
            c["focus"]["y"].as_f64().unwrap(),
            c["skin"].as_f64().unwrap(),
            c["contrast"].as_f64().unwrap(),
            c["lum"].as_f64().unwrap(),
            c["color"]["r"].as_f64().unwrap(),
            c["color"]["g"].as_f64().unwrap(),
            c["color"]["b"].as_f64().unwrap(),
        ];
        let got = [
            m.focus.0, m.focus.1, m.skin, m.contrast, m.lum, m.color.0, m.color.1, m.color.2,
        ];
        assert_eq!(close("measure", &got, &want, 1e-9), 0);
        assert_eq!(close("hist", &m.hist, &f64s(&c["hist"]), 1e-6), 0);
        n += 1;
    }
    eprintln!("{n} layout / picture cases match");
}

#[test]
fn diff_long_mix() {
    let Some((dir, _)) = fixtures() else { return };
    let Ok(b) = std::fs::read(dir.join("fixtures_long.json")) else {
        return;
    };
    let c: Value = serde_json::from_slice(&b).unwrap();
    let s = crate::song::load(
        &dir.join(c["file"].as_str().unwrap()),
        0.0,
        None,
        22050,
        &|_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(s.a.long);
    let mut bad = 0;
    if (s.a.bpm - c["bpm"].as_f64().unwrap()).abs() > 1e-6 {
        eprintln!("  bpm {} vs {}", s.a.bpm, c["bpm"]);
        bad += 1;
    }
    bad += close("beats", &s.a.beats, &f64s(&c["beats"]), 1e-9);
    bad += close("energy", &s.a.energy, &f64s(&c["energy"]), 1e-6);
    bad += close("pace", &s.a.pace, &f64s(&c["pace"]), 1e-6);
    eprintln!(
        "46-min mix: {:.3} BPM, {} beats – {}",
        s.a.bpm,
        s.a.beats.len(),
        if bad == 0 {
            "all match".into()
        } else {
            format!("{bad} mismatches")
        }
    );
    assert_eq!(bad, 0);
}
