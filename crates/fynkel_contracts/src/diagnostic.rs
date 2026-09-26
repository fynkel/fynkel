//! Authoring diagnostics (blueprint §14.2). One record shape for documents,
//! assets, projects and codegen so the `--json` CLI, the editor and agents
//! read the same thing. Codes are stable strings in five namespaces:
//! `FYN-D####` documents, `FYN-A####` assets, `FYN-P####` project,
//! `FYN-E####` editing (authoring commands), `FYN-G####` codegen, and
//! later `FYN-R` (runtime loading), `FYN-I` (input), `FYN-S` (schema),
//! `FYN-H` (physics/components), `FYN-T` (the authoring session and its
//! ABI). Append only; never renumber.

/// A stable diagnostic code such as `FYN-D0002`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Code(pub &'static str);

impl Code {
    // Documents (FYN-D): strict JSON layer.
    pub const JSON_SYNTAX: Code = Code("FYN-D0001");
    pub const DUPLICATE_KEY: Code = Code("FYN-D0002");
    /// Non-finite, out of range, or an integer beyond 2^53.
    pub const NUMBER_NOT_REPRESENTABLE: Code = Code("FYN-D0003");
    pub const ROOT_NOT_OBJECT: Code = Code("FYN-D0004");
    pub const NESTING_TOO_DEEP: Code = Code("FYN-D0005");
    // Documents (FYN-D): envelope.
    pub const ENVELOPE_MISSING_FIELD: Code = Code("FYN-D0010");
    pub const ENVELOPE_WRONG_TYPE: Code = Code("FYN-D0011");
    pub const MALFORMED_ID: Code = Code("FYN-D0012");
    /// `formatVersion` newer than this tool supports: read-only, never rewritten.
    pub const UNSUPPORTED_FORMAT_VERSION: Code = Code("FYN-D0013");
    pub const SCHEMA_REFERENCE_MISMATCH: Code = Code("FYN-D0014");
    pub const UNKNOWN_DOCUMENT_KIND: Code = Code("FYN-D0015");
    /// `formatVersion` older than the current one: migrate before use.
    pub const OUTDATED_FORMAT_VERSION: Code = Code("FYN-D0016");
    /// No chain of migrations leads from the document's version to the current one.
    pub const NO_MIGRATION_PATH: Code = Code("FYN-D0017");
    /// A migration step could not be applied to this document.
    pub const MIGRATION_FAILED: Code = Code("FYN-D0018");
    // Documents (FYN-D): JSON Schema.
    pub const SCHEMA_VIOLATION: Code = Code("FYN-D0100");
    pub const SCHEMA_UNAVAILABLE: Code = Code("FYN-D0101");
    // Documents (FYN-D): scene semantics (blueprint §6; `fynkel_document::scene`).
    pub const SCENE_DANGLING_PARENT: Code = Code("FYN-D0200");
    pub const SCENE_UNKNOWN_CHILD: Code = Code("FYN-D0201");
    pub const SCENE_CHILD_PARENT_MISMATCH: Code = Code("FYN-D0202");
    pub const SCENE_HIERARCHY_CYCLE: Code = Code("FYN-D0203");
    pub const SCENE_DUPLICATE_COMPONENT: Code = Code("FYN-D0204");
    pub const SCENE_UNKNOWN_COMPONENT_TYPE: Code = Code("FYN-D0205");
    pub const SCENE_COMPONENT_DATA: Code = Code("FYN-D0206");
    pub const SCENE_COMPONENT_NOT_AUTHORED: Code = Code("FYN-D0207");
    pub const SCENE_DUPLICATE_CHILD: Code = Code("FYN-D0208");
    pub const SCENE_MALFORMED_ENTRY: Code = Code("FYN-D0209");
    // Documents (FYN-D): material vs shader manifest (blueprint §6;
    // `fynkel_document::material`).
    pub const MATERIAL_UNKNOWN_SHADER: Code = Code("FYN-D0300");
    pub const MATERIAL_UNKNOWN_PARAMETER: Code = Code("FYN-D0301");
    pub const MATERIAL_PARAMETER_TYPE: Code = Code("FYN-D0302");
    pub const MATERIAL_PARAMETER_RANGE: Code = Code("FYN-D0303");
    pub const MATERIAL_UNKNOWN_TEXTURE: Code = Code("FYN-D0304");
    pub const MATERIAL_MISSING_TEXTURE: Code = Code("FYN-D0305");
    pub const MATERIAL_UNSUPPORTED_ALPHA_MODE: Code = Code("FYN-D0306");
    pub const MATERIAL_MALFORMED_ENTRY: Code = Code("FYN-D0307");
    /// A texture binding overrides the slot's colour space (warning).
    pub const MATERIAL_COLOR_SPACE_OVERRIDE: Code = Code("FYN-D0308");
    // Documents (FYN-D): shader manifest semantics.
    pub const SHADER_BAD_DEFAULT: Code = Code("FYN-D0310");
    pub const SHADER_BAD_RANGE: Code = Code("FYN-D0311");
    pub const SHADER_MALFORMED_ENTRY: Code = Code("FYN-D0312");
    // Documents (FYN-D): `.shader` files (ADR-0031; `fynkel_shader`).
    /// The WGSL does not compile (parse or validation), at its line in
    /// the `.shader` file.
    pub const SHADER_COMPILE: Code = Code("FYN-D0313");
    /// `surface` is missing, or `surface`/`vertex` has the wrong signature
    /// for the contract.
    pub const SHADER_ENTRY: Code = Code("FYN-D0314");
    /// The header does not read: syntax, an unknown word, a duplicate or
    /// badly named property, a limit exceeded.
    pub const SHADER_HEADER: Code = Code("FYN-D0315");
    // Documents (FYN-D): prefabs and instances (blueprint §6 Prefabs;
    // `fynkel_document::prefab`).
    /// A prefab's `root` is not its one parentless entity (ADR-0022).
    pub const PREFAB_ROOT: Code = Code("FYN-D0400");
    /// An instance names a prefab the library does not have.
    pub const PREFAB_UNKNOWN: Code = Code("FYN-D0401");
    /// A prefab's nesting cannot be resolved: an instance chain leads back
    /// to the prefab itself (a cycle), so the instance stays unexpanded.
    pub const PREFAB_NESTED: Code = Code("FYN-D0402");
    /// An override addresses an entity/component/field that does not exist.
    pub const PREFAB_OVERRIDE_TARGET: Code = Code("FYN-D0403");
    /// An override is not applicable (duplicate id or type, data not matching the schema).
    pub const PREFAB_OVERRIDE_INVALID: Code = Code("FYN-D0404");
    /// Two instances in one scene share an instance id.
    pub const PREFAB_DUPLICATE_INSTANCE: Code = Code("FYN-D0405");
    /// A derived id collides with an existing entity or component id.
    pub const PREFAB_ID_COLLISION: Code = Code("FYN-D0406");
    pub const PREFAB_MALFORMED_ENTRY: Code = Code("FYN-D0407");
    /// An instance carries components of its own: the prefab root's are
    /// the instance's, changed by overrides (ADR-0022).
    pub const PREFAB_INSTANCE_COMPONENTS: Code = Code("FYN-D0408");
    // Documents (FYN-D): rig documents (blueprint §19.2d item 19b;
    // `fynkel_document::rig`).
    /// `skeleton` does not name an imported model (a source without a sub).
    pub const RIG_SKELETON: Code = Code("FYN-D0500");
    /// A clip does not name an imported model's `anim/<n>` sub-asset.
    pub const RIG_CLIP: Code = Code("FYN-D0501");
    /// A field the schema accepts but the rig cannot use (a malformed
    /// assetRef, a clip name that is not an identifier).
    pub const RIG_MALFORMED_ENTRY: Code = Code("FYN-D0502");
    // Documents (FYN-D): blend spaces (blueprint §19.2d item 19c;
    // `fynkel_document::blend`).
    /// The points are not strictly increasing in `at` (or there are none).
    pub const BLEND_POINTS: Code = Code("FYN-D0600");
    /// A point's clip is not an animation clip (a rig's or a model's `anim/*`).
    pub const BLEND_CLIP: Code = Code("FYN-D0601");
    /// A field the schema accepts but the blend space cannot use (a
    /// malformed assetRef, a non-finite `at`).
    pub const BLEND_MALFORMED_ENTRY: Code = Code("FYN-D0602");
    // Documents (FYN-D): volume profiles (R3a; `fynkel_document::volume`).
    /// A field the schema accepts but the renderer cannot use (a value
    /// that is not a finite number, a block that is not an object).
    pub const VOLUME_MALFORMED_ENTRY: Code = Code("FYN-D0700");
    // Documents (FYN-D): physics materials (ADR-0025;
    // `fynkel_document::physics_material`).
    /// A field the schema accepts but physics cannot use (a value that is
    /// not a finite number, a combine rule that is not one of the four).
    pub const PHYSICS_MATERIAL_MALFORMED_ENTRY: Code = Code("FYN-D0800");
    // Documents (FYN-D): I/O.
    pub const IO: Code = Code("FYN-D0900");
    // Documents (FYN-D): data assets (ADR-0027; `fynkel_document::data`).
    /// An envelope field of a data asset that does not read (`type` not a
    /// namespaced type id, `version` not an integer >= 1, `data` not an
    /// object).
    pub const DATA_MALFORMED_ENTRY: Code = Code("FYN-D1000");
    /// A data asset of a type no loaded schema or registry entry declares
    /// (a warning: `fynkel codegen` has not run, or the class is gone).
    pub const DATA_UNKNOWN_TYPE: Code = Code("FYN-D1001");
    /// A data asset whose `data` does not fit its type's schema, or that
    /// was written for another version of the type.
    pub const DATA_INVALID: Code = Code("FYN-D1002");
    // Documents (FYN-D): Animator Controllers (ADR-0032;
    // `fynkel_document::controller`).
    /// A field the schema accepts but the controller cannot use (a
    /// malformed assetRef, a number that is not finite, a parameter's
    /// default of another type).
    pub const CONTROLLER_MALFORMED_ENTRY: Code = Code("FYN-D1100");
    /// A name that does not resolve or is not unique: two states, layers
    /// or parameters with one name, a transition or condition naming
    /// one that is not there, a layer with states and no default state.
    pub const CONTROLLER_NAMES: Code = Code("FYN-D1101");
    /// A condition that does not fit its parameter's type (`greater` on a
    /// bool, `is` on a float), a speed parameter that is not a float, or
    /// an Any State transition with neither conditions nor exit time.
    pub const CONTROLLER_CONDITION: Code = Code("FYN-D1102");
    /// A state's motion is not an animation clip or a blend space.
    pub const CONTROLLER_MOTION: Code = Code("FYN-D1103");

