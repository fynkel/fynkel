//! The optional engine modules (ADR-0029 §3): physics, audio and
//! animation, as Unity's built-in packages are. A project turns one off in
//! `fynkel.project.json` (`modules`); the world then runs none of its
//! systems and refuses its commands. Every component, command, event and
//! record stays in the one registry whatever the set, so documents, the
//! wire and the ABI never depend on it.

/// One optional module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Module {
    Physics,
    Audio,
    Animation,
}

impl Module {
    pub const ALL: [Module; 3] = [Module::Physics, Module::Audio, Module::Animation];

    /// Its key in `fynkel.project.json`'s `modules`.
    pub const fn key(self) -> &'static str {
        match self {
            Module::Physics => "physics",
            Module::Audio => "audio",
            Module::Animation => "animation",
        }
    }

    /// As the editor's Project Settings name it.
    pub const fn title(self) -> &'static str {
        match self {
            Module::Physics => "Physics",
            Module::Audio => "Audio",
            Module::Animation => "Animation",
        }
    }

    /// The component types that need it.
    pub const fn components(self) -> &'static [&'static str] {
        match self {
            Module::Physics => &[
                "fynkel.RigidBody",
                "fynkel.Collider",
                "fynkel.CharacterController",
            ],
            Module::Audio => &["fynkel.AudioSource", "fynkel.AudioListener"],
            Module::Animation => &["fynkel.Animator", "fynkel.Skin"],
        }
    }

    /// The module a component type needs; `None` for every other type.
    pub fn of_component(type_id: &str) -> Option<Module> {
        Module::ALL
            .into_iter()
            .find(|m| m.components().contains(&type_id))
    }

    pub fn from_key(key: &str) -> Option<Module> {
        Module::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Which modules are on; all by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modules {
    pub physics: bool,
    pub audio: bool,
    pub animation: bool,
}

impl Default for Modules {
    fn default() -> Self {
        Self::ALL
    }
}

impl Modules {
    pub const ALL: Modules = Modules {
        physics: true,
        audio: true,
        animation: true,
    };

    /// The modules this build carries (the cargo features `physics`,
    /// `audio`, `animation`, all on by default; a runtime built with
    /// `modules.strip` leaves the project's off ones out). A world runs
    /// at most these, whatever the project asks.
    pub const COMPILED: Modules = Modules {
        physics: cfg!(feature = "physics"),
        audio: cfg!(feature = "audio"),
        animation: cfg!(feature = "animation"),
    };

    /// On in both.
    pub const fn and(self, other: Modules) -> Modules {
        Modules {
            physics: self.physics && other.physics,
            audio: self.audio && other.audio,
            animation: self.animation && other.animation,
        }
    }

    pub const fn has(&self, m: Module) -> bool {
        match m {
            Module::Physics => self.physics,
            Module::Audio => self.audio,
            Module::Animation => self.animation,
        }
    }

    pub fn set(&mut self, m: Module, on: bool) {
        match m {
            Module::Physics => self.physics = on,
            Module::Audio => self.audio = on,
            Module::Animation => self.animation = on,
        }
    }

    /// The module [`type_id`] needs when it is off; `None` when the type
    /// needs none or its module is on.
    pub fn off_for(&self, type_id: &str) -> Option<Module> {
        Module::of_component(type_id).filter(|m| !self.has(*m))
    }

    /// The modules that are off, in [`Module::ALL`]'s order.
    pub fn off(&self) -> Vec<Module> {
        Module::ALL.into_iter().filter(|m| !self.has(*m)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_map_to_their_module() {
        assert_eq!(
            Module::of_component("fynkel.Collider"),
            Some(Module::Physics)
        );
        assert_eq!(
            Module::of_component("fynkel.AudioSource"),
            Some(Module::Audio)
        );
        assert_eq!(Module::of_component("fynkel.Skin"), Some(Module::Animation));
        assert_eq!(Module::of_component("fynkel.Transform"), None);
        let mut m = Modules::ALL;
        m.set(Module::Animation, false);
        assert_eq!(m.off_for("fynkel.Animator"), Some(Module::Animation));
        assert_eq!(m.off_for("fynkel.Collider"), None);
        assert_eq!(m.off(), vec![Module::Animation]);
    }
}
