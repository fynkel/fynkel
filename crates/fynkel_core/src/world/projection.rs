//! Projected world anchors (M3-13, blueprint §10 "Projected world
//! labels"): a bounded per-frame view that maps points attached to
//! entities into a viewport's pixels, from the same camera and the same
//! interpolated poses the frame is rendered with. The host puts screen-space
//! widgets (Flutter labels) at the answers; nothing here draws.
//!
//! The camera is the one `extract` picks (the enabled `fynkel.Camera` with
//! the highest priority, ties to the earliest record) and the matrices are
//! the renderer's ([`view_matrix`], [`projection_matrix`]), so a label sits
//! where the mesh is drawn. Depth occlusion is an explicit policy: an
//! anchor with a collider `mask` casts a ray from the camera to its point
//! and is `OCCLUDED` when a collider on those layers is hit first; without
//! a mask a label is never hidden by geometry (a screen overlay is not
//! automatically behind the 3D scene).

use fynkel_contracts::{ComponentId, DiagnosticCode, Entity};
use fynkel_physics::{ColliderShape, Hit, QueryKind, QuerySpec};
use glam::{Mat4, Vec3, Vec4};

use super::{Diagnostic, World};
use crate::command::Lane;
use crate::components::{Camera, Projection};
use crate::schema::{ProjectedPointRecord, ProjectionAnchorRecord};

/// The most anchors one request projects; the rest come back
/// [`PROJECTED_DROPPED`] with a `TOO_MANY_ANCHORS` diagnostic (blueprint
/// §10: bound the number of anchored widgets).
pub const MAX_PROJECTION_ANCHORS: usize = 64;

/// `flags` bits of a `fynkel.rec.ProjectedPoint`.
/// The point is in front of the near plane; `x`/`y` are meaningful.
pub const PROJECTED_IN_FRONT: u32 = 1;
/// In front and inside the viewport rectangle.
pub const PROJECTED_ON_SCREEN: u32 = 2;
/// A collider on the anchor's `mask` sits between the camera and the point.
pub const PROJECTED_OCCLUDED: u32 = 4;
/// The entity is dead or has no transform.
pub const PROJECTED_MISSING: u32 = 8;
/// The world has no enabled camera; nothing was projected.
pub const PROJECTED_NO_CAMERA: u32 = 16;
/// Beyond the first `MAX_PROJECTION_ANCHORS` of the request.
pub const PROJECTED_DROPPED: u32 = 32;

/// `PROJECTED_IN_FRONT | PROJECTED_ON_SCREEN` and not occluded: the label
/// should be shown.
pub const PROJECTED_VISIBLE: u32 = PROJECTED_IN_FRONT | PROJECTED_ON_SCREEN;

/// What one request projected from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProjectionInfo {
    /// The camera entity the frame is rendered from (`None`: no camera,
    /// every point is `PROJECTED_NO_CAMERA`).
    pub camera: Option<Entity>,
    /// Anchors projected (at most `MAX_PROJECTION_ANCHORS`).
    pub projected: u32,
    /// Anchors beyond the bound.
    pub dropped: u32,
}

/// The renderer's view matrix of a camera entity's world matrix (rotation
/// and translation only — a scaled camera does not scale the view).
pub fn view_matrix(world: &Mat4) -> Mat4 {
    let (_, rotation, translation) = world.to_scale_rotation_translation();
    Mat4::from_rotation_translation(rotation, translation).inverse()
}

/// The renderer's projection matrix of a `fynkel.Camera` at an aspect
/// ratio (right-handed, clip depth 0..=1).
pub fn projection_matrix(camera: &Camera, aspect: f32) -> Mat4 {
    match camera.projection {
        Projection::Perspective => glam::camera::rh::proj::directx::perspective(
            camera.fov_y,
            aspect,
            camera.near,
            camera.far,
        ),
        Projection::Orthographic => {
            let h = camera.ortho_height * 0.5;
            let w = h * aspect;
            glam::camera::rh::proj::directx::orthographic(-w, w, -h, h, camera.near, camera.far)
        }
    }
}

impl World {
    /// The camera `extract` renders from, with its interpolated world
    /// matrix for this frame: the enabled `Camera` with the highest
    /// priority, ties to the earliest entity.
    fn frame_camera(
        &self,
        alpha: f32,
        memo: &mut fynkel_contracts::FastMap<Entity, Mat4>,
    ) -> Option<(Entity, Mat4, Camera)> {
        let mut best: Option<(Entity, Camera)> = None;
        for e in self.gameplay.ecs.entities() {
            if !self.gameplay.ecs.has::<super::WorldTransform>(e) {
                continue;
            }
            if let Ok(c) = self.gameplay.ecs.copy::<Camera>(e) {
                let better = match &best {
                    Some((_, cur)) => c.priority > cur.priority,
                    None => true,
                };
                if better {
                    best = Some((e, c));
                }
            }
        }
        let (e, c) = best?;
        let world = self.resolve_extract(e, alpha, memo);
        Some((e, world, c))
    }