    // Assets (`FYN-A####`, blueprint §19.2c item 8): the asset database.
    /// A source file under `assets/` has no `.meta.json` sidecar.
    pub const ASSET_MISSING_SIDECAR: Code = Code("FYN-A0001");
    /// A `.meta.json` whose source file is gone (moved without it, or deleted).
    pub const ASSET_ORPHANED_SIDECAR: Code = Code("FYN-A0002");
    /// Two assets (documents, sidecars or sub-assets) claim the same id.
    pub const ASSET_DUPLICATE_ID: Code = Code("FYN-A0003");
    /// An `assetRef` names an id the database does not have.
    pub const ASSET_UNRESOLVED: Code = Code("FYN-A0004");
    /// An `assetRef` names a sub-asset its asset does not declare.
    pub const ASSET_UNKNOWN_SUB: Code = Code("FYN-A0005");
    /// A sidecar entry the schema accepts but the database cannot use.
    pub const ASSET_MALFORMED_ENTRY: Code = Code("FYN-A0006");
    /// An `assetRef` resolves, but to an asset of the wrong kind (a model's
    /// material remap naming something that is not a material document).
    pub const ASSET_WRONG_KIND: Code = Code("FYN-A0007");

    // Import (`FYN-A01##`, blueprint §19.2c items 10–12).
    /// The file is not what its extension says, or cannot be parsed.
    pub const IMPORT_UNREADABLE: Code = Code("FYN-A0100");
    /// A resource budget (file size, buffer bytes, node count/depth, vertices, images) was exceeded.
    pub const IMPORT_LIMIT: Code = Code("FYN-A0101");
    /// The file references data outside itself and the import settings do not allow that.
    pub const IMPORT_EXTERNAL_URI: Code = Code("FYN-A0102");
    /// A required extension, encoding or feature the importer does not implement.
    pub const IMPORT_UNSUPPORTED: Code = Code("FYN-A0103");
    /// Accessor, buffer view or index data that does not add up.
    pub const IMPORT_INVALID_DATA: Code = Code("FYN-A0104");
    /// Something was left out of the intermediate (warning; the rest imported).
    pub const IMPORT_SKIPPED: Code = Code("FYN-A0105");
    /// A rig's clip animates a bone its skeleton does not have (item 19b:
    /// clips are matched by bone name; the message names the first).
    pub const RIG_BONE_MISMATCH: Code = Code("FYN-A0106");

