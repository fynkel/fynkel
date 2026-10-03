//! FBX importer (`fynkel.fbx`, blueprint §19.2d item 19b) over `ufbx`.
//!
//! The same intermediate as the glTF importer, from the files most rigs
//! arrive in (Mixamo, Blender's exporter): one [`MeshData`] per mesh
//! (`mesh/<n>`, or `mesh/<n>/<p>` per material part when a mesh uses
//! several materials), a `fynkel.pbr@1` document per material
//! (`material/<m>`), every embedded texture a material binds as its
//! encoded bytes (`image/<t>`), a [`SkinData`] per skin deformer
//! (`skin/<s>`: the clusters in the file's order, joints by bone name,
//! `geometry_to_bone` as the inverse bind matrix), every animation stack
//! that animates something as an [`AnimationData`] (`anim/<a>` in import
//! order, named after the stack) and the node hierarchy as a prefab
//! (`prefab`) through [`crate::graph`], so a rig looks the same to the
//! runtime whichever file it came from.
//!
//! `ufbx` does the format work: binary and ASCII FBX of every version,
//! axis and unit conversion to metres, Y up (`SpaceConversion::
//! ModifyGeometry`: the conversion goes into the vertices and root
//! transforms, so a centimetre Mixamo file and a metre Blender file give
//! the same numbers), helper nodes for geometry transforms and
//! non-standard inherit modes, missing normals, and the animation curves
//! sampled at the file's frame rate into per-node translation/rotation/
//! scale keys (`bake_anim`, keys reduced). Faces are triangulated; the
//! per-corner attributes are welded back into an indexed mesh; UVs are
//! flipped to the glTF convention (`v' = 1 - v`) the renderer samples.
//!
//! What it refuses, by name: a texture that is not embedded (unless the
//! sidecar's `allowExternalFiles` allows a plain relative path), images
//! other than PNG and JPEG, a skin cluster without a bone, duplicate
//! joint names in a skin, anything past the [`Limits`]. What it skips
//! with a warning: cameras, lights, blend shapes, curves and surfaces,
//! layered/procedural textures, roughness and metalness textures (the
//! PBR material wants them packed), opacity textures, UV transforms,
//! non-repeat wraps. Empty animation stacks (Mixamo's default "Take 001")
//! are skipped silently.

use std::collections::HashMap;
use std::path::Path;

use fynkel_contracts::Code;
use fynkel_cooked::payload::{
    AnimationChannel, AnimationPayload, SkinJoint, SkinPayload, SkinWeight,
    ANIM_INTERPOLATION_LINEAR, ANIM_PROPERTY_ROTATION, ANIM_PROPERTY_SCALE,
    ANIM_PROPERTY_TRANSLATION,
};
use fynkel_document::asset::Meta;
use serde_json::{json, Value};

use crate::graph::{
    check_channel, file_stem, image_ref, material_header, num, nums, prefab_document, shown_file,
    sniff_mime, split_dual_use_images, MeshSubs, Produced, Report, SourceGraph, SourceNode,
};
use crate::intermediate::{
    AnimationData, ColorSpace, ImageData, MaterialData, MeshData, SkinData, Vertex,
};
use crate::subkey::Keys;
use crate::{read_source, Imported, Limits};

pub const IMPORTER: &str = fynkel_document::asset::FBX_IMPORTER;
/// 2: sub-asset keys are the parts' names once the sidecar is at
/// `formatVersion` 2 (`subkey`). 3: `settings.materials` remaps embedded
/// materials in the model's hierarchy (`graph::remap_materials`). 4: the
/// hierarchy has one root (ADR-0022; a new root named after the file over
/// several top-level nodes).
pub const IMPORTER_VERSION: u32 = 5;

struct Ctx<'a> {
    asset_id: uuid::Uuid,
    path: &'a Path,
    report: Report,
    limits: &'a Limits,
    allow_external_files: bool,
    /// Textures already warned about (one warning per texture, not per use).
    warned_textures: Vec<u32>,
    /// `(position, stack name)` of every kept clip, for its name key.
    anim_names: Vec<(String, Option<String>)>,
}

impl Ctx<'_> {
    fn error(&mut self, code: Code, msg: impl Into<String>) {
        self.report.error(code, msg);
    }

    fn warn(&mut self, code: Code, msg: impl Into<String>) {
        self.report.warn(code, msg);
    }
}

