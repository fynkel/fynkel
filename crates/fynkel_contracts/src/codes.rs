//! Status and diagnostic codes (ADR-0014). Stable: append only.

/// Result of an ABI call. `0` is success; positive values are recoverable
/// (the call did what it could and diagnostics say what was rejected);
/// `POISONED`/`PANIC` mean the world must be destroyed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    /// Some commands were rejected; see diagnostics.
    CommandsRejected = 1,
    InvalidArgument = 2,
    /// Malformed buffer; nothing was applied.
    Protocol = 3,
    /// A previous panic poisoned the world.
    Poisoned = 4,
    /// This call panicked; the world is now poisoned.
    Panic = 5,
    OutOfResources = 6,
    /// Version / schema fingerprint mismatch.
    Incompatible = 7,
    /// A scene or project could not be loaded; the load report says why.
    LoadFailed = 8,
    /// The build or the platform lacks what the call needs (an audio
    /// device with the library built without one, no output device); the
    /// world stays usable (M3-11).
    Unsupported = 9,
}

impl Status {
    pub const fn from_i32(v: i32) -> Option<Status> {
        Some(match v {
            0 => Status::Ok,
            1 => Status::CommandsRejected,
            2 => Status::InvalidArgument,
            3 => Status::Protocol,
            4 => Status::Poisoned,
            5 => Status::Panic,
            6 => Status::OutOfResources,
            7 => Status::Incompatible,
            8 => Status::LoadFailed,
            9 => Status::Unsupported,
            _ => return None,
        })
    }
}

/// Per-command diagnostic codes. Shared with the CLI `--json` output.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
pub struct DiagnosticCode(pub u32);

impl DiagnosticCode {
    pub const UNKNOWN_COMMAND: DiagnosticCode = DiagnosticCode(1);
    pub const STALE_ENTITY: DiagnosticCode = DiagnosticCode(2);
    // Code 3 is reserved (retired temporary entity IDs).
    pub const INVALID_PAYLOAD: DiagnosticCode = DiagnosticCode(4);
    pub const UNKNOWN_COMPONENT: DiagnosticCode = DiagnosticCode(5);
    pub const MISSING_COMPONENT: DiagnosticCode = DiagnosticCode(6);
    pub const HIERARCHY_CYCLE: DiagnosticCode = DiagnosticCode(7);
    pub const WRONG_WORLD: DiagnosticCode = DiagnosticCode(8);
    /// Presentation command on a physics-owned transform (§5.1).
    pub const PHYSICS_OWNED: DiagnosticCode = DiagnosticCode(9);
    /// Time dropped after `max_steps_per_frame` catch-up ticks.
    pub const MISSED_DEADLINE: DiagnosticCode = DiagnosticCode(10);
    pub const DUPLICATE_COMPONENT: DiagnosticCode = DiagnosticCode(11);
    // Code 12 is reserved (retired command submission lanes).
    /// A `shape: mesh` collider whose mesh is not available for collision
    /// (not cooked, not registered, or over the triangle limit); the
    /// collider is skipped (M3-08).
    pub const MESH_UNAVAILABLE: DiagnosticCode = DiagnosticCode(13);
    /// A `fynkel.Emitter` whose prefab the world has not been given
    /// (`World::register_prefab` — the loaders register every prefab a
    /// loaded scene's emitters name); reported once, nothing is spawned
    /// (M3-10).
    pub const PREFAB_UNAVAILABLE: DiagnosticCode = DiagnosticCode(14);
    /// A projection request (`World::project`, M3-13) with more than
    /// `MAX_PROJECTION_ANCHORS` (4096) anchors: the first 4096 are projected, the
    /// rest come back `PROJECTED_DROPPED`.
    pub const TOO_MANY_ANCHORS: DiagnosticCode = DiagnosticCode(15);
    /// A `fynkel.Animator` whose clip the world has not been given
    /// (`World::register_clip` — the loaders register every clip a loaded
    /// scene's animators name and every one a `Play`/`Crossfade` asks
    /// for); reported once per clip, nothing is animated (M3-19).
    pub const CLIP_UNAVAILABLE: DiagnosticCode = DiagnosticCode(16);
    /// A `fynkel.Skin` whose skin the world has not been given
    /// (`World::register_skin`); reported once, the mesh draws in its bind
    /// pose (M3-19).
    pub const SKIN_UNAVAILABLE: DiagnosticCode = DiagnosticCode(17);
    /// A `fynkel.AudioSource` (or a `PlaySound`) naming a clip the world
    /// has not been given (`World::register_sound` — the loaders register
    /// every clip a loaded scene's sources name and every one a
    /// `PlaySound`/`play` asks for); reported once per clip, nothing
    /// plays (M3-11).
    pub const SOUND_UNAVAILABLE: DiagnosticCode = DiagnosticCode(18);
    /// The output device could not be opened (`fynkel_world_audio_open`);
    /// the world keeps playing on the null backend, silently (M3-11).
    pub const AUDIO_DEVICE: DiagnosticCode = DiagnosticCode(19);
    /// A tick's snapshot exceeded the host's subscription budget
    /// (`fynkel_world_set_snapshot_budget`, blueprint C5); reported once
    /// per crossing, informational — the snapshot is still published whole
    /// (M3-16).
    pub const SNAPSHOT_BUDGET: DiagnosticCode = DiagnosticCode(20);
    /// A command of a module the project turned off (ADR-0029 §3): a body
    /// command with physics off, `Animate` with animation off, an audio
    /// command with audio off. Rejected; the module's systems do not run.
    pub const MODULE_OFF: DiagnosticCode = DiagnosticCode(21);
    /// An IK goal (ADR-0033 § 5) that cannot run: a node it names is
    /// not under the animator, or foot placement with the physics module
    /// off. Reported once per goal; the goal is skipped.
    pub const IK_UNAVAILABLE: DiagnosticCode = DiagnosticCode(22);
    pub const ANIMATION_BINDING_UNAVAILABLE: DiagnosticCode = DiagnosticCode(23);

    pub const fn name(self) -> &'static str {
        match self.0 {
            1 => "unknown_command",
            2 => "stale_entity",
            4 => "invalid_payload",
            5 => "unknown_component",
            6 => "missing_component",
            7 => "hierarchy_cycle",
            8 => "wrong_world",
            9 => "physics_owned",
            10 => "missed_deadline",
            11 => "duplicate_component",
            13 => "mesh_unavailable",
            14 => "prefab_unavailable",
            15 => "too_many_anchors",
            16 => "clip_unavailable",
            17 => "skin_unavailable",
            18 => "sound_unavailable",
            19 => "audio_device",
            20 => "snapshot_budget",
            21 => "module_off",
            22 => "ik_unavailable",
            23 => "animation_binding_unavailable",
            _ => "unknown",
        }
    }
}
