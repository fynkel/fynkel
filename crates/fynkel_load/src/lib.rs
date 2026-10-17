//! Turns bytes into plain values the engine can use (`docs/stage-2.md`
//! §2): PNG and JPEG images, Radiance `.hdr` skies, and `.glb` models.
//!
//! Nothing here touches the GPU or the world, or starts a thread: decoding
//! runs on the caller's thread, which for a game is a helper isolate.

use std::io::Cursor;

use image::{ImageFormat, ImageReader};

mod clip;
mod model;

pub use clip::{ChannelData, ClipData, Interpolation, Property, normalize, slerp};
pub use model::{MaterialData, ModelData, ModelError, NodeData, Part};

/// An image as 8-bit RGBA, straight (not premultiplied) alpha, rows from
/// the top. Colour channels are as the file stored them: whether they are
/// sRGB or linear is decided by the material slot that uses the image.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageData {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageData({} × {})", self.width, self.height)
    }
}

/// Why bytes or pixels couldn't become an [`ImageData`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageError {
    Empty,
    /// Not an image format at all.
    Unrecognized,
    /// An image format other than PNG and JPEG.
    Unsupported(&'static str),
    /// The file is a PNG or JPEG, but broken.
    Decode {
        format: &'static str,
        reason: String,
    },
    /// A sky that isn't a Radiance `.hdr`: the format it is.
    NotHdr(&'static str),
    TooBig {
        width: u32,
        height: u32,
    },
    NoPixels {
        width: u32,
        height: u32,
    },
    WrongLength {
        width: u32,
        height: u32,
        len: usize,
    },
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Empty => write!(f, "the image is empty (0 bytes)"),
            ImageError::Unrecognized => write!(f, "the image isn't a PNG or JPEG"),
            ImageError::Unsupported(format) => {
                write!(f, "the image is a {format}; only PNG and JPEG can be loaded")
            }
            ImageError::NotHdr(format) => write!(f, "the image is a {format}; a sky is a Radiance .hdr"),
            ImageError::Decode { format, reason } => write!(f, "the {format} couldn't be decoded: {reason}"),
            ImageError::TooBig { width, height } => write!(
                f,
                "the image is {width} × {height} pixels; textures can be at most {} on each side",
                ImageData::MAX_SIZE
            ),
            ImageError::NoPixels { width, height } => {
                write!(f, "the image is {width} × {height} pixels; it needs at least one")
            }
            ImageError::WrongLength { width, height, len } => write!(
                f,
                "a {width} × {height} image has {} bytes of RGBA, but there are {len}",
                *width as u64 * *height as u64 * 4
            ),
        }
    }
}

impl std::error::Error for ImageError {}

impl ImageData {
    /// The largest width and height: WebGPU's default limit, which every
    /// device the engine runs on meets.
    pub const MAX_SIZE: u32 = 8192;

    /// Decodes a PNG or JPEG. Any PNG layout (grey, palette, 16-bit)
    /// becomes 8-bit RGBA; a JPEG is opaque.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageError> {
        if bytes.is_empty() {
            return Err(ImageError::Empty);
        }
        let format = image::guess_format(bytes).map_err(|_| ImageError::Unrecognized)?;
        let name = match format {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
            other => return Err(ImageError::Unsupported(format_name(other))),
        };
        let decode_error = |e: image::ImageError| ImageError::Decode { format: name, reason: e.to_string() };
        // The size comes from the header, so an oversized image is refused
        // before its pixels are allocated.
        let (width, height) =
            ImageReader::with_format(Cursor::new(bytes), format).into_dimensions().map_err(decode_error)?;
        Self::check_size(width, height)?;
        let decoded = ImageReader::with_format(Cursor::new(bytes), format).decode().map_err(decode_error)?;
        let rgba = decoded.into_rgba8();
        Ok(Self { width, height, pixels: rgba.into_raw() })
    }

    /// An image from RGBA8 pixels, rows from the top.
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        Self::check_size(width, height)?;
        if pixels.len() as u64 != width as u64 * height as u64 * 4 {
            return Err(ImageError::WrongLength { width, height, len: pixels.len() });
        }
        Ok(Self { width, height, pixels })
    }

    fn check_size(width: u32, height: u32) -> Result<(), ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::NoPixels { width, height });
        }
        if width > Self::MAX_SIZE || height > Self::MAX_SIZE {
            return Err(ImageError::TooBig { width, height });
        }
        Ok(())
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// RGBA8, rows from the top, `width × height × 4` bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// A Radiance `.hdr` image as linear RGB, 32-bit floats, rows from the
/// top: light, which may be far brighter than 1, as a sky is.
#[derive(Clone, PartialEq)]
pub struct HdrData {
    width: u32,
    height: u32,
    rgb: Vec<f32>,
}

impl std::fmt::Debug for HdrData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HdrData({} × {})", self.width, self.height)
    }
}

impl HdrData {
    /// Decodes a Radiance `.hdr` (RGBE). Anything else is refused.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageError> {
        if bytes.is_empty() {
            return Err(ImageError::Empty);
        }
        let format = image::guess_format(bytes).map_err(|_| ImageError::Unrecognized)?;
        if format != ImageFormat::Hdr {
            return Err(ImageError::NotHdr(format_name(format)));
        }
        let decode_error = |e: image::ImageError| ImageError::Decode { format: "Radiance HDR", reason: e.to_string() };
        // As ImageData::decode: the size from the header first.
        let (width, height) =
            ImageReader::with_format(Cursor::new(bytes), format).into_dimensions().map_err(decode_error)?;
        ImageData::check_size(width, height)?;
        let decoded = ImageReader::with_format(Cursor::new(bytes), format).decode().map_err(decode_error)?;
        Ok(Self { width, height, rgb: decoded.into_rgb32f().into_raw() })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Linear RGB, rows from the top, `width × height × 3` floats.
    pub fn rgb(&self) -> &[f32] {
        &self.rgb
    }
}

fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Avif => "AVIF",
        ImageFormat::Hdr => "Radiance HDR",
        ImageFormat::OpenExr => "OpenEXR",
        ImageFormat::Ico => "ICO",
        ImageFormat::Tga => "TGA",
        ImageFormat::Dds => "DDS",
        ImageFormat::Qoi => "QOI",
        _ => "format other than these",
    }
}