fn load_opts() -> ufbx::LoadOpts<'static> {
    ufbx::LoadOpts {
        target_axes: ufbx::CoordinateAxes::right_handed_y_up(),
        target_unit_meters: 1.0,
        space_conversion: ufbx::SpaceConversion::ModifyGeometry,
        geometry_transform_handling: ufbx::GeometryTransformHandling::HelperNodes,
        inherit_mode_handling: ufbx::InheritModeHandling::HelperNodes,
        generate_missing_normals: true,
        normalize_normals: true,
        normalize_tangents: true,
        clean_skin_weights: true,
        // Nothing is read besides the bytes handed in; embedded media
        // stays in the scene, external files are our decision.
        load_external_files: false,
        ignore_missing_external_files: true,
        ..Default::default()
    }
}

pub fn import(path: &Path, meta: &Meta, limits: &Limits) -> Imported {
    let mut ctx = Ctx {
        asset_id: meta.id(),
        path,
        report: Report::new(path),
        limits,
        allow_external_files: meta
            .settings
            .get("allowExternalFiles")
            .and_then(|n| n.as_bool())
            .unwrap_or(false),
        warned_textures: Vec::new(),
        anim_names: Vec::new(),
    };
    let fail = |ctx: Ctx| ctx.report.fail(meta);

    let bytes = match read_source(path, limits) {
        Ok(b) => b,
        Err(d) => {
            ctx.report.diagnostics.push(d);
            return fail(ctx);
        }
    };
    let scene = match ufbx::load_memory(&bytes, load_opts()) {
        Ok(s) => s,
        Err(e) => {
            let info = e.info();
            let detail = if info.is_empty() {
                e.description.to_string()
            } else {
                format!("{} ({info})", &*e.description)
            };
            ctx.error(
                Code::IMPORT_UNREADABLE,
                format!("not an FBX file: {detail}"),
            );
            return fail(ctx);
        }
    };
    drop(bytes);

    // Features the intermediate has no shape for, once each.
    let skipped: [(usize, &str); 5] = [
        (scene.cameras.len(), "camera(s)"),
        (scene.lights.len(), "light(s)"),
        (
            scene.blend_deformers.len(),
            "blend shape deformer(s) (morph targets are not imported)",
        ),
        (
            scene.nurbs_curves.len() + scene.line_curves.len(),
            "curve(s)",
        ),
        (
            scene.nurbs_surfaces.len() + scene.procedural_geometries.len(),
            "surface(s) (only polygon meshes are imported)",
        ),
    ];
    for (count, what) in skipped {
        if count > 0 {
            ctx.warn(Code::IMPORT_SKIPPED, format!("{count} {what} skipped"));
        }
    }

    // Skins first: the meshes' weights index their clusters.
    let Ok(skins) = import_skins(&mut ctx, &scene) else {
        return fail(ctx);
    };

    // Materials and the textures they bind → documents and images.
    let mut images: Vec<ImageData> = Vec::new();
    let mut materials = Vec::with_capacity(scene.materials.len());
    for material in scene.materials.iter() {
        let index = material.element.typed_id;
        let sub_id = format!("material/{index}");
        let name = named(&material.element.name, "material", index);
        let Some(document) =
            material_document(&mut ctx, material, &sub_id, &name, meta, &mut images)
        else {
            return fail(ctx);
        };
        materials.push(MaterialData {
            sub_id,
            name,
            document,
        });
    }
    split_dual_use_images(&mut materials, &mut images);

    // Meshes → one MeshData per material part.
    let mut meshes = Vec::new();
    let mut mesh_subs: Vec<MeshSubs> = vec![Vec::new(); scene.meshes.len()];
    for mesh in scene.meshes.iter() {
        let index = mesh.element.typed_id as usize;
        let mesh_name = named(&mesh.element.name, "mesh", index as u32);
        let parts: Vec<&ufbx::MeshPart> = mesh
            .material_parts
            .iter()
            .filter(|p| p.num_triangles > 0)
            .collect();
        let dropped = mesh.num_point_faces + mesh.num_line_faces;
        if dropped > 0 {
            ctx.warn(
                Code::IMPORT_SKIPPED,
                format!(
                    "mesh/{index} ({mesh_name}): {dropped} point/line face(s) skipped; only polygons are imported"
                ),
            );
        }
        if parts.is_empty() {
            ctx.warn(
                Code::IMPORT_SKIPPED,
                format!("mesh/{index} ({mesh_name}): no polygons; skipped"),
            );
            continue;
        }
        let single = mesh.material_parts.len() == 1;
        for part in parts {
            let sub_id = if single {
                format!("mesh/{index}")
            } else {
                format!("mesh/{index}/{}", part.index)
            };
            let name = if single {
                mesh_name.clone()
            } else {
                format!("{mesh_name}[{}]", part.index)
            };
            match part_mesh(&mut ctx, mesh, part, &skins, &sub_id, &name) {
                Some(m) => {
                    mesh_subs[index].push((sub_id, name));
                    meshes.push(m);
                }
                None => return fail(ctx),
            }
        }
    }

    // Animation stacks → clips.
    let Ok(animations) = import_animations(&mut ctx, &scene) else {
        return fail(ctx);
    };

    // Every part's source name, for its name key (`subkey`).
    let mut keys = Keys::default();
    let element = |e: &ufbx::Element| {
        (
            e.typed_id.to_string(),
            (!e.name.is_empty()).then(|| e.name.to_string()),
        )
    };
    keys.kind(
        "image",
        scene.textures.iter().map(|t| {
            let stem = Path::new(relative_or_name(t))
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty());
            let (position, name) = element(&t.element);
            (position, stem.or(name))
        }),
    );
    keys.kind(
        "material",
        scene.materials.iter().map(|m| element(&m.element)),
    );
    keys.kind("mesh", scene.meshes.iter().map(|m| element(&m.element)));
    keys.kind(
        "skin",
        scene.skin_deformers.iter().map(|s| element(&s.element)),
    );
    keys.kind("anim", std::mem::take(&mut ctx.anim_names));

    // Nodes → prefab.
    let graph = source_graph(&scene, &file_stem(path), &mesh_subs);
    let prefab = prefab_document(
        &mut ctx.report,
        limits,
        &graph,
        &mesh_subs,
        &meshes,
        &skins,
        meta,
    );
    let Some((prefab, prefab_ids)) = prefab else {
        return fail(ctx);
    };
    if ctx.report.has_errors() {
        return fail(ctx);
    }

    crate::graph::finish(
        ctx.report,
        meta,
        path,
        IMPORTER,
        IMPORTER_VERSION,
        Produced {
            meshes,
            materials,
            images,
            skins,
            animations,
            prefab,
            prefab_ids,
            keys,
        },
    )
}

