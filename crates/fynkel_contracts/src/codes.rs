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
    pub const INVALID_TEMP_ID: DiagnosticCode = DiagnosticCode(3);
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

    pub const fn name(self) -> &'static str {
        match self.0 {
            1 => "unknown_command",
            2 => "stale_entity",
            3 => "invalid_temp_id",
            4 => "invalid_payload",
            5 => "unknown_component",
            6 => "missing_component",
            7 => "hierarchy_cycle",
            8 => "wrong_world",
            9 => "physics_owned",
            10 => "missed_deadline",
            11 => "duplicate_component",
            _ => "unknown",
        }
    }
}
