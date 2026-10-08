//! Decoding PNG and JPEG into RGBA8, and refusing what can't be used.

use fynkel_load::{ImageData, ImageError};

/// A PNG of `pixels` (`channels` per pixel, `depth` bits each).
fn png(width: u32, height: u32, color: png::ColorType, depth: png::BitDepth, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
    bytes
}

fn rgba_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    png(width, height, png::ColorType::Rgba, png::BitDepth::Eight, pixels)
}

#[test]
fn a_png_decodes_to_its_exact_pixels_top_row_first() {
    let pixels = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 10, 20, 30, 40];
    let image = ImageData::decode(&rgba_png(2, 2, &pixels)).unwrap();
    assert_eq!((image.width(), image.height()), (2, 2));
    assert_eq!(image.pixels(), pixels);
}

#[test]
fn other_png_layouts_become_rgba8() {
    // Grey with alpha.
    let image = ImageData::decode(&png(2, 1, png::ColorType::GrayscaleAlpha, png::BitDepth::Eight, &[50, 255, 200, 0]))
        .unwrap();
    assert_eq!(image.pixels(), [50, 50, 50, 255, 200, 200, 200, 0]);
    // RGB: opaque.
    let image = ImageData::decode(&png(1, 1, png::ColorType::Rgb, png::BitDepth::Eight, &[1, 2, 3])).unwrap();
    assert_eq!(image.pixels(), [1, 2, 3, 255]);
    // 16 bits per channel: the high byte, rounded.
    let image =
        ImageData::decode(&png(1, 1, png::ColorType::Rgb, png::BitDepth::Sixteen, &[0xFF, 0xFF, 0x80, 0x00, 0, 0]))
            .unwrap();
    assert_eq!(image.pixels(), [255, 128, 0, 255]);
}

#[test]
fn a_jpeg_decodes_close_to_its_colour() {
    let (width, height) = (16, 8);
    let rgb: Vec<u8> = (0..width * height).flat_map(|_| [200, 100, 50]).collect();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
        .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
    let image = ImageData::decode(&bytes).unwrap();
    assert_eq!((image.width(), image.height()), (width, height));
    for pixel in image.pixels().chunks(4) {
        for (got, want) in pixel.iter().zip([200u8, 100, 50, 255]) {
            assert!(got.abs_diff(want) <= 3, "pixel {pixel:?}, want about [200, 100, 50, 255]");
        }
    }
}

#[test]
fn what_cant_be_used_is_refused_with_a_clear_message() {
    let message = |bytes: &[u8]| ImageData::decode(bytes).unwrap_err().to_string();
    assert_eq!(message(&[]), "the image is empty (0 bytes)");
    assert_eq!(message(b"hello, world"), "the image isn't a PNG or JPEG");
    assert_eq!(message(b"GIF89a\x01\x00\x01\x00"), "the image is a GIF; only PNG and JPEG can be loaded");
    let truncated = &rgba_png(4, 4, &[7; 64])[..40];
    assert!(message(truncated).starts_with("the PNG couldn't be decoded: "), "{}", message(truncated));

    let wide = rgba_png(ImageData::MAX_SIZE + 1, 1, &vec![0; (ImageData::MAX_SIZE as usize + 1) * 4]);
    assert_eq!(ImageData::decode(&wide).unwrap_err(), ImageError::TooBig { width: ImageData::MAX_SIZE + 1, height: 1 });
    assert_eq!(message(&wide), "the image is 8193 × 1 pixels; textures can be at most 8192 on each side");
}

#[test]
fn pixels_from_code_are_checked() {
    let image = ImageData::from_rgba(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    assert_eq!(image.pixels(), [1, 2, 3, 4, 5, 6, 7, 8]);
    let message = |w, h, len| ImageData::from_rgba(w, h, vec![0; len]).unwrap_err().to_string();
    assert_eq!(message(2, 2, 15), "a 2 × 2 image has 16 bytes of RGBA, but there are 15");
    assert_eq!(message(0, 4, 0), "the image is 0 × 4 pixels; it needs at least one");
    assert_eq!(message(8193, 1, 8193 * 4), "the image is 8193 × 1 pixels; textures can be at most 8192 on each side");
}
