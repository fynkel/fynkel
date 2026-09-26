//! The Inspector's preview stages (authoring ABI version 4, ADR-0030 §3):
//! one small edit-mode world per open 3D preview, holding one asset alone
//! as `fynkel_preview` stages it — a prefab or a model's hierarchy, a mesh
//! with its material, a material on a shape — lit by the stage's own sun
//! and framed from the three-quarter view, the way the Project's tiles
//! show it. The world draws through the viewport ABI like the Scene
//! view's (`fynkel_apple_viewport_render` over
//! [`fynkel_authoring_stage_world`]); it shares the session's cooked
//! store, so an unsaved material edit (`setMaterial`) shows in it too.

use fynkel_assets::{Assets, Residency};
use fynkel_contracts::{AssetRef, Code, Diagnostic, Status};
use fynkel_core::components::{Animator, ComponentValue};
use fynkel_core::{AnimateOp, Command, Entity, Lane, WorldConfig};
use fynkel_preview::{Framing, Shape, Subject};
use glam::Vec3;
use serde_json::{json, Value};

use super::{
    f32_arg, guarded_session, invalid, kind_and_args, one, refused, uuid_of, vec3_arg,
    FynkelAuthoringSession, Outcome,
};
use crate::FynkelWorld;

/// One open stage. Boxed in the session so the world pointer stays put.
pub(super) struct StageSlot {
    world: FynkelWorld,
    subject: Subject,
    /// The subject's closure, held resident while it is shown.
    residency: Option<Residency>,
    sun: Option<Entity>,
    /// Toward the stage's sun.
    toward: Vec3,
    framing: Option<Framing>,
    diagnostics: Vec<String>,
    /// The subject's top entity: where a clip's animator goes.
    root: Option<Entity>,
    /// The joints of the subject's skins.
    bones: usize,
    /// The clip or blend shown on the subject, and where it stands.
    posed: Option<Posed>,
}

/// A clip (or blend space) on a stage's subject (ADR-0030 step 7). The
/// host owns the clock: each `clip` call seeks the stage's `Animator` to
/// `time`, and the world's own animation system poses the hierarchy and
/// the skins, so what the preview shows is what a game plays.
struct Posed {
    clip: AssetRef,
    time: f32,
    /// Root motion held in place (the Animator's `root_motion`); off, the
    /// root moves with the clip.
    in_place: bool,
    /// A blend space's parameter: its name and value.
    parameter: Option<(String, f32)>,
}

impl StageSlot {
    fn new(subject: Subject) -> StageSlot {
        StageSlot {
            world: FynkelWorld::new(WorldConfig {
                edit_mode: true,
                ..WorldConfig::default()
            }),
            subject,
            residency: None,
            sun: None,
            toward: Vec3::Y,
            framing: None,
            diagnostics: Vec::new(),
            root: None,
            bones: 0,
            posed: None,
        }
    }

    /// Loads the subject afresh from `assets` (after an open, a shape, a
    /// recook), steps once so the transforms are propagated, and frames
    /// what is drawn. The camera the host steers starts from the framing;
    /// the sun keeps the direction it was turned to.
    fn load(&mut self, assets: &Assets) -> Outcome {
        self.world.assets = Some(assets.clone());
        let first = self.framing.is_none();
        let stood = fynkel_preview::stage_into(
            assets,
            &self.subject,
            self.world.core_mut(),
            Some(self.toward),
        )
        .map_err(refusal)?;
        self.residency = Some(stood.residency);
        self.diagnostics = stood.diagnostics;
        self.sun = stood.sun;
        self.root = stood.root;
        self.bones = stood.bones;
        let world = self.world.core_mut();
        world.step();
        world.drain_events();
        let extract = world.extract();
        let bounds = fynkel_preview::world_bounds(assets, &extract);
        let framing = fynkel_preview::frame(
            bounds.unwrap_or((Vec3::splat(-0.5), Vec3::splat(0.5))),
            fynkel_preview::VIEW_FROM,
        );
        if first {
            self.toward = framing.light_dir;
            if let Some(sun) = self.sun {
                fynkel_preview::set_sun(self.world.core_mut(), sun, self.toward);
            }
        }
        self.framing = Some(framing);
        // Framed in its rest pose; then posed by its clip (a recook
        // keeps what was shown).
        if self.posed.is_some() {
            self.animate(assets)?;
        }
        Ok(self.json(assets, bounds))
    }