    // Project (`FYN-P`, blueprint §19.2c item 13).
    /// No `fynkel.project.json` where one was expected.
    pub const PROJECT_MISSING: Code = Code("FYN-P0001");
    /// `startupScene` does not resolve to a scene document.
    pub const PROJECT_STARTUP_SCENE: Code = Code("FYN-P0002");
    /// A registry entry names a native component type no schema knows.
    pub const PROJECT_UNKNOWN_COMPONENT: Code = Code("FYN-P0003");
    /// A document uses a component type the registry does not list (warning).
    pub const PROJECT_UNREGISTERED_COMPONENT: Code = Code("FYN-P0004");
    /// `quality.default` is not one of `quality.profiles`.
    pub const PROJECT_QUALITY_DEFAULT: Code = Code("FYN-P0005");
    /// No build target is enabled (warning).
    pub const PROJECT_NO_TARGET: Code = Code("FYN-P0006");
    /// The asset root does not exist or is not a directory.
    pub const PROJECT_ASSET_ROOT: Code = Code("FYN-P0007");
    /// An entry of the project file that could not be read as its shape says.
    pub const PROJECT_MALFORMED_ENTRY: Code = Code("FYN-P0008");
    /// An `inputActions` binding the grammar does not accept (§5.1).
    pub const PROJECT_INPUT_BINDING: Code = Code("FYN-P0009");
    /// No `fynkel.lock.json` next to the project file.
    pub const LOCK_MISSING: Code = Code("FYN-P0010");
    /// The lock records versions this build does not match.
    pub const LOCK_MISMATCH: Code = Code("FYN-P0011");
    /// The lock file is not valid (JSON, schema).
    pub const LOCK_MALFORMED: Code = Code("FYN-P0012");
    /// `physics.gravity` is not three finite numbers (M3-08).
    pub const PROJECT_PHYSICS: Code = Code("FYN-P0013");
    /// `layers` has more than 32 names or does not start with `default`,
    /// `tags` more than 32; a name is empty or listed twice (ADR-0020).
    pub const PROJECT_LAYERS_TAGS: Code = Code("FYN-P0014");
    /// A file of the game package outside `lib/editor/` imports editor
    /// code: `package:fynkel_editor_api` or the package's own
    /// `lib/editor/` (ADR-0028 §2), which would ship it in the game.
    pub const PROJECT_EDITOR_IMPORT: Code = Code("FYN-P0015");
    /// A behaviour or data type id two packages declare, or a package and
    /// the game (ADR-0029 §2): one registry holds every package's types.
    pub const PACKAGE_TYPE_CLASH: Code = Code("FYN-P0016");
    /// A package's behaviour or data type whose id does not start with the
    /// package's namespace (`pickups.Pickup`, ADR-0029 §2).
    pub const PACKAGE_TYPE_NAMESPACE: Code = Code("FYN-P0017");
    /// A document uses a component of a module the project turned off
    /// (`modules` in `fynkel.project.json`, ADR-0029 §3): `fynkel.Collider`
    /// with physics off.
    pub const MODULE_OFF: Code = Code("FYN-P0018");
    /// A dependency's `fynkel.package.json` that could not be read: not
    /// JSON, an unknown field, a newer `formatVersion`, an asset folder
    /// outside the package or a namespace that is not an identifier
    /// (ADR-0029 §1), or a namespace the engine or the game owns (`fynkel`,
    /// `game`). The package is then not a Fynkel package.
    pub const PACKAGE_MANIFEST: Code = Code("FYN-P0019");

