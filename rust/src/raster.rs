// Fast pixel work for the compositor: drawing a picture with an affine transform (bilinear), and running
// per-row work on all cores. tiny-skia does shapes and text; the big pictures go through here.

use tiny_skia::{Pixmap, Transform};

pub fn threads() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 16)
    })
}

/// Run f(first_row, rows) over bands of a pixmap's rows on all cores
pub fn par_rows<F>(data: &mut [u8], row_bytes: usize, f: F)
where
    F: Fn(usize, &mut [u8]) + Sync,
{
    if row_bytes == 0 || data.is_empty() {
        return;
    }
    let rows = data.len() / row_bytes;
    let n = threads().min(rows.max(1));
    if n <= 1 || rows < 32 {
        f(0, data);
        return;
    }
    let per = rows.div_ceil(n);
    std::thread::scope(|sc| {
        for (i, chunk) in data.chunks_mut(per * row_bytes).enumerate() {
            let f = &f;
            sc.spawn(move || f(i * per, chunk));
        }
    });
}

/// Draw `src` onto `dst` through `t` (source pixels → destination pixels), only inside `clip`
/// (x0, y0, x1, y1), bilinear (or nearest), source-over with `opacity`. Pixmaps are premultiplied.
pub fn draw_affine(
    dst: &mut Pixmap,
    src: &Pixmap,
    t: Transform,
    clip: (i32, i32, i32, i32),
    opacity: f32,
    smooth: bool,
) {
    draw_affine_cm(dst, src, t, clip, opacity, smooth, None)
}

/// Fixed-point color matrix (×1024; offsets in 0–255 ×1024), applied to each drawn pixel
pub type CmFixed = [i32; 12];

#[inline]
fn apply_cm(m: &CmFixed, px: [u32; 4]) -> [u32; 4] {
    let (r, g, b, a) = (px[0] as i32, px[1] as i32, px[2] as i32, px[3] as i32);
    let f = |i: usize| {
        ((m[i] * r + m[i + 1] * g + m[i + 2] * b + (m[i + 3] * a) / 255) >> 10).clamp(0, a) as u32
    };
    [f(0), f(4), f(8), px[3]]
}

