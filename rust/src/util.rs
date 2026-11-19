// Small helpers shared by every module.

use std::io::Write;
use std::path::Path;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// Locking that survives a panicked holder: the data is still there (all shared state here stays consistent
/// between statements), so one failed worker never takes the whole app down with it.
pub trait LockExt<T> {
    fn lk(&self) -> MutexGuard<'_, T>;
}
impl<T> LockExt<T> for Mutex<T> {
    fn lk(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Condvar wait with a timeout that also survives poisoning
pub fn wait_timeout<'a, T>(cv: &Condvar, g: MutexGuard<'a, T>, d: Duration) -> MutexGuard<'a, T> {
    match cv.wait_timeout(g, d) {
        Ok((g, _)) => g,
        Err(e) => e.into_inner().0,
    }
}

/// Write a file so that it is either fully there or not at all (a temp file next to it, then renamed)
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!(".{name}.tmp{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// "1:23", "1:02:03"
pub fn fmt_time(sec: f64) -> String {
    let sec = if sec.is_finite() { sec.max(0.0) } else { 0.0 }.round() as i64;
    let (h, m, x) = (sec / 3600, (sec % 3600) / 60, sec % 60);
    if h > 0 {
        format!("{h}:{m:02}:{x:02}")
    } else {
        format!("{m}:{x:02}")
    }
}

/// "1:23.4" – a moment inside a video
pub fn fmt_at(sec: f64) -> String {
    let sec = if sec.is_finite() { sec.max(0.0) } else { 0.0 };
    let m = (sec / 60.0).floor();
    format!("{}:{:04.1}", m as i64, sec - m * 60.0)
}

/// "1:23.456" – an exact moment (to the millisecond)
pub fn fmt_ms(sec: f64) -> String {
    let sec = if sec.is_finite() { sec.max(0.0) } else { 0.0 };
    let ms = (sec * 1000.0).round() as i64;
    format!("{}:{:02}.{:03}", ms / 60000, (ms / 1000) % 60, ms % 1000)
}

/// "1:23", "1:02:03", "83" → seconds; "" → 0; None = not a time
pub fn parse_time(v: &str) -> Option<f64> {
    let v = v.trim().replace(',', ".");
    if v.is_empty() {
        return Some(0.0);
    }
    let mut acc = 0.0;
    for p in v.split(':') {
        let n: f64 = p.trim().parse().ok()?;
        if !n.is_finite() || n < 0.0 {
            return None;
        }
        acc = acc * 60.0 + n;
    }
    Some(acc)
}

/// Total order for f64 sorting (NaN last) – never panics like partial_cmp().unwrap()
pub fn cmp_f64(a: f64, b: f64) -> std::cmp::Ordering {
    a.total_cmp(&b)
}

/// A file name without characters Windows / Linux refuse
pub fn safe_file_name(s: &str, max: usize) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || (c as u32) < 32 {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_end_matches('.').to_string();
    let s: String = s.chars().take(max).collect();
    if s.is_empty() {
        "untitled".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn times() {
        assert_eq!(parse_time("1:23"), Some(83.0));
        assert_eq!(parse_time("1:02:03"), Some(3723.0));
        assert_eq!(parse_time(""), Some(0.0));
        assert_eq!(parse_time("x"), None);
        assert_eq!(parse_time("-1"), None);
        assert_eq!(fmt_time(83.4), "1:23");
        assert_eq!(fmt_time(f64::NAN), "0:00");
        assert_eq!(fmt_at(83.45), "1:23.5");
        assert_eq!(safe_file_name("a/b:c.", 10), "a_b_c");
        assert_eq!(safe_file_name("...", 10), "untitled");
    }
    #[test]
    fn poisoned_lock_still_works() {
        let m = std::sync::Arc::new(Mutex::new(5));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lk();
            panic!("boom");
        })
        .join();
        assert_eq!(*m.lk(), 5);
    }
}
