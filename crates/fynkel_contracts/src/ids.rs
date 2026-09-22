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
    pub const RIGID_BODY: ComponentId = ComponentId(10);
    pub const COLLIDER: ComponentId = ComponentId(11);
    pub const CHARACTER_CONTROLLER: ComponentId = ComponentId(12);
    pub const THIRD_PERSON_CAMERA: ComponentId = ComponentId(13);
    /// The native component catalogue (blueprint §3.5, M3-10).
    pub const ROTATOR: ComponentId = ComponentId(14);
    pub const OSCILLATE: ComponentId = ComponentId(15);
    pub const FOLLOW: ComponentId = ComponentId(16);
    pub const LOOK_AT_CAMERA: ComponentId = ComponentId(17);
    pub const LIFETIME_DESPAWN: ComponentId = ComponentId(18);
    pub const EMITTER: ComponentId = ComponentId(19);
    /// A sensor collider that names an event for Dart (M3-12).
    pub const TRIGGER_ZONE: ComponentId = ComponentId(20);
    /// Animation (M3-19): a clip player and a skinned mesh's joints.
    pub const ANIMATOR: ComponentId = ComponentId(21);
    pub const SKIN: ComponentId = ComponentId(22);
    /// Audio (M3-11): a clip with its playback settings, and the ears.
    pub const AUDIO_SOURCE: ComponentId = ComponentId(23);
    pub const AUDIO_LISTENER: ComponentId = ComponentId(24);
    /// A post-processing volume (R3a): a profile of look overrides, global
    /// or blended in by the camera's distance to a box or a sphere.
    pub const VOLUME: ComponentId = ComponentId(25);

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
            10 => Some("fynkel.RigidBody"),
            11 => Some("fynkel.Collider"),
            12 => Some("fynkel.CharacterController"),
            13 => Some("fynkel.ThirdPersonCamera"),
            14 => Some("fynkel.Rotator"),
            15 => Some("fynkel.Oscillate"),
            16 => Some("fynkel.Follow"),
            17 => Some("fynkel.LookAtCamera"),
            18 => Some("fynkel.LifetimeDespawn"),
            19 => Some("fynkel.Emitter"),
            20 => Some("fynkel.TriggerZone"),
            21 => Some("fynkel.Animator"),
            22 => Some("fynkel.Skin"),
            23 => Some("fynkel.AudioSource"),
            24 => Some("fynkel.AudioListener"),
            25 => Some("fynkel.Volume"),
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
    /// A scene's entities were unloaded (M3-12; after their `DESTROYED`s).
    pub const SCENE_UNLOADED: EventKind = EventKind(6);
    /// A `fynkel.TriggerZone`'s sensor saw a collider enter / leave (M3-12;
    /// general lane, next to the physics lane's `TRIGGER_ENTER`/`EXIT`).
    pub const ZONE_ENTERED: EventKind = EventKind(7);
    pub const ZONE_EXITED: EventKind = EventKind(8);
    /// A `fynkel.Animator`'s non-looping clip reached its end (M3-19).
    pub const ANIMATION_ENDED: EventKind = EventKind(9);
    /// An animator's clock crossed a clip marker (M3-19c).
    pub const ANIMATION_MARKER: EventKind = EventKind(10);
    /// A `fynkel.AudioSource`'s non-looping clip reached its end (M3-11).
    pub const PLAYBACK_ENDED: EventKind = EventKind(11);

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
