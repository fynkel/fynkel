//! Clips decoded from the three formats a game loads, and the files that
//! are refused.

use fynkel_audio::{Clip, wav};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn peak(clip: &Clip) -> f32 {
    clip.samples().iter().fold(0.0, |m, s| m.max(s.abs()))
}

#[test]
fn wav_ogg_and_mp3_decode_to_the_same_half_second_tone() {
    // Made with ffmpeg: a 440 Hz sine at 1/8, half a second long; in stereo
    // it's 3 dB lower in each ear.
    let stereo = 0.125 / 2f32.sqrt();
    for (name, rate, channels, loud) in
        [("tone.wav", 22_050, 2, stereo), ("tone.ogg", 44_100, 1, 0.125), ("tone.mp3", 44_100, 2, stereo)]
    {
        let clip = Clip::decode(&fixture(name), name).unwrap();
        assert_eq!((clip.rate(), clip.channels()), (rate, channels), "{name}");
        // MP3 and Vorbis pad a little at the ends.
        assert!((clip.seconds() - 0.5).abs() < 0.06, "{name}: {} s", clip.seconds());
        assert!((peak(&clip) - loud).abs() < 0.01, "{name}: peak {}", peak(&clip));
        assert_eq!(clip.bytes(), clip.samples().len() * 4);
    }
}

#[test]
fn a_file_that_isnt_sound_is_refused_with_its_name() {
    let error = Clip::decode(b"this is not a sound file at all", "notes.ogg").unwrap_err();
    assert!(error.starts_with("notes.ogg: "), "{error}");
    let error = Clip::decode(&[], "empty.wav").unwrap_err();
    assert!(error.starts_with("empty.wav: "), "{error}");
}

#[test]
fn more_than_two_channels_are_refused() {
    let error = Clip::decode(&wav(8_000, 3, &[0.0; 30]), "surround.wav").unwrap_err();
    assert!(error.contains("3 channels"), "{error}");
}

#[test]
fn a_wav_written_reads_back() {
    let samples = [0.0, 0.5, -0.5, 1.0, -1.0, 0.25];
    let clip = Clip::decode(&wav(16_000, 2, &samples), "").unwrap();
    assert_eq!((clip.rate(), clip.channels(), clip.frames()), (16_000, 2, 3));
    for (a, b) in clip.samples().iter().zip(samples) {
        assert!((a - b).abs() < 1.0 / 16_000.0, "{a} vs {b}");
    }
}

#[test]
fn clips_made_by_hand_are_checked() {
    assert!(Clip::new(0, 1, vec![]).unwrap_err().contains("rate"));
    assert!(Clip::new(8_000, 0, vec![]).unwrap_err().contains("1 or 2"));
    assert!(Clip::new(8_000, 2, vec![0.0; 3]).unwrap_err().contains("whole frames"));
    assert!(Clip::new(8_000, 1, vec![0.0, f32::NAN]).unwrap_err().contains("sample 1"));
}
