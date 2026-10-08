//! Times decoding images (stage 2 step 3): photo-like PNGs and JPEGs of
//! the sizes a mobile game uses, made here so the numbers don't depend on
//! files that aren't in the repository.
//!
//! `cargo run --release -p fynkel_load --example decode`
//!
//! Each line is the best of 5 runs.

use std::io::Cursor;
use std::time::{Duration, Instant};

use fynkel_load::ImageData;
use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, RgbaImage};

/// Smooth colour with noise in the low bits, like a photo or a painted
/// texture, so PNG can't compress it unrealistically well (about half
/// size, as photos go).
fn photo(size: u32) -> RgbaImage {
    let mut seed = 1u32;
    RgbaImage::from_fn(size, size, |x, y| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 28) as f32;
        let (u, v) = (x as f32 / size as f32, y as f32 / size as f32);
        let wave = |f: f32, p: f32| 127.0 + 100.0 * (f * u + p).sin() * (f * v - p).cos();
        let channel = |c: f32| (c + noise).clamp(0.0, 255.0) as u8;
        image::Rgba([channel(wave(9.0, 0.0)), channel(wave(13.0, 1.0)), channel(wave(7.0, 2.0)), 255])
    })
}

fn best(bytes: &[u8]) -> Duration {
    (0..5)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(ImageData::decode(bytes).unwrap());
            t.elapsed()
        })
        .min()
        .unwrap()
}

fn main() {
    for size in [512, 1024, 2048] {
        let image = photo(size);
        let mut png = Vec::new();
        image.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let mut jpeg = Vec::new();
        let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
        rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, 85)).unwrap();
        for (format, bytes) in [("PNG", &png), ("JPEG", &jpeg)] {
            let kib = bytes.len() / 1024;
            println!("{size}² {format:4} {kib:>6} KiB  {:.1} ms", best(bytes).as_secs_f64() * 1000.0);
        }
    }
}
