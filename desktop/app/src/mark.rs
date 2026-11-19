// SPDX-License-Identifier: GPL-3.0-or-later
//! The Linkroid mark (`assets/brand/icon.png`): PC monitor and mobile phone
//! connected by an interlocking chain link and dynamic flow, symbolizing real-time
//! cross-device continuity. Rendered sharp at every pixel size: tray, window,
//! notifications, and the executable's icon at build time.

static MARK_DATA: &[u8] = include_bytes!("mark_data.bin");

const SIZES: &[(usize, usize, usize)] = &[
    (16, 0, 1024),
    (20, 1024, 1600),
    (24, 2624, 2304),
    (32, 4928, 4096),
    (40, 9024, 6400),
    (48, 15424, 9216),
    (64, 24640, 16384),
    (128, 41024, 65536),
    (256, 106560, 262144),
];

/// Returns a size * size RGBA byte buffer for the mark.
pub fn render(size: usize) -> Vec<u8> {
    for &(s, offset, len) in SIZES {
        if s == size {
            return MARK_DATA[offset..offset + len].to_vec();
        }
    }

    // For any custom non-standard size, resample from the 256x256 master
    let (_, offset_256, _) = SIZES[8];
    let src = &MARK_DATA[offset_256..offset_256 + 256 * 256 * 4];
    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        let src_y = (y * 256) / size;
        for x in 0..size {
            let src_x = (x * 256) / size;
            let src_idx = (src_y * 256 + src_x) * 4;
            let dst_idx = (y * size + x) * 4;
            out[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * size + x) * 4;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn corners_are_transparent_and_image_is_valid() {
        let size = 64;
        let img = render(size);
        assert_eq!(img.len(), size * size * 4);
        assert_eq!(pixel(&img, size, 0, 0)[3], 0, "corner is transparent");
        let center = pixel(&img, size, 32, 32);
        assert!(center[3] > 0, "center has ink");
    }

    #[test]
    fn both_sides_have_ink() {
        let size = 128;
        let img = render(size);
        let has_ink = |p: [u8; 4]| p[3] > 100;
        let left = (10..60).any(|x| has_ink(pixel(&img, size, x, 64)));
        let right = (68..118).any(|x| has_ink(pixel(&img, size, x, 64)));
        assert!(left && right, "both PC and Phone sides have ink");
    }

    #[test]
    fn renders_every_tray_size() {
        for size in [16, 20, 24, 32, 40, 48, 64, 128, 256] {
            let img = render(size);
            assert_eq!(img.len(), size * size * 4);
            assert!(img.chunks(4).any(|p| p[3] > 0), "{size}px has visible content");
        }
    }
}
