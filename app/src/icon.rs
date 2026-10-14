//! The app icon, drawn in code: a small deck with a 3x2 grid of coloured keys.

pub const SIZE: u32 = 64;

/// Coverage (0..1) of a rounded rectangle at pixel centre (x, y).
fn rounded_rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    // One pixel of anti-aliasing at the edge.
    (r - d + 0.5).clamp(0.0, 1.0)
}

fn over(dst: &mut [u8], color: [u8; 3], alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    let da = dst[3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    if out_a <= 0.0 {
        return;
    }
    for c in 0..3 {
        let blended = (color[c] as f32 * a + dst[c] as f32 * da * (1.0 - a)) / out_a;
        dst[c] = blended.round() as u8;
    }
    dst[3] = (out_a * 255.0).round() as u8;
}

/// RGBA pixels, `SIZE` x `SIZE`.
pub fn rgba() -> Vec<u8> {
    let s = SIZE as f32;
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    let keys: [[u8; 3]; 6] = [
        [80, 160, 255],
        [90, 200, 120],
        [240, 190, 80],
        [230, 90, 80],
        [170, 110, 240],
        [70, 200, 210],
    ];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let i = ((y * SIZE + x) * 4) as usize;
            let p = &mut px[i..i + 4];
            // Deck body.
            over(
                p,
                [44, 48, 58],
                rounded_rect(fx, fy, 2.0, 10.0, s - 2.0, s - 6.0, 9.0),
            );
            // Keys: 3 columns x 2 rows.
            let (gx, gy, k, gap) = (9.0, 17.0, 13.0, 4.5);
            for row in 0..2 {
                for col in 0..3 {
                    let x0 = gx + col as f32 * (k + gap);
                    let y0 = gy + row as f32 * (k + gap);
                    let c = keys[row * 3 + col];
                    over(p, c, rounded_rect(fx, fy, x0, y0, x0 + k, y0 + k, 3.0));
                }
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_has_expected_size_and_transparent_corner() {
        let px = super::rgba();
        assert_eq!(px.len(), (super::SIZE * super::SIZE * 4) as usize);
        assert_eq!(px[3], 0, "top-left corner is transparent");
        let mid = ((super::SIZE / 2 * super::SIZE + super::SIZE / 2) * 4) as usize;
        assert_eq!(px[mid + 3], 255, "centre is opaque");
    }
}
