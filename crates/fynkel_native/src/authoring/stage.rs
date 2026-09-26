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
use fynkel_core::{Entity, WorldConfig};
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
}

impl StageSlot {
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
        Ok(self.json(assets, bounds))
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
                "bounds": bounds.map(|(lo, hi)| json!({ "min": lo.to_array(), "max": hi.to_array() })),
            },
            "diagnostics": self.diagnostics,
        })
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
        let mut subject = fynkel_preview::subject(&assets, asset, sub).map_err(refusal)?;
        if let (Subject::Material { shape, .. }, Some(v)) =
            (&mut subject, args.get("shape").filter(|v| !v.is_null()))
        {
            *shape = shape_arg(v)?;
        }
        let mut slot = Box::new(StageSlot {
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
        });
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

const STAGE_KINDS: [&str; 5] = ["shape", "light", "reload", "stats", "setMaterial"];

/// A call on a stage, by kind with JSON arguments: `shape` (a material's:
/// `{ "shape" }`), `light` (`{ "toward": [x, y, z] }` or `{ "yaw",
/// "pitch" }` radians turning it), `reload`, `stats`, and `setMaterial` /
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