/// An element's name, or `<what><index>` for an unnamed one.
fn named(name: &str, what: &str, index: u32) -> String {
    if name.is_empty() {
        format!("{what}{index}")
    } else {
        name.to_string()
    }
}

fn node_name(node: &ufbx::Node) -> String {
    named(&node.element.name, "node", node.element.typed_id)
}

fn v3(v: ufbx::Vec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

fn quat(q: ufbx::Quat) -> [f32; 4] {
    [q.x as f32, q.y as f32, q.z as f32, q.w as f32]
}

/// A ufbx affine matrix as the column-major 4×4 the payload stores.
fn mat4(m: &ufbx::Matrix) -> [f32; 16] {
    [
        m.m00 as f32,
        m.m10 as f32,
        m.m20 as f32,
        0.0,
        m.m01 as f32,
        m.m11 as f32,
        m.m21 as f32,
        0.0,
        m.m02 as f32,
        m.m12 as f32,
        m.m22 as f32,
        0.0,
        m.m03 as f32,
        m.m13 as f32,
        m.m23 as f32,
        1.0,
    ]
}

/// The node hierarchy as a [`SourceGraph`]: every node by its ufbx index
/// (the scene root at 0 is never reached; its children are the roots),
/// with the mesh it instances and that mesh's first skin deformer.
fn source_graph(scene: &ufbx::Scene, stem: &str, mesh_subs: &[MeshSubs]) -> SourceGraph {
    let nodes = scene
        .nodes
        .iter()
        .map(|node| {
            let t = &node.local_transform;
            let mesh = node
                .mesh
                .as_ref()
                .map(|m| m.element.typed_id as usize)
                .filter(|m| !mesh_subs[*m].is_empty());
            let skin = node
                .mesh
                .as_ref()
                .and_then(|m| m.skin_deformers.first())
                .map(|s| s.element.typed_id as usize);
            SourceNode {
                index: node.element.typed_id as usize,
                name: node_name(node),
                named: !node.element.name.is_empty(),
                translation: v3(t.translation),
                rotation: quat(t.rotation),
                scale: v3(t.scale),
                children: node
                    .children
                    .iter()
                    .map(|c| c.element.typed_id as usize)
                    .collect(),
                mesh,
                skin,
            }
        })
        .collect();
    SourceGraph {
        name: stem.to_string(),
        nodes,
        roots: scene
            .root_node
            .children
            .iter()
            .map(|c| c.element.typed_id as usize)
            .collect(),
    }
}

/// One part of a mesh (the faces of one material slot) as an indexed
/// triangle mesh: every polygon triangulated, corners with identical
/// attributes welded into one vertex, skin weights from the mesh's first
/// skin deformer. `None` = an error was recorded.
fn part_mesh(
    ctx: &mut Ctx,
    mesh: &ufbx::Mesh,
    part: &ufbx::MeshPart,
    skins: &[SkinData],
    sub_id: &str,
    name: &str,
) -> Option<MeshData> {
    let skin = mesh.skin_deformers.first();
    if mesh.skin_deformers.len() > 1 {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!(
                "{sub_id} ({name}): {} skin deformers; only the first ({}) is applied",
                mesh.skin_deformers.len(),
                skins
                    .get(skin.map(|s| s.element.typed_id as usize).unwrap_or(0))
                    .map(|s| s.sub_id.as_str())
                    .unwrap_or("?")
            ),
        );
    }
    let uv1 = mesh.uv_sets.get(1).map(|set| &set.vertex_uv);
    let has_tangents = mesh.vertex_tangent.exists && mesh.vertex_bitangent.exists;

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut skin_weights: Vec<SkinWeight> = Vec::new();
    let mut indices: Vec<u32> = Vec::with_capacity(part.num_triangles * 3);
    let mut welded: HashMap<[u32; 16], u32> = HashMap::new();
    let mut tri = vec![0u32; mesh.max_face_triangles.max(1) * 3];
    for &face_index in part.face_indices.iter() {
        let face = mesh.faces[face_index as usize];
        if face.num_indices < 3 {
            continue;
        }
        let triangles = mesh.triangulate_face(&mut tri, face) as usize;
        for &corner in &tri[..triangles * 3] {
            let corner = corner as usize;
            let vertex_index = mesh.vertex_indices[corner];
            let position = v3(mesh.vertex_position[corner]);
            let normal = if mesh.vertex_normal.exists {
                v3(mesh.vertex_normal[corner])
            } else {
                [0.0, 0.0, 1.0]
            };
            // FBX UVs have their origin at the bottom-left; the
            // renderer samples glTF's top-left convention.
            let uv = |set: &ufbx::VertexVec2| -> [f32; 2] {
                if set.exists {
                    let uv = set[corner];
                    [uv.x as f32, 1.0 - uv.y as f32]
                } else {
                    [0.0; 2]
                }
            };
            let uv0 = uv(&mesh.vertex_uv);
            let uv1 = uv1.map(uv).unwrap_or([0.0; 2]);
            let tangent = if has_tangents {
                let t = glam::Vec3::from(v3(mesh.vertex_tangent[corner]));
                let b = glam::Vec3::from(v3(mesh.vertex_bitangent[corner]));
                let n = glam::Vec3::from(normal);
                let sign = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
                [t.x, t.y, t.z, sign]
            } else {
                [0.0; 4]
            };
            let color = if mesh.vertex_color.exists {
                let c = mesh.vertex_color[corner];
                [c.x as f32, c.y as f32, c.z as f32, c.w as f32]
            } else {
                [1.0; 4]
            };
            let mut key = [0u32; 16];
            key[0] = vertex_index;
            for (k, x) in normal
                .iter()
                .chain(&uv0)
                .chain(&uv1)
                .chain(&tangent)
                .chain(&color)
                .enumerate()
            {
                key[1 + k] = x.to_bits();
            }
            let index = match welded.get(&key) {
                Some(i) => *i,
                None => {
                    let i = vertices.len() as u32;
                    if vertices.len() >= ctx.limits.max_vertices {
                        ctx.error(
                            Code::IMPORT_LIMIT,
                            format!(
                                "{sub_id} ({name}): more than {} vertices; that is the limit",
                                ctx.limits.max_vertices
                            ),
                        );
                        return None;
                    }
                    welded.insert(key, i);
                    vertices.push(Vertex {
                        position,
                        normal,
                        tangent,
                        uv0,
                        uv1,
                        color,
                    });
                    if let Some(skin) = skin {
                        skin_weights.push(skin_weight(skin, vertex_index as usize));
                    }
                    i
                }
            };
            indices.push(index);
        }
    }

    let mut bounds_min = [f32::MAX; 3];
    let mut bounds_max = [f32::MIN; 3];
    for v in &vertices {
        for k in 0..3 {
            bounds_min[k] = bounds_min[k].min(v.position[k]);
            bounds_max[k] = bounds_max[k].max(v.position[k]);
        }
    }
    if vertices.is_empty() {
        bounds_min = [0.0; 3];
        bounds_max = [0.0; 3];
    }
    Some(MeshData {
        sub_id: sub_id.to_string(),
        name: name.to_string(),
        material: mesh
            .materials
            .get(part.index as usize)
            .map(|m| format!("material/{}", m.element.typed_id)),
        vertices,
        indices,
        bounds_min,
        bounds_max,
        skin: skin_weights,
    })
}

