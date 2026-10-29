//! Golden image tests: compares a rendered image with a reference PNG in
//! `tests/golden`, with a small tolerance for GPU differences.
//!
//! Only tests use this crate.

use std::path::{Path, PathBuf};

/// RGBA8 pixels, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].try_into().unwrap()
    }
}

// --- Golden images -----------------------------------------------------------

/// `tests/golden`, or `FYNKEL_GOLDEN_DIR` when the tests run on a device
/// or emulator, where the source tree isn't.
fn golden_dir() -> PathBuf {
    std::env::var_os("FYNKEL_GOLDEN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden"))
}

fn failure_dir() -> PathBuf {
    match std::env::var_os("FYNKEL_GOLDEN_DIR") {
        Some(dir) => PathBuf::from(dir).join("failures"),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/golden-failures"),
    }
}

pub fn write_png(path: &Path, image: &Image) {
    let file = std::fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&image.rgba).unwrap();
}

pub fn read_png(path: &Path) -> Option<Image> {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).ok()?));
    let mut reader = decoder.read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;
    assert_eq!(info.color_type, png::ColorType::Rgba, "{} is not RGBA", path.display());
    rgba.truncate(info.buffer_size());
    Some(Image { width: info.width, height: info.height, rgba })
}

/// Compares `image` with `tests/golden/<name>.png`. A channel may differ by
/// at most 2/255 (4/255 near black, see [`within`]), and at most 0.1 % of
/// pixels may be outside that.
///
/// `FYNKEL_BLESS=1` writes the reference instead. Do that only on purpose,
/// and look at the image before committing it. On a mismatch the actual
/// image and a diff are written to `target/golden-failures/`.
pub fn assert_golden(name: &str, image: &Image) {
    let pixels = (image.width * image.height) as usize;
    assert_golden_within(name, image, pixels / 1000);
}

/// How far a channel may be from the golden's: 2/255, or 4/255 where both
/// are near black, as a 10-bit scene image's step is there, and its dither
/// moves a pixel by up to one (fynkel_render's forward.wgsl, `fy_dither`).
pub fn within(a: u8, b: u8) -> u8 {
    if a.max(b) <= 48 { 4 } else { 2 }
}

/// [`assert_golden`], with at most `pixels` outside 2/255 instead of
/// 0.1 %: for a test whose subject is a few pixels wide, as a shadow's
/// edge is, which 0.1 % of the image would let move.
pub fn assert_golden_within(name: &str, image: &Image, pixels: usize) {
    let allowed = pixels;
    let path = golden_dir().join(format!("{name}.png"));
    if std::env::var("FYNKEL_BLESS").as_deref() == Ok("1") {
        std::fs::create_dir_all(golden_dir()).unwrap();
        write_png(&path, image);
        eprintln!("blessed {}", path.display());
        return;
    }
    let golden = read_png(&path)
        .unwrap_or_else(|| panic!("no golden image {} (run with FYNKEL_BLESS=1 to create it)", path.display()));
    assert_eq!((golden.width, golden.height), (image.width, image.height), "{name}: size differs from the golden");

    let mut diff = Image { width: image.width, height: image.height, rgba: vec![0; image.rgba.len()] };
    let (mut off, mut worst) = (0usize, 0u8);
    for (i, (a, b)) in image.rgba.chunks(4).zip(golden.rgba.chunks(4)).enumerate() {
        let d = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
        worst = worst.max(d);
        if a.iter().zip(b).any(|(x, y)| x.abs_diff(*y) > within(*x, *y)) {
            off += 1;
        }
        diff.rgba[i * 4..i * 4 + 4].copy_from_slice(&[d.saturating_mul(16), 0, 0, 255]);
    }
    let pixels = (image.width * image.height) as usize;
    if off > allowed {
        std::fs::create_dir_all(failure_dir()).unwrap();
        write_png(&failure_dir().join(format!("{name}.actual.png")), image);
        write_png(&failure_dir().join(format!("{name}.diff.png")), &diff);
        panic!(
            "{name}: {off} of {pixels} pixels differ by more than 2/255, 4 near black ({allowed} may; worst {worst}); see {}",
            failure_dir().display()
        );
    }
}
