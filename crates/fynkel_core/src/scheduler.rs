//! Sequential registered schedule. Dependencies, rather than registration order,
//! define execution. Native callbacks publish one gameplay transaction each.
use crate::{EditResult, GameplayEdit, Module};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SystemPhase {
    Input,
    Simulation,
    Physics,
    PostPhysics,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemSpec {
    pub id: String,
    pub phase: SystemPhase,
    pub before: Vec<String>,
    pub after: Vec<String>,
    /// Component identities, or `*` for unrestricted native engine work.
    /// Declarations are an audit contract, not permission for parallel execution.
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub structural: bool,
    pub requires: Vec<Module>,
}
impl SystemSpec {
    pub fn new(id: impl Into<String>, phase: SystemPhase) -> Self {
        Self {
            id: id.into(),
            phase,
            before: vec![],
            after: vec![],
            reads: vec![],
            writes: vec![],
            structural: false,
            requires: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    InvalidIdentity(String),
    Duplicate(String),
    UnknownDependency(String, String),
    ReversedPhase(String, String),
    Cycle(Vec<String>),
    MissingModule(String, Module),
    UnknownComponent(String, String),
    ReservedIdentity(String),
    Poisoned,
    UnsupportedPhase(String),
}
impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "schedule: {self:?}")
    }
}
impl std::error::Error for ScheduleError {}
#[derive(Clone, Copy, Debug)]
pub struct SystemContext {
    pub tick: crate::Tick,
    pub delta_seconds: f32,
}
pub type SystemCallback = fn(&mut GameplayEdit<'_>, SystemContext) -> EditResult<u32>;
#[derive(Clone)]
pub struct RegisteredSystem {
    pub spec: SystemSpec,
    pub run: SystemCallback,
}
#[derive(Clone, Debug)]
pub struct RegisteredSystemTime {
    pub id: String,
    pub micros: u64,
    pub entities: u32,
    pub rejected: bool,
}

/// Compile a complete registration batch. No partially valid schedule is installed.
/// Stable lexical identity breaks ties. Cross-phase forward edges are allowed;
/// backward edges are errors even when no cycle would result.
pub fn compile_schedule(specs: &[SystemSpec]) -> Result<Vec<usize>, ScheduleError> {
    let mut ids = BTreeMap::new();
    for (i, spec) in specs.iter().enumerate() {
        if !spec.id.contains('.')
            || spec
                .id
                .split('.')
                .any(|s| !crate::registry::identifier_word(s))
        {
            return Err(ScheduleError::InvalidIdentity(spec.id.clone()));
        }
        if ids.insert(spec.id.clone(), i).is_some() {
            return Err(ScheduleError::Duplicate(spec.id.clone()));
        }
    }
    let mut edges = vec![BTreeSet::new(); specs.len()];
    let mut indegree = vec![0usize; specs.len()];
    for (i, spec) in specs.iter().enumerate() {
        for (dependency, before) in spec
            .before
            .iter()
            .map(|s| (s, true))
            .chain(spec.after.iter().map(|s| (s, false)))
        {
            let &j = ids.get(dependency).ok_or_else(|| {
                ScheduleError::UnknownDependency(spec.id.clone(), dependency.clone())
            })?;
            let (a, b) = if before { (i, j) } else { (j, i) };
            if specs[a].phase > specs[b].phase {
                return Err(ScheduleError::ReversedPhase(
                    specs[a].id.clone(),
                    specs[b].id.clone(),
                ));
            }
            if edges[a].insert(b) {
                indegree[b] += 1;
            }
        }
    }
    // Implicit phase boundaries are also edges. A ready later phase cannot
    // leap over a cycle in an earlier phase.
    for a in 0..specs.len() {
        for b in 0..specs.len() {
            if specs[a].phase < specs[b].phase && edges[a].insert(b) {
                indegree[b] += 1;
            }
        }
    }
    let mut ready = BTreeSet::new();
    for (i, s) in specs.iter().enumerate() {
        if indegree[i] == 0 {
            ready.insert((s.phase, s.id.clone(), i));
        }
    }
    let mut result = Vec::new();
    while let Some((_, _, i)) = ready.pop_first() {
        result.push(i);
        for &j in &edges[i] {
            indegree[j] -= 1;
            if indegree[j] == 0 {
                ready.insert((specs[j].phase, specs[j].id.clone(), j));
            }
        }
    }
    if result.len() != specs.len() {
        return Err(ScheduleError::Cycle(
            specs
                .iter()
                .enumerate()
                .filter(|(i, _)| indegree[*i] != 0)
                .map(|(_, s)| s.id.clone())
                .collect(),
        ));
    }
    Ok(result)
}