/// A vertex's four strongest cluster weights, normalized (a vertex bound
/// to nothing follows joint 0).
fn skin_weight(skin: &ufbx::SkinDeformer, vertex: usize) -> SkinWeight {
    let Some(sv) = skin.vertices.get(vertex) else {
        return SkinWeight {
            joints: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
        };
    };
    let begin = sv.weight_begin as usize;
    let weights: &[ufbx::SkinWeight] = &skin.weights;
    let mut all: Vec<(u32, f32)> = weights[begin..begin + sv.num_weights as usize]
        .iter()
        .map(|w| (w.cluster_index, w.weight as f32))
        .filter(|(_, w)| *w > 0.0)
        .collect();
    all.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    all.truncate(4);
    let sum: f32 = all.iter().map(|(_, w)| w).sum();
    if sum <= 0.0 {
        return SkinWeight {
            joints: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
        };
    }
    let mut joints = [0u16; 4];
    let mut weights = [0.0f32; 4];
    for (k, (j, w)) in all.iter().enumerate() {
        joints[k] = *j as u16;
        weights[k] = w / sum;
    }
    SkinWeight { joints, weights }
}

/// One [`SkinData`] per skin deformer (`skin/<s>`): the clusters in the
/// file's order (what the weights index), each joint named after its
/// bone node with its parent's index when the parent is a joint of the
/// same skin, `geometry_to_bone` as the inverse bind matrix and the
/// bone's local transform as the rest pose. The skeleton root is the one
/// joint without a parent in the skin (empty when there are several: the
/// runtime then searches the whole prefab).
fn import_skins(ctx: &mut Ctx, scene: &ufbx::Scene) -> Result<Vec<SkinData>, ()> {
    let mut skins = Vec::new();
    for skin in scene.skin_deformers.iter() {
        let index = skin.element.typed_id;
        let sub_id = format!("skin/{index}");
        let name = named(&skin.element.name, "skin", index);
        if skin.clusters.is_empty() {
            ctx.error(
                Code::IMPORT_INVALID_DATA,
                format!("{sub_id} ({name}): a skin needs at least one cluster"),
            );
            return Err(());
        }
        if skin.clusters.len() > ctx.limits.max_joints {
            ctx.error(
                Code::IMPORT_LIMIT,
                format!(
                    "{sub_id} ({name}): {} joints; the limit is {}",
                    skin.clusters.len(),
                    ctx.limits.max_joints
                ),
            );
            return Err(());
        }
        let bones: Vec<&ufbx::Node> = skin
            .clusters
            .iter()
            .enumerate()
            .map(|(c, cluster)| match cluster.bone_node.as_ref() {
                Some(bone) => Ok(&**bone),
                None => {
                    ctx.error(
                        Code::IMPORT_INVALID_DATA,
                        format!("{sub_id} ({name}): cluster {c} is bound to no bone"),
                    );
                    Err(())
                }
            })
            .collect::<Result<_, ()>>()?;
        let mut joints: Vec<SkinJoint> = Vec::with_capacity(bones.len());
        for (cluster, bone) in skin.clusters.iter().zip(&bones) {
            let joint_name = node_name(bone);
            let parent = bone.parent.as_ref().and_then(|p| {
                bones
                    .iter()
                    .position(|b| b.element.element_id == p.element.element_id)
                    .map(|p| p as u32)
            });
            let t = &bone.local_transform;
            joints.push(SkinJoint {
                name: joint_name,
                binding: crate::graph::binding_key(ctx.asset_id, bone.element.typed_id as usize),
                parent,
                inverse_bind: mat4(&cluster.geometry_to_bone),
                rest_translation: v3(t.translation),
                rest_rotation: quat(t.rotation),
                rest_scale: v3(t.scale),
            });
        }
        let mut roots = joints.iter().filter(|j| j.parent.is_none());
        let skeleton = match (roots.next(), roots.next()) {
            (Some(root), None) => root.binding.clone(),
            _ => String::new(),
        };
        skins.push(SkinData {
            sub_id,
            name,
            payload: SkinPayload { skeleton, joints },
        });
    }
    Ok(skins)
}

