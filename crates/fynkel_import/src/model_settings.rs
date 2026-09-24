//! A model's import settings (Unity's Model tab; docs/unity-parity.md
//! Tier 2), read from the sidecar and applied by both scene importers
//! (glTF and FBX) before their parts get ids:
//!
//! - `scaleFactor` (number `> 0`, default 1): baked in, as Unity does —
//!   vertex positions and bounds, every node's translation, every joint's
//!   inverse bind and rest translation, and every translation key are
//!   multiplied by it; rotations and scales stay. A uniform scale `S`
//!   conjugates every node's matrix (`S · M · S⁻¹` scales its translation
//!   only), so the model is drawn `S` times its size and skinning and
//!   animation still line up.
//! - `normals` (`"import"`, the default, or `"calculate"`) and
//!   `smoothingAngle` (degrees in `[0, 180]`, default 60): calculated
//!   normals are the face normals of the triangles that meet at a
//!   position within the angle of each other, weighted by each one's
//!   angle at that corner (so a quad split in two counts once) — flat edges above
//!   it, smooth ones below; a vertex splits where its corners disagree.
//!   The tangents are cleared, so the cook derives them again
//!   ([`crate::tangents`]).
//! - `importAnimation` (bool, default true): off, the file's clips are not
//!   imported (no `anim/<a>` parts).
//!
//! A bad value is an `FYN-A0104` error at its pointer: the model is not
//! imported rather than imported at a size or with normals nobody asked
//! for.

use std::collections::HashMap;

use fynkel_contracts::{Code, Diagnostic};
use fynkel_cooked::payload::{Vertex, ANIM_PROPERTY_TRANSLATION};
use fynkel_document::asset::Meta;
use glam::Vec3;
use serde_json::Value;

use crate::intermediate::{AnimationData, MeshData, SkinData};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelSettings {
    pub scale_factor: f32,
    /// `Some(degrees)` when the normals are calculated.
    pub calculate_normals: Option<f32>,
    pub import_animation: bool,
}

impl Default for ModelSettings {
    fn default() -> Self {
        ModelSettings {
            scale_factor: 1.0,
            calculate_normals: None,
            import_animation: true,
        }
    }
}

impl ModelSettings {
    pub fn from_meta(meta: &Meta) -> Result<ModelSettings, Diagnostic> {
        let bad = |key: &str, what: &str| {
            Diagnostic::error(
                Code::IMPORT_INVALID_DATA,
                format!("settings.{key} must be {what}"),
            )
            .with_pointer(format!("/settings/{key}"))
        };
        let s = &meta.settings;
        let mut out = ModelSettings::default();
        if let Some(n) = s.get("scaleFactor") {
            match n.as_f64() {
                Some(v) if v.is_finite() && v > 0.0 => out.scale_factor = v as f32,
                _ => return Err(bad("scaleFactor", "a number greater than 0")),
            }
        }
        let angle = match s.get("smoothingAngle") {
            None => 60.0,
            Some(n) => match n.as_f64() {
                Some(v) if (0.0..=180.0).contains(&v) => v as f32,
                _ => return Err(bad("smoothingAngle", "degrees from 0 to 180")),
            },
        };
        match s.get("normals").map(|n| n.as_str()) {
            None | Some(Some("import")) => {}
            Some(Some("calculate")) => out.calculate_normals = Some(angle),
            Some(_) => return Err(bad("normals", "\"import\" or \"calculate\"")),
        }
        if let Some(n) = s.get("importAnimation") {
            out.import_animation = n
                .as_bool()
                .ok_or_else(|| bad("importAnimation", "true or false"))?;
        }
        Ok(out)
    }

    /// Applies the settings to what an importer produced.
    pub(crate) fn apply(
        &self,
        meshes: &mut [MeshData],
        skins: &mut [SkinData],
        animations: &mut Vec<AnimationData>,
        prefab: &mut Value,
    ) {
        if !self.import_animation {
            animations.clear();
        }
        if let Some(angle) = self.calculate_normals {
            for m in meshes.iter_mut() {
                calculate_normals(m, angle);
            }
        }
        let s = self.scale_factor;
        if s != 1.0 {
            for m in meshes.iter_mut() {
                for v in &mut m.vertices {
                    v.position = (Vec3::from(v.position) * s).into();
                }
                m.bounds_min = (Vec3::from(m.bounds_min) * s).into();
                m.bounds_max = (Vec3::from(m.bounds_max) * s).into();
            }
            for skin in skins.iter_mut() {
                for j in &mut skin.payload.joints {
                    for c in &mut j.inverse_bind[12..15] {
                        *c *= s;
                    }
                    j.rest_translation = (Vec3::from(j.rest_translation) * s).into();
                }
            }
            for a in animations.iter_mut() {
                for c in &mut a.payload.channels {
                    if c.property == ANIM_PROPERTY_TRANSLATION {
                        for v in &mut c.values {
                            *v *= s;
                        }
                    }
                }
            }
            scale_prefab(prefab, s);
        }
    }
}