    // Physics (`FYN-H`, blueprint §9, §19.2d item 8): scene rules the
    // component schemas cannot express.
    /// A `fynkel.RigidBody` value the runtime would reject (mass, damping).
    pub const PHYSICS_BODY_VALUE: Code = Code("FYN-H0001");
    /// A `fynkel.Collider` value the runtime would reject (size per shape,
    /// friction, restitution, a mesh collider without `mesh`).
    pub const PHYSICS_COLLIDER_VALUE: Code = Code("FYN-H0002");
    /// A `shape: mesh` collider on a dynamic or kinematic body: mesh
    /// colliders are static only.
    pub const PHYSICS_MESH_ON_MOVING_BODY: Code = Code("FYN-H0003");
    /// A dynamic `RigidBody` without a `Collider` (warning): it falls
    /// through everything.
    pub const PHYSICS_BODY_WITHOUT_COLLIDER: Code = Code("FYN-H0004");
    /// A `fynkel.CharacterController` value the runtime would reject
    /// (capsule, slope, speeds, mass; M3-09).
    pub const CHARACTER_VALUE: Code = Code("FYN-H0005");
    /// A `fynkel.CharacterController` next to a `RigidBody` or `Collider`:
    /// the controller is its own kinematic capsule and ignores them.
    pub const CHARACTER_WITH_BODY: Code = Code("FYN-H0006");
    /// A `fynkel.ThirdPersonCamera` value the runtime would reject
    /// (distances, pitch range, sensitivity).
    pub const CAMERA_RIG_VALUE: Code = Code("FYN-H0007");
    /// A catalogue component value the runtime would reject (`Rotator`,
    /// `Oscillate`, `Follow`, `LookAtCamera`, `LifetimeDespawn`,
    /// `Emitter`: axes, rates, lifetimes; M3-10).
    pub const CATALOGUE_VALUE: Code = Code("FYN-H0008");
    /// A `fynkel.TriggerZone` on an entity without a sensor `Collider`
    /// (`is_sensor: true`): it never fires (M3-12).
    pub const TRIGGER_ZONE_WITHOUT_SENSOR: Code = Code("FYN-H0009");
    /// A `fynkel.AudioSource`/`fynkel.AudioListener` value the runtime
    /// would reject (a volume outside `0..=1`, a pitch outside
    /// `0.05..=8`; M3-11).
    pub const AUDIO_VALUE: Code = Code("FYN-H0010");
    /// A `fynkel.Volume` value the runtime would reject (a weight outside
    /// `0..=1`, a non-positive size or radius, a negative blend distance;
    /// R3a).
    pub const VOLUME_VALUE: Code = Code("FYN-H0011");
    /// A `fynkel.Layer` or `fynkel.Tags` that names a layer or tag the
    /// project does not declare (ADR-0020).
    pub const UNDECLARED_LAYER_OR_TAG: Code = Code("FYN-H0012");