/// One [`AnimationData`] per animation stack that animates a node
/// (`anim/<a>` in import order, named after the stack): the stack's
/// curves baked by `ufbx` at the file's frame rate — every layer and
/// pre/post rotation folded into linear translation, rotation and scale
/// keys per node (constant runs reduced), the first key at 0 — and the
/// clip's duration (its last key). A stack that animates nothing
/// (Mixamo's default "Take 001") is skipped silently.
fn import_animations(ctx: &mut Ctx, scene: &ufbx::Scene) -> Result<Vec<AnimationData>, ()> {
    let fps = scene.settings.frames_per_second;
    let opts = || ufbx::BakeOpts {
        trim_start_time: true,
        resample_rate: if fps > 0.0 && fps.is_finite() {
            fps
        } else {
            30.0
        },
        key_reduction_enabled: true,
        key_reduction_rotation: true,
        ..Default::default()
    };
    let mut animations = Vec::new();
    for stack in scene.anim_stacks.iter() {
        let stack_name = &*stack.element.name;
        let baked = match ufbx::bake_anim(scene, &stack.anim, opts()) {
            Ok(b) => b,
            Err(e) => {
                ctx.error(
                    Code::IMPORT_INVALID_DATA,
                    format!(
                        "animation stack {stack_name:?} cannot be baked: {}",
                        &*e.description
                    ),
                );
                return Err(());
            }
        };
        if baked.nodes.is_empty() {
            continue;
        }
        let a = animations.len();
        let sub_id = format!("anim/{a}");
        let name = named(stack_name, "anim", a as u32);
        ctx.anim_names.push((
            a.to_string(),
            (!stack_name.is_empty()).then(|| stack_name.to_string()),
        ));
        let mut channels = Vec::new();
        let mut duration = 0.0f32;
        for node in baked.nodes.iter() {
            let Some(target) = scene.nodes.get(node.typed_id as usize) else {
                continue;
            };
            let target_name = node_name(target);
            let mut push = |ctx: &mut Ctx, property: u32, times: Vec<f32>, values: Vec<f32>| {
                if times.is_empty() {
                    return Ok(());
                }
                let channel = AnimationChannel {
                    target: crate::graph::binding_key(ctx.asset_id, node.typed_id as usize),
                    target_name: target_name.clone(),
                    property,
                    interpolation: ANIM_INTERPOLATION_LINEAR,
                    times,
                    values,
                };
                check_channel(&mut ctx.report, &sub_id, &name, channels.len(), &channel)?;
                duration = duration.max(*channel.times.last().unwrap());
                channels.push(channel);
                Ok::<(), ()>(())
            };
            push(
                ctx,
                ANIM_PROPERTY_TRANSLATION,
                node.translation_keys
                    .iter()
                    .map(|k| k.time as f32)
                    .collect(),
                node.translation_keys
                    .iter()
                    .flat_map(|k| v3(k.value))
                    .collect(),
            )?;
            push(
                ctx,
                ANIM_PROPERTY_ROTATION,
                node.rotation_keys.iter().map(|k| k.time as f32).collect(),
                node.rotation_keys
                    .iter()
                    .flat_map(|k| quat(k.value))
                    .collect(),
            )?;
            push(
                ctx,
                ANIM_PROPERTY_SCALE,
                node.scale_keys.iter().map(|k| k.time as f32).collect(),
                node.scale_keys.iter().flat_map(|k| v3(k.value)).collect(),
            )?;
        }
        if channels.is_empty() {
            continue;
        }
        animations.push(AnimationData {
            sub_id,
            name,
            payload: AnimationPayload {
                duration,
                channels,
                markers: Vec::new(),
            },
        });
    }
    Ok(animations)
}

