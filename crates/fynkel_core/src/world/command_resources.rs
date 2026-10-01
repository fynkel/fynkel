//! Host-owned resource preparation for live edits and immediate audio.
//! Core depends on the publication contract, never on an asset store or cooker.

use crate::{Command, EditResult, GameplayEdit, World};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::Arc;

/// Prepares dependencies without changing world registries. An atomic live edit
/// uses one plan for the whole batch; immediate sound prepares its clip.
pub trait CommandResourceSource: Send + Sync {
    fn prepare_sound(
        &self,
        world: &World,
        clip: &crate::AssetRef,
    ) -> Box<dyn PreparedCommandResources>;
    fn prepare(&self, world: &World, commands: &[&Command]) -> Box<dyn PreparedCommandResources>;
}

/// Owned decoded dependencies. Dropping an unpublished plan must have no world
/// effects. Publication must be infallible after successful preflight; a panic
/// poisons the world because already-published data cannot be rolled back.
pub trait PreparedCommandResources {
    fn preflight(&self, world: &World) -> EditResult<()>;
    fn stage(&self, edit: &mut GameplayEdit<'_>) -> EditResult<()>;
    fn publish(self: Box<Self>, world: &mut World);
}

impl World {
    /// Installs the host's cooked-resource source for live edits and immediate sound.
    /// Prepared resources publish only after successful validation and commit.
    pub fn set_command_resource_source(&mut self, source: Option<Arc<dyn CommandResourceSource>>) {
        self.command_resources = source;
    }

    pub(super) fn prepare_command_resources(
        &self,
        commands: &[&Command],
    ) -> EditResult<Option<Box<dyn PreparedCommandResources>>> {
        let Some(source) = &self.command_resources else {
            return Ok(None);
        };
        let plan = source.prepare(self, commands);
        plan.preflight(self)?;
        Ok(Some(plan))
    }

    pub(crate) fn publish_command_resources(
        &mut self,
        plan: Option<Box<dyn PreparedCommandResources>>,
    ) {
        if let Some(plan) = plan {
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| plan.publish(self))) {
                self.poison();
                resume_unwind(panic);
            }
        }
    }
}
