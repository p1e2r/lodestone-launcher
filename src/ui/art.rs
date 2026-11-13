//! Procedural pixel-art banners: a little blocky landscape per instance
//! (sky, sun/moon, clouds, hills, terrain, water, trees). Deterministic from a
//! seed string, rendered once and cached as a PNG. All art is generated here.

use std::path::PathBuf;

use image::{Rgb, RgbImage};

use crate::core::paths::paths;

pub const W: u32 = 960;
pub const H: u32 = 320;
const B: u32 = 16; // block size in pixels
const T: u32 = 4; // texel size (4×4 texels per block)
const VERSION: u32 = 5; // bump to regenerate cached banners

struct Rng(u64);
impl Rng {
    fn new(seed: &str) -> Rng {
        let h = crate::core::net::sha1_bytes(seed.as_bytes());
        let v = u64::from_str_radix(&h[..16], 16).unwrap_or(0x9e3779b97f4a7c15);
        Rng(v | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn f(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % ((hi - lo + 1) as u64)) as i32
    }
}

#[derive(Clone, Copy)]
enum Sky {
    Day,
    Sunset,
    Night,
    Dawn,
}

#[derive(Clone, Copy, PartialEq)]
enum Biome {
    Plains,
    Desert,
    Snowy,
    Mesa,
    Cherry,
}

fn hex(c: u32) -> [f32; 3] {
    [((c >> 16) & 255) as f32, ((c >> 8) & 255) as f32, (c & 255) as f32]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn shade(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn px(c: [f32; 3]) -> Rgb<u8> {
    Rgb([c[0].clamp(0., 255.) as u8, c[1].clamp(0., 255.) as u8, c[2].clamp(0., 255.) as u8])
}

struct Canvas {
    img: RgbImage,
}

impl Canvas {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [f32; 3]) {
        for yy in y.max(0)..(y + h).min(H as i32) {
            for xx in x.max(0)..(x + w).min(W as i32) {
                self.img.put_pixel(xx as u32, yy as u32, px(c));
            }
        }
    }
    fn blend_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [f32; 3], a: f32) {
        for yy in y.max(0)..(y + h).min(H as i32) {
            for xx in x.max(0)..(x + w).min(W as i32) {
                let p = self.img.get_pixel(xx as u32, yy as u32).0;
                let base = [p[0] as f32, p[1] as f32, p[2] as f32];
                self.img.put_pixel(xx as u32, yy as u32, px(mix(base, c, a)));
            }
        }
    }
    /// One textured block at block coordinates.
    fn block(&mut self, bx: i32, by: i32, c: [f32; 3], rng: &mut Rng, jitter: f32) {
        for ty in 0..(B / T) as i32 {
            for tx in 0..(B / T) as i32 {
                let k = 1.0 + (rng.f() - 0.5) * jitter;
                self.rect(bx * B as i32 + tx * T as i32, by * B as i32 + ty * T as i32, T as i32, T as i32, shade(c, k));
            }
        }
    }
    /// Grass-like block: colored top texel rows over a side color.
    fn capped_block(&mut self, bx: i32, by: i32, top: [f32; 3], side: [f32; 3], rng: &mut Rng) {
        for ty in 0..(B / T) as i32 {
            for tx in 0..(B / T) as i32 {
                let drip = ty == 1 && rng.f() < 0.45;
                let c = if ty == 0 || drip { top } else { side };
                let k = 1.0 + (rng.f() - 0.5) * 0.16;
                self.rect(bx * B as i32 + tx * T as i32, by * B as i32 + ty * T as i32, T as i32, T as i32, shade(c, k));
            }
        }
    }
}

fn heightmap(rng: &mut Rng, cols: i32, base: f32, amp: f32, freq: f32) -> Vec<i32> {
    let (p1, p2, p3) = (rng.f() * 6.28, rng.f() * 6.28, rng.f() * 6.28);
    (0..cols)
        .map(|x| {
            let x = x as f32;
            let v = (x * freq + p1).sin() * amp + (x * freq * 2.3 + p2).sin() * amp * 0.45 + (x * freq * 5.1 + p3).sin() * amp * 0.18;
            (base + v).round() as i32
        })
        .collect()
}

/// Render the scene for `seed` and return the cached PNG path.
pub fn banner(seed: &str) -> PathBuf {
    let path = paths().cache().join("banners").join(format!("{}-v{VERSION}.png", &crate::core::net::sha1_bytes(seed.as_bytes())[..16]));
    if path.exists() {
        return path;
    }
    // Save at 2× with nearest-neighbour so pixels stay crisp when scaled.
    let img = image::imageops::resize(&render(seed), W * 2, H * 2, image::imageops::FilterType::Nearest);
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = img.save(&path);
    path
}

fn render(seed: &str) -> RgbImage {
    let mut rng = Rng::new(seed);
    let sky = [Sky::Day, Sky::Day, Sky::Sunset, Sky::Night, Sky::Dawn][rng.range(0, 4) as usize];
    let biome = [Biome::Plains, Biome::Plains, Biome::Desert, Biome::Snowy, Biome::Mesa, Biome::Cherry][rng.range(0, 5) as usize];
    let mut cv = Canvas { img: RgbImage::new(W, H) };
    let cols = (W / B) as i32;
    let rows = (H / B) as i32;

    // --- Sky gradient
    let (top, bottom) = match sky {
        Sky::Day => (hex(0x4f97f0), hex(0xbfe0ff)),
        Sky::Sunset => (hex(0x2a1c55), hex(0xff9d5c)),
        Sky::Night => (hex(0x050817), hex(0x1c2a5a)),
        Sky::Dawn => (hex(0x7aa8e6), hex(0xf8c9a8)),
    };
    for y in 0..H {
        // Banded (stepped) gradient for a pixel-art look.
        let band = (y / 8) as f32 * 8.0 / H as f32;
        let mut c = mix(top, bottom, band.powf(0.9));
        if let Sky::Sunset = sky {
            if band > 0.45 {
                c = mix(c, hex(0xe0577b), ((0.75 - band).abs() * -2.5 + 0.6).clamp(0.0, 0.5));
            }
        }
        for x in 0..W {
            cv.img.put_pixel(x, y, px(c));
        }
    }

    // --- Stars
    if let Sky::Night = sky {
        for _ in 0..90 {
            let (x, y) = (rng.range(0, W as i32), rng.range(0, (H as f32 * 0.55) as i32));
            let s = if rng.f() < 0.15 { 4 } else { 2 };
            cv.rect(x, y, s, s, hex(if rng.f() < 0.2 { 0xfff3c4 } else { 0xdfe7ff }));
        }
    }

    // --- Sun / moon
    let (sx, sy) = (rng.range(4, cols - 8), rng.range(1, 4));
    match sky {
        Sky::Night => {
            cv.blend_rect(sx * B as i32 - 12, sy * B as i32 - 12, 4 * B as i32 + 24, 4 * B as i32 + 24, hex(0x9fb4ff), 0.10);
            cv.rect(sx * B as i32, sy * B as i32, 4 * B as i32, 4 * B as i32, hex(0xe9eef8));
            for (dx, dy) in [(1, 1), (2, 2), (1, 3)] {
                cv.rect(sx * B as i32 + dx * B as i32, sy * B as i32 + dy * B as i32 - 8, 10, 10, hex(0xc3cbe0));
            }
        }
        _ => {
            let core = match sky {
                Sky::Sunset => hex(0xffcf6b),
                Sky::Dawn => hex(0xffe9a8),
                _ => hex(0xfff2a3),
            };
            cv.blend_rect(sx * B as i32 - 20, sy * B as i32 - 20, 4 * B as i32 + 40, 4 * B as i32 + 40, core, 0.18);
            cv.blend_rect(sx * B as i32 - 8, sy * B as i32 - 8, 4 * B as i32 + 16, 4 * B as i32 + 16, core, 0.30);
            cv.rect(sx * B as i32, sy * B as i32, 4 * B as i32, 4 * B as i32, core);
        }
    }

    // --- Clouds
    let cloud = match sky {
        Sky::Night => (hex(0x3a4670), 0.55),
        Sky::Sunset => (hex(0xffc7b0), 0.75),
        _ => (hex(0xffffff), 0.85),
    };
    for _ in 0..rng.range(3, 6) {
        let (cx, cy) = (rng.range(-4, cols), rng.range(0, 5));
        let len = rng.range(4, 10);
        cv.blend_rect(cx * B as i32, cy * B as i32 + 6, len * B as i32, B as i32, cloud.0, cloud.1);
        cv.blend_rect((cx + 1) * B as i32, cy * B as i32 - 10, (len - 3).max(2) * B as i32, B as i32, cloud.0, cloud.1);
    }

    // --- Far hills (atmospheric, flat)
    let far_col = match biome {
        Biome::Desert | Biome::Mesa => hex(0xc98f5a),
        Biome::Snowy => hex(0xdfe8f2),
        Biome::Cherry => hex(0x7fae6a),
        Biome::Plains => hex(0x4d8f4a),
    };
    let far_col = mix(far_col, bottom, 0.55);
    let far = heightmap(&mut rng, cols, rows as f32 * 0.48, 2.2, 0.21);
    for x in 0..cols {
        for y in far[x as usize]..rows {
            cv.block(x, y, far_col, &mut rng, 0.05);
        }
    }

    // --- Palette
    let (topc, sub, deep) = match biome {
        Biome::Plains | Biome::Cherry => (hex(0x5daa36), hex(0x7a5432), hex(0x7f7f82)),
        Biome::Desert => (hex(0xe3d19a), hex(0xd4bf83), hex(0xbfa66a)),
        Biome::Snowy => (hex(0xf3f7fb), hex(0x7a5432), hex(0x7f7f82)),
        Biome::Mesa => (hex(0xc96a3a), hex(0xb35b32), hex(0x8f4a35)),
    };
    let mesa_bands = [hex(0xc98a4b), hex(0xb35b32), hex(0xd8a16a), hex(0x8f4a35), hex(0xe2c09a)];
    let sea = (rows as f32 * 0.74) as i32;
    let near = heightmap(&mut rng, cols, rows as f32 * 0.68, 2.6, 0.17);

    // --- Terrain columns
    for x in 0..cols {
        let t = near[x as usize].clamp(5, rows - 2);
        for y in t..rows {
            let depth = y - t;
            if depth == 0 {
                match biome {
                    Biome::Plains | Biome::Cherry => cv.capped_block(x, y, topc, sub, &mut rng),
                    Biome::Snowy => cv.capped_block(x, y, topc, sub, &mut rng),
                    _ => cv.block(x, y, topc, &mut rng, 0.12),
                }
            } else if depth <= 3 {
                let c = if biome == Biome::Mesa { mesa_bands[(y as usize) % mesa_bands.len()] } else { sub };
                cv.block(x, y, c, &mut rng, 0.18);
            } else {
                let c = if biome == Biome::Mesa { mesa_bands[(y as usize + 2) % mesa_bands.len()] } else { deep };
                cv.block(x, y, c, &mut rng, 0.2);
                // Ores: coal specks, and the occasional green ore.
                if biome != Biome::Mesa && rng.f() < 0.09 {
                    let ore = if rng.f() < 0.25 { hex(0x3ddc84) } else { hex(0x2b2b2e) };
                    let (ox, oy) = (x * B as i32 + rng.range(2, 9), y * B as i32 + rng.range(2, 9));
                    cv.rect(ox, oy, 4, 4, ore);
                    cv.rect(ox + 4, oy + 4, 4, 4, ore);
                }
            }
        }
        // Water fills low areas.
        if t > sea {
            for y in sea..t {
                let water = if let Sky::Night = sky { hex(0x1f3a7a) } else { hex(0x3f76e4) };
                cv.block(x, y, water, &mut rng, 0.06);
                if y == sea {
                    cv.blend_rect(x * B as i32, y * B as i32, B as i32, 3, hex(0xffffff), 0.25);
                }
            }
        }
    }

    // --- Vegetation
    let mut x = rng.range(1, 4);
    while x < cols - 2 {
        let t = near[x as usize].clamp(5, rows - 2);
        if t <= sea {
            match biome {
                Biome::Plains | Biome::Cherry | Biome::Snowy if rng.f() < 0.55 => {
                    let h = rng.range(3, 4);
                    for i in 1..=h {
                        cv.block(x, t - i, hex(0x6b4f2a), &mut rng, 0.2);
                    }
                    let leaves = match biome {
                        Biome::Cherry => hex(0xf0a6c8),
                        Biome::Snowy => hex(0x2f6b3a),
                        _ => hex(0x3e8a2e),
                    };
                    let top = t - h - 1;
                    for (dx, dy) in [(-2, 0), (-1, 0), (0, 0), (1, 0), (2, 0), (-1, -1), (0, -1), (1, -1), (-1, 1), (0, 1), (1, 1), (0, -2)] {
                        if !(dy == 1 && dx == 0) {
                            cv.block(x + dx, top + dy, leaves, &mut rng, 0.28);
                        }
                    }
                    if biome == Biome::Snowy {
                        for dx in -1..=1 {
                            cv.rect((x + dx) * B as i32, (top - 2 + if dx == 0 { 0 } else { 1 }) * B as i32, B as i32, 5, hex(0xf3f7fb));
                        }
                    }
                    x += rng.range(4, 7);
                    continue;
                }
                Biome::Desert if rng.f() < 0.35 => {
                    for i in 1..=rng.range(2, 3) {
                        cv.block(x, t - i, hex(0x4f8a3c), &mut rng, 0.15);
                    }
                }
                _ => {
                    if rng.f() < 0.3 {
                        let flower = [hex(0xe84a5f), hex(0xf7d154), hex(0x9b7bdc)][rng.range(0, 2) as usize];
                        let (fx, fy) = (x * B as i32 + 6, (t - 1) * B as i32 + 6);
                        cv.rect(fx + 1, fy + 4, 2, 6, hex(0x3e8a2e));
                        cv.rect(fx, fy, 4, 4, flower);
                    }
                }
            }
        }
        x += rng.range(2, 4);
    }
    cv.img
}

/// The Lodestone logo: a chiseled stone block with a glowing green core.
pub fn logo() -> PathBuf {
    let path = paths().cache().join("banners").join(format!("logo-v{VERSION}.png"));
    if path.exists() {
        return path;
    }
    let n = 16u32;
    let scale = 8u32;
    let mut img = RgbImage::new(n * scale, n * scale);
    let mut rng = Rng::new("lodestone-logo");
    for y in 0..n {
        for x in 0..n {
            let edge = x == 0 || y == 0 || x == n - 1 || y == n - 1;
            let ring = (x == 3 || x == 12) && (3..=12).contains(&y) || (y == 3 || y == 12) && (3..=12).contains(&x);
            let core = (6..=9).contains(&x) && (6..=9).contains(&y);
            let core_glow = (5..=10).contains(&x) && (5..=10).contains(&y);
            let c = if core {
                if (x + y) % 3 == 0 { hex(0x8ef5b9) } else { hex(0x3ddc84) }
            } else if core_glow {
                hex(0x1f6b44)
            } else if ring {
                hex(0x2f3542)
            } else if edge {
                hex(0x3a404c)
            } else {
                shade(hex(0x59606e), 0.9 + rng.f() * 0.2)
            };
            for yy in 0..scale {
                for xx in 0..scale {
                    img.put_pixel(x * scale + xx, y * scale + yy, px(c));
                }
            }
        }
    }
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = img.save(&path);
    path
}

/// Decorative strip for the bottom of the sidebar: stars fading into a
/// blocky hillside (grass, dirt, stone with ores), a tree and flowers.
/// Transparent at the top so it blends into the sidebar.
pub fn sidebar_scene() -> PathBuf {
    let path = paths().cache().join("banners").join(format!("sidebar-v{VERSION}.png"));
    if path.exists() {
        return path;
    }
    const SW: u32 = 232;
    const SH: u32 = 240;
    const BS: i32 = 8; // block
    const TS: i32 = 2; // texel
    let mut img = image::RgbaImage::new(SW, SH);
    let mut rng = Rng::new("lodestone-sidebar");
    let put = |img: &mut image::RgbaImage, x: i32, y: i32, w: i32, h: i32, c: [f32; 3], a: u8| {
        for yy in y.max(0)..(y + h).min(SH as i32) {
            for xx in x.max(0)..(x + w).min(SW as i32) {
                let p = px(c).0;
                img.put_pixel(xx as u32, yy as u32, image::Rgba([p[0], p[1], p[2], a]));
            }
        }
    };
    let block = |img: &mut image::RgbaImage, rng: &mut Rng, bx: i32, by: i32, c: [f32; 3], top: Option<[f32; 3]>| {
        for ty in 0..BS / TS {
            for tx in 0..BS / TS {
                let base = match top {
                    Some(t) if ty == 0 || (ty == 1 && rng.f() < 0.4) => t,
                    _ => c,
                };
                let k = 0.88 + rng.f() * 0.22;
                put(img, bx * BS + tx * TS, by * BS + ty * TS, TS, TS, shade(base, k), 255);
            }
        }
    };
    let cols = SW as i32 / BS + 1;
    let rows = SH as i32 / BS;
    // Stars in the upper part, fading out towards the top.
    for _ in 0..26 {
        let (x, y) = (rng.range(0, SW as i32), rng.range(6, (SH as f32 * 0.42) as i32));
        let a = (50.0 + y as f32 * 1.8).min(220.0) as u8;
        let s = if rng.f() < 0.2 { 3 } else { 2 };
        put(&mut img, x, y, s, s, hex(if rng.f() < 0.3 { 0xfff3c4 } else { 0xcfd8ff }), a);
    }
    // Distant hill silhouette.
    let far = heightmap(&mut rng, cols, rows as f32 * 0.42, 1.6, 0.35);
    for x in 0..cols {
        for y in far[x as usize]..rows {
            put(&mut img, x * BS, y * BS, BS, BS, hex(0x1c2230), 255);
        }
    }
    // Near terrain.
    let near = heightmap(&mut rng, cols, rows as f32 * 0.55, 1.4, 0.42);
    for x in 0..cols {
        let t = near[x as usize];
        for y in t..rows {
            let d = y - t;
            if d == 0 {
                block(&mut img, &mut rng, x, y, hex(0x6b4a2c), Some(hex(0x4f9a33)));
            } else if d <= 2 {
                block(&mut img, &mut rng, x, y, hex(0x6b4a2c), None);
            } else {
                block(&mut img, &mut rng, x, y, hex(0x5a5f68), None);
                if rng.f() < 0.12 {
                    let ore = if rng.f() < 0.4 { hex(0x3ddc84) } else { hex(0x24262b) };
                    put(&mut img, x * BS + 2, y * BS + 2, 2, 2, ore, 255);
                    put(&mut img, x * BS + 4, y * BS + 4, 2, 2, ore, 255);
                }
            }
        }
    }
    // A tree and some flowers.
    let tx = rng.range(cols / 2, cols - 4);
    let tt = near[tx as usize];
    for i in 1..=3 {
        block(&mut img, &mut rng, tx, tt - i, hex(0x6b4f2a), None);
    }
    for (dx, dy) in [(-1, -4), (0, -4), (1, -4), (-2, -5), (-1, -5), (0, -5), (1, -5), (2, -5), (-1, -6), (0, -6), (1, -6), (0, -7)] {
        block(&mut img, &mut rng, tx + dx, tt + dy, hex(0x3e8a2e), None);
    }
    for _ in 0..5 {
        let fx = rng.range(1, cols - 2);
        if (fx - tx).abs() < 3 {
            continue;
        }
        let ft = near[fx as usize];
        let col = [hex(0xe84a5f), hex(0xf7d154), hex(0x9b7bdc)][rng.range(0, 2) as usize];
        put(&mut img, fx * BS + 3, ft * BS - 5, 2, 5, hex(0x3e8a2e), 255);
        put(&mut img, fx * BS + 2, ft * BS - 8, 4, 3, col, 255);
    }
    // Torch glow on the left.
    let gx = rng.range(1, cols / 3);
    let gt = near[gx as usize];
    put(&mut img, gx * BS + 3, gt * BS - 9, 2, 9, hex(0x7a5a33), 255);
    put(&mut img, gx * BS + 2, gt * BS - 12, 4, 4, hex(0xffc94a), 255);
    for r in [10, 6] {
        for yy in -r..=r {
            for xx in -r..=r {
                let (x, y) = (gx * BS + 4 + xx, gt * BS - 10 + yy);
                if x < 0 || y < 0 || x >= SW as i32 || y >= SH as i32 || xx * xx + yy * yy > r * r {
                    continue;
                }
                let p = img.get_pixel(x as u32, y as u32).0;
                if p[3] == 0 {
                    img.put_pixel(x as u32, y as u32, image::Rgba([255, 200, 90, 18]));
                }
            }
        }
    }
    let img = image::imageops::resize(&img, SW * 2, SH * 2, image::imageops::FilterType::Nearest);
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = img.save(&path);
    path
}

/// The 16×16 Lodestone block as RGB cells.
fn logo_cells() -> Vec<[f32; 3]> {
    let n = 16u32;
    let mut rng = Rng::new("lodestone-logo");
    let mut out = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let edge = x == 0 || y == 0 || x == n - 1 || y == n - 1;
            let ring = (x == 3 || x == 12) && (3..=12).contains(&y) || (y == 3 || y == 12) && (3..=12).contains(&x);
            let core = (6..=9).contains(&x) && (6..=9).contains(&y);
            let core_glow = (5..=10).contains(&x) && (5..=10).contains(&y);
            out.push(if core {
                if (x + y) % 3 == 0 { hex(0x8ef5b9) } else { hex(0x3ddc84) }
            } else if core_glow {
                hex(0x1f6b44)
            } else if ring {
                hex(0x2f3542)
            } else if edge {
                hex(0x3a404c)
            } else {
                shade(hex(0x59606e), 0.9 + rng.f() * 0.2)
            });
        }
    }
    out
}

