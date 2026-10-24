//! `ow` as a library: the parts of the CLI that integration tests drive in-process.
//! `daemon` talks to a running `owd`; `install` is `ow install|list|remove` (see
//! docs/INSTALL_DESIGN.md).
#![forbid(unsafe_code)]

pub mod daemon;
pub mod doctor;
pub mod install;
