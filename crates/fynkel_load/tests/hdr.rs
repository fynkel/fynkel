//! Decoding Radiance `.hdr` skies into linear RGB, and refusing what can't
//! be used.

use fynkel_load::{HdrData, ImageError};
use image::Rgb;
use image::codecs::hdr::HdrEncoder;

fn hdr(width: usize, height: usize, pixels: &[[f32; 3]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let rgb: Vec<Rgb<f32>> = pixels.iter().map(|p| Rgb(*p)).collect();
    HdrEncoder::new(&mut bytes).encode(&rgb, width, height).unwrap();
    bytes
}

#[test]
fn an_hdr_decodes_to_linear_rgb_top_row_first_brighter_than_white() {
    let pixels = [[1.0, 0.5, 0.25], [8.0, 16.0, 32.0], [0.0, 0.0, 0.0], [0.125, 0.125, 0.125]];
    let image = HdrData::decode(&hdr(2, 2, &pixels)).unwrap();
    assert_eq!((image.width(), image.height()), (2, 2));
    // RGBE keeps 8 bits of mantissa: these are exact.
    assert_eq!(image.rgb(), pixels.concat());
}

#[test]
fn what_isnt_an_hdr_is_refused_saying_why() {
    assert_eq!(HdrData::decode(&[]), Err(ImageError::Empty));
    let png = {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.write_header().unwrap().write_image_data(&[0, 0, 0, 255]).unwrap();
        bytes
    };
    let error = HdrData::decode(&png).unwrap_err();
    assert_eq!(error, ImageError::NotHdr("PNG"));
    assert_eq!(error.to_string(), "the image is a PNG; a sky is a Radiance .hdr");
    let mut broken = hdr(2, 2, &[[1.0; 3]; 4]);
    broken.truncate(broken.len() - 3);
    assert!(matches!(HdrData::decode(&broken), Err(ImageError::Decode { format: "Radiance HDR", .. })));
}
