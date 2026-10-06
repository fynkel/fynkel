//! Projected world anchors (blueprint §19.2d item 13, §10): the frame's
//! camera and poses mapped into a viewport, the visibility flags, the
//! explicit occlusion policy, the 64-anchor bound.

use fynkel_core::components::ComponentValue;
use fynkel_core::schema::{ProjectedPointRecord, ProjectionAnchorRecord};
use fynkel_core::*;
use glam::{Quat, Vec3, Vec4};

fn world() -> World {
    World::new(WorldConfig::default())
}

fn spawn(
    w: &mut World,
    _fixture_id: u64,
    transform: Transform,
    components: Vec<ComponentValue>,
) -> Entity {
    let commit = w
        .edit_gameplay(|edit| edit.spawn("", transform, None))
        .unwrap();
    let entity = commit.resolve(commit.value).unwrap();
    assert_eq!(w.step().status, Status::Ok);
    w.drain_events();
    if !components.is_empty() {
        w.edit_gameplay(|edit| {
            for value in components {
                edit.set_component(entity, value)?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(w.step().status, Status::Ok);
        w.drain_events();
    }
    entity
}

fn anchor(entity: Entity) -> ProjectionAnchorRecord {
    ProjectionAnchorRecord {
        entity,
        offset: Vec3::ZERO,
        mask: 0,
    }
}

/// A camera at (0, 0, 5) looking down -Z with a 60° vertical field of
/// view: at depth 5 the visible half height is 5·tan 30° ≈ 2.887 m.
fn camera_world(w: &mut World, priority: i32) -> Entity {
    spawn(
        w,
        1 + priority as u64,
        Transform::from_position(Vec3::new(0.0, 0.0, 5.0)),
        vec![ComponentValue::Camera(Camera {
            fov_y: 60f32.to_radians(),
            priority,
            ..Camera::default()
        })],
    )
}

#[test]
fn points_land_where_the_renderer_draws_them() {
    let mut w = world();
    let cam = camera_world(&mut w, 0);
    let centre = spawn(&mut w, 10, Transform::from_position(Vec3::ZERO), vec![]);
    let right = spawn(
        &mut w,
        11,
        Transform::from_position(Vec3::new(100.0, 0.0, 0.0)),
        vec![],
    );
    let behind = spawn(
        &mut w,
        12,
        Transform::from_position(Vec3::new(0.0, 0.0, 6.0)),
        vec![],
    );
    let mut out = Vec::new();
    let info = w.project(
        800,
        600,
        &[
            anchor(centre),
            ProjectionAnchorRecord {
                entity: centre,
                offset: Vec3::new(0.0, 1.0, 0.0),
                mask: 0,
            },
            anchor(right),
            anchor(behind),
        ],
        &mut out,
    );
    assert_eq!(info.camera, Some(cam));
    assert_eq!((info.projected, info.dropped), (4, 0));
    assert_eq!(out.len(), 4);
    let c = &out[0];
    assert_eq!(c.entity, centre);
    assert_eq!(c.flags, PROJECTED_IN_FRONT | PROJECTED_ON_SCREEN);
    assert!(
        (c.x - 400.0).abs() < 1e-3 && (c.y - 300.0).abs() < 1e-3,
        "{c:?}"
    );
    assert!((c.depth - 5.0).abs() < 1e-5);
    // One metre up at depth 5: 300 · (1 / 2.887) ≈ 103.9 px above centre.
    let up = &out[1];
    assert!((up.x - 400.0).abs() < 1e-3, "{up:?}");
    assert!(
        (up.y - (300.0 - 300.0 / (5.0 * 30f32.to_radians().tan()))).abs() < 1e-2,
        "{up:?}"
    );
    assert_eq!(up.flags, PROJECTED_IN_FRONT | PROJECTED_ON_SCREEN);
    // Far to the right: in front, off the viewport.
    let r = &out[2];
    assert_eq!(r.flags, PROJECTED_IN_FRONT, "{r:?}");
    assert!(r.x > 800.0);
    // Behind the camera: nothing to place.
    let b = &out[3];
    assert_eq!(b.flags, 0, "{b:?}");
    assert!(b.depth < 0.0);

    // The same answer as projecting the extract's camera and record by
    // hand — the renderer's matrices.
    let ex = w.extract();
    let ex_cam = ex.camera.unwrap();
    let vp = projection_matrix(&ex_cam.camera, 800.0 / 600.0) * view_matrix(&ex_cam.world);
    let rec = ex.records.iter().find(|r| r.entity == centre).unwrap();
    let p = rec.world.transform_point3(Vec3::new(0.0, 1.0, 0.0));
    let clip = vp * Vec4::new(p.x, p.y, p.z, 1.0);
    let x = (clip.x / clip.w * 0.5 + 0.5) * 800.0;
    let y = (0.5 - clip.y / clip.w * 0.5) * 600.0;
    assert!((x - up.x).abs() < 1e-3 && (y - up.y).abs() < 1e-3);
}

#[test]
fn the_camera_is_the_one_the_frame_renders_from() {
    let mut w = world();
    let mut out = Vec::new();
    let e = spawn(&mut w, 10, Transform::from_position(Vec3::ZERO), vec![]);
    let info = w.project(800, 600, &[anchor(e)], &mut out);
    assert_eq!(info.camera, None);
    assert_eq!(out[0].flags, PROJECTED_NO_CAMERA);

    let low = camera_world(&mut w, 0);
    let high = spawn(
        &mut w,
        20,
        Transform {
            position: Vec3::new(0.0, 0.0, 5.0),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(3.0),
        },
        vec![ComponentValue::Camera(Camera {
            priority: 7,
            ..Camera::default()
        })],
    );
    let info = w.project(800, 600, &[anchor(e)], &mut out);
    assert_eq!(
        info.camera,
        Some(high),
        "highest priority wins over {low:?}"
    );
    assert_eq!(w.extract().camera.map(|c| c.camera.priority), Some(7));
    // Rotated 90° about Y the high camera looks down -X; the origin sits
    // 5 m off its side, at its right angle: not in front.
    assert_eq!(out[0].flags & PROJECTED_IN_FRONT, 0, "{:?}", out[0]);
    // The scale did not change the view (a scaled camera does not zoom).
    let ex = w.extract().camera.unwrap();
    let view = view_matrix(&ex.world);
    let (s, _, _) = view.inverse().to_scale_rotation_translation();
    assert!((s - Vec3::ONE).length() < 1e-5);
}

#[test]
fn manual_step_snap_displays_current_pose_and_preserves_interpolation_remainder() {
    for fraction in [0.0, 0.5] {
        let mut w = world();
        camera_world(&mut w, 0);
        let root = spawn(&mut w, 10, Transform::IDENTITY, vec![]);
        let child = spawn(&mut w, 11, Transform::from_position(Vec3::X), vec![]);
        w.edit_gameplay(|edit| {
            edit.set_parent(child, Some(root.into()), false)?;
            Ok(())
        })
        .unwrap();
        w.step();
        w.begin_frame(w.config().fixed_dt * fraction);
        let alpha = w.alpha();
        w.prepare_gameplay_tick();
        w.edit_gameplay(|edit| {
            edit.set_transform(root, Transform::from_position(Vec3::X * 2.0))?;
            Ok(())
        })
        .unwrap();
        w.step();
        w.snap_interpolation();
        assert_eq!(w.alpha(), alpha);
        let ex = w.extract();
        for (entity, x) in [(root, 2.0), (child, 3.0)] {
            assert!(
                (ex.records
                    .iter()
                    .find(|r| r.entity == entity)
                    .unwrap()
                    .world
                    .w_axis
                    .x
                    - x)
                    .abs()
                    < 1e-5
            );
        }
        let mut projected = Vec::new();
        w.project(800, 600, &[anchor(child)], &mut projected);
        let expected_x = 400.0 + 3.0 / (5.0 * 30f32.to_radians().tan() * 800.0 / 600.0) * 400.0;
        assert!((projected[0].x - expected_x).abs() < 1e-2);

        // The next regular tick interpolates normally from the snapped pose.
        w.begin_frame(w.config().fixed_dt);
        w.prepare_gameplay_tick();
        w.edit_gameplay(|edit| {
            edit.set_transform(root, Transform::from_position(Vec3::X * 4.0))?;
            Ok(())
        })
        .unwrap();
        w.step();
        let ex = w.extract();
        let x = ex
            .records
            .iter()
            .find(|r| r.entity == root)
            .unwrap()
            .world
            .w_axis
            .x;
        assert!((x - (2.0 + 2.0 * fraction)).abs() < 1e-5);
    }
}

#[test]
fn a_dead_entity_is_missing_and_a_moving_one_is_interpolated() {
    let mut w = world();
    camera_world(&mut w, 0);
    let e = spawn(&mut w, 10, Transform::from_position(Vec3::ZERO), vec![]);
    let mut out = Vec::new();
    // Move it 2 m right over one tick; halfway through the next frame the
    // extract and the projection agree on the interpolated pose.
    w.prepare_gameplay_tick();
    w.edit_gameplay(|edit| {
        edit.set_transform(e, Transform::from_position(Vec3::new(2.0, 0.0, 0.0)))?;
        Ok(())
    })
    .unwrap();
    w.step();
    w.drain_events();
    let plan = w.begin_frame(w.config().fixed_dt * 0.5);
    assert_eq!(plan.steps, 0);
    let ex = w.extract();
    let rec = ex.records.iter().find(|r| r.entity == e).unwrap();
    let px = rec.world.transform_point3(Vec3::ZERO).x;
    assert!(px > 0.0 && px < 2.0, "interpolated: {px}");
    w.project(800, 600, &[anchor(e)], &mut out);
    let expected_x = 400.0 + px / (5.0 * 30f32.to_radians().tan() * 800.0 / 600.0) * 400.0;
    assert!(
        (out[0].x - expected_x).abs() < 1e-2,
        "{} vs {expected_x}",
        out[0].x
    );

    w.edit_gameplay(|edit| {
        edit.destroy(e)?;
        Ok(())
    })
    .unwrap();
    w.step();
    w.drain_events();
    w.project(800, 600, &[anchor(e)], &mut out);
    assert_eq!(out[0].flags, PROJECTED_MISSING);
    assert_eq!(out[0].entity, e);
}

#[test]
fn occlusion_is_an_explicit_policy_by_collider_mask() {
    let mut w = world();
    camera_world(&mut w, 0);
    // A wall between the camera (z 5) and the point (z 0), on layer 2.
    let _wall = spawn(
        &mut w,
        10,
        Transform::from_position(Vec3::new(0.0, 0.0, 2.5)),
        vec![
            ComponentValue::Collider(Collider {
                size: Vec3::new(4.0, 4.0, 0.2),
                ..Collider::default()
            }),
            ComponentValue::Layer(Layer { layer: 1 }),
        ],
    );
    let target = spawn(
        &mut w,
        11,
        Transform::from_position(Vec3::ZERO),
        vec![ComponentValue::Collider(Collider {
            size: Vec3::new(0.5, 0.5, 0.5),
            ..Collider::default()
        })],
    );
    w.step();
    let mut out = Vec::new();
    w.project(
        800,
        600,
        &[
            anchor(target),
            ProjectionAnchorRecord {
                entity: target,
                offset: Vec3::ZERO,
                mask: 0b10,
            },
            ProjectionAnchorRecord {
                entity: target,
                offset: Vec3::ZERO,
                mask: 0b1,
            },
            ProjectionAnchorRecord {
                entity: target,
                offset: Vec3::ZERO,
                mask: 0xffff_ffff,
            },
        ],
        &mut out,
    );
    assert_eq!(
        out[0].flags, PROJECTED_VISIBLE,
        "no mask: never hidden by geometry"
    );
    assert_eq!(
        out[1].flags,
        PROJECTED_VISIBLE | PROJECTED_OCCLUDED,
        "the wall's layer occludes"
    );
    assert_eq!(
        out[2].flags, PROJECTED_VISIBLE,
        "the anchor's own collider is excluded and the wall is not on layer 1"
    );
    assert_eq!(out[3].flags, PROJECTED_VISIBLE | PROJECTED_OCCLUDED);
    // Position is unaffected by occlusion.
    assert!((out[1].x - 400.0).abs() < 1e-3 && (out[1].y - 300.0).abs() < 1e-3);
}

#[test]
fn the_view_is_bounded_to_sixty_four_anchors() {
    let mut w = world();
    camera_world(&mut w, 0);
    let e = spawn(&mut w, 10, Transform::from_position(Vec3::ZERO), vec![]);
    let anchors: Vec<ProjectionAnchorRecord> =
        (0..MAX_PROJECTION_ANCHORS + 2).map(|_| anchor(e)).collect();
    let mut out: Vec<ProjectedPointRecord> = Vec::new();
    let info = w.project(800, 600, &anchors, &mut out);
    assert_eq!((info.projected, info.dropped), (64, 2));
    assert_eq!(out.len(), 66, "one record per anchor, in order");
    assert_eq!(out[63].flags, PROJECTED_VISIBLE);
    assert_eq!(out[64].flags, PROJECTED_DROPPED);
    assert_eq!(out[65].flags, PROJECTED_DROPPED);
    let d = w.diagnostics();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].code, DiagnosticCode::TOO_MANY_ANCHORS);
    assert_eq!(d[0].command_index, u32::MAX);
    assert!(d[0].message.starts_with("FYN-R0012"), "{}", d[0].message);
    // Exactly 64 is fine and reports nothing; the diagnostics are the
    // step's, cleared by the next one.
    w.step();
    let info = w.project(800, 600, &anchors[..64], &mut out);
    assert_eq!((info.projected, info.dropped), (64, 0));
    assert!(w.diagnostics().is_empty());
}

#[test]
fn sheared_anchor_projection_uses_the_same_affine_matrix_as_rendering() {
    let mut w = world();
    camera_world(&mut w, 0);
    let root = fynkel_scene::spawn(
        w.ecs_mut(),
        Transform {
            scale: Vec3::new(2.0, 1.0, 1.0),
            ..Transform::IDENTITY
        },
        None,
    )
    .unwrap();
    let child = fynkel_scene::spawn(
        w.ecs_mut(),
        Transform {
            rotation: Quat::from_rotation_z(0.7),
            ..Transform::IDENTITY
        },
        Some(root),
    )
    .unwrap();
    w.step();
    let offset = Vec3::new(0.5, 0.25, 0.0);
    let extract = w.extract();
    let matrix = extract
        .records
        .iter()
        .find(|r| r.entity == child)
        .unwrap()
        .world;
    let point = matrix.transform_point3(offset);
    let mut out = Vec::new();
    w.project(
        800,
        600,
        &[ProjectionAnchorRecord {
            entity: child,
            offset,
            mask: 0,
        }],
        &mut out,
    );
    let expected_x = 400.0 + point.x / (5.0 * 30f32.to_radians().tan() * 800.0 / 600.0) * 400.0;
    let expected_y = 300.0 - point.y / (5.0 * 30f32.to_radians().tan()) * 300.0;
    assert!((out[0].x - expected_x).abs() < 1e-3);
    assert!((out[0].y - expected_y).abs() < 1e-3);
}

/// ADR-0062 step 2: an anchor without an entity is a point in the world
/// (`offset` in world space) — a one-off world point to the screen.
#[test]
fn an_anchor_without_an_entity_projects_a_world_point() {
    let mut w = world();
    camera_world(&mut w, 0);
    let mut out = Vec::new();
    w.project(
        800,
        600,
        &[
            ProjectionAnchorRecord {
                entity: Entity::NULL,
                offset: Vec3::new(0.0, 1.0, 0.0),
                mask: 0,
            },
            ProjectionAnchorRecord {
                entity: Entity::NULL,
                offset: Vec3::new(0.0, 0.0, 6.0),
                mask: 0,
            },
        ],
        &mut out,
    );
    let up = &out[0];
    assert_eq!(up.entity, Entity::NULL);
    assert_eq!(up.flags, PROJECTED_IN_FRONT | PROJECTED_ON_SCREEN, "{up:?}");
    assert!((up.x - 400.0).abs() < 1e-3, "{up:?}");
    assert!(
        (up.y - (300.0 - 300.0 / (5.0 * 30f32.to_radians().tan()))).abs() < 1e-2,
        "{up:?}"
    );
    assert!((up.depth - 5.0).abs() < 1e-5);
    // Behind the camera: not placed, not missing.
    assert_eq!(out[1].flags, 0, "{:?}", out[1]);
}

/// ADR-0062 step 2: a viewport point becomes the ray the frame renders
/// along (Unity's `Camera.ScreenPointToRay`): from the near plane, through
/// whatever the point shows.
#[test]
fn a_screen_point_becomes_the_ray_through_what_it_shows() {
    let mut w = world();
    assert_eq!(
        w.screen_ray(800, 600, 400.0, 300.0),
        None,
        "no camera, no ray"
    );
    let cam = camera_world(&mut w, 0);
    let centre = w.screen_ray(800, 600, 400.0, 300.0).unwrap();
    assert_eq!(centre.camera, cam);
    let near = Camera::default().near;
    assert!(
        (centre.origin - Vec3::new(0.0, 0.0, 5.0 - near)).length() < 1e-4,
        "{centre:?}"
    );
    assert!(
        (centre.direction - Vec3::NEG_Z).length() < 1e-5,
        "{centre:?}"
    );

    // Round trip: project a world point, cast the ray at its pixel, and
    // the ray passes through the point.
    let point = Vec3::new(1.5, -0.75, -2.0);
    let mut out = Vec::new();
    w.project(
        800,
        600,
        &[ProjectionAnchorRecord {
            entity: Entity::NULL,
            offset: point,
            mask: 0,
        }],
        &mut out,
    );
    let ray = w.screen_ray(800, 600, out[0].x, out[0].y).unwrap();
    assert!((ray.direction.length() - 1.0).abs() < 1e-5);
    let along = (point - ray.origin).dot(ray.direction);
    let closest = ray.origin + ray.direction * along;
    assert!((closest - point).length() < 1e-3, "{ray:?} misses {point}");

    // An orthographic camera casts parallel rays from different origins.
    w.edit_gameplay(|edit| {
        edit.set_component(
            cam,
            ComponentValue::Camera(Camera {
                projection: Projection::Orthographic,
                ortho_height: 4.0,
                ..Camera::default()
            }),
        )
    })
    .unwrap();
    assert_eq!(w.step().status, Status::Ok);
    let a = w.screen_ray(800, 600, 0.0, 0.0).unwrap();
    let b = w.screen_ray(800, 600, 800.0, 600.0).unwrap();
    assert!((a.direction - Vec3::NEG_Z).length() < 1e-5, "{a:?}");
    assert!((b.direction - Vec3::NEG_Z).length() < 1e-5, "{b:?}");
    // Top-left of a 4 m high, 4:3 view: (-8/3, 2).
    assert!(
        (a.origin.x + 8.0 / 3.0).abs() < 1e-4 && (a.origin.y - 2.0).abs() < 1e-4,
        "{a:?}"
    );
    assert!(
        (b.origin.x - 8.0 / 3.0).abs() < 1e-4 && (b.origin.y + 2.0).abs() < 1e-4,
        "{b:?}"
    );
}
