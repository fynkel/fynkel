//! A reference to another object in the world (ADR-0035), Unity's object
//! field.

use core::fmt;

use crate::{Entity, Uuid};

/// A reference to another object. At runtime and on the wire it is the
/// object's handle (`u64`, `0` = None). A document stores the object's
/// authoring id instead, `{ "entity": "<uuid>" }` or `null`; a load maps
/// that id to the handle it spawned, once, so a prefab's reference to
/// one of its own objects points at each instance's copy.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ObjectRef {
    /// The referenced object; `NULL` = None, or not resolved yet.
    pub entity: Entity,
    /// The authoring id a document named, `NIL` for a reference made at
    /// runtime. Never on the wire.
    pub id: Uuid,
}

impl ObjectRef {
    pub const NONE: ObjectRef = ObjectRef {
        entity: Entity::NULL,
        id: Uuid::NIL,
    };

    /// A reference to a live object.
    pub const fn to(entity: Entity) -> Self {
        Self {
            entity,
            id: Uuid::NIL,
        }
    }

    /// A document's reference, resolved by the load.
    pub const fn authored(id: Uuid) -> Self {
        Self {
            entity: Entity::NULL,
            id,
        }
    }

    /// Neither a live object nor an authoring id.
    pub fn is_none(&self) -> bool {
        self.entity.is_null() && self.id.is_nil()
    }

    /// The live object, `None` until resolved.
    pub fn get(&self) -> Option<Entity> {
        (!self.entity.is_null()).then_some(self.entity)
    }
}

impl fmt::Debug for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.entity.is_null(), self.id.is_nil()) {
            (true, true) => write!(f, "ObjectRef(None)"),
            (false, true) => write!(f, "ObjectRef({:?})", self.entity),
            (true, false) => write!(f, "ObjectRef({})", self.id),
            (false, false) => write!(f, "ObjectRef({:?} from {})", self.entity, self.id),
        }
    }
}
