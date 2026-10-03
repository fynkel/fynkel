//! Editor data writes use the same atomic gameplay journal as game code.

use super::gameplay::{EditError, GameplayEntity};
use super::World;
use crate::{Command, ComponentId, Diagnostic, DiagnosticCode, Entity, Lane};

impl World {
    /// Atomic editor component replacements, including registered authored data.
    /// Resource preparation and publication follow the ordinary gameplay journal.
    pub fn set_live_components(
        &mut self,
        values: Vec<(Entity, crate::ComponentValue)>,
        registered: Vec<(Entity, crate::RegisteredDescription)>,
    ) -> Vec<Diagnostic> {
        match self.try_set_live_components(values, registered) {
            Ok(()) => Vec::new(),
            Err(error) => {
                self.reject(
                    u32::MAX,
                    Lane::Simulation,
                    DiagnosticCode::INVALID_PAYLOAD,
                    Entity::NULL,
                    ComponentId::NONE,
                    error.to_string(),
                );
                vec![self
                    .publication
                    .diagnostics
                    .last()
                    .expect("just rejected")
                    .clone()]
            }
        }
    }

    fn try_set_live_components(
        &mut self,
        values: Vec<(Entity, crate::ComponentValue)>,
        registered: Vec<(Entity, crate::RegisteredDescription)>,
    ) -> Result<(), EditError> {
        let commands: Vec<_> = values
            .into_iter()
            .map(|(entity, value)| Command::SetComponent { entity, value })
            .collect();
        let resources = self.prepare_command_resources(&commands.iter().collect::<Vec<_>>())?;
        self.edit_gameplay(|edit| {
            if let Some(resources) = &resources {
                resources.stage(edit)?;
            }
            for command in &commands {
                if let Command::SetComponent { entity, value } = command {
                    edit.set_component(*entity, value.clone())?;
                }
            }
            for (entity, value) in &registered {
                edit.set_registered_authored((*entity).into(), value, |_| None)?;
            }
            Ok(())
        })?;
        self.publish_command_resources(resources);
        Ok(())
    }

    /// Applies one atomic batch of live editor data changes without advancing time.
    /// Validation and reference checks are identical to gameplay edits. Failed
    /// batches publish nothing. Successful poses are visible even
    /// while paused, with interpolation reset by the shared commit path.
    ///
    /// This data boundary accepts destroy, transform, hierarchy, name, activity,
    /// and component changes. Scene-aware creation uses `spawn_now`, which opens
    /// the same journal and publishes scene identity after successful commit;
    /// runtime effects and global operations require their own explicit boundary.
    /// Commands in this batch never succeed independently.
    /// Failures are returned and also appended to `diagnostics`.
    pub fn apply_gameplay_now(&mut self, commands: Vec<Command>) -> Vec<Diagnostic> {
        let mut failed_index = u32::MAX;
        let result = self
            .prepare_command_resources(&commands.iter().collect::<Vec<_>>())
            .and_then(|resources| {
                let commit = self.edit_gameplay(|edit| {
                    if let Some(resources) = &resources {
                        resources.stage(edit)?;
                    }
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
                        match command {
                            Command::Destroy(e) => edit.destroy(*e)?,
                            Command::SetTransform { entity, transform } => {
                                edit.set_transform(*entity, *transform)?
                            }
                            Command::SetParent {
                                child,
                                parent,
                                keep_world,
                            } => edit.set_parent(*child, parent.map(Into::into), *keep_world)?,
                            Command::SetName { entity, name } => {
                                edit.set_name(*entity, name.clone())?
                            }
                            Command::SetActive { entity, active } => {
                                edit.set_active(*entity, *active)?
                            }
                            Command::SetComponent { entity, value } => {
                                edit.set_component(*entity, value.clone())?
                            }
                            Command::RemoveComponent { entity, component } => {
                                edit.remove_component(*entity, *component)?
                            }
                            _ => unreachable!("data operations checked above"),
                        }
                    }
                    // A final-state or boundary failure belongs to the batch itself.
                    failed_index = u32::MAX;
                    Ok(())
                })?;
                self.publish_command_resources(resources);
                Ok(commit)
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
                vec![self
                    .publication
                    .diagnostics
                    .last()
                    .expect("just rejected")
                    .clone()]
            }
        }
    }
}
