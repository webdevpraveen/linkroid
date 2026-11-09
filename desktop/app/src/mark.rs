// SPDX-License-Identifier: GPL-3.0-or-later
//! The Linkroid mark (`assets/brand/icon.png`): two interlocking device loops,
//! the Android mobile phone (vertical loop in Android Green) and the PC desktop
//! (horizontal loop in Electric Blue), interlocked around a central connection nexus,
//! on a deep obsidian slate tile with an electric cyan/blue glow. Drawn in code at any pixel
//! size: the tray, window and toast icons at runtime, and the executable's
//! icon at build time (`build.rs` includes this file), so every size is sharp
//! without bitmap assets.

/// Deep obsidian slate tile, its electric cyan glow, the cyan rim, the Android phone loop,
/// the PC loop, and the central connection nexus.
const TILE: [f32; 3] = rgb(0x0B, 0x13, 0x2B);
const GLOW: [f32; 3] = rgb(0x02, 0x84, 0xC7);
const RIM: [f32; 3] = rgb(0x38, 0xBD, 0xF8);
const PHONE: [f32; 3] = rgb(0x00, 0xE6, 0x76);
const PC: [f32; 3] = rgb(0x00, 0x91, 0xFF);
const LINKROID_ACCENT: [f32; 3] = rgb(0x38, 0xBD, 0xF8);

/// Supersampling grid per pixel (SS×SS samples) for smooth edges.
const SS: usize = 4;

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32, g as f32, b as f32]
}

/// Distance from (x, y) to the centerline of the vertical Phone rounded rectangle loop.
fn distance_to_phone(x: f32, y: f32) -> f32 {
    let cx = 37.0;
    let cy = 50.0;
    let hw = 15.0;
    let hh = 26.0;
    let r = 8.0;
    let dx = (x - cx).abs() - (hw - r);
    let dy = (y - cy).abs() - (hh - r);
    let qx = dx.max(0.0);
    let qy = dy.max(0.0);
    let dist_outside = (qx * qx + qy * qy).sqrt();
    let dist_inside = dx.max(dy).min(0.0);
    let sdf = dist_outside + dist_inside - r;
    sdf.abs()
}

/// Distance from (x, y) to the centerline of the horizontal PC desktop rounded rectangle loop.
fn distance_to_pc(x: f32, y: f32) -> f32 {
    let cx = 63.0;
    let cy = 50.0;
    let hw = 24.0;
    let hh = 15.0;
    let r = 6.0;
    let dx = (x - cx).abs() - (hw - r);
    let dy = (y - cy).abs() - (hh - r);
    let qx = dx.max(0.0);
    let qy = dy.max(0.0);
    let dist_outside = (qx * qx + qy * qy).sqrt();
    let dist_inside = dx.max(dy).min(0.0);
    let sdf = dist_outside + dist_inside - r;
    sdf.abs()
}

/// Sizes in the design grid: small sizes are drawn with slightly heavier stroke.
struct Geometry {
    inset: f32,
    corner: f32,
    stroke: f32,
    node_radius: f32,
}

impl Geometry {
    fn for_size(size: usize) -> Self {
        if size <= 32 {
            Geometry { inset: 2.0, corner: 20.0, stroke: 8.5, node_radius: 6.5 }
        } else {
            Geometry { inset: 4.0, corner: 22.0, stroke: 7.5, node_radius: 5.5 }
        }
    }
}

