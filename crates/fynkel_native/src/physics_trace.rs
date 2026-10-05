//! Opt-in bounded diagnostics at the host step boundary; never game state or ABI.
use fynkel_core::physics::PhysicsStepProfile;
use std::{
    fs::File,
    io::Write,
    sync::{Arc, Mutex, OnceLock},
};

const LIMIT: usize = 8192;
pub(crate) struct PhysicsTrace {
    file: File,
    rows: usize,
}
pub(crate) type Shared = Arc<Mutex<PhysicsTrace>>;

pub(crate) fn from_env() -> Option<Shared> {
    static TRACE: OnceLock<Option<Shared>> = OnceLock::new();
    TRACE
        .get_or_init(|| {
            let mut path = std::path::PathBuf::from(std::env::var_os("FYNKEL_TRACE_PHYSICS_FILE")?);
            if path.is_relative() {
                path = std::path::PathBuf::from(std::env::var_os("HOME")?).join(path);
            }
            PhysicsTrace::new(File::create(path).ok()?)
                .map(|trace| Arc::new(Mutex::new(trace)))
                .ok()
        })
        .clone()
}

impl PhysicsTrace {
    fn new(mut file: File) -> std::io::Result<Self> {
        writeln!(file, "world,tick,ran,sync_ns,commands_ns,characters_ns,solver_ns,writeback_ns,events_ns,total_ns,collect_ns,hierarchy_ns,classify_ns,apply_ns,propagation_ns,core_adapter_ns,publication_ns,core_total_ns")?;
        Ok(Self { file, rows: 0 })
    }
    pub(crate) fn active(&self) -> bool {
        self.rows < LIMIT
    }
    pub(crate) fn record(
        &mut self,
        world: u64,
        tick: u64,
        profile: Option<PhysicsStepProfile>,
        core: Option<fynkel_core::PhysicsSystemProfile>,
    ) {
        if !self.active() {
            return;
        }
        let ran = profile.is_some() as u8;
        let p = profile.unwrap_or_default();
        let core = core.unwrap_or_default();
        // A skipped physics stage is explicit, never the preceding tick's sample.
        if writeln!(
            self.file,
            "{world},{tick},{ran},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            p.sync.as_nanos(),
            p.commands.as_nanos(),
            p.characters.as_nanos(),
            p.solver.as_nanos(),
            p.writeback.as_nanos(),
            p.events.as_nanos(),
            p.total.as_nanos(),
            p.synchronization.collect.as_nanos(),
            p.synchronization.hierarchy.as_nanos(),
            p.synchronization.classify.as_nanos(),
            p.synchronization.apply.as_nanos(),
            core.propagation.as_nanos(),
            core.adapter.as_nanos(),
            core.publication.as_nanos(),
            core.total.as_nanos()
        )
        .is_err()
        {
            self.rows = LIMIT;
        } else {
            self.rows += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_trace_distinguishes_worlds_and_skipped_steps() {
        let path = std::env::temp_dir().join(format!(
            "fynkel-physics-{}-{}.csv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut trace = PhysicsTrace::new(File::create(&path).unwrap()).unwrap();
        trace.record(
            1,
            1,
            Some(PhysicsStepProfile {
                synchronization: fynkel_core::physics::PhysicsSyncProfile {
                    collect: std::time::Duration::from_nanos(1),
                    classify: std::time::Duration::from_nanos(2),
                    ..Default::default()
                },
                sync: std::time::Duration::from_nanos(3),
                total: std::time::Duration::from_nanos(5),
                ..Default::default()
            }),
            Some(fynkel_core::PhysicsSystemProfile {
                propagation: std::time::Duration::from_nanos(2),
                adapter: std::time::Duration::from_nanos(5),
                publication: std::time::Duration::from_nanos(1),
                total: std::time::Duration::from_nanos(8),
            }),
        );
        trace.record(2, 1, None, None);
        for tick in 2..LIMIT as u64 + 10 {
            trace.record(1, tick, None, None);
        }
        assert!(!trace.active());
        drop(trace);
        let text = std::fs::read_to_string(&path).unwrap();
        let rows: Vec<_> = text.lines().collect();
        assert_eq!(rows.len(), LIMIT + 1);
        assert_eq!(rows[1], "1,1,1,3,0,0,0,0,0,5,1,0,2,0,2,5,1,8");
        assert_eq!(rows[2], "2,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0");
        std::fs::remove_file(path).unwrap();
    }
}