/// Every `fynkel.Transform`'s position in the prefab body, times `s`.
fn scale_prefab(prefab: &mut Value, s: f32) {
    let Some(entities) = prefab.get_mut("entities").and_then(Value::as_object_mut) else {
        return;
    };
    for e in entities.values_mut() {
        let Some(components) = e.get_mut("components").and_then(Value::as_object_mut) else {
            continue;
        };
        for c in components.values_mut() {
            if c.get("type").and_then(Value::as_str) != Some("fynkel.Transform") {
                continue;
            }
            if let Some(p) = c
                .pointer_mut("/data/position")
                .and_then(Value::as_array_mut)
            {
                for x in p.iter_mut() {
                    if let Some(v) = x.as_f64() {
                        *x = crate::graph::num(v as f32 * s);
                    }
                }
            }
        }
    }
}

/// Normals from the triangles (see the module doc); vertices split where
/// the corners that share one disagree.
fn calculate_normals(mesh: &mut MeshData, angle_degrees: f32) {
    let n = mesh.vertices.len();
    let tris: Vec<[usize; 3]> = mesh
        .indices
        .chunks_exact(3)
        .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
        .filter(|t| t.iter().all(|i| *i < n))
        .collect();
    if tris.is_empty() {
        return;
    }
    let pos = |i: usize| Vec3::from(mesh.vertices[i].position);
    let unit: Vec<Vec3> = tris
        .iter()
        .map(|t| {
            (pos(t[1]) - pos(t[0]))
                .cross(pos(t[2]) - pos(t[0]))
                .normalize_or_zero()
        })
        .collect();
    // The faces meeting at each position (exactly equal positions: the
    // same point split for its UVs or normals), each with its angle there.
    let key = |i: usize| mesh.vertices[i].position.map(f32::to_bits);
    let mut at: HashMap<[u32; 3], Vec<(usize, f32)>> = HashMap::new();
    for (f, t) in tris.iter().enumerate() {
        for c in 0..3 {
            let p = pos(t[c]);
            let angle = (pos(t[(c + 1) % 3]) - p).angle_between(pos(t[(c + 2) % 3]) - p);
            let faces = at.entry(key(t[c])).or_default();
            if faces.last().map(|(g, _)| *g) != Some(f) && angle.is_finite() {
                faces.push((f, angle));
            }
        }
    }
    let cos = angle_degrees.to_radians().cos() - 1e-6;
    let mut vertices: Vec<Vertex> = Vec::with_capacity(n);
    let mut skin = Vec::new();
    let mut seen: HashMap<(usize, [u32; 3]), u32> = HashMap::new();
    let mut indices = Vec::with_capacity(tris.len() * 3);
    for (f, t) in tris.iter().enumerate() {
        for &i in t {
            let mut sum = Vec3::ZERO;
            for &(g, angle) in at.get(&key(i)).into_iter().flatten() {
                if g == f || unit[f].dot(unit[g]) >= cos {
                    sum += unit[g] * angle;
                }
            }
            let normal = sum.try_normalize().unwrap_or(unit[f]);
            let normal: [f32; 3] = normal.into();
            let index = *seen
                .entry((i, normal.map(f32::to_bits)))
                .or_insert_with(|| {
                    let mut v = mesh.vertices[i];
                    v.normal = normal;
                    v.tangent = [0.0; 4];
                    vertices.push(v);
                    if !mesh.skin.is_empty() {
                        skin.push(mesh.skin[i]);
                    }
                    (vertices.len() - 1) as u32
                });
            indices.push(index);
        }
    }
    mesh.vertices = vertices;
    mesh.indices = indices;
    mesh.skin = skin;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad_fold(fold: f32) -> MeshData {
        // Two triangles sharing the edge x = 0, the second folded by `fold` radians.
        let v = |p: [f32; 3]| Vertex {
            position: p,
            normal: [0.0, 0.0, 1.0],
            ..Default::default()
        };
        let far = [fold.cos(), 0.0, -fold.sin()];
        MeshData {
            sub_id: "mesh/0".into(),
            name: "fold".into(),
            material: None,
            vertices: vec![
                v([-1.0, 0.0, 0.0]),
                v([0.0, -1.0, 0.0]),
                v([0.0, 1.0, 0.0]),
                v(far),
            ],
            indices: vec![0, 1, 2, 2, 1, 3],
            bounds_min: [-1.0, -1.0, -1.0],
            bounds_max: [1.0, 1.0, 0.0],
            skin: Vec::new(),
        }
    }

    #[test]
    fn a_fold_under_the_angle_is_smooth_and_over_it_is_split() {
        let mut gentle = quad_fold(20f32.to_radians());
        calculate_normals(&mut gentle, 60.0);
        assert_eq!(gentle.vertices.len(), 4, "the shared edge stays shared");
        let edge = Vec3::from(gentle.vertices[1].normal);
        assert!((edge.length() - 1.0).abs() < 1e-5);
        assert!(edge.z > 0.9 && edge.x > 0.0, "bent halfway: {edge}");

        let mut sharp = quad_fold(90f32.to_radians());
        calculate_normals(&mut sharp, 60.0);
        assert_eq!(sharp.vertices.len(), 6, "the edge splits in two");
        assert_eq!(sharp.indices.len(), 6);
        let first: Vec3 = sharp.vertices[sharp.indices[0] as usize].normal.into();
        let last: Vec3 = sharp.vertices[sharp.indices[5] as usize].normal.into();
        assert!((first - Vec3::Z).length() < 1e-5, "{first}");
        assert!((last - Vec3::X).length() < 1e-5, "{last}");
    }
}
