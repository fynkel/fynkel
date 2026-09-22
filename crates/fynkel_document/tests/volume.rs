//! Volume profiles (R3a): `.volume.json` read into the core's
//! `VolumeProfile` — present fields override, absent ones inherit —
//! and checked by the schema's ranges.

use std::path::Path;

use fynkel_core::volume::{AoOverrides, VolumeProfile};
use fynkel_document::{Code, Document, DocumentKind, SchemaSet, VolumeDocument};

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

fn doc(body: &str) -> VolumeDocument {
    let text = format!(
        r#"{{"$schema":"https://fynkel.dev/schemas/volume.schema.json","formatVersion":1,"id":"c0000000-0000-4000-8000-000000000701","name":"Look"{body}}}"#
    );
    VolumeDocument::from_document(
        Document::parse(DocumentKind::Volume, Some("look.volume.json"), &text).unwrap(),
    )
}

fn codes(v: &VolumeDocument) -> Vec<(String, String)> {
    v.validate(&schemas())
        .iter()
        .map(|d| (d.code.0.to_string(), d.pointer.clone().unwrap_or_default()))
        .collect()
}

#[test]
fn present_fields_override_and_absent_ones_inherit() {
    let empty = doc("");
    assert_eq!(empty.profile, VolumeProfile::default());
    assert_eq!(codes(&empty), []);

    let radius = doc(r#","ambientOcclusion":{"radius":1.5}"#);
    assert_eq!(
        radius.profile.ambient_occlusion,
        AoOverrides {
            radius: Some(1.5),
            intensity: None
        }
    );
    assert_eq!(codes(&radius), []);

    let both = doc(r#","ambientOcclusion":{"radius":0.4,"intensity":0}"#);
    assert_eq!(both.profile.ambient_occlusion.intensity, Some(0.0));
    assert_eq!(codes(&both), []);
    assert_eq!(
        DocumentKind::from_path(Path::new("a/look.volume.json")),
        Some(DocumentKind::Volume)
    );
}

#[test]
fn the_schema_holds_the_ranges_and_the_view_reads_what_it_can() {
    let d = |p: &str| ("FYN-D0100".to_string(), p.to_string());
    assert_eq!(
        codes(&doc(r#","ambientOcclusion":{"radius":0}"#)),
        [d("/ambientOcclusion/radius")]
    );
    assert_eq!(
        codes(&doc(r#","ambientOcclusion":{"intensity":-1}"#)),
        [d("/ambientOcclusion/intensity")]
    );
    assert_eq!(
        codes(&doc(r#","ambientOcclusion":{"radius":1,"samples":64}"#)),
        [d("/ambientOcclusion/samples")]
    );
    assert_eq!(codes(&doc(r#","bloom":{}"#)), [d("/bloom")]);
    // A value of the wrong type: the schema says so once, and the view
    // leaves the field inherited.
    let wrong = doc(r#","ambientOcclusion":{"radius":"wide"}"#);
    assert_eq!(wrong.profile.ambient_occlusion.radius, None);
    assert!(wrong
        .read_diagnostics()
        .iter()
        .any(|x| x.code == Code::VOLUME_MALFORMED_ENTRY));
    assert_eq!(codes(&wrong), [d("/ambientOcclusion/radius")]);
    // A new document spells the defaults out and validates.
    let fresh = Document::new(DocumentKind::Volume, "Look");
    let fresh = VolumeDocument::from_document(fresh);
    assert_eq!(fresh.profile.ambient_occlusion.radius, Some(0.6));
    assert_eq!(codes(&fresh), []);
}
