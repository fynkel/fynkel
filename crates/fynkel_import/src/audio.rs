//! Audio importer (`fynkel.audio`, blueprint §19.2c item 11): WAV (PCM
//! 8/16/24/32-bit and float) and Ogg Vorbis → interleaved `f32` PCM with
//! its sample rate and channel count. Only those two containers and codecs
//! are compiled in (`symphonia` with `wav`, `pcm`, `ogg`, `vorbis`);
//! anything else is `FYN-A0103`. No resampling, no channel mixing, no
//! normalization: the clip is stored as the file had it, and the runtime's
//! mixer (a later milestone) decides. Metadata tags are not carried over.
//!
//! The importer has no settings yet.

use std::io::Cursor;
use std::path::Path;

use fynkel_contracts::{Code, Diagnostic};
use fynkel_document::asset::Meta;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::intermediate::{AudioData, Intermediate};
use crate::{read_source, Imported, Limits};

pub const IMPORTER: &str = fynkel_document::asset::AUDIO_IMPORTER;
pub const IMPORTER_VERSION: u32 = 1;

pub fn import(path: &Path, meta: &Meta, limits: &Limits) -> Imported {
    let shown = path.display().to_string();
    let fail = |d: Diagnostic| Imported {
        intermediate: None,
        sub_assets: meta.sub_assets.clone(),
        diagnostics: vec![d.with_document(shown.clone())],
    };
    let bytes = match read_source(path, limits) {
        Ok(b) => b,
        Err(d) => return fail(d),
    };
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let clip = match decode(bytes, &name, extension.as_deref(), limits) {
        Ok(c) => c,
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
    intermediate.clips.push(clip);
    let mut diagnostics = Vec::new();
    if !meta.sub_assets.is_empty() {
        diagnostics.push(
            Diagnostic::warning(
                Code::IMPORT_SKIPPED,
                format!(
                    "sidecar lists {} sub-asset(s); an audio clip has none (references to them will not resolve)",
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

/// Decodes a whole WAV or Ogg Vorbis file to one clip (the first audio
/// track with a known codec).
pub fn decode(
    bytes: Vec<u8>,
    name: &str,
    extension: Option<&str>,
    limits: &Limits,
) -> Result<AudioData, Diagnostic> {
    let translate = |what: &str, e: SymphoniaError| match e {
        SymphoniaError::Unsupported(f) => Diagnostic::error(
            Code::IMPORT_UNSUPPORTED,
            format!("{what}: unsupported ({f}); only WAV (PCM) and Ogg Vorbis are imported"),
        ),
        SymphoniaError::LimitError(l) => {
            Diagnostic::error(Code::IMPORT_LIMIT, format!("{what}: {l}"))
        }
        SymphoniaError::IoError(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            Diagnostic::error(
                Code::IMPORT_INVALID_DATA,
                format!("{what}: the file ends before the stream does"),
            )
        }
        other => Diagnostic::error(Code::IMPORT_INVALID_DATA, format!("{what}: {other}")),
    };

    let source = MediaSourceStream::new(
        Box::new(Cursor::new(bytes)),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| match e {
            SymphoniaError::Unsupported(_) => Diagnostic::error(
                Code::IMPORT_UNSUPPORTED,
                "not a WAV or Ogg file (only those two audio containers are imported)",
            ),
            other => translate("container", other),
        })?;
    let track = format
        .first_track_known_codec(TrackType::Audio)
        .ok_or_else(|| {
            Diagnostic::error(
                Code::IMPORT_UNSUPPORTED,
                "no audio track with a codec this importer knows (PCM, Vorbis)",
            )
        })?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err(Diagnostic::error(
            Code::IMPORT_INVALID_DATA,
            "the audio track has no codec parameters",
        ));
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| translate("codec", e))?;
    let codec = {
        let short = decoder.codec_info().short_name;
        if short.starts_with("pcm") {
            "pcm".to_string()
        } else {
            short.to_string()
        }
    };

    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut samples: Vec<f32> = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => return Err(translate("stream", e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymphoniaError::DecodeError(msg)) => {
                // A corrupt packet in an otherwise fine file: symphonia
                // says skip it. We do not — a game asset with a hole in
                // it is a bug to fix at the source.
                return Err(Diagnostic::error(
                    Code::IMPORT_INVALID_DATA,
                    format!("packet at {}: {msg}", packet.pts),
                ));
            }
            Err(e) => return Err(translate("decode", e)),
        };
        let spec = buffer.spec();
        let (rate, count) = (spec.rate(), spec.channels().count());
        if count == 0 || count > u16::MAX as usize || rate == 0 {
            return Err(Diagnostic::error(
                Code::IMPORT_INVALID_DATA,
                format!("{count} channel(s) at {rate} Hz"),
            ));
        }
        if sample_rate == 0 {
            sample_rate = rate;
            channels = count as u16;
        } else if rate != sample_rate || count as u16 != channels {
            return Err(Diagnostic::error(
                Code::IMPORT_UNSUPPORTED,
                "the stream changes its sample rate or channel count mid-way",
            ));
        }
        let frames = samples.len() as u64 / channels as u64 + buffer.frames() as u64;
        if frames > limits.max_audio_frames {
            return Err(Diagnostic::error(
                Code::IMPORT_LIMIT,
                format!(
                    "longer than {} frames ({:.0} s at {sample_rate} Hz)",
                    limits.max_audio_frames,
                    limits.max_audio_frames as f64 / sample_rate as f64
                ),
            ));
        }
        buffer.copy_to_vec_interleaved(&mut chunk);
        // Lossy decoders overshoot a little; the contract is [-1, 1].
        samples.extend(chunk.iter().map(|s| s.clamp(-1.0, 1.0)));
    }
    if sample_rate == 0 {
        return Err(Diagnostic::error(
            Code::IMPORT_INVALID_DATA,
            "the stream has no audio frames",
        ));
    }
    Ok(AudioData {
        sub_id: None,
        name: name.to_string(),
        codec,
        sample_rate,
        channels,
        samples,
    })
}
