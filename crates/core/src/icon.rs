//! The tray icon, drawn in code so it can show the time that is left.
//!
//! It fills the whole square like the other tray icons: a rounded tile in
//! the state's color (tomato red working, amber stopped, green time's up)
//! with a white timer dial.  Inside the dial a dark pie shows the time left,
//! shrinking clockwise from 12 o'clock like a kitchen timer; a green leaf on
//! top of the dial makes it a tomato.  Time's up: a check mark.
//!
//! Output is ARGB32 in network byte order (A, R, G, B per pixel), which is
//! what StatusNotifierItem's `IconPixmap` expects.

use std::f64::consts::TAU;

use crate::timer::Phase;

type Rgb = (u8, u8, u8);

pub const TOMATO: Rgb = (0xe5, 0x39, 0x35);
pub const AMBER: Rgb = (0xfb, 0x8c, 0x00);
pub const GREEN: Rgb = (0x2e, 0x9d, 0x48);
const LEAF: Rgb = (0x2e, 0x7d, 0x32);
const DIAL: Rgb = (0xff, 0xff, 0xff);
/// The time left: a darker shade of the tile, so it stands out at 16 px.
const PIE_RED: Rgb = (0xa3, 0x15, 0x15);
const PIE_AMBER: Rgb = (0xb4, 0x53, 0x09);
const CHECK: Rgb = (0x1b, 0x6b, 0x30);

/// Supersampling per axis (antialiasing).
const SS: u32 = 4;

pub fn tile_color(phase: Phase) -> Rgb {
    match phase {
        Phase::Idle | Phase::Running => TOMATO,
        Phase::Paused => AMBER,
        Phase::Finished => GREEN,
    }
}

/// `fraction_left` is 1.0 for a full pie, 0.0 for an empty one.
pub fn render(size: u32, phase: Phase, fraction_left: f64) -> Vec<u8> {
    let fraction = fraction_left.clamp(0.0, 1.0);
    let n = f64::from(size);
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut a, mut rr, mut gg, mut bb) = (0.0, 0.0, 0.0, 0.0);
            for sy in 0..SS {
                for sx in 0..SS {
                    // Coordinates in 0..1 across the icon.
                    let u = (f64::from(x) + (f64::from(sx) + 0.5) / f64::from(SS)) / n;
                    let v = (f64::from(y) + (f64::from(sy) + 0.5) / f64::from(SS)) / n;
                    if let Some((cr, cg, cb)) = sample(u, v, phase, fraction) {
                        a += 1.0;
                        rr += f64::from(cr);
                        gg += f64::from(cg);
                        bb += f64::from(cb);
                    }
                }
            }
            if a > 0.0 {
                let samples = f64::from(SS * SS);
                out.extend_from_slice(&[
                    (a / samples * 255.0).round() as u8,
                    (rr / a).round() as u8,
                    (gg / a).round() as u8,
                    (bb / a).round() as u8,
                ]);
            } else {
                out.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    out
}

/// Color at a point (0..1 on both axes); `None` outside the tile.
fn sample(u: f64, v: f64, phase: Phase, fraction: f64) -> Option<Rgb> {
    if !in_rounded_square(u, v, 0.22) {
        return None;
    }
    if in_leaf(u, v) {
        return Some(LEAF);
    }
    // The dial, a bit below the middle to leave room for the leaf.
    let (cx, cy, r) = (0.5, 0.56, 0.38);
    let (dx, dy) = (u - cx, v - cy);
    let dist = dx.hypot(dy);
    if dist > r {
        return Some(tile_color(phase));
    }
    if phase == Phase::Finished {
        return Some(if in_check(dx / r, dy / r) {
            CHECK
        } else {
            DIAL
        });
    }
    if dist > r * 0.78 {
        return Some(DIAL);
    }
    // Angle clockwise from 12 o'clock, as a part of the full turn.
    let turn = dx.atan2(-dy).rem_euclid(TAU) / TAU;
    if turn >= fraction {
        Some(DIAL)
    } else if phase == Phase::Paused {
        Some(PIE_AMBER)
    } else {
        Some(PIE_RED)
    }
}

fn in_rounded_square(u: f64, v: f64, radius: f64) -> bool {
    let qx = (u - 0.5).abs() - (0.5 - radius);
    let qy = (v - 0.5).abs() - (0.5 - radius);
    qx.max(0.0).hypot(qy.max(0.0)) <= radius
}