    /// Puts the clip on the subject's top (a fresh animator: the world
    /// was just loaded) and seeks it.
    fn animate(&mut self, assets: &Assets) -> Result<(), Vec<Diagnostic>> {
        let (Some(posed), Some(root)) = (&self.posed, self.root) else {
            return Err(invalid("stage: this stage has nothing to play a clip on"));
        };
        let world = self.world.core_mut();
        world.set_edit_animation(true);
        assets
            .register_clip(&posed.clip, world)
            .map_err(|d| invalid(format!("stage: {}", d.message)))?;
        world.submit(
            Lane::Simulation,
            vec![Command::SetComponent {
                entity: root,
                value: ComponentValue::Animator(Animator {
                    clip: posed.clip.clone(),
                    speed: 1.0,
                    looping: true,
                    playing: false,
                    root_motion: posed.in_place,
                    ..Animator::default()
                }),
            }],
        );
        world.step();
        world.drain_events();
        self.seek()
    }

    /// The animator at the posed time (and parameter).
    fn seek(&mut self) -> Result<(), Vec<Diagnostic>> {
        let (Some(posed), Some(root)) = (&self.posed, self.root) else {
            return Ok(());
        };
        let mut commands = Vec::new();
        if let Some((name, value)) = &posed.parameter {
            commands.push(Command::Animate {
                entity: root,
                op: AnimateOp::SetParameter {
                    name: name.clone(),
                    value: *value,
                },
            });
        }
        commands.push(Command::Animate {
            entity: root,
            op: AnimateOp::SetTime(posed.time.max(0.0)),
        });
        let world = self.world.core_mut();
        world.submit(Lane::Simulation, commands);
        world.step();
        world.drain_events();
        Ok(())
    }

    /// Where the clip stands: `null` without one.
    fn clip_json(&self, assets: &Assets) -> Value {
        let (Some(posed), Some(root)) = (&self.posed, self.root) else {
            return Value::Null;
        };
        let state = self.world.core().animator_state(root);
        let blend = assets
            .is_blend(&posed.clip)
            .then(|| assets.blend(&posed.clip).ok())
            .flatten();
        // The frame rate the keys were baked at (no clip carries one).
        let first = match &blend {
            Some(b) => b.points.first().map(|p| p.clip.clone()),
            None => Some(posed.clip.clone()),
        };
        let fps = first
            .and_then(|r| assets.clip(&r).ok())
            .map(|c| key_rate(&c))
            .unwrap_or(30.0);
        json!({
            "clip": { "asset": posed.clip.asset.to_string(), "sub": posed.clip.sub },
            "time": state.as_ref().map_or(posed.time, |s| s.time),
            "duration": state.as_ref().map_or(0.0, |s| s.duration),
            "fps": fps,
            "inPlace": posed.in_place,
            "blend": blend.map(|b| json!({
                "parameter": b.parameter,
                "value": posed.parameter.as_ref().map(|p| p.1),
                "points": b.points.iter().map(|p| json!({
                    "at": p.at,
                    "clip": { "asset": p.clip.asset.to_string(), "sub": p.clip.sub },
                })).collect::<Vec<_>>(),
            })),
        })
    }

