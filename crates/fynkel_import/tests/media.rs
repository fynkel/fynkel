//! The texture and audio importers (blueprint §19.2c item 11): every
//! source layout normalizes to RGBA8 / interleaved f32, the sidecar's
//! colour space is honoured, only PNG/JPEG and WAV/Ogg Vorbis are read,
//! limits hold, and the bytes are deterministic (committed for the
//! lossless sources).

use std::path::{Path, PathBuf};

use fynkel_contracts::Code;
use fynkel_document::asset::Meta;
use fynkel_document::{Document, DocumentKind, Node};
use fynkel_import::{import, ColorSpace, Imported, Intermediate, Limits};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn import_fixture(name: &str) -> Imported {
    let path = fixture(name);
    import(&path, &Meta::new(&path), &Limits::default())
}

fn codes(r: &Imported) -> Vec<Code> {
    r.diagnostics.iter().map(|d| d.code).collect()
}

fn meta_with_settings(path: &Path, settings: &str) -> Meta {
    let mut meta = Meta::new(path);
    let doc = Document::parse(
        DocumentKind::Meta,
        None,
        &format!(
            r#"{{"$schema":"https://fynkel.dev/schemas/meta.schema.json","formatVersion":1,"id":"{}","name":"x","importer":"{}","importerVersion":1,"settings":{settings},"subAssets":{{}}}}"#,
            meta.id(),
            meta.importer
        ),
    )
    .unwrap();
    meta.settings = doc
        .root
        .get("settings")
        .cloned()
        .unwrap_or_else(|| Node::object(Vec::new()));
    meta
}

/// Pins bytes: `FYNKEL_UPDATE_FIXTURES=1` rewrites the expectation.
fn assert_matches_expected(i: &Intermediate, expected: &str) {
    let bytes = i.to_bytes();
    let path = fixture(expected);
    if std::env::var_os("FYNKEL_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &bytes).unwrap();
    }
    let want = std::fs::read(&path)
        .unwrap_or_else(|_| panic!("run with FYNKEL_UPDATE_FIXTURES=1 to (re)create {expected}"));
    assert!(
        bytes == want,
        "{expected}: the import no longer produces the committed bytes ({} vs {}); rerun with FYNKEL_UPDATE_FIXTURES=1 if intended",
        bytes.len(),
        want.len()
    );
    assert_eq!(&Intermediate::from_bytes(&bytes).unwrap(), i);
}

// --- textures ---------------------------------------------------------------