/// `fynkel.PbrMaterial`'s document form from `ufbx`'s PBR view of the
/// material (the classic Phong/Lambert properties mapped: diffuse →
/// base colour, shininess → roughness, emissive; a real PBR material's
/// own values): factors as authored, one `<slot>_texture` per bound
/// file texture that is embedded (or, when allowed, next to the file).
/// `None` = an error was recorded.
fn material_document(
    ctx: &mut Ctx,
    material: &ufbx::Material,
    sub_id: &str,
    name: &str,
    meta: &Meta,
    images: &mut Vec<ImageData>,
) -> Option<Value> {
    let pbr = &material.pbr;
    let value = |map: &ufbx::MaterialMap, default: f32| -> f32 {
        if map.has_value {
            map.value_vec4.x as f32
        } else {
            default
        }
    };
    let color = |map: &ufbx::MaterialMap, default: [f32; 3]| -> [f32; 3] {
        if map.has_value {
            [
                map.value_vec4.x as f32,
                map.value_vec4.y as f32,
                map.value_vec4.z as f32,
            ]
        } else {
            default
        }
    };
    let mut doc = material_header(name, sub_id, &shown_file(ctx.path));
    let base = color(&pbr.base_color, [1.0; 3]);
    let base_factor = value(&pbr.base_factor, 1.0);
    let opacity = value(&pbr.opacity, 1.0).clamp(0.0, 1.0);
    doc.insert(
        "base_color".into(),
        nums(&[
            base[0] * base_factor,
            base[1] * base_factor,
            base[2] * base_factor,
            opacity,
        ]),
    );
    doc.insert("metallic".into(), num(value(&pbr.metalness, 0.0)));
    doc.insert("roughness".into(), num(value(&pbr.roughness, 0.5)));
    // Emission colour × factor; anything past 1 becomes the strength (the
    // way glTF's `KHR_materials_emissive_strength` splits it), so a
    // Blender emission of white at 3 lands as `[1, 1, 1]` at 3.
    let emission = color(&pbr.emission_color, [0.0; 3]);
    let emission_factor = value(&pbr.emission_factor, 1.0);
    let mut emissive = emission.map(|c| (c * emission_factor).max(0.0));
    let peak = emissive.iter().copied().fold(0.0_f32, f32::max);
    if peak > 1.0 {
        emissive = emissive.map(|c| c / peak);
        doc.insert("emissive_strength".into(), num(peak));
    }
    doc.insert("emissive".into(), nums(&emissive));
    if opacity < 1.0 || pbr.opacity.texture.is_some() {
        doc.insert("alpha_mode".into(), json!("blend"));
    }
    if material.features.double_sided.enabled {
        doc.insert("double_sided".into(), json!(true));
    }

    // Texture slots, in the material model's order.
    let slots: [(&str, &ufbx::MaterialMap); 4] = [
        ("base_color_texture", &pbr.base_color),
        ("normal_texture", &pbr.normal_map),
        ("occlusion_texture", &pbr.ambient_occlusion),
        ("emissive_texture", &pbr.emission_color),
    ];
    for (field, map) in slots {
        let Some(texture) = map.texture.as_ref() else {
            continue;
        };
        let image_sub = bind_texture(ctx, texture, sub_id, name, field, images)?;
        if let Some(image_sub) = image_sub {
            doc.insert(field.into(), image_ref(meta, &image_sub));
        }
    }
    let unbound: [(&str, &ufbx::MaterialMap); 3] = [
        ("roughness", &pbr.roughness),
        ("metalness", &pbr.metalness),
        ("opacity", &pbr.opacity),
    ];
    for (what, map) in unbound {
        if let Some(texture) = map.texture.as_ref() {
            ctx.warn(
                Code::IMPORT_SKIPPED,
                format!(
                    "{sub_id} ({name}): the {what} texture ({}) is not bound; the PBR material reads roughness and metallic from one packed metallic_roughness_texture and opacity from the base colour's alpha (set them on the material document)",
                    texture_label(texture)
                ),
            );
        }
    }
    Some(Value::Object(doc))
}