    fn json(&self, assets: &Assets, bounds: Option<(Vec3, Vec3)>) -> Value {
        let extract = self.world.core().extract();
        let stats = fynkel_preview::mesh_stats(assets, &extract);
        let framing = self.framing.expect("a loaded stage is framed");
        json!({
            "subject": match &self.subject {
                Subject::Hierarchy(_) => "hierarchy",
                Subject::Mesh { .. } => "mesh",
                Subject::Material { .. } => "material",
            },
            "shape": match &self.subject {
                Subject::Material { shape, .. } => shape_json(shape),
                _ => Value::Null,
            },
            "framing": {
                "eye": framing.eye.to_array(),
                "target": framing.target.to_array(),
                "fovY": framing.fov_y_radians,
                "near": framing.near,
                "far": framing.far,
                "radius": framing.radius,
                "world": framing.camera_world().to_cols_array(),
            },
            "light": self.toward.to_array(),
            "stats": {
                "meshes": stats.meshes,
                "vertices": stats.vertices,
                "triangles": stats.triangles,
                "skinned": stats.skinned,
                "bones": self.bones,
                "bounds": bounds.map(|(lo, hi)| json!({ "min": lo.to_array(), "max": hi.to_array() })),
            },
            "diagnostics": self.diagnostics,
            "clip": self.clip_json(assets),
        })
    }
}

/// The rate a clip's keys were baked at: the closest spacing of any
/// channel's keys, as frames per second. Keys farther apart than a tenth
/// of a second were set by hand, not baked: then 30, as Unity shows a
/// clip with no rate of its own.
fn key_rate(clip: &fynkel_core::animation::Clip) -> f32 {
    let gap = clip
        .channels
        .iter()
        .flat_map(|c| c.times.windows(2).map(|w| w[1] - w[0]))
        .filter(|g| *g > 1e-4)
        .fold(f32::INFINITY, f32::min);
    if gap.is_finite() && gap <= 0.1 {
        (1.0 / gap).round().clamp(10.0, 240.0)
    } else {
        30.0
    }
}

fn refusal(r: fynkel_preview::Refusal) -> Vec<Diagnostic> {
    invalid(format!("stage: {}", r.message()))
}

/// An argument's id in the cache's terms.
fn asset_id(v: Option<&Value>, what: &str) -> Result<fynkel_contracts::Uuid, Vec<Diagnostic>> {
    Ok(fynkel_contracts::Uuid::from_bytes(
        *uuid_of(v, what)?.as_bytes(),
    ))
}

fn shape_json(shape: &Shape) -> Value {
    match shape {
        Shape::Sphere => json!("sphere"),
        Shape::Cube => json!("cube"),
        Shape::Cylinder => json!("cylinder"),
        Shape::Capsule => json!("capsule"),
        Shape::Plane => json!("plane"),
        Shape::Mesh(m) => json!({ "asset": m.asset.to_string(), "sub": m.sub }),
    }
}

/// `shape`: a built-in's name, or `{ "asset", "sub"? }` for a mesh.
fn shape_arg(v: &Value) -> Result<Shape, Vec<Diagnostic>> {
    if let Some(name) = v.as_str() {
        return Shape::parse(name).ok_or_else(|| {
            invalid("stage: `shape` must be sphere, cube, cylinder, capsule, plane or a mesh")
        });
    }
    let asset = asset_id(v.get("asset"), "shape.asset")?;
    Ok(Shape::Mesh(match v.get("sub").and_then(Value::as_str) {
        Some(sub) => AssetRef::with_sub(asset, sub),
        None => AssetRef::new(asset),
    }))
}

impl FynkelAuthoringSession {
    fn stage(&mut self, stage: u32) -> Result<&mut StageSlot, Vec<Diagnostic>> {
        self.stages
            .get_mut(&stage)
            .map(|b| &mut **b)
            .ok_or_else(|| {
                one(Diagnostic::error(
                    Code::SESSION_UNKNOWN_DOCUMENT,
                    format!("stage: {stage} is not open"),
                )
                .with_suggestion("open one with fynkel_authoring_stage_open first"))
            })
    }

    /// Every stage loaded again from `assets` (after a recook); a stage
    /// whose asset is gone keeps its last world and reports why.
    pub(super) fn reload_stages(&mut self, assets: &Assets) -> Vec<Value> {
        self.stages
            .iter_mut()
            .map(|(id, slot)| {
                let mut v = slot
                    .load(assets)
                    .unwrap_or_else(|d| json!({ "diagnostics": super::diags(&d) }));
                v["stage"] = json!(*id);
                v
            })
            .collect()
    }

