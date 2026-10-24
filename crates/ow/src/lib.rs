//! `ow` as a library: the parts of the CLI that integration tests drive in-process.
//! `daemon` talks to a running `owd`; `install` is `ow install|list|remove` (see
//! docs/INSTALL_DESIGN.md).
#![forbid(unsafe_code)]

pub mod connect;
pub mod daemon;
pub mod doctor;
pub mod input;
pub mod install;
pub mod vault_cmd;