/// Two small leaves and a stem on top of the dial.
fn in_leaf(u: f64, v: f64) -> bool {
    let ellipse = |cx: f64, cy: f64, rx: f64, ry: f64| {
        ((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2) <= 1.0
    };
    let stem = (u - 0.5).abs() < 0.045 && v > 0.07 && v < 0.22;
    stem || ellipse(0.39, 0.19, 0.13, 0.065) || ellipse(0.61, 0.19, 0.13, 0.065)
}

/// A check mark in dial units (the dial has radius 1).
fn in_check(x: f64, y: f64) -> bool {
    let near = |(ax, ay): (f64, f64), (bx, by): (f64, f64)| {
        let (vx, vy) = (bx - ax, by - ay);
        let t = (((x - ax) * vx + (y - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
        (x - ax - t * vx).hypot(y - ay - t * vy) < 0.14
    };
    near((-0.48, 0.0), (-0.12, 0.38)) || near((-0.12, 0.38), (0.52, -0.34))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        img[i..i + 4].try_into().unwrap()
    }

    fn opaque(c: Rgb) -> [u8; 4] {
        [255, c.0, c.1, c.2]
    }

    fn count(img: &[u8], c: Rgb) -> usize {
        img.chunks(4).filter(|p| *p == opaque(c)).count()
    }

    #[test]
    fn has_the_right_size() {
        for size in [16, 22, 32, 48] {
            assert_eq!(
                render(size, Phase::Running, 0.5).len(),
                (size * size * 4) as usize
            );
        }
    }

    #[test]
    fn fills_the_square_except_rounded_corners() {
        let img = render(32, Phase::Idle, 1.0);
        assert_eq!(pixel(&img, 32, 0, 0)[0], 0, "corner cut off");
        for (x, y) in [(16, 0), (0, 16), (31, 16), (16, 31), (3, 28)] {
            assert_eq!(pixel(&img, 32, x, y)[0], 255, "edge ({x},{y}) is covered");
        }
        let opaque_px = img.chunks(4).filter(|p| p[0] == 255).count();
        assert!(opaque_px > 32 * 32 * 9 / 10);
    }

    #[test]
    fn tile_color_shows_the_state() {
        let tile = |phase| pixel(&render(32, phase, 0.5), 32, 1, 16);
        assert_eq!(tile(Phase::Idle), opaque(TOMATO));
        assert_eq!(tile(Phase::Running), opaque(TOMATO));
        assert_eq!(tile(Phase::Paused), opaque(AMBER));
        assert_eq!(tile(Phase::Finished), opaque(GREEN));
    }

    #[test]
    fn has_a_white_dial_and_a_leaf() {
        let img = render(48, Phase::Idle, 1.0);
        // Dial rim at 9 o'clock: center (24, 26.9), radius 18.2.
        assert_eq!(pixel(&img, 48, 7, 26), opaque(DIAL));
        assert_eq!(pixel(&img, 48, 24, 6), opaque(LEAF));
    }

    #[test]
    fn pie_shrinks_clockwise_from_twelve() {
        // A quarter left: only the top-right quadrant of the dial is filled.
        let img = render(48, Phase::Running, 0.25);
        assert_eq!(pixel(&img, 48, 29, 21), opaque(PIE_RED), "top right");
        assert_eq!(pixel(&img, 48, 29, 32), opaque(DIAL), "bottom right");
        assert_eq!(pixel(&img, 48, 19, 32), opaque(DIAL), "bottom left");
        assert_eq!(pixel(&img, 48, 19, 21), opaque(DIAL), "top left");
    }

    #[test]
    fn paused_pie_is_amber_and_finished_shows_a_check() {
        let paused = render(48, Phase::Paused, 1.0);
        assert_eq!(pixel(&paused, 48, 26, 28), opaque(PIE_AMBER));
        let done = render(48, Phase::Finished, 0.0);
        assert!(count(&done, CHECK) > 60);
        assert_eq!(count(&done, PIE_RED), 0);
    }

    #[test]
    fn empty_and_full_pie() {
        assert_eq!(count(&render(22, Phase::Running, 0.0), PIE_RED), 0);
        assert!(count(&render(22, Phase::Running, 1.0), PIE_RED) > 80);
        assert_eq!(
            render(22, Phase::Running, 7.0),
            render(22, Phase::Running, 1.0),
            "clamped"
        );
    }

    #[test]
    fn edges_are_antialiased() {
        let img = render(32, Phase::Idle, 1.0);
        assert!(img.chunks(4).any(|p| p[0] > 0 && p[0] < 255));
    }
}