    /// The distinct asset stores of every world the session holds: an
    /// unsaved material edit is shown in all of them.
    pub(super) fn stores(&self) -> Vec<Assets> {
        let mut stores: Vec<Assets> = Vec::new();
        let worlds = self
            .previews
            .values()
            .map(|p| &p.world)
            .chain(self.stages.values().map(|s| &s.world));
        for a in worlds
            .filter_map(|w| w.assets())
            .chain(self.assets.as_ref())
        {
            if !stores.iter().any(|b| b.same_store(a)) {
                stores.push(a.clone());
            }
        }
        stores
    }
}

/// Opens a stage for one asset (version 4): `args_json` is `{ "asset":
/// uuid, "sub"?: "mesh/…" | "material/…" | "prefab", "shape"?: a shape }`.
/// The project is cooked first when nothing opened the cache yet.
///
/// # Safety
/// `session` live or NULL; `args_json_utf8` readable for `args_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_stage_open(
    session: *mut FynkelAuthoringSession,
    args_json_utf8: *const u8,
    args_len: u32,
) -> Status {
    let kind = "open";
    let args = match kind_and_args(
        "stage",
        kind.as_ptr(),
        kind.len() as u32,
        args_json_utf8,
        args_len,
    ) {
        Ok((_, args)) => args,
        Err(diagnostics) => return refused(session, diagnostics),
    };
    guarded_session(session, move |s| {
        let asset = asset_id(args.get("asset"), "asset")?;
        let sub = args.get("sub").and_then(Value::as_str);
        let (assets, cook) = s.ensure_assets()?;
        // A sound has a stage without a world (the sound preview).
        if sub.is_none()
            && assets
                .entry(asset)
                .is_some_and(|e| fynkel_preview::kind_name(&e.kind) == "sound")
        {
            let r = AssetRef::new(asset);
            let clip = assets
                .sound(&r)
                .map_err(|d| invalid(format!("stage: {}", d.message)))?;
            let slot = super::sound::SoundSlot::new(r, clip);
            let mut v = slot.json();
            s.next_stage += 1;
            let id = s.next_stage;
            s.sounds.insert(id, Box::new(slot));
            v["ok"] = json!(true);
            v["stage"] = json!(id);
            v["cook"] = cook.unwrap_or(Value::Null);
            return Ok(v);
        }
        // A blend space plays on the model its first point's clip is from.
        let blend = AssetRef::new(asset);
        let posed = if sub.is_none() && assets.is_blend(&blend) {
            let space = assets
                .blend(&blend)
                .map_err(|d| invalid(format!("stage: {}", d.message)))?;
            let first = space
                .points
                .first()
                .ok_or_else(|| invalid("stage: the blend space has no points"))?;
            Some((
                first.clip.asset,
                Posed {
                    clip: blend,
                    time: 0.0,
                    in_place: true,
                    parameter: Some((space.parameter.clone(), first.at)),
                },
            ))
        } else {
            None
        };
        let (asset, sub) = match &posed {
            Some((owner, _)) => (*owner, None),
            None => (asset, sub),
        };
        let mut subject = fynkel_preview::subject(&assets, asset, sub).map_err(refusal)?;
        if let (Subject::Material { shape, .. }, Some(v)) =
            (&mut subject, args.get("shape").filter(|v| !v.is_null()))
        {
            *shape = shape_arg(v)?;
        }
        let mut slot = Box::new(StageSlot::new(subject));
        slot.posed = posed.map(|(_, p)| p);
        let mut v = slot.load(&assets)?;
        s.next_stage += 1;
        let id = s.next_stage;
        s.stages.insert(id, slot);
        v["ok"] = json!(true);
        v["stage"] = json!(id);
        v["cook"] = cook.unwrap_or(Value::Null);
        Ok(v)
    })
}

