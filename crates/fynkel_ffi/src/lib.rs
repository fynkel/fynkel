//! The C interface (`fy_*`) that `package:fynkel` calls through `dart:ffi`,
//! and what sits right behind it: the extract (world → render scene) and
//! the render worker.
//!
//! This is the only crate where unsafe code is allowed throughout. See
//! `docs/stage-1.md` §3.4 and §3.5.
#![allow(unsafe_code)]

mod abi;
mod error;
mod extract;
mod worker;

pub use abi::*;
pub use error::FyStatus;