    // Editing (`FYN-E`, blueprint §11.1, §19.2c item 14): authoring commands.
    /// The transaction's base revision is not the document's current one.
    pub const EDIT_CONFLICT: Code = Code("FYN-E0001");
    /// A command names an entity the document does not have.
    pub const EDIT_UNKNOWN_ENTITY: Code = Code("FYN-E0002");
    /// A command names a component the entity does not have.
    pub const EDIT_UNKNOWN_COMPONENT: Code = Code("FYN-E0003");
    /// A command is malformed (missing or mistyped argument).
    pub const EDIT_INVALID_COMMAND: Code = Code("FYN-E0004");
    /// The transaction would leave the document invalid; nothing was applied
    /// (the validation errors are `related`).
    pub const EDIT_REJECTED: Code = Code("FYN-E0005");
    /// A reparent would make an entity its own ancestor.
    pub const EDIT_CYCLE: Code = Code("FYN-E0006");
    /// The document cannot be edited (read-only, outdated, or not a kind
    /// with entities).
    pub const EDIT_NOT_EDITABLE: Code = Code("FYN-E0007");
    /// The entity is not a prefab instance, or the override does not exist.
    pub const EDIT_NOT_AN_INSTANCE: Code = Code("FYN-E0008");
    /// Nothing to undo / redo.
    pub const EDIT_NO_HISTORY: Code = Code("FYN-E0009");
    /// An id the command would create already exists in the document.
    pub const EDIT_DUPLICATE_ID: Code = Code("FYN-E0010");
    /// A save or file creation failed.
    pub const EDIT_IO: Code = Code("FYN-E0011");
    /// A document-level pointer a command may not touch (the envelope's
    /// `kind`/`formatVersion`/`id`) or that does not resolve.
    pub const EDIT_INVALID_POINTER: Code = Code("FYN-E0012");