/// Drops a stage; its world pointer must not be drawn afterwards.
///
/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_stage_close(
    session: *mut FynkelAuthoringSession,
    stage: u32,
) -> Status {
    guarded_session(session, move |s| {
        let closed = s.stages.remove(&stage).is_some() | s.sounds.remove(&stage).is_some();
        Ok(json!({ "ok": true, "stage": stage, "closed": closed }))
    })
}

/// A stage's world for the viewport ABI; NULL when it is not open, or the
/// session is NULL or poisoned. Valid until the stage's close, the
/// project's close or the session's destruction.
///
/// # Safety
/// `session` live or NULL.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_stage_world(
    session: *const FynkelAuthoringSession,
    stage: u32,
) -> *const FynkelWorld {
    match session.as_ref() {
        Some(s) if !s.poisoned => s
            .stages
            .get(&stage)
            .map(|slot| &slot.world as *const FynkelWorld)
            .unwrap_or(std::ptr::null()),
        _ => std::ptr::null(),
    }
}

const STAGE_KINDS: [&str; 6] = ["shape", "light", "clip", "reload", "stats", "setMaterial"];

/// A call on a stage, by kind with JSON arguments: `shape` (a material's:
/// `{ "shape" }`), `light` (`{ "toward": [x, y, z] }` or `{ "yaw",
/// "pitch" }` radians turning it), `clip` (`{ "clip"?: { "asset", "sub"? }
/// | null, "time"?, "inPlace"?, "parameter"? }`: a clip or blend space on
/// a hierarchy, seeked), `reload`, `stats`, and `setMaterial` /
/// `restoreMaterial` (as the edit-preview's: on every store).
///
/// # Safety
/// As [`fynkel_authoring_stage_open`]; `kind_utf8` readable for `kind_len`.
#[no_mangle]
pub unsafe extern "C" fn fynkel_authoring_stage_call(
    session: *mut FynkelAuthoringSession,
    stage: u32,
    kind_utf8: *const u8,
    kind_len: u32,
    args_json_utf8: *const u8,
    args_len: u32,
) -> Status {
    let (kind, args) = match kind_and_args("stage", kind_utf8, kind_len, args_json_utf8, args_len) {
        Ok(pair) => pair,
        Err(diagnostics) => return refused(session, diagnostics),
    };
    guarded_session(session, move |s| stage_call(s, stage, &kind, &args))
}

