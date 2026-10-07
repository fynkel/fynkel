//! The C interface (`fy_*`) that `package:fynkel` calls through `dart:ffi`.
//!
//! This is the only crate where unsafe code is allowed throughout. See
//! `docs/stage-1.md` §3.4.
#![allow(unsafe_code)]