/// App icon: the block with a hard pixel shadow on a transparent square canvas.
pub fn icon_rgba(size: u32) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(size, size);
    let cells = logo_cells();
    let margin = (size as f32 * 0.1).round() as u32;
    let cell = ((size - margin * 2) as f32 / 16.0).floor().max(1.0) as u32;
    let block = cell * 16;
    let off = (size - block) / 2;
    let shadow = (cell / 2).max(1);
    for y in 0..block {
        for x in 0..block {
            let (sx, sy) = (off + x + shadow, off + y + shadow);
            if sx < size && sy < size {
                img.put_pixel(sx, sy, image::Rgba([0, 0, 0, 120]));
            }
        }
    }
    for y in 0..block {
        for x in 0..block {
            let c = cells[((y / cell) * 16 + x / cell) as usize];
            let p = px(c).0;
            img.put_pixel(off + x, off + y, image::Rgba([p[0], p[1], p[2], 255]));
        }
    }
    img
}

/// Write `icon.png` (1024²) and a multi-size `icon.ico` into `dir`.
pub fn write_icons(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    icon_rgba(1024).save(dir.join("icon.png")).map_err(std::io::Error::other)?;
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let mut pngs = Vec::new();
    for s in sizes {
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(icon_rgba(s))
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .map_err(std::io::Error::other)?;
        pngs.push((s, buf));
    }
    // ICO container with PNG-compressed entries.
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (s, data) in &pngs {
        let dim = if *s >= 256 { 0u8 } else { *s as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &pngs {
        ico.extend_from_slice(data);
    }
    std::fs::write(dir.join("icon.ico"), ico)
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_deterministically() {
        let a = super::render("abc");
        let b = super::render("abc");
        assert_eq!(a.as_raw(), b.as_raw());
        assert_eq!(a.width(), super::W);
    }
}