fn texture_label(texture: &ufbx::Texture) -> String {
    let file = &*texture.relative_filename;
    if !file.is_empty() {
        file.to_string()
    } else if !texture.element.name.is_empty() {
        texture.element.name.to_string()
    } else {
        format!("texture {}", texture.element.typed_id)
    }
}

/// The image sub-asset for a texture bound to a material slot: the
/// texture's embedded bytes (`image/<t>`, imported once however many
/// slots bind it), or a plain relative path next to the file when the
/// sidecar allows external files. `Some(None)` = the slot stays unbound
/// with a warning (a layered or procedural texture); `None` = an error
/// was recorded (a file the importer will not read, an image it cannot
/// decode, the image limits).
fn bind_texture(
    ctx: &mut Ctx,
    texture: &ufbx::Texture,
    sub_id: &str,
    name: &str,
    field: &str,
    images: &mut Vec<ImageData>,
) -> Option<Option<String>> {
    let label = texture_label(texture);
    if texture.type_ != ufbx::TextureType::File {
        ctx.warn(
            Code::IMPORT_SKIPPED,
            format!(
                "{sub_id} ({name}): {field} is a {:?} texture ({label}); only file textures are bound",
                texture.type_
            ),
        );
        return Some(None);
    }
    let index = texture.element.typed_id;
    let image_sub = format!("image/{index}");
    if !ctx.warned_textures.contains(&index) {
        ctx.warned_textures.push(index);
        if texture.wrap_u != ufbx::WrapMode::Repeat || texture.wrap_v != ufbx::WrapMode::Repeat {
            ctx.warn(
                Code::IMPORT_SKIPPED,
                format!(
                    "{label}: clamped wrapping is not kept; materials sample linear/repeat (sampler settings belong to the texture's sidecar)"
                ),
            );
        }
        if texture.has_uv_transform {
            ctx.warn(
                Code::IMPORT_SKIPPED,
                format!("{label}: its UV transform is not kept; the material samples the mesh's UVs as they are"),
            );
        }
    }
    if images.iter().any(|i| i.sub_id == image_sub) {
        return Some(Some(image_sub));
    }
    let bytes: Vec<u8> = if !texture.content.is_empty() {
        texture.content.to_vec()
    } else {
        let what = format!("{sub_id} ({name}): {field} ({label})");
        if !ctx.allow_external_files {
            ctx.report.error_with(
                Code::IMPORT_EXTERNAL_URI,
                format!("{what} is not embedded in the file; external files are not read"),
                "export with embedded media (Blender: Path Mode \"Copy\" with Embed Textures; Mixamo: download \"With Skin\"), or set \"allowExternalFiles\": true in the sidecar's settings for plain relative paths",
            );
            return None;
        }
        let relative = &*texture.relative_filename;
        if !plain_relative(relative) {
            ctx.error(
                Code::IMPORT_EXTERNAL_URI,
                format!(
                    "{what}: only plain relative paths inside the source's directory are read ({relative:?})"
                ),
            );
            return None;
        }
        let file = ctx
            .path
            .parent()
            .unwrap_or(Path::new("."))
            .join(relative.replace('\\', "/"));
        match read_source(&file, ctx.limits) {
            Ok(bytes) => bytes,
            Err(d) => {
                ctx.report.diagnostics.push(
                    d.with_document(ctx.report.shown.clone())
                        .with_suggestion(format!("{what} → {}", file.display())),
                );
                return None;
            }
        }
    };
    if bytes.len() as u64 > ctx.limits.max_image_bytes {
        ctx.error(
            Code::IMPORT_LIMIT,
            format!(
                "{label}: {} bytes; the limit is {}",
                bytes.len(),
                ctx.limits.max_image_bytes
            ),
        );
        return None;
    }
    let mime = sniff_mime(&bytes);
    if mime == "application/octet-stream" {
        ctx.report.error_with(
            Code::IMPORT_UNSUPPORTED,
            format!("{sub_id} ({name}): {field} ({label}) is not a PNG or JPEG image"),
            "export textures as PNG or JPEG (TGA, DDS, PSD, EXR are not decoded)",
        );
        return None;
    }
    if images.len() >= ctx.limits.max_images {
        ctx.error(
            Code::IMPORT_LIMIT,
            format!(
                "more than {} images; that is the limit",
                ctx.limits.max_images
            ),
        );
        return None;
    }
    let image_name = Path::new(relative_or_name(texture))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("image{index}"));
    images.push(ImageData {
        sub_id: image_sub.clone(),
        name: image_name,
        mime_type: mime.to_string(),
        bytes,
        color_space: ColorSpace::Srgb,
    });
    Some(Some(image_sub))
}

