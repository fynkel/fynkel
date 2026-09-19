//! Stable numeric ids that cross the ABI. Names are the authoring-facing
//! identity (`fynkel.Transform`); numbers are the wire identity. Built-in
//! numbers are fixed here; the schema generator (ADR-0012) checks that the
//! Rust definitions agree with this table.

/// Wire id of a component type. Built-ins occupy `1..0x1000`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct ComponentId(pub u32);

impl ComponentId {
    pub const NONE: ComponentId = ComponentId(0);
    pub const TRANSFORM: ComponentId = ComponentId(1);
    pub const HIERARCHY: ComponentId = ComponentId(2);
    pub const NAME: ComponentId = ComponentId(3);
    pub const MESH_RENDERER: ComponentId = ComponentId(4);
    pub const CAMERA: ComponentId = ComponentId(5);
    pub const DIRECTIONAL_LIGHT: ComponentId = ComponentId(6);
    pub const POINT_LIGHT: ComponentId = ComponentId(7);
    pub const SPOT_LIGHT: ComponentId = ComponentId(8);
    pub const ENVIRONMENT: ComponentId = ComponentId(9);

    pub const fn name(self) -> Option<&'static str> {
        match self.0 {
            1 => Some("fynkel.Transform"),
            2 => Some("fynkel.Hierarchy"),
            3 => Some("fynkel.Name"),
            4 => Some("fynkel.MeshRenderer"),
            5 => Some("fynkel.Camera"),
            6 => Some("fynkel.DirectionalLight"),
            7 => Some("fynkel.PointLight"),
            8 => Some("fynkel.SpotLight"),
            9 => Some("fynkel.Environment"),
            _ => None,
        }
    }
}

/// Event kinds drained by Dart (pull-only, ADR-0008). Lane is implied by
/// kind: physics events are `0x100..0x200`, everything else is the general lane.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
pub struct EventKind(pub u32);

impl EventKind {
    pub const SPAWNED: EventKind = EventKind(1);
    pub const DESTROYED: EventKind = EventKind(2);
    pub const PARENT_CHANGED: EventKind = EventKind(3);
    pub const MISSED_DEADLINE: EventKind = EventKind(4);
    pub const SCENE_READY: EventKind = EventKind(5);

    pub const PHYSICS_FIRST: u32 = 0x100;
    pub const COLLISION_ENTER: EventKind = EventKind(0x100);
    pub const COLLISION_STAY: EventKind = EventKind(0x101);
    pub const COLLISION_EXIT: EventKind = EventKind(0x102);
    pub const TRIGGER_ENTER: EventKind = EventKind(0x103);
    pub const TRIGGER_EXIT: EventKind = EventKind(0x104);
    pub const PHYSICS_LAST: u32 = 0x1ff;

    pub const fn is_physics(self) -> bool {
        self.0 >= Self::PHYSICS_FIRST && self.0 <= Self::PHYSICS_LAST
    }
}