#[allow(clippy::too_many_arguments)]
pub fn draw_affine_cm(
    dst: &mut Pixmap,
    src: &Pixmap,
    t: Transform,
    clip: (i32, i32, i32, i32),
    opacity: f32,
    smooth: bool,
    cm: Option<&CmFixed>,
) {
    let Some(inv) = t.invert() else { return };
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    // the destination box the picture covers
    let corners = [
        (0.0, 0.0),
        (sw as f32, 0.0),
        (0.0, sh as f32),
        (sw as f32, sh as f32),
    ];
    let (mut bx0, mut by0, mut bx1, mut by1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in corners {
        let px = t.sx * x + t.kx * y + t.tx;
        let py = t.ky * x + t.sy * y + t.ty;
        bx0 = bx0.min(px);
        by0 = by0.min(py);
        bx1 = bx1.max(px);
        by1 = by1.max(py);
    }
    let x0 = (bx0.floor() as i32).max(clip.0).max(0);
    let y0 = (by0.floor() as i32).max(clip.1).max(0);
    let x1 = (bx1.ceil() as i32).min(clip.2).min(dw);
    let y1 = (by1.ceil() as i32).min(clip.3).min(dh);
    if x0 >= x1 || y0 >= y1 || opacity <= 0.0 {
        return;
    }
    let row_bytes = dw as usize * 4;
    let sdata = src.data();
    let op = (opacity.clamp(0.0, 1.0) * 256.0) as u32;
    let fw = sw as f32;
    let fh = sh as f32;
    let band = &mut dst.data_mut()[y0 as usize * row_bytes..y1 as usize * row_bytes];
    if inv.kx == 0.0 && inv.ky == 0.0 {
        // Only scaled / mirrored / moved (all the clips): per-column and per-row tables
        let cols: Vec<(i32, i32, u32, bool)> = (x0..x1)
            .map(|x| {
                let u = inv.sx * (x as f32 + 0.5) + inv.tx;
                let inside = u >= 0.0 && u < fw;
                if smooth {
                    let uu = (u - 0.5).clamp(0.0, fw - 1.0);
                    let ix = (uu as i32).min(sw - 1);
                    (
                        ix,
                        (ix + 1).min(sw - 1),
                        ((uu - ix as f32) * 256.0) as u32,
                        inside,
                    )
                } else {
                    let ix = (u.max(0.0) as i32).min(sw - 1);
                    (ix, ix, 0, inside)
                }
            })
            .collect();
        par_rows(band, row_bytes, |first, rows| {
            for (ri, row) in rows.chunks_exact_mut(row_bytes).enumerate() {
                let y = y0 + (first + ri) as i32;
                let v = inv.sy * (y as f32 + 0.5) + inv.ty;
                if v < 0.0 || v >= fh {
                    continue;
                }
                let (iy, iy1, fy) = if smooth {
                    let vv = (v - 0.5).clamp(0.0, fh - 1.0);
                    let iy = (vv as i32).min(sh - 1);
                    (iy, (iy + 1).min(sh - 1), ((vv - iy as f32) * 256.0) as u32)
                } else {
                    let iy = (v as i32).min(sh - 1);
                    (iy, iy, 0)
                };
                let r0 = &sdata[(iy * sw * 4) as usize..((iy + 1) * sw * 4) as usize];
                let r1 = &sdata[(iy1 * sw * 4) as usize..((iy1 + 1) * sw * 4) as usize];
                for (ci, &(ix, ix1, fx, inside)) in cols.iter().enumerate() {
                    if !inside {
                        continue;
                    }
                    let (a0, a1) = (ix as usize * 4, ix1 as usize * 4);
                    let mut px = [0u32; 4];
                    if fx == 0 && fy == 0 {
                        for c in 0..4 {
                            px[c] = r0[a0 + c] as u32;
                        }
                    } else {
                        for c in 0..4 {
                            let top = r0[a0 + c] as u32 * (256 - fx) + r0[a1 + c] as u32 * fx;
                            let bot = r1[a0 + c] as u32 * (256 - fx) + r1[a1 + c] as u32 * fx;
                            px[c] = (top * (256 - fy) + bot * fy) >> 16;
                        }
                    }
                    if let Some(m) = cm {
                        px = apply_cm(m, px);
                    }
                    let di = (x0 as usize + ci) * 4;
                    if op >= 256 && px[3] >= 255 {
                        row[di] = px[0] as u8;
                        row[di + 1] = px[1] as u8;
                        row[di + 2] = px[2] as u8;
                        row[di + 3] = 255;
                    } else {
                        let sa = (px[3] * op) >> 8;
                        let inv_a = 255 - sa.min(255);
                        for c in 0..4 {
                            let s = (px[c] * op) >> 8;
                            row[di + c] =
                                (s + (row[di + c] as u32 * inv_a + 127) / 255).min(255) as u8;
                        }
                    }
                }
            }
        });
        return;
    }
    par_rows(band, row_bytes, |first, rows| {
        for (ri, row) in rows.chunks_exact_mut(row_bytes).enumerate() {
            let y = y0 + (first + ri) as i32;
            let (dyf, dxf) = (y as f32 + 0.5, x0 as f32 + 0.5);
            let mut u = inv.sx * dxf + inv.kx * dyf + inv.tx;
            let mut v = inv.ky * dxf + inv.sy * dyf + inv.ty;
            for x in x0..x1 {
                if u >= 0.0 && v >= 0.0 && u < fw && v < fh {
                    let px = if smooth {
                        // bilinear, centres at .5
                        let uu = (u - 0.5).max(0.0);
                        let vv = (v - 0.5).max(0.0);
                        let ix = (uu as i32).min(sw - 1);
                        let iy = (vv as i32).min(sh - 1);
                        let ix1 = (ix + 1).min(sw - 1);
                        let iy1 = (iy + 1).min(sh - 1);
                        let fx = ((uu - ix as f32) * 256.0) as u32;
                        let fy = ((vv - iy as f32) * 256.0) as u32;
                        let i00 = ((iy * sw + ix) * 4) as usize;
                        let i01 = ((iy * sw + ix1) * 4) as usize;
                        let i10 = ((iy1 * sw + ix) * 4) as usize;
                        let i11 = ((iy1 * sw + ix1) * 4) as usize;
                        let mut o = [0u32; 4];
                        for c in 0..4 {
                            let a = sdata[i00 + c] as u32 * (256 - fx) + sdata[i01 + c] as u32 * fx;
                            let b = sdata[i10 + c] as u32 * (256 - fx) + sdata[i11 + c] as u32 * fx;
                            o[c] = (a * (256 - fy) + b * fy) >> 16;
                        }
                        o
                    } else {
                        let i =
                            (((v as i32).min(sh - 1) * sw + (u as i32).min(sw - 1)) * 4) as usize;
                        [
                            sdata[i] as u32,
                            sdata[i + 1] as u32,
                            sdata[i + 2] as u32,
                            sdata[i + 3] as u32,
                        ]
                    };
                    let px = match cm {
                        Some(m) => apply_cm(m, px),
                        None => px,
                    };
                    let di = x as usize * 4;
                    if op >= 256 && px[3] >= 255 {
                        row[di] = px[0] as u8;
                        row[di + 1] = px[1] as u8;
                        row[di + 2] = px[2] as u8;
                        row[di + 3] = 255;
                    } else {
                        let sa = (px[3] * op) >> 8;
                        let inv_a = 255 - sa.min(255);
                        for c in 0..4 {
                            let s = (px[c] * op) >> 8;
                            row[di + c] =
                                (s + (row[di + c] as u32 * inv_a + 127) / 255).min(255) as u8;
                        }
                    }
                }
                u += inv.sx;
                v += inv.ky;
            }
        }
    });
}

/// Source-over a same-size-or-smaller pixmap at an integer offset (fast)
pub fn blit(dst: &mut Pixmap, src: &Pixmap, ox: i32, oy: i32) {
    draw_affine(
        dst,
        src,
        Transform::from_translate(ox as f32, oy as f32),
        (0, 0, i32::MAX, i32::MAX),
        1.0,
        false,
    );
}