/// Whether `relative` is a plain relative path the importer would read
/// next to the source (no scheme, drive, absolute path or `..`).
fn plain_relative(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.contains("://")
        && !relative.starts_with('/')
        && !relative.starts_with('\\')
        && !relative.contains(':')
        && !relative.contains("..")
        && !relative.contains('\0')
}

/// The files an FBX may read besides itself: the relative path of every
/// file texture that is not embedded, sorted and unique (`\` → `/`).
/// Read from a load without geometry or animation (embedded content is
/// kept: it is what tells an embedded texture from an external one), so
/// the cook can key the cache on them (`cook.rs`) without importing; an
/// unreadable file has none.
pub fn external_files(bytes: &[u8]) -> Vec<String> {
    let opts = ufbx::LoadOpts {
        ignore_geometry: true,
        ignore_animation: true,
        load_external_files: false,
        ignore_missing_external_files: true,
        ..Default::default()
    };
    let Ok(scene) = ufbx::load_memory(bytes, opts) else {
        return Vec::new();
    };
    let mut names: Vec<String> = scene
        .textures
        .iter()
        .filter(|t| t.type_ == ufbx::TextureType::File && t.content.is_empty())
        .map(|t| t.relative_filename.replace('\\', "/"))
        .filter(|r| !r.is_empty())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

fn relative_or_name(texture: &ufbx::Texture) -> &str {
    if !texture.relative_filename.is_empty() {
        &texture.relative_filename
    } else {
        &texture.element.name
    }
}