fn stage_call(s: &mut FynkelAuthoringSession, stage: u32, kind: &str, args: &Value) -> Outcome {
    if let Some(sound) = s.sounds.get_mut(&stage) {
        let mut v = sound.call(kind, args)?;
        v["ok"] = json!(true);
        v["stage"] = json!(stage);
        return Ok(v);
    }
    let (assets, _) = s.ensure_assets()?;
    let mut v = match kind {
        "shape" => {
            let shape = shape_arg(super::field(args, "shape")?)?;
            let slot = s.stage(stage)?;
            let Subject::Material { shape: current, .. } = &mut slot.subject else {
                return Err(invalid("stage: only a material's stage has a shape"));
            };
            let before = std::mem::replace(current, shape);
            // Refused (a dropped mesh that does not load): the stage keeps
            // the shape it had.
            match slot.load(&assets) {
                Ok(v) => v,
                Err(d) => {
                    if let Subject::Material { shape, .. } = &mut slot.subject {
                        *shape = before;
                    }
                    slot.load(&assets)?;
                    return Err(d);
                }
            }
        }
        "light" => {
            let slot = s.stage(stage)?;
            slot.toward = if args.get("toward").is_some() {
                vec3_arg(args, "toward")?
                    .try_normalize()
                    .ok_or_else(|| invalid("stage: `toward` must not be zero"))?
            } else {
                // Turned like an orbit: about the world up, then about the
                // level axis across it, the elevation held off the poles.
                let yaw = f32_arg(args, "yaw")?;
                let pitch = f32_arg(args, "pitch")?;
                let t = glam::Quat::from_rotation_y(yaw) * slot.toward;
                let across = Vec3::new(t.z, 0.0, -t.x).try_normalize().unwrap_or(Vec3::X);
                let turned = glam::Quat::from_axis_angle(across, pitch) * t;
                if turned.y.abs() < 0.995 {
                    turned
                } else {
                    t
                }
            };
            if let Some(sun) = slot.sun {
                fynkel_preview::set_sun(slot.world.core_mut(), sun, slot.toward);
            }
            json!({ "light": slot.toward.to_array() })
        }
        "clip" => {
            let slot = s.stage(stage)?;
            if !matches!(slot.subject, Subject::Hierarchy(_)) {
                return Err(invalid("stage: only a model, rig or prefab plays a clip"));
            }
            let mut reload = false;
            if let Some(v) = args.get("clip") {
                let next = if v.is_null() {
                    None
                } else {
                    let asset = asset_id(v.get("asset"), "clip.asset")?;
                    Some(match v.get("sub").and_then(Value::as_str) {
                        Some(sub) if !sub.is_empty() => AssetRef::with_sub(asset, sub),
                        _ => AssetRef::new(asset),
                    })
                };
                if next.as_ref() != slot.posed.as_ref().map(|p| &p.clip) {
                    let parameter = match &next {
                        Some(r) if assets.is_blend(r) => {
                            let space = assets
                                .blend(r)
                                .map_err(|d| invalid(format!("stage: {}", d.message)))?;
                            space
                                .points
                                .first()
                                .map(|p| (space.parameter.clone(), p.at))
                        }
                        _ => None,
                    };
                    let in_place = slot.posed.as_ref().is_none_or(|p| p.in_place);
                    slot.posed = next.map(|clip| Posed {
                        clip,
                        time: 0.0,
                        in_place,
                        parameter,
                    });
                    reload = true;
                }
            }
            let mut animate = false;
            if let Some(posed) = &mut slot.posed {
                if args.get("time").is_some_and(|t| !t.is_null()) {
                    posed.time = f32_arg(args, "time")?.max(0.0);
                }
                if let Some(on) = args.get("inPlace").and_then(Value::as_bool) {
                    animate |= on != posed.in_place;
                    posed.in_place = on;
                }
                if args.get("parameter").is_some_and(|t| !t.is_null()) {
                    let value = f32_arg(args, "parameter")?;
                    if let Some(p) = &mut posed.parameter {
                        p.1 = value;
                    }
                }
            }
            if reload {
                // Back to the rest pose, then the new clip (a bone the old
                // clip moved and the new one does not keeps no stale pose).
                let before = slot.posed.take();
                slot.load(&assets)?;
                slot.posed = before;
                if slot.posed.is_some() {
                    slot.animate(&assets)?;
                }
            } else if animate {
                slot.animate(&assets)?;
            } else {
                slot.seek()?;
            }
            match slot.clip_json(&assets) {
                Value::Null => json!({ "clip": null }),
                v => v,
            }
        }
        "reload" => s.stage(stage)?.load(&assets)?,
        "stats" => {
            let slot = s.stage(stage)?;
            let bounds = fynkel_preview::world_bounds(&assets, &slot.world.core().extract());
            slot.json(&assets, bounds)
        }
        "setMaterial" | "restoreMaterial" => {
            let material = uuid_of(args.get("material"), "material")?;
            s.stage(stage)?;
            let stores = s.stores();
            for a in &stores {
                if kind == "setMaterial" {
                    let data = args.get("data").filter(|d| d.is_object()).ok_or_else(|| {
                        invalid("setMaterial: `data` must be the material's fields, an object")
                    })?;
                    fynkel_authoring::Preview::set_material(a, material, data).map_err(one)?;
                } else {
                    fynkel_authoring::Preview::restore_material(a, material);
                }
            }
            json!({ "stores": stores.len() })
        }
        other => {
            return Err(one(Diagnostic::error(
                Code::SESSION_UNKNOWN_QUERY,
                format!("stage: unknown call `{other}`"),
            )
            .with_suggestion(format!(
                "one of: {}, restoreMaterial",
                STAGE_KINDS.join(", ")
            ))))
        }
    };
    v["ok"] = json!(true);
    v["stage"] = json!(stage);
    Ok(v)
}
