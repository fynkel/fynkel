//! The C functions (`fy_*`) and the plain types they pass (stage 1 plan
//! §3.4). `include/fynkel.h` is generated from this module by cbindgen.
//!
//! Every function that can fail returns an [`FyStatus`](crate::error::FyStatus) and writes its
//! results through out-pointers. Ids are `uint64_t`, with 0 meaning "none".
//! Colours are sRGB, 0–1 per channel. Angles are degrees.
//!
//! Handles: an `FyWorld` lives until `fy_world_free`; an `FyView` until
//! `fy_view_free`. A view keeps its world's memory alive, so they can be
//! freed in either order. Passing a handle after freeing it is undefined
//! behaviour; `package:fynkel` makes that impossible from Dart.

use std::ffi::c_char;

use crate::error::last_error;

// cbindgen reads the modules in the order they are declared here, and the
// header keeps that order: a new module goes where its part of the header
// should be.
mod types;
pub use types::*;

mod convert;
use convert::*;

mod handles;
pub use handles::*;

mod world;
pub use world::*;

mod hierarchy;
pub use hierarchy::*;

mod sky;
pub use sky::*;

mod raycast;
pub use raycast::*;

mod debug;
pub use debug::*;

mod components;
pub use components::*;

mod meshes;
pub use meshes::*;

mod shapes;
pub use shapes::*;

mod shaders;
pub use shaders::*;

mod textures;
pub use textures::*;

mod models;
pub use models::*;

mod animation;
pub use animation::*;

mod particles;
pub use particles::*;

mod trails;
pub use trails::*;

mod texts;
pub use texts::*;

mod sprites;
pub use sprites::*;

mod views;
pub use views::*;

/// Bumped on any change to the functions or types in this file.
pub const FY_ABI_VERSION: u32 = 52;

// --- Library -----------------------------------------------------------------

/// [`FY_ABI_VERSION`]. `package:fynkel` checks it when it loads.
#[unsafe(no_mangle)]
pub extern "C" fn fy_abi_version() -> u32 {
    FY_ABI_VERSION
}

/// The message of the last failed call on this thread, as UTF-8 ("" before
/// any). Valid until the next failed call on this thread.
#[unsafe(no_mangle)]
pub extern "C" fn fy_last_error() -> *const c_char {
    last_error()
}