#[test]
fn png_rgba_decodes_to_rgba8_top_row_first() {
    let r = import_fixture("textures/swatch.png");
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    assert!(r.diagnostics.is_empty());
    assert!(r.sub_assets.is_empty());
    let i = r.intermediate.unwrap();
    assert_eq!(i.importer, "fynkel.image");
    assert_eq!(i.source, "swatch.png");
    assert!(i.prefab.is_none());
    assert_eq!(i.textures.len(), 1);
    let t = &i.textures[0];
    assert_eq!(t.sub_id, None);
    assert_eq!(t.name, "swatch");
    assert_eq!((t.width, t.height), (4, 3));
    assert_eq!(t.color_space, ColorSpace::Srgb);
    assert!(t.source_has_alpha);
    assert_eq!(t.rgba8.len(), 4 * 3 * 4);
    // R = x*60, G = y*100, B = 200; (3,2) is transparent.
    let px = |x: usize, y: usize| &t.rgba8[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
    assert_eq!(px(0, 0), [0, 0, 200, 255]);
    assert_eq!(px(3, 0), [180, 0, 200, 255]);
    assert_eq!(px(1, 2), [60, 200, 200, 255]);
    assert_eq!(px(3, 2), [180, 200, 200, 0]);
    assert_matches_expected(&i, "textures/swatch.expected.fyni");
}

#[test]
fn every_png_layout_and_jpeg_normalize_to_rgba8() {
    let rgba = import_fixture("textures/swatch.png").intermediate.unwrap();
    let rgb = import_fixture("textures/swatch_rgb.png")
        .intermediate
        .unwrap();
    let palette = import_fixture("textures/swatch_palette.png")
        .intermediate
        .unwrap();
    let gray = import_fixture("textures/swatch_gray.png")
        .intermediate
        .unwrap();
    let jpeg = import_fixture("textures/swatch.jpg").intermediate.unwrap();
    let gray16 = import_fixture("textures/gray16.png").intermediate.unwrap();

    // RGB: same colours, opaque everywhere, no alpha reported.
    assert!(!rgb.textures[0].source_has_alpha);
    for (a, b) in rgba.textures[0]
        .rgba8
        .chunks(4)
        .zip(rgb.textures[0].rgba8.chunks(4))
    {
        assert_eq!(a[..3], b[..3]);
        assert_eq!(b[3], 255);
    }
    // Palette with transparency round-trips exactly.
    assert!(palette.textures[0].source_has_alpha);
    assert_eq!(palette.textures[0].rgba8, rgba.textures[0].rgba8);
    // Grey expands to R = G = B.
    for px in gray.textures[0].rgba8.chunks(4) {
        assert_eq!(px[0], px[1]);
        assert_eq!(px[1], px[2]);
        assert_eq!(px[3], 255);
    }
    // JPEG: lossy, but the shape and the gradient direction survive.
    let t = &jpeg.textures[0];
    assert_eq!((t.width, t.height), (4, 3));
    assert!(!t.source_has_alpha);
    assert!(t.rgba8[0] < t.rgba8[12], "red rises along x");
    // 16-bit grey narrows to 8 bits.
    assert_eq!(gray16.textures[0].rgba8[0..4], [0, 0, 0, 255]);
    assert_eq!(gray16.textures[0].rgba8[4..8], [128, 128, 128, 255]);
    assert_eq!(gray16.textures[0].rgba8[8..12], [255, 255, 255, 255]);
}

#[test]
fn colour_space_comes_from_the_sidecar() {
    let path = fixture("textures/swatch.png");
    let linear = import(
        &path,
        &meta_with_settings(&path, r#"{"colorSpace":"linear"}"#),
        &Limits::default(),
    );
    assert!(!linear.has_errors());
    let i = linear.intermediate.unwrap();
    assert_eq!(i.textures[0].color_space, ColorSpace::Linear);
    // The texels are the same bytes; only the declaration differs.
    let srgb = import_fixture("textures/swatch.png").intermediate.unwrap();
    assert_eq!(i.textures[0].rgba8, srgb.textures[0].rgba8);
    assert_ne!(i.to_bytes(), srgb.to_bytes());

    let bad = import(
        &path,
        &meta_with_settings(&path, r#"{"colorSpace":"gamma"}"#),
        &Limits::default(),
    );
    assert_eq!(codes(&bad), [Code::IMPORT_INVALID_DATA]);
    assert_eq!(
        bad.diagnostics[0].pointer.as_deref(),
        Some("/settings/colorSpace")
    );
    assert!(bad.diagnostics[0]
        .document
        .as_deref()
        .unwrap()
        .ends_with("swatch.png.meta.json"));
}

#[test]
fn only_png_and_jpeg_are_images() {
    // A GIF pushed through the image importer by its sidecar.
    let path = fixture("textures/swatch.gif");
    let mut meta = Meta::new(&path);
    assert_eq!(meta.importer, "fynkel.file");
    meta.importer = "fynkel.image".to_string();
    let r = import(&path, &meta, &Limits::default());
    assert_eq!(
        codes(&r),
        [Code::IMPORT_UNSUPPORTED],
        "{:#?}",
        r.diagnostics
    );
    assert!(r.intermediate.is_none());

    // Not an image at all.
    let path = fixture("robot.bin");
    let mut meta = Meta::new(&path);
    meta.importer = "fynkel.image".to_string();
    let r = import(&path, &meta, &Limits::default());
    assert_eq!(codes(&r), [Code::IMPORT_UNSUPPORTED]);

    // Truncated PNG.
    let dir = std::env::temp_dir().join(format!("fynkel-import-media-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = std::fs::read(fixture("textures/swatch_palette.png")).unwrap();
    let cut = dir.join("cut.png");
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    let r = import(&cut, &Meta::new(&cut), &Limits::default());
    assert_eq!(
        codes(&r),
        [Code::IMPORT_INVALID_DATA],
        "{:#?}",
        r.diagnostics
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn texture_limits_hold() {
    let path = fixture("textures/swatch.png");
    let meta = Meta::new(&path);
    let small = Limits {
        max_texture_dimension: 2,
        ..Limits::default()
    };
    let r = import(&path, &meta, &small);
    assert_eq!(codes(&r), [Code::IMPORT_LIMIT], "{:#?}", r.diagnostics);
    let tiny_file = Limits {
        max_image_bytes: 10,
        ..Limits::default()
    };
    let r = import(&path, &meta, &tiny_file);
    assert_eq!(codes(&r), [Code::IMPORT_LIMIT]);
}

// --- audio -------------------------------------------------------------------

#[test]
fn wav_16_bit_stereo_decodes_to_interleaved_f32() {
    let r = import_fixture("audio/tone.wav");
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    assert!(r.diagnostics.is_empty());
    assert!(r.sub_assets.is_empty());
    let i = r.intermediate.unwrap();
    assert_eq!(i.importer, "fynkel.audio");
    assert_eq!(i.clips.len(), 1);
    let c = &i.clips[0];
    assert_eq!(c.sub_id, None);
    assert_eq!(c.name, "tone");
    assert_eq!(c.codec, "pcm");
    assert_eq!(c.sample_rate, 8000);
    assert_eq!(c.channels, 2);
    assert_eq!(c.frames(), 400);
    assert!((c.duration_seconds() - 0.05).abs() < 1e-9);
    assert_eq!(c.samples.len(), 800);
    // Left: 100 Hz sine at half amplitude; right: a ramp from -1 to 1.
    assert_eq!(c.samples[0], 0.0);
    assert!((c.samples[1] + 1.0).abs() < 1e-4);
    assert!(
        (c.samples[40] - 0.5).abs() < 1e-3,
        "left channel at frame 20 (a quarter period) is the sine's peak"
    );
    assert!((c.samples[799] - 1.0).abs() < 1e-4);
    assert!(c.samples.iter().all(|s| (-1.0..=1.0).contains(s)));
    assert_matches_expected(&i, "audio/tone.expected.fyni");
}

#[test]
fn wav_8_and_24_bit_are_normalized() {
    let tiny = import_fixture("audio/tiny8.wav").intermediate.unwrap();
    let c = &tiny.clips[0];
    assert_eq!((c.sample_rate, c.channels, c.frames()), (22050, 1, 3));
    assert_eq!(c.samples[0], -1.0);
    assert_eq!(c.samples[1], 0.0);
    assert!((c.samples[2] - 1.0).abs() < 0.01);

    let deep = import_fixture("audio/tone24.wav").intermediate.unwrap();
    let c = &deep.clips[0];
    assert_eq!((c.sample_rate, c.channels, c.frames()), (48000, 1, 48));
    // 1 kHz at 48 kHz: frame 12 is the peak.
    assert!((c.samples[12] - 1.0).abs() < 1e-5, "{}", c.samples[12]);
    assert!((c.samples[36] + 1.0).abs() < 1e-5);
}

#[test]
fn ogg_vorbis_decodes_close_to_the_wav_it_came_from() {
    let r = import_fixture("audio/tone.ogg");
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    let ogg = r.intermediate.unwrap();
    let wav = import_fixture("audio/tone.wav").intermediate.unwrap();
    let (a, b) = (&ogg.clips[0], &wav.clips[0]);
    assert_eq!(a.codec, "vorbis");
    assert_eq!((a.sample_rate, a.channels), (b.sample_rate, b.channels));
    assert_eq!(
        a.frames(),
        b.frames(),
        "gapless: no encoder padding leaks in"
    );
    let worst = a
        .samples
        .iter()
        .zip(&b.samples)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 0.1, "worst sample error {worst}");
    assert!(a.samples.iter().all(|s| (-1.0..=1.0).contains(s)));

    // Decoding is deterministic in-process and across runs of the same build.
    let again = import_fixture("audio/tone.ogg").intermediate.unwrap();
    assert_eq!(again.to_bytes(), ogg.to_bytes());

    let mono = import_fixture("audio/tone_mono.ogg").intermediate.unwrap();
    let c = &mono.clips[0];
    assert_eq!((c.sample_rate, c.channels, c.frames()), (44100, 1, 22050));
    let peak = c.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    // ffmpeg's `sine` source plays at about -18 dBFS.
    assert!(peak > 0.1 && peak < 0.2, "peak {peak}");
}

#[test]
fn only_wav_and_ogg_vorbis_are_audio() {
    let path = fixture("audio/tone.flac");
    let mut meta = Meta::new(&path);
    assert_eq!(meta.importer, "fynkel.file");
    meta.importer = "fynkel.audio".to_string();
    let r = import(&path, &meta, &Limits::default());
    assert_eq!(
        codes(&r),
        [Code::IMPORT_UNSUPPORTED],
        "{:#?}",
        r.diagnostics
    );

    let path = fixture("textures/swatch.png");
    let mut meta = Meta::new(&path);
    meta.importer = "fynkel.audio".to_string();
    let r = import(&path, &meta, &Limits::default());
    assert_eq!(codes(&r), [Code::IMPORT_UNSUPPORTED]);

    let r = import_fixture("audio/truncated.wav");
    assert_eq!(
        codes(&r),
        [Code::IMPORT_INVALID_DATA],
        "{:#?}",
        r.diagnostics
    );
    assert!(r.intermediate.is_none());
}

#[test]
fn audio_limits_hold() {
    let path = fixture("audio/tone.wav");
    let short = Limits {
        max_audio_frames: 100,
        ..Limits::default()
    };
    let r = import(&path, &Meta::new(&path), &short);
    assert_eq!(codes(&r), [Code::IMPORT_LIMIT], "{:#?}", r.diagnostics);
}

#[test]
fn stray_sidecar_sub_assets_are_reported_and_dropped() {
    for f in ["textures/swatch.png", "audio/tone.wav"] {
        let path = fixture(f);
        let mut meta = Meta::new(&path);
        meta.sub_assets
            .push(("mesh/0".to_string(), uuid::Uuid::new_v4()));
        let r = import(&path, &meta, &Limits::default());
        assert!(!r.has_errors());
        assert_eq!(codes(&r), [Code::IMPORT_SKIPPED]);
        assert!(r.sub_assets.is_empty());
        assert!(r.sidecar_changed(&meta));
    }
}
