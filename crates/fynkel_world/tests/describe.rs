//! Self-describing types (ADR-0001 rule 1): the listed fields match the
//! real struct, and every write through `set` is validated.

use std::collections::HashSet;

use fynkel_world::*;

/// The checks every described type must pass.
fn check_description<T: Describe + Default + PartialEq + std::fmt::Debug>() {
    let fields = T::fields();
    let value = T::default();
    assert!(!fields.is_empty(), "{} lists no fields", T::NAME);

    let names: HashSet<_> = fields.iter().map(|f| f.name).collect();
    assert_eq!(names.len(), fields.len(), "{} has duplicate field names", T::NAME);

    for (i, field) in fields.iter().enumerate() {
        assert!(field.default.is(field.kind), "{}.{}: default has the wrong kind", T::NAME, field.name);
        assert_eq!(
            value.get(i),
            Some(field.default),
            "{}.{}: listed default differs from Default::default()",
            T::NAME,
            field.name
        );
        // Writing the default back is accepted and changes nothing.
        let mut copy = T::default();
        copy.set(i, field.default).unwrap();
        assert_eq!(copy, value);
    }
    assert_eq!(value.get(fields.len()), None, "{} has more readable fields than it lists", T::NAME);
}

#[test]
fn every_type_describes_itself_truthfully() {
    check_description::<Transform>();
    check_description::<MeshRenderer>();
    check_description::<Camera>();
    check_description::<Light>();
    check_description::<Material>();
}

#[test]
fn set_writes_a_valid_value() {
    let mut camera = Camera::default();
    camera.set(0, Value::F32(75.0)).unwrap();
    assert_eq!(camera.fov_y, 75.0);

    let mut light = Light::default();
    light.set(0, Value::Enum(1)).unwrap();
    assert_eq!(light.kind, LightKind::Ambient);

    let mut t = Transform::default();
    let turn = Quat::from_rotation_y(1.0);
    t.set(1, Value::Quat(turn)).unwrap();
    assert_eq!(t.rotation, turn);
}

#[test]
fn set_refuses_bad_values_and_changes_nothing() {
    let mut camera = Camera::default();
    let cases = [
        (0, Value::Bool(true), "wrong kind"),
        (0, Value::F32(f32::NAN), "NaN"),
        (0, Value::F32(f32::INFINITY), "infinity"),
        (0, Value::F32(180.0), "above the range"),
        (1, Value::F32(0.0), "near plane at zero"),
        (3, Value::Color(Color::rgb(f32::NAN, 0.0, 0.0)), "NaN colour"),
    ];
    for (index, value, why) in cases {
        assert!(camera.set(index, value).is_err(), "accepted {why}");
        assert_eq!(camera, Camera::default(), "{why} changed the camera");
    }
    assert_eq!(camera.set(14, Value::F32(1.0)), Err(FieldError::NoField { ty: "Camera", index: 14 }));

    let mut light = Light::default();
    assert_eq!(light.set(0, Value::Enum(4)), Err(FieldError::WrongKind { ty: "Light", field: "kind" }));
    assert!(light.set(2, Value::F32(-1.0)).is_err(), "negative intensity");
    assert!(light.set(3, Value::F32(-1.0)).is_err(), "negative range");
    assert!(light.set(4, Value::F32(180.0)).is_err(), "a spot as wide as a plane");
    assert!(light.set(5, Value::F32(1.5)).is_err(), "softness past 1");
    assert_eq!(light, Light::default());
    assert_eq!(light.set(0, Value::Enum(3)), Ok(()));
    assert_eq!(light.kind, LightKind::Spot);

    let mut t = Transform::default();
    assert_eq!(
        t.set(1, Value::Quat(Quat::from_xyzw(0.0, 0.0, 0.0, 0.5))),
        Err(FieldError::NotUnit { ty: "Transform", field: "rotation" })
    );
    assert_eq!(t, Transform::default());

    // A Vec2's range holds for each component.
    let mut material = Material::default();
    for (value, why) in [
        (Vec2::new(1.0, 2e4), "y above the range"),
        (Vec2::new(-2e4, 1.0), "x below the range"),
        (Vec2::new(f32::NAN, 1.0), "NaN"),
    ] {
        assert!(material.set(5, Value::Vec2(value)).is_err(), "uv_scale accepted {why}");
    }
    assert_eq!(material, Material::default());
    material.set(5, Value::Vec2(Vec2::new(-1e4, 1e4))).unwrap();
    assert_eq!(material.uv_scale, Vec2::new(-1e4, 1e4));
}

#[test]
fn errors_read_well() {
    let mut camera = Camera::default();
    let e = camera.set(0, Value::F32(200.0)).unwrap_err();
    assert_eq!(e.to_string(), "Camera.fov_y: 200 is outside 1..=179");
}

// --- Small types -----------------------------------------------------------

#[test]
fn colors_come_in_as_srgb_and_convert_to_linear() {
    let c = Color::from_argb32(0x80FF8000);
    assert_eq!((c.r, c.g, c.b), (1.0, 128.0 / 255.0, 0.0));
    assert!((c.a - 128.0 / 255.0).abs() < 1e-6);

    let [r, g, b, a] = Color::rgba(0.5, 0.04, 1.0, 0.25).to_linear();
    assert!((r - 0.214_041).abs() < 1e-5, "mid grey is about 21% linear");
    assert!((g - 0.04 / 12.92).abs() < 1e-7, "the linear toe");
    assert_eq!((b, a), (1.0, 0.25));
}

#[test]
fn transform_looks_at_its_target() {
    let t = Transform::from_position(Vec3::new(0.0, 1.5, 4.0)).looking_at(Vec3::ZERO);
    let wanted = (Vec3::ZERO - t.position).normalize();
    assert!(t.forward().abs_diff_eq(wanted, 1e-5), "forward {} != {}", t.forward(), wanted);
    assert!((t.rotation.length() - 1.0).abs() < 1e-5);
    // A target straight above can't define "up", so the rotation is kept.
    assert_eq!(Transform::default().looking_at(Vec3::Y * 3.0).rotation, Quat::IDENTITY);

    let moved = Transform { position: Vec3::X, scale: Vec3::splat(2.0), ..Transform::default() };
    assert!(moved.matrix().transform_point3(Vec3::ONE).abs_diff_eq(Vec3::new(3.0, 2.0, 2.0), 1e-6));
}