    // Authoring session and ABI (`FYN-T`, blueprint §19.2e item 1): the
    // editor's stateful service over the commands above. Never document
    // content — those stay `FYN-D`/`FYN-E`.
    /// No project is open in the session, or the project could not be
    /// opened (the `FYN-P` reasons are `related`).
    pub const SESSION_NO_PROJECT: Code = Code("FYN-T0001");
    /// A document handle the session does not have (closed, or never opened).
    pub const SESSION_UNKNOWN_DOCUMENT: Code = Code("FYN-T0002");
    /// A request to the session is malformed (not UTF-8, not JSON, an
    /// argument of the wrong shape).
    pub const SESSION_INVALID_REQUEST: Code = Code("FYN-T0003");
    /// A query kind the session does not know.
    pub const SESSION_UNKNOWN_QUERY: Code = Code("FYN-T0004");
    /// A call panicked; the session is poisoned and must be destroyed.
    pub const SESSION_PANIC: Code = Code("FYN-T0005");
    /// A document could not be read or written by the session (the path
    /// does not exist, is not a document, or the file system refused).
    pub const SESSION_IO: Code = Code("FYN-T0006");

    // Runtime loading and residency (`FYN-R`, blueprint §19.2d item 3): a
    // host reads the cooked cache only, and every miss is an error that
    // names the asset, never a fallback to source documents.
    /// The cache directory has no readable `index.json` (not cooked yet, or
    /// not a cache at all).
    pub const RUNTIME_CACHE_UNREADABLE: Code = Code("FYN-R0001");
    /// The cache was cooked by a different build (schema fingerprint or
    /// cache version): re-run `fynkel cook`.
    pub const RUNTIME_CACHE_STALE: Code = Code("FYN-R0002");
    /// The asset id is not in the cache index.
    pub const RUNTIME_ASSET_MISSING: Code = Code("FYN-R0003");
    /// The index lists the asset but its cook failed (no object).
    pub const RUNTIME_ASSET_FAILED: Code = Code("FYN-R0004");
    /// The cooked object cannot be read (file missing, container damaged).
    pub const RUNTIME_OBJECT_UNREADABLE: Code = Code("FYN-R0005");
    /// The cooked object has no chunk of the requested kind for the
    /// sub-asset (a mesh reference into a texture, an unknown `sub`, a
    /// custom-shader material asked for as `MATL`).
    pub const RUNTIME_CHUNK_MISSING: Code = Code("FYN-R0006");
    /// The chunk's payload does not decode (a cache written by another
    /// version): re-run `fynkel cook --force`.
    pub const RUNTIME_PAYLOAD_MALFORMED: Code = Code("FYN-R0007");
    /// The cooked scene does not describe (component data this build
    /// rejects, hierarchy errors): re-run `fynkel cook`.
    pub const RUNTIME_SCENE_INVALID: Code = Code("FYN-R0008");
    /// The cache names no startup scene (`fynkel cook` ran without a
    /// project file).
    pub const RUNTIME_NO_STARTUP_SCENE: Code = Code("FYN-R0009");
    /// `unload_scene` named a scene the world has not loaded (M3-12).
    pub const RUNTIME_SCENE_NOT_LOADED: Code = Code("FYN-R0010");
    /// An additive load named a scene the world already holds (M3-12).
    pub const RUNTIME_SCENE_ALREADY_LOADED: Code = Code("FYN-R0011");
    /// A projection request with more than 64 anchors (M3-13, blueprint
    /// §10): the first 64 project, the rest are dropped. The world reports
    /// it as `DiagnosticCode::TOO_MANY_ANCHORS`; the SDK reports the same
    /// under this code when it caps what it sends.
    pub const RUNTIME_TOO_MANY_ANCHORS: Code = Code("FYN-R0012");
    /// A tick's snapshot exceeded the host's subscription budget (M3-16,
    /// blueprint C5): the host subscribed more than it meant to pay for.
    /// The world reports it as `DiagnosticCode::SNAPSHOT_BUDGET`, once per
    /// crossing; the snapshot is still delivered whole.
    pub const RUNTIME_SNAPSHOT_BUDGET: Code = Code("FYN-R0013");
    /// A command of a module the project turned off (ADR-0029 §3); the
    /// world reports it as `DiagnosticCode::MODULE_OFF`.
    pub const RUNTIME_MODULE_OFF: Code = Code("FYN-R0014");

