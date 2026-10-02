//! Bridge-facing revisions and render membership have no authority over data.
use super::*;
#[derive(Default)]
pub(super) struct TableRevision {
    structure: Option<fynkel_ecs::ChangeStamp>,
    fields: Option<[fynkel_ecs::ComponentStamp; 8]>,
    lifecycle: u64,
    schemas: usize,
    revision: u64,
}
impl World {
    pub fn identity_changes_since(&self, token: u64) -> Option<fynkel_ecs::ChangeSet> {
        self.gameplay.ecs.changes_since(
            self.gameplay.ecs.cursor_from_token(token),
            &[std::any::TypeId::of::<Name>()],
            false,
        )
    }

    /// Identity/type/ordering table revision. Plain gameplay value changes do
    /// not require encoding or comparing a complete entity table.
    pub fn entity_table_revision(&self) -> u64 {
        let structure = self
            .gameplay
            .ecs
            .change_stamp()
            .expect("settled table storage");
        let fields = [
            self.gameplay.ecs.component_stamp::<Name>().unwrap(),
            self.gameplay
                .ecs
                .component_stamp::<scene::Parent>()
                .unwrap(),
            self.gameplay
                .ecs
                .component_stamp::<scene::Children>()
                .unwrap(),
            self.gameplay.ecs.component_stamp::<scene::Layer>().unwrap(),
            self.gameplay.ecs.component_stamp::<scene::Tags>().unwrap(),
            self.gameplay
                .ecs
                .component_stamp::<scene::InactiveSelf>()
                .unwrap(),
            self.gameplay
                .ecs
                .component_stamp::<scene::Inactive>()
                .unwrap(),
            self.gameplay
                .ecs
                .component_stamp::<crate::registry::DynamicPresence>()
                .unwrap(),
        ];
        let mut cache = self.table_revision.lock().unwrap();
        let lifecycle = self.lifecycle.revision();
        let schemas = self.registered_schemas().count();
        if cache.structure.is_none_or(|s| !s.same_structure(structure))
            || cache.fields.as_ref() != Some(&fields)
            || cache.lifecycle != lifecycle
            || cache.schemas != schemas
        {
            cache.revision = cache
                .revision
                .checked_add(1)
                .expect("entity table revision exhausted");
            cache.structure = Some(structure);
            cache.fields = Some(fields);
            cache.lifecycle = lifecycle;
            cache.schemas = schemas;
        }
        cache.revision
    }
}

/// Owns publication, subscriptions and outbound lifecycle notifications. A new
/// snapshot is built before replacing the prior immutable allocation.
#[derive(Default)]
pub(super) struct Publication {
    pub(super) snapshot: Arc<[SnapshotRecord]>,
    pub(super) tick: Tick,
    pub(super) subscriptions: BTreeSet<Entity>,
    membership_cursor: Option<fynkel_ecs::ChangeCursor>,
    pub(super) events: EventQueue,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) last_events: u32,
}
impl Publication {
    pub(super) fn publish(&mut self, ecs: &Ecs, tick: Tick) {
        // Native ECS destruction also retires publications. Transactions may
        // remove subscriptions eagerly, but are not the only supported writer.
        let changes = ecs
            .changes_since(self.membership_cursor, &[], false)
            .expect("settled publication storage");
        if changes.reset {
            self.subscriptions.retain(|e| ecs.contains(*e));
        } else {
            for e in changes.entities {
                if !ecs.contains(e) {
                    self.subscriptions.remove(&e);
                }
            }
        }
        self.membership_cursor = Some(changes.cursor);
        if self.subscriptions.is_empty() {
            if !self.snapshot.is_empty() {
                self.snapshot = Arc::default();
            }
            self.tick = tick;
            return;
        }
        let mut next = Vec::with_capacity(self.subscriptions.len());
        for &e in &self.subscriptions {
            if let (Ok(local), Ok(wt)) = (ecs.copy::<Transform>(e), ecs.get::<WorldTransform>(e)) {
                next.push(SnapshotRecord {
                    entity: e,
                    local,
                    world: Transform::from_matrix(wt.matrix),
                });
            }
        }
        self.snapshot = next.into();
        self.tick = tick;
    }
}

impl World {
    pub fn scene_metadata_revision(&self) -> u64 {
        self.lifecycle.revision()
    }
    pub fn entity_table_changes_since(&self, token: u64) -> Option<fynkel_ecs::ChangeSet> {
        use std::any::TypeId;
        self.ecs().changes_since(
            self.ecs().cursor_from_token(token),
            &[
                TypeId::of::<Name>(),
                TypeId::of::<scene::Parent>(),
                TypeId::of::<scene::Children>(),
                TypeId::of::<scene::Layer>(),
                TypeId::of::<scene::Tags>(),
                TypeId::of::<scene::InactiveSelf>(),
                TypeId::of::<scene::Inactive>(),
                TypeId::of::<crate::registry::DynamicPresence>(),
            ],
            true,
        )
    }
    pub fn hierarchy_changes_since(&self, token: u64) -> Option<fynkel_ecs::ChangeSet> {
        use std::any::TypeId;
        self.ecs().changes_since(
            self.ecs().cursor_from_token(token),
            &[
                TypeId::of::<scene::Parent>(),
                TypeId::of::<scene::Children>(),
            ],
            false,
        )
    }
}
