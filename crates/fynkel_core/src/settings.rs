//! User settings (blueprint §19.2d item 12, scope rule "Scenes and
//! settings"): the Rust model of `settings.json`. `fynkel codegen` writes
//! `schemas/settings.schema.json` from this record and the Dart/Rust
//! document codecs; `fynkel_document::settings` reads and writes the file
//! (the flat fields here plus the `input.rebinds` map, which a wire record
//! cannot hold); the Dart `Settings` service in `GameContext` is what a
//! game touches. Volumes are linear gains in `0..=1` applied per bus
//! (`master` scales the other two); `quality` names one of the project's
//! quality profiles (empty: the project's default).

use fynkel_schema_derive::Schema;

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.Settings", document = "settings")]
pub struct Settings {
    /// Gain of the `master` bus, `0..=1`.
    pub master_volume: f32,
    /// Gain of the `music` bus, `0..=1`.
    pub music_volume: f32,
    /// Gain of the `sfx` bus, `0..=1`.
    pub sfx_volume: f32,
    /// Every bus silent; the volumes are kept.
    pub muted: bool,
    /// A quality profile name from the project file; empty means the
    /// project's default profile.
    pub quality: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            music_volume: 1.0,
            sfx_volume: 1.0,
            muted: false,
            quality: String::new(),
        }
    }
}

impl Settings {
    /// `(field, message)` per value out of range, in field order.
    pub fn violations(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        for (name, v) in [
            ("master_volume", self.master_volume),
            ("music_volume", self.music_volume),
            ("sfx_volume", self.sfx_volume),
        ] {
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                out.push((name, format!("{name} must be in 0..=1, got {v}")));
            }
        }
        out
    }

    pub fn validate(&self) -> Result<(), String> {
        match self.violations().into_iter().next() {
            Some((_, msg)) => Err(msg),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::HasSchema;

    #[test]
    fn defaults_are_full_volume_and_the_project_profile() {
        let s = Settings::default();
        assert_eq!(s.master_volume, 1.0);
        assert!(!s.muted);
        assert!(s.quality.is_empty());
        assert!(s.validate().is_ok());
        assert_eq!(Settings::SCHEMA.document, Some("settings"));
    }

    #[test]
    fn volumes_outside_the_unit_range_are_violations() {
        let s = Settings {
            master_volume: 1.5,
            sfx_volume: -0.1,
            ..Settings::default()
        };
        let v = s.violations();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, "master_volume");
        assert_eq!(v[1].0, "sfx_volume");
        assert!(s.validate().is_err());
    }
}