    /// Projects `anchors` into a `width` × `height` viewport with the
    /// frame's camera and poses (the clock's current interpolation, like
    /// `extract`), one `ProjectedPointRecord` per anchor in order. At most
    /// [`MAX_PROJECTION_ANCHORS`] are projected; the rest are
    /// `PROJECTED_DROPPED` and a `TOO_MANY_ANCHORS` diagnostic is reported.
    pub fn project(
        &mut self,
        width: u32,
        height: u32,
        anchors: &[ProjectionAnchorRecord],
        out: &mut Vec<ProjectedPointRecord>,
    ) -> ProjectionInfo {
        out.clear();
        let alpha = self.clock.alpha();
        let mut memo: fynkel_contracts::FastMap<Entity, Mat4> = Default::default();
        let camera = self.frame_camera(alpha, &mut memo);
        let take = anchors.len().min(MAX_PROJECTION_ANCHORS);
        let dropped = (anchors.len() - take) as u32;
        let info = ProjectionInfo {
            camera: camera.map(|c| c.0),
            projected: take as u32,
            dropped,
        };
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        let matrices = camera.map(|(_, world, c)| {
            let view = view_matrix(&world);
            let eye = world.transform_point3(Vec3::ZERO);
            (projection_matrix(&c, w / h) * view, view, eye, c.near)
        });
        let mut hits: Vec<Hit> = Vec::new();
        for (i, a) in anchors.iter().enumerate() {
            let mut rec = ProjectedPointRecord {
                entity: a.entity,
                x: 0.0,
                y: 0.0,
                depth: 0.0,
                flags: 0,
            };
            if i >= take {
                rec.flags = PROJECTED_DROPPED;
                out.push(rec);
                continue;
            }
            let Some((view_proj, view, eye, near)) = matrices else {
                rec.flags = PROJECTED_NO_CAMERA;
                out.push(rec);
                continue;
            };
            if !self.gameplay.ecs.contains(a.entity)
                || !self.gameplay.ecs.has::<super::WorldTransform>(a.entity)
            {
                rec.flags = PROJECTED_MISSING;
                out.push(rec);
                continue;
            }
            let world = self.resolve_extract(a.entity, alpha, &mut memo);
            let point = world.transform_point3(a.offset);
            // Depth along the view axis (the camera looks down -Z).
            rec.depth = -view.transform_point3(point).z;
            let clip = view_proj * Vec4::new(point.x, point.y, point.z, 1.0);
            if clip.w > 0.0 && rec.depth >= near {
                rec.flags |= PROJECTED_IN_FRONT;
                let ndc_x = clip.x / clip.w;
                let ndc_y = clip.y / clip.w;
                rec.x = (ndc_x * 0.5 + 0.5) * w;
                rec.y = (0.5 - ndc_y * 0.5) * h;
                if rec.x >= 0.0 && rec.x <= w && rec.y >= 0.0 && rec.y <= h {
                    rec.flags |= PROJECTED_ON_SCREEN;
                }
                if a.mask != 0 {
                    let to = point - eye;
                    let distance = to.length();
                    if distance > 0.0 {
                        hits.clear();
                        self.physics.query(
                            &QuerySpec {
                                kind: QueryKind::Raycast,
                                shape: ColliderShape::Box,
                                size: Vec3::ONE,
                                origin: eye,
                                rotation: glam::Quat::IDENTITY,
                                direction: to / distance,
                                max_distance: distance,
                                mask: a.mask,
                                exclude: Some(a.entity),
                                max_hits: 1,
                                hit_sensors: false,
                            },
                            &mut hits,
                        );
                        if hits.first().is_some_and(|hit| hit.distance < distance) {
                            rec.flags |= PROJECTED_OCCLUDED;
                        }
                    }
                }
            }
            out.push(rec);
        }
        if dropped > 0 {
            self.publication.diagnostics.push(Diagnostic {
                command_index: u32::MAX,
                lane: Lane::Presentation,
                code: DiagnosticCode::TOO_MANY_ANCHORS,
                entity: Entity::NULL,
                component: ComponentId::NONE,
                message: format!(
                    "FYN-R0012: projection request with {} anchors; at most {} are projected, {} dropped",
                    anchors.len(),
                    MAX_PROJECTION_ANCHORS,
                    dropped
                ),
            });
        }
        info
    }
}
