//! Texture importer (`fynkel.image`, blueprint §19.2c item 11): PNG and
//! JPEG → RGBA8 with a declared colour space. Only those two formats are
//! compiled in; anything else is `FYN-A0103`. Grey, grey+alpha, RGB, RGBA,
//! palette and 16-bit sources all become 8-bit RGBA (16-bit is narrowed;
//! HDR inputs are a later item). No resizing, no mipmaps, no flipping:
//! rows are stored top first, as the file has them — M3 decides what the
//! GPU upload does with that.
//!
//! Sidecar settings: `colorSpace` (`srgb` | `linear`, default `srgb`).

use std::io::Cursor;
use std::path::Path;

use fynkel_contracts::{Code, Diagnostic};
use fynkel_document::asset::Meta;

use crate::intermediate::{ColorSpace, Intermediate, TextureData};
use crate::{read_source, Imported, Limits};

pub const IMPORTER: &str = fynkel_document::asset::IMAGE_IMPORTER;
pub const IMPORTER_VERSION: u32 = 1;

/// Import settings read from the sidecar's `settings`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub color_space: ColorSpace,
}

impl Settings {
    /// Reads the settings; a bad value is an error (the sidecar is the
    /// user's, and silently reading a normal map as sRGB is the classic
    /// invisible bug).
    pub fn from_meta(meta: &Meta) -> Result<Settings, Diagnostic> {
        let color_space = match meta.settings.get("colorSpace") {
            None => ColorSpace::Srgb,
            Some(n) => match n.as_str().and_then(ColorSpace::parse) {
                Some(c) => c,
                None => {
                    return Err(Diagnostic::error(
                        Code::IMPORT_INVALID_DATA,
                        "settings.colorSpace must be \"srgb\" or \"linear\"",
                    )
                    .with_pointer("/settings/colorSpace"))
                }
            },
        };
        Ok(Settings { color_space })
    }
}

pub fn import(path: &Path, meta: &Meta, limits: &Limits) -> Imported {
    let shown = path.display().to_string();
    let fail = |d: Diagnostic| Imported {
        intermediate: None,
        sub_assets: meta.sub_assets.clone(),
        diagnostics: vec![if d.document.is_none() {
            d.with_document(shown.clone())
        } else {
            d
        }],
    };
    let settings = match Settings::from_meta(meta) {
        Ok(s) => s,
        Err(d) => {
            return fail(
                d.with_document(
                    fynkel_document::asset::sidecar_path(path)
                        .display()
                        .to_string(),
                ),
            )
        }
    };
    let bytes = match read_source(path, limits) {
        Ok(b) => b,
        Err(d) => return fail(d),
    };
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let texture = match decode(&bytes, &name, settings.color_space, limits) {
        Ok(t) => t,
        Err(d) => return fail(d),
    };
    let mut intermediate = Intermediate::empty(
        IMPORTER,
        IMPORTER_VERSION,
        &path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    intermediate.textures.push(texture);
    let mut diagnostics = Vec::new();
    if !meta.sub_assets.is_empty() {
        diagnostics.push(
            Diagnostic::warning(
                Code::IMPORT_SKIPPED,
                format!(
                    "sidecar lists {} sub-asset(s); a texture has none (references to them will not resolve)",
                    meta.sub_assets.len()
                ),
            )
            .with_document(shown),
        );
    }
    Imported {
        intermediate: Some(intermediate),
        sub_assets: Vec::new(),
        diagnostics,
    }
}

/// Decodes PNG/JPEG bytes to a texture. Shared with the glTF path, whose
/// embedded images go through the same limits and normalization.
pub fn decode(
    bytes: &[u8],
    name: &str,
    color_space: ColorSpace,
    limits: &Limits,
) -> Result<TextureData, Diagnostic> {
    if bytes.len() as u64 > limits.max_image_bytes {
        return Err(Diagnostic::error(
            Code::IMPORT_LIMIT,
            format!(
                "{} bytes; the limit for one image is {}",
                bytes.len(),
                limits.max_image_bytes
            ),
        ));
    }
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Diagnostic::error(Code::IMPORT_UNREADABLE, format!("cannot read: {e}")))?;
    let Some(format) = reader.format() else {
        return Err(Diagnostic::error(
            Code::IMPORT_UNSUPPORTED,
            "not a PNG or JPEG (only those two image formats are imported)",
        ));
    };
    let mut reader = reader;
    let mut image_limits = image::Limits::default();
    image_limits.max_image_width = Some(limits.max_texture_dimension);
    image_limits.max_image_height = Some(limits.max_texture_dimension);
    // RGBA8 output plus the decoder's own working set; the decoders
    // count their allocations against this.
    image_limits.max_alloc = Some(
        (limits.max_texture_dimension as u64 * limits.max_texture_dimension as u64 * 4)
            .max(1 << 20)
            * 2,
    );
    reader.limits(image_limits);
    let decoded = reader.decode().map_err(|e| match e {
        image::ImageError::Limits(l) => Diagnostic::error(Code::IMPORT_LIMIT, format!("{l}")),
        image::ImageError::Unsupported(u) => {
            Diagnostic::error(Code::IMPORT_UNSUPPORTED, format!("{u}"))
        }
        other => Diagnostic::error(
            Code::IMPORT_INVALID_DATA,
            format!("{}: {other}", format.extensions_str()[0]),
        ),
    })?;
    let (width, height) = (decoded.width(), decoded.height());
    if width == 0 || height == 0 {
        return Err(Diagnostic::error(
            Code::IMPORT_INVALID_DATA,
            "the image has no pixels",
        ));
    }
    if width > limits.max_texture_dimension || height > limits.max_texture_dimension {
        return Err(Diagnostic::error(
            Code::IMPORT_LIMIT,
            format!(
                "{width}x{height}; the limit is {} per side",
                limits.max_texture_dimension
            ),
        ));
    }
    let source_has_alpha = decoded.color().has_alpha();
    let rgba8 = decoded.into_rgba8().into_raw();
    Ok(TextureData {
        sub_id: None,
        name: name.to_string(),
        width,
        height,
        color_space,
        source_has_alpha,
        rgba8,
    })
}