    // User settings (`FYN-U`, blueprint §19.2d item 12): `settings.json`.
    /// The settings file is not a settings document (wrong `fynkel` kind
    /// or `formatVersion`, not an object).
    pub const SETTINGS_NOT_A_DOCUMENT: Code = Code("FYN-U0001");
    /// A settings value is out of range (a volume outside `0..=1`, an
    /// unknown quality profile, a binding the grammar rejects).
    pub const SETTINGS_VALUE: Code = Code("FYN-U0002");
    /// The settings file could not be read or written.
    pub const SETTINGS_IO: Code = Code("FYN-U0003");

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// The §14.2 record. Every optional field is absent rather than empty when
/// unknown, so consumers can tell "no pointer" from "root".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    /// Document path as given to the tool (not canonicalised).
    pub document: Option<String>,
    /// RFC 6901 JSON pointer into the document.
    pub pointer: Option<String>,
    /// Persistent object / component / asset id the diagnostic is about.
    pub id: Option<String>,
    /// 1-based, when the source position is known.
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub suggestion: Option<String>,
    pub related: Vec<Diagnostic>,
}

impl Diagnostic {
    pub fn new(code: Code, severity: Severity, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code,
            severity,
            message: message.into(),
            document: None,
            pointer: None,
            id: None,
            line: None,
            column: None,
            suggestion: None,
            related: Vec::new(),
        }
    }

    pub fn error(code: Code, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(code, Severity::Error, message)
    }

    pub fn warning(code: Code, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(code, Severity::Warning, message)
    }

    pub fn with_document(mut self, document: impl Into<String>) -> Diagnostic {
        self.document = Some(document.into());
        self
    }

    pub fn with_pointer(mut self, pointer: impl Into<String>) -> Diagnostic {
        self.pointer = Some(pointer.into());
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Diagnostic {
        self.id = Some(id.into());
        self
    }

    pub fn at(mut self, line: u32, column: u32) -> Diagnostic {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Diagnostic {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn with_related(mut self, related: Diagnostic) -> Diagnostic {
        self.related.push(related);
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// True when any diagnostic is an error.
pub fn has_errors(list: &[Diagnostic]) -> bool {
    list.iter().any(Diagnostic::is_error)
}

impl std::fmt::Display for Diagnostic {
    /// `severity[code] document:line:col pointer: message` — the human
    /// form; the CLI emits JSON for tools.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}[{}]", self.severity.as_str(), self.code.as_str())?;
        if let Some(d) = &self.document {
            write!(f, " {d}")?;
            if let (Some(l), Some(c)) = (self.line, self.column) {
                write!(f, ":{l}:{c}")?;
            }
        }
        if let Some(p) = &self.pointer {
            write!(f, " {}", if p.is_empty() { "/" } else { p })?;
        }
        write!(f, ": {}", self.message)?;
        if let Some(s) = &self.suggestion {
            write!(f, " ({s})")?;
        }
        Ok(())
    }
}
