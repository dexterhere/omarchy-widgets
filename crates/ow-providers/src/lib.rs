//! Data providers. One module per provider; fixtures for tests.
#![forbid(unsafe_code)]

mod battery;
mod system;

pub use battery::Battery;
pub use system::System;
