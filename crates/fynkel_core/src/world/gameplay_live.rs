//! Editor data writes use the same atomic gameplay journal as game code.

use std::collections::BTreeMap;

use super::gameplay::{adopt_command, EditError, GameplayEntity};
use super::World;
use crate::{Command, ComponentId, Diagnostic, DiagnosticCode, Entity, Lane};

impl World {
    /// Applies one atomic batch of live editor data changes without advancing time.
    /// Validation and reference checks are identical to gameplay edits. Failed
    /// batches publish nothing and leave any queued simulation work untouched.
    /// Existing queued work must be drained or explicitly adopted by the host;
    /// editor calls never silently commit it. Successful poses are visible even
    /// while paused, with interpolation reset by the shared commit path.
    ///
    /// This data boundary accepts destroy, transform, hierarchy, name, activity,
    /// and component changes. Scene-aware creation uses `spawn_now`, which opens
    /// the same journal and publishes scene identity after successful commit;
    /// runtime effects and global operations require their own explicit boundary.
    /// Unlike `apply_now`, commands in this batch do not succeed independently.
    /// Failures are returned and also appended to `diagnostics`.
    pub fn apply_gameplay_now(&mut self, commands: Vec<Command>) -> Vec<Diagnostic> {
        let mut failed_index = u32::MAX;
        let result = self.edit_gameplay(|edit| {
            let mut pending = BTreeMap::new();
            for (index, command) in commands.iter().enumerate() {
                failed_index = index as u32;
                if !matches!(
                    command,
                    Command::Destroy(_)
                        | Command::SetTransform { .. }
                        | Command::SetParent { .. }
                        | Command::SetName { .. }
                        | Command::SetActive { .. }
                        | Command::SetComponent { .. }
                        | Command::RemoveComponent { .. }
                ) {
                    return Err(EditError::InvalidEffect(
                        GameplayEntity::Live(command.target().unwrap_or(Entity::NULL)),
                        "live data edits do not accept creation or runtime effects".into(),
                    ));
                }
                adopt_command(edit, &mut pending, command)?;
            }
            // A final-state or boundary failure belongs to the batch itself.
            failed_index = u32::MAX;
            Ok(())
        });
        match result {
            Ok(_) => Vec::new(),
            Err(error) => {
                let component = match &error {
                    EditError::UnknownComponent(id)
                    | EditError::UnsupportedComponent(id)
                    | EditError::ProtectedComponent(id)
                    | EditError::MissingComponent(_, id)
                    | EditError::InvalidComponent { component: id, .. } => *id,
                    _ => ComponentId::NONE,
                };
                let message = match component.name() {
                    Some(name) => format!("{name}: {error}"),
                    None => error.to_string(),
                };
                self.reject(
                    failed_index,
                    Lane::Simulation,
                    DiagnosticCode::INVALID_PAYLOAD,
                    commands
                        .get(failed_index as usize)
                        .and_then(Command::target)
                        .unwrap_or(Entity::NULL),
                    component,
                    message,
                );
                vec![self.diagnostics.last().expect("just rejected").clone()]
            }
        }
    }
}