/// Renders the mark as straight-alpha RGBA, row by row, `size`×`size`.
pub fn render(size: usize) -> Vec<u8> {
    let g = Geometry::for_size(size);
    let unit = 100.0 / size as f32;
    // The rim is at least about a pixel wide, so the tile keeps an edge on dark taskbars.
    let rim = (1.5f32).max(unit * 0.6);

    let mut out = vec![0u8; size * size * 4];
    for py in 0..size {
        for px in 0..size {
            let mut color = [0f32; 3];
            let mut covered = 0usize;
            for sy in 0..SS {
                for sx in 0..SS {
                    // The sample in the design grid.
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) * unit;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) * unit;
                    let edge = rounded_square_distance(x, y, g.inset, g.corner);
                    if edge > 0.0 {
                        continue;
                    }
                    covered += 1;

                    let (dx, dy) = (x - 50.0, y - 50.0);
                    let dist_center = (dx * dx + dy * dy).sqrt();
                    let half = g.stroke / 2.0;

                    let d_phone = distance_to_phone(x, y);
                    let d_pc = distance_to_pc(x, y);

                    let in_phone = d_phone <= half;
                    let in_pc = d_pc <= half;

                    let paint = if dist_center <= g.node_radius {
                        LINKROID_ACCENT
                    } else if in_phone && in_pc {
                        // Interlocking weave: Phone on top above center, PC on top below
                        if y < 50.0 {
                            PHONE
                        } else {
                            PC
                        }
                    } else if in_phone {
                        PHONE
                    } else if in_pc {
                        PC
                    } else if -edge <= rim {
                        mix(TILE, RIM, 0.34)
                    } else {
                        // The glow fades from the middle to the tile color at 42 units.
                        let t = (dist_center / 42.0).min(1.0);
                        mix(GLOW, TILE, t)
                    };
                    for c in 0..3 {
                        color[c] += paint[c];
                    }
                }
            }
            if covered == 0 {
                continue;
            }
            let i = (py * size + px) * 4;
            for c in 0..3 {
                out[i + c] = (color[c] / covered as f32).round() as u8;
            }
            out[i + 3] = ((covered as f32 / (SS * SS) as f32) * 255.0).round() as u8;
        }
    }
    out
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// How far (x, y) is outside the rounded square from `inset` to
/// `100 - inset` with corner radius `r` (negative inside).
fn rounded_square_distance(x: f32, y: f32, inset: f32, r: f32) -> f32 {
    let (lo, hi) = (inset + r, 100.0 - inset - r);
    let qx = (x.clamp(lo, hi) - x).abs();
    let qy = (y.clamp(lo, hi) - y).abs();
    if x > lo && x < hi || y > lo && y < hi {
        // Along a straight side.
        let to_x = (x - inset).min(100.0 - inset - x);
        let to_y = (y - inset).min(100.0 - inset - y);
        return -to_x.min(to_y);
    }
    (qx * qx + qy * qy).sqrt() - r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * size + x) * 4;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn corners_are_transparent_and_the_drop_sits_in_the_middle() {
        let size = 64;
        let img = render(size);
        assert_eq!(img.len(), size * size * 4);
        assert_eq!(pixel(&img, size, 0, 0)[3], 0, "rounded corner");
        let middle = pixel(&img, size, 32, 32);
        assert_eq!(middle, [0x38, 0xBD, 0xF8, 255], "the center node");
        // Near the top edge, between the loops: the dark tile.
        let tile = pixel(&img, size, 32, 8);
        assert!(tile[0] < 0x80 && tile[3] == 255, "{tile:?}");
    }

    #[test]
    fn both_loops_are_drawn() {
        let size = 128;
        let img = render(size);
        let colored = |p: [u8; 4]| (p[1] > 0xC0 || p[2] > 0xC0) && p[3] == 255;
        // Somewhere left and right of the center node there's loop ink.
        let left = (8..56).any(|x| colored(pixel(&img, size, x, 64)) || colored(pixel(&img, size, x, 70)));
        let right = (72..120).any(|x| colored(pixel(&img, size, x, 64)) || colored(pixel(&img, size, x, 58)));
        assert!(left && right);
    }

    #[test]
    fn renders_every_tray_size() {
        for size in [16, 20, 24, 32, 48, 64, 256] {
            let img = render(size);
            assert!(img.chunks(4).any(|p| p[3] == 255), "{size}px has an opaque tile");
        }
    }
}
