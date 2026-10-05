/// Grouping of messages into day sections.
pub mod grouping;
/// Domain types the views render.
pub mod model;
/// Test doubles for the daemon side of `/api/v3`.
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
/// The client of the daemon's `/api/v3`.
pub mod v3;
